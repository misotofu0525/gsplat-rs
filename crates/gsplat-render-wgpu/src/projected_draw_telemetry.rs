//! Compatibility types for renderer-owned Exact projected-draw evidence.
//!
//! The standalone presenter no longer owns projected Candidate/Compact GPU
//! resources or a readback ring. Product receipts are published by the
//! renderer-owned Exact runtime and retained through the compatibility
//! evidence store.

pub use crate::api::SurfaceProjectedDrawExecution;
pub use crate::evidence::{
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
    SurfaceProjectedDrawMeasurementFailureReason,
};

pub(crate) struct ProjectedDrawTelemetryPoll {
    pub(crate) completed: Vec<SurfaceProjectedDrawMeasurement>,
    pub(crate) failures: Vec<SurfaceProjectedDrawMeasurementFailure>,
}
