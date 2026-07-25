//! Shared Exact runtime preparation and frame dispatch.

mod controller;
mod current_stats;
pub(crate) mod frame;
pub(crate) mod gpu_prepare;
mod sampler;

use std::sync::Arc;

use gsplat_core::Camera;
use thiserror::Error;

use crate::evidence::{
    BoundedEvidenceRing, PlanComparisonKey, PlanCountSemantics, PlanSample, PlanSampleTicket,
};
use crate::plans::{
    DirectCountSemantics, FrameIdentity, GpuCapabilityReceipt, GpuOwnerToken,
    GpuPlanAdmissionRequest, HostCpuOrderReceipt, IndirectCountSemantics, OrderLane,
    PlanExecutionContext, PlanFrameInput, PlanId, PlanSet, PlanSetError, ProjectedWork,
    StagedGpuPlanAdmission,
};
use crate::raster::{CanonicalRaster, CanonicalRasterError, CanonicalRasterInput};
use crate::scene::{ResidentSceneCpu, ResidentSceneError, SceneRuntime};

use controller::{PlanDecision, SampleDisposition, WholePlanController, WholePlanControllerError};
use frame::{FrameState, GenerationError, Viewport};
use gpu_prepare::{
    CurrentStatsCapabilityCandidate, GpuExecutionOwner, GpuPreparationError, GpuPreparationReceipt,
    GpuScenePreparation,
};
use sampler::{PlanSampleDescriptor, PlanSampler, PlanSamplerError, StagedPlanSample};

