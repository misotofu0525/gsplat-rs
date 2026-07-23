use crate::api::{SurfaceOrderBackendUsed, SurfaceProjectedDrawExecution};

/// Completion measurement for one projected draw execution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceProjectedDrawMeasurement {
    pub ticket: u64,
    pub camera_revision: u64,
    pub execution: SurfaceProjectedDrawExecution,
    pub order_backend: SurfaceOrderBackendUsed,
    /// Monotonic generation of the projected cache contents consumed by this
    /// presented frame.
    pub projection_generation: u64,
    /// Explicit probe generation included in the cache key. A change proves
    /// the sample could not reuse an earlier ABBA mode's projection.
    pub probe_generation: u64,
    pub projection_rebuilt: bool,
    pub order_refreshed: bool,
    pub frame_complete_ms: f32,
    /// Visibility-stage candidates V.
    pub visible_count: u32,
    /// Exact post-projection contributors C.
    pub contributor_count: u32,
    /// Instances issued by the selected projected draw D.
    pub drawn_count: u32,
    /// True only for stable contributor compaction (`D == C`).
    pub exact_contributor_compaction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceProjectedDrawMeasurementFailureReason {
    ReadbackMap,
    GenerationInvalidated,
    /// V/C/D or completion timing violated the execution contract, so the
    /// sample is terminal but must never enter an adaptive policy.
    InvariantViolation,
}

/// Terminal failure for a projected draw ticket returned by `arm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceProjectedDrawMeasurementFailure {
    pub ticket: u64,
    pub camera_revision: u64,
    pub execution: SurfaceProjectedDrawExecution,
    pub order_backend: SurfaceOrderBackendUsed,
    pub projection_generation: u64,
    pub probe_generation: u64,
    pub reason: SurfaceProjectedDrawMeasurementFailureReason,
}
