//! Compatibility types for renderer-owned Exact GPU producer evidence.
//!
//! The standalone presenter no longer owns a Packed producer graph or a
//! readback ring. Product receipts are published by the renderer-owned Exact
//! runtime and retained through the compatibility evidence store.

pub use crate::api::SurfaceGpuOrderProducer;
pub use crate::evidence::{
    SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
};

pub(crate) struct GpuProducerTelemetryPoll {
    pub(crate) completed: Vec<SurfaceGpuProducerMeasurement>,
    pub(crate) failures: Vec<SurfaceGpuProducerMeasurementFailure>,
}
