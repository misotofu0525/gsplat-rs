use crate::api::SurfaceGpuOrderProducer;

/// Meaning of the issued draw count for one producer frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceGpuProducerDrawScope {
    /// The order was refreshed for this camera and `D == C`.
    ExactCurrentContributors,
    /// Projection/count are current, but the draw consumes the previous
    /// refresh's stable order prefix. `D` must not be presented as current C.
    StaleOrderCandidates,
}

/// Queue-terminal receipt for one presented Packed GPU-producer frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceGpuProducerMeasurement {
    pub ticket: u64,
    pub camera_revision: u64,
    pub producer: SurfaceGpuOrderProducer,
    pub order_generation: u64,
    pub projection_generation: u64,
    pub source_count: u32,
    /// Contributor count computed from this frame's camera. For the
    /// preproject producer this is always scanned from the complete S pass,
    /// including non-refresh frames.
    pub contributor_count: u32,
    /// Instances in the indirect draw actually issued by this frame.
    pub drawn_count: u32,
    pub order_refreshed: bool,
    pub draw_scope: SurfaceGpuProducerDrawScope,
    pub frame_complete_ms: f32,
}

impl SurfaceGpuProducerMeasurement {
    pub const fn exact_current_contributor_draw(self) -> bool {
        matches!(
            self.draw_scope,
            SurfaceGpuProducerDrawScope::ExactCurrentContributors
        )
    }

    pub const fn stale_order(self) -> bool {
        matches!(
            self.draw_scope,
            SurfaceGpuProducerDrawScope::StaleOrderCandidates
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceGpuProducerMeasurementFailureReason {
    ReadbackMap,
    GenerationInvalidated,
    InvariantViolation,
}

/// Terminal failure for an already exposed producer ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceGpuProducerMeasurementFailure {
    pub ticket: u64,
    pub camera_revision: u64,
    pub producer: SurfaceGpuOrderProducer,
    pub order_generation: u64,
    pub projection_generation: u64,
    pub reason: SurfaceGpuProducerMeasurementFailureReason,
}

#[cfg(test)]
mod tests {
    use super::{
        SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
        SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
    };
    use crate::SurfaceGpuOrderProducer;

    #[test]
    fn producer_receipts_remain_available_at_compatibility_paths() {
        let _: crate::gpu_producer_telemetry::SurfaceGpuOrderProducer =
            SurfaceGpuOrderProducer::PostSort;
        let _: crate::SurfaceGpuProducerDrawScope =
            SurfaceGpuProducerDrawScope::ExactCurrentContributors;
        let _: crate::gpu_producer_telemetry::SurfaceGpuProducerDrawScope =
            SurfaceGpuProducerDrawScope::StaleOrderCandidates;
        let _: Option<crate::SurfaceGpuProducerMeasurement> = None;
        let _: Option<crate::gpu_producer_telemetry::SurfaceGpuProducerMeasurement> = None;
        let _: Option<crate::SurfaceGpuProducerMeasurementFailure> = None;
        let _: Option<crate::gpu_producer_telemetry::SurfaceGpuProducerMeasurementFailure> = None;
        let _: crate::SurfaceGpuProducerMeasurementFailureReason =
            SurfaceGpuProducerMeasurementFailureReason::ReadbackMap;
        let _: crate::gpu_producer_telemetry::SurfaceGpuProducerMeasurementFailureReason =
            SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated;

        let measurement = SurfaceGpuProducerMeasurement {
            ticket: 1,
            camera_revision: 2,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: 3,
            projection_generation: 4,
            source_count: 5,
            contributor_count: 4,
            drawn_count: 4,
            order_refreshed: true,
            draw_scope: SurfaceGpuProducerDrawScope::ExactCurrentContributors,
            frame_complete_ms: 6.0,
        };
        let failure = SurfaceGpuProducerMeasurementFailure {
            ticket: measurement.ticket,
            camera_revision: measurement.camera_revision,
            producer: measurement.producer,
            order_generation: measurement.order_generation,
            projection_generation: measurement.projection_generation,
            reason: SurfaceGpuProducerMeasurementFailureReason::InvariantViolation,
        };

        assert!(measurement.exact_current_contributor_draw());
        assert!(!measurement.stale_order());
        assert_eq!(failure.ticket, measurement.ticket);
    }
}
