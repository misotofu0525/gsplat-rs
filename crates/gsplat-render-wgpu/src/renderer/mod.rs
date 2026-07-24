//! Shadow-only Exact runtime preparation and frame dispatch.

pub(crate) mod frame;
pub(crate) mod gpu_prepare;

use std::sync::Arc;

use gsplat_core::Camera;
use thiserror::Error;

use crate::plans::{
    GpuCapabilityReceipt, GpuPlanAdmissionRequest, PlanExecutionContext, PlanFrameInput, PlanId,
    PlanSet, PlanSetError, ProjectedWork, StagedGpuPlanAdmission,
};
use crate::scene::{ResidentSceneCpu, ResidentSceneError, SceneRuntime};

use frame::{FrameState, GenerationError, Viewport};
use gpu_prepare::{
    GpuExecutionOwner, GpuPreparationError, GpuPreparationReceipt, GpuScenePreparation,
};

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
    #[error("GPU execution context is unavailable: {0}")]
    GpuPreparation(#[from] GpuPreparationError),
}

#[derive(Debug, Error)]
pub(crate) enum GpuRuntimePreparationError {
    #[error("GPU resource preparation failed: {0}")]
    Resources(#[from] GpuPreparationError),
    #[error("GPU PlanSet admission failed: {0}")]
    PlanSet(#[from] PlanSetError),
    #[error("GPU PlanSet generation update failed: {0}")]
    Generation(#[from] GenerationError),
}

/// Result of optional device preparation. An omitted GPU candidate never
/// weakens or removes the already prepared Exact CPU fallback.
#[derive(Debug)]
pub(crate) enum GpuPreparationStatus {
    Ready(GpuPreparationReceipt),
    Omitted(GpuRuntimePreparationError),
}

impl GpuPreparationStatus {
    pub(crate) const fn receipt(&self) -> Option<GpuPreparationReceipt> {
        match self {
            Self::Ready(receipt) => Some(*receipt),
            Self::Omitted(_) => None,
        }
    }

    pub(crate) const fn omission(&self) -> Option<&GpuRuntimePreparationError> {
        match self {
            Self::Ready(_) => None,
            Self::Omitted(error) => Some(error),
        }
    }
}

/// One fully validated shadow runtime candidate.
///
/// Canonical raster remains intentionally absent: E8a can attach one complete
/// dormant device-owned scene/project graph, but no raster resource or fake
/// GPU plan is published by this adapter task.
pub(crate) struct PreparedRuntime {
    contract: RenderContract,
    scene: SceneRuntime,
    plans: PlanSet,
}

impl PreparedRuntime {
    fn prepare(
        resident: ResidentSceneCpu,
        frame: FrameState,
    ) -> Result<Self, PreparedRuntimeError> {
        let scene = SceneRuntime::prepare(resident)?;
        let contract = RenderContract::exact_all_resident(&scene)?;
        contract.validate(&scene)?;
        let plans =
            PlanSet::prepare_cpu(scene.source_count(), scene.sh_degree(), frame.identity())?;
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
/// its complete input identity. Any optional GPU graph stays under the sole
/// `SceneRuntime` owner and uses the caller's device; the slot has no target,
/// controller, sampler, evidence sink, submission or presentation behavior.
pub(crate) struct PreparedRuntimeSlot {
    runtime: PreparedRuntime,
    frame: FrameState,
    gpu_owner: Option<GpuExecutionOwner>,
}

struct StagedGpuRuntimeAdmission {
    scene: GpuScenePreparation,
    plans: StagedGpuPlanAdmission,
    frame: FrameState,
    owner: GpuExecutionOwner,
}

impl PreparedRuntimeSlot {
    pub(crate) fn prepare(resident: ResidentSceneCpu) -> Result<Self, PreparedRuntimeError> {
        let frame = FrameState::initial();
        Ok(Self {
            runtime: PreparedRuntime::prepare(resident, frame)?,
            frame,
            gpu_owner: None,
        })
    }

    pub(crate) fn replace(
        &mut self,
        resident: ResidentSceneCpu,
    ) -> Result<(), PreparedRuntimeError> {
        let next_frame = self.frame.after_runtime_replacement()?;
        let next_runtime = PreparedRuntime::prepare(resident, next_frame)?;
        let candidate = Self {
            runtime: next_runtime,
            frame: next_frame,
            gpu_owner: None,
        };
        *self = candidate;
        Ok(())
    }

    /// Prepares the complete dormant GPU scene/project graph against the
    /// caller's existing device/queue pair. No adapter or second device is
    /// acquired; Arc identity keeps later queue borrows tied to this owner.
    /// Publication into `SceneRuntime` is one infallible commit after every
    /// resource constructor and asynchronous device error scope succeeds.
    pub(crate) async fn prepare_gpu(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
    ) -> Result<GpuPreparationReceipt, GpuRuntimePreparationError> {
        if self.gpu_owner.is_some() {
            return Err(GpuPreparationError::ExecutionOwnerAlreadyBound.into());
        }
        let previous_frame = self.frame.identity();
        let next_frame = self.frame.candidate_for_plan_set_admission()?;
        let owner = GpuExecutionOwner::new(device, queue);
        let scene = self
            .runtime
            .scene
            .stage_gpu(&owner, next_frame.identity())
            .await?;
        let receipt = scene.receipt();
        let plans = self
            .runtime
            .plans
            .stage_gpu_admission(GpuPlanAdmissionRequest::new(
                receipt.source_count(),
                receipt.capacity(),
                receipt.resident_count(),
                receipt.addressable_count(),
                receipt.sh_degree(),
                receipt.scene_generation(),
                receipt.contract_generation(),
                previous_frame.plan_set_generation(),
                receipt.plan_set_generation(),
            ))?;
        self.commit_gpu_admission(StagedGpuRuntimeAdmission {
            scene,
            plans,
            frame: next_frame,
            owner,
        });
        Ok(receipt)
    }

    fn commit_gpu_admission(&mut self, staged: StagedGpuRuntimeAdmission) {
        self.runtime.scene.commit_gpu(staged.scene);
        self.runtime.plans.commit_gpu_admission(staged.plans);
        self.frame = staged.frame;
        self.gpu_owner = Some(staged.owner);
    }

    /// Best-effort admission for the future closed plan set. Failure leaves
    /// CPU PostSort prepared and eligible; the error remains structured so a
    /// forced future GPU selection can fail closed instead of silently lying.
    pub(crate) async fn prepare_gpu_optional(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
    ) -> GpuPreparationStatus {
        match self.prepare_gpu(device, queue).await {
            Ok(receipt) => GpuPreparationStatus::Ready(receipt),
            Err(error) => GpuPreparationStatus::Omitted(error),
        }
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

    pub(crate) fn gpu_preparation(&self) -> Option<GpuPreparationReceipt> {
        self.runtime.scene.gpu_preparation()
    }

    pub(crate) fn gpu_capability(&self) -> Option<GpuCapabilityReceipt> {
        self.runtime.plans.gpu_capability()
    }

    #[cfg(test)]
    pub(crate) fn set_test_gpu_admission_mode(&mut self, mode: crate::plans::TestGpuAdmissionMode) {
        self.runtime.plans.set_test_gpu_admission_mode(mode);
    }

    #[cfg(test)]
    pub(crate) fn test_gpu_post_capability(&self) -> Option<GpuCapabilityReceipt> {
        self.runtime.plans.test_gpu_post_capability()
    }

    pub(crate) fn last_usable_cpu_order(&self) -> Option<&[u32]> {
        self.runtime.plans.last_usable_cpu_order()
    }
}

/// Sole prepared-plan frame dispatch. It delegates the closed dispatch to
/// `PlanSet` and does not inspect plan-internal variants. E8a only supplies the
/// device/encoder seam; the later concrete GPU plan remains unregistered.
pub(crate) fn execute_frame<'a>(
    slot: &'a mut PreparedRuntimeSlot,
    requested: PlanId,
    camera: &Camera,
    viewport: Viewport,
) -> Result<ProjectedWork<'a>, FrameExecutionError> {
    let candidate_frame = slot.frame.candidate_for_frame(*camera, viewport)?;
    let work = execute_prepared_runtime(
        &mut slot.runtime,
        requested,
        camera,
        viewport,
        candidate_frame.identity(),
        PlanExecutionContext::Cpu,
    )?;
    slot.frame = candidate_frame;
    Ok(work)
}

/// GPU-capable entry to the same renderer execute boundary. The queue comes
/// from the renderer-owned context bound during preparation; the caller lends
/// only its existing same-device encoder. The owner token proves owner/queue,
/// while encoder provenance is a private caller invariant because wgpu has no
/// encoder identity API. Until E8 registers a concrete plan, PlanSet returns
/// structured unprepared rather than bypassing this route.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn execute_frame_gpu<'runtime>(
    slot: &'runtime mut PreparedRuntimeSlot,
    requested: PlanId,
    camera: &Camera,
    viewport: Viewport,
    queue: &Arc<wgpu::Queue>,
    encoder: &mut wgpu::CommandEncoder,
) -> Result<ProjectedWork<'runtime>, FrameExecutionError> {
    let candidate_frame = slot.frame.candidate_for_frame(*camera, viewport)?;
    let owner = slot
        .gpu_owner
        .as_ref()
        .ok_or(GpuPreparationError::Unavailable)?;
    let work = execute_prepared_runtime(
        &mut slot.runtime,
        requested,
        camera,
        viewport,
        candidate_frame.identity(),
        PlanExecutionContext::Gpu(owner.context(queue, encoder)?),
    )?;
    slot.frame = candidate_frame;
    Ok(work)
}

fn execute_prepared_runtime<'runtime>(
    runtime: &'runtime mut PreparedRuntime,
    requested: PlanId,
    camera: &Camera,
    viewport: Viewport,
    frame: crate::plans::FrameIdentity,
    execution: PlanExecutionContext<'_>,
) -> Result<ProjectedWork<'runtime>, FrameExecutionError> {
    let source_count = runtime.contract.source_count;
    runtime
        .plans
        .execute(
            requested,
            &mut runtime.scene,
            PlanFrameInput::new(
                camera,
                frame,
                source_count,
                viewport.width(),
                viewport.height(),
            ),
            execution,
        )
        .map_err(FrameExecutionError::from)
}

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
pub(crate) mod tests;
