//! Private control plane for the shared Surface session.
//!
//! This module owns the closed Exact compatibility mapping and the pure
//! arbitration rules between CPU/GPU ordering and Candidate/Compact projected
//! drawing. It never executes a frame, consumes telemetry, publishes evidence,
//! or commits presentation state.

#[cfg(test)]
use crate::SurfaceProjectedDrawExecution;
use crate::plans::PlanId;
use crate::renderer::ExactPlanPolicy;
use crate::surface_session::{
    SurfaceOrderBackend, SurfaceOrderBackendUsed, SurfaceProjectedDrawPolicy, SurfaceSortSchedule,
};
use crate::{
    GeometryPath, Renderer, RendererError, SurfaceGpuOrderProducer, SurfacePresenterError,
    SurfaceRasterExecutionPlan,
};

use super::{
    AdaptiveMetric, AdaptiveOrderPolicy, AdaptiveProjectedDrawPolicy, AdaptiveRefreshChoice,
    ProjectedAdaptiveChoice, ProjectedAdaptiveSampleKind, adaptive_primary_metric,
};

/// Closed compatibility state for the renderer-owned complete Exact plan set.
///
/// Each forced state names one complete internal [`PlanId`]. `Adaptive` is the
/// renderer-owned policy over that same admitted set, never an extra plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactSurfacePlanState {
    CpuPostSort,
    GpuPostSort,
    GpuPreproject,
    Adaptive,
}

impl ExactSurfacePlanState {
    #[cfg(test)]
    const ALL: [Self; 4] = [
        Self::CpuPostSort,
        Self::GpuPostSort,
        Self::GpuPreproject,
        Self::Adaptive,
    ];

    pub(crate) const fn policy(self) -> ExactPlanPolicy {
        match self {
            Self::CpuPostSort => ExactPlanPolicy::Forced(PlanId::CpuPostSort),
            Self::GpuPostSort => ExactPlanPolicy::Forced(PlanId::GpuPostSort),
            Self::GpuPreproject => ExactPlanPolicy::Forced(PlanId::GpuPreproject),
            Self::Adaptive => ExactPlanPolicy::Adaptive,
        }
    }

    pub(crate) const fn from_policy(policy: ExactPlanPolicy) -> Self {
        match policy {
            ExactPlanPolicy::Forced(PlanId::CpuPostSort) => Self::CpuPostSort,
            ExactPlanPolicy::Forced(PlanId::GpuPostSort) => Self::GpuPostSort,
            ExactPlanPolicy::Forced(PlanId::GpuPreproject) => Self::GpuPreproject,
            ExactPlanPolicy::Adaptive => Self::Adaptive,
        }
    }

    pub(crate) const fn order_backend(self) -> SurfaceOrderBackend {
        match self {
            Self::CpuPostSort => SurfaceOrderBackend::Cpu,
            Self::GpuPostSort | Self::GpuPreproject => SurfaceOrderBackend::Gpu,
            Self::Adaptive => SurfaceOrderBackend::Adaptive,
        }
    }

    pub(crate) const fn projected_policy(self) -> SurfaceProjectedDrawPolicy {
        match self {
            Self::GpuPreproject => SurfaceProjectedDrawPolicy::Compact,
            Self::Adaptive => SurfaceProjectedDrawPolicy::Adaptive,
            Self::CpuPostSort | Self::GpuPostSort => SurfaceProjectedDrawPolicy::Candidate,
        }
    }

    pub(crate) const fn producer(self) -> SurfaceGpuOrderProducer {
        match self {
            Self::GpuPreproject => SurfaceGpuOrderProducer::Preproject,
            Self::CpuPostSort | Self::GpuPostSort | Self::Adaptive => {
                SurfaceGpuOrderProducer::PostSort
            }
        }
    }

    pub(crate) const fn with_order_backend(self, backend: SurfaceOrderBackend) -> Self {
        match backend {
            SurfaceOrderBackend::Cpu => Self::CpuPostSort,
            SurfaceOrderBackend::Gpu => match self {
                Self::GpuPreproject => Self::GpuPreproject,
                Self::CpuPostSort | Self::GpuPostSort | Self::Adaptive => Self::GpuPostSort,
            },
            SurfaceOrderBackend::Adaptive => Self::Adaptive,
        }
    }

