//! Closed Exact plan set and the private plan-to-renderer handoff.

mod cpu_post;
mod gpu_post;
mod gpu_pre;

use std::{fmt, sync::Arc};

use gsplat_core::Camera;
use thiserror::Error;

use crate::scene::SceneRuntime;
use crate::{
    TimerInstant,
    cpu_order::{CpuOrderTimings, DepthKeyPrecision},
};

use cpu_post::{CpuPostSortError, CpuPostSortGpuWork, CpuPostSortPlan};
use gpu_post::{GpuPostSortError, GpuPostSortPlan, GpuPostSortWork};
use gpu_pre::{GpuPreprojectError, GpuPreprojectPlan, GpuPreprojectWork};

struct GpuOwnerIdentity;

/// Collision-free capability minted once for one renderer-owned GPU context.
///
/// The inner identity is private and equality is pointer identity, so another
/// wgpu instance reusing a proxy device ID cannot impersonate this owner.
#[derive(Clone)]
pub(crate) struct GpuOwnerToken(Arc<GpuOwnerIdentity>);

impl GpuOwnerToken {
    pub(crate) fn fresh() -> Self {
        Self(Arc::new(GpuOwnerIdentity))
    }

    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for GpuOwnerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GpuOwnerToken(..)")
    }
}

/// Strict GPU inputs routed through the renderer's sole execute boundary.
///
/// This context borrows the renderer-owned owner capability and its existing
/// queue plus the caller-owned encoder. It cannot submit, finish, poll, map,
/// read back or present. wgpu exposes no stable encoder device identity, so
/// construction keeps “encoder came from this owner device” as a private
/// renderer precondition; backend validation/rejection is only a fail-closed
/// backstop, not identity proof supplied by this token.
pub(crate) struct GpuExecutionContext<'a> {
    owner: &'a GpuOwnerToken,
    queue: &'a wgpu::Queue,
    encoder: &'a mut wgpu::CommandEncoder,
}

impl<'a> GpuExecutionContext<'a> {
    pub(crate) fn new(
        owner: &'a GpuOwnerToken,
        queue: &'a wgpu::Queue,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Self {
        Self {
            owner,
            queue,
            encoder,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        &'a GpuOwnerToken,
        &'a wgpu::Queue,
        &'a mut wgpu::CommandEncoder,
    ) {
        (self.owner, self.queue, self.encoder)
    }

    pub(crate) const fn owner_token(&self) -> &GpuOwnerToken {
        self.owner
    }
}

/// Per-frame execution inputs. CPU callers retain the existing route while a
/// future GPU plan receives the same mutable SceneRuntime through PlanSet.
pub(crate) enum PlanExecutionContext<'a> {
    Cpu,
    Gpu(GpuExecutionContext<'a>),
}

impl PlanExecutionContext<'_> {
    const fn has_gpu(&self) -> bool {
        matches!(self, Self::Gpu(_))
    }
}

/// Complete immutable frame input routed from Renderer to one concrete plan.
pub(crate) struct PlanFrameInput<'a> {
    camera: &'a Camera,
    frame: FrameIdentity,
    source_count: u32,
    viewport_width: u32,
    viewport_height: u32,
    force_cpu_order_refresh: bool,
}

impl<'a> PlanFrameInput<'a> {
    pub(crate) const fn new(
        camera: &'a Camera,
        frame: FrameIdentity,
        source_count: u32,
        viewport_width: u32,
        viewport_height: u32,
    ) -> Self {
        Self {
            camera,
            frame,
            source_count,
            viewport_width,
            viewport_height,
            force_cpu_order_refresh: false,
        }
    }

    pub(crate) const fn with_forced_cpu_order_refresh(mut self) -> Self {
        self.force_cpu_order_refresh = true;
        self
    }
}

#[derive(Clone, Copy)]
pub(crate) struct HostCpuOrderReceipt {
    timings: CpuOrderTimings,
    completed_at: TimerInstant,
}

impl HostCpuOrderReceipt {
    pub(crate) const fn new(timings: CpuOrderTimings, completed_at: TimerInstant) -> Self {
        Self {
            timings,
            completed_at,
        }
    }

    pub(crate) const fn timings(self) -> CpuOrderTimings {
        self.timings
    }

    pub(crate) const fn completed_at(self) -> TimerInstant {
        self.completed_at
    }
}

/// Closed identities for complete Exact execution plans.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanId {
    CpuPostSort,
    GpuPostSort,
    GpuPreproject,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderLane {
    Cpu,
    Gpu,
}

