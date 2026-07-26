mod adaptive_order;
mod capture;
mod configuration;
mod current_stats;
mod lifecycle;
mod projected_adaptive;
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
