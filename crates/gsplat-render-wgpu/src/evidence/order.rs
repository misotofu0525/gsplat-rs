#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceTimingSource {
    TimestampQuery,
    CompletionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceOrderMeasurement {
    pub ticket: u64,
    pub camera_revision: u64,
    pub timing_source: SurfaceTimingSource,
    pub gpu_preprocess_ms: Option<f32>,
    pub gpu_radix_ms: Option<f32>,
    pub gpu_order_ms: Option<f32>,
    pub gpu_complete_ms: f32,
    pub timestamp_period_ns: Option<f32>,
    pub below_timestamp_resolution: bool,
    /// Visibility-stage candidates V. This preserves the existing Rust/C ABI
    /// meaning of `visible_count`; post-projection filtering is reported
    /// separately instead of being disguised as source or resident count.
    pub visible_count: u32,
    /// Exact post-projection contributors C in stable candidate-rank order.
    pub contributor_count: u32,
    /// Instances actually issued to the selected draw D.
    pub drawn_count: u32,
    /// True only when the presenter issued the stable compacted contributor
    /// prefix, so `drawn_count == contributor_count` by construction.
    pub exact_contributor_compaction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceOrderMeasurementFailureReason {
    ReadbackMap,
    GenerationInvalidated,
}

/// Terminal failure receipt for an order measurement ticket that was returned
/// to the caller. Every issued CPU or GPU ticket eventually produces exactly
/// one success or failure receipt; an unsampled request never issues a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceOrderMeasurementFailure {
    pub ticket: u64,
    pub camera_revision: u64,
    pub reason: SurfaceOrderMeasurementFailureReason,
}

/// Completion receipt for one CPU-order refresh and the draw submission that
/// consumed it. The queue-complete interval is directly comparable with a GPU
/// completion sample and is also available to benchmark/ABI consumers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCpuOrderMeasurement {
    pub ticket: u64,
    pub camera_revision: u64,
    pub preprocess_ms: f32,
    pub sort_ms: f32,
    pub frame_complete_ms: f32,
    pub visible_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub exact_contributor_compaction: bool,
}
