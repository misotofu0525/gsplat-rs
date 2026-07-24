//! Shadow-only Exact runtime preparation and frame dispatch.

pub(crate) mod frame;
pub(crate) mod gpu_prepare;

use std::sync::Arc;

use gsplat_core::Camera;
use thiserror::Error;

use crate::plans::{
    DirectCountSemantics, FrameIdentity, GpuCapabilityReceipt, GpuOwnerToken,
    GpuPlanAdmissionRequest, IndirectCountSemantics, OrderLane, PlanExecutionContext,
    PlanFrameInput, PlanId, PlanSet, PlanSetError, ProjectedWork, StagedGpuPlanAdmission,
};
use crate::raster::{CanonicalRaster, CanonicalRasterError, CanonicalRasterInput};
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
    #[error("canonical raster encoding failed: {0}")]
    Raster(#[from] CanonicalRasterError),
    #[error("prepared projected work is inconsistent at {component}")]
    ProjectedWorkMismatch { component: &'static str },
    #[error("GPU frame encode-attempt generation is exhausted")]
    EncodeAttemptExhausted,
    #[error("pending GPU frame is stale or inconsistent at {component}")]
    PendingFrameMismatch { component: &'static str },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RasterCountSemantics {
    DirectDrawEqualsVisible,
    IndirectDrawEqualsVisible,
    IndirectDrawEqualsContributor,
}

/// Immutable identity for one complete Exact shadow-core submission. Numeric
/// V/C/D remain optional when the authoritative count is GPU-owned; the count
/// relationship is always explicit.
pub(crate) struct GpuFrameSubmission {
    submission_index: wgpu::SubmissionIndex,
    frame: FrameIdentity,
    plan: PlanId,
    order_lane: OrderLane,
    order_generation: u64,
    source_count: u32,
    visible_count: Option<u32>,
    contributor_count: Option<u32>,
    draw_count: Option<u32>,
    count_semantics: RasterCountSemantics,
    encode_attempt: u64,
}

struct FrameSubmissionMetadata {
    frame: FrameIdentity,
    plan: PlanId,
    order_lane: OrderLane,
    order_generation: u64,
    source_count: u32,
    visible_count: Option<u32>,
    contributor_count: Option<u32>,
    draw_count: Option<u32>,
    count_semantics: RasterCountSemantics,
}

/// Owned transaction returned after one plan and one canonical raster have
/// been encoded. Renderer retains the encoder identity; the host may only
/// append capture/readback work to that same encoder before handing the
/// transaction back for the sole finish, submit and semantic publication.
pub(crate) struct PendingGpuFrame {
    encoder: wgpu::CommandEncoder,
    owner: GpuOwnerToken,
    base_frame: FrameState,
    candidate_frame: FrameState,
    encode_attempt: u64,
    metadata: FrameSubmissionMetadata,
}

impl PendingGpuFrame {
    /// Borrows the exact encoder that already contains plan and raster work.
    /// The encoder cannot be replaced or finished by the host, so the pending
    /// identity remains structurally tied to the submitted command stream.
    pub(crate) fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        &mut self.encoder
    }
}

pub(crate) struct GpuFrameEncodeRequest<'a> {
    requested: PlanId,
    camera: &'a Camera,
    viewport: Viewport,
    target: &'a wgpu::TextureView,
    target_format: wgpu::TextureFormat,
    clear: wgpu::Color,
}

impl<'a> GpuFrameEncodeRequest<'a> {
    pub(crate) const fn new(
        requested: PlanId,
        camera: &'a Camera,
        viewport: Viewport,
        target: &'a wgpu::TextureView,
        target_format: wgpu::TextureFormat,
        clear: wgpu::Color,
    ) -> Self {
        Self {
            requested,
            camera,
            viewport,
            target,
            target_format,
            clear,
        }
    }
}

#[allow(dead_code)]
impl GpuFrameSubmission {
    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn plan_id(&self) -> PlanId {
        self.plan
    }

    pub(crate) const fn order_lane(&self) -> OrderLane {
        self.order_lane
    }

