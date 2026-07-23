use gsplat_core::{Camera, SceneBuffers, Vec3f};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

use crate::{RendererError, canonical_dot3_f32, quat_inverse, quat_to_mat3};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const PARALLEL_PREPROCESS_THRESHOLD: usize = 256 * 1024;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_PARALLEL_PREPROCESS_CHUNKS: usize = 4;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct PreprocessChunkScratch {
    depth_keys: Vec<u32>,
    indices: Vec<u32>,
}

pub(crate) fn is_visible(depth_z: f32, camera: &Camera) -> bool {
    depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane
}

pub(crate) fn depth_to_key(depth_z: f32) -> u32 {
    // Positive finite depth values preserve a monotonic relationship when using IEEE-754 bits.
    depth_z.max(0.0).to_bits()
}

pub(crate) fn preprocess_positions_visible_into(
    positions: &[Vec3f],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    depth_keys.clear();
    indices.clear();
    if depth_keys.capacity() < positions.len() {
        depth_keys.reserve(positions.len() - depth_keys.capacity());
    }
    if indices.capacity() < positions.len() {
        indices.reserve(positions.len() - indices.capacity());
    }

    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;

    for (idx, position) in positions.iter().enumerate() {
        let depth_z = world_to_camera_depth_with_view_row(*position, camera_position, depth_row);
        if is_visible(depth_z, camera) {
            indices.push(idx as u32);
            depth_keys.push(depth_to_key(depth_z));
        }
    }

    Ok(())
}

/// Native exact visibility preprocessing with deterministic source-order
/// concatenation. Each worker owns reusable output vectors, so large moving
/// scenes gain CPU parallelism without per-frame allocation or changing the
/// stable equal-depth source-ID order consumed by the radix sorter.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn preprocess_positions_visible_into_parallel(
    positions: &[Vec3f],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
    chunks: &mut Vec<PreprocessChunkScratch>,
) -> Result<(), RendererError> {
    if positions.len() < PARALLEL_PREPROCESS_THRESHOLD || rayon::current_num_threads() < 2 {
        return preprocess_positions_visible_into(positions, camera, depth_keys, indices);
    }

    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    let chunk_count = rayon::current_num_threads()
        .min(MAX_PARALLEL_PREPROCESS_CHUNKS)
        .min(positions.len());
    let chunk_len = positions.len().div_ceil(chunk_count);
    if chunks.len() < chunk_count {
        chunks.resize_with(chunk_count, PreprocessChunkScratch::default);
    }

    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;
    let near_plane = camera.intrinsics.near_plane;
    let far_plane = camera.intrinsics.far_plane;

    chunks[..chunk_count]
        .par_iter_mut()
        .enumerate()
        .for_each(|(chunk_index, scratch)| {
            let begin = chunk_index * chunk_len;
            let end = (begin + chunk_len).min(positions.len());
            let source = &positions[begin..end];
            scratch.depth_keys.clear();
            scratch.indices.clear();
            if scratch.depth_keys.capacity() < source.len() {
                scratch
                    .depth_keys
                    .reserve(source.len() - scratch.depth_keys.capacity());
            }
            if scratch.indices.capacity() < source.len() {
                scratch
                    .indices
                    .reserve(source.len() - scratch.indices.capacity());
            }
            for (local_index, position) in source.iter().enumerate() {
                let depth_z =
                    world_to_camera_depth_with_view_row(*position, camera_position, depth_row);
                if depth_z >= near_plane && depth_z <= far_plane {
                    scratch.indices.push((begin + local_index) as u32);
                    scratch.depth_keys.push(depth_to_key(depth_z));
                }
            }
        });

    depth_keys.clear();
    indices.clear();
    if depth_keys.capacity() < positions.len() {
        depth_keys.reserve(positions.len() - depth_keys.capacity());
    }
    if indices.capacity() < positions.len() {
        indices.reserve(positions.len() - indices.capacity());
    }
    for scratch in &chunks[..chunk_count] {
        depth_keys.extend_from_slice(&scratch.depth_keys);
        indices.extend_from_slice(&scratch.indices);
    }
    Ok(())
}

pub(crate) fn preprocess_paged_visible_into(
    scene: &SceneBuffers,
    entries: &[(u32, u32)],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;
    depth_keys.clear();
    indices.clear();
    if depth_keys.capacity() < entries.len() {
        depth_keys.reserve(entries.len() - depth_keys.capacity());
    }
    if indices.capacity() < entries.len() {
        indices.reserve(entries.len() - indices.capacity());
    }

    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;

    for &(global_index, scene_index) in entries {
        let Some(position) = scene.positions.get(scene_index as usize).copied() else {
            continue;
        };
        let depth_z = world_to_camera_depth_with_view_row(position, camera_position, depth_row);
        if is_visible(depth_z, camera) {
            indices.push(global_index);
            depth_keys.push(depth_to_key(depth_z));
        }
    }
    Ok(())
}

pub(crate) fn world_to_camera_depth_with_view_row(
    pos_world: Vec3f,
    camera_position: Vec3f,
    depth_row: [f32; 3],
) -> f32 {
    canonical_dot3_f32(
        depth_row,
        [
            pos_world.x - camera_position.x,
            pos_world.y - camera_position.y,
            pos_world.z - camera_position.z,
        ],
    )
}
