//! Immutable public DTOs for the Surface current-stats receipt seam.
//!
//! Runtime interpretation, ticket allocation, generations, bounded queues and
//! terminal publication remain owned by `Renderer` and `PlanSampler`. Surface
//! and later C consumers only translate these values.

use gsplat_core::FrameStats;

use crate::{
    evidence::PlanCountSemantics,
    plans::{FrameIdentity, PlanId},
    renderer::{
        CurrentStatsCounts, CurrentStatsFailure, CurrentStatsJoinIdentity, CurrentStatsPoll,
        CurrentStatsReceipt, CurrentStatsRequest, CurrentStatsSubmission,
        CurrentStatsSubmissionReceipt, CurrentStatsTerminal, CurrentStatsUnsampledReason,
    },
};

/// Why a requested observer sample could not be issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCurrentStatsUnsampledReason {
    Busy,
    GpuUnavailable,
    ResourceUnavailable,
    TicketExhausted,
}

/// Immediate result of requesting current stats for the next eligible frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCurrentStatsRequest {
    Requested,
    Unsampled(SurfaceCurrentStatsUnsampledReason),
}

impl From<CurrentStatsRequest> for SurfaceCurrentStatsRequest {
    fn from(request: CurrentStatsRequest) -> Self {
        match request {
            CurrentStatsRequest::Requested => Self::Requested,
            CurrentStatsRequest::Unsampled(reason) => Self::Unsampled(reason.into()),
        }
    }
}

impl From<CurrentStatsUnsampledReason> for SurfaceCurrentStatsUnsampledReason {
    fn from(reason: CurrentStatsUnsampledReason) -> Self {
        match reason {
            CurrentStatsUnsampledReason::Busy => Self::Busy,
            CurrentStatsUnsampledReason::GpuUnavailable => Self::GpuUnavailable,
            CurrentStatsUnsampledReason::ResourceUnavailable => Self::ResourceUnavailable,
            CurrentStatsUnsampledReason::TicketExhausted => Self::TicketExhausted,
        }
    }
}

/// Closed identity for the complete Exact plan that produced a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCurrentStatsPlan {
    CpuPostSort,
    GpuPostSort,
    GpuPreproject,
}

impl From<PlanId> for SurfaceCurrentStatsPlan {
    fn from(plan: PlanId) -> Self {
        match plan {
            PlanId::CpuPostSort => Self::CpuPostSort,
            PlanId::GpuPostSort => Self::GpuPostSort,
            PlanId::GpuPreproject => Self::GpuPreproject,
        }
    }
}

/// Renderer-owned semantic generations for the frame that produced a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCurrentStatsFrameIdentity {
    scene_generation: u64,
    camera_revision: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
}

impl SurfaceCurrentStatsFrameIdentity {
    pub const fn scene_generation(self) -> u64 {
        self.scene_generation
    }

    pub const fn camera_revision(self) -> u64 {
        self.camera_revision
    }

    pub const fn viewport_generation(self) -> u64 {
        self.viewport_generation
    }

    pub const fn contract_generation(self) -> u64 {
        self.contract_generation
    }

    pub const fn plan_set_generation(self) -> u64 {
        self.plan_set_generation
    }
}

impl From<FrameIdentity> for SurfaceCurrentStatsFrameIdentity {
    fn from(frame: FrameIdentity) -> Self {
        Self {
            scene_generation: frame.scene_generation(),
            camera_revision: frame.camera_revision(),
            viewport_generation: frame.viewport_generation(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
        }
    }
}

/// Full join identity shared by submission and terminal receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCurrentStatsJoinIdentity {
    frame: SurfaceCurrentStatsFrameIdentity,
    executed_plan: SurfaceCurrentStatsPlan,
    order_generation: u64,
    raster_generation: u64,
    encode_attempt: u64,
    presentation_sequence: u64,
}

impl SurfaceCurrentStatsJoinIdentity {
    pub const fn frame_identity(self) -> SurfaceCurrentStatsFrameIdentity {
        self.frame
    }

    pub const fn executed_plan(self) -> SurfaceCurrentStatsPlan {
        self.executed_plan
    }

    pub const fn order_generation(self) -> u64 {
        self.order_generation
    }

    pub const fn raster_generation(self) -> u64 {
        self.raster_generation
    }

    pub const fn encode_attempt(self) -> u64 {
        self.encode_attempt
    }

    pub const fn presentation_sequence(self) -> u64 {
        self.presentation_sequence
    }
}