    pub(crate) fn with_projected_policy(
        self,
        policy: SurfaceProjectedDrawPolicy,
    ) -> Result<Self, SurfacePresenterError> {
        match policy {
            SurfaceProjectedDrawPolicy::Candidate => Ok(match self {
                Self::CpuPostSort => Self::CpuPostSort,
                Self::GpuPostSort | Self::GpuPreproject => Self::GpuPostSort,
                Self::Adaptive => {
                    return Err(SurfacePresenterError::PreprojectProducerIncompatible);
                }
            }),
            SurfaceProjectedDrawPolicy::Compact => match self {
                Self::GpuPostSort | Self::GpuPreproject => Ok(Self::GpuPreproject),
                Self::CpuPostSort | Self::Adaptive => {
                    Err(SurfacePresenterError::PreprojectProducerIncompatible)
                }
            },
            // Platform bindings apply the order axis first and then the
            // projected default. Preserve a forced order while retaining the
            // separately configured public Adaptive receipt in the Session.
            SurfaceProjectedDrawPolicy::Adaptive => Ok(match self {
                Self::CpuPostSort => Self::CpuPostSort,
                Self::GpuPostSort | Self::GpuPreproject => Self::GpuPostSort,
                Self::Adaptive => Self::Adaptive,
            }),
        }
    }

    pub(crate) fn with_producer(
        self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<Self, SurfacePresenterError> {
        match producer {
            SurfaceGpuOrderProducer::PostSort => Ok(match self {
                Self::GpuPreproject => Self::GpuPostSort,
                Self::CpuPostSort | Self::GpuPostSort | Self::Adaptive => self,
            }),
            SurfaceGpuOrderProducer::Preproject => match self {
                Self::GpuPostSort | Self::GpuPreproject => Ok(Self::GpuPreproject),
                Self::CpuPostSort | Self::Adaptive => {
                    Err(SurfacePresenterError::PreprojectProducerIncompatible)
                }
            },
        }
    }

    pub(crate) fn with_raster(
        self,
        plan: SurfaceRasterExecutionPlan,
    ) -> Result<Self, SurfacePresenterError> {
        if plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact {
            Ok(self)
        } else {
            Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported)
        }
    }

