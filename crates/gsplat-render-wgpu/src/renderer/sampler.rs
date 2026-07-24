//! Mandatory non-blocking completion sampler for complete Exact plans.

use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU32, Ordering},
};

use thiserror::Error;

use crate::TimerInstant;
use crate::evidence::{PlanComparisonKey, PlanCountSemantics, PlanSample, PlanSampleTicket};
use crate::plans::{FrameIdentity, OrderLane, PlanId};
use crate::timer_elapsed_ms;

use super::current_stats::{
    ArmedCurrentStats, CurrentStatsFrameCounts, CurrentStatsHandoff, CurrentStatsLane,
    CurrentStatsPoll, CurrentStatsRequest, CurrentStatsSubmission, CurrentStatsTicket,
    StagedCurrentStats,
};

const PENDING: u8 = 0;
const COMPLETE: u8 = 1;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanSamplerError {
    #[error("a formal whole-plan completion sample is already pending")]
    Busy,
    #[error("whole-plan completion ticket space is exhausted")]
    TicketExhausted,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlanSampleDescriptor {
    pub(super) probe_generation: u64,
    pub(super) comparison: PlanComparisonKey,
    pub(super) frame: FrameIdentity,
    pub(super) plan: PlanId,
    pub(super) order_lane: OrderLane,
    pub(super) order_generation: u64,
    pub(super) visible_count: Option<u32>,
    pub(super) contributor_count: Option<u32>,
    pub(super) draw_count: Option<u32>,
    pub(super) count_semantics: PlanCountSemantics,
}

struct PendingTerminalSample {
    descriptor: PlanSampleDescriptor,
    ticket: PlanSampleTicket,
    state: Arc<AtomicU8>,
    completion_ms_bits: Arc<AtomicU32>,
}

/// Transaction-owned terminal sample. Attaching its callback to a command
/// buffer does not make it visible to the live sampler; only a successful
/// target finalize moves this sole consumer into `PlanSampler`.
pub(super) struct StagedPlanSample {
    pending: PendingTerminalSample,
}

impl StagedPlanSample {
    pub(super) const fn ticket(&self) -> PlanSampleTicket {
        self.pending.ticket
    }
}

/// One mandatory slot is sufficient because the controller never schedules a
/// second formal sample until the first has reached a terminal callback.
pub(super) struct PlanSampler {
    next_ticket: u64,
    pending: Option<PendingTerminalSample>,
    current_stats: CurrentStatsLane,
}

impl PlanSampler {
    pub(super) const fn new() -> Self {
        Self {
            next_ticket: 1,
            pending: None,
            current_stats: CurrentStatsLane::new(),
        }
    }

    /// Binds a ticket to the final command buffer of the definite submission.
    /// The callback writes only two atomics and never blocks or reads GPU data.
    pub(super) fn arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        descriptor: PlanSampleDescriptor,
        completion_started: TimerInstant,
    ) -> Result<PlanSampleTicket, PlanSamplerError> {
        if self.pending.is_some() {
            return Err(PlanSamplerError::Busy);
        }
        let staged = self.stage_arm(command_buffer, descriptor, completion_started)?;
        let ticket = staged.ticket();
        self.commit_staged(staged);
        Ok(ticket)
    }

    /// Attaches a callback while retaining the sample outside the live slot.
    /// An abandoned target drops this owner; a late callback then has no path
    /// into controller or evidence state.
    pub(super) fn stage_arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        descriptor: PlanSampleDescriptor,
        completion_started: TimerInstant,
    ) -> Result<StagedPlanSample, PlanSamplerError> {
        let ticket_number = self.next_ticket;
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .ok_or(PlanSamplerError::TicketExhausted)?;
        let ticket = PlanSampleTicket::new(
            ticket_number,
            descriptor.probe_generation,
            descriptor.comparison,
            descriptor.plan,
        );
        let state = Arc::new(AtomicU8::new(PENDING));
        let completion_ms_bits = Arc::new(AtomicU32::new(0));
        let callback_state = Arc::clone(&state);
        let callback_completion = Arc::clone(&completion_ms_bits);
        command_buffer.on_submitted_work_done(move || {
            callback_completion.store(
                timer_elapsed_ms(completion_started).to_bits(),
                Ordering::Relaxed,
            );
            callback_state.store(COMPLETE, Ordering::Release);
        });
        Ok(StagedPlanSample {
            pending: PendingTerminalSample {
                descriptor,
                ticket,
                state,
                completion_ms_bits,
            },
        })
    }

    pub(super) fn commit_staged(&mut self, staged: StagedPlanSample) {
        debug_assert!(self.pending.is_none());
        self.pending = Some(staged.pending);
    }

    pub(super) fn poll(&mut self, device: &wgpu::Device) -> Option<PlanSample> {
        let _ = device.poll(wgpu::PollType::Poll);
        let pending = self.pending.as_ref()?;
        if pending.state.load(Ordering::Acquire) != COMPLETE {
            return None;
        }
        let completion_ms = f32::from_bits(pending.completion_ms_bits.load(Ordering::Relaxed));
        let descriptor = pending.descriptor;
        let ticket = pending.ticket;
        self.pending = None;
        Some(PlanSample::new(
            ticket,
            descriptor.frame,
            descriptor.order_lane,
            descriptor.order_generation,
            descriptor.visible_count,
            descriptor.contributor_count,
            descriptor.draw_count,
            descriptor.count_semantics,
            completion_ms,
        ))
    }

    pub(super) fn invalidate(&mut self) {
        // E11 tickets are private shadow-policy identities, not public
        // receipts with a terminal failure queue. Runtime, plan-set or key
        // replacement therefore expires the ticket by dropping this sole
        // owner. The old callback retains only its atomics, so late completion
        // cannot enter the replacement controller or optional evidence ring.
        self.pending = None;
    }

    pub(super) const fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) const fn pending_ticket(&self) -> Option<PlanSampleTicket> {
        match &self.pending {
            Some(pending) => Some(pending.ticket),
            None => None,
        }
    }

    pub(super) fn request_current_stats(&mut self, device: &wgpu::Device) -> CurrentStatsRequest {
        self.current_stats.request(device)
    }

    pub(super) fn encode_current_stats(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        counts: CurrentStatsFrameCounts<'_>,
    ) -> Option<StagedCurrentStats> {
        self.current_stats.encode(encoder, counts)
    }

    pub(super) fn prepare_current_stats_encode(&mut self, device: &wgpu::Device) -> bool {
        self.current_stats.prepare_encode(device)
    }

    pub(super) fn arm_current_stats(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        staged: StagedCurrentStats,
    ) -> ArmedCurrentStats {
        self.current_stats.arm(command_buffer, staged)
    }

    pub(super) fn commit_current_stats(
        &mut self,
        armed: ArmedCurrentStats,
        encode_attempt: u64,
        presentation_sequence: u64,
    ) -> CurrentStatsSubmission {
        self.current_stats
            .commit(armed, encode_attempt, presentation_sequence)
    }

    pub(super) fn accepts_current_stats_armed(
        &self,
        armed: &ArmedCurrentStats,
        frame: FrameIdentity,
        plan: PlanId,
    ) -> bool {
        self.current_stats.accepts_armed(armed, frame, plan)
    }

    pub(super) fn poll_current_stats(&mut self, device: Option<&wgpu::Device>) -> CurrentStatsPoll {
        self.current_stats.poll(device)
    }

    pub(super) fn resolve_current_stats_request_unsampled(
        &mut self,
        reason: super::current_stats::CurrentStatsUnsampledReason,
    ) -> bool {
        self.current_stats.resolve_request_unsampled(reason)
    }

    pub(super) fn expire_current_stats(&mut self, ticket: CurrentStatsTicket) -> bool {
        self.current_stats.expire(ticket)
    }

    pub(super) fn current_stats_replacement_handoff(&self) -> CurrentStatsHandoff {
        self.current_stats.replacement_handoff()
    }

    pub(super) const fn has_current_stats_request(&self) -> bool {
        self.current_stats.request_pending()
    }

    pub(super) fn import_current_stats_handoff(&mut self, handoff: CurrentStatsHandoff) {
        self.current_stats.import_handoff(handoff);
    }

    #[cfg(test)]
    pub(super) const fn current_stats_copy_count_for_test(&self) -> u64 {
        self.current_stats.encoded_copy_count()
    }

    #[cfg(test)]
    pub(super) fn current_stats_readback_bytes_for_test(&self) -> u64 {
        self.current_stats.allocated_readback_bytes()
    }

    pub(super) const fn current_stats_request_pending_for_test(&self) -> bool {
        self.current_stats.request_pending()
    }

    #[cfg(test)]
    pub(super) fn force_current_stats_map_failure_for_test(
        &mut self,
        ticket: CurrentStatsTicket,
    ) -> bool {
        self.current_stats.force_map_failure(ticket)
    }

    #[cfg(test)]
    pub(super) fn hold_current_stats_callback_for_test(
        &mut self,
        ticket: CurrentStatsTicket,
    ) -> bool {
        self.current_stats.hold_callback_for_test(ticket)
    }

    #[cfg(test)]
    pub(super) fn poll_current_stats_without_device_for_test(&mut self) -> CurrentStatsPoll {
        self.current_stats.poll(None)
    }
}
