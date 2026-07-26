use gsplat_core::{Camera, SceneBuffers, Vec3f};

use crate::RendererError;
use crate::cpu::preprocess;
#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::cpu::preprocess::PreprocessChunkScratch;
use crate::cpu::workspace::{CpuOrderWorkspace, WorkspaceAllocationError};
use crate::data::CpuPositionView;

pub(crate) use crate::cpu::preprocess::DepthKeyPrecision;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use crate::cpu::preprocess::PARALLEL_PREPROCESS_THRESHOLD;

#[derive(Debug)]
pub(crate) struct CpuOrderAllocationError {
    resource: &'static str,
}

impl CpuOrderAllocationError {
    pub(crate) const fn resource(&self) -> &'static str {
        self.resource
    }
}

impl From<WorkspaceAllocationError> for CpuOrderAllocationError {
    fn from(error: WorkspaceAllocationError) -> Self {
        Self {
            resource: error.resource,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CpuOrderTimings {
    pub(crate) preprocess_ms: f32,
    pub(crate) sort_ms: f32,
}

/// One lane-local exact CPU ordering engine.
///
/// The engine owns preprocess buffers and the reusable `gsplat-sort` radix
/// backend. The caller owns the authoritative ID buffer; successful execution
/// swaps a completed candidate into that buffer as the final step. This keeps
/// sync Surface, native async, offscreen/legacy and shadow CPU PostSort lanes
/// independent without a global mutex.
#[derive(Default)]
pub(crate) struct CpuOrderEngine {
    workspace: CpuOrderWorkspace,
}

impl CpuOrderEngine {
    pub(crate) fn try_with_capacity(source_count: usize) -> Result<Self, CpuOrderAllocationError> {
        Ok(Self {
            workspace: CpuOrderWorkspace::try_with_capacity(source_count)?,
        })
    }

    pub(crate) fn order_positions(
        &mut self,
        positions: CpuPositionView<'_>,
        camera: &Camera,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
    ) -> Result<CpuOrderTimings, RendererError> {
        let timings =
            self.workspace
                .order_positions(positions, camera, stable_full32, authoritative_ids)?;
        Ok(CpuOrderTimings {
            preprocess_ms: timings.preprocess_ms,
            sort_ms: timings.sort_ms,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn order_paged(
        &mut self,
        scene: &SceneBuffers,
        entries: &[(u32, u32)],
        camera: &Camera,
        stable_full32: bool,
        authoritative_ids: &mut Vec<u32>,
    ) -> Result<CpuOrderTimings, RendererError> {
        let timings =
            self.workspace
                .order_paged(scene, entries, camera, stable_full32, authoritative_ids)?;
        Ok(CpuOrderTimings {
            preprocess_ms: timings.preprocess_ms,
            sort_ms: timings.sort_ms,
        })
    }

    #[cfg(test)]
    pub(crate) fn buffer_state(&self) -> ((usize, usize), (usize, usize), (usize, usize)) {
        self.workspace.buffer_state()
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn calibration_state(
        &self,
    ) -> (Option<crate::cpu::calibration::CalibrationDecision>, usize) {
        self.workspace.calibration_state()
    }
}

pub(crate) fn is_visible(depth_z: f32, camera: &Camera) -> bool {
    depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane
}

#[cfg(test)]
pub(crate) fn depth_to_key(depth_z: f32) -> u32 {
    preprocess::visible_depth_key(depth_z, DepthKeyPrecision::ExactFull32)
}

#[cfg(test)]
pub(crate) fn depth_to_key_with_precision(depth_z: f32, precision: DepthKeyPrecision) -> u32 {
    preprocess::visible_depth_key(depth_z, precision)
}

#[cfg(test)]
pub(crate) fn preprocess_positions_visible_into_with_precision(
    positions: &[Vec3f],
    camera: &Camera,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    preprocess::positions_visible_into_scalar_with_precision(
        CpuPositionView::new(positions),
        camera,
        precision,
        depth_keys,
        indices,
    )
}

/// Scalar authority retained for focused parity tests and narrow callers that
/// need visibility preprocessing without an ordering workspace.
pub(crate) fn preprocess_positions_visible_into(
    positions: &[Vec3f],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    preprocess::positions_visible_into_scalar(
        CpuPositionView::new(positions),
        camera,
        depth_keys,
        indices,
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn preprocess_positions_visible_into_parallel(
    positions: &[Vec3f],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
    chunks: &mut Vec<PreprocessChunkScratch>,
) -> Result<(), RendererError> {
    preprocess::positions_visible_into(
        CpuPositionView::new(positions),
        camera,
        depth_keys,
        indices,
        chunks,
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn preprocess_positions_visible_into_parallel_with_precision(
    positions: &[Vec3f],
    camera: &Camera,
    precision: DepthKeyPrecision,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
    chunks: &mut Vec<PreprocessChunkScratch>,
) -> Result<(), RendererError> {
    preprocess::positions_visible_into_with_precision(
        CpuPositionView::new(positions),
        camera,
        precision,
        depth_keys,
        indices,
        chunks,
    )
}

pub(crate) fn preprocess_paged_visible_into(
    scene: &SceneBuffers,
    entries: &[(u32, u32)],
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    preprocess::paged_visible_into(scene, entries, camera, depth_keys, indices)
}

#[cfg(test)]
pub(crate) fn world_to_camera_depth_with_view_row(
    pos_world: Vec3f,
    camera_position: Vec3f,
    depth_row: [f32; 3],
) -> f32 {
    preprocess::depth(pos_world, camera_position, depth_row)
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, Vec3f};
    #[cfg(not(target_arch = "wasm32"))]
    use std::time::Instant;

    #[cfg(not(target_arch = "wasm32"))]
    use super::preprocess_positions_visible_into_parallel;
    #[cfg(not(target_arch = "wasm32"))]
    use super::preprocess_positions_visible_into_parallel_with_precision;
    use super::{
        CpuOrderEngine, DepthKeyPrecision, depth_to_key, depth_to_key_with_precision,
        preprocess_positions_visible_into, preprocess_positions_visible_into_with_precision,
        world_to_camera_depth_with_view_row,
    };
    use crate::data::CpuPositionView;

    fn camera(near_plane: f32, far_plane: f32) -> Camera {
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = near_plane;
        camera.intrinsics.far_plane = far_plane;
        camera
    }

    #[test]
    fn scalar_oracle_covers_adversarial_values_and_257_tail() {
        let near = 1.0_f32;
        let far = 3.0_f32;
        let below_near = f32::from_bits(near.to_bits() - 1);
        let above_near = f32::from_bits(near.to_bits() + 1);
        let below_far = f32::from_bits(far.to_bits() - 1);
        let above_far = f32::from_bits(far.to_bits() + 1);
        let mut positions = vec![Vec3f::new(0.0, 0.0, 2.0); 257];
        positions[..10].copy_from_slice(&[
            Vec3f::new(0.0, 0.0, near),
            Vec3f::new(0.0, 0.0, far),
            Vec3f::new(0.0, 0.0, below_near),
            Vec3f::new(0.0, 0.0, above_near),
            Vec3f::new(0.0, 0.0, below_far),
            Vec3f::new(0.0, 0.0, above_far),
            Vec3f::new(0.0, 0.0, f32::NAN),
            Vec3f::new(0.0, 0.0, f32::INFINITY),
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::new(0.0, 0.0, -0.0),
        ]);

        let mut scalar_keys = Vec::new();
        let mut scalar_ids = Vec::new();
        preprocess_positions_visible_into(
            &positions,
            &camera(near, far),
            &mut scalar_keys,
            &mut scalar_ids,
        )
        .expect("scalar oracle");

        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::with_capacity(positions.len());
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(near, far),
                true,
                &mut order,
            )
            .expect("dispatched order");

        let mut expected = scalar_ids.clone();
        gsplat_sort::CpuSortBackend::default()
            .sort_values_by_keys(&scalar_keys, &mut expected)
            .expect("reference radix");
        assert_eq!(order, expected);
        assert!(order.contains(&0));
        assert!(order.contains(&1));
        assert!(!order.iter().any(|id| matches!(*id, 2 | 5 | 6 | 7 | 8 | 9)));
    }

    #[test]
    fn explicit_fma_depth_and_key_bits_are_authoritative() {
        let position = Vec3f::new(
            f32::from_bits(0x4b00_0001),
            f32::from_bits(0xcb00_0000),
            f32::from_bits(0x3f80_0001),
        );
        let camera_position = Vec3f::new(1.0, -1.0, f32::from_bits(1));
        let row = [
            f32::from_bits(0x3f00_0001),
            0.5,
            f32::from_bits(0x3f7f_ffff),
        ];
        let relative = [
            position.x - camera_position.x,
            position.y - camera_position.y,
            position.z - camera_position.z,
        ];
        let expected = row[2].mul_add(
            relative[2],
            row[1].mul_add(relative[1], row[0] * relative[0]),
        );
        let actual = world_to_camera_depth_with_view_row(position, camera_position, row);
        assert_eq!(actual.to_bits(), expected.to_bits());
        assert_eq!(depth_to_key(actual), actual.max(0.0).to_bits());
    }

    #[test]
    fn candidate_high24_retains_visibility_and_stable_source_id_ties() {
        let depths = [
            f32::from_bits(0x3f80_0001),
            f32::from_bits(0x3f80_00fe),
            f32::from_bits(0x3fc0_0000),
            f32::from_bits(1.0_f32.to_bits() - 1),
            f32::from_bits(2.0_f32.to_bits() + 1),
        ];
        let positions = depths
            .into_iter()
            .map(|depth| Vec3f::new(0.0, 0.0, depth))
            .collect::<Vec<_>>();
        let camera = camera(1.0, 2.0);
        let mut exact_keys = Vec::new();
        let mut exact_ids = Vec::new();
        let mut candidate_keys = Vec::new();
        let mut candidate_ids = Vec::new();

        preprocess_positions_visible_into_with_precision(
            &positions,
            &camera,
            DepthKeyPrecision::ExactFull32,
            &mut exact_keys,
            &mut exact_ids,
        )
        .expect("Exact preprocess");
        preprocess_positions_visible_into_with_precision(
            &positions,
            &camera,
            DepthKeyPrecision::CandidateStable24,
            &mut candidate_keys,
            &mut candidate_ids,
        )
        .expect("candidate preprocess");

        assert_eq!(exact_ids, [0, 1, 2]);
        assert_eq!(candidate_ids, exact_ids);
        assert_eq!(exact_keys, [0x3f80_0001, 0x3f80_00fe, 0x3fc0_0000]);
        assert_eq!(candidate_keys, [0x3f80_0000, 0x3f80_0000, 0x3fc0_0000]);
        assert_eq!(DepthKeyPrecision::ExactFull32.retained_high_bits(), 32);
        assert_eq!(
            DepthKeyPrecision::CandidateStable24.retained_high_bits(),
            24
        );

        gsplat_sort::CpuSortBackend::default()
            .sort_values_by_keys(&candidate_keys, &mut candidate_ids)
            .expect("candidate stable radix");
        assert_eq!(candidate_ids, [2, 0, 1]);

        let mut exact_order = exact_ids;
        gsplat_sort::CpuSortBackend::default()
            .sort_values_by_keys(&exact_keys, &mut exact_order)
            .expect("Exact stable radix");
        assert_eq!(exact_order, [2, 1, 0]);
    }

    #[test]
    fn candidate_high24_reserves_zero_without_changing_exact_bits() {
        let smallest_positive = f32::from_bits(1);
        assert_eq!(
            depth_to_key_with_precision(smallest_positive, DepthKeyPrecision::ExactFull32),
            1
        );
        assert_eq!(
            depth_to_key_with_precision(smallest_positive, DepthKeyPrecision::CandidateStable24),
            0x0000_0100
        );
        assert_eq!(
            depth_to_key_with_precision(1.0, DepthKeyPrecision::CandidateStable24),
            1.0_f32.to_bits()
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn architecture_leaf_uses_the_shared_candidate_quantizer() {
        let positions = (0..257)
            .map(|index| {
                let low_bits = (index as u32).wrapping_mul(37) & 0xff;
                Vec3f::new(0.0, 0.0, f32::from_bits(0x3f80_0000 | low_bits))
            })
            .collect::<Vec<_>>();
        let camera = camera(1.0, 2.0);
        let mut scalar_keys = Vec::new();
        let mut scalar_ids = Vec::new();
        let mut leaf_keys = Vec::new();
        let mut leaf_ids = Vec::new();
        let mut chunks = Vec::new();

        preprocess_positions_visible_into_with_precision(
            &positions,
            &camera,
            DepthKeyPrecision::CandidateStable24,
            &mut scalar_keys,
            &mut scalar_ids,
        )
        .expect("scalar candidate preprocess");
        preprocess_positions_visible_into_parallel_with_precision(
            &positions,
            &camera,
            DepthKeyPrecision::CandidateStable24,
            &mut leaf_keys,
            &mut leaf_ids,
            &mut chunks,
        )
        .expect("architecture candidate preprocess");

        assert_eq!(leaf_keys, scalar_keys);
        assert_eq!(leaf_ids, scalar_ids);
        assert!(leaf_keys.iter().all(|key| key & 0xff == 0));
    }

    #[test]
    fn equal_depth_is_stable_source_id_order() {
        let positions = vec![Vec3f::new(0.0, 0.0, 2.0); 257];
        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::with_capacity(positions.len());
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("order");
        assert_eq!(order, (0..257_u32).collect::<Vec<_>>());
    }

    #[test]
    fn unsorted_position_order_preserves_visible_source_id_order() {
        let positions = [
            Vec3f::new(0.0, 0.0, 2.5),
            Vec3f::new(0.0, 0.0, 0.5),
            Vec3f::new(0.0, 0.0, 1.5),
            Vec3f::new(0.0, 0.0, 3.5),
            Vec3f::new(0.0, 0.0, 2.0),
        ];
        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::with_capacity(positions.len());

        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                false,
                &mut order,
            )
            .expect("visible source order");

        assert_eq!(order, [0, 2, 4]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn packed_order_matches_parallel_split_baseline_and_hash() {
        let positions = (0..(super::PARALLEL_PREPROCESS_THRESHOLD + 257))
            .map(|index| {
                let depth = 1.0 + (index % 8192) as f32 * 0.000_1;
                Vec3f::new(index as f32 * 0.000_01, 0.0, depth)
            })
            .collect::<Vec<_>>();
        let camera = camera(0.5, 4.0);
        let mut keys = Vec::new();
        let mut expected = Vec::new();
        let mut chunks = Vec::new();
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .expect("rayon pool")
            .install(|| {
                preprocess_positions_visible_into_parallel(
                    &positions,
                    &camera,
                    &mut keys,
                    &mut expected,
                    &mut chunks,
                )
                .expect("split preprocess");
            });
        gsplat_sort::CpuSortBackend::default()
            .sort_values_by_keys(&keys, &mut expected)
            .expect("split radix");

        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut actual = Vec::with_capacity(positions.len());
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .expect("rayon pool")
            .install(|| {
                engine
                    .order_positions(CpuPositionView::new(&positions), &camera, true, &mut actual)
                    .expect("packed order");
            });

        assert_eq!(actual, expected);
        assert_eq!(order_hash(&actual), order_hash(&expected));
        let frozen = engine.calibration_state();
        engine
            .order_positions(CpuPositionView::new(&positions), &camera, true, &mut actual)
            .expect("frozen packed order");
        assert_eq!(engine.calibration_state(), frozen);
        assert_eq!(order_hash(&actual), order_hash(&expected));
    }

    #[test]
    fn empty_single_and_invalid_camera_are_transactional() {
        let mut engine = CpuOrderEngine::try_with_capacity(1).expect("engine");
        let mut order = vec![99];
        engine
            .order_positions(
                CpuPositionView::new(&[]),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("empty");
        assert!(order.is_empty());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (decision, attempts) = engine.calibration_state();
            assert_eq!(attempts, 0);
            assert_eq!(decision, None);
        }

        let positions = [Vec3f::new(0.0, 0.0, 2.0)];
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("single");
        assert_eq!(order, [0]);

        let published = order.clone();
        assert!(
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(3.0, 1.0),
                    true,
                    &mut order,
                )
                .is_err()
        );
        assert_eq!(order, published);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn invalid_initial_camera_leaves_calibration_pending_and_authoritative_order_untouched() {
        let positions = vec![Vec3f::new(0.0, 0.0, 2.0); super::PARALLEL_PREPROCESS_THRESHOLD + 257];
        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = vec![99];

        assert!(
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(3.0, 1.0),
                    true,
                    &mut order,
                )
                .is_err()
        );
        assert_eq!(order, [99]);
        let (decision, attempts) = engine.calibration_state();
        assert_eq!(decision, None);
        assert_eq!(attempts, 0);

        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("first valid large input calibrates");
        assert_eq!(order.len(), positions.len());
        assert_eq!(order.first(), Some(&0));
        assert_eq!(order.last(), Some(&(positions.len() as u32 - 1)));
        let frozen = engine.calibration_state();
        assert!(frozen.0.is_some());
        engine
            .order_positions(
                CpuPositionView::new(&positions),
                &camera(1.0, 3.0),
                true,
                &mut order,
            )
            .expect("valid large input reuses frozen choice");
        assert_eq!(engine.calibration_state(), frozen);
    }

    #[test]
    fn warmup_reuses_bounded_double_buffers_without_reallocation() {
        let positions = (0..257)
            .map(|index| Vec3f::new(0.0, 0.0, 1.0 + index as f32 * 0.001))
            .collect::<Vec<_>>();
        let mut engine = CpuOrderEngine::try_with_capacity(positions.len()).expect("engine");
        let mut order = Vec::new();
        order.reserve_exact(positions.len());

        for _ in 0..2 {
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(0.5, 4.0),
                    true,
                    &mut order,
                )
                .expect("warmup");
        }
        #[cfg(not(target_arch = "wasm32"))]
        let frozen_calibration = engine.calibration_state();
        let first = (
            engine.buffer_state(),
            order.as_ptr() as usize,
            order.capacity(),
        );
        for _ in 0..4 {
            engine
                .order_positions(
                    CpuPositionView::new(&positions),
                    &camera(0.5, 4.0),
                    true,
                    &mut order,
                )
                .expect("steady state");
        }
        let second = (
            engine.buffer_state(),
            order.as_ptr() as usize,
            order.capacity(),
        );
        assert_eq!(second, first);
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert_eq!(frozen_calibration.1, 0);
            assert_eq!(frozen_calibration.0, None);
            assert_eq!(engine.calibration_state(), frozen_calibration);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn same_engine_small_then_large_initializes_calibration_only_at_threshold() {
        let small = [Vec3f::new(0.0, 0.0, 2.0)];
        let large = (0..(super::PARALLEL_PREPROCESS_THRESHOLD + 257))
            .map(|index| Vec3f::new(0.0, 0.0, 1.0 + (index % 4096) as f32 * 0.000_1))
            .collect::<Vec<_>>();
        let camera = camera(0.5, 4.0);
        let mut engine = CpuOrderEngine::try_with_capacity(large.len()).expect("engine");
        let mut order = Vec::with_capacity(large.len());

        engine
            .order_positions(CpuPositionView::new(&small), &camera, true, &mut order)
            .expect("small serial order");
        assert_eq!(engine.calibration_state(), (None, 0));

        engine
            .order_positions(CpuPositionView::new(&large), &camera, true, &mut order)
            .expect("large initialized order");
        let frozen = engine.calibration_state();
        assert!(frozen.0.is_some());
        engine
            .order_positions(CpuPositionView::new(&large), &camera, true, &mut order)
            .expect("large frozen order");
        assert_eq!(engine.calibration_state(), frozen);
    }

    #[test]
    fn scalar_output_growth_reaches_target_then_reuses_the_same_addresses() {
        let mut positions = vec![Vec3f::new(0.0, 0.0, 2.0); 64];
        let mut depth_keys = Vec::with_capacity(64);
        let mut source_ids = Vec::with_capacity(64);
        let camera = camera(1.0, 3.0);
        preprocess_positions_visible_into(&positions, &camera, &mut depth_keys, &mut source_ids)
            .expect("small warmup");

        positions.resize(100, Vec3f::new(0.0, 0.0, 2.0));
        preprocess_positions_visible_into(&positions, &camera, &mut depth_keys, &mut source_ids)
            .expect("grown input");
        assert!(depth_keys.capacity() >= positions.len());
        assert!(source_ids.capacity() >= positions.len());
        let grown = (
            depth_keys.as_ptr() as usize,
            depth_keys.capacity(),
            source_ids.as_ptr() as usize,
            source_ids.capacity(),
        );

        preprocess_positions_visible_into(&positions, &camera, &mut depth_keys, &mut source_ids)
            .expect("same-size reuse");
        assert_eq!(
            (
                depth_keys.as_ptr() as usize,
                depth_keys.capacity(),
                source_ids.as_ptr() as usize,
                source_ids.capacity(),
            ),
            grown
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn order_hash(order: &[u32]) -> u64 {
        order.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, id| {
            (hash ^ u64::from(*id)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn e6_positions(count: usize) -> Vec<Vec3f> {
        let mut state = 0x6d2b_79f5_u32;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = (state & 0xffff) as f32 * (1.0 / 65_535.0) - 0.5;
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let y = (state & 0xffff) as f32 * (1.0 / 65_535.0) - 0.5;
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let z = 1.0 + (state & 0x00ff_ffff) as f32 * (998.0 / 16_777_215.0);
                Vec3f::new(x, y, z)
            })
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn e6_baseline_sample(
        positions: &[Vec3f],
        camera: &Camera,
        keys: &mut Vec<u32>,
        order: &mut Vec<u32>,
        chunks: &mut Vec<super::PreprocessChunkScratch>,
        sorter: &mut gsplat_sort::CpuSortBackend,
    ) -> (f64, f64, u64) {
        let total_started = Instant::now();
        let preprocess_started = Instant::now();
        preprocess_positions_visible_into_parallel(positions, camera, keys, order, chunks)
            .expect("baseline preprocess");
        let preprocess_ms = preprocess_started.elapsed().as_secs_f64() * 1000.0;
        sorter
            .sort_values_by_keys(keys, order)
            .expect("baseline radix");
        let total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
        (preprocess_ms, total_ms, order_hash(order))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn e6_packed_sample(
        positions: &[Vec3f],
        camera: &Camera,
        engine: &mut CpuOrderEngine,
        order: &mut Vec<u32>,
    ) -> (f64, f64, u64) {
        let total_started = Instant::now();
        let timings = engine
            .order_positions(CpuPositionView::new(positions), camera, true, order)
            .expect("packed order");
        let total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
        (
            f64::from(timings.preprocess_ms),
            total_ms,
            order_hash(order),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "finite E7 native initialization calibration observation"]
    fn e7_native_initialization_calibration_observation() {
        const COUNT: usize = 500_000;

        let positions = e6_positions(COUNT);
        let camera = camera(0.5, 1_000.0);
        let mut engine = CpuOrderEngine::try_with_capacity(COUNT).expect("engine");
        let mut order = Vec::with_capacity(COUNT);
        let first_started = Instant::now();
        engine
            .order_positions(CpuPositionView::new(&positions), &camera, true, &mut order)
            .expect("calibrated order");
        let first_elapsed = first_started.elapsed();
        let first_hash = order_hash(&order);
        let frozen = engine.calibration_state();

        let second_started = Instant::now();
        engine
            .order_positions(CpuPositionView::new(&positions), &camera, true, &mut order)
            .expect("frozen order");
        let second_elapsed = second_started.elapsed();
        assert_eq!(order_hash(&order), first_hash);
        assert_eq!(engine.calibration_state(), frozen);
        eprintln!(
            "E7_CALIBRATION count={COUNT} decision={:?} attempts={} first_total_ms={:.6} frozen_total_ms={:.6} order_hash={first_hash:016x}",
            frozen.0,
            frozen.1,
            first_elapsed.as_secs_f64() * 1_000.0,
            second_elapsed.as_secs_f64() * 1_000.0,
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "finite E6 native release benchmark"]
    fn e6_direct_packed_m4_release_benchmark() {
        const COUNT: usize = 2_541_226;
        const SCHEDULE: [bool; 6] = [false, true, true, false, false, true];

        let positions = e6_positions(COUNT);
        let camera = camera(0.5, 1_000.0);
        let mut baseline_keys = Vec::with_capacity(COUNT);
        let mut baseline_order = Vec::with_capacity(COUNT);
        let mut baseline_chunks = Vec::new();
        let mut baseline_sorter = gsplat_sort::CpuSortBackend::default();
        let mut packed_engine = CpuOrderEngine::try_with_capacity(COUNT).expect("packed engine");
        let mut packed_order = Vec::with_capacity(COUNT);

        let baseline_warmup = e6_baseline_sample(
            &positions,
            &camera,
            &mut baseline_keys,
            &mut baseline_order,
            &mut baseline_chunks,
            &mut baseline_sorter,
        );
        let packed_warmup =
            e6_packed_sample(&positions, &camera, &mut packed_engine, &mut packed_order);
        assert_eq!(baseline_order, packed_order);
        assert_eq!(baseline_warmup.2, packed_warmup.2);

        for (run, packed) in SCHEDULE.into_iter().enumerate() {
            let (preprocess_ms, total_ms, hash) = if packed {
                e6_packed_sample(&positions, &camera, &mut packed_engine, &mut packed_order)
            } else {
                e6_baseline_sample(
                    &positions,
                    &camera,
                    &mut baseline_keys,
                    &mut baseline_order,
                    &mut baseline_chunks,
                    &mut baseline_sorter,
                )
            };
            assert_eq!(hash, baseline_warmup.2);
            eprintln!(
                "E6_SAMPLE run={} path={} count={} preprocess_ms={:.6} total_ms={:.6} order_hash={:016x}",
                run + 1,
                if packed { "packed" } else { "baseline" },
                COUNT,
                preprocess_ms,
                total_ms,
                hash,
            );
        }
    }
}
