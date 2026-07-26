use gsplat_core::{Camera, SceneBuffers, Vec3f};
#[cfg(all(test, not(target_arch = "wasm32")))]
use rayon::prelude::*;

use crate::data::CpuPositionView;
use crate::{RendererError, quat_inverse, quat_to_mat3};

#[cfg(all(test, not(target_arch = "wasm32"), target_arch = "aarch64"))]
mod aarch64;
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod packed;
mod scalar;
#[cfg(all(test, not(target_arch = "wasm32"), target_arch = "x86_64"))]
mod x86_64;

/// Private depth-key precision used by Exact ordering and B1 contract tests.
///
/// Product callers remain on `ExactFull32`. The 24-bit variant retains the
/// high IEEE-754 bits and reserves key zero for non-visible sources so the
/// precision experiment cannot change visible membership.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DepthKeyPrecision {
    #[default]
    ExactFull32,
    CandidateStable24,
}

impl DepthKeyPrecision {
    pub(crate) const fn retained_high_bits(self) -> u32 {
        match self {
            Self::ExactFull32 => 32,
            Self::CandidateStable24 => 24,
        }
    }

    pub(crate) const fn low_bits_to_clear(self) -> u32 {
        32 - self.retained_high_bits()
    }
}

/// Convert one already-visible positive depth into its stable radix key.
///
/// Positive finite IEEE-754 values sort in the same direction as their bits.
/// CandidateStable24 clears the low eight bits. Its lowest positive bin is
/// clamped to one retained-bit unit because zero remains the GPU visibility
/// sentinel; this changes no near/far decision or source membership.
#[inline]
pub(crate) const fn visible_depth_key(depth: f32, precision: DepthKeyPrecision) -> u32 {
    let bits = depth.max(0.0).to_bits();
    let low_bits = precision.low_bits_to_clear();
    if low_bits == 0 {
        bits
    } else {
        let retained = bits >> low_bits;
        let nonzero_retained = if retained == 0 { 1 } else { retained };
        nonzero_retained << low_bits
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const PARALLEL_PREPROCESS_THRESHOLD: usize = 256 * 1024;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_PARALLEL_PREPROCESS_CHUNKS: usize = 4;

#[derive(Clone, Copy)]
pub(super) struct PreprocessContext {
    camera_position: Vec3f,
    depth_row: [f32; 3],
    near_plane: f32,
    far_plane: f32,
}

impl PreprocessContext {
    fn from_camera(camera: &Camera) -> Result<Self, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        let view_rotation = quat_to_mat3(quat_inverse(camera.pose.rotation_xyzw));
        Ok(Self {
            camera_position: camera.pose.position,
            depth_row: view_rotation[2],
            near_plane: camera.intrinsics.near_plane,
            far_plane: camera.intrinsics.far_plane,
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[derive(Default)]
pub(crate) struct PreprocessChunkScratch {
    depth_keys: Vec<u32>,
    source_ids: Vec<u32>,
}

fn reserve_outputs(depth_keys: &mut Vec<u32>, source_ids: &mut Vec<u32>, capacity: usize) {
    depth_keys.clear();
    source_ids.clear();
    if depth_keys.capacity() < capacity {
        depth_keys.reserve(capacity);
    }
    if source_ids.capacity() < capacity {
        source_ids.reserve(capacity);
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn dispatch_leaf(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) {
    #[cfg(target_arch = "wasm32")]
    scalar::preprocess_into_with_precision(
        positions,
        source_base,
        context,
        precision,
        depth_keys,
        source_ids,
    );

    #[cfg(all(not(target_arch = "wasm32"), target_arch = "aarch64"))]
    aarch64::preprocess_into(
        positions,
        source_base,
        context,
        precision,
        depth_keys,
        source_ids,
    );

    #[cfg(all(not(target_arch = "wasm32"), target_arch = "x86_64"))]
    x86_64::preprocess_into(
        positions,
        source_base,
        context,
        precision,
        depth_keys,
        source_ids,
    );

    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_arch = "aarch64"),
        not(target_arch = "x86_64")
    ))]
    scalar::preprocess_into_with_precision(
        positions,
        source_base,
        context,
        precision,
        depth_keys,
        source_ids,
    );
}

pub(crate) fn positions_visible_into_scalar(
    positions: CpuPositionView<'_>,
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) -> Result<(), RendererError> {
    positions_visible_into_scalar_with_precision(
        positions,
        camera,
        DepthKeyPrecision::ExactFull32,
        depth_keys,
        source_ids,
    )
}

pub(crate) fn positions_visible_into_scalar_with_precision(
    positions: CpuPositionView<'_>,
    camera: &Camera,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) -> Result<(), RendererError> {
    let context = PreprocessContext::from_camera(camera)?;
    reserve_outputs(depth_keys, source_ids, positions.len());
    scalar::preprocess_into_with_precision(
        positions, 0, context, precision, depth_keys, source_ids,
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn positions_visible_into(
    positions: CpuPositionView<'_>,
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
    #[cfg(not(target_arch = "wasm32"))] chunks: &mut Vec<PreprocessChunkScratch>,
) -> Result<(), RendererError> {
    positions_visible_into_with_precision(
        positions,
        camera,
        DepthKeyPrecision::ExactFull32,
        depth_keys,
        source_ids,
        #[cfg(not(target_arch = "wasm32"))]
        chunks,
    )
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn positions_visible_into_with_precision(
    positions: CpuPositionView<'_>,
    camera: &Camera,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
    #[cfg(not(target_arch = "wasm32"))] chunks: &mut Vec<PreprocessChunkScratch>,
) -> Result<(), RendererError> {
    let context = PreprocessContext::from_camera(camera)?;

    #[cfg(not(target_arch = "wasm32"))]
    if positions.len() >= PARALLEL_PREPROCESS_THRESHOLD && rayon::current_num_threads() >= 2 {
        let chunk_count = rayon::current_num_threads()
            .min(MAX_PARALLEL_PREPROCESS_CHUNKS)
            .min(positions.len());
        let chunk_len = positions.len().div_ceil(chunk_count);
        if chunks.len() < chunk_count {
            chunks.resize_with(chunk_count, PreprocessChunkScratch::default);
        }
        chunks[..chunk_count]
            .par_iter_mut()
            .enumerate()
            .for_each(|(chunk_index, scratch)| {
                let begin = chunk_index * chunk_len;
                let end = (begin + chunk_len).min(positions.len());
                reserve_outputs(
                    &mut scratch.depth_keys,
                    &mut scratch.source_ids,
                    end - begin,
                );
                dispatch_leaf(
                    positions.slice(begin..end),
                    begin,
                    context,
                    precision,
                    &mut scratch.depth_keys,
                    &mut scratch.source_ids,
                );
            });

        reserve_outputs(depth_keys, source_ids, positions.len());
        for scratch in &chunks[..chunk_count] {
            depth_keys.extend_from_slice(&scratch.depth_keys);
            source_ids.extend_from_slice(&scratch.source_ids);
        }
        return Ok(());
    }

    reserve_outputs(depth_keys, source_ids, positions.len());
    dispatch_leaf(positions, 0, context, precision, depth_keys, source_ids);
    Ok(())
}

pub(crate) fn paged_visible_into(
    scene: &SceneBuffers,
    entries: &[(u32, u32)],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) -> Result<(), RendererError> {
    let context = PreprocessContext::from_camera(camera)?;
    reserve_outputs(depth_keys, source_ids, entries.len());
    for &(global_index, scene_index) in entries {
        let Some(position) = scene.positions.get(scene_index as usize).copied() else {
            continue;
        };
        let depth = scalar::depth(position, context.camera_position, context.depth_row);
        if depth >= context.near_plane && depth <= context.far_plane {
            source_ids.push(global_index);
            depth_keys.push(visible_depth_key(depth, DepthKeyPrecision::ExactFull32));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn depth(position: Vec3f, camera_position: Vec3f, depth_row: [f32; 3]) -> f32 {
    scalar::depth(position, camera_position, depth_row)
}