impl From<CurrentStatsJoinIdentity> for SurfaceCurrentStatsJoinIdentity {
    fn from(join: CurrentStatsJoinIdentity) -> Self {
        Self {
            frame: join.frame_identity().into(),
            executed_plan: join.plan_id().into(),
            order_generation: join.order_generation(),
            raster_generation: join.raster_generation(),
            encode_attempt: join.encode_attempt(),
            presentation_sequence: join.presentation_sequence(),
        }
    }
}

/// Ticket plus complete join identity published at presentation commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCurrentStatsSubmissionReceipt {
    ticket: u64,
    join: SurfaceCurrentStatsJoinIdentity,
}

impl SurfaceCurrentStatsSubmissionReceipt {
    pub const fn ticket(self) -> u64 {
        self.ticket
    }

    pub const fn join(self) -> SurfaceCurrentStatsJoinIdentity {
        self.join
    }
}

impl From<CurrentStatsSubmissionReceipt> for SurfaceCurrentStatsSubmissionReceipt {
    fn from(submission: CurrentStatsSubmissionReceipt) -> Self {
        Self {
            ticket: submission.ticket().get(),
            join: submission.join().into(),
        }
    }
}

/// Per-frame current-stats submission result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceCurrentStatsSubmission {
    #[default]
    NotRequested,
    Issued(SurfaceCurrentStatsSubmissionReceipt),
}

impl SurfaceCurrentStatsSubmission {
    pub const fn receipt(self) -> Option<SurfaceCurrentStatsSubmissionReceipt> {
        match self {
            Self::NotRequested => None,
            Self::Issued(receipt) => Some(receipt),
        }
    }
}

impl From<CurrentStatsSubmission> for SurfaceCurrentStatsSubmission {
    fn from(submission: CurrentStatsSubmission) -> Self {
        match submission {
            CurrentStatsSubmission::NotRequested => Self::NotRequested,
            CurrentStatsSubmission::Issued(receipt) => Self::Issued(receipt.into()),
        }
    }
}

/// Exact relationship between the visible, contributor and issued counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCurrentStatsCountSemantics {
    DirectDrawEqualsVisible,
    IndirectDrawEqualsVisible,
    IndirectDrawEqualsContributor,
}

impl From<PlanCountSemantics> for SurfaceCurrentStatsCountSemantics {
    fn from(semantics: PlanCountSemantics) -> Self {
        match semantics {
            PlanCountSemantics::DirectDrawEqualsVisible => Self::DirectDrawEqualsVisible,
            PlanCountSemantics::IndirectDrawEqualsVisible => Self::IndirectDrawEqualsVisible,
            PlanCountSemantics::IndirectDrawEqualsContributor => {
                Self::IndirectDrawEqualsContributor
            }
        }
    }
}

/// Atomic S/V/C/D values for one issued ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCurrentStatsCounts {
    source: u32,
    visible: u32,
    contributor: u32,
    drawn: u32,
}

impl SurfaceCurrentStatsCounts {
    pub const fn source(self) -> u32 {
        self.source
    }

    pub const fn visible(self) -> u32 {
        self.visible
    }

    pub const fn contributor(self) -> u32 {
        self.contributor
    }

    pub const fn drawn(self) -> u32 {
        self.drawn
    }
}

impl From<CurrentStatsCounts> for SurfaceCurrentStatsCounts {
    fn from(counts: CurrentStatsCounts) -> Self {
        Self {
            source: counts.source(),
            visible: counts.visible(),
            contributor: counts.contributor(),
            drawn: counts.drawn(),
        }
    }
}

/// Successful atomic terminal receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCurrentStatsReceipt {
    submission: SurfaceCurrentStatsSubmissionReceipt,
    counts: SurfaceCurrentStatsCounts,
    count_semantics: SurfaceCurrentStatsCountSemantics,
    frame_complete_ms_bits: u32,
    cpu_preprocess_ms_bits: Option<u32>,
    cpu_sort_ms_bits: Option<u32>,
}

impl SurfaceCurrentStatsReceipt {
    pub const fn submission(self) -> SurfaceCurrentStatsSubmissionReceipt {
        self.submission
    }

    pub const fn counts(self) -> SurfaceCurrentStatsCounts {
        self.counts
    }

    pub const fn count_semantics(self) -> SurfaceCurrentStatsCountSemantics {
        self.count_semantics
    }

    pub(crate) const fn frame_complete_ms(self) -> f32 {
        f32::from_bits(self.frame_complete_ms_bits)
    }

    pub(crate) const fn cpu_preprocess_ms(self) -> Option<f32> {
        match self.cpu_preprocess_ms_bits {
            Some(bits) => Some(f32::from_bits(bits)),
            None => None,
        }
    }