    pub(crate) const fn order_generation(&self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn source_count(&self) -> u32 {
        self.source_count
    }

    pub(crate) const fn visible_count(&self) -> Option<u32> {
        self.visible_count
    }

    pub(crate) const fn contributor_count(&self) -> Option<u32> {
        self.contributor_count
    }

    pub(crate) const fn draw_count(&self) -> Option<u32> {
        self.draw_count
    }

    pub(crate) const fn count_semantics(&self) -> RasterCountSemantics {
        self.count_semantics
    }

    pub(crate) const fn submission_index(&self) -> &wgpu::SubmissionIndex {
        &self.submission_index
    }

    pub(crate) const fn encode_attempt(&self) -> u64 {
        self.encode_attempt
    }
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
/// CPU-only preparation leaves raster absent. GPU admission stages scene,
/// plans and target-format raster together, then publishes them in one commit.
pub(crate) struct PreparedRuntime {
    contract: RenderContract,
    scene: SceneRuntime,
    plans: PlanSet,
    raster: Option<CanonicalRaster>,
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
            raster: None,
        })
    }
}

/// Publication slot used only by the private shadow core.
///
/// This is not a second product renderer: it owns only the prepared bundle and
/// its complete input identity. Any optional GPU graph stays under the sole
/// `SceneRuntime` owner and uses the caller's device; the slot has no target,
/// controller, sampler, evidence sink or presentation behavior. It publishes
/// a semantic frame only through the single `submit_encoded_frame` boundary.
pub(crate) struct PreparedRuntimeSlot {
    runtime: PreparedRuntime,
    frame: FrameState,
    gpu_owner: Option<GpuExecutionOwner>,
    latest_encode_attempt: u64,
}

struct StagedGpuRuntimeAdmission {
    scene: GpuScenePreparation,
    plans: StagedGpuPlanAdmission,
    raster: CanonicalRaster,
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
            latest_encode_attempt: 0,
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
            latest_encode_attempt: 0,
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
        target_format: wgpu::TextureFormat,
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
        let raster = scene
            .prepare_canonical_raster(&owner, target_format)
            .await?;
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
            raster,
            frame: next_frame,
            owner,
        });
        Ok(receipt)
    }

    fn commit_gpu_admission(&mut self, staged: StagedGpuRuntimeAdmission) {
        self.runtime.scene.commit_gpu(staged.scene);
        self.runtime.plans.commit_gpu_admission(staged.plans);
        self.runtime.raster = Some(staged.raster);
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
        target_format: wgpu::TextureFormat,
    ) -> GpuPreparationStatus {
        match self.prepare_gpu(device, queue, target_format).await {
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
#[cfg(test)]
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

/// Encodes one complete Exact plan and invokes the single canonical raster in
/// a Renderer-owned encoder. A host may append capture/readback copies through
/// the returned transaction, but cannot replace or finish its command stream.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn encode_frame_gpu(
    slot: &mut PreparedRuntimeSlot,
    request: GpuFrameEncodeRequest<'_>,
) -> Result<PendingGpuFrame, FrameExecutionError> {
    let encode_attempt = slot
        .latest_encode_attempt
        .checked_add(1)
        .ok_or(FrameExecutionError::EncodeAttemptExhausted)?;
    // Every later attempt, including one that fails before producing a
    // pending frame, invalidates all older command streams before any queue
    // write or plan-local cache mutation can occur.
    slot.latest_encode_attempt = encode_attempt;
    let base_frame = slot.frame;
    let candidate_frame = slot
        .frame
        .candidate_for_frame(*request.camera, request.viewport)?;
    let owner = slot
        .gpu_owner
        .as_ref()
        .ok_or(GpuPreparationError::Unavailable)?;

    let PreparedRuntime {
        contract,
        scene,
        plans,
        raster,
    } = &mut slot.runtime;
    let raster = raster.as_ref().ok_or(GpuPreparationError::Unavailable)?;
    raster.validate_target_format(request.target_format)?;
    let mut encoder = owner
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gsplat-exact-frame-encoder"),
        });
    let work = plans.execute(
        request.requested,
        scene,
        PlanFrameInput::new(
            request.camera,
            candidate_frame.identity(),
            contract.source_count,
            request.viewport.width(),
            request.viewport.height(),
        ),
        PlanExecutionContext::Gpu(owner.context(owner.queue(), &mut encoder)?),
    )?;
    let (input, metadata) = canonical_input_for_work(&work, owner.token())?;
    raster.encode(
        &mut encoder,
        request.target,
        request.target_format,
        request.clear,
        input,
    )?;

    // End every scene/plan borrow before returning the owned transaction.
    drop(work);
    Ok(PendingGpuFrame {
        encoder,
        owner: owner.token().clone(),
        base_frame,
        candidate_frame,
        encode_attempt,
        metadata,
    })
}

