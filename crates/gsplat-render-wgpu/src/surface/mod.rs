mod capture;
mod configuration;
mod current_stats;
mod lifecycle;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod shadow;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use capture::SurfaceCapture;
pub use capture::SurfaceFrameCapture;
pub(crate) use configuration::SurfaceConfigurationOwner;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use current_stats::LegacySurfaceStatsAvailability;
pub use current_stats::{
    SurfaceCurrentStatsCountSemantics, SurfaceCurrentStatsCounts, SurfaceCurrentStatsFailure,
    SurfaceCurrentStatsFrameIdentity, SurfaceCurrentStatsJoinIdentity, SurfaceCurrentStatsPlan,
    SurfaceCurrentStatsPoll, SurfaceCurrentStatsReceipt, SurfaceCurrentStatsRequest,
    SurfaceCurrentStatsSubmission, SurfaceCurrentStatsSubmissionReceipt,
    SurfaceCurrentStatsTerminal, SurfaceCurrentStatsUnsampledReason,
};
pub(crate) use lifecycle::{SurfaceLifecycle, create_surface_instance, select_present_mode};