    pub(crate) fn with_geometry(self, path: GeometryPath) -> Result<Self, SurfacePresenterError> {
        if path == GeometryPath::PackedAtlas {
            Ok(self)
        } else {
            Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported)
        }
    }

    pub(crate) fn with_gpu_producer_measurement(
        self,
        enabled: bool,
    ) -> Result<Self, SurfacePresenterError> {
        if enabled {
            Err(SurfacePresenterError::PreprojectProducerIncompatible)
        } else {
            Ok(self)
        }
    }

    pub(crate) fn with_sort_interval(self, interval: u32) -> Result<Self, RendererError> {
        if interval == 1 {
            Ok(self)
        } else {
            Err(RendererError::InvalidConfig)
        }
    }

    pub(crate) fn with_sort_schedule(
        self,
        schedule: SurfaceSortSchedule,
    ) -> Result<Self, RendererError> {
        match schedule {
            SurfaceSortSchedule::Interval(1) => Ok(self),
            SurfaceSortSchedule::Interval(_) | SurfaceSortSchedule::AsyncLatest { .. } => {
                Err(RendererError::InvalidConfig)
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn with_async_sort(self, enabled: bool) -> Result<Self, RendererError> {
        if enabled {
            Err(RendererError::InvalidConfig)
        } else {
            Ok(self)
        }
    }
}

pub(crate) const fn exact_gpu_plan_for_producer(producer: SurfaceGpuOrderProducer) -> PlanId {
    match producer {
        SurfaceGpuOrderProducer::PostSort => PlanId::GpuPostSort,
        SurfaceGpuOrderProducer::Preproject => PlanId::GpuPreproject,
    }
}

pub(crate) fn prepare_exact_gpu_order_producer(
    renderer: &Renderer,
    producer: SurfaceGpuOrderProducer,
) -> Result<(), RendererError> {
    if renderer.exact_surface_plan_is_eligible(exact_gpu_plan_for_producer(producer)) == Some(true)
    {
        Ok(())
    } else {
        Err(SurfacePresenterError::GpuOrderUnsupported.into())
    }
}

pub(crate) fn prepare_exact_gpu_order(
    renderer: &Renderer,
    state: ExactSurfacePlanState,
) -> Result<(), RendererError> {
    prepare_exact_gpu_order_producer(renderer, state.producer())
}

/// Commits one complete Exact policy and only then updates compatibility
/// receipts. Rejection by the renderer leaves every caller-owned receipt
/// unchanged.
pub(crate) fn commit_exact_plan_state(
    renderer: &mut Renderer,
    exact_plan_receipt: &mut Option<ExactSurfacePlanState>,
    order_backend: &mut SurfaceOrderBackend,
    projected_draw_policy: &mut SurfaceProjectedDrawPolicy,
    state: ExactSurfacePlanState,
) -> Result<(), RendererError> {
    renderer.set_exact_surface_policy(state.policy())?;
    *order_backend = state.order_backend();
    *projected_draw_policy = state.projected_policy();
    *exact_plan_receipt = Some(state);
    Ok(())
}

pub(crate) fn validate_projected_draw_policy_transition(
    current: SurfaceProjectedDrawPolicy,
    next: SurfaceProjectedDrawPolicy,
    compact_available: bool,
) -> Result<bool, SurfacePresenterError> {
    if current == next {
        return Ok(false);
    }
    if next == SurfaceProjectedDrawPolicy::Compact && !compact_available {
        return Err(SurfacePresenterError::ProjectedCompactionUnsupported);
    }
    Ok(true)
}

/// Applies a forced Candidate/Compact/Adaptive policy change as one control
/// transaction. Validation happens before any state is touched. Accepted
/// changes discard only samples and ownership that cannot cross the new raster
/// workload; completed per-lane history remains owned by the controllers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_projected_draw_policy_transition(
    current: &mut SurfaceProjectedDrawPolicy,
    next: SurfaceProjectedDrawPolicy,
    compact_available: bool,
    adaptive_policy: &mut AdaptiveOrderPolicy,
    projected_cpu: &mut AdaptiveProjectedDrawPolicy,
    projected_gpu: &mut AdaptiveProjectedDrawPolicy,
    owner: &mut Option<AdaptiveProbeOwner>,
    blocked_order_choice: &mut Option<AdaptiveRefreshChoice>,
    pending_order_backend: &mut Option<SurfaceOrderBackendUsed>,
    pending_adaptive_choice: &mut Option<AdaptiveRefreshChoice>,
    pending_projected_choice: &mut Option<ProjectedAdaptiveChoice>,
) -> Result<bool, SurfacePresenterError> {
    if !validate_projected_draw_policy_transition(*current, next, compact_available)? {
        return Ok(false);
    }
    if adaptive_policy.metric() == AdaptiveMetric::FrameCompletion {
        adaptive_policy.reset(AdaptiveMetric::FrameCompletion);
    }
    projected_cpu.suspend_learning();
    projected_gpu.suspend_learning();
    *owner = None;
    *blocked_order_choice = None;
    *current = next;
    *pending_order_backend = None;
    *pending_adaptive_choice = None;
    *pending_projected_choice = None;
    Ok(true)
}

pub(crate) fn validate_gpu_order_producer_transition(
    current: SurfaceGpuOrderProducer,
    next: SurfaceGpuOrderProducer,
    geometry_path: GeometryPath,
    raster_plan: SurfaceRasterExecutionPlan,
    projected_policy: SurfaceProjectedDrawPolicy,
) -> Result<bool, SurfacePresenterError> {
    if current == next {
        return Ok(false);
    }
    if next == SurfaceGpuOrderProducer::Preproject
        && !gpu_producer_measurement_context_is_valid(geometry_path, raster_plan, projected_policy)
    {
        return Err(SurfacePresenterError::PreprojectProducerIncompatible);
    }
    Ok(true)
}

pub(crate) fn gpu_producer_measurement_context_is_valid(
    geometry_path: GeometryPath,
    raster_plan: SurfaceRasterExecutionPlan,
    projected_policy: SurfaceProjectedDrawPolicy,
) -> bool {
    geometry_path == GeometryPath::PackedAtlas
        && raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
        && projected_policy == SurfaceProjectedDrawPolicy::Compact
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdaptiveProbeOwner {
    Order,
    ProjectedCpu,
    ProjectedGpu,
}

impl AdaptiveProbeOwner {
    pub(crate) const fn projected(backend: SurfaceOrderBackendUsed) -> Self {
        match backend {
            SurfaceOrderBackendUsed::Cpu => Self::ProjectedCpu,
            SurfaceOrderBackendUsed::Gpu => Self::ProjectedGpu,
        }
    }
}

pub(crate) fn reset_adaptive_for_gpu_producer_measurement_transition(
    adaptive_policy: &mut AdaptiveOrderPolicy,
    projected_cpu: &mut AdaptiveProjectedDrawPolicy,
    projected_gpu: &mut AdaptiveProjectedDrawPolicy,
    owner: &mut Option<AdaptiveProbeOwner>,
    blocked_order_choice: &mut Option<AdaptiveRefreshChoice>,
) {
    adaptive_policy.reset(adaptive_primary_metric());
    projected_cpu.suspend_learning();
    projected_gpu.suspend_learning();
    *owner = None;
    *blocked_order_choice = None;
}

pub(crate) fn projected_policy_can_sample(
    owner: Option<AdaptiveProbeOwner>,
    lane_owner: AdaptiveProbeOwner,
    order_emits_sample: bool,
) -> bool {
    !order_emits_sample
        && match owner {
            None => true,
            Some(active) => active == lane_owner,
        }
}

pub(crate) const fn arbitrate_new_probe_owner(
    current: Option<AdaptiveProbeOwner>,
    order_wants_formal_sample: bool,
    projected_wants_formal_sample: bool,
    projected_owner: AdaptiveProbeOwner,
) -> Option<AdaptiveProbeOwner> {
    match current {
        Some(owner) => Some(owner),
        None if projected_wants_formal_sample => Some(projected_owner),
        None if order_wants_formal_sample => Some(AdaptiveProbeOwner::Order),
        None => None,
    }
}

pub(crate) const fn projected_formal_sample_requested(
    choice: ProjectedAdaptiveChoice,
    order_changed: bool,
) -> bool {
    !order_changed
        && matches!(
            choice.sample,
            Some(
                ProjectedAdaptiveSampleKind::CandidateBootstrap
                    | ProjectedAdaptiveSampleKind::Probe(_)
            )
        )
}

pub(crate) const fn projected_order_changed(
    refresh_sort: bool,
    upload_order: bool,
    actual_sort_refreshed: bool,
) -> bool {
    refresh_sort || upload_order || actual_sort_refreshed
}

pub(crate) const fn gpu_projected_order_changed(
    refresh_sort: bool,
    actual_sort_refreshed: bool,
) -> bool {
    // The deferred CPU upload bit does not describe GPU order ownership. A
    // GPU frame must not be denied a projected sample merely because a future
    // CPU switch still owes an upload.
    projected_order_changed(refresh_sort, false, actual_sort_refreshed)
}

pub(crate) const fn defer_projected_formal_choice(
    choice: ProjectedAdaptiveChoice,
    order_changed: bool,
) -> bool {
    order_changed
        && matches!(
            choice.sample,
            Some(
                ProjectedAdaptiveSampleKind::CandidateBootstrap
                    | ProjectedAdaptiveSampleKind::Probe(_)
            )
        )
}

pub(crate) const fn projected_probe_claims_owner(
    choice: ProjectedAdaptiveChoice,
    order_changed: bool,
    choice_was_pending: bool,
) -> bool {
    projected_formal_sample_requested(choice, order_changed)
        || (defer_projected_formal_choice(choice, order_changed) && !choice_was_pending)
}

pub(crate) const fn order_probe_owner_should_yield(
    owner: Option<AdaptiveProbeOwner>,
    order_pending: bool,
    refresh_sort: bool,
    order_wants_formal_sample: bool,
) -> bool {
    matches!(owner, Some(AdaptiveProbeOwner::Order))
        && !order_pending
        && !refresh_sort
        && !order_wants_formal_sample
}

pub(crate) const fn should_reset_order_for_projected_incumbent_change(
    order_backend: SurfaceOrderBackend,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    metric: AdaptiveMetric,
    order_pending: bool,
    projected_owner_finished: bool,
    projected_incumbent_changed: bool,
) -> bool {
    projected_owner_finished
        && projected_incumbent_changed
        && matches!(order_backend, SurfaceOrderBackend::Adaptive)
        && matches!(projected_draw_policy, SurfaceProjectedDrawPolicy::Adaptive)
        && matches!(metric, AdaptiveMetric::FrameCompletion)
        && !order_pending
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_policy_mapping_round_trips_every_closed_state() {
        for state in ExactSurfacePlanState::ALL {
            assert_eq!(ExactSurfacePlanState::from_policy(state.policy()), state);
        }
        assert_eq!(
            ExactSurfacePlanState::CpuPostSort.policy(),
            ExactPlanPolicy::Forced(PlanId::CpuPostSort)
        );
        assert_eq!(
            ExactSurfacePlanState::GpuPostSort.policy(),
            ExactPlanPolicy::Forced(PlanId::GpuPostSort)
        );
        assert_eq!(
            ExactSurfacePlanState::GpuPreproject.policy(),
            ExactPlanPolicy::Forced(PlanId::GpuPreproject)
        );
        assert_eq!(
            ExactSurfacePlanState::Adaptive.policy(),
            ExactPlanPolicy::Adaptive
        );
    }

    #[test]
    fn projected_formal_sample_never_claims_a_changed_order_as_evidence() {
        let choice = ProjectedAdaptiveChoice {
            execution: SurfaceProjectedDrawExecution::Compact,
            sample: Some(ProjectedAdaptiveSampleKind::Probe(0)),
        };
        assert!(!projected_formal_sample_requested(choice, true));
        assert!(defer_projected_formal_choice(choice, true));
        assert_eq!(
            arbitrate_new_probe_owner(None, true, true, AdaptiveProbeOwner::ProjectedGpu),
            Some(AdaptiveProbeOwner::ProjectedGpu)
        );
        assert!(!projected_policy_can_sample(
            Some(AdaptiveProbeOwner::Order),
            AdaptiveProbeOwner::ProjectedGpu,
            true
        ));
    }

    #[test]
    fn rejected_forced_projected_switch_preserves_all_control_state() {
        let mut current = SurfaceProjectedDrawPolicy::Candidate;
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        let mut owner = Some(AdaptiveProbeOwner::Order);
        let blocked = AdaptiveRefreshChoice {
            backend: SurfaceOrderBackendUsed::Cpu,
            sample: None,
        };
        let mut blocked_order_choice = Some(blocked);
        let mut pending_order_backend = Some(SurfaceOrderBackendUsed::Gpu);
        let mut pending_adaptive_choice = Some(blocked);
        let pending_projected = ProjectedAdaptiveChoice {
            execution: SurfaceProjectedDrawExecution::Candidate,
            sample: None,
        };
        let mut pending_projected_choice = Some(pending_projected);
        let before_order_state = order.state();
        let before_cpu = cpu.held_choice();
        let before_gpu = gpu.held_choice();

        assert!(matches!(
            commit_projected_draw_policy_transition(
                &mut current,
                SurfaceProjectedDrawPolicy::Compact,
                false,
                &mut order,
                &mut cpu,
                &mut gpu,
                &mut owner,
                &mut blocked_order_choice,
                &mut pending_order_backend,
                &mut pending_adaptive_choice,
                &mut pending_projected_choice,
            ),
            Err(SurfacePresenterError::ProjectedCompactionUnsupported)
        ));
        assert_eq!(current, SurfaceProjectedDrawPolicy::Candidate);
        assert_eq!(order.state(), before_order_state);
        assert_eq!(cpu.held_choice(), before_cpu);
        assert_eq!(gpu.held_choice(), before_gpu);
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));
        assert_eq!(blocked_order_choice, Some(blocked));
        assert_eq!(pending_order_backend, Some(SurfaceOrderBackendUsed::Gpu));
        assert_eq!(pending_adaptive_choice, Some(blocked));
        assert_eq!(pending_projected_choice, Some(pending_projected));
    }

    #[test]
    fn accepted_forced_projected_switch_clears_only_cross_policy_state() {
        let mut current = SurfaceProjectedDrawPolicy::Adaptive;
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        let mut owner = Some(AdaptiveProbeOwner::ProjectedCpu);
        let choice = AdaptiveRefreshChoice {
            backend: SurfaceOrderBackendUsed::Cpu,
            sample: None,
        };
        let mut blocked_order_choice = Some(choice);
        let mut pending_order_backend = Some(SurfaceOrderBackendUsed::Cpu);
        let mut pending_adaptive_choice = Some(choice);
        let mut pending_projected_choice = Some(ProjectedAdaptiveChoice {
            execution: SurfaceProjectedDrawExecution::Candidate,
            sample: None,
        });

        let changed = commit_projected_draw_policy_transition(
            &mut current,
            SurfaceProjectedDrawPolicy::Candidate,
            true,
            &mut order,
            &mut cpu,
            &mut gpu,
            &mut owner,
            &mut blocked_order_choice,
            &mut pending_order_backend,
            &mut pending_adaptive_choice,
            &mut pending_projected_choice,
        )
        .expect("supported forced policy transition");
        assert!(changed);
        assert_eq!(current, SurfaceProjectedDrawPolicy::Candidate);
        assert_eq!(owner, None);
        assert_eq!(blocked_order_choice, None);
        assert_eq!(pending_order_backend, None);
        assert_eq!(pending_adaptive_choice, None);
        assert_eq!(pending_projected_choice, None);
        assert_eq!(order.state(), crate::SurfaceAdaptiveState::CpuLearning);
    }
}
