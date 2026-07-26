//! Presentation-fenced publication state for the shared Surface session.
//!
//! The session facade still polls telemetry and advances policy before choosing
//! a frame plan. This evidence-layer owner receives already-consumed terminals,
//! retains them across unavailable or failed attempts, and commits one complete
//! immutable DTO only after a successful primitive present. Explicit receipt
//! drains and successful owner transitions terminalize retained tickets without
//! publishing a semantic frame.

use gsplat_core::FrameStats;

use super::{
    SessionEvidence, SurfaceCompatibilityOrderSubmission, SurfaceCompatibilityProducerSubmission,
    SurfaceCompatibilityProjectedSubmission,
};
use crate::gpu_telemetry::SurfaceCpuOrderMeasurement;
use crate::plans::{FrameIdentity, PlanId};
use crate::renderer::{ProjectedCachePrecisionProfile, SurfaceDepthPrecisionProfile};
use crate::{
    SurfaceCurrentStatsPoll, SurfaceCurrentStatsSubmission, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
};

/// Renderer-observed depth precision joined to one successfully presented
/// Packed Exact frame. This is private evidence: later collectors must still
/// join it to the existing presented capture and Balanced image gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PresentedDepthPrecisionReceipt {
    profile: SurfaceDepthPrecisionProfile,
    frame: FrameIdentity,
    plan: PlanId,
    order_generation: u64,
    presentation_sequence: u64,
}

#[cfg_attr(not(test), allow(dead_code))]
impl PresentedDepthPrecisionReceipt {
    pub(crate) const fn new(
        profile: SurfaceDepthPrecisionProfile,
        frame: FrameIdentity,
        plan: PlanId,
        order_generation: u64,
        presentation_sequence: u64,
    ) -> Self {
        Self {
            profile,
            frame,
            plan,
            order_generation,
            presentation_sequence,
        }
    }

    pub(crate) const fn profile(self) -> SurfaceDepthPrecisionProfile {
        self.profile
    }

    pub(crate) const fn frame(self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn plan(self) -> PlanId {
        self.plan
    }

    pub(crate) const fn order_generation(self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn presentation_sequence(self) -> u64 {
        self.presentation_sequence
    }
}

/// Renderer-realized projected-cache precision joined to one successfully
/// presented Packed Exact frame. Construction intent alone cannot create this
/// private receipt: the profile comes from the admitted GPU preparation
/// receipt and enters publication only with the presented frame DTO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PresentedProjectedCachePrecisionReceipt {
    profile: ProjectedCachePrecisionProfile,
    frame: FrameIdentity,
    plan: PlanId,
    order_generation: u64,
    presentation_sequence: u64,
}

#[cfg_attr(not(test), allow(dead_code))]
impl PresentedProjectedCachePrecisionReceipt {
    pub(crate) const fn new(
        profile: ProjectedCachePrecisionProfile,
        frame: FrameIdentity,
        plan: PlanId,
        order_generation: u64,
        presentation_sequence: u64,
    ) -> Self {
        Self {
            profile,
            frame,
            plan,
            order_generation,
            presentation_sequence,
        }
    }

    pub(crate) const fn profile(self) -> ProjectedCachePrecisionProfile {
        self.profile
    }

