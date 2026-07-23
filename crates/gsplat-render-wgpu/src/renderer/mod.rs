//! Shadow-only Exact runtime preparation and frame dispatch.

pub(crate) mod frame;

use gsplat_core::Camera;
use thiserror::Error;

use crate::plans::{PlanId, PlanSet, PlanSetError, ProjectedWork};
use crate::scene::{ResidentSceneCpu, ResidentSceneError, SceneRuntime};

use frame::{FrameState, GenerationError, Viewport};

/// The only E1 contract: Exact fidelity over one complete resident scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderContract {
    source_count: u32,
    sh_degree: u8,
}

impl RenderContract {
    fn exact_all_resident(scene: &SceneRuntime) -> Result<Self, PreparedRuntimeError> {
        let source_count = u32::try_from(scene.source_count())
            .map_err(|_| PreparedRuntimeError::SourceCountOverflow)?;
        Ok(Self {
            source_count,
            sh_degree: scene.sh_degree(),
        })
    }

    fn validate(self, scene: &SceneRuntime) -> Result<(), PreparedRuntimeError> {
        if scene.source_count() != self.source_count as usize
            || scene.sh_degree() != self.sh_degree
            || self.sh_degree > 3
        {
            return Err(PreparedRuntimeError::ContractMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub(crate) enum PreparedRuntimeError {
    #[error("resident scene preparation failed: {0}")]
    Scene(#[from] ResidentSceneError),
    #[error("Exact all-resident source count exceeds u32 addressability")]
    SourceCountOverflow,
    #[error("prepared scene and Exact contract are incompatible")]
    ContractMismatch,
    #[error("plan-set preparation failed: {0}")]
    PlanSet(#[from] PlanSetError),
    #[error("runtime generation update failed: {0}")]
    Generation(#[from] GenerationError),
}

#[derive(Debug, Error)]
pub(crate) enum FrameExecutionError {
    #[error("frame generation update failed: {0}")]
    Generation(#[from] GenerationError),
    #[error("plan execution failed: {0}")]
    PlanSet(#[from] PlanSetError),
}

/// One fully validated shadow runtime candidate.
///
/// Canonical raster is intentionally absent in E1: no real raster resource is
/// constructed in this task, so the shadow boundary ends at `ProjectedWork`
/// instead of publishing a zero-sized or semantically fake raster owner.
pub(crate) struct PreparedRuntime {
    contract: RenderContract,
    scene: SceneRuntime,
    plans: PlanSet,
}

impl PreparedRuntime {
    fn prepare(resident: ResidentSceneCpu) -> Result<Self, PreparedRuntimeError> {
        let scene = SceneRuntime::prepare(resident)?;
        let contract = RenderContract::exact_all_resident(&scene)?;
        contract.validate(&scene)?;
        let plans = PlanSet::prepare_cpu(scene.source_count())?;
        Ok(Self {
            contract,
            scene,
            plans,
        })
    }
}

/// Publication slot used only by the private shadow core.
///
/// This is not a second product renderer: it owns only the prepared bundle and
/// its complete input identity. It has no GPU context, target, controller,
/// sampler, evidence sink, submission or presentation behavior, and exists to
/// make whole-runtime replacement one infallible swap.
pub(crate) struct PreparedRuntimeSlot {
    runtime: PreparedRuntime,
    frame: FrameState,
}

impl PreparedRuntimeSlot {
    pub(crate) fn prepare(resident: ResidentSceneCpu) -> Result<Self, PreparedRuntimeError> {
        Ok(Self {
            runtime: PreparedRuntime::prepare(resident)?,
            frame: FrameState::initial(),
        })
    }

    pub(crate) fn replace(
        &mut self,
        resident: ResidentSceneCpu,
    ) -> Result<(), PreparedRuntimeError> {
        let next_frame = self.frame.after_runtime_replacement()?;
        let next_runtime = PreparedRuntime::prepare(resident)?;
        let candidate = Self {
            runtime: next_runtime,
            frame: next_frame,
        };
        *self = candidate;
        Ok(())
    }

    pub(crate) const fn frame_state(&self) -> FrameState {
        self.frame
    }

    pub(crate) fn fallback(&self) -> PlanId {
        self.runtime.plans.fallback()
    }

    pub(crate) fn eligible(&self) -> &[PlanId] {
        self.runtime.plans.eligible()
    }

    pub(crate) fn scene(&self) -> &SceneRuntime {
        &self.runtime.scene
    }

    pub(crate) fn last_usable_cpu_order(&self) -> Option<&[u32]> {
        self.runtime.plans.last_usable_cpu_order()
    }
}

/// E1's sole per-frame renderer boundary. It delegates the closed dispatch to
/// `PlanSet` and does not inspect plan-internal variants. A later real GPU
/// projected-view/preparation adapter is intentionally outside this skeleton.
pub(crate) fn execute_frame<'a>(
    slot: &'a mut PreparedRuntimeSlot,
    requested: PlanId,
    camera: &Camera,
    viewport: Viewport,
) -> Result<ProjectedWork<'a>, FrameExecutionError> {
    let candidate_frame = slot.frame.candidate_for_frame(*camera, viewport)?;
    let source_count = slot.runtime.contract.source_count;
    let work = slot.runtime.plans.execute(
        requested,
        &slot.runtime.scene,
        camera,
        candidate_frame.identity(),
        source_count,
    )?;
    slot.frame = candidate_frame;
    Ok(work)
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, SceneBuffers, Vec3f};

    use super::{PlanId, PreparedRuntimeSlot, execute_frame};
    use crate::renderer::frame::Viewport;
    use crate::scene::ResidentSceneCpu;

    fn resident(depths: &[f32]) -> ResidentSceneCpu {
        let count = depths.len();
        ResidentSceneCpu::encode_owned(SceneBuffers {
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
        })
        .expect("resident scene")
    }

    #[test]
    fn failed_replacement_preserves_runtime_generations_fallback_and_order() {
        let mut slot =
            PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
        let work = execute_frame(
            &mut slot,
            PlanId::CpuPostSort,
            &Camera::default(),
            Viewport::new(640, 480).expect("viewport"),
        )
        .expect("CPU frame");
        let old_order = work.cpu_order_ids().expect("CPU IDs").to_vec();
        let old_frame = slot.frame_state();
        let old_fallback = slot.fallback();
        let old_scene_positions = slot.scene().positions().as_ptr();

        let mut invalid = resident(&[9.0]);
        invalid.sh_degree = 4;
        let result = slot.replace(invalid);

        assert!(result.is_err());
        assert_eq!(slot.frame_state(), old_frame);
        assert_eq!(slot.fallback(), old_fallback);
        assert_eq!(slot.eligible(), [PlanId::CpuPostSort]);
        assert_eq!(slot.scene().positions().as_ptr(), old_scene_positions);
        assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
    }

    #[test]
    fn successful_replacement_publishes_one_fresh_runtime() {
        let mut slot = PreparedRuntimeSlot::prepare(resident(&[1.0])).expect("prepared runtime");
        let old_frame = slot.frame_state().identity();

        slot.replace(resident(&[2.0, 3.0]))
            .expect("runtime replacement");

        let new_frame = slot.frame_state().identity();
        assert_eq!(
            new_frame.scene_generation(),
            old_frame.scene_generation() + 1
        );
        assert_eq!(
            new_frame.contract_generation(),
            old_frame.contract_generation() + 1
        );
        assert_eq!(
            new_frame.plan_set_generation(),
            old_frame.plan_set_generation() + 1
        );
        assert_eq!(slot.scene().source_count(), 2);
        assert_eq!(slot.last_usable_cpu_order(), None);
    }

    #[test]
    fn frame_identity_tracks_camera_and_viewport_only_after_success() {
        let mut slot =
            PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
        let viewport = Viewport::new(640, 480).expect("viewport");
        let first_work =
            execute_frame(&mut slot, PlanId::CpuPostSort, &Camera::default(), viewport)
                .expect("first frame");
        let first = first_work.frame_identity();
        let first_order_generation = first_work.order_generation();
        let unchanged_work =
            execute_frame(&mut slot, PlanId::CpuPostSort, &Camera::default(), viewport)
                .expect("unchanged frame");
        let unchanged = unchanged_work.frame_identity();
        let unchanged_order_generation = unchanged_work.order_generation();

        let mut moved_camera = Camera::default();
        moved_camera.pose.position.z = -1.0;
        let moved_work = execute_frame(&mut slot, PlanId::CpuPostSort, &moved_camera, viewport)
            .expect("moved frame");
        let moved = moved_work.frame_identity();
        let moved_order_generation = moved_work.order_generation();
        let resized_work = execute_frame(
            &mut slot,
            PlanId::CpuPostSort,
            &moved_camera,
            Viewport::new(800, 600).expect("resized viewport"),
        )
        .expect("resized frame");
        let resized = resized_work.frame_identity();
        let resized_order_generation = resized_work.order_generation();

        assert_eq!(unchanged, first);
        assert_eq!(unchanged_order_generation, first_order_generation);
        assert_eq!(moved.camera_revision(), first.camera_revision() + 1);
        assert_eq!(moved.viewport_generation(), first.viewport_generation());
        assert_eq!(moved_order_generation, first_order_generation + 1);
        assert_eq!(resized.camera_revision(), moved.camera_revision());
        assert_eq!(
            resized.viewport_generation(),
            moved.viewport_generation() + 1
        );
        assert_eq!(resized_order_generation, moved_order_generation);
    }

    #[test]
    fn failed_frame_keeps_input_identity_and_last_usable_order() {
        let mut slot =
            PreparedRuntimeSlot::prepare(resident(&[1.0, 3.0, 2.0])).expect("prepared runtime");
        let viewport = Viewport::new(640, 480).expect("viewport");
        let first = execute_frame(&mut slot, PlanId::CpuPostSort, &Camera::default(), viewport)
            .expect("first frame");
        let old_order = first.cpu_order_ids().expect("CPU IDs").to_vec();
        let old_frame = slot.frame_state();

        let mut invalid_camera = Camera::default();
        invalid_camera.intrinsics.near_plane = 2.0;
        invalid_camera.intrinsics.far_plane = 1.0;
        let result = execute_frame(
            &mut slot,
            PlanId::CpuPostSort,
            &invalid_camera,
            Viewport::new(800, 600).expect("candidate viewport"),
        );

        assert!(result.is_err());
        assert_eq!(slot.frame_state(), old_frame);
        assert_eq!(slot.last_usable_cpu_order(), Some(old_order.as_slice()));
    }
}