pub(crate) use current_stats::{
    CurrentStatsCounts, CurrentStatsFailure, CurrentStatsJoinIdentity, CurrentStatsPoll,
    CurrentStatsReceipt, CurrentStatsRequest, CurrentStatsSubmission,
    CurrentStatsSubmissionReceipt, CurrentStatsTerminal, CurrentStatsTicket,
    CurrentStatsUnsampledReason,
};
use current_stats::{CurrentStatsFrameCounts, CurrentStatsVisibleSource};

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
    #[error("whole-plan controller failed: {0}")]
    Controller(#[from] WholePlanControllerError),
    #[error("mandatory whole-plan sampler failed: {0}")]
    Sampler(#[from] PlanSamplerError),
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

#[derive(Debug, Error)]
pub(crate) enum PreparedGpuRuntimeError {
    #[error("Exact runtime preparation failed: {0}")]
    Runtime(#[from] PreparedRuntimeError),
    #[error("Exact GPU runtime preparation failed: {0}")]
    Gpu(#[from] GpuRuntimePreparationError),
}

/// Result of optional device preparation. An omitted GPU candidate never
/// weakens or removes the already prepared Exact CPU fallback.
#[derive(Debug)]
pub(crate) enum GpuPreparationStatus {
    Ready(GpuPreparationReceipt),
    Omitted(GpuRuntimePreparationError),
}

pub(crate) type RasterCountSemantics = PlanCountSemantics;

/// Immutable identity for one complete Exact renderer submission. Numeric
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
    plan_sample_ticket: Option<PlanSampleTicket>,
    host_timings: Option<HostFrameTimings>,
    presentation_sequence: u64,
    current_stats: CurrentStatsSubmission,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HostFrameTimings {
    preprocess_ms: f32,
    sort_ms: f32,
    raster_ms: f32,
    frame_ms: f32,
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
    host_cpu_order: Option<HostCpuOrderReceipt>,
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
    decision: PlanDecision,
    base_sampler_ticket: Option<PlanSampleTicket>,
    reset_sampler_on_finalize: bool,
    staged_controller: WholePlanController,
    completion_started: crate::TimerInstant,
    staged_current_stats: Option<current_stats::StagedCurrentStats>,
    arm_formal_sample: bool,
}

impl PendingGpuFrame {
    /// Borrows the exact encoder that already contains plan and raster work.
    /// The encoder cannot be replaced or finished by the host, so the pending
    /// identity remains structurally tied to the submitted command stream.
    pub(crate) fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        &mut self.encoder
    }
}

/// Queue-submitted Exact frame whose semantic state is still unpublished.
///
/// A Surface host retains this token across primitive presentation. Dropping
/// or abandoning it leaves FrameState, controller progress, sampler ownership
/// and frame results untouched; any late completion callback can reach only
/// the orphaned atomics retained by wgpu.
pub(crate) struct SubmittedGpuFrame {
    state: Option<SubmittedGpuFrameState>,
}

/// Identity-checked submitted frame whose only remaining operation is the
/// infallible semantic commit. Holding the exclusive slot borrow across the
/// primitive presentation prevents any newer encode from invalidating the
/// checked transaction between validation and publication.
pub(crate) struct ValidatedSubmittedGpuFrame<'a> {
    slot: &'a mut PreparedRuntimeSlot,
    submitted: &'a mut SubmittedGpuFrame,
    presentation_sequence: u64,
}

struct SubmittedGpuFrameState {
    submission_index: wgpu::SubmissionIndex,
    owner: GpuOwnerToken,
    base_frame: FrameState,
    candidate_frame: FrameState,
    encode_attempt: u64,
    metadata: FrameSubmissionMetadata,
    decision: PlanDecision,
    base_sampler_ticket: Option<PlanSampleTicket>,
    reset_sampler_on_finalize: bool,
    staged_controller: WholePlanController,
    staged_sample: Option<StagedPlanSample>,
    plan_sample_ticket: Option<PlanSampleTicket>,
    completion_started: crate::TimerInstant,
    armed_current_stats: Option<current_stats::ArmedCurrentStats>,
}

impl SubmittedGpuFrame {
    /// This is only a wait primitive for tests/hosts. No semantic metadata or
    /// formal ticket escapes before successful target finalization.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn submission_index(&self) -> Option<&wgpu::SubmissionIndex> {
        self.state.as_ref().map(|state| &state.submission_index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactPlanPolicy {
    Forced(PlanId),
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactAdaptivePolicyState {
    Disabled,
    CpuLearning,
    CpuStable,
    GpuStable,
    GpuProbe,
    CpuProbe,
}

pub(crate) struct GpuFrameEncodeRequest<'a> {
    selection: ExactPlanPolicy,
    camera: &'a Camera,
    viewport: Viewport,
    target: &'a wgpu::TextureView,
    target_format: wgpu::TextureFormat,
    clear: wgpu::Color,
    force_cpu_order_refresh: bool,
    host_frame_started: Option<crate::TimerInstant>,
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
            selection: ExactPlanPolicy::Forced(requested),
            camera,
            viewport,
            target,
            target_format,
            clear,
            force_cpu_order_refresh: false,
            host_frame_started: None,
        }
    }

    pub(crate) const fn adaptive(
        camera: &'a Camera,
        viewport: Viewport,
        target: &'a wgpu::TextureView,
        target_format: wgpu::TextureFormat,
        clear: wgpu::Color,
    ) -> Self {
        Self {
            selection: ExactPlanPolicy::Adaptive,
            camera,
            viewport,
            target,
            target_format,
            clear,
            force_cpu_order_refresh: false,
            host_frame_started: None,
        }
    }

    pub(crate) const fn with_forced_cpu_order_refresh(mut self) -> Self {
        self.force_cpu_order_refresh = true;
        self
    }

    pub(crate) const fn with_host_frame_started(mut self, started: crate::TimerInstant) -> Self {
        self.host_frame_started = Some(started);
        self
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

    pub(crate) const fn plan_sample_ticket(&self) -> Option<PlanSampleTicket> {
        self.plan_sample_ticket
    }

    pub(crate) const fn host_timings(&self) -> Option<HostFrameTimings> {
        self.host_timings
    }

    pub(crate) const fn presentation_sequence(&self) -> u64 {
        self.presentation_sequence
    }

    pub(crate) const fn current_stats_submission(&self) -> CurrentStatsSubmission {
        self.current_stats
    }
}

impl HostFrameTimings {
    pub(crate) const fn preprocess_ms(self) -> f32 {
        self.preprocess_ms
    }

    pub(crate) const fn sort_ms(self) -> f32 {
        self.sort_ms
    }

    pub(crate) const fn raster_ms(self) -> f32 {
        self.raster_ms
    }

    pub(crate) const fn frame_ms(self) -> f32 {
        self.frame_ms
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

/// One fully validated Exact runtime candidate.
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

    fn prepare_surface_retained(
        source: &ResidentSceneCpu,
        frame: FrameState,
    ) -> Result<Self, PreparedRuntimeError> {
        let scene = SceneRuntime::prepare_surface_retained(source)?;
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

/// Renderer-owned publication slot shared by native Exact targets.
///
/// This is not a second product renderer: it owns only the prepared bundle and
/// its complete input identity. Any optional GPU graph stays under the sole
/// `SceneRuntime` owner and uses the caller's device. The slot owns no target
/// or presentation behavior; it does own the Exact runtime's sole whole-plan
/// controller, mandatory completion sampler, and optional evidence sink. It
/// publishes a semantic frame only through the single
/// `submit_encoded_frame` boundary.
pub(crate) struct PreparedRuntimeSlot {
    runtime: PreparedRuntime,
    frame: FrameState,
    gpu_owner: Option<GpuExecutionOwner>,
    latest_encode_attempt: u64,
    presentation_sequence: u64,
    controller: WholePlanController,
    sampler: PlanSampler,
    optional_plan_evidence: Option<BoundedEvidenceRing<PlanSample>>,
    active_policy: ExactPlanPolicy,
    force_cpu_order_refresh: bool,
    last_published_plan: Option<PlanId>,
    #[cfg(test)]
    current_stats_capability_test_failure: Option<gpu_prepare::CurrentStatsCapabilityTestFailure>,
    #[cfg(test)]
    current_stats_capability_last_error: Option<GpuPreparationError>,
}

struct StagedGpuRuntimeAdmission {
    scene: GpuScenePreparation,
    plans: StagedGpuPlanAdmission,
    raster: CanonicalRaster,
    frame: FrameState,
    owner: GpuExecutionOwner,
    current_stats_capability: Option<CurrentStatsCapabilityCandidate>,
    current_stats_unsampled: Option<CurrentStatsUnsampledReason>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompleteGpuCandidateTestFailure {
    GpuResource,
    Plan,
    Raster,
}

impl PreparedRuntimeSlot {
    pub(crate) fn prepare(resident: ResidentSceneCpu) -> Result<Self, PreparedRuntimeError> {
        Self::prepare_at_frame(resident, FrameState::initial())
    }

    fn prepare_at_frame(
        resident: ResidentSceneCpu,
        frame: FrameState,
    ) -> Result<Self, PreparedRuntimeError> {
        let runtime = PreparedRuntime::prepare(resident, frame)?;
        let controller = WholePlanController::new(
            runtime.plans.fallback(),
            OrderLane::Cpu,
            runtime.plans.eligible(),
            comparison_key(&runtime, frame.identity()),
        );
        Ok(Self {
            runtime,
            frame,
            gpu_owner: None,
            latest_encode_attempt: 0,
            presentation_sequence: 0,
            controller,
            sampler: PlanSampler::new(),
            optional_plan_evidence: None,
            active_policy: ExactPlanPolicy::Forced(PlanId::CpuPostSort),
            force_cpu_order_refresh: false,
            last_published_plan: None,
            #[cfg(test)]
            current_stats_capability_test_failure: None,
            #[cfg(test)]
            current_stats_capability_last_error: None,
        })
    }

    /// Builds the durable CPU half of a Surface candidate by sharing only the
    /// exact position allocation. Upload staging stays with the unpublished
    /// renderer source until complete GPU preparation succeeds.
    pub(crate) fn prepare_surface_candidate(
        source: &ResidentSceneCpu,
    ) -> Result<Self, PreparedRuntimeError> {
        let frame = FrameState::initial();
        let runtime = PreparedRuntime::prepare_surface_retained(source, frame)?;
        let controller = WholePlanController::new(
            runtime.plans.fallback(),
            OrderLane::Cpu,
            runtime.plans.eligible(),
            comparison_key(&runtime, frame.identity()),
        );
        Ok(Self {
            runtime,
            frame,
            gpu_owner: None,
            latest_encode_attempt: 0,
            presentation_sequence: 0,
            controller,
            sampler: PlanSampler::new(),
            optional_plan_evidence: None,
            active_policy: ExactPlanPolicy::Forced(PlanId::CpuPostSort),
            force_cpu_order_refresh: false,
            last_published_plan: None,
            #[cfg(test)]
            current_stats_capability_test_failure: None,
            #[cfg(test)]
            current_stats_capability_last_error: None,
        })
    }

    pub(crate) async fn prepare_complete_surface_gpu_candidate(
        source: &ResidentSceneCpu,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        target_format: wgpu::TextureFormat,
        indirect_execution_supported: bool,
    ) -> Result<Self, PreparedGpuRuntimeError> {
        let mut candidate = Self::prepare_surface_candidate(source)?;
        candidate
            .prepare_gpu_from_source(
                source,
                device,
                queue,
                target_format,
                indirect_execution_supported,
            )
            .await?;
        Ok(candidate)
    }

    /// Builds a complete unpublished replacement against the existing
    /// offscreen owner. The prior slot is borrowed only for its next semantic
    /// generation; failure leaves it and every published renderer field
    /// untouched.
    pub(crate) async fn prepare_complete_gpu_candidate(
        resident: ResidentSceneCpu,
        previous: Option<&Self>,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, PreparedGpuRuntimeError> {
        let frame = match previous {
            Some(previous) => previous
                .frame
                .after_runtime_replacement()
                .map_err(PreparedRuntimeError::Generation)?,
            None => FrameState::initial(),
        };
        let mut candidate = Self::prepare_at_frame(resident, frame)?;
        if let Some(previous) = previous {
            candidate.presentation_sequence = previous.presentation_sequence;
            candidate
                .sampler
                .import_current_stats_handoff(previous.sampler.current_stats_replacement_handoff());
        }
        candidate.prepare_gpu(device, queue, target_format).await?;
        Ok(candidate)
    }

    #[cfg(test)]
    pub(crate) async fn prepare_complete_gpu_candidate_with_test_failure(
        resident: ResidentSceneCpu,
        previous: Option<&Self>,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        target_format: wgpu::TextureFormat,
        failure: CompleteGpuCandidateTestFailure,
    ) -> Result<Self, PreparedGpuRuntimeError> {
        let frame = match previous {
            Some(previous) => previous
                .frame
                .after_runtime_replacement()
                .map_err(PreparedRuntimeError::Generation)?,
            None => FrameState::initial(),
        };
        let mut candidate = Self::prepare_at_frame(resident, frame)?;
        match failure {
            CompleteGpuCandidateTestFailure::GpuResource => {
                return Err(
                    GpuRuntimePreparationError::Resources(GpuPreparationError::Internal(
                        "injected complete GPU resource failure".into(),
                    ))
                    .into(),
                );
            }
            CompleteGpuCandidateTestFailure::Plan => {
                candidate.set_test_gpu_admission_mode(crate::plans::TestGpuAdmissionMode::Fail);
            }
            CompleteGpuCandidateTestFailure::Raster => {
                candidate
                    .set_test_gpu_admission_mode(crate::plans::TestGpuAdmissionMode::ConcreteAll);
            }
        }
        let format = match failure {
            CompleteGpuCandidateTestFailure::Raster => wgpu::TextureFormat::Depth32Float,
            CompleteGpuCandidateTestFailure::GpuResource
            | CompleteGpuCandidateTestFailure::Plan => target_format,
        };
        candidate.prepare_gpu(device, queue, format).await?;
        unreachable!("every injected complete-candidate stage must fail")
    }

    pub(crate) fn replace(
        &mut self,
        resident: ResidentSceneCpu,
    ) -> Result<(), PreparedRuntimeError> {
        let next_frame = self.frame.after_runtime_replacement()?;
        let next_runtime = PreparedRuntime::prepare(resident, next_frame)?;
        let controller = WholePlanController::new(
            next_runtime.plans.fallback(),
            OrderLane::Cpu,
            next_runtime.plans.eligible(),
            comparison_key(&next_runtime, next_frame.identity()),
        );
        let current_stats_handoff = self.sampler.current_stats_replacement_handoff();
        let candidate = Self {
            runtime: next_runtime,
            frame: next_frame,
            gpu_owner: None,
            latest_encode_attempt: 0,
            presentation_sequence: self.presentation_sequence,
            controller,
            sampler: PlanSampler::new(),
            optional_plan_evidence: None,
            active_policy: ExactPlanPolicy::Forced(PlanId::CpuPostSort),
            force_cpu_order_refresh: false,
            last_published_plan: None,
            #[cfg(test)]
            current_stats_capability_test_failure: None,
            #[cfg(test)]
            current_stats_capability_last_error: None,
        };
        let mut candidate = candidate;
        candidate
            .sampler
            .import_current_stats_handoff(current_stats_handoff);
        *self = candidate;
        Ok(())
    }

    /// Prepares the complete dormant GPU scene/project graph against the
    /// caller's existing device/queue pair. No adapter or second device is
    /// acquired; Arc identity keeps later queue borrows tied to this owner.
    /// Publication into `SceneRuntime` is one infallible commit after every
    /// product resource constructor and asynchronous device error scope
    /// succeeds. Optional observer-resource failure resolves only that
    /// pre-ticket request and cannot reject Scene/PlanSet/Raster admission.
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
        #[cfg(test)]
        let current_stats_candidate = scene
            .stage_current_stats_capability_with_test_failure(
                &owner,
                self.current_stats_capability_test_failure,
            )
            .await;
        #[cfg(not(test))]
        let current_stats_candidate = scene.stage_current_stats_capability(&owner).await;
        let (current_stats_capability, current_stats_unsampled) = match current_stats_candidate {
            Ok(candidate) => (Some(candidate), None),
            Err(_error) => {
                #[cfg(test)]
                {
                    self.current_stats_capability_last_error = Some(_error.clone());
                }
                (
                    None,
                    self.sampler
                        .has_current_stats_request()
                        .then_some(CurrentStatsUnsampledReason::ResourceUnavailable),
                )
            }
        };
        let receipt = scene.receipt();
        let raster = scene
            .prepare_canonical_raster(&owner, target_format)
            .await?;
        let plans = self.runtime.plans.stage_gpu_admission(
            GpuPlanAdmissionRequest::new(
                receipt.source_count(),
                receipt.capacity(),
                receipt.resident_count(),
                receipt.addressable_count(),
                receipt.sh_degree(),
                receipt.scene_generation(),
                receipt.contract_generation(),
                previous_frame.plan_set_generation(),
                receipt.plan_set_generation(),
            ),
            true,
        )?;
        self.commit_gpu_admission(StagedGpuRuntimeAdmission {
            scene,
            plans,
            raster,
            frame: next_frame,
            owner,
            current_stats_capability,
            current_stats_unsampled,
        });
        Ok(receipt)
    }

    /// Surface preparation variant that uploads from the live unpublished
    /// renderer source. Optional current-stats resources remain observer-only:
    /// their failure cannot reject the complete Scene/PlanSet/Raster graph.
    pub(crate) async fn prepare_gpu_from_source(
        &mut self,
        source: &ResidentSceneCpu,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        target_format: wgpu::TextureFormat,
        indirect_execution_supported: bool,
    ) -> Result<GpuPreparationReceipt, GpuRuntimePreparationError> {
        if self.gpu_owner.is_some() {
            return Err(GpuPreparationError::ExecutionOwnerAlreadyBound.into());
        }
        if !self.runtime.scene.has_same_surface_source(source) {
            return Err(GpuPreparationError::Internal(
                "Surface Exact source identity changed before GPU preparation".into(),
            )
            .into());
        }
        let previous_frame = self.frame.identity();
        let next_frame = self.frame.candidate_for_plan_set_admission()?;
        let owner = GpuExecutionOwner::new(device, queue);
        let scene = self
            .runtime
            .scene
            .stage_gpu_from(
                &owner,
                source,
                next_frame.identity(),
                indirect_execution_supported,
            )
            .await?;
        #[cfg(test)]
        let current_stats_candidate = scene
            .stage_current_stats_capability_with_test_failure(
                &owner,
                self.current_stats_capability_test_failure,
            )
            .await;
        #[cfg(not(test))]
        let current_stats_candidate = scene.stage_current_stats_capability(&owner).await;
        let (current_stats_capability, current_stats_unsampled) = match current_stats_candidate {
            Ok(candidate) => (Some(candidate), None),
            Err(_error) => {
                #[cfg(test)]
                {
                    self.current_stats_capability_last_error = Some(_error.clone());
                }
                (
                    None,
                    self.sampler
                        .has_current_stats_request()
                        .then_some(CurrentStatsUnsampledReason::ResourceUnavailable),
                )
            }
        };
        let receipt = scene.receipt();
        let raster = scene
            .prepare_canonical_raster(&owner, target_format)
            .await?;
        let plans = self.runtime.plans.stage_gpu_admission(
            GpuPlanAdmissionRequest::new(
                receipt.source_count(),
                receipt.capacity(),
                receipt.resident_count(),
                receipt.addressable_count(),
                receipt.sh_degree(),
                receipt.scene_generation(),
                receipt.contract_generation(),
                previous_frame.plan_set_generation(),
                receipt.plan_set_generation(),
            ),
            receipt.preproject_compute(),
        )?;
        self.commit_gpu_admission(StagedGpuRuntimeAdmission {
            scene,
            plans,
            raster,
            frame: next_frame,
            owner,
            current_stats_capability,
            current_stats_unsampled,
        });
        Ok(receipt)
    }

    fn commit_gpu_admission(&mut self, mut staged: StagedGpuRuntimeAdmission) {
        if let Some(capability) = staged.current_stats_capability {
            let (scan, readback_pool) = capability.into_parts();
            staged.scene.install_current_stats_contributor_scan(scan);
            self.sampler.install_current_stats_capability(readback_pool);
        }
        self.runtime.scene.commit_gpu(staged.scene);
        self.runtime.plans.commit_gpu_admission(staged.plans);
        self.runtime.raster = Some(staged.raster);
        self.frame = staged.frame;
        self.gpu_owner = Some(staged.owner);
        self.reset_plan_policy_for_current_runtime();
        if let Some(reason) = staged.current_stats_unsampled {
            let resolved = self.sampler.resolve_current_stats_request_unsampled(reason);
            debug_assert!(resolved);
        }
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

    pub(crate) const fn active_policy(&self) -> ExactPlanPolicy {
        self.active_policy
    }

    pub(crate) fn seed_surface_frame_baseline(&mut self, camera: Camera, viewport: Viewport) {
        debug_assert_eq!(self.presentation_sequence, 0);
        debug_assert!(self.last_published_plan.is_none());
        self.frame = self.frame.with_surface_baseline(camera, viewport);
    }

    pub(crate) fn set_active_policy(&mut self, policy: ExactPlanPolicy) {
        self.active_policy = policy;
    }

    pub(crate) fn adaptive_policy_state(&self) -> ExactAdaptivePolicyState {
        match self.active_policy {
            ExactPlanPolicy::Forced(_) => ExactAdaptivePolicyState::Disabled,
            ExactPlanPolicy::Adaptive => self.controller.adaptive_state(),
        }
    }

    pub(crate) const fn cpu_order_refresh_requested(&self) -> bool {
        self.force_cpu_order_refresh
    }

    pub(crate) fn request_cpu_order_refresh(&mut self) {
        self.force_cpu_order_refresh = true;
        self.sampler.require_fresh_cpu_order_for_current_stats();
    }

    /// Restarts only whole-plan performance learning for a changed Surface
    /// queue/backpressure context. Semantic generations, active policy,
    /// presentation identity, current-stats work and prepared plan resources
    /// remain owned by this same runtime.
    pub(crate) fn reset_surface_performance_learning(&mut self) {
        self.sampler.invalidate_performance_context();
        self.controller.reset_performance_learning();
    }

    pub(crate) const fn last_published_plan(&self) -> Option<PlanId> {
        self.last_published_plan
    }

    pub(crate) fn eligible(&self) -> &[PlanId] {
        self.runtime.plans.eligible()
    }

    pub(crate) fn plan_is_eligible(&self, plan: PlanId) -> bool {
        self.runtime.plans.eligible().contains(&plan)
    }

    pub(crate) fn scene(&self) -> &SceneRuntime {
        &self.runtime.scene
    }

    pub(crate) fn has_same_surface_source(&self, source: &ResidentSceneCpu) -> bool {
        if !self.runtime.scene.has_same_surface_source(source)
            || self.runtime.contract.source_count as usize != source.len()
            || self.runtime.contract.sh_degree != source.sh_degree
        {
            return false;
        }
        self.gpu_preparation().is_some_and(|receipt| {
            receipt.source_count() as usize == source.len()
                && receipt.capacity() == receipt.source_count()
                && receipt.resident_count() == receipt.source_count()
                && receipt.addressable_count() == receipt.source_count()
                && receipt.sh_degree() == source.sh_degree
        })
    }

    pub(crate) fn gpu_preparation(&self) -> Option<GpuPreparationReceipt> {
        self.runtime.scene.gpu_preparation()
    }

    pub(crate) fn gpu_capability(&self) -> Option<GpuCapabilityReceipt> {
        self.runtime.plans.gpu_capability()
    }

    /// Requests one observer receipt from an eligible Exact frame. The request
    /// is private and one-shot; a pending request may receive one bounded turn
    /// before a formal sample, after which formal work takes priority across a
    /// proved queue-safe boundary. No formal frame carries observer GPU work.
    pub(crate) fn request_current_stats(&mut self) -> CurrentStatsRequest {
        if self.gpu_owner.is_none() {
            return CurrentStatsRequest::Unsampled(CurrentStatsUnsampledReason::GpuUnavailable);
        }
        self.sampler
            .request_current_stats(self.force_cpu_order_refresh)
    }

    /// Non-blocking observer poll. It performs `Poll` only when this lane has
    /// an explicitly requested submitted map or queued terminal.
    pub(crate) fn poll_current_stats(&mut self) -> CurrentStatsPoll {
        self.sampler
            .poll_current_stats(self.gpu_owner.as_ref().map(GpuExecutionOwner::device))
    }

    pub(crate) fn expire_current_stats(&mut self, ticket: CurrentStatsTicket) -> bool {
        self.sampler.expire_current_stats(ticket)
    }

    #[cfg(test)]
    pub(crate) fn set_test_gpu_admission_mode(&mut self, mode: crate::plans::TestGpuAdmissionMode) {
        self.runtime.plans.set_test_gpu_admission_mode(mode);
    }

    #[cfg(test)]
    fn set_current_stats_capability_failure_for_test(
        &mut self,
        failure: gpu_prepare::CurrentStatsCapabilityTestFailure,
    ) {
        self.current_stats_capability_test_failure = Some(failure);
    }

    #[cfg(test)]
    fn current_stats_capability_last_error_for_test(&self) -> Option<&GpuPreparationError> {
        self.current_stats_capability_last_error.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn test_gpu_post_capability(&self) -> Option<GpuCapabilityReceipt> {
        self.runtime.plans.test_gpu_post_capability()
    }

    #[cfg(test)]
    pub(crate) fn indirect_execution_resources_prepared_for_test(&self) -> Option<bool> {
        self.runtime.scene.indirect_execution_resources_prepared()
    }

    #[cfg(test)]
    pub(crate) fn complete_adaptive_cpu_frame_for_test(
        &mut self,
    ) -> Result<(PlanId, bool), WholePlanControllerError> {
        let decision = self.controller.choose_adaptive()?;
        let formal = decision.formal_kind().is_some();
        if decision.plan() == PlanId::CpuPostSort {
            self.controller
                .submitted_without_sample(decision, OrderLane::Cpu);
        }
        Ok((decision.plan(), formal))
    }

    #[cfg(test)]
    pub(crate) fn adaptive_probe_generation_for_test(&self) -> u64 {
        self.controller.probe_generation_for_test()
    }

    pub(crate) fn last_usable_cpu_order(&self) -> Option<&[u32]> {
        self.runtime.plans.last_usable_cpu_order()
    }

    #[cfg(test)]
    pub(crate) fn current_cpu_order_generation(&self) -> Option<u64> {
        self.runtime.plans.current_cpu_order_generation()
    }

    #[cfg(test)]
    pub(crate) fn same_gpu_arc_owner(
        &self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
    ) -> bool {
        self.gpu_owner
            .as_ref()
            .is_some_and(|owner| owner.same_arc_owner(device, queue))
    }

    fn reset_plan_policy_for_current_runtime(&mut self) {
        self.sampler.invalidate();
        let key = comparison_key(&self.runtime, self.frame.identity());
        let _ = self.controller.synchronize(
            self.runtime.plans.fallback(),
            OrderLane::Cpu,
            self.runtime.plans.eligible(),
            key,
        );
    }

    fn poll_plan_sampler(&mut self) -> Option<(PlanSample, SampleDisposition)> {
        let owner = self.gpu_owner.as_ref()?;
        let sample = self.sampler.poll(owner.device())?;
        // Mandatory policy consumes the immutable sample before any optional
        // observer can drop or overwrite its copy.
        let disposition = self.controller.observe(sample);
        if let Some(ring) = &mut self.optional_plan_evidence {
            ring.push(sample);
        }
        Some((sample, disposition))
    }

    #[cfg(test)]
    fn set_test_controller_config(&mut self, config: controller::ControllerConfig) {
        self.sampler.invalidate();
        self.controller.set_config_for_test(config);
    }

    #[cfg(test)]
    fn enable_test_plan_evidence(&mut self) {
        self.optional_plan_evidence = Some(BoundedEvidenceRing::new());
    }

    #[cfg(test)]
    fn drain_test_plan_evidence(&mut self) -> Vec<PlanSample> {
        self.optional_plan_evidence
            .as_mut()
            .map(|ring| ring.drain().collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn poll_test_plan_sampler(&mut self) -> Option<(PlanSample, SampleDisposition)> {
        self.poll_plan_sampler()
    }

    #[cfg(test)]
    const fn sampler_pending_for_test(&self) -> bool {
        self.sampler.has_pending()
    }

    #[cfg(test)]
    const fn retired_performance_ticket_for_test(&self) -> Option<PlanSampleTicket> {
        self.sampler.retired_performance_ticket_for_test()
    }

    #[cfg(test)]
    const fn presentation_sequence_for_test(&self) -> u64 {
        self.presentation_sequence
    }

    #[cfg(test)]
    const fn current_stats_copy_count_for_test(&self) -> u64 {
        self.sampler.current_stats_copy_count_for_test()
    }

    #[cfg(test)]
    pub(crate) const fn current_stats_request_pending_for_test(&self) -> bool {
        self.sampler.current_stats_request_pending_for_test()
    }

    #[cfg(test)]
    pub(crate) const fn current_stats_fresh_cpu_order_required_for_test(&self) -> bool {
        self.sampler
            .current_stats_fresh_cpu_order_required_for_test()
    }

    #[cfg(test)]
    fn current_stats_resource_bytes_for_test(&self) -> (Option<u64>, u64) {
        (
            self.runtime.scene.current_stats_resource_bytes(),
            self.sampler.current_stats_readback_bytes_for_test(),
        )
    }

    #[cfg(test)]
    fn current_stats_live_object_count_for_test(&self) -> Option<usize> {
        self.runtime
            .scene
            .current_stats_live_object_count()
            .map(|scan_objects| scan_objects + 4)
    }

    #[cfg(test)]
    const fn current_stats_observer_activity_for_test(&self) -> (u64, u64, u64, u64) {
        (
            self.sampler.current_stats_observer_encode_count_for_test(),
            self.sampler.current_stats_copy_count_for_test(),
            self.sampler.current_stats_map_arm_count_for_test(),
            self.sampler.current_stats_device_poll_count_for_test(),
        )
    }

    #[cfg(test)]
    fn force_current_stats_map_failure_for_test(&mut self, ticket: CurrentStatsTicket) -> bool {
        self.sampler
            .force_current_stats_map_failure_for_test(ticket)
    }

    #[cfg(test)]
    fn hold_current_stats_callback_for_test(&mut self, ticket: CurrentStatsTicket) -> bool {
        self.sampler.hold_current_stats_callback_for_test(ticket)
    }

    #[cfg(test)]
    fn poll_current_stats_without_device_for_test(&mut self) -> CurrentStatsPoll {
        self.sampler.poll_current_stats_without_device_for_test()
    }
}

fn comparison_key(runtime: &PreparedRuntime, frame: FrameIdentity) -> PlanComparisonKey {
    PlanComparisonKey::new(
        frame,
        runtime.contract.source_count,
        runtime.contract.sh_degree,
    )
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
    // Advancing the attempt logically invalidates every unresolved target
    // token before polling the sole live sampler. An unpresented token owns
    // its callback outside the slot, so a fast completion can never be
    // mistaken for presented-frame evidence.
    let formal_queue_safe_at_frame_entry = slot.sampler.formal_queue_safe_at_frame_entry();
    let _ = slot.poll_plan_sampler();
    let base_frame = slot.frame;
    let base_sampler_ticket = slot.sampler.pending_ticket();
    let candidate_frame = slot
        .frame
        .candidate_for_frame(*request.camera, request.viewport)?;
    let mut staged_controller = slot.controller.clone();
    let reset_sampler_on_finalize = staged_controller.synchronize(
        slot.runtime.plans.fallback(),
        OrderLane::Cpu,
        slot.runtime.plans.eligible(),
        comparison_key(&slot.runtime, candidate_frame.identity()),
    );
    let decision = match request.selection {
        ExactPlanPolicy::Forced(plan) => staged_controller.choose_forced(plan),
        ExactPlanPolicy::Adaptive => staged_controller.choose_adaptive()?,
    };
    let (arm_formal_sample, encode_current_stats) = if decision.formal_kind().is_some() {
        if !formal_queue_safe_at_frame_entry {
            // Even when the mandatory non-blocking poll above observes
            // completion, this frame began while the observer was still
            // queue-unsafe. Retry the same formal decision on a later frame
            // whose entry is known safe.
            (false, false)
        } else if slot.sampler.should_yield_formal_to_current_stats() {
            // One pending request receives a bounded turn. Its submission
            // establishes a barrier, so the retried formal sample cannot arm
            // until a later frame begins queue-safe.
            (false, true)
        } else {
            (true, false)
        }
    } else {
        (false, true)
    };
    let completion_started = request.host_frame_started.unwrap_or_else(crate::timer_now);

    let encoded = (|| {
        let owner = slot
            .gpu_owner
            .as_ref()
            .ok_or(GpuPreparationError::Unavailable)?;
        let (runtime, sampler) = (&mut slot.runtime, &mut slot.sampler);
        let PreparedRuntime {
            contract,
            scene,
            plans,
            raster,
        } = runtime;
        let raster = raster.as_ref().ok_or(GpuPreparationError::Unavailable)?;
        raster.validate_target_format(request.target_format)?;
        let mut encoder = owner
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gsplat-exact-frame-encoder"),
            });
        // A moving formal frame binds freshness to its observer request, not
        // to whichever controller or queue-boundary presentation happened to
        // run first. Static requests leave this false and retain normal cache
        // reuse. The request-owned bit survives presentation deferral and is
        // cleared only with request issuance or an unsampled resolution.
        let force_cpu_order_refresh = request.force_cpu_order_refresh
            || (encode_current_stats && sampler.current_stats_requires_fresh_cpu_order());
        let mut input = PlanFrameInput::new(
            request.camera,
            candidate_frame.identity(),
            contract.source_count,
            request.viewport.width(),
            request.viewport.height(),
        );
        if force_cpu_order_refresh {
            input = input.with_forced_cpu_order_refresh();
        }
        let work = plans.execute(
            decision.plan(),
            scene,
            input,
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
        // A formal sample never shares a command stream with observer work.
        // The scheduling decision above also protects it from observer work
        // left in the same queue by an earlier submission.
        let staged_current_stats =
            if encode_current_stats && sampler.prepare_current_stats_encode(owner.device()) {
                let counts = current_stats_counts_for_work(&work, &mut encoder)?;
                sampler.encode_current_stats(&mut encoder, counts)
            } else {
                None
            };
        drop(work);
        Ok::<_, FrameExecutionError>((encoder, metadata, staged_current_stats))
    })();
    let (encoder, metadata, staged_current_stats) = match encoded {
        Ok(encoded) => encoded,
        Err(error) => {
            // A genuine plan/encode failure under the still-published
            // comparison key must retain the existing adaptive cooldown
            // behavior. A failure while staging a different comparison key
            // is not allowed to replace the old controller or sampler.
            if !reset_sampler_on_finalize {
                staged_controller.execution_failed(decision);
                slot.controller = staged_controller;
            }
            return Err(error);
        }
    };
    let pending_owner = slot
        .gpu_owner
        .as_ref()
        .ok_or(GpuPreparationError::Unavailable)?
        .token()
        .clone();
    Ok(PendingGpuFrame {
        encoder,
        owner: pending_owner,
        base_frame,
        candidate_frame,
        encode_attempt,
        metadata,
        decision,
        base_sampler_ticket,
        reset_sampler_on_finalize,
        staged_controller,
        completion_started,
        staged_current_stats,
        arm_formal_sample,
    })
}

/// Sole Exact renderer submission boundary. A stale/discarded encode cannot
/// publish its semantic frame, and only the latest successful encode attempt
/// may be submitted.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn submit_encoded_frame(
    slot: &mut PreparedRuntimeSlot,
    pending: PendingGpuFrame,
) -> Result<GpuFrameSubmission, FrameExecutionError> {
    let mut submitted = submit_pending_frame(slot, pending, std::iter::empty())?;
    finalize_submitted_frame(slot, &mut submitted)
}

/// Finite E10 control path: keep one queue submission while placing a caller
/// copy in a second command buffer after the exact render command buffer.
#[cfg(test)]
pub(crate) fn submit_encoded_frame_with_followup_for_test(
    slot: &mut PreparedRuntimeSlot,
    pending: PendingGpuFrame,
    followup: wgpu::CommandBuffer,
) -> Result<GpuFrameSubmission, FrameExecutionError> {
    let mut submitted = submit_pending_frame(slot, pending, std::iter::once(followup))?;
    finalize_submitted_frame(slot, &mut submitted)
}

fn submit_pending_frame(
    slot: &mut PreparedRuntimeSlot,
    pending: PendingGpuFrame,
    followups: impl IntoIterator<Item = wgpu::CommandBuffer>,
) -> Result<SubmittedGpuFrame, FrameExecutionError> {
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
    if pending.decision.plan() != pending.metadata.plan
        || pending.decision.comparison() != comparison_key(&slot.runtime, pending.metadata.frame)
        || !pending
            .staged_controller
            .accepts_execution_lane(pending.decision, pending.metadata.order_lane)
    {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "whole-plan decision identity",
        });
    }

    let queue = Arc::clone(owner.queue());
    let mut command_buffers = Vec::with_capacity(2);
    command_buffers.push(pending.encoder.finish());
    command_buffers.extend(followups);
    let terminal_index = command_buffers.len() - 1;
    let mut staged_controller = pending.staged_controller;
    let mut staged_sample = None;
    let mut plan_sample_ticket = None;
    if pending.arm_formal_sample {
        if !staged_controller.can_arm(pending.decision) {
            return Err(FrameExecutionError::PendingFrameMismatch {
                component: "formal whole-plan sample",
            });
        }
        let sample = slot.sampler.stage_arm(
            &command_buffers[terminal_index],
            PlanSampleDescriptor {
                probe_generation: pending.decision.probe_generation(),
                comparison: pending.decision.comparison(),
                frame: pending.metadata.frame,
                plan: pending.metadata.plan,
                order_lane: pending.metadata.order_lane,
                order_generation: pending.metadata.order_generation,
                visible_count: pending.metadata.visible_count,
                contributor_count: pending.metadata.contributor_count,
                draw_count: pending.metadata.draw_count,
                count_semantics: pending.metadata.count_semantics,
            },
            pending.completion_started,
        )?;
        let ticket = sample.ticket();
        if !staged_controller.register_pending(
            pending.decision,
            ticket,
            pending.metadata.order_lane,
        ) {
            return Err(FrameExecutionError::PendingFrameMismatch {
                component: "formal whole-plan ticket",
            });
        }
        staged_sample = Some(sample);
        plan_sample_ticket = Some(ticket);
    }
    // All fallible formal-lane staging is complete before the observer map is
    // armed. From here queue submission is the only operation, so an error
    // cannot strand an unpublished observer slot waiting on an unsubmitted
    // callback.
    let armed_current_stats = pending.staged_current_stats.map(|staged| {
        slot.sampler
            .arm_current_stats(&command_buffers[terminal_index], staged)
    });

    let submission_index = queue.submit(command_buffers);
    Ok(SubmittedGpuFrame {
        state: Some(SubmittedGpuFrameState {
            submission_index,
            owner: pending.owner,
            base_frame: pending.base_frame,
            candidate_frame: pending.candidate_frame,
            encode_attempt: pending.encode_attempt,
            metadata: pending.metadata,
            decision: pending.decision,
            base_sampler_ticket: pending.base_sampler_ticket,
            reset_sampler_on_finalize: pending.reset_sampler_on_finalize,
            staged_controller,
            staged_sample,
            plan_sample_ticket,
            completion_started: pending.completion_started,
            armed_current_stats,
        }),
    })
}

/// Submits one Exact frame while retaining all semantic publication in the
/// returned token. Surface hosts finalize it only after actual presentation.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn submit_encoded_frame_unpublished(
    slot: &mut PreparedRuntimeSlot,
    pending: PendingGpuFrame,
) -> Result<SubmittedGpuFrame, FrameExecutionError> {
    submit_pending_frame(slot, pending, std::iter::empty())
}

/// Publishes a queue-submitted frame exactly once after the primitive target
/// outcome is known successful. Every fallible identity check occurs before
/// the infallible FrameState/controller/sampler commit.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn finalize_submitted_frame(
    slot: &mut PreparedRuntimeSlot,
    submitted: &mut SubmittedGpuFrame,
) -> Result<GpuFrameSubmission, FrameExecutionError> {
    Ok(validate_submitted_frame(slot, submitted)?.publish())
}

/// Performs every fallible identity check before a Surface host presents.
/// The returned guard keeps both semantic owners exclusively borrowed; once
/// the primitive target is presented its `publish` operation cannot fail.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn validate_submitted_frame<'a>(
    slot: &'a mut PreparedRuntimeSlot,
    submitted: &'a mut SubmittedGpuFrame,
) -> Result<ValidatedSubmittedGpuFrame<'a>, FrameExecutionError> {
    let state = submitted
        .state
        .as_ref()
        .ok_or(FrameExecutionError::PendingFrameMismatch {
            component: "submitted target token",
        })?;
    let owner = slot
        .gpu_owner
        .as_ref()
        .ok_or(GpuPreparationError::Unavailable)?;
    if !state.owner.same_owner(owner.token()) {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "GPU execution owner",
        });
    }
    if state.base_frame != slot.frame {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "base frame",
        });
    }
    if state.encode_attempt != slot.latest_encode_attempt {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "latest encode attempt",
        });
    }
    if slot.sampler.pending_ticket() != state.base_sampler_ticket {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "mandatory sampler base",
        });
    }
    if state.candidate_frame.identity() != state.metadata.frame
        || state.decision.plan() != state.metadata.plan
        || state.decision.comparison() != comparison_key(&slot.runtime, state.metadata.frame)
    {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "submitted frame identity",
        });
    }
    if state.staged_sample.is_some()
        && !state.reset_sampler_on_finalize
        && slot.sampler.has_pending()
    {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "mandatory sampler commit",
        });
    }
    if let Some(armed) = state.armed_current_stats.as_ref()
        && !slot.sampler.accepts_current_stats_armed(
            armed,
            state.metadata.frame,
            state.metadata.plan,
        )
    {
        return Err(FrameExecutionError::PendingFrameMismatch {
            component: "current-stats observer ticket",
        });
    }

    let presentation_sequence = slot.presentation_sequence.checked_add(1).ok_or(
        FrameExecutionError::PendingFrameMismatch {
            component: "presentation sequence",
        },
    )?;

    Ok(ValidatedSubmittedGpuFrame {
        slot,
        submitted,
        presentation_sequence,
    })
}