/// Immutable generation identity copied from the sole renderer-owned
/// [`crate::renderer::frame::FrameState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameIdentity {
    scene_generation: u64,
    camera_revision: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
}

impl FrameIdentity {
    pub(crate) const fn new(
        scene_generation: u64,
        camera_revision: u64,
        viewport_generation: u64,
        contract_generation: u64,
        plan_set_generation: u64,
    ) -> Self {
        Self {
            scene_generation,
            camera_revision,
            viewport_generation,
            contract_generation,
            plan_set_generation,
        }
    }

    pub(crate) const fn scene_generation(self) -> u64 {
        self.scene_generation
    }

    pub(crate) const fn camera_revision(self) -> u64 {
        self.camera_revision
    }

    pub(crate) const fn viewport_generation(self) -> u64 {
        self.viewport_generation
    }

    pub(crate) const fn contract_generation(self) -> u64 {
        self.contract_generation
    }

    pub(crate) const fn plan_set_generation(self) -> u64 {
        self.plan_set_generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlanSetContract {
    source_count: u32,
    sh_degree: u8,
    scene_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
}

impl PlanSetContract {
    const fn new(source_count: u32, sh_degree: u8, frame: FrameIdentity) -> Self {
        Self {
            source_count,
            sh_degree,
            scene_generation: frame.scene_generation(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
        }
    }
}

/// Complete validated Scene GPU capability proposed to the PlanSet.
///
/// The previous/next generation pair prevents a staged receipt from being
/// admitted against another PlanSet state. Later plans-only work can consume
/// the same request while constructing its concrete plan candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuPlanAdmissionRequest {
    source_count: u32,
    capacity: u32,
    resident_count: u32,
    addressable_count: u32,
    sh_degree: u8,
    scene_generation: u64,
    contract_generation: u64,
    previous_plan_set_generation: u64,
    plan_set_generation: u64,
}

impl GpuPlanAdmissionRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        source_count: u32,
        capacity: u32,
        resident_count: u32,
        addressable_count: u32,
        sh_degree: u8,
        scene_generation: u64,
        contract_generation: u64,
        previous_plan_set_generation: u64,
        plan_set_generation: u64,
    ) -> Self {
        Self {
            source_count,
            capacity,
            resident_count,
            addressable_count,
            sh_degree,
            scene_generation,
            contract_generation,
            previous_plan_set_generation,
            plan_set_generation,
        }
    }

    const fn capability(self) -> GpuCapabilityReceipt {
        GpuCapabilityReceipt {
            source_count: self.source_count,
            capacity: self.capacity,
            resident_count: self.resident_count,
            addressable_count: self.addressable_count,
            sh_degree: self.sh_degree,
            scene_generation: self.scene_generation,
            contract_generation: self.contract_generation,
            plan_set_generation: self.plan_set_generation,
        }
    }
}

/// Durable PlanSet-side proof of the admitted Scene GPU resource graph. This
/// is capability only: it does not make a GPU plan prepared or eligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuCapabilityReceipt {
    source_count: u32,
    capacity: u32,
    resident_count: u32,
    addressable_count: u32,
    sh_degree: u8,
    scene_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
}

impl GpuCapabilityReceipt {
    pub(crate) const fn source_count(self) -> u32 {
        self.source_count
    }

    pub(crate) const fn capacity(self) -> u32 {
        self.capacity
    }

    pub(crate) const fn resident_count(self) -> u32 {
        self.resident_count
    }

    pub(crate) const fn addressable_count(self) -> u32 {
        self.addressable_count
    }

    pub(crate) const fn sh_degree(self) -> u8 {
        self.sh_degree
    }

    pub(crate) const fn scene_generation(self) -> u64 {
        self.scene_generation
    }

    pub(crate) const fn contract_generation(self) -> u64 {
        self.contract_generation
    }

    pub(crate) const fn plan_set_generation(self) -> u64 {
        self.plan_set_generation
    }

    const fn same_resource_capability(self, request: GpuPlanAdmissionRequest) -> bool {
        self.source_count == request.source_count
            && self.capacity == request.capacity
            && self.resident_count == request.resident_count
            && self.addressable_count == request.addressable_count
            && self.sh_degree == request.sh_degree
            && self.scene_generation == request.scene_generation
            && self.contract_generation == request.contract_generation
    }
}