    pub(crate) const fn cpu_sort_ms(self) -> Option<f32> {
        match self.cpu_sort_ms_bits {
            Some(bits) => Some(f32::from_bits(bits)),
            None => None,
        }
    }
}

impl From<CurrentStatsReceipt> for SurfaceCurrentStatsReceipt {
    fn from(receipt: CurrentStatsReceipt) -> Self {
        Self {
            submission: receipt.submission().into(),
            counts: receipt.counts().into(),
            count_semantics: receipt.count_semantics().into(),
            frame_complete_ms_bits: receipt.frame_complete_ms().to_bits(),
            cpu_preprocess_ms_bits: receipt.cpu_preprocess_ms().map(f32::to_bits),
            cpu_sort_ms_bits: receipt.cpu_sort_ms().map(f32::to_bits),
        }
    }
}

/// Failed atomic terminal receipt retaining the original ticket and join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCurrentStatsFailure {
    submission: SurfaceCurrentStatsSubmissionReceipt,
}

impl SurfaceCurrentStatsFailure {
    pub const fn submission(self) -> SurfaceCurrentStatsSubmissionReceipt {
        self.submission
    }
}

impl From<CurrentStatsFailure> for SurfaceCurrentStatsFailure {
    fn from(failure: CurrentStatsFailure) -> Self {
        Self {
            submission: failure.submission().into(),
        }
    }
}

/// Exactly one terminal may be returned for an issued ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCurrentStatsTerminal {
    Ready(SurfaceCurrentStatsReceipt),
    MapFailure(SurfaceCurrentStatsFailure),
    GenerationInvalidated(SurfaceCurrentStatsFailure),
    Expired(SurfaceCurrentStatsFailure),
    Dropped(SurfaceCurrentStatsFailure),
}

impl SurfaceCurrentStatsTerminal {
    pub const fn submission(self) -> SurfaceCurrentStatsSubmissionReceipt {
        match self {
            Self::Ready(receipt) => receipt.submission,
            Self::MapFailure(failure)
            | Self::GenerationInvalidated(failure)
            | Self::Expired(failure)
            | Self::Dropped(failure) => failure.submission,
        }
    }

    pub const fn ticket(self) -> u64 {
        self.submission().ticket
    }
}

impl From<CurrentStatsTerminal> for SurfaceCurrentStatsTerminal {
    fn from(terminal: CurrentStatsTerminal) -> Self {
        match terminal {
            CurrentStatsTerminal::Ready(receipt) => Self::Ready(receipt.into()),
            CurrentStatsTerminal::MapFailure(failure) => Self::MapFailure(failure.into()),
            CurrentStatsTerminal::GenerationInvalidated(failure) => {
                Self::GenerationInvalidated(failure.into())
            }
            CurrentStatsTerminal::Expired(failure) => Self::Expired(failure.into()),
            CurrentStatsTerminal::Dropped(failure) => Self::Dropped(failure.into()),
        }
    }
}

/// One global single-pop poll result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceCurrentStatsPoll {
    #[default]
    Empty,
    Unsampled(SurfaceCurrentStatsUnsampledReason),
    Terminal(SurfaceCurrentStatsTerminal),
}

impl SurfaceCurrentStatsPoll {
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }
}

impl From<CurrentStatsPoll> for SurfaceCurrentStatsPoll {
    fn from(poll: CurrentStatsPoll) -> Self {
        match poll {
            CurrentStatsPoll::Empty => Self::Empty,
            CurrentStatsPoll::Unsampled(reason) => Self::Unsampled(reason.into()),
            CurrentStatsPoll::Terminal(terminal) => Self::Terminal(terminal.into()),
        }
    }
}