    pub(crate) const fn frame(self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn plan(self) -> PlanId {
        self.plan
    }

    pub(crate) const fn order_generation(self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn presentation_sequence(self) -> u64 {
        self.presentation_sequence
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PresentedFramePrecisionReceipts {
    depth: Option<PresentedDepthPrecisionReceipt>,
    projected_cache: Option<PresentedProjectedCachePrecisionReceipt>,
}

impl PresentedFramePrecisionReceipts {
    pub(crate) const fn new(
        depth: Option<PresentedDepthPrecisionReceipt>,
        projected_cache: Option<PresentedProjectedCachePrecisionReceipt>,
    ) -> Self {
        Self {
            depth,
            projected_cache,
        }
    }
}

/// Capture evidence admitted only when the Surface lifecycle and renderer
/// receipt identify the same successful presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PresentedCaptureDepthPrecisionReceipt {
    depth_precision: PresentedDepthPrecisionReceipt,
}

impl PresentedCaptureDepthPrecisionReceipt {
    const fn join(
        presentation_sequence: u64,
        depth_precision: PresentedDepthPrecisionReceipt,
    ) -> Option<Self> {
        if presentation_sequence != depth_precision.presentation_sequence() {
            return None;
        }
        Some(Self { depth_precision })
    }

    const fn depth_precision(self) -> PresentedDepthPrecisionReceipt {
        self.depth_precision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureDepthPrecisionState {
    Disabled,
    Idle,
    Armed,
    Presented(PresentedCaptureDepthPrecisionReceipt),
    Unavailable,
}

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
}

pub(crate) struct PresentedTelemetry {
    batch: SurfaceTelemetryBatch,
    pub(crate) completed_order_measurement: Option<SurfaceOrderMeasurement>,
    pub(crate) completed_order_measurement_failure: Option<SurfaceOrderMeasurementFailure>,
    pub(crate) completed_projected_draw_measurement: Option<SurfaceProjectedDrawMeasurement>,
    pub(crate) completed_projected_draw_measurement_failure:
        Option<SurfaceProjectedDrawMeasurementFailure>,
    pub(crate) completed_gpu_producer_measurement: Option<SurfaceGpuProducerMeasurement>,
    pub(crate) completed_gpu_producer_measurement_failure:
        Option<SurfaceGpuProducerMeasurementFailure>,
}

impl PresentedTelemetry {
    fn from_batch(batch: SurfaceTelemetryBatch) -> Self {
        Self {
            completed_order_measurement: batch.gpu_order_completed.last().copied(),
            completed_order_measurement_failure: batch.order_failures.last().copied(),
            completed_projected_draw_measurement: batch.projected_completed.last().copied(),
            completed_projected_draw_measurement_failure: batch.projected_failures.last().copied(),
            completed_gpu_producer_measurement: batch.producer_completed.last().copied(),
            completed_gpu_producer_measurement_failure: batch.producer_failures.last().copied(),
            batch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentedCurrentStats {
    Preserve,
    Exact {
        submission: SurfaceCurrentStatsSubmission,
        counts_current: bool,
    },
}

pub(crate) struct PresentedFramePublication {
    stats: FrameStats,
    current_stats: PresentedCurrentStats,
    precision: PresentedFramePrecisionReceipts,
    order: SurfaceCompatibilityOrderSubmission,
    projected: SurfaceCompatibilityProjectedSubmission,
    producer: SurfaceCompatibilityProducerSubmission,
    telemetry: PresentedTelemetry,
}

impl PresentedFramePublication {
    pub(crate) const fn new(
        stats: FrameStats,
        current_stats: PresentedCurrentStats,
        precision: PresentedFramePrecisionReceipts,
        order: SurfaceCompatibilityOrderSubmission,
        projected: SurfaceCompatibilityProjectedSubmission,
        producer: SurfaceCompatibilityProducerSubmission,
        telemetry: PresentedTelemetry,
    ) -> Self {
        Self {
            stats,
            current_stats,
            precision,
            order,
            projected,
            producer,
            telemetry,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacySurfaceStatsAvailability {
    Current,
    AwaitingCurrentReceipt,
    Unavailable,
}

impl LegacySurfaceStatsAvailability {
    pub(crate) const fn for_presented_frame(
        counts_are_synchronous: bool,
        submission: SurfaceCurrentStatsSubmission,
    ) -> Self {
        if counts_are_synchronous {
            Self::Current
        } else if matches!(submission, SurfaceCurrentStatsSubmission::Issued(_)) {
            Self::AwaitingCurrentReceipt
        } else {
            Self::Unavailable
        }
    }

    pub(crate) fn observe_poll(
        &mut self,
        submission: SurfaceCurrentStatsSubmission,
        poll: SurfaceCurrentStatsPoll,
        stats: &mut FrameStats,
    ) {
        if *self != Self::AwaitingCurrentReceipt {
            return;
        }
        let Some(expected) = submission.receipt() else {
            *self = Self::Unavailable;
            return;
        };
        let SurfaceCurrentStatsPoll::Terminal(terminal) = poll else {
            return;
        };
        if terminal.submission() != expected {
            return;
        }

        match terminal {
            crate::SurfaceCurrentStatsTerminal::Ready(receipt) => {
                let counts = receipt.counts();
                stats.visible_count = counts.visible();
                stats.drawn_count = counts.drawn();
                *self = Self::Current;
            }
            crate::SurfaceCurrentStatsTerminal::MapFailure(_)
            | crate::SurfaceCurrentStatsTerminal::GenerationInvalidated(_)
            | crate::SurfaceCurrentStatsTerminal::Expired(_)
            | crate::SurfaceCurrentStatsTerminal::Dropped(_) => {
                *self = Self::Unavailable;
            }
        }
    }

    pub(crate) const fn get(self, stats: FrameStats) -> Option<FrameStats> {
        match self {
            Self::Current => Some(stats),
            Self::AwaitingCurrentReceipt | Self::Unavailable => None,
        }
    }
}

pub(crate) struct SessionPublication {
    current_stats_submission: SurfaceCurrentStatsSubmission,
    legacy_stats_availability: LegacySurfaceStatsAvailability,
    last_stats: FrameStats,
    presented_depth_precision: Option<PresentedDepthPrecisionReceipt>,
    presented_projected_cache_precision: Option<PresentedProjectedCachePrecisionReceipt>,
    capture_depth_precision: CaptureDepthPrecisionState,
    pending_telemetry: SurfaceTelemetryBatch,
    evidence: SessionEvidence,
    #[cfg(test)]
    presented_commit_count: u64,
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
            presented_depth_precision: None,
            presented_projected_cache_precision: None,
            capture_depth_precision: if exact_surface {
                CaptureDepthPrecisionState::Idle
            } else {
                CaptureDepthPrecisionState::Disabled
            },
            pending_telemetry: SurfaceTelemetryBatch::default(),
            evidence: SessionEvidence::new(),
            #[cfg(test)]
            presented_commit_count: 0,
        }
    }

    pub(crate) const fn current_stats_submission(&self) -> SurfaceCurrentStatsSubmission {
        self.current_stats_submission
    }

    pub(crate) const fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub(crate) const fn presented_depth_precision_receipt(
        &self,
    ) -> Option<PresentedDepthPrecisionReceipt> {
        self.presented_depth_precision
    }

    #[cfg(test)]
    pub(crate) const fn presented_projected_cache_precision_receipt(
        &self,
    ) -> Option<PresentedProjectedCachePrecisionReceipt> {
        self.presented_projected_cache_precision
    }

    pub(crate) fn arm_capture_depth_precision(&mut self) -> bool {
        match self.capture_depth_precision {
            CaptureDepthPrecisionState::Idle => {
                self.capture_depth_precision = CaptureDepthPrecisionState::Armed;
                true
            }
            CaptureDepthPrecisionState::Disabled => true,
            CaptureDepthPrecisionState::Armed
            | CaptureDepthPrecisionState::Presented(_)
            | CaptureDepthPrecisionState::Unavailable => false,
        }
    }

    pub(crate) fn cancel_capture_depth_precision(&mut self) {
        if self.capture_depth_precision != CaptureDepthPrecisionState::Disabled {
            self.capture_depth_precision = CaptureDepthPrecisionState::Idle;
        }
    }

    pub(crate) fn observe_presented_capture(
        &mut self,
        presentation_sequence: Option<u64>,
        depth_precision: Option<PresentedDepthPrecisionReceipt>,
    ) {
        if self.capture_depth_precision == CaptureDepthPrecisionState::Armed {
            let joined =
                presentation_sequence
                    .zip(depth_precision)
                    .and_then(|(sequence, receipt)| {
                        PresentedCaptureDepthPrecisionReceipt::join(sequence, receipt)
                    });
            self.capture_depth_precision = joined.map_or(
                CaptureDepthPrecisionState::Unavailable,
                CaptureDepthPrecisionState::Presented,
            );
        }
    }

    pub(crate) fn take_capture_depth_precision(
        &mut self,
    ) -> Option<PresentedDepthPrecisionReceipt> {
        let previous = std::mem::replace(
            &mut self.capture_depth_precision,
            CaptureDepthPrecisionState::Idle,
        );
        match previous {
            CaptureDepthPrecisionState::Presented(receipt) => Some(receipt.depth_precision()),
            CaptureDepthPrecisionState::Disabled => {
                self.capture_depth_precision = CaptureDepthPrecisionState::Disabled;
                None
            }
            CaptureDepthPrecisionState::Idle
            | CaptureDepthPrecisionState::Armed
            | CaptureDepthPrecisionState::Unavailable => None,
        }
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

    pub(crate) fn retain_consumed_telemetry(&mut self, telemetry: SurfaceTelemetryBatch) {
        self.pending_telemetry.append(telemetry);
    }

    pub(crate) fn prepare_presented_telemetry(&mut self) -> PresentedTelemetry {
        PresentedTelemetry::from_batch(std::mem::take(&mut self.pending_telemetry))
    }

    pub(crate) fn publish_presented_frame(&mut self, publication: PresentedFramePublication) {
        let PresentedFramePublication {
            stats,
            current_stats,
            precision,
            order,
            projected,
            producer,
            telemetry,
        } = publication;
        self.last_stats = stats;
        if let Some(depth_precision) = precision.depth {
            self.presented_depth_precision = Some(depth_precision);
        }
        if let Some(projected_cache_precision) = precision.projected_cache {
            self.presented_projected_cache_precision = Some(projected_cache_precision);
        }
        if let PresentedCurrentStats::Exact {
            submission,
            counts_current,
        } = current_stats
        {
            self.current_stats_submission = submission;
            self.legacy_stats_availability =
                LegacySurfaceStatsAvailability::for_presented_frame(counts_current, submission);
        }
        // Retained terminals belong to submissions issued by earlier
        // presented frames. Resolve them against that old identity before the
        // new frame installs its submissions, so even an accidental numeric
        // ticket reuse cannot join an old terminal to the new frame.
        self.publish_telemetry(telemetry.batch);
        self.evidence
            .observe_submissions(order, projected, producer);
        #[cfg(test)]
        {
            self.presented_commit_count += 1;
        }
    }

    pub(crate) fn publish_polled_telemetry(&mut self, telemetry: SurfaceTelemetryBatch) {
        self.pending_telemetry.append(telemetry);
        self.terminalize_deferred_telemetry();
    }

    pub(crate) fn terminalize_deferred_telemetry(&mut self) {
        let batch = std::mem::take(&mut self.pending_telemetry);
        self.publish_telemetry(batch);
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

    #[cfg(test)]
    pub(crate) const fn presented_commit_count(&self) -> u64 {
        self.presented_commit_count
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        SurfaceAdaptiveState, SurfaceCompatibilityCountFamily, SurfaceCompatibilityCountsTake,
        SurfaceCompatibilityCountsUnavailableReason, SurfaceCompatibilityTerminal,
        SurfaceCompatibilityTerminalPoll, SurfaceCompatibilityTerminalSelector,
        SurfaceGpuOrderProducer, SurfaceGpuProducerDrawScope,
        SurfaceGpuProducerMeasurementFailureReason, SurfaceGpuProducerMeasurementSubmission,
        SurfaceOrderBackend, SurfaceOrderBackendUsed, SurfaceOrderMeasurementFailureReason,
        SurfaceOrderMeasurementSubmission, SurfaceProjectedDrawAdaptiveState,
        SurfaceProjectedDrawExecution, SurfaceProjectedDrawMeasurementFailureReason,
        SurfaceProjectedDrawMeasurementSubmission, SurfaceProjectedDrawPolicy,
    };

    fn submissions(
        ticket: u64,
    ) -> (
        SurfaceCompatibilityOrderSubmission,
        SurfaceCompatibilityProjectedSubmission,
        SurfaceCompatibilityProducerSubmission,
    ) {
        (
            SurfaceCompatibilityOrderSubmission {
                camera_revision: 7,
                requested_backend: SurfaceOrderBackend::Adaptive,
                actual_backend: SurfaceOrderBackendUsed::Cpu,
                adaptive_state: SurfaceAdaptiveState::CpuStable,
                measurement: SurfaceOrderMeasurementSubmission::Issued {
                    backend: SurfaceOrderBackendUsed::Cpu,
                    ticket,
                },
            },
            SurfaceCompatibilityProjectedSubmission {
                camera_revision: 7,
                requested_policy: SurfaceProjectedDrawPolicy::Adaptive,
                actual_execution: SurfaceProjectedDrawExecution::Compact,
                order_backend: SurfaceOrderBackendUsed::Cpu,
                adaptive_state: SurfaceProjectedDrawAdaptiveState::CompactStable,
                measurement: SurfaceProjectedDrawMeasurementSubmission::Issued {
                    execution: SurfaceProjectedDrawExecution::Compact,
                    ticket,
                },
            },
            SurfaceCompatibilityProducerSubmission {
                camera_revision: 7,
                requested_producer: SurfaceGpuOrderProducer::Preproject,
                actual_producer: Some(SurfaceGpuOrderProducer::Preproject),
                order_backend: SurfaceOrderBackendUsed::Cpu,
                projected_execution: SurfaceProjectedDrawExecution::Compact,
                measurement_enabled: true,
                measurement: SurfaceGpuProducerMeasurementSubmission::Issued {
                    producer: SurfaceGpuOrderProducer::Preproject,
                    ticket,
                },
            },
        )
    }

    fn cpu_success(ticket: u64) -> SurfaceCpuOrderMeasurement {
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

    fn projected_success(ticket: u64) -> SurfaceProjectedDrawMeasurement {
        SurfaceProjectedDrawMeasurement {
            ticket,
            camera_revision: 7,
            execution: SurfaceProjectedDrawExecution::Compact,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            projection_generation: 3,
            probe_generation: 4,
            projection_rebuilt: true,
            order_refreshed: false,
            frame_complete_ms: 5.0,
            visible_count: 11,
            contributor_count: 9,
            drawn_count: 9,
            exact_contributor_compaction: true,
        }
    }

    fn producer_success(ticket: u64) -> SurfaceGpuProducerMeasurement {
        SurfaceGpuProducerMeasurement {
            ticket,
            camera_revision: 7,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: 2,
            projection_generation: 3,
            source_count: 12,
            contributor_count: 9,
            drawn_count: 9,
            order_refreshed: false,
            draw_scope: SurfaceGpuProducerDrawScope::ExactCurrentContributors,
            frame_complete_ms: 5.0,
        }
    }

    fn ticket(ticket: u64) -> NonZeroU64 {
        NonZeroU64::new(ticket).expect("test ticket is non-zero")
    }

    #[test]
    fn transition_terminalizes_all_success_lanes_and_counts_are_take_once() {
        let issued = 17;
        let mut publication = SessionPublication::new(false);
        let (order, projected, producer) = submissions(issued);
        publication
            .evidence
            .observe_submissions(order, projected, producer);
        let mut batch = SurfaceTelemetryBatch::default();
        batch.record_cpu_order(cpu_success(issued));
        batch.record_projected_success(projected_success(issued));
        batch.record_producer_success(producer_success(issued));
        publication.retain_consumed_telemetry(batch);

        publication.terminalize_deferred_telemetry();

        for selector in [
            SurfaceCompatibilityTerminalSelector::OrderCpuSuccess,
            SurfaceCompatibilityTerminalSelector::ProjectedSuccess,
            SurfaceCompatibilityTerminalSelector::ProducerSuccess,
        ] {
            assert!(matches!(
                publication.evidence.poll_terminal(selector),
                SurfaceCompatibilityTerminalPoll::Ready(_)
            ));
        }
        for family in [
            SurfaceCompatibilityCountFamily::Order,
            SurfaceCompatibilityCountFamily::Projected,
        ] {
            assert!(matches!(
                publication.evidence.take_counts(family, ticket(issued)),
                SurfaceCompatibilityCountsTake::Ready(counts) if counts.ticket == issued
            ));
            assert!(matches!(
                publication.evidence.take_counts(family, ticket(issued)),
                SurfaceCompatibilityCountsTake::Unavailable(unavailable)
                    if unavailable.reason == SurfaceCompatibilityCountsUnavailableReason::Consumed
            ));
        }
    }

    #[test]
    fn transition_terminalizes_all_failure_lanes_and_fails_count_ledgers() {
        let issued = 23;
        let mut publication = SessionPublication::new(false);
        let (order, projected, producer) = submissions(issued);
        publication
            .evidence
            .observe_submissions(order, projected, producer);
        let mut batch = SurfaceTelemetryBatch::default();
        batch.record_order_failure(SurfaceOrderMeasurementFailure {
            ticket: issued,
            camera_revision: 7,
            reason: SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
        });
        batch.record_projected_failure(SurfaceProjectedDrawMeasurementFailure {
            ticket: issued,
            camera_revision: 7,
            execution: SurfaceProjectedDrawExecution::Compact,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            projection_generation: 3,
            probe_generation: 4,
            reason: SurfaceProjectedDrawMeasurementFailureReason::GenerationInvalidated,
        });
        batch.record_producer_failure(SurfaceGpuProducerMeasurementFailure {
            ticket: issued,
            camera_revision: 7,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: 2,
            projection_generation: 3,
            reason: SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated,
        });
        publication.retain_consumed_telemetry(batch);

        publication.terminalize_deferred_telemetry();

        for selector in [
            SurfaceCompatibilityTerminalSelector::OrderFailure,
            SurfaceCompatibilityTerminalSelector::ProjectedFailure,
            SurfaceCompatibilityTerminalSelector::ProducerFailure,
        ] {
            assert!(matches!(
                publication.evidence.poll_terminal(selector),
                SurfaceCompatibilityTerminalPoll::Ready(_)
            ));
        }
        for family in [
            SurfaceCompatibilityCountFamily::Order,
            SurfaceCompatibilityCountFamily::Projected,
        ] {
            assert!(matches!(
                publication.evidence.take_counts(family, ticket(issued)),
                SurfaceCompatibilityCountsTake::Unavailable(unavailable)
                    if unavailable.reason == SurfaceCompatibilityCountsUnavailableReason::Failed
            ));
        }
    }

    #[test]
    fn complete_dto_commits_once_and_keeps_old_terminal_off_new_submission() {
        let old_ticket = 31;
        let new_ticket = 32;
        let mut publication = SessionPublication::new(false);
        let (old_order, old_projected, old_producer) = submissions(old_ticket);
        publication
            .evidence
            .observe_submissions(old_order, old_projected, old_producer);
        let mut batch = SurfaceTelemetryBatch::default();
        batch.record_cpu_order(cpu_success(old_ticket));
        batch.record_projected_success(projected_success(old_ticket));
        batch.record_producer_success(producer_success(old_ticket));
        publication.retain_consumed_telemetry(batch);
        let telemetry = publication.prepare_presented_telemetry();
        let (new_order, new_projected, new_producer) = submissions(new_ticket);
        let stats = FrameStats {
            frame_ms: 1.0,
            preprocess_ms: 2.0,
            sort_ms: 3.0,
            raster_ms: 4.0,
            visible_count: 11,
            drawn_count: 9,
        };

        publication.publish_presented_frame(PresentedFramePublication::new(
            stats,
            PresentedCurrentStats::Preserve,
            PresentedFramePrecisionReceipts::default(),
            new_order,
            new_projected,
            new_producer,
            telemetry,
        ));

        assert_eq!(publication.presented_commit_count(), 1);
        assert_eq!(publication.last_stats(), stats);
        for selector in [
            SurfaceCompatibilityTerminalSelector::OrderCpuSuccess,
            SurfaceCompatibilityTerminalSelector::ProjectedSuccess,
            SurfaceCompatibilityTerminalSelector::ProducerSuccess,
        ] {
            let SurfaceCompatibilityTerminalPoll::Ready(terminal) =
                publication.evidence.poll_terminal(selector)
            else {
                panic!("old terminal must publish");
            };
            let published_ticket = match terminal {
                SurfaceCompatibilityTerminal::OrderCpuSuccess(success) => success.ticket,
                SurfaceCompatibilityTerminal::ProjectedSuccess(success) => success.ticket,
                SurfaceCompatibilityTerminal::ProducerSuccess(success) => success.ticket,
                _ => panic!("selector returned the wrong terminal family"),
            };
            assert_eq!(published_ticket, old_ticket);
        }
        for family in [
            SurfaceCompatibilityCountFamily::Order,
            SurfaceCompatibilityCountFamily::Projected,
        ] {
            assert!(matches!(
                publication.evidence.take_counts(family, ticket(new_ticket)),
                SurfaceCompatibilityCountsTake::Unavailable(unavailable)
                    if unavailable.reason == SurfaceCompatibilityCountsUnavailableReason::Pending
            ));
        }
    }

    fn depth_precision_receipt(
        profile: SurfaceDepthPrecisionProfile,
        camera_revision: u64,
        order_generation: u64,
        presentation_sequence: u64,
    ) -> PresentedDepthPrecisionReceipt {
        PresentedDepthPrecisionReceipt::new(
            profile,
            FrameIdentity::new(2, camera_revision, 3, 4, 5),
            PlanId::CpuPostSort,
            order_generation,
            presentation_sequence,
        )
    }

    fn publish_depth_precision_receipt(
        publication: &mut SessionPublication,
        receipt: PresentedDepthPrecisionReceipt,
    ) {
        let (order, projected, producer) = submissions(receipt.presentation_sequence());
        publication.publish_presented_frame(PresentedFramePublication::new(
            FrameStats::zero(),
            PresentedCurrentStats::Preserve,
            PresentedFramePrecisionReceipts::new(Some(receipt), None),
            order,
            projected,
            producer,
            PresentedTelemetry::from_batch(SurfaceTelemetryBatch::default()),
        ));
    }

    #[test]
    fn depth_precision_receipt_is_present_fenced_and_identity_bound() {
        let mut publication = SessionPublication::new(true);
        assert_eq!(publication.presented_depth_precision_receipt(), None);

        let candidate =
            depth_precision_receipt(SurfaceDepthPrecisionProfile::CandidateStable24, 7, 11, 13);
        // Unavailable and failed attempts have no presented DTO. Retained or
        // terminalized telemetry therefore cannot manufacture the receipt.
        publication.retain_consumed_telemetry(SurfaceTelemetryBatch::default());
        publication.terminalize_deferred_telemetry();
        assert_eq!(publication.presented_depth_precision_receipt(), None);

        publish_depth_precision_receipt(&mut publication, candidate);
        let published = publication
            .presented_depth_precision_receipt()
            .expect("successful present publishes the depth precision receipt");
        assert_eq!(
            published.profile(),
            SurfaceDepthPrecisionProfile::CandidateStable24
        );
        assert_eq!(published.frame().camera_revision(), 7);
        assert_eq!(published.plan(), PlanId::CpuPostSort);
        assert_eq!(published.order_generation(), 11);
        assert_eq!(published.presentation_sequence(), 13);

        // Another unpresented attempt cannot overwrite the last successful
        // presentation identity with its requested profile.
        publication.retain_consumed_telemetry(SurfaceTelemetryBatch::default());
        assert_eq!(
            publication.presented_depth_precision_receipt(),
            Some(candidate)
        );

        let next_presented =
            depth_precision_receipt(SurfaceDepthPrecisionProfile::ExactFull32, 9, 15, 16);
        publish_depth_precision_receipt(&mut publication, next_presented);
        assert_eq!(
            publication.presented_depth_precision_receipt(),
            Some(next_presented)
        );
    }

    fn projected_cache_precision_receipt(
        profile: ProjectedCachePrecisionProfile,
        camera_revision: u64,
        order_generation: u64,
        presentation_sequence: u64,
    ) -> PresentedProjectedCachePrecisionReceipt {
        PresentedProjectedCachePrecisionReceipt::new(
            profile,
            FrameIdentity::new(2, camera_revision, 3, 4, 5),
            PlanId::GpuPreproject,
            order_generation,
            presentation_sequence,
        )
    }

    fn publish_projected_cache_precision_receipt(
        publication: &mut SessionPublication,
        receipt: PresentedProjectedCachePrecisionReceipt,
    ) {
        let (order, projected, producer) = submissions(receipt.presentation_sequence());
        publication.publish_presented_frame(PresentedFramePublication::new(
            FrameStats::zero(),
            PresentedCurrentStats::Preserve,
            PresentedFramePrecisionReceipts::new(None, Some(receipt)),
            order,
            projected,
            producer,
            PresentedTelemetry::from_batch(SurfaceTelemetryBatch::default()),
        ));
    }

    #[test]
    fn projected_cache_precision_receipt_is_present_fenced_and_identity_bound() {
        let mut publication = SessionPublication::new(true);
        assert_eq!(
            publication.presented_projected_cache_precision_receipt(),
            None
        );

        let candidate = projected_cache_precision_receipt(
            ProjectedCachePrecisionProfile::CandidateAxes16,
            7,
            11,
            13,
        );
        // Preparation, replacement, unavailable acquisition and failed present
        // have no presented DTO and therefore cannot manufacture or overwrite
        // a candidate receipt.
        publication.retain_consumed_telemetry(SurfaceTelemetryBatch::default());
        publication.terminalize_deferred_telemetry();
        assert_eq!(
            publication.presented_projected_cache_precision_receipt(),
            None
        );

        publish_projected_cache_precision_receipt(&mut publication, candidate);
        let published = publication
            .presented_projected_cache_precision_receipt()
            .expect("successful present publishes the realized projected-cache profile");
        assert_eq!(
            published.profile(),
            ProjectedCachePrecisionProfile::CandidateAxes16
        );
        assert_eq!(published.frame().camera_revision(), 7);
        assert_eq!(published.plan(), PlanId::GpuPreproject);
        assert_eq!(published.order_generation(), 11);
        assert_eq!(published.presentation_sequence(), 13);

        publication.retain_consumed_telemetry(SurfaceTelemetryBatch::default());
        assert_eq!(
            publication.presented_projected_cache_precision_receipt(),
            Some(candidate),
            "an unpresented attempt cannot overwrite the last successful receipt"
        );

        let next_presented = projected_cache_precision_receipt(
            ProjectedCachePrecisionProfile::ExactAxes32,
            9,
            15,
            16,
        );
        publish_projected_cache_precision_receipt(&mut publication, next_presented);
        assert_eq!(
            publication.presented_projected_cache_precision_receipt(),
            Some(next_presented)
        );
    }

    #[test]
    fn capture_depth_receipt_join_is_sequence_matched_take_once_and_not_overwritten() {
        let mut publication = SessionPublication::new(true);
        let captured =
            depth_precision_receipt(SurfaceDepthPrecisionProfile::CandidateStable24, 7, 11, 13);
        let later = depth_precision_receipt(SurfaceDepthPrecisionProfile::ExactFull32, 9, 15, 16);

        assert!(publication.arm_capture_depth_precision());
        publication.observe_presented_capture(Some(13), Some(captured));
        publication.observe_presented_capture(Some(16), Some(later));

        assert_eq!(publication.take_capture_depth_precision(), Some(captured));
        assert_eq!(publication.take_capture_depth_precision(), None);
    }

    #[test]
    fn capture_depth_receipt_join_fails_closed_without_a_matching_presented_receipt() {
        let receipt =
            depth_precision_receipt(SurfaceDepthPrecisionProfile::CandidateStable24, 7, 11, 13);

        for (presentation_sequence, depth_precision) in
            [(None, None), (Some(12), Some(receipt)), (Some(13), None)]
        {
            let mut publication = SessionPublication::new(true);
            assert!(publication.arm_capture_depth_precision());
            if presentation_sequence.is_some() || depth_precision.is_some() {
                publication.observe_presented_capture(presentation_sequence, depth_precision);
            }
            assert_eq!(publication.take_capture_depth_precision(), None);
        }
    }

    #[test]
    fn failed_or_duplicate_capture_lifecycle_cannot_manufacture_a_join() {
        let mut publication = SessionPublication::new(true);
        let receipt =
            depth_precision_receipt(SurfaceDepthPrecisionProfile::CandidateStable24, 7, 11, 13);

        assert!(publication.arm_capture_depth_precision());
        assert!(!publication.arm_capture_depth_precision());
        assert_eq!(publication.take_capture_depth_precision(), None);
        publication.observe_presented_capture(Some(13), Some(receipt));
        assert_eq!(publication.take_capture_depth_precision(), None);

        assert!(publication.arm_capture_depth_precision());
        publication.cancel_capture_depth_precision();
        publication.observe_presented_capture(Some(13), Some(receipt));
        assert_eq!(publication.take_capture_depth_precision(), None);
    }
}
