//! Immutable public DTOs for the Surface current-stats receipt seam.
//!
//! Runtime interpretation, ticket allocation, generations, bounded queues and
//! terminal publication remain owned by `Renderer` and `PlanSampler`. Surface
//! and later C consumers only translate these values.

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
}

impl From<CurrentStatsReceipt> for SurfaceCurrentStatsReceipt {
    fn from(receipt: CurrentStatsReceipt) -> Self {
        Self {
            submission: receipt.submission().into(),
            counts: receipt.counts().into(),
            count_semantics: receipt.count_semantics().into(),
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
