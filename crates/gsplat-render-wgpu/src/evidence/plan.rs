//! Immutable terminal evidence for one complete Exact execution plan.

use crate::plans::{FrameIdentity, OrderLane, PlanId};

/// Generations and immutable scene properties that make terminal plan samples
/// comparable. Camera revision deliberately stays on [`PlanSample`]: a moving
/// trace must be able to learn while every sample remains individually bound
/// to the camera that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanComparisonKey {
    scene_generation: u64,
    viewport_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
    source_count: u32,
    sh_degree: u8,
}

#[allow(dead_code)]
impl PlanComparisonKey {
    pub(crate) const fn new(frame: FrameIdentity, source_count: u32, sh_degree: u8) -> Self {
        Self {
            scene_generation: frame.scene_generation(),
            viewport_generation: frame.viewport_generation(),
            contract_generation: frame.contract_generation(),
            plan_set_generation: frame.plan_set_generation(),
            source_count,
            sh_degree,
        }
    }

    pub(crate) const fn accepts_frame(self, frame: FrameIdentity) -> bool {
        self.scene_generation == frame.scene_generation()
            && self.viewport_generation == frame.viewport_generation()
            && self.contract_generation == frame.contract_generation()
            && self.plan_set_generation == frame.plan_set_generation()
    }

    pub(crate) const fn source_count(self) -> u32 {
        self.source_count
    }

    pub(crate) const fn sh_degree(self) -> u8 {
        self.sh_degree
    }
}

/// Exact draw-count relationship retained even when its numeric value remains
/// in a GPU-owned indirect-argument buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanCountSemantics {
    DirectDrawEqualsVisible,
    IndirectDrawEqualsVisible,
    IndirectDrawEqualsContributor,
}

/// Identity published only after a formal sample has been bound to the last
/// command buffer of the definite queue submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanSampleTicket {
    ticket: u64,
    probe_generation: u64,
    comparison: PlanComparisonKey,
    plan: PlanId,
}

impl PlanSampleTicket {
    pub(crate) const fn new(
        ticket: u64,
        probe_generation: u64,
        comparison: PlanComparisonKey,
        plan: PlanId,
    ) -> Self {
        Self {
            ticket,
            probe_generation,
            comparison,
            plan,
        }
    }

    pub(crate) const fn ticket(self) -> u64 {
        self.ticket
    }

    pub(crate) const fn probe_generation(self) -> u64 {
        self.probe_generation
    }

    pub(crate) const fn comparison(self) -> PlanComparisonKey {
        self.comparison
    }

    pub(crate) const fn plan(self) -> PlanId {
        self.plan
    }
}

/// One queue-terminal, complete-plan sample. Numeric GPU V/C/D values remain
/// unavailable until a later asynchronous evidence owner supplies them; this
/// receipt never substitutes source count, capacity or zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlanSample {
    ticket: PlanSampleTicket,
    frame: FrameIdentity,
    order_lane: OrderLane,
    order_generation: u64,
    visible_count: Option<u32>,
    contributor_count: Option<u32>,
    draw_count: Option<u32>,
    count_semantics: PlanCountSemantics,
    frame_complete_ms: f32,
}

#[allow(dead_code)]
impl PlanSample {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        ticket: PlanSampleTicket,
        frame: FrameIdentity,
        order_lane: OrderLane,
        order_generation: u64,
        visible_count: Option<u32>,
        contributor_count: Option<u32>,
        draw_count: Option<u32>,
        count_semantics: PlanCountSemantics,
        frame_complete_ms: f32,
    ) -> Self {
        Self {
            ticket,
            frame,
            order_lane,
            order_generation,
            visible_count,
            contributor_count,
            draw_count,
            count_semantics,
            frame_complete_ms,
        }
    }

    pub(crate) const fn sample_ticket(self) -> PlanSampleTicket {
        self.ticket
    }

    pub(crate) const fn ticket(self) -> u64 {
        self.ticket.ticket()
    }

    pub(crate) const fn probe_generation(self) -> u64 {
        self.ticket.probe_generation()
    }

    pub(crate) const fn comparison(self) -> PlanComparisonKey {
        self.ticket.comparison()
    }

    pub(crate) const fn frame_identity(self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn plan_id(self) -> PlanId {
        self.ticket.plan()
    }

    pub(crate) const fn order_lane(self) -> OrderLane {
        self.order_lane
    }

    pub(crate) const fn order_generation(self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn visible_count(self) -> Option<u32> {
        self.visible_count
    }

    pub(crate) const fn contributor_count(self) -> Option<u32> {
        self.contributor_count
    }

    pub(crate) const fn draw_count(self) -> Option<u32> {
        self.draw_count
    }

    pub(crate) const fn count_semantics(self) -> PlanCountSemantics {
        self.count_semantics
    }

    pub(crate) const fn frame_complete_ms(self) -> f32 {
        self.frame_complete_ms
    }

    /// Structural comparability is independent from camera revision. It binds
    /// every sample to one exact frame while allowing an interleaved moving
    /// trace to form a cohort under the same immutable scene/viewport contract.
    pub(crate) fn is_comparable(self) -> bool {
        if self.ticket.ticket() == 0
            || !self.comparison().accepts_frame(self.frame)
            || !self.frame_complete_ms.is_finite()
            || self.frame_complete_ms <= 0.0
        {
            return false;
        }

        match (self.plan_id(), self.order_lane, self.count_semantics) {
            (PlanId::CpuPostSort, OrderLane::Cpu, PlanCountSemantics::DirectDrawEqualsVisible) => {
                self.visible_count
                    .is_some_and(|visible| visible <= self.comparison().source_count())
                    && self.draw_count == self.visible_count
                    && self.contributor_count.is_none()
            }
            (
                PlanId::GpuPostSort,
                OrderLane::Gpu,
                PlanCountSemantics::IndirectDrawEqualsVisible,
            )
            | (
                PlanId::GpuPreproject,
                OrderLane::Gpu,
                PlanCountSemantics::IndirectDrawEqualsContributor,
            ) => {
                self.visible_count.is_none()
                    && self.contributor_count.is_none()
                    && self.draw_count.is_none()
            }
            _ => false,
        }
    }
}
