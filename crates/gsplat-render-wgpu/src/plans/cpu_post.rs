use gsplat_core::{Camera, Vec3f};
use gsplat_sort::CpuSortBackend;
use thiserror::Error;

use crate::RendererError;
#[cfg(target_arch = "wasm32")]
use crate::cpu_order::preprocess_positions_visible_into;
#[cfg(not(target_arch = "wasm32"))]
use crate::cpu_order::{
    MAX_PARALLEL_PREPROCESS_CHUNKS, PreprocessChunkScratch,
    preprocess_positions_visible_into_parallel,
};
use crate::scene::SceneRuntime;

use super::{FrameIdentity, ProjectedWork};

#[derive(Debug, Error)]
pub(crate) enum CpuPostSortError {
    #[error("CPU PostSort allocation failed for {resource}")]
    AllocationFailed { resource: &'static str },
    #[error("CPU PostSort scene count mismatch: expected {expected}, got {actual}")]
    SceneCountMismatch { expected: u32, actual: usize },
    #[error("CPU PostSort order generation is exhausted")]
    OrderGenerationExhausted,
    #[error("CPU PostSort preprocessing or sorting failed: {0}")]
    Renderer(#[from] RendererError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CpuOrderGuard {
    scene_generation: u64,
    camera_revision: u64,
    contract_generation: u64,
    plan_set_generation: u64,
    source_count: u32,
    camera: Camera,
}

impl CpuOrderGuard {
    fn new(frame: FrameIdentity, source_count: u32, camera: Camera) -> Self {
        Self {
            scene_generation: frame.scene_generation(),
            camera_revision: frame.camera_revision(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
            source_count,
            camera,
        }
    }
}

/// Reusable workspace and authoritative order cache for Exact CPU PostSort.
pub(super) struct CpuPostSortPlan {
    depth_keys: Vec<u32>,
    working_ids: Vec<u32>,
    ordered_ids: Vec<u32>,
    sorter: CpuSortBackend,
    #[cfg(not(target_arch = "wasm32"))]
    native_chunks: Vec<PreprocessChunkScratch>,
    guard: Option<CpuOrderGuard>,
    order_generation: u64,
}

impl CpuPostSortPlan {
    pub(super) fn prepare(source_count: usize) -> Result<Self, CpuPostSortError> {
        let mut depth_keys = Vec::new();
        depth_keys.try_reserve_exact(source_count).map_err(|_| {
            CpuPostSortError::AllocationFailed {
                resource: "depth keys",
            }
        })?;
        let mut working_ids = Vec::new();
        working_ids.try_reserve_exact(source_count).map_err(|_| {
            CpuPostSortError::AllocationFailed {
                resource: "working source IDs",
            }
        })?;
        let mut ordered_ids = Vec::new();
        ordered_ids.try_reserve_exact(source_count).map_err(|_| {
            CpuPostSortError::AllocationFailed {
                resource: "authoritative source IDs",
            }
        })?;

        #[cfg(not(target_arch = "wasm32"))]
        let native_chunks = {
            let mut chunks = Vec::new();
            chunks
                .try_reserve_exact(MAX_PARALLEL_PREPROCESS_CHUNKS)
                .map_err(|_| CpuPostSortError::AllocationFailed {
                    resource: "native chunk scratch",
                })?;
            chunks.resize_with(
                MAX_PARALLEL_PREPROCESS_CHUNKS,
                PreprocessChunkScratch::default,
            );
            chunks
        };

        Ok(Self {
            depth_keys,
            working_ids,
            ordered_ids,
            sorter: CpuSortBackend::default(),
            #[cfg(not(target_arch = "wasm32"))]
            native_chunks,
            guard: None,
            order_generation: 0,
        })
    }

    pub(super) fn execute<'a>(
        &'a mut self,
        scene: &SceneRuntime,
        camera: &Camera,
        frame: FrameIdentity,
        source_count: u32,
    ) -> Result<ProjectedWork<'a>, CpuPostSortError> {
        if scene.source_count() != source_count as usize {
            return Err(CpuPostSortError::SceneCountMismatch {
                expected: source_count,
                actual: scene.source_count(),
            });
        }

        let requested_guard = CpuOrderGuard::new(frame, source_count, *camera);
        if self.guard != Some(requested_guard) {
            self.refresh_order(scene.positions(), camera, requested_guard)?;
        }

        Ok(ProjectedWork::from_cpu_post_sort(
            frame,
            self.order_generation,
            source_count,
            &self.ordered_ids,
        ))
    }

    fn refresh_order(
        &mut self,
        positions: &[Vec3f],
        camera: &Camera,
        requested_guard: CpuOrderGuard,
    ) -> Result<(), CpuPostSortError> {
        let next_generation = self
            .order_generation
            .checked_add(1)
            .ok_or(CpuPostSortError::OrderGenerationExhausted)?;

        #[cfg(not(target_arch = "wasm32"))]
        preprocess_positions_visible_into_parallel(
            positions,
            camera,
            &mut self.depth_keys,
            &mut self.working_ids,
            &mut self.native_chunks,
        )?;
        #[cfg(target_arch = "wasm32")]
        preprocess_positions_visible_into(
            positions,
            camera,
            &mut self.depth_keys,
            &mut self.working_ids,
        )?;

        self.sorter
            .sort_values_by_keys(&self.depth_keys, &mut self.working_ids)
            .map_err(RendererError::from)?;

        std::mem::swap(&mut self.ordered_ids, &mut self.working_ids);
        self.order_generation = next_generation;
        self.guard = Some(requested_guard);
        Ok(())
    }

    pub(super) fn last_usable_order(&self) -> Option<&[u32]> {
        self.guard.as_ref().map(|_| self.ordered_ids.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, SceneBuffers, Vec3f};

    use super::CpuPostSortPlan;
    use crate::plans::FrameIdentity;
    use crate::scene::{ResidentSceneCpu, SceneRuntime};

    fn runtime(depths: &[f32]) -> SceneRuntime {
        let count = depths.len();
        let scene = SceneBuffers {
            positions: depths
                .iter()
                .copied()
                .map(|z| Vec3f::new(0.0, 0.0, z))
                .collect(),
            opacity: vec![0.0; count],
            scale_xyz: vec![[0.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.0; 3]; count],
            sh_degree: 0,
            sh_rest: None,
        };
        let resident = ResidentSceneCpu::encode_owned(scene).expect("resident scene");
        SceneRuntime::prepare(resident).expect("scene runtime")
    }

    fn camera(near_plane: f32, far_plane: f32) -> Camera {
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = near_plane;
        camera.intrinsics.far_plane = far_plane;
        camera
    }

    fn identity(camera_revision: u64) -> FrameIdentity {
        FrameIdentity::new(1, camera_revision, 1, 1, 1)
    }

    #[test]
    fn near_and_far_are_inclusive() {
        let below_near = f32::from_bits(1.0_f32.to_bits() - 1);
        let above_far = f32::from_bits(3.0_f32.to_bits() + 1);
        let scene = runtime(&[1.0, below_near, 3.0, above_far]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");

        let work = plan
            .execute(&scene, &camera(1.0, 3.0), identity(1), 4)
            .expect("CPU order");

        assert_eq!(work.cpu_order_ids().expect("CPU IDs"), [2, 0]);
        assert_eq!(work.visible_count(), Ok(2));
    }

    #[test]
    fn depth_is_descending_and_equal_depth_ties_keep_source_ids() {
        let scene = runtime(&[2.0, 3.0, 2.0, 1.0]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");

        let work = plan
            .execute(&scene, &camera(0.5, 4.0), identity(1), 4)
            .expect("CPU order");

        assert_eq!(work.cpu_order_ids().expect("CPU IDs"), [1, 0, 2, 3]);
    }

    #[test]
    fn empty_and_single_point_orders_are_exact() {
        let empty = runtime(&[]);
        let mut empty_plan = CpuPostSortPlan::prepare(0).expect("empty plan");
        let empty_work = empty_plan
            .execute(&empty, &camera(0.5, 4.0), identity(1), 0)
            .expect("empty order");
        assert!(empty_work.cpu_order_ids().expect("CPU IDs").is_empty());

        let single = runtime(&[2.0]);
        let mut single_plan = CpuPostSortPlan::prepare(1).expect("single plan");
        let single_work = single_plan
            .execute(&single, &camera(0.5, 4.0), identity(1), 1)
            .expect("single order");
        assert_eq!(single_work.cpu_order_ids().expect("CPU IDs"), [0]);
    }

    #[test]
    fn invalid_camera_preserves_the_last_usable_order() {
        let scene = runtime(&[1.0, 3.0, 2.0]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");
        let first = plan
            .execute(&scene, &camera(0.5, 4.0), identity(1), 3)
            .expect("CPU order")
            .cpu_order_ids()
            .expect("CPU IDs")
            .to_vec();

        let error = plan.execute(&scene, &camera(4.0, 1.0), identity(2), 3);

        assert!(error.is_err());
        assert_eq!(plan.last_usable_order(), Some(first.as_slice()));
    }

    #[test]
    fn identical_complete_guard_reuses_the_order_generation() {
        let scene = runtime(&[1.0, 3.0, 2.0]);
        let mut plan = CpuPostSortPlan::prepare(scene.source_count()).expect("plan");
        let first_generation = plan
            .execute(&scene, &camera(0.5, 4.0), identity(1), 3)
            .expect("first order")
            .order_generation();
        let second_generation = plan
            .execute(&scene, &camera(0.5, 4.0), identity(1), 3)
            .expect("reused order")
            .order_generation();

        assert_eq!(first_generation, second_generation);
    }
}
