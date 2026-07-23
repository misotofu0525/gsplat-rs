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
mod contract_tests;
#[cfg(test)]
pub(crate) mod tests;
