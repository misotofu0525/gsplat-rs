//! Exact-count tiled Gaussian rasterization primitives.
//!
//! This module deliberately starts as a CPU reference and wire-format contract.
//! It does not own scene storage: Direct and Resident scenes can both decode one
//! source Gaussian into [`TiledSourceSplat`] without changing their codecs.  A
//! projected Gaussian is duplicated only into the 16x16 tiles containing at
//! least one sample above the pinned INRIA 3-sigma bbox and alpha contract. No
//! point budget, sampling, LOD, or incomplete residency is permitted here.

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, RendererConfig};
use thiserror::Error;

pub const TILE_WIDTH: u32 = 16;
pub const TILE_HEIGHT: u32 = 16;
pub const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;
pub const MAX_ALPHA: f32 = 0.99;
pub const TRANSMITTANCE_EARLY_OUT: f32 = 1.0e-4;

/// One source Gaussian after SH color has been evaluated for the current view.
///
/// `opacity` is the post-sigmoid opacity in `[0, 1]`. `covariance_world`
/// stores the symmetric matrix as `[xx, xy, xz, yy, yz, zz]`. Source IDs are
/// assigned from slice order so they remain the canonical scene IDs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TiledSourceSplat {
    pub position_world: [f32; 3],
    pub covariance_world: [f32; 6],
    pub opacity: f32,
    pub color_rgb: [f32; 3],
}

/// GPU-storage-compatible projected Gaussian.
///
/// Every field group occupies one 16-byte lane, making the Rust array stride
/// match the equivalent WGSL structure without implicit padding. Pixel centers
/// are addressed in top-left coordinates (`x + 0.5`, `y + 0.5`). The inverse
/// conic is `[A, B, C]` for `A dx² + 2 B dx dy + C dy²`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ProjectedSplat {
    pub screen_center_depth_opacity: [f32; 4],
    pub inverse_conic_and_qmax: [f32; 4],
    pub color_rgb_pad: [f32; 4],
    /// `[min_x, min_y, max_x_exclusive, max_y_exclusive]`.
    pub bbox_min_max: [u32; 4],
    /// `[source_id, depth_key, 0, 0]`.
    pub source_depth_pad: [u32; 4],
}

impl ProjectedSplat {
    pub fn screen_center(self) -> [f32; 2] {
        [
            self.screen_center_depth_opacity[0],
            self.screen_center_depth_opacity[1],
        ]
    }

    pub fn depth(self) -> f32 {
        self.screen_center_depth_opacity[2]
    }

    pub fn opacity(self) -> f32 {
        self.screen_center_depth_opacity[3]
    }

    pub fn inverse_conic(self) -> [f32; 3] {
        [
            self.inverse_conic_and_qmax[0],
            self.inverse_conic_and_qmax[1],
            self.inverse_conic_and_qmax[2],
        ]
    }

    pub fn qmax(self) -> f32 {
        self.inverse_conic_and_qmax[3]
    }

    pub fn color_rgb(self) -> [f32; 3] {
        [
            self.color_rgb_pad[0],
            self.color_rgb_pad[1],
            self.color_rgb_pad[2],
        ]
    }

    pub fn bbox(self) -> [u32; 4] {
        self.bbox_min_max
    }

    pub fn source_id(self) -> u32 {
        self.source_depth_pad[0]
    }

    pub fn depth_key(self) -> u32 {
        self.source_depth_pad[1]
    }
}

