//! Decode SOG images into `SceneBuffers` from a directory or archive.

use std::borrow::Cow;
use std::fs;
use std::path::Path;

use gsplat_core::{SceneBuffers, Vec3f};
use image::RgbaImage;

use crate::SogError;
use crate::meta::{SogChunkMeta, SogShnMeta};

const OPACITY_LOGIT_LIMIT: f32 = 16.0;
const SQRT2: f32 = std::f32::consts::SQRT_2;
const SH_FLIP_RUB_TO_RUF: [f32; 15] = [
    1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0,
];

pub(crate) trait SogAssets {
    fn read(&self, name: &str) -> Result<Cow<'_, [u8]>, SogError>;
}

struct DirAssets<'a> {
    dir: &'a Path,
}

impl SogAssets for DirAssets<'_> {
    fn read(&self, name: &str) -> Result<Cow<'_, [u8]>, SogError> {
        reject_unsafe_name(name)?;
        Ok(Cow::Owned(fs::read(self.dir.join(name))?))
    }
}

pub fn decode_sog_dir(meta_path: &Path) -> Result<SceneBuffers, SogError> {
    let meta = crate::meta::load_chunk_meta(meta_path)?;
    decode_sog_range(meta_path, &meta, 0, meta.count)
}

pub fn decode_sog_range(
    meta_path: &Path,
    meta: &SogChunkMeta,
    offset: usize,
    count: usize,
) -> Result<SceneBuffers, SogError> {
    let dir = meta_path
        .parent()
        .ok_or(SogError::Malformed("SOG meta.json has no parent directory"))?;
    decode_sog_range_from(&DirAssets { dir }, meta, offset, count)
}

pub(crate) fn decode_sog_range_from(
    source: &impl SogAssets,
    meta: &SogChunkMeta,
    offset: usize,
    count: usize,
) -> Result<SceneBuffers, SogError> {
    if offset.saturating_add(count) > meta.count {
        return Err(SogError::Malformed(
            "requested splat range exceeds chunk count",
        ));
    }

    let means_l = load_image(source, &meta.means_files[0])?;
    let means_u = load_image(source, &meta.means_files[1])?;
    let scales = load_image(source, &meta.scales_file)?;
    let quats = load_image(source, &meta.quats_file)?;
    let sh0 = load_image(source, &meta.sh0_file)?;
    let width = means_l.width();
    let height = means_l.height();
    let capacity = (width as usize).saturating_mul(height as usize);
    if meta.count > capacity {
        return Err(SogError::Malformed("meta.count exceeds image capacity"));
    }
    for image in [&means_u, &scales, &quats, &sh0] {
        if image.width() != width || image.height() != height {
            return Err(SogError::Malformed(
                "SOG property images must share dimensions",
            ));
        }
    }

    let sh_dim = meta.shn.as_ref().map(|shn| coeffs_for_bands(shn.bands));
    let (centroids, labels) = if let Some(shn) = &meta.shn {
        let centroids = load_image(source, &shn.centroids_file)?;
        let labels = load_image(source, &shn.labels_file)?;
        let dim = coeffs_for_bands(shn.bands) as u32;
        if centroids.width() != 64 * dim {
            return Err(SogError::Malformed(
                "shN_centroids width must be 64 times the coefficient count",
            ));
        }
        let min_height = u32::try_from(shn.count.div_ceil(64))
            .map_err(|_| SogError::Malformed("shN.count is too large"))?;
        if centroids.height() < min_height {
            return Err(SogError::Malformed("shN_centroids height is too small"));
        }
        if labels.width() != width || labels.height() != height {
            return Err(SogError::Malformed("shN_labels image size mismatch"));
        }
        (Some(centroids), Some(labels))
    } else {
        (None, None)
    };

    let mut scene = SceneBuffers {
        positions: Vec::with_capacity(count),
        opacity: Vec::with_capacity(count),
        scale_xyz: Vec::with_capacity(count),
        rotation_xyzw: Vec::with_capacity(count),
        color_dc: Vec::with_capacity(count),
        sh_degree: meta.shn.as_ref().map(|shn| shn.bands).unwrap_or(0),
        sh_rest: sh_dim.map(|dim| Vec::with_capacity(count.saturating_mul(dim).saturating_mul(3))),
    };

    for index in offset..offset + count {
        let x = (index as u32) % width;
        let y = (index as u32) / width;
        let mean_l = pixel(&means_l, x, y);
        let mean_u = pixel(&means_u, x, y);
        let scale = pixel(&scales, x, y);
        let quat = pixel(&quats, x, y);
        let dc = pixel(&sh0, x, y);

        scene.positions.push(decode_position(meta, mean_l, mean_u));
        scene.opacity.push(alpha_to_logit(f32::from(dc[3]) / 255.0));
        scene.scale_xyz.push([
            meta.scales_codebook[scale[0] as usize],
            meta.scales_codebook[scale[1] as usize],
            meta.scales_codebook[scale[2] as usize],
        ]);
        scene.rotation_xyzw.push(decode_quat(quat)?);
        scene.color_dc.push([
            meta.sh0_codebook[dc[0] as usize],
            meta.sh0_codebook[dc[1] as usize],
            meta.sh0_codebook[dc[2] as usize],
        ]);
        if let (Some(dim), Some(shn), Some(centroids), Some(labels), Some(rest)) = (
            sh_dim,
            meta.shn.as_ref(),
            centroids.as_ref(),
            labels.as_ref(),
            scene.sh_rest.as_mut(),
        ) {
            decode_sh_rest(shn, dim, centroids, pixel(labels, x, y), rest)?;
        }
    }

    scene
        .validate()
        .map_err(|_| SogError::Malformed("decoded SOG scene buffers are inconsistent"))?;
    Ok(scene)
}