impl ValidatedSubmittedGpuFrame<'_> {
    /// Commits FrameState, controller progress and formal sampler ownership.
    /// Validation and the exclusive borrow make this operation infallible.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn publish(self) -> GpuFrameSubmission {
        let mut state = self
            .submitted
            .state
            .take()
            .expect("validated submitted target token");
        if state.plan_sample_ticket.is_none() {
            state
                .staged_controller
                .submitted_without_sample(state.decision, state.metadata.order_lane);
        }
        if state.reset_sampler_on_finalize {
            self.slot.sampler.invalidate();
        }
        if let Some(sample) = state.staged_sample {
            debug_assert!(!self.slot.sampler.has_pending());
            self.slot.sampler.commit_staged(sample);
            self.slot.sampler.note_formal_sample_published();
        }
        self.slot.frame = state.candidate_frame;
        self.slot.controller = state.staged_controller;
        self.slot.presentation_sequence = self.presentation_sequence;
        self.slot.last_published_plan = Some(state.metadata.plan);
        // A queue-unsafe entry may defer the requested observer after this
        // CPU plan already refreshed. Keep the renderer-owned force latch in
        // that case so the later ticket-bearing presentation refreshes the
        // same camera/order revision instead of merely reusing it.
        let cpu_refresh_reaches_requested_observer = state.metadata.plan == PlanId::CpuPostSort
            && (!self.slot.sampler.has_current_stats_request()
                || state.armed_current_stats.is_some());
        if cpu_refresh_reaches_requested_observer {
            self.slot.force_cpu_order_refresh = false;
        }
        let current_stats = state
            .armed_current_stats
            .map(|armed| {
                self.slot.sampler.commit_current_stats(
                    armed,
                    state.encode_attempt,
                    self.presentation_sequence,
                )
            })
            .unwrap_or_default();

        let host_timings = state.metadata.host_cpu_order.map(|receipt| {
            let timings = receipt.timings();
            HostFrameTimings {
                preprocess_ms: timings.preprocess_ms,
                sort_ms: timings.sort_ms,
                raster_ms: crate::timer_elapsed_ms(receipt.completed_at()),
                frame_ms: crate::timer_elapsed_ms(state.completion_started),
            }
        });
        GpuFrameSubmission {
            submission_index: state.submission_index,
            frame: state.metadata.frame,
            plan: state.metadata.plan,
            order_lane: state.metadata.order_lane,
            order_generation: state.metadata.order_generation,
            source_count: state.metadata.source_count,
            visible_count: state.metadata.visible_count,
            contributor_count: state.metadata.contributor_count,
            draw_count: state.metadata.draw_count,
            count_semantics: state.metadata.count_semantics,
            encode_attempt: state.encode_attempt,
            plan_sample_ticket: state.plan_sample_ticket,
            host_timings,
            presentation_sequence: self.presentation_sequence,
            current_stats,
        }
    }
}

