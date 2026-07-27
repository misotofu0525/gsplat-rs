use gsplat_core::Vec3f;
#[cfg(any(test, target_arch = "wasm32"))]
use gsplat_sort::CpuSortBackend;

use crate::data::CpuPositionView;

use super::{DepthKeyPrecision, PreprocessContext, visible_depth_key};

/// Authoritative element-by-element CPU visibility and key oracle.
///
/// Keep the explicit FMA sequence in `depth` identical to the renderer/WGSL
/// contract. Architecture leaves must match this output bit-for-bit.
#[cfg(test)]
pub(super) fn preprocess_into(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) {
    preprocess_into_with_precision(
        positions,
        source_base,
        context,
        DepthKeyPrecision::ExactFull32,
        depth_keys,
        source_ids,
    );
}

pub(super) fn preprocess_into_with_precision(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    source_ids: &mut Vec<u32>,
) {
    for (local_index, position) in positions.as_slice().iter().copied().enumerate() {
        let depth = depth(position, context.camera_position, context.depth_row);
        if depth >= context.near_plane && depth <= context.far_plane {
            source_ids.push((source_base + local_index) as u32);
            depth_keys.push(visible_depth_key(depth, precision));
        }
    }
}

/// Scalar visibility/key preprocessing directly into the radix pair ABI.
///
/// WebAssembly uses this entry to avoid materializing split key/source arrays
/// and then reading both arrays again solely to pack the radix input. Source
/// enumeration remains ascending, so the key-only stable radix keeps equal
/// depths in ascending source-ID order.
#[cfg(any(test, target_arch = "wasm32"))]
pub(super) fn preprocess_packed_into_with_precision(
    positions: CpuPositionView<'_>,
    source_base: usize,
    context: PreprocessContext,
    precision: DepthKeyPrecision,
    packed_pairs: &mut Vec<u64>,
) {
    for (local_index, position) in positions.as_slice().iter().copied().enumerate() {
        let depth = depth(position, context.camera_position, context.depth_row);
        if depth >= context.near_plane && depth <= context.far_plane {
            packed_pairs.push(CpuSortBackend::pack_key_value(
                visible_depth_key(depth, precision),
                (source_base + local_index) as u32,
            ));
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

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, Vec3f};

    use super::{preprocess_into_with_precision, preprocess_packed_into_with_precision};
    use crate::cpu::preprocess::{DepthKeyPrecision, PreprocessContext};
    use crate::data::CpuPositionView;

    #[test]
    fn packed_output_matches_split_exact_oracle_and_stable_source_order() {
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = 1.0;
        camera.intrinsics.far_plane = 3.0;
        let context = PreprocessContext::from_camera(&camera).expect("camera");
        let positions = [
            Vec3f::new(0.0, 0.0, 2.0),
            Vec3f::new(1.0, 0.0, 2.0),
            Vec3f::new(0.0, 0.0, 1.0),
            Vec3f::new(0.0, 0.0, 3.0),
            Vec3f::new(0.0, 0.0, f32::from_bits(1.0_f32.to_bits() - 1)),
            Vec3f::new(0.0, 0.0, f32::from_bits(3.0_f32.to_bits() + 1)),
            Vec3f::new(0.0, 0.0, f32::NAN),
        ];
        let view = CpuPositionView::new(&positions);
        let source_base = 37;
        let mut expected_keys = Vec::new();
        let mut expected_ids = Vec::new();
        preprocess_into_with_precision(
            view,
            source_base,
            context,
            DepthKeyPrecision::ExactFull32,
            &mut expected_keys,
            &mut expected_ids,
        );

        let mut packed = Vec::new();
        preprocess_packed_into_with_precision(
            view,
            source_base,
            context,
            DepthKeyPrecision::ExactFull32,
            &mut packed,
        );
        let actual = packed
            .into_iter()
            .map(|pair| ((pair >> 32) as u32, !(pair as u32)))
            .collect::<Vec<_>>();
        let expected = expected_keys
            .into_iter()
            .zip(expected_ids)
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert_eq!(
            actual[0].0, actual[1].0,
            "fixture must exercise a depth tie"
        );
        assert!(
            actual[0].1 < actual[1].1,
            "source enumeration must remain stable"
        );
    }
}
