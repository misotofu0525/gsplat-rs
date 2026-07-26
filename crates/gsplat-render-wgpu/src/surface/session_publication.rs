//! Presentation-fenced publication state for the shared Surface session.
//!
//! The session facade still polls telemetry and advances policy before choosing
//! a frame plan. This owner receives those already-consumed terminals, retains
//! them across unavailable or failed attempts, and publishes them only after a
//! successful present or an explicit receipt-only drain.

use gsplat_core::FrameStats;

use crate::evidence::SessionEvidence;
use crate::gpu_telemetry::SurfaceCpuOrderMeasurement;
use crate::{
    SurfaceCurrentStatsPoll, SurfaceCurrentStatsSubmission, SurfaceGpuOrderProducer,
    SurfaceGpuProducerMeasurement, SurfaceGpuProducerMeasurementFailure, SurfaceOrderBackend,
    SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure, SurfaceProjectedDrawMeasurement,
    SurfaceProjectedDrawMeasurementFailure, surface::LegacySurfaceStatsAvailability,
    surface_session::SurfaceFrameOutput,
};

#[derive(Default)]
pub(crate) struct SurfaceTelemetryBatch {
    cpu_order_completed: Vec<SurfaceCpuOrderMeasurement>,
    gpu_order_completed: Vec<SurfaceOrderMeasurement>,
    order_failures: Vec<SurfaceOrderMeasurementFailure>,
    projected_completed: Vec<SurfaceProjectedDrawMeasurement>,
    projected_failures: Vec<SurfaceProjectedDrawMeasurementFailure>,
    producer_completed: Vec<SurfaceGpuProducerMeasurement>,
    producer_failures: Vec<SurfaceGpuProducerMeasurementFailure>,
}

impl SurfaceTelemetryBatch {
    pub(crate) fn record_cpu_order(&mut self, measurement: SurfaceCpuOrderMeasurement) {
        self.cpu_order_completed.push(measurement);
    }

    pub(crate) fn record_gpu_order(&mut self, measurement: SurfaceOrderMeasurement) {
        self.gpu_order_completed.push(measurement);
    }

    pub(crate) fn record_order_failure(&mut self, failure: SurfaceOrderMeasurementFailure) {
        self.order_failures.push(failure);
    }

    pub(crate) fn record_projected_success(
        &mut self,
        measurement: SurfaceProjectedDrawMeasurement,
    ) {
        self.projected_completed.push(measurement);
    }

    pub(crate) fn record_projected_failure(
        &mut self,
        failure: SurfaceProjectedDrawMeasurementFailure,
    ) {
        self.projected_failures.push(failure);
    }

    pub(crate) fn record_producer_success(&mut self, measurement: SurfaceGpuProducerMeasurement) {
        self.producer_completed.push(measurement);
    }

    pub(crate) fn record_producer_failure(
        &mut self,
        failure: SurfaceGpuProducerMeasurementFailure,
    ) {
        self.producer_failures.push(failure);
    }

    fn append(&mut self, mut other: Self) {
        self.cpu_order_completed
            .append(&mut other.cpu_order_completed);
        self.gpu_order_completed
            .append(&mut other.gpu_order_completed);
        self.order_failures.append(&mut other.order_failures);
        self.projected_completed
            .append(&mut other.projected_completed);
        self.projected_failures
            .append(&mut other.projected_failures);
        self.producer_completed
            .append(&mut other.producer_completed);
        self.producer_failures.append(&mut other.producer_failures);
    }