/// Whether the legacy FrameStats projection for the last presented Surface
/// frame has demonstrably current V/D counts.
///
/// This is compatibility availability only. Renderer remains the ticket,
/// generation, queue and terminal owner; Surface retains no terminal ledger.
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
            SurfaceCurrentStatsTerminal::Ready(receipt) => {
                let counts = receipt.counts();
                stats.visible_count = counts.visible();
                stats.drawn_count = counts.drawn();
                *self = Self::Current;
            }
            SurfaceCurrentStatsTerminal::MapFailure(_)
            | SurfaceCurrentStatsTerminal::GenerationInvalidated(_)
            | SurfaceCurrentStatsTerminal::Expired(_)
            | SurfaceCurrentStatsTerminal::Dropped(_) => {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::evidence::{
        SessionEvidence, SurfaceCompatibilityOrderSubmission,
        SurfaceCompatibilityProducerSubmission, SurfaceCompatibilityProjectedSubmission,
        SurfaceCompatibilityTerminal, SurfaceCompatibilityTerminalPoll,
        SurfaceCompatibilityTerminalSelector, SurfaceGpuProducerMeasurementSubmission,
        SurfaceOrderMeasurementSubmission, SurfaceProjectedDrawMeasurementSubmission,
    };
    use crate::surface_session::{
        SurfaceAdaptiveState, SurfaceOrderBackend, SurfaceProjectedDrawAdaptiveState,
        SurfaceProjectedDrawPolicy,
    };
    use crate::{SurfaceGpuOrderProducer, SurfaceOrderBackendUsed, SurfaceProjectedDrawExecution};

    fn submission(ticket: u64, camera_revision: u64) -> SurfaceCurrentStatsSubmissionReceipt {
        SurfaceCurrentStatsSubmissionReceipt {
            ticket,
            join: SurfaceCurrentStatsJoinIdentity {
                frame: SurfaceCurrentStatsFrameIdentity {
                    scene_generation: 3,
                    camera_revision,
                    viewport_generation: 5,
                    contract_generation: 7,
                    plan_set_generation: 11,
                },
                executed_plan: SurfaceCurrentStatsPlan::GpuPostSort,
                order_generation: 13,
                raster_generation: 11,
                encode_attempt: 17,
                presentation_sequence: 19,
            },
        }
    }

    fn ready(submission: SurfaceCurrentStatsSubmissionReceipt) -> SurfaceCurrentStatsPoll {
        SurfaceCurrentStatsPoll::Terminal(SurfaceCurrentStatsTerminal::Ready(
            SurfaceCurrentStatsReceipt {
                submission,
                counts: SurfaceCurrentStatsCounts {
                    source: 100,
                    visible: 80,
                    contributor: 70,
                    drawn: 80,
                },
                count_semantics: SurfaceCurrentStatsCountSemantics::IndirectDrawEqualsVisible,
                frame_complete_ms_bits: 6.0_f32.to_bits(),
                cpu_preprocess_ms_bits: None,
                cpu_sort_ms_bits: None,
            },
        ))
    }

    fn observe_order_ticket(
        store: &mut SessionEvidence,
        ticket: u64,
        camera_revision: u64,
        backend: SurfaceOrderBackendUsed,
    ) {
        store.observe_submissions(
            SurfaceCompatibilityOrderSubmission {
                camera_revision,
                requested_backend: match backend {
                    SurfaceOrderBackendUsed::Cpu => SurfaceOrderBackend::Cpu,
                    SurfaceOrderBackendUsed::Gpu => SurfaceOrderBackend::Gpu,
                },
                actual_backend: backend,
                adaptive_state: SurfaceAdaptiveState::Disabled,
                measurement: SurfaceOrderMeasurementSubmission::Issued { backend, ticket },
            },
            SurfaceCompatibilityProjectedSubmission {
                camera_revision,
                requested_policy: SurfaceProjectedDrawPolicy::Candidate,
                actual_execution: SurfaceProjectedDrawExecution::Candidate,
                order_backend: backend,
                adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
                measurement: SurfaceProjectedDrawMeasurementSubmission::NotRequested,
            },
            SurfaceCompatibilityProducerSubmission {
                camera_revision,
                requested_producer: SurfaceGpuOrderProducer::PostSort,
                actual_producer: None,
                order_backend: backend,
                projected_execution: SurfaceProjectedDrawExecution::Candidate,
                measurement_enabled: false,
                measurement: SurfaceGpuProducerMeasurementSubmission::NotRequested,
            },
        );
    }

    #[test]
    fn exact_cpu_current_stats_terminal_resolves_same_ticket_order_ledger() {
        let mut store = SessionEvidence::new();
        observe_order_ticket(&mut store, 23, 29, SurfaceOrderBackendUsed::Cpu);
        let mut receipt = match ready(submission(23, 29)) {
            SurfaceCurrentStatsPoll::Terminal(SurfaceCurrentStatsTerminal::Ready(receipt)) => {
                receipt
            }
            _ => unreachable!(),
        };
        receipt.submission.join.executed_plan = SurfaceCurrentStatsPlan::CpuPostSort;
        receipt.cpu_preprocess_ms_bits = Some(1.25_f32.to_bits());
        receipt.cpu_sort_ms_bits = Some(2.5_f32.to_bits());

        store.publish_exact_order_terminal(SurfaceCurrentStatsTerminal::Ready(receipt));

        let SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::OrderCpuSuccess(
            success,
        )) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess)
        else {
            panic!("same-ticket CPU compatibility terminal must be ready");
        };
        assert_eq!(success.ticket, 23);
        assert_eq!(success.camera_revision, 29);
        assert_eq!(success.preprocess_ms, 1.25);
        assert_eq!(success.sort_ms, 2.5);
        assert_eq!(success.frame_complete_ms, 6.0);
    }

    fn pending_stats() -> FrameStats {
        FrameStats {
            frame_ms: 1.0,
            preprocess_ms: 2.0,
            sort_ms: 3.0,
            raster_ms: 4.0,
            visible_count: 0,
            drawn_count: 0,
        }
    }

    #[test]
    fn presented_frame_preserves_synchronous_current_success() {
        let issued = SurfaceCurrentStatsSubmission::Issued(submission(23, 29));
        assert_eq!(
            LegacySurfaceStatsAvailability::for_presented_frame(
                true,
                SurfaceCurrentStatsSubmission::NotRequested,
            ),
            LegacySurfaceStatsAvailability::Current,
        );
        assert_eq!(
            LegacySurfaceStatsAvailability::for_presented_frame(true, issued),
            LegacySurfaceStatsAvailability::Current,
        );
        assert_eq!(
            LegacySurfaceStatsAvailability::for_presented_frame(
                false,
                SurfaceCurrentStatsSubmission::NotRequested,
            ),
            LegacySurfaceStatsAvailability::Unavailable,
        );
        assert_eq!(
            LegacySurfaceStatsAvailability::for_presented_frame(false, issued),
            LegacySurfaceStatsAvailability::AwaitingCurrentReceipt,
        );
    }

    #[test]
    fn matching_ready_receipt_makes_pending_legacy_counts_current() {
        let expected = submission(23, 29);
        let mut availability = LegacySurfaceStatsAvailability::AwaitingCurrentReceipt;
        let mut stats = pending_stats();

        availability.observe_poll(
            SurfaceCurrentStatsSubmission::Issued(expected),
            ready(expected),
            &mut stats,
        );

        assert_eq!(availability, LegacySurfaceStatsAvailability::Current);
        assert_eq!(stats.visible_count, 80);
        assert_eq!(stats.drawn_count, 80);
        assert_eq!(stats.frame_ms, 1.0);
        assert_eq!(stats.preprocess_ms, 2.0);
        assert_eq!(stats.sort_ms, 3.0);
        assert_eq!(stats.raster_ms, 4.0);
    }

    #[test]
    fn pending_expired_and_mismatched_receipts_never_publish_legacy_counts() {
        let expected = submission(23, 29);
        let mismatched_ticket = submission(24, 29);
        let mismatched_generation = submission(23, 30);

        for poll in [
            SurfaceCurrentStatsPoll::Empty,
            ready(mismatched_ticket),
            ready(mismatched_generation),
        ] {
            let mut availability = LegacySurfaceStatsAvailability::AwaitingCurrentReceipt;
            let mut stats = pending_stats();
            let before = stats;
            availability.observe_poll(
                SurfaceCurrentStatsSubmission::Issued(expected),
                poll,
                &mut stats,
            );
            assert_eq!(
                availability,
                LegacySurfaceStatsAvailability::AwaitingCurrentReceipt,
            );
            assert_eq!(stats, before);
            assert_eq!(availability.get(stats), None);
        }

        for terminal in [
            SurfaceCurrentStatsTerminal::MapFailure(SurfaceCurrentStatsFailure {
                submission: expected,
            }),
            SurfaceCurrentStatsTerminal::GenerationInvalidated(SurfaceCurrentStatsFailure {
                submission: expected,
            }),
            SurfaceCurrentStatsTerminal::Expired(SurfaceCurrentStatsFailure {
                submission: expected,
            }),
            SurfaceCurrentStatsTerminal::Dropped(SurfaceCurrentStatsFailure {
                submission: expected,
            }),
        ] {
            let mut availability = LegacySurfaceStatsAvailability::AwaitingCurrentReceipt;
            let mut stats = pending_stats();
            let before = stats;
            availability.observe_poll(
                SurfaceCurrentStatsSubmission::Issued(expected),
                SurfaceCurrentStatsPoll::Terminal(terminal),
                &mut stats,
            );
            assert_eq!(availability, LegacySurfaceStatsAvailability::Unavailable);
            assert_eq!(stats, before);
            assert_eq!(availability.get(stats), None);
        }
    }
}