/// Complete unpublished PlanSet replacement produced by the staged hook.
/// The value is intentionally non-Clone, so one staged candidate has one
/// consuming commit.
pub(crate) struct StagedGpuPlanAdmission {
    capability: GpuCapabilityReceipt,
    eligible: Box<[PlanId]>,
    gpu_post: Option<GpuPostSortPlan>,
    gpu_pre: Option<GpuPreprojectPlan>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestGpuAdmissionMode {
    CapabilityOnly,
    Fail,
    Concrete,
    ConcreteAll,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkUnavailable {
    #[error("CPU order IDs are unavailable for this projected work")]
    CpuOrderIds,
    #[error("host-visible candidate count is not available")]
    VisibleCount,
    #[error("post-projection contributor count is not available")]
    ContributorCount,
    #[error("issued draw count is not available")]
    DrawCount,
    #[error("rank-indexed GPU projected work is not available")]
    GpuProjectedWork,
}

/// Exact relationship carried by a GPU-owned indirect count without
/// fabricating a host-visible numeric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndirectCountSemantics {
    DrawEqualsVisible,
    DrawEqualsContributor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectCountSemantics {
    DrawEqualsVisible,
}

/// Private, accessor-only plan handoff.
///
/// E1 stops after the authoritative CPU order and therefore reports only
/// host-known S/V. V, C and D have independent availability so a future GPU
/// plan cannot substitute capacity or zero while an indirect count remains
/// unresolved. The fields stay private so common tests and renderer code never
/// exhaustively match plan-internal CPU/GPU variants.
#[allow(dead_code)]
pub(crate) struct ProjectedWork<'a> {
    plan: PlanId,
    order_lane: OrderLane,
    frame: FrameIdentity,
    order_generation: u64,
    source_count: u32,
    visible_count: Option<u32>,
    contributor_count: Option<u32>,
    draw_count: Option<u32>,
    cpu_order_ids: Option<&'a [u32]>,
    cpu_post_gpu: Option<CpuPostSortGpuWork<'a>>,
    gpu_post: Option<GpuPostSortWork<'a>>,
    gpu_pre: Option<GpuPreprojectWork<'a>>,
    host_cpu_order: Option<HostCpuOrderReceipt>,
}

#[allow(dead_code)]
impl<'a> ProjectedWork<'a> {
    fn from_cpu_post_sort(
        frame: FrameIdentity,
        order_generation: u64,
        source_count: u32,
        ordered_ids: &'a [u32],
        cpu_post_gpu: Option<CpuPostSortGpuWork<'a>>,
        host_cpu_order: HostCpuOrderReceipt,
    ) -> Self {
        let draw_count = cpu_post_gpu.as_ref().map(CpuPostSortGpuWork::direct_count);
        Self {
            plan: PlanId::CpuPostSort,
            order_lane: OrderLane::Cpu,
            frame,
            order_generation,
            source_count,
            visible_count: Some(ordered_ids.len() as u32),
            contributor_count: None,
            draw_count,
            cpu_order_ids: Some(ordered_ids),
            cpu_post_gpu,
            gpu_post: None,
            gpu_pre: None,
            host_cpu_order: Some(host_cpu_order),
        }
    }

    fn from_gpu_post_sort(
        frame: FrameIdentity,
        order_generation: u64,
        source_count: u32,
        gpu_post: GpuPostSortWork<'a>,
    ) -> Self {
        Self {
            plan: PlanId::GpuPostSort,
            order_lane: OrderLane::Gpu,
            frame,
            order_generation,
            source_count,
            // V and D live only in sorter-owned indirect arguments. Keep both
            // numeric values explicitly unavailable until a later terminal
            // evidence owner provides them; capacity/source/zero are not V.
            visible_count: None,
            contributor_count: None,
            draw_count: None,
            cpu_order_ids: None,
            cpu_post_gpu: None,
            gpu_post: Some(gpu_post),
            gpu_pre: None,
            host_cpu_order: None,
        }
    }

    fn from_gpu_preproject(
        frame: FrameIdentity,
        order_generation: u64,
        source_count: u32,
        gpu_pre: GpuPreprojectWork<'a>,
    ) -> Self {
        Self {
            plan: PlanId::GpuPreproject,
            order_lane: OrderLane::Gpu,
            frame,
            order_generation,
            source_count,
            // V, C and D remain GPU-owned. Their buffers and exact D=C
            // relationship travel with GpuPreprojectWork; numeric host values
            // stay unavailable rather than borrowing S/capacity/zero.
            visible_count: None,
            contributor_count: None,
            draw_count: None,
            cpu_order_ids: None,
            cpu_post_gpu: None,
            gpu_post: None,
            gpu_pre: Some(gpu_pre),
            host_cpu_order: None,
        }
    }

    pub(crate) const fn plan_id(&self) -> PlanId {
        self.plan
    }

