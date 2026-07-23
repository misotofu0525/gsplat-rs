use gsplat_core::Vec3f;

use crate::data::CpuPositionView;

use super::PreprocessContext;

/// Authoritative element-by-element CPU visibility and key oracle.
///
/// Keep the explicit FMA sequence in `depth` identical to the renderer/WGSL
/// contract. Architecture leaves must match this output bit-for-bit.
pub(super) fn preprocess_into(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) {
    for (local_index, position) in positions.as_slice().iter().copied().enumerate() {
        let depth = depth(position, context.camera_position, context.depth_row);
        if depth >= context.near_plane && depth <= context.far_plane {
            source_ids.push((source_base + local_index) as u32);
            depth_keys.push(depth.max(0.0).to_bits());
        }
    }
}

#[inline]
pub(super) fn depth(position: Vec3f, camera_position: Vec3f, depth_row: [f32; 3]) -> f32 {
    let relative = [
        position.x - camera_position.x,
        position.y - camera_position.y,
        position.z - camera_position.z,
    ];
    depth_row[2].mul_add(
        relative[2],
        depth_row[1].mul_add(relative[1], depth_row[0] * relative[0]),
    )
}