/// Sole Exact shadow-core submission boundary. A stale/discarded encode cannot
/// publish its semantic frame, and only the latest successful encode attempt
/// may be submitted.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn submit_encoded_frame(
    slot: &mut PreparedRuntimeSlot,
    pending: PendingGpuFrame,
) -> Result<GpuFrameSubmission, FrameExecutionError> {
    let owner = slot
        .gpu_owner
        .as_ref()
        .ok_or(GpuPreparationError::Unavailable)?;
    if !pending.owner.same_owner(owner.token()) {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "GPU execution owner",
        });
    }
    if pending.base_frame != slot.frame {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "base frame",
        });
    }
    if pending.encode_attempt != slot.latest_encode_attempt {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "latest encode attempt",
        });
    }

    let command_buffer = pending.encoder.finish();
    let submission_index = owner.queue().submit(Some(command_buffer));
    slot.frame = pending.candidate_frame;
    Ok(GpuFrameSubmission {
        submission_index,
        frame: pending.metadata.frame,
        plan: pending.metadata.plan,
        order_lane: pending.metadata.order_lane,
        order_generation: pending.metadata.order_generation,
        source_count: pending.metadata.source_count,
        visible_count: pending.metadata.visible_count,
        contributor_count: pending.metadata.contributor_count,
        draw_count: pending.metadata.draw_count,
        count_semantics: pending.metadata.count_semantics,
        encode_attempt: pending.encode_attempt,
    })
}

fn canonical_input_for_work(
    work: &ProjectedWork<'_>,
    owner: &GpuOwnerToken,
) -> Result<(CanonicalRasterInput, FrameSubmissionMetadata), FrameExecutionError> {
    let (input, count_semantics) = match work.plan_id() {
        PlanId::CpuPostSort => {
            let gpu =
                work.cpu_post_gpu()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "CPU PostSort projected planes",
                    })?;
            let visible =
                work.visible_count()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "CPU PostSort visible count",
                    })?;
            let draw =
                work.draw_count()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "CPU PostSort draw count",
                    })?;
            if !gpu.same_owner(owner)
                || gpu.frame_identity() != work.frame_identity()
                || gpu.source_count() != work.source_count()
                || gpu.order_generation() != work.order_generation()
                || gpu.direct_count() != visible
                || draw != visible
                || gpu.count_semantics() != DirectCountSemantics::DrawEqualsVisible
                || work.contributor_count().is_ok()
            {
                return Err(FrameExecutionError::ProjectedWorkMismatch {
                    component: "CPU PostSort direct D=V contract",
                });
            }
            (
                CanonicalRasterInput::RankIndexedDirect {
                    instance_count: draw,
                },
                RasterCountSemantics::DirectDrawEqualsVisible,
            )
        }
        PlanId::GpuPostSort => {
            let gpu = work
                .gpu_post()
                .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                    component: "GPU PostSort projected planes",
                })?;
            if !gpu.same_owner(owner)
                || gpu.frame_identity() != work.frame_identity()
                || gpu.source_count() != work.source_count()
                || gpu.count_semantics() != IndirectCountSemantics::DrawEqualsVisible
                || work.visible_count().is_ok()
                || work.contributor_count().is_ok()
                || work.draw_count().is_ok()
            {
                return Err(FrameExecutionError::ProjectedWorkMismatch {
                    component: "GPU PostSort indirect D=V contract",
                });
            }
            (
                CanonicalRasterInput::RankIndexedIndirect,
                RasterCountSemantics::IndirectDrawEqualsVisible,
            )
        }
        PlanId::GpuPreproject => {
            let gpu =
                work.gpu_preproject()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "GPU Preproject projected planes",
                    })?;
            if !gpu.same_owner(owner)
                || gpu.frame_identity() != work.frame_identity()
                || gpu.source_count() != work.source_count()
                || gpu.count_semantics() != IndirectCountSemantics::DrawEqualsContributor
                || work.visible_count().is_ok()
                || work.contributor_count().is_ok()
                || work.draw_count().is_ok()
            {
                return Err(FrameExecutionError::ProjectedWorkMismatch {
                    component: "GPU Preproject indirect D=C contract",
                });
            }
            (
                CanonicalRasterInput::SourceIndexedIndirect,
                RasterCountSemantics::IndirectDrawEqualsContributor,
            )
        }
    };

    Ok((
        input,
        FrameSubmissionMetadata {
            frame: work.frame_identity(),
            plan: work.plan_id(),
            order_lane: work.order_lane(),
            order_generation: work.order_generation(),
            source_count: work.source_count(),
            visible_count: work.visible_count().ok(),
            contributor_count: work.contributor_count().ok(),
            draw_count: work.draw_count().ok(),
            count_semantics,
        },
    ))
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