    pub(crate) const fn order_lane(&self) -> OrderLane {
        self.order_lane
    }

    pub(crate) const fn frame_identity(&self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn order_generation(&self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn source_count(&self) -> u32 {
        self.source_count
    }

    pub(crate) fn visible_count(&self) -> Result<u32, WorkUnavailable> {
        self.visible_count.ok_or(WorkUnavailable::VisibleCount)
    }

    pub(crate) fn contributor_count(&self) -> Result<u32, WorkUnavailable> {
        self.contributor_count
            .ok_or(WorkUnavailable::ContributorCount)
    }

    pub(crate) fn draw_count(&self) -> Result<u32, WorkUnavailable> {
        self.draw_count.ok_or(WorkUnavailable::DrawCount)
    }

    pub(crate) fn cpu_order_ids(&self) -> Result<&[u32], WorkUnavailable> {
        self.cpu_order_ids.ok_or(WorkUnavailable::CpuOrderIds)
    }

    pub(crate) fn cpu_post_gpu(&self) -> Result<&CpuPostSortGpuWork<'a>, WorkUnavailable> {
        self.cpu_post_gpu
            .as_ref()
            .ok_or(WorkUnavailable::GpuProjectedWork)
    }

    pub(crate) fn gpu_post(&self) -> Result<&GpuPostSortWork<'a>, WorkUnavailable> {
        self.gpu_post
            .as_ref()
            .ok_or(WorkUnavailable::GpuProjectedWork)
    }

    pub(crate) fn gpu_preproject(&self) -> Result<&GpuPreprojectWork<'a>, WorkUnavailable> {
        self.gpu_pre
            .as_ref()
            .ok_or(WorkUnavailable::GpuProjectedWork)
    }

    pub(crate) const fn host_cpu_order(&self) -> Option<HostCpuOrderReceipt> {
        self.host_cpu_order
    }
}