    fn summary(&self) -> SurfaceTelemetryPublication {
        SurfaceTelemetryPublication {
            completed_order_measurement: self.gpu_order_completed.last().copied(),
            completed_order_measurement_failure: self.order_failures.last().copied(),
            completed_projected_draw_measurement: self.projected_completed.last().copied(),
            completed_projected_draw_measurement_failure: self.projected_failures.last().copied(),
            completed_gpu_producer_measurement: self.producer_completed.last().copied(),
            completed_gpu_producer_measurement_failure: self.producer_failures.last().copied(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SurfaceTelemetryPublication {
    pub(crate) completed_order_measurement: Option<SurfaceOrderMeasurement>,
    pub(crate) completed_order_measurement_failure: Option<SurfaceOrderMeasurementFailure>,
    pub(crate) completed_projected_draw_measurement: Option<SurfaceProjectedDrawMeasurement>,
    pub(crate) completed_projected_draw_measurement_failure:
        Option<SurfaceProjectedDrawMeasurementFailure>,
    pub(crate) completed_gpu_producer_measurement: Option<SurfaceGpuProducerMeasurement>,
    pub(crate) completed_gpu_producer_measurement_failure:
        Option<SurfaceGpuProducerMeasurementFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceFramePublicationOutcome {
    Presented,
    Unavailable,
    Error,
}

pub(crate) struct SessionPublication {
    current_stats_submission: SurfaceCurrentStatsSubmission,
    legacy_stats_availability: LegacySurfaceStatsAvailability,
    last_stats: FrameStats,
    pending_telemetry: SurfaceTelemetryBatch,
    evidence: SessionEvidence,
}

impl SessionPublication {
    pub(crate) fn new(exact_surface: bool) -> Self {
        Self {
            current_stats_submission: SurfaceCurrentStatsSubmission::NotRequested,
            legacy_stats_availability: if exact_surface {
                LegacySurfaceStatsAvailability::Unavailable
            } else {
                LegacySurfaceStatsAvailability::Current
            },
            last_stats: FrameStats::zero(),
            pending_telemetry: SurfaceTelemetryBatch::default(),
            evidence: SessionEvidence::new(),
        }
    }

    pub(crate) const fn current_stats_submission(&self) -> SurfaceCurrentStatsSubmission {
        self.current_stats_submission
    }

    pub(crate) const fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub(crate) fn legacy_stats(&self) -> Option<FrameStats> {
        self.legacy_stats_availability.get(self.last_stats)
    }

    pub(crate) fn observe_current_stats_poll(&mut self, poll: SurfaceCurrentStatsPoll) {
        if let SurfaceCurrentStatsPoll::Terminal(terminal) = poll {
            self.evidence.publish_exact_order_terminal(terminal);
        }
        self.legacy_stats_availability.observe_poll(
            self.current_stats_submission,
            poll,
            &mut self.last_stats,
        );
    }

    pub(crate) fn publish_exact_presented_frame(
        &mut self,
        current_stats_submission: SurfaceCurrentStatsSubmission,
        stats: FrameStats,
        counts_current: bool,
    ) {
        self.current_stats_submission = current_stats_submission;
        self.last_stats = stats;
        self.legacy_stats_availability = LegacySurfaceStatsAvailability::for_presented_frame(
            counts_current,
            current_stats_submission,
        );
    }

    pub(crate) fn publish_presented_stats(&mut self, stats: FrameStats) {
        self.last_stats = stats;
    }

    pub(crate) fn retain_consumed_telemetry(&mut self, telemetry: SurfaceTelemetryBatch) {
        self.pending_telemetry.append(telemetry);
    }

    pub(crate) fn finish_telemetry_attempt(
        &mut self,
        outcome: SurfaceFramePublicationOutcome,
    ) -> Option<SurfaceTelemetryPublication> {
        if outcome != SurfaceFramePublicationOutcome::Presented {
            return None;
        }
        let batch = std::mem::take(&mut self.pending_telemetry);
        let publication = batch.summary();
        self.publish_telemetry(batch);
        Some(publication)
    }

    pub(crate) fn publish_polled_telemetry(&mut self, telemetry: SurfaceTelemetryBatch) {
        self.pending_telemetry.append(telemetry);
        let batch = std::mem::take(&mut self.pending_telemetry);
        self.publish_telemetry(batch);
    }

    pub(crate) fn discard_deferred_telemetry(&mut self) {
        self.pending_telemetry = SurfaceTelemetryBatch::default();
    }

    pub(crate) fn observe_presented_frame(
        &mut self,
        output: SurfaceFrameOutput,
        requested_backend: SurfaceOrderBackend,
        requested_producer: SurfaceGpuOrderProducer,
        producer_measurement_enabled: bool,
    ) -> bool {
        if !output.frame_presented {
            return false;
        }
        self.evidence.observe_frame_output(
            output,
            requested_backend,
            requested_producer,
            producer_measurement_enabled,
        );
        true
    }

    fn publish_telemetry(&mut self, batch: SurfaceTelemetryBatch) {
        for measurement in batch.cpu_order_completed {
            self.evidence.publish_cpu_order(measurement);
        }
        for measurement in batch.gpu_order_completed {
            self.evidence.publish_gpu_order(measurement);
        }
        for failure in batch.order_failures {
            self.evidence.publish_order_failure(failure);
        }
        for measurement in batch.projected_completed {
            self.evidence.publish_projected_success(measurement);
        }
        for failure in batch.projected_failures {
            self.evidence.publish_projected_failure(failure);
        }
        for measurement in batch.producer_completed {
            self.evidence.publish_producer_success(measurement);
        }
        for failure in batch.producer_failures {
            self.evidence.publish_producer_failure(failure);
        }
    }

    pub(crate) fn evidence(&self) -> &SessionEvidence {
        &self.evidence
    }

    pub(crate) fn evidence_mut(&mut self) -> &mut SessionEvidence {
        &mut self.evidence
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionPublication, SurfaceFramePublicationOutcome, SurfaceTelemetryBatch};
    use crate::gpu_telemetry::SurfaceCpuOrderMeasurement;
    use crate::{
        SurfaceCompatibilityOrderSubmission, SurfaceCompatibilityProducerSubmission,
        SurfaceCompatibilityProjectedSubmission, SurfaceCompatibilityTerminal,
        SurfaceCompatibilityTerminalPoll, SurfaceCompatibilityTerminalSelector,
        SurfaceCompatibilityTerminalUnavailable, SurfaceGpuOrderProducer,
        SurfaceGpuProducerMeasurementSubmission, SurfaceOrderBackend, SurfaceOrderBackendUsed,
        SurfaceOrderMeasurementSubmission, SurfaceProjectedDrawAdaptiveState,
        SurfaceProjectedDrawExecution, SurfaceProjectedDrawMeasurementSubmission,
        SurfaceProjectedDrawPolicy, surface_session::SurfaceAdaptiveState,
    };

    fn cpu_measurement(ticket: u64) -> SurfaceCpuOrderMeasurement {
        SurfaceCpuOrderMeasurement {
            ticket,
            camera_revision: 7,
            preprocess_ms: 1.0,
            sort_ms: 2.0,
            frame_complete_ms: 3.0,
            visible_count: 11,
            contributor_count: 9,
            drawn_count: 9,
            exact_contributor_compaction: true,
        }
    }

    fn publication_with_issued_cpu_ticket(ticket: u64) -> SessionPublication {
        let mut publication = SessionPublication::new(false);
        publication.evidence.observe_submissions(
            SurfaceCompatibilityOrderSubmission {
                camera_revision: 7,
                requested_backend: SurfaceOrderBackend::Cpu,
                actual_backend: SurfaceOrderBackendUsed::Cpu,
                adaptive_state: SurfaceAdaptiveState::Disabled,
                measurement: SurfaceOrderMeasurementSubmission::Issued {
                    backend: SurfaceOrderBackendUsed::Cpu,
                    ticket,
                },
            },
            SurfaceCompatibilityProjectedSubmission {
                camera_revision: 7,
                requested_policy: SurfaceProjectedDrawPolicy::Candidate,
                actual_execution: SurfaceProjectedDrawExecution::Candidate,
                order_backend: SurfaceOrderBackendUsed::Cpu,
                adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
                measurement: SurfaceProjectedDrawMeasurementSubmission::NotRequested,
            },
            SurfaceCompatibilityProducerSubmission {
                camera_revision: 7,
                requested_producer: SurfaceGpuOrderProducer::PostSort,
                actual_producer: None,
                order_backend: SurfaceOrderBackendUsed::Cpu,
                projected_execution: SurfaceProjectedDrawExecution::Candidate,
                measurement_enabled: false,
                measurement: SurfaceGpuProducerMeasurementSubmission::NotRequested,
            },
        );
        publication
    }

    #[test]
    fn unavailable_and_error_retain_terminals_until_present_publication() {
        let ticket = 17;
        let mut publication = publication_with_issued_cpu_ticket(ticket);
        let mut batch = SurfaceTelemetryBatch::default();
        batch.record_cpu_order(cpu_measurement(ticket));
        publication.retain_consumed_telemetry(batch);

        assert!(
            publication
                .finish_telemetry_attempt(SurfaceFramePublicationOutcome::Unavailable)
                .is_none()
        );
        assert!(
            publication
                .finish_telemetry_attempt(SurfaceFramePublicationOutcome::Error)
                .is_none()
        );
        assert_eq!(
            publication
                .evidence
                .poll_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess),
            SurfaceCompatibilityTerminalPoll::Unavailable(
                SurfaceCompatibilityTerminalUnavailable::Empty
            )
        );

        assert!(
            publication
                .finish_telemetry_attempt(SurfaceFramePublicationOutcome::Presented)
                .is_some()
        );
        assert!(matches!(
            publication
                .evidence
                .poll_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess),
            SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::OrderCpuSuccess(
                _
            ))
        ));
    }

    #[test]
    fn reset_discards_deferred_terminals_before_a_new_path_or_raster_owner() {
        let ticket = 23;
        let mut publication = publication_with_issued_cpu_ticket(ticket);
        let mut batch = SurfaceTelemetryBatch::default();
        batch.record_cpu_order(cpu_measurement(ticket));
        publication.retain_consumed_telemetry(batch);
        publication.discard_deferred_telemetry();

        assert!(
            publication
                .finish_telemetry_attempt(SurfaceFramePublicationOutcome::Presented)
                .is_some()
        );
        assert_eq!(
            publication
                .evidence
                .poll_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess),
            SurfaceCompatibilityTerminalPoll::Unavailable(
                SurfaceCompatibilityTerminalUnavailable::Empty
            )
        );
    }
}