/// One exact tile contribution. The first three words are the public stable
/// ordering tuple; `projected_index` avoids an auxiliary source-ID lookup while
/// rasterizing. Front-to-back uses ascending depth and descending source ID so
/// reversing it exactly preserves the current back-to-front oracle's ascending
/// source-ID tie order.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct TileContribution {
    pub tile_id: u32,
    pub depth_key: u32,
    pub source_id: u32,
    pub projected_index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedScene {
    pub width: u32,
    pub height: u32,
    pub source_count: u32,
    pub splats: Vec<ProjectedSplat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileBins {
    pub tiles_x: u32,
    pub tiles_y: u32,
    pub entries: Vec<TileContribution>,
    /// `tile_offsets[t]..tile_offsets[t + 1]` is tile `t`'s entry range.
    pub tile_offsets: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TiledRgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<[f32; 4]>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TiledRasterError {
    #[error("tiled raster dimensions must be non-zero")]
    InvalidDimensions,
    #[error("invalid tiled raster camera")]
    InvalidCamera,
    #[error("source count exceeds the u32 source-ID contract")]
    SourceCountOverflow,
    #[error("invalid source Gaussian at source ID {source_id}")]
    InvalidSource { source_id: u32 },
    #[error("tiled raster size arithmetic overflow")]
    SizeOverflow,
    #[error("tiled raster allocation failed")]
    AllocationFailed,
    #[error("tile contribution references an invalid projected Gaussian")]
    InvalidContribution,
}

/// Project every source Gaussian without a point budget.
///
/// Rejection is limited to invalid camera/source data (an error), depth and
/// viewport rejection, or proof that peak opacity is below `1/255`. Large
/// finite covariance is retained. Its finite raster support is the pinned
/// INRIA `ceil(3 * sqrt(max_eigenvalue))` pixel radius, not a point budget.
pub fn project_sources(
    sources: &[TiledSourceSplat],
    camera: &Camera,
    config: RendererConfig,
) -> Result<ProjectedScene, TiledRasterError> {
    if config.width == 0 || config.height == 0 {
        return Err(TiledRasterError::InvalidDimensions);
    }
    camera
        .validate()
        .map_err(|_| TiledRasterError::InvalidCamera)?;
    let source_count =
        u32::try_from(sources.len()).map_err(|_| TiledRasterError::SourceCountOverflow)?;
    let params = ProjectionParams::new(camera, config)?;

    let mut splats = Vec::new();
    splats
        .try_reserve(sources.len())
        .map_err(|_| TiledRasterError::AllocationFailed)?;
    for (source_id, source) in sources.iter().copied().enumerate() {
        let source_id = source_id as u32;
        validate_source(source, source_id)?;
        if let Some(projected) = project_source(source, source_id, camera, params)? {
            splats.push(projected);
        }
    }

    Ok(ProjectedScene {
        width: config.width,
        height: config.height,
        source_count,
        splats,
    })
}

/// Build the exact 16x16 contribution stream and stable front-to-back order.
///
/// A `(tile, splat)` pair is emitted iff at least one pixel center in the tile
/// survives the `alpha >= 1/255` rule. Sorting uses the complete tuple
/// `(tile_id ascending, depth_key ascending, source_id descending)`; stable
/// sort is intentional and tested. This is the exact reverse of the current
/// global back-to-front `(depth descending, source_id ascending)` contract.
pub fn build_tile_bins(scene: &ProjectedScene) -> Result<TileBins, TiledRasterError> {
    if scene.width == 0 || scene.height == 0 {
        return Err(TiledRasterError::InvalidDimensions);
    }
    let tiles_x = scene.width.div_ceil(TILE_WIDTH);
    let tiles_y = scene.height.div_ceil(TILE_HEIGHT);
    let tile_count = tiles_x
        .checked_mul(tiles_y)
        .ok_or(TiledRasterError::SizeOverflow)?;

    let mut entries = Vec::new();
    entries
        .try_reserve(scene.splats.len())
        .map_err(|_| TiledRasterError::AllocationFailed)?;
    for (projected_index, splat) in scene.splats.iter().copied().enumerate() {
        let projected_index =
            u32::try_from(projected_index).map_err(|_| TiledRasterError::SizeOverflow)?;
        let [min_x, min_y, max_x, max_y] = splat.bbox();
        if min_x >= max_x || min_y >= max_y {
            continue;
        }
        let min_tile_x = min_x / TILE_WIDTH;
        let min_tile_y = min_y / TILE_HEIGHT;
        let max_tile_x = (max_x - 1) / TILE_WIDTH;
        let max_tile_y = (max_y - 1) / TILE_HEIGHT;

        for tile_y in min_tile_y..=max_tile_y {
            for tile_x in min_tile_x..=max_tile_x {
                let rect = tile_pixel_rect(tile_x, tile_y, scene.width, scene.height);
                if !rect_has_contribution(splat, rect) {
                    continue;
                }
                entries
                    .try_reserve(1)
                    .map_err(|_| TiledRasterError::AllocationFailed)?;
                entries.push(TileContribution {
                    tile_id: tile_y * tiles_x + tile_x,
                    depth_key: splat.depth_key(),
                    source_id: splat.source_id(),
                    projected_index,
                });
            }
        }
    }

    entries.sort_by_key(|entry| (entry.tile_id, entry.depth_key, !entry.source_id));

    let offsets_len = usize::try_from(tile_count)
        .ok()
        .and_then(|count| count.checked_add(1))
        .ok_or(TiledRasterError::SizeOverflow)?;
    let mut tile_offsets = Vec::<u32>::new();
    tile_offsets
        .try_reserve_exact(offsets_len)
        .map_err(|_| TiledRasterError::AllocationFailed)?;
    tile_offsets.resize(offsets_len, 0);
    for entry in &entries {
        let offset_index =
            usize::try_from(entry.tile_id + 1).map_err(|_| TiledRasterError::SizeOverflow)?;
        tile_offsets[offset_index] = tile_offsets[offset_index]
            .checked_add(1)
            .ok_or(TiledRasterError::SizeOverflow)?;
    }
    for index in 1..tile_offsets.len() {
        tile_offsets[index] = tile_offsets[index]
            .checked_add(tile_offsets[index - 1])
            .ok_or(TiledRasterError::SizeOverflow)?;
    }

    Ok(TileBins {
        tiles_x,
        tiles_y,
        entries,
        tile_offsets,
    })
}

/// CPU reference for the target per-tile front-to-back compute rasterizer.
pub fn rasterize_tiles_front_to_back(
    scene: &ProjectedScene,
    bins: &TileBins,
) -> Result<TiledRgbaImage, TiledRasterError> {
    let expected_tiles_x = scene.width.div_ceil(TILE_WIDTH);
    let expected_tiles_y = scene.height.div_ceil(TILE_HEIGHT);
    if bins.tiles_x != expected_tiles_x || bins.tiles_y != expected_tiles_y {
        return Err(TiledRasterError::InvalidContribution);
    }
    let tile_count = bins
        .tiles_x
        .checked_mul(bins.tiles_y)
        .ok_or(TiledRasterError::SizeOverflow)?;
    if bins.tile_offsets.len()
        != usize::try_from(tile_count)
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or(TiledRasterError::SizeOverflow)?
    {
        return Err(TiledRasterError::InvalidContribution);
    }

    let pixel_count = pixel_count(scene.width, scene.height)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(pixel_count)
        .map_err(|_| TiledRasterError::AllocationFailed)?;
    rgba.resize(pixel_count, [0.0; 4]);

    for tile_id in 0..tile_count {
        let tile_x = tile_id % bins.tiles_x;
        let tile_y = tile_id / bins.tiles_x;
        let [min_x, min_y, max_x, max_y] =
            tile_pixel_rect(tile_x, tile_y, scene.width, scene.height);
        let begin = bins.tile_offsets[tile_id as usize] as usize;
        let end = bins.tile_offsets[tile_id as usize + 1] as usize;
        let entries = bins
            .entries
            .get(begin..end)
            .ok_or(TiledRasterError::InvalidContribution)?;

        for y in min_y..max_y {
            for x in min_x..max_x {
                let mut transmittance = 1.0_f32;
                let mut rgb = [0.0_f32; 3];
                for entry in entries {
                    if entry.tile_id != tile_id {
                        return Err(TiledRasterError::InvalidContribution);
                    }
                    let splat = scene
                        .splats
                        .get(entry.projected_index as usize)
                        .copied()
                        .ok_or(TiledRasterError::InvalidContribution)?;
                    if splat.source_id() != entry.source_id || splat.depth_key() != entry.depth_key
                    {
                        return Err(TiledRasterError::InvalidContribution);
                    }
                    let alpha = sample_alpha(splat, x, y);
                    if alpha < ALPHA_THRESHOLD {
                        continue;
                    }
                    let color = splat.color_rgb();
                    let weight = transmittance * alpha;
                    rgb[0] += weight * color[0];
                    rgb[1] += weight * color[1];
                    rgb[2] += weight * color[2];
                    transmittance *= 1.0 - alpha;
                    if transmittance < TRANSMITTANCE_EARLY_OUT {
                        break;
                    }
                }
                let pixel_index = usize::try_from(y)
                    .ok()
                    .and_then(|row| row.checked_mul(scene.width as usize))
                    .and_then(|row| row.checked_add(x as usize))
                    .ok_or(TiledRasterError::SizeOverflow)?;
                rgba[pixel_index] = [rgb[0], rgb[1], rgb[2], 1.0 - transmittance];
            }
        }
    }

    Ok(TiledRgbaImage {
        width: scene.width,
        height: scene.height,
        rgba,
    })
}

/// Deliberately naive all-splat back-to-front reference.
///
/// Its `(depth descending, source_id ascending)` ordering is the existing
/// SortedAlpha stable-order contract. It is algebraically equivalent to the
/// reversed front-to-back transmittance accumulation aside from the documented
/// `T < 1e-4` early-out.
pub fn rasterize_naive_back_to_front(
    scene: &ProjectedScene,
) -> Result<TiledRgbaImage, TiledRasterError> {
    let pixel_count = pixel_count(scene.width, scene.height)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(pixel_count)
        .map_err(|_| TiledRasterError::AllocationFailed)?;
    rgba.resize(pixel_count, [0.0; 4]);

    let mut order = Vec::new();
    order
        .try_reserve(scene.splats.len())
        .map_err(|_| TiledRasterError::AllocationFailed)?;
    order.extend(0..scene.splats.len());
    order.sort_by_key(|&index| {
        let splat = scene.splats[index];
        (!splat.depth_key(), splat.source_id())
    });

    for y in 0..scene.height {
        for x in 0..scene.width {
            let mut out = [0.0_f32; 4];
            for &index in &order {
                let splat = scene.splats[index];
                let [min_x, min_y, max_x, max_y] = splat.bbox();
                if x < min_x || x >= max_x || y < min_y || y >= max_y {
                    continue;
                }
                let alpha = sample_alpha(splat, x, y);
                if alpha < ALPHA_THRESHOLD {
                    continue;
                }
                let one_minus_alpha = 1.0 - alpha;
                let color = splat.color_rgb();
                out[0] = color[0] * alpha + out[0] * one_minus_alpha;
                out[1] = color[1] * alpha + out[1] * one_minus_alpha;
                out[2] = color[2] * alpha + out[2] * one_minus_alpha;
                out[3] = alpha + out[3] * one_minus_alpha;
            }
            rgba[y as usize * scene.width as usize + x as usize] = out;
        }
    }

    Ok(TiledRgbaImage {
        width: scene.width,
        height: scene.height,
        rgba,
    })
}

/// Evaluate the exact per-pixel alpha contract at one integer pixel.
pub fn sample_alpha(splat: ProjectedSplat, x: u32, y: u32) -> f32 {
    let [min_x, min_y, max_x, max_y] = splat.bbox();
    if x < min_x || x >= max_x || y < min_y || y >= max_y {
        return 0.0;
    }
    let center = splat.screen_center();
    let conic = splat.inverse_conic();
    let dx = x as f32 + 0.5 - center[0];
    let dy = y as f32 + 0.5 - center[1];
    let q = conic[0] * dx * dx + 2.0 * conic[1] * dx * dy + conic[2] * dy * dy;
    let power = -0.5 * q;
    (splat.opacity() * power.exp()).min(MAX_ALPHA)
}

fn validate_source(source: TiledSourceSplat, source_id: u32) -> Result<(), TiledRasterError> {
    let finite = source.position_world.iter().all(|value| value.is_finite())
        && source
            .covariance_world
            .iter()
            .all(|value| value.is_finite())
        && source.opacity.is_finite()
        && source.color_rgb.iter().all(|value| value.is_finite());
    if !finite || !(0.0..=1.0).contains(&source.opacity) {
        return Err(TiledRasterError::InvalidSource { source_id });
    }
    Ok(())
}

fn project_source(
    source: TiledSourceSplat,
    source_id: u32,
    camera: &Camera,
    params: ProjectionParams,
) -> Result<Option<ProjectedSplat>, TiledRasterError> {
    if source.opacity < ALPHA_THRESHOLD {
        return Ok(None);
    }

    let relative = [
        source.position_world[0] - camera.pose.position.x,
        source.position_world[1] - camera.pose.position.y,
        source.position_world[2] - camera.pose.position.z,
    ];
    let p_cam = [
        canonical_dot3(params.view_rot[0], relative),
        canonical_dot3(params.view_rot[1], relative),
        canonical_dot3(params.view_rot[2], relative),
    ];
    let depth = p_cam[2];
    if depth < camera.intrinsics.near_plane || depth > camera.intrinsics.far_plane {
        return Ok(None);
    }

    let inv_z = 1.0 / depth;
    let x_ndc = p_cam[0] * params.fx * inv_z;
    let y_ndc = p_cam[1] * params.fy * inv_z;
    let center = [
        (x_ndc * 0.5 + 0.5) * params.width_f,
        (0.5 - y_ndc * 0.5) * params.height_f,
    ];

    let cov_cam = transform_covariance(source.covariance_world, params.view_rot);
    let x_clamped = (p_cam[0] * inv_z).clamp(-params.lim_x, params.lim_x) * depth;
    let y_clamped = (p_cam[1] * inv_z).clamp(-params.lim_y, params.lim_y) * depth;
    let inv_z2 = inv_z * inv_z;
    let j00 = params.fx * inv_z;
    let j02 = -params.fx * x_clamped * inv_z2;
    let j11 = params.fy * inv_z;
    let j12 = -params.fy * y_clamped * inv_z2;

    let cov_ndc_xy = j00 * j11 * cov_cam[1]
        + j00 * j12 * cov_cam[2]
        + j02 * j11 * cov_cam[4]
        + j02 * j12 * cov_cam[5];
    let cov_ndc_xx = j00 * j00 * cov_cam[0] + 2.0 * j00 * j02 * cov_cam[2] + j02 * j02 * cov_cam[5];
    let cov_ndc_yy = j11 * j11 * cov_cam[3] + 2.0 * j11 * j12 * cov_cam[4] + j12 * j12 * cov_cam[5];

    let half_width = params.width_f * 0.5;
    let half_height = params.height_f * 0.5;
    let cov_xx = cov_ndc_xx * half_width * half_width + 0.3;
    // Top-left screen Y reverses the NDC Y axis.
    let cov_xy = -cov_ndc_xy * half_width * half_height;
    let cov_yy = cov_ndc_yy * half_height * half_height + 0.3;
    let det = cov_xx * cov_yy - cov_xy * cov_xy;
    if !center.iter().all(|value| value.is_finite())
        || !cov_xx.is_finite()
        || !cov_xy.is_finite()
        || !cov_yy.is_finite()
        || !det.is_finite()
        || det <= 0.0
    {
        return Err(TiledRasterError::InvalidSource { source_id });
    }

    let inverse_conic = [cov_yy / det, -cov_xy / det, cov_xx / det];
    let covariance_mid = 0.5 * (cov_xx + cov_yy);
    let covariance_half_difference = 0.5 * (cov_xx - cov_yy);
    let eigen_term =
        (covariance_half_difference * covariance_half_difference + cov_xy * cov_xy).sqrt();
    let max_eigenvalue = (covariance_mid + eigen_term).max(0.0);
    let radius_pixels = (3.0 * max_eigenvalue.sqrt()).ceil();
    let qmax = 9.0;
    if !inverse_conic.iter().all(|value| value.is_finite()) || !radius_pixels.is_finite() {
        return Err(TiledRasterError::InvalidSource { source_id });
    }

    let bbox = conservative_pixel_bbox(
        center,
        [radius_pixels, radius_pixels],
        params.width,
        params.height,
    );
    if bbox[0] >= bbox[2] || bbox[1] >= bbox[3] {
        return Ok(None);
    }

    let color = source.color_rgb.map(|channel| channel.max(0.0));
    Ok(Some(ProjectedSplat {
        screen_center_depth_opacity: [center[0], center[1], depth, source.opacity],
        inverse_conic_and_qmax: [inverse_conic[0], inverse_conic[1], inverse_conic[2], qmax],
        color_rgb_pad: [color[0], color[1], color[2], 0.0],
        bbox_min_max: bbox,
        source_depth_pad: [source_id, depth.to_bits(), 0, 0],
    }))
}

#[derive(Clone, Copy)]
struct ProjectionParams {
    width: u32,
    height: u32,
    width_f: f32,
    height_f: f32,
    fx: f32,
    fy: f32,
    lim_x: f32,
    lim_y: f32,
    view_rot: [[f32; 3]; 3],
}

impl ProjectionParams {
    fn new(camera: &Camera, config: RendererConfig) -> Result<Self, TiledRasterError> {
        let tan_half_fovy = (camera.intrinsics.vertical_fov_radians * 0.5).tan();
        if !tan_half_fovy.is_finite() || tan_half_fovy <= 0.0 {
            return Err(TiledRasterError::InvalidCamera);
        }
        let aspect = config.width as f32 / config.height as f32;
        let fy = 1.0 / tan_half_fovy;
        let fx = fy / aspect;
        let camera_inverse = quat_inverse(camera.pose.rotation_xyzw);
        Ok(Self {
            width: config.width,
            height: config.height,
            width_f: config.width as f32,
            height_f: config.height as f32,
            fx,
            fy,
            lim_x: 1.3 * tan_half_fovy * aspect,
            lim_y: 1.3 * tan_half_fovy,
            view_rot: quat_to_mat3(camera_inverse),
        })
    }
}

fn conservative_pixel_bbox(
    center: [f32; 2],
    extent: [f32; 2],
    width: u32,
    height: u32,
) -> [u32; 4] {
    let min_x = (center[0] - extent[0]).floor().clamp(0.0, width as f32) as u32;
    let min_y = (center[1] - extent[1]).floor().clamp(0.0, height as f32) as u32;
    let max_x = (center[0] + extent[0]).ceil().clamp(0.0, width as f32) as u32;
    let max_y = (center[1] + extent[1]).ceil().clamp(0.0, height as f32) as u32;
    [min_x, min_y, max_x, max_y]
}

fn tile_pixel_rect(tile_x: u32, tile_y: u32, width: u32, height: u32) -> [u32; 4] {
    let min_x = tile_x * TILE_WIDTH;
    let min_y = tile_y * TILE_HEIGHT;
    [
        min_x,
        min_y,
        min_x.saturating_add(TILE_WIDTH).min(width),
        min_y.saturating_add(TILE_HEIGHT).min(height),
    ]
}

/// Exact maximum-alpha test over a rectangular integer pixel grid.
///
/// For each coordinate along the shorter dimension, the quadratic is convex
/// in the other dimension, so its discrete minimum is the pixel center nearest
/// the clamped analytic minimizer. This is exact while requiring at most 16
/// samples for a tile rather than 256.
fn rect_has_contribution(splat: ProjectedSplat, rect: [u32; 4]) -> bool {
    let [min_x, min_y, max_x, max_y] = rect;
    if min_x >= max_x || min_y >= max_y {
        return false;
    }
    let center = splat.screen_center();
    let [a, b, c] = splat.inverse_conic();
    let width = max_x - min_x;
    let height = max_y - min_y;

    if width <= height {
        for x in min_x..max_x {
            let dx = x as f32 + 0.5 - center[0];
            let optimal_y_center = center[1] - b * dx / c;
            let y = nearest_pixel_index(optimal_y_center, min_y, max_y);
            if sample_alpha(splat, x, y) >= ALPHA_THRESHOLD {
                return true;
            }
        }
    } else {
        for y in min_y..max_y {
            let dy = y as f32 + 0.5 - center[1];
            let optimal_x_center = center[0] - b * dy / a;
            let x = nearest_pixel_index(optimal_x_center, min_x, max_x);
            if sample_alpha(splat, x, y) >= ALPHA_THRESHOLD {
                return true;
            }
        }
    }
    false
}

fn nearest_pixel_index(center_coordinate: f32, min: u32, max_exclusive: u32) -> u32 {
    debug_assert!(min < max_exclusive);
    (center_coordinate - 0.5)
        .round()
        .clamp(min as f32, (max_exclusive - 1) as f32) as u32
}

fn pixel_count(width: u32, height: u32) -> Result<usize, TiledRasterError> {
    usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(TiledRasterError::SizeOverflow)?,
    )
    .map_err(|_| TiledRasterError::SizeOverflow)
}

fn canonical_dot3(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[2].mul_add(right[2], left[1].mul_add(right[1], left[0] * right[0]))
}

fn transform_covariance(cov: [f32; 6], rotation: [[f32; 3]; 3]) -> [f32; 6] {
    [
        covariance_quadratic(cov, rotation[0]),
        covariance_bilinear(cov, rotation[0], rotation[1]),
        covariance_bilinear(cov, rotation[0], rotation[2]),
        covariance_quadratic(cov, rotation[1]),
        covariance_bilinear(cov, rotation[1], rotation[2]),
        covariance_quadratic(cov, rotation[2]),
    ]
}

fn covariance_quadratic(cov: [f32; 6], row: [f32; 3]) -> f32 {
    row[0] * row[0] * cov[0]
        + 2.0 * row[0] * row[1] * cov[1]
        + 2.0 * row[0] * row[2] * cov[2]
        + row[1] * row[1] * cov[3]
        + 2.0 * row[1] * row[2] * cov[4]
        + row[2] * row[2] * cov[5]
}

fn covariance_bilinear(cov: [f32; 6], left: [f32; 3], right: [f32; 3]) -> f32 {
    let cr = [
        cov[0] * right[0] + cov[1] * right[1] + cov[2] * right[2],
        cov[1] * right[0] + cov[3] * right[1] + cov[4] * right[2],
        cov[2] * right[0] + cov[4] * right[1] + cov[5] * right[2],
    ];
    left[0] * cr[0] + left[1] * cr[1] + left[2] * cr[2]
}

fn quat_inverse(q: [f32; 4]) -> [f32; 4] {
    let normalized = quat_normalize(q);
    [
        -normalized[0],
        -normalized[1],
        -normalized[2],
        normalized[3],
    ]
}

fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let norm_squared = q.iter().map(|value| value * value).sum::<f32>();
    let inverse_norm = norm_squared.sqrt().recip();
    [
        q[0] * inverse_norm,
        q[1] * inverse_norm,
        q[2] * inverse_norm,
        q[3] * inverse_norm,
    ]
}

fn quat_to_mat3(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = quat_normalize(q);
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
        [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
        [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
    ]
}
