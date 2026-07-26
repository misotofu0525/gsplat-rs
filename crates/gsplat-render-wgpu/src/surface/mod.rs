mod adaptive_order;
mod capture;
mod configuration;
mod current_stats;
mod lifecycle;
mod projected_adaptive;
mod session_control;
mod session_owner;
pub(crate) mod shadow;
pub(crate) mod standalone_direct_runtime;
pub(crate) mod standalone_paged_runtime;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use capture::SurfaceCapture;
pub use capture::SurfaceFrameCapture;
pub(crate) use configuration::SurfaceConfigurationOwner;
pub(crate) use current_stats::LegacySurfaceStatsAvailability;
pub use current_stats::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsCounts, SurfaceCurrentStatsFailure,
    SurfaceCurrentStatsFrameIdentity, SurfaceCurrentStatsJoinIdentity, SurfaceCurrentStatsPlan,
    SurfaceCurrentStatsPoll, SurfaceCurrentStatsReceipt, SurfaceCurrentStatsRequest,
    SurfaceCurrentStatsSubmission, SurfaceCurrentStatsSubmissionReceipt,
    SurfaceCurrentStatsTerminal, SurfaceCurrentStatsUnsampledReason,
};
pub(crate) use lifecycle::{SurfaceLifecycle, create_surface_instance, select_present_mode};
pub(crate) use session_control::{
    AdaptiveProbeOwner, ExactSurfacePlanState, arbitrate_new_probe_owner, commit_exact_plan_state,
    commit_projected_draw_policy_transition, defer_projected_formal_choice,
    gpu_producer_measurement_context_is_valid, gpu_projected_order_changed,
    order_probe_owner_should_yield, prepare_exact_gpu_order, prepare_exact_gpu_order_producer,
    projected_order_changed, projected_policy_can_sample, projected_probe_claims_owner,
    reset_adaptive_for_gpu_producer_measurement_transition,
    should_reset_order_for_projected_incumbent_change, validate_gpu_order_producer_transition,
};
#[cfg(test)]
pub(crate) use session_control::{
    exact_gpu_plan_for_producer, projected_formal_sample_requested,
    validate_projected_draw_policy_transition,
};
pub(crate) use session_owner::SessionSurfaceOwner;

#[cfg(test)]
pub(crate) use adaptive_order::{ADAPTIVE_CPU_BOOTSTRAP_SAMPLES, ADAPTIVE_INITIAL_PROBE_DELAY};
pub(crate) use adaptive_order::{
    AdaptiveMetric, AdaptiveOrderPolicy, AdaptiveRefreshChoice, AdaptiveSampleKind,
    adaptive_primary_metric,
};
pub use adaptive_order::{
    SurfaceAdaptiveGpuFailureReason, SurfaceAdaptivePendingSample, SurfaceAdaptiveState,
};
pub(crate) use projected_adaptive::{
    AdaptiveProjectedDrawPolicy, ProjectedAdaptiveChoice, ProjectedAdaptiveSampleKind,
};
pub use projected_adaptive::{
    SurfaceProjectedDrawAdaptivePendingSample, SurfaceProjectedDrawAdaptiveState,
};