/// Explicitly abandons a submitted target transaction. Queued work may finish,
/// but the callback has no route into Renderer policy or evidence state.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn abandon_submitted_frame(submitted: &mut SubmittedGpuFrame) -> bool {
    submitted.state.take().is_some()
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
            host_cpu_order: work.host_cpu_order(),
        },
    ))
}

fn current_stats_counts_for_work<'a>(
    work: &'a ProjectedWork<'a>,
    encoder: &mut wgpu::CommandEncoder,
) -> Result<CurrentStatsFrameCounts<'a>, FrameExecutionError> {
    let (visible, contributor, count_semantics) = match work.plan_id() {
        PlanId::CpuPostSort => {
            let gpu =
                work.cpu_post_gpu()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "CPU PostSort current-count sources",
                    })?;
            let visible =
                work.visible_count()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "CPU PostSort current visible count",
                    })?;
            (
                CurrentStatsVisibleSource::Host(visible),
                gpu.encode_contributor_count(encoder)?,
                RasterCountSemantics::DirectDrawEqualsVisible,
            )
        }
        PlanId::GpuPostSort => {
            let gpu = work
                .gpu_post()
                .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                    component: "GPU PostSort current-count sources",
                })?;
            (
                CurrentStatsVisibleSource::Gpu(gpu.visible_count()),
                gpu.encode_contributor_count(encoder)?,
                RasterCountSemantics::IndirectDrawEqualsVisible,
            )
        }
        PlanId::GpuPreproject => {
            let gpu =
                work.gpu_preproject()
                    .map_err(|_| FrameExecutionError::ProjectedWorkMismatch {
                        component: "GPU Preproject current-count sources",
                    })?;
            (
                CurrentStatsVisibleSource::Gpu(gpu.candidate_count()),
                gpu.contributor_count(),
                RasterCountSemantics::IndirectDrawEqualsContributor,
            )
        }
    };
    Ok(CurrentStatsFrameCounts {
        frame: work.frame_identity(),
        plan: work.plan_id(),
        order_generation: work.order_generation(),
        source_count: work.source_count(),
        visible,
        contributor,
        count_semantics,
    })
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
mod current_stats_tests;
#[cfg(test)]
mod e11_tests;
#[cfg(test)]
mod e12_tests;
#[cfg(test)]
pub(crate) mod tests;