#[derive(Debug, Error)]
pub(crate) enum PlanSetError {
    #[error("Exact PlanSet must contain at least one eligible plan")]
    Empty,
    #[error("eligible plan {plan:?} has no prepared concrete entry")]
    EligiblePlanUnprepared { plan: PlanId },
    #[error("eligible plan {plan:?} is duplicated")]
    DuplicateEligiblePlan { plan: PlanId },
    #[error("fallback plan {fallback:?} has no prepared concrete entry")]
    FallbackUnprepared { fallback: PlanId },
    #[error("fallback plan {fallback:?} is not eligible")]
    FallbackIneligible { fallback: PlanId },
    #[error("requested plan {requested:?} has no prepared concrete entry")]
    RequestedPlanUnprepared { requested: PlanId },
    #[error("requested prepared plan {requested:?} is not eligible")]
    RequestedPlanIneligible { requested: PlanId },
    #[error("requested GPU plan {requested:?} has no renderer-owned GPU execution context")]
    GpuExecutionUnavailable { requested: PlanId },
    #[error("Exact PlanSet source count exceeds u32 addressability")]
    SourceCountOverflow,
    #[error("CPU PostSort depth-key precision does not match the Exact PlanSet")]
    DepthKeyPrecisionMismatch,
    #[error("GPU admission count/SH receipt does not match the Exact PlanSet contract")]
    GpuAdmissionContractMismatch,
    #[error(
        "GPU admission generation mismatch: current={current}, previous={previous}, next={next}"
    )]
    GpuAdmissionGenerationMismatch {
        current: u64,
        previous: u64,
        next: u64,
    },
    #[error("GPU capability is already admitted at PlanSet generation {generation}")]
    GpuCapabilityAlreadyAdmitted { generation: u64 },
    #[error("a different GPU capability is already admitted")]
    GpuCapabilityConflict,
    #[error("prepared GPU PostSort plan does not match its admitted capability")]
    GpuPlanCapabilityMismatch,
    #[error("prepared GPU Preproject plan does not match its admitted capability")]
    GpuPreprojectCapabilityMismatch,
    #[error("GPU admission staging rejected: {reason}")]
    GpuAdmissionRejected { reason: &'static str },
    #[error("CPU PostSort failed: {0}")]
    CpuPostSort(#[from] CpuPostSortError),
    #[error("GPU PostSort failed: {0}")]
    GpuPostSort(#[from] GpuPostSortError),
    #[error("GPU Preproject failed: {0}")]
    GpuPreproject(#[from] GpuPreprojectError),
}

/// Validated, immutable-membership set of prepared Exact plans.
pub(crate) struct PlanSet {
    fallback: PlanId,
    eligible: Box<[PlanId]>,
    cpu_post: Option<CpuPostSortPlan>,
    gpu_post: Option<GpuPostSortPlan>,
    gpu_pre: Option<GpuPreprojectPlan>,
    depth_key_precision: DepthKeyPrecision,
    contract: PlanSetContract,
    gpu_capability: Option<GpuCapabilityReceipt>,
    #[cfg(test)]
    test_gpu_admission_mode: TestGpuAdmissionMode,
}

impl PlanSet {
    pub(crate) fn prepare_cpu(
        source_count: usize,
        sh_degree: u8,
        frame: FrameIdentity,
    ) -> Result<Self, PlanSetError> {
        Self::prepare_cpu_with_depth_key_precision(
            source_count,
            sh_degree,
            frame,
            DepthKeyPrecision::ExactFull32,
        )
    }

    pub(crate) fn prepare_cpu_with_depth_key_precision(
        source_count: usize,
        sh_degree: u8,
        frame: FrameIdentity,
        depth_key_precision: DepthKeyPrecision,
    ) -> Result<Self, PlanSetError> {
        let cpu_post =
            CpuPostSortPlan::prepare_with_depth_key_precision(source_count, depth_key_precision)?;
        let source_count =
            u32::try_from(source_count).map_err(|_| PlanSetError::SourceCountOverflow)?;
        Self::try_new(
            Some(cpu_post),
            vec![PlanId::CpuPostSort].into_boxed_slice(),
            PlanId::CpuPostSort,
            PlanSetContract::new(source_count, sh_degree, frame),
            depth_key_precision,
        )
    }

    fn try_new(
        cpu_post: Option<CpuPostSortPlan>,
        eligible: Box<[PlanId]>,
        fallback: PlanId,
        contract: PlanSetContract,
        depth_key_precision: DepthKeyPrecision,
    ) -> Result<Self, PlanSetError> {
        let candidate = Self {
            fallback,
            eligible,
            cpu_post,
            gpu_post: None,
            gpu_pre: None,
            depth_key_precision,
            contract,
            gpu_capability: None,
            #[cfg(test)]
            test_gpu_admission_mode: TestGpuAdmissionMode::CapabilityOnly,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    fn validate(&self) -> Result<(), PlanSetError> {
        if self
            .cpu_post
            .as_ref()
            .is_some_and(|plan| plan.depth_key_precision() != self.depth_key_precision)
        {
            return Err(PlanSetError::DepthKeyPrecisionMismatch);
        }
        if self.eligible.is_empty() {
            return Err(PlanSetError::Empty);
        }
        for (index, plan) in self.eligible.iter().copied().enumerate() {
            if self.eligible[..index].contains(&plan) {
                return Err(PlanSetError::DuplicateEligiblePlan { plan });
            }
            if !self.is_prepared(plan) {
                return Err(PlanSetError::EligiblePlanUnprepared { plan });
            }
        }
        if !self.is_prepared(self.fallback) {
            return Err(PlanSetError::FallbackUnprepared {
                fallback: self.fallback,
            });
        }
        if !self.eligible.contains(&self.fallback) {
            return Err(PlanSetError::FallbackIneligible {
                fallback: self.fallback,
            });
        }
        if let Some(gpu_post) = self.gpu_post.as_ref()
            && self.gpu_capability != Some(gpu_post.capability())
        {
            return Err(PlanSetError::GpuPlanCapabilityMismatch);
        }
        if let Some(gpu_pre) = self.gpu_pre.as_ref()
            && self.gpu_capability != Some(gpu_pre.capability())
        {
            return Err(PlanSetError::GpuPreprojectCapabilityMismatch);
        }
        Ok(())
    }

    pub(crate) const fn depth_key_precision(&self) -> DepthKeyPrecision {
        self.depth_key_precision
    }

    fn is_prepared(&self, plan: PlanId) -> bool {
        match plan {
            PlanId::CpuPostSort => self.cpu_post.is_some(),
            PlanId::GpuPostSort => self.gpu_post.is_some(),
            PlanId::GpuPreproject => self.gpu_pre.is_some(),
        }
    }

    /// Stages the PlanSet-side half of device admission without modifying
    /// membership, capability or generation. Downlevel devices still publish
    /// the device-owned CPU Exact graph but do not admit indirect GPU plans.
    pub(crate) fn stage_gpu_admission(
        &self,
        request: GpuPlanAdmissionRequest,
        complete_gpu_plans: bool,
    ) -> Result<StagedGpuPlanAdmission, PlanSetError> {
        self.validate_gpu_admission(request)?;
        if let Some(existing) = self.gpu_capability {
            return if existing.same_resource_capability(request) {
                Err(PlanSetError::GpuCapabilityAlreadyAdmitted {
                    generation: existing.plan_set_generation,
                })
            } else {
                Err(PlanSetError::GpuCapabilityConflict)
            };
        }

        #[cfg(test)]
        if self.test_gpu_admission_mode == TestGpuAdmissionMode::Fail {
            return Err(PlanSetError::GpuAdmissionRejected {
                reason: "injected staged-plan failure",
            });
        }

        let capability = request.capability();
        let mut eligible = self.eligible.to_vec();
        #[cfg(not(test))]
        let (gpu_post, gpu_pre) = if complete_gpu_plans {
            (
                Some(GpuPostSortPlan::prepare(capability)?),
                Some(GpuPreprojectPlan::prepare(capability)?),
            )
        } else {
            (None, None)
        };
        #[cfg(test)]
        let (gpu_post, gpu_pre) = match (complete_gpu_plans, self.test_gpu_admission_mode) {
            (false, _) | (true, TestGpuAdmissionMode::CapabilityOnly) => (None, None),
            (true, TestGpuAdmissionMode::Fail) => unreachable!("failure returned above"),
            (true, TestGpuAdmissionMode::Concrete) => {
                (Some(GpuPostSortPlan::prepare(capability)?), None)
            }
            (true, TestGpuAdmissionMode::ConcreteAll) => (
                Some(GpuPostSortPlan::prepare(capability)?),
                Some(GpuPreprojectPlan::prepare(capability)?),
            ),
        };
        if gpu_post.is_some() {
            eligible.push(PlanId::GpuPostSort);
        }
        if gpu_pre.is_some() {
            eligible.push(PlanId::GpuPreproject);
        }
        let candidate = StagedGpuPlanAdmission {
            capability,
            eligible: eligible.into_boxed_slice(),
            gpu_post,
            gpu_pre,
        };
        self.validate_staged_gpu_admission(&candidate)?;
        Ok(candidate)
    }

    /// Publishes a previously validated candidate. Candidate fields are fully
    /// owned and this method contains no fallible work.
    pub(crate) fn commit_gpu_admission(&mut self, candidate: StagedGpuPlanAdmission) {
        self.eligible = candidate.eligible;
        self.contract.plan_set_generation = candidate.capability.plan_set_generation;
        self.gpu_capability = Some(candidate.capability);
        self.gpu_post = candidate.gpu_post;
        self.gpu_pre = candidate.gpu_pre;
        debug_assert!(self.validate().is_ok());
    }

    fn validate_gpu_admission(&self, request: GpuPlanAdmissionRequest) -> Result<(), PlanSetError> {
        let current = self.contract.plan_set_generation;
        let expected_next = current.checked_add(1);
        if request.previous_plan_set_generation != current
            || expected_next != Some(request.plan_set_generation)
        {
            return Err(PlanSetError::GpuAdmissionGenerationMismatch {
                current,
                previous: request.previous_plan_set_generation,
                next: request.plan_set_generation,
            });
        }
        if request.source_count != self.contract.source_count
            || request.capacity != request.source_count
            || request.resident_count != request.source_count
            || request.addressable_count != request.source_count
            || request.sh_degree != self.contract.sh_degree
            || request.sh_degree > 3
            || request.scene_generation != self.contract.scene_generation
            || request.contract_generation != self.contract.contract_generation
        {
            return Err(PlanSetError::GpuAdmissionContractMismatch);
        }
        Ok(())
    }

    fn validate_staged_gpu_admission(
        &self,
        candidate: &StagedGpuPlanAdmission,
    ) -> Result<(), PlanSetError> {
        if let Some(gpu_post) = candidate.gpu_post.as_ref()
            && gpu_post.capability() != candidate.capability
        {
            return Err(PlanSetError::GpuPlanCapabilityMismatch);
        }
        if let Some(gpu_pre) = candidate.gpu_pre.as_ref()
            && gpu_pre.capability() != candidate.capability
        {
            return Err(PlanSetError::GpuPreprojectCapabilityMismatch);
        }
        if candidate.eligible.is_empty() {
            return Err(PlanSetError::Empty);
        }
        for (index, plan) in candidate.eligible.iter().copied().enumerate() {
            if candidate.eligible[..index].contains(&plan) {
                return Err(PlanSetError::DuplicateEligiblePlan { plan });
            }
            let prepared = match plan {
                PlanId::CpuPostSort => self.cpu_post.is_some(),
                PlanId::GpuPostSort => candidate.gpu_post.is_some(),
                PlanId::GpuPreproject => candidate.gpu_pre.is_some(),
            };
            if !prepared {
                return Err(PlanSetError::EligiblePlanUnprepared { plan });
            }
        }
        Ok(())
    }

    pub(crate) const fn gpu_capability(&self) -> Option<GpuCapabilityReceipt> {
        self.gpu_capability
    }

    #[cfg(test)]
    pub(crate) fn set_test_gpu_admission_mode(&mut self, mode: TestGpuAdmissionMode) {
        self.test_gpu_admission_mode = mode;
    }

    #[cfg(test)]
    pub(crate) fn test_gpu_post_capability(&self) -> Option<GpuCapabilityReceipt> {
        self.gpu_post.as_ref().map(GpuPostSortPlan::capability)
    }

    pub(crate) fn execute<'scene>(
        &'scene mut self,
        requested: PlanId,
        scene: &'scene mut SceneRuntime,
        input: PlanFrameInput<'_>,
        execution: PlanExecutionContext<'_>,
    ) -> Result<ProjectedWork<'scene>, PlanSetError> {
        let gpu_requested = matches!(requested, PlanId::GpuPostSort | PlanId::GpuPreproject);
        if gpu_requested && !execution.has_gpu() {
            return Err(PlanSetError::GpuExecutionUnavailable { requested });
        }
        if !self.is_prepared(requested) {
            return Err(PlanSetError::RequestedPlanUnprepared { requested });
        }
        if !self.eligible.contains(&requested) {
            return Err(PlanSetError::RequestedPlanIneligible { requested });
        }
        match requested {
            PlanId::CpuPostSort => {
                let gpu = match execution {
                    PlanExecutionContext::Cpu => None,
                    PlanExecutionContext::Gpu(gpu) => Some(gpu),
                };
                self.cpu_post
                    .as_mut()
                    .ok_or(PlanSetError::RequestedPlanUnprepared { requested })?
                    .execute(scene, input, gpu)
                    .map_err(PlanSetError::from)
            }
            PlanId::GpuPostSort => {
                let PlanExecutionContext::Gpu(gpu) = execution else {
                    unreachable!("GPU context was validated above")
                };
                self.gpu_post
                    .as_mut()
                    .ok_or(PlanSetError::RequestedPlanUnprepared { requested })?
                    .execute(scene, input, gpu)
                    .map_err(PlanSetError::from)
            }
            PlanId::GpuPreproject => {
                let PlanExecutionContext::Gpu(gpu) = execution else {
                    unreachable!("GPU context was validated above")
                };
                self.gpu_pre
                    .as_mut()
                    .ok_or(PlanSetError::RequestedPlanUnprepared { requested })?
                    .execute(scene, input, gpu)
                    .map_err(PlanSetError::from)
            }
        }
    }

    pub(crate) const fn fallback(&self) -> PlanId {
        self.fallback
    }

    pub(crate) fn eligible(&self) -> &[PlanId] {
        &self.eligible
    }

    pub(crate) fn last_usable_cpu_order(&self) -> Option<&[u32]> {
        self.cpu_post
            .as_ref()
            .and_then(CpuPostSortPlan::last_usable_order)
    }

    #[cfg(test)]
    pub(crate) fn current_cpu_order_generation(&self) -> Option<u64> {
        self.cpu_post
            .as_ref()
            .map(CpuPostSortPlan::current_order_generation)
    }
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, SceneBuffers};

    use super::{
        CpuPostSortPlan, DepthKeyPrecision, FrameIdentity, GpuPlanAdmissionRequest,
        PlanExecutionContext, PlanFrameInput, PlanId, PlanSet, PlanSetContract, PlanSetError,
        WorkUnavailable,
    };
    use crate::scene::{ResidentSceneCpu, SceneRuntime};

    fn empty_scene() -> SceneRuntime {
        let resident = ResidentSceneCpu::encode_owned(SceneBuffers {
            positions: Vec::new(),
            opacity: Vec::new(),
            scale_xyz: Vec::new(),
            rotation_xyzw: Vec::new(),
            color_dc: Vec::new(),
            sh_degree: 0,
            sh_rest: None,
        })
        .expect("resident scene");
        SceneRuntime::prepare(resident).expect("scene runtime")
    }

    fn cpu_plan() -> CpuPostSortPlan {
        CpuPostSortPlan::prepare(0).expect("CPU plan")
    }

    fn identity(plan_set_generation: u64) -> FrameIdentity {
        FrameIdentity::new(1, 0, 0, 1, plan_set_generation)
    }

    fn contract() -> PlanSetContract {
        PlanSetContract::new(0, 0, identity(1))
    }

    fn gpu_request(previous: u64, next: u64) -> GpuPlanAdmissionRequest {
        GpuPlanAdmissionRequest::new(0, 0, 0, 0, 0, 1, 1, previous, next)
    }

    #[test]
    fn empty_plan_set_fails_closed() {
        assert!(matches!(
            PlanSet::try_new(
                None,
                Vec::new().into_boxed_slice(),
                PlanId::CpuPostSort,
                contract(),
                DepthKeyPrecision::ExactFull32,
            ),
            Err(PlanSetError::Empty)
        ));
    }

    #[test]
    fn missing_fallback_fails_closed() {
        assert!(matches!(
            PlanSet::try_new(
                Some(cpu_plan()),
                vec![PlanId::CpuPostSort].into_boxed_slice(),
                PlanId::GpuPostSort,
                contract(),
                DepthKeyPrecision::ExactFull32,
            ),
            Err(PlanSetError::FallbackUnprepared {
                fallback: PlanId::GpuPostSort
            })
        ));
    }

    #[test]
    fn request_for_unprepared_plan_fails_closed() {
        let mut plans = PlanSet::prepare_cpu(0, 0, identity(1)).expect("plan set");
        let mut scene = empty_scene();
        let frame = FrameIdentity::new(1, 1, 1, 1, 1);

        assert!(matches!(
            plans.execute(
                PlanId::GpuPostSort,
                &mut scene,
                PlanFrameInput::new(&Camera::default(), frame, 0, 1, 1),
                PlanExecutionContext::Cpu,
            ),
            Err(PlanSetError::GpuExecutionUnavailable {
                requested: PlanId::GpuPostSort
            })
        ));
        assert!(matches!(
            plans.execute(
                PlanId::GpuPreproject,
                &mut scene,
                PlanFrameInput::new(&Camera::default(), frame, 0, 1, 1),
                PlanExecutionContext::Cpu,
            ),
            Err(PlanSetError::GpuExecutionUnavailable {
                requested: PlanId::GpuPreproject
            })
        ));
        assert_eq!(plans.fallback(), PlanId::CpuPostSort);
        assert_eq!(plans.eligible(), [PlanId::CpuPostSort]);
    }

    #[test]
    fn cpu_handoff_reports_only_source_and_visible_counts() {
        let mut plans = PlanSet::prepare_cpu(0, 0, identity(1)).expect("plan set");
        let mut scene = empty_scene();
        let work = plans
            .execute(
                PlanId::CpuPostSort,
                &mut scene,
                PlanFrameInput::new(
                    &Camera::default(),
                    FrameIdentity::new(1, 1, 1, 1, 1),
                    0,
                    1,
                    1,
                ),
                PlanExecutionContext::Cpu,
            )
            .expect("CPU work");

        assert_eq!(work.source_count(), 0);
        assert_eq!(work.visible_count(), Ok(0));
        assert!(work.cpu_order_ids().expect("CPU IDs").is_empty());
        assert_eq!(
            work.contributor_count(),
            Err(WorkUnavailable::ContributorCount)
        );
        assert_eq!(work.draw_count(), Err(WorkUnavailable::DrawCount));
    }

    #[test]
    fn gpu_capability_stage_commit_reentry_and_generation_are_explicit() {
        let mut plans = PlanSet::prepare_cpu(0, 0, identity(1)).expect("plan set");
        let candidate = plans
            .stage_gpu_admission(gpu_request(1, 2), true)
            .expect("staged capability");

        assert!(plans.gpu_capability().is_none());
        assert_eq!(plans.eligible(), &[PlanId::CpuPostSort]);
        assert_eq!(plans.contract.plan_set_generation, 1);

        plans.commit_gpu_admission(candidate);
        assert_eq!(
            plans
                .gpu_capability()
                .expect("committed capability")
                .plan_set_generation(),
            2
        );
        assert_eq!(plans.eligible(), &[PlanId::CpuPostSort]);
        assert_eq!(plans.contract.plan_set_generation, 2);
        assert!(matches!(
            plans.stage_gpu_admission(gpu_request(2, 3), true),
            Err(PlanSetError::GpuCapabilityAlreadyAdmitted { generation: 2 })
        ));
        assert!(matches!(
            plans.stage_gpu_admission(gpu_request(1, 2), true),
            Err(PlanSetError::GpuAdmissionGenerationMismatch {
                current: 2,
                previous: 1,
                next: 2,
            })
        ));
        assert_eq!(plans.contract.plan_set_generation, 2);
    }
}