pub fn append_scene(dst: &mut SceneBuffers, src: SceneBuffers) -> Result<(), SogError> {
    if dst.is_empty() {
        *dst = src;
        return Ok(());
    }
    if dst.sh_degree != src.sh_degree {
        return Err(SogError::Malformed(
            "cannot concatenate SOG ranges with different SH degrees",
        ));
    }
    dst.positions.extend(src.positions);
    dst.opacity.extend(src.opacity);
    dst.scale_xyz.extend(src.scale_xyz);
    dst.rotation_xyzw.extend(src.rotation_xyzw);
    dst.color_dc.extend(src.color_dc);
    match (dst.sh_rest.as_mut(), src.sh_rest) {
        (Some(left), Some(right)) => left.extend(right),
        (None, None) => {}
        _ => {
            return Err(SogError::Malformed(
                "cannot concatenate SOG ranges with mixed SH rest",
            ));
        }
    }
    dst.validate()
        .map_err(|_| SogError::Malformed("concatenated SOG scene buffers are inconsistent"))?;
    Ok(())
}

pub(crate) fn reject_unsafe_name(name: &str) -> Result<(), SogError> {
    let normalized = name.replace('\\', "/");
    if normalized.is_empty() {
        return Err(SogError::Malformed("SOG file name is empty"));
    }
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err(SogError::Malformed("SOG file name must be relative"));
    }
    if normalized.split('/').any(|component| component == "..") {
        return Err(SogError::Malformed(
            "SOG file name must not contain parent segments",
        ));
    }
    Ok(())
}

fn load_image(source: &impl SogAssets, name: &str) -> Result<RgbaImage, SogError> {
    let bytes = source.read(name)?;
    Ok(image::load_from_memory(bytes.as_ref())?.to_rgba8())
}

fn pixel(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    image.get_pixel(x, y).0
}

fn decode_position(meta: &SogChunkMeta, low: [u8; 4], high: [u8; 4]) -> Vec3f {
    let decode_axis = |lo: u8, hi: u8, min: f32, max: f32| {
        let quantized = (u16::from(hi) << 8) | u16::from(lo);
        let normalized = f32::from(quantized) / 65535.0;
        unlog(lerp(min, max, normalized))
    };
    let x = decode_axis(low[0], high[0], meta.means_mins[0], meta.means_maxs[0]);
    let y = decode_axis(low[1], high[1], meta.means_mins[1], meta.means_maxs[1]);
    let z = decode_axis(low[2], high[2], meta.means_mins[2], meta.means_maxs[2]);
    // RUB -> RUF: z-axis reflection.
    Vec3f::new(x, y, -z)
}

fn decode_quat(pixel: [u8; 4]) -> Result<[f32; 4], SogError> {
    if pixel[3] < 252 {
        return Err(SogError::Malformed("quaternion mode must be 252..=255"));
    }
    let mode = pixel[3] - 252;
    let a = to_comp(pixel[0]);
    let b = to_comp(pixel[1]);
    let c = to_comp(pixel[2]);
    let omitted = (1.0 - (a * a + b * b + c * c)).max(0.0).sqrt();
    let wxyz = match mode {
        0 => [omitted, a, b, c],
        1 => [a, omitted, b, c],
        2 => [a, b, omitted, c],
        3 => [a, b, c, omitted],
        _ => return Err(SogError::Malformed("quaternion mode must be 252..=255")),
    };
    // wxyz -> xyzw, then RUB -> RUF flips x and y.
    Ok([-wxyz[1], -wxyz[2], wxyz[3], wxyz[0]])
}

fn decode_sh_rest(
    shn: &SogShnMeta,
    dim: usize,
    centroids: &RgbaImage,
    label_pixel: [u8; 4],
    rest: &mut Vec<f32>,
) -> Result<(), SogError> {
    let label = usize::from(label_pixel[0]) + (usize::from(label_pixel[1]) << 8);
    if label >= shn.count {
        return Err(SogError::Malformed("shN label is out of range"));
    }
    for channel in 0..3 {
        for (coeff, flip) in SH_FLIP_RUB_TO_RUF.iter().copied().enumerate().take(dim) {
            let u = (label % 64) * dim + coeff;
            let v = label / 64;
            let pixel = centroids.get_pixel(u as u32, v as u32).0;
            let packed = pixel[channel];
            rest.push(shn.codebook[packed as usize] * flip);
        }
    }
    Ok(())
}

fn coeffs_for_bands(bands: u8) -> usize {
    match bands {
        1 => 3,
        2 => 8,
        _ => 15,
    }
}

fn to_comp(value: u8) -> f32 {
    (f32::from(value) / 255.0 - 0.5) * 2.0 / SQRT2
}

fn lerp(min: f32, max: f32, t: f32) -> f32 {
    min + (max - min) * t
}

fn unlog(value: f32) -> f32 {
    value.signum() * (value.abs().exp() - 1.0)
}

fn alpha_to_logit(alpha: f32) -> f32 {
    if alpha <= 0.0 {
        -OPACITY_LOGIT_LIMIT
    } else if alpha >= 1.0 {
        OPACITY_LOGIT_LIMIT
    } else {
        (alpha / (1.0 - alpha)).ln()
    }
}
