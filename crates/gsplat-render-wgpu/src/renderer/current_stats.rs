//! Requested, bounded and non-blocking Exact S/V/C/D receipts.
//!
//! This is an observer lane owned by [`super::sampler::PlanSampler`]. It does
//! not feed the whole-plan controller and it never reserves work unless the
//! caller explicitly requests the next frame.

use std::{
    collections::VecDeque,
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

use crate::evidence::PlanCountSemantics;
use crate::plans::{FrameIdentity, PlanId};

use super::gpu_prepare::GpuCountSource;

const RING_CAPACITY: usize = 4;
const READBACK_BYTES: u64 = 2 * size_of::<u32>() as u64;
const VISIBLE_OFFSET: u64 = 0;
const CONTRIBUTOR_OFFSET: u64 = size_of::<u32>() as u64;

const SLOT_IDLE: u8 = 0;
const SLOT_RESERVED: u8 = 1;
const SLOT_ENCODED: u8 = 2;
const SLOT_SUBMITTED: u8 = 3;
const SLOT_MAPPED: u8 = 4;
const SLOT_MAP_ERROR: u8 = 5;
const PUBLICATION_UNPUBLISHED: u8 = 0;
const PUBLICATION_COMMITTED: u8 = 1;
const PUBLICATION_ABANDONED: u8 = 2;
const COMPLETION_PENDING_BITS: u32 = u32::MAX;

/// Why an explicit request could not reserve bounded observer capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentStatsUnsampledReason {
    Busy,
    GpuUnavailable,
    ResourceUnavailable,
    TicketExhausted,
}

/// Immediate result of requesting the next eligible Exact frame. A frame
/// carrying a ControllerFormal sample is deliberately ineligible so observer
/// GPU work cannot enter the controller's completion metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentStatsRequest {
    Requested,
    Unsampled(CurrentStatsUnsampledReason),
}

/// One non-blocking observer poll. A pre-ticket request resolution is kept
/// separate from issued-ticket terminals so unavailable is never represented
/// by fabricated counts or a fake ticket. Each call consumes at most one
/// caller-visible result; additional ready terminals stay in the
/// Renderer-owned bounded queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CurrentStatsPoll {
    #[default]
    Empty,
    Unsampled(CurrentStatsUnsampledReason),
    Terminal(CurrentStatsTerminal),
}

impl CurrentStatsPoll {
    pub(crate) const fn unsampled(&self) -> Option<CurrentStatsUnsampledReason> {
        match self {
            Self::Unsampled(reason) => Some(*reason),
            Self::Empty | Self::Terminal(_) => None,
        }
    }

    pub(crate) const fn terminal(&self) -> Option<CurrentStatsTerminal> {
        match self {
            Self::Terminal(terminal) => Some(*terminal),
            Self::Empty | Self::Unsampled(_) => None,
        }
    }

    pub(crate) const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

/// Opaque ticket published only by a successful target finalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CurrentStatsTicket(u64);

impl CurrentStatsTicket {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// Full identity shared by submission and terminal receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentStatsJoinIdentity {
    frame: FrameIdentity,
    plan: PlanId,
    order_generation: u64,
    raster_generation: u64,
    encode_attempt: u64,
    presentation_sequence: u64,
}

impl CurrentStatsJoinIdentity {
    pub(crate) const fn frame_identity(self) -> FrameIdentity {
        self.frame
    }

    pub(crate) const fn plan_id(self) -> PlanId {
        self.plan
    }

    pub(crate) const fn order_generation(self) -> u64 {
        self.order_generation
    }

    pub(crate) const fn raster_generation(self) -> u64 {
        self.raster_generation
    }

    pub(crate) const fn encode_attempt(self) -> u64 {
        self.encode_attempt
    }

    pub(crate) const fn presentation_sequence(self) -> u64 {
        self.presentation_sequence
    }
}

/// Ticket and complete join identity exposed at the presentation commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentStatsSubmissionReceipt {
    ticket: CurrentStatsTicket,
    join: CurrentStatsJoinIdentity,
}

impl CurrentStatsSubmissionReceipt {
    pub(crate) const fn ticket(self) -> CurrentStatsTicket {
        self.ticket
    }

    pub(crate) const fn join(self) -> CurrentStatsJoinIdentity {
        self.join
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CurrentStatsSubmission {
    #[default]
    NotRequested,
    Issued(CurrentStatsSubmissionReceipt),
}

impl CurrentStatsSubmission {
    pub(crate) const fn receipt(self) -> Option<CurrentStatsSubmissionReceipt> {
        match self {
            Self::NotRequested => None,
            Self::Issued(receipt) => Some(receipt),
        }
    }
}

/// Numeric Exact count receipt. No field has an unavailable sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentStatsCounts {
    source: u32,
    visible: u32,
    contributor: u32,
    drawn: u32,
}

impl CurrentStatsCounts {
    pub(crate) const fn source(self) -> u32 {
        self.source
    }

    pub(crate) const fn visible(self) -> u32 {
        self.visible
    }

    pub(crate) const fn contributor(self) -> u32 {
        self.contributor
    }

    pub(crate) const fn drawn(self) -> u32 {
        self.drawn
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentStatsReceipt {
    submission: CurrentStatsSubmissionReceipt,
    counts: CurrentStatsCounts,
    count_semantics: PlanCountSemantics,
    frame_complete_ms_bits: u32,
    cpu_preprocess_ms_bits: Option<u32>,
    cpu_sort_ms_bits: Option<u32>,
}

impl CurrentStatsReceipt {
    pub(crate) const fn submission(self) -> CurrentStatsSubmissionReceipt {
        self.submission
    }

    pub(crate) const fn counts(self) -> CurrentStatsCounts {
        self.counts
    }

    pub(crate) const fn count_semantics(self) -> PlanCountSemantics {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentStatsFailure {
    submission: CurrentStatsSubmissionReceipt,
}

impl CurrentStatsFailure {
    pub(crate) const fn submission(self) -> CurrentStatsSubmissionReceipt {
        self.submission
    }
}

/// Exactly one of these may be observed for an issued ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentStatsTerminal {
    Ready(CurrentStatsReceipt),
    MapFailure(CurrentStatsFailure),
    GenerationInvalidated(CurrentStatsFailure),
    Expired(CurrentStatsFailure),
    Dropped(CurrentStatsFailure),
}

impl CurrentStatsTerminal {
    pub(crate) const fn submission(self) -> CurrentStatsSubmissionReceipt {
        match self {
            Self::Ready(receipt) => receipt.submission,
            Self::MapFailure(failure)
            | Self::GenerationInvalidated(failure)
            | Self::Expired(failure)
            | Self::Dropped(failure) => failure.submission,
        }
    }

    pub(crate) const fn ticket(self) -> CurrentStatsTicket {
        self.submission().ticket
    }
}

#[cfg(any(
    feature = "qualification-q3-cpu-scalar",
    feature = "qualification-q3-cpu-neon"
))]
pub(crate) fn qualification_terminal_record(poll: CurrentStatsPoll) -> Option<String> {
    let CurrentStatsPoll::Terminal(terminal) = poll else {
        return None;
    };
    let submission = terminal.submission();
    let join = submission.join();
    let frame = join.frame_identity();
    let identity = format!(
        "ticket_namespace=current_stats ticket={} executed_plan={} scene_generation={} camera_revision={} viewport_generation={} contract_generation={} plan_set_generation={} order_generation={} raster_generation={} encode_attempt={} presentation_sequence={}",
        submission.ticket().get(),
        qualification_plan_label(join.plan_id()),
        frame.scene_generation(),
        frame.camera_revision(),
        frame.viewport_generation(),
        frame.contract_generation(),
        frame.plan_set_generation(),
        join.order_generation(),
        join.raster_generation(),
        join.encode_attempt(),
        join.presentation_sequence(),
    );
    match terminal {
        CurrentStatsTerminal::Ready(receipt) => {
            let counts = receipt.counts();
            Some(format!(
                "SURFACE_CURRENT_STATS_TERMINAL status=ready {identity} count_semantics={} source_count={} visible_count={} contributor_count={} drawn_count={} cpu_preprocess_ms={} cpu_sort_ms={} queue_completion_ms={:.6}",
                qualification_count_semantics_label(receipt.count_semantics()),
                counts.source(),
                counts.visible(),
                counts.contributor(),
                counts.drawn(),
                qualification_optional_ms(receipt.cpu_preprocess_ms()),
                qualification_optional_ms(receipt.cpu_sort_ms()),
                receipt.frame_complete_ms(),
            ))
        }
        CurrentStatsTerminal::MapFailure(_)
        | CurrentStatsTerminal::GenerationInvalidated(_)
        | CurrentStatsTerminal::Expired(_)
        | CurrentStatsTerminal::Dropped(_) => Some(format!(
            "SURFACE_CURRENT_STATS_TERMINAL status={} {identity}",
            qualification_failure_label(terminal),
        )),
    }
}

#[cfg(any(
    feature = "qualification-q3-cpu-scalar",
    feature = "qualification-q3-cpu-neon"
))]
const fn qualification_plan_label(plan: PlanId) -> &'static str {
    match plan {
        PlanId::CpuPostSort => "cpu_post_sort",
        PlanId::GpuPostSort => "gpu_post_sort",
        PlanId::GpuPreproject => "gpu_preproject",
    }
}

#[cfg(any(
    feature = "qualification-q3-cpu-scalar",
    feature = "qualification-q3-cpu-neon"
))]
const fn qualification_count_semantics_label(semantics: PlanCountSemantics) -> &'static str {
    match semantics {
        PlanCountSemantics::DirectDrawEqualsVisible => "direct_draw_equals_visible",
        PlanCountSemantics::IndirectDrawEqualsVisible => "indirect_draw_equals_visible",
        PlanCountSemantics::IndirectDrawEqualsContributor => "indirect_draw_equals_contributor",
    }
}

#[cfg(any(
    feature = "qualification-q3-cpu-scalar",
    feature = "qualification-q3-cpu-neon"
))]
const fn qualification_failure_label(terminal: CurrentStatsTerminal) -> &'static str {
    match terminal {
        CurrentStatsTerminal::Ready(_) => "ready",
        CurrentStatsTerminal::MapFailure(_) => "map_failure",
        CurrentStatsTerminal::GenerationInvalidated(_) => "generation_invalidated",
        CurrentStatsTerminal::Expired(_) => "expired",
        CurrentStatsTerminal::Dropped(_) => "dropped",
    }
}

#[cfg(any(
    feature = "qualification-q3-cpu-scalar",
    feature = "qualification-q3-cpu-neon"
))]
fn qualification_optional_ms(value: Option<f32>) -> String {
    value.map_or_else(|| "none".to_owned(), |value| format!("{value:.6}"))
}

#[derive(Clone, Copy)]
pub(super) enum CurrentStatsVisibleSource<'a> {
    Host(u32),
    Gpu(GpuCountSource<'a>),
}

#[derive(Clone, Copy)]
pub(super) struct CurrentStatsFrameCounts<'a> {
    pub(super) frame: FrameIdentity,
    pub(super) plan: PlanId,
    pub(super) order_generation: u64,
    pub(super) source_count: u32,
    pub(super) visible: CurrentStatsVisibleSource<'a>,
    pub(super) contributor: GpuCountSource<'a>,
    pub(super) count_semantics: PlanCountSemantics,
    pub(super) cpu_preprocess_ms: Option<f32>,
    pub(super) cpu_sort_ms: Option<f32>,
}

#[derive(Clone, Copy)]
struct EncodedDescriptor {
    frame: FrameIdentity,
    plan: PlanId,
    order_generation: u64,
    source_count: u32,
    host_visible: Option<u32>,
    count_semantics: PlanCountSemantics,
    cpu_preprocess_ms_bits: Option<u32>,
    cpu_sort_ms_bits: Option<u32>,
}

struct CurrentStatsSlot {
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    publication: Arc<AtomicU8>,
    completion_ms_bits: Arc<AtomicU32>,
    ticket: CurrentStatsTicket,
    descriptor: Option<EncodedDescriptor>,
    submission: Option<CurrentStatsSubmissionReceipt>,
    terminal_reported: bool,
}

/// Encode-owned token. Dropping before queue submission releases only this
/// unpublished reservation; no ticket or terminal becomes observable.
pub(super) struct StagedCurrentStats {
    slot: usize,
    ticket: CurrentStatsTicket,
    state: Arc<AtomicU8>,
}

impl Drop for StagedCurrentStats {
    fn drop(&mut self) {
        let _ = self.state.compare_exchange(
            SLOT_ENCODED,
            SLOT_IDLE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

pub(super) struct ArmedCurrentStats {
    slot: usize,
    ticket: CurrentStatsTicket,
    publication: Arc<AtomicU8>,
    committed: bool,
}

impl Drop for ArmedCurrentStats {
    fn drop(&mut self) {
        if !self.committed {
            self.publication
                .store(PUBLICATION_ABANDONED, Ordering::Release);
        }
    }
}

#[derive(Clone)]
pub(super) struct CurrentStatsHandoff {
    next_ticket: u64,
    terminals: VecDeque<CurrentStatsTerminal>,
    pending_request: bool,
    fresh_cpu_order_required: bool,
    request_unsampled: Option<CurrentStatsUnsampledReason>,
    inherited_queue_barriers: Vec<Arc<AtomicU8>>,
    observer_since_formal: bool,
}

/// The fixed readback half of the optional current-stats capability. It is
/// constructed beside the contributor scan under the same device error
/// scopes, then moved into the live sampler only with the complete product
/// GPU admission.
pub(super) struct CurrentStatsReadbackPool {
    slots: Vec<CurrentStatsSlot>,
}

impl CurrentStatsReadbackPool {
    pub(super) fn create_candidate(device: &wgpu::Device) -> Self {
        let slots = (0..RING_CAPACITY)
            .map(|_| CurrentStatsSlot {
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("gsplat-exact-current-stats-readback"),
                    size: READBACK_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(SLOT_IDLE)),
                publication: Arc::new(AtomicU8::new(PUBLICATION_UNPUBLISHED)),
                completion_ms_bits: Arc::new(AtomicU32::new(COMPLETION_PENDING_BITS)),
                ticket: CurrentStatsTicket(0),
                descriptor: None,
                submission: None,
                terminal_reported: false,
            })
            .collect();
        Self { slots }
    }
}

pub(super) struct CurrentStatsLane {
    slots: Vec<CurrentStatsSlot>,
    pending_request: bool,
    fresh_cpu_order_required: bool,
    request_unsampled: Option<CurrentStatsUnsampledReason>,
    reserved: Option<usize>,
    next_slot: usize,
    next_ticket: u64,
    terminals: VecDeque<CurrentStatsTerminal>,
    inherited_queue_barriers: Vec<Arc<AtomicU8>>,
    #[cfg(test)]
    encoded_copy_count: u64,
    #[cfg(test)]
    encoded_observer_count: u64,
    #[cfg(test)]
    map_arm_count: u64,
    #[cfg(test)]
    device_poll_count: u64,
}

impl CurrentStatsLane {
    pub(super) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            pending_request: false,
            fresh_cpu_order_required: false,
            request_unsampled: None,
            reserved: None,
            next_slot: 0,
            next_ticket: 1,
            terminals: VecDeque::new(),
            inherited_queue_barriers: Vec::new(),
            #[cfg(test)]
            encoded_copy_count: 0,
            #[cfg(test)]
            encoded_observer_count: 0,
            #[cfg(test)]
            map_arm_count: 0,
            #[cfg(test)]
            device_poll_count: 0,
        }
    }

    pub(super) fn install_capability(&mut self, pool: CurrentStatsReadbackPool) {
        debug_assert!(self.slots.is_empty());
        self.slots = pool.slots;
    }

    pub(super) fn request(&mut self, fresh_cpu_order_required: bool) -> CurrentStatsRequest {
        if self.pending_request || self.request_unsampled.is_some() {
            return CurrentStatsRequest::Unsampled(CurrentStatsUnsampledReason::Busy);
        }
        if self.slots.is_empty() {
            return CurrentStatsRequest::Unsampled(
                CurrentStatsUnsampledReason::ResourceUnavailable,
            );
        }
        match self.reserve() {
            Ok(()) => {
                self.pending_request = true;
                self.fresh_cpu_order_required = fresh_cpu_order_required;
                CurrentStatsRequest::Requested
            }
            Err(reason) => CurrentStatsRequest::Unsampled(reason),
        }
    }

    /// Upgrades an already accepted observer request when its logical camera
    /// transaction is invalidated after admission. The requirement is
    /// one-way until that request issues or resolves unsampled.
    pub(super) fn require_fresh_cpu_order(&mut self) {
        if self.pending_request {
            self.fresh_cpu_order_required = true;
        }
    }

    pub(super) const fn fresh_cpu_order_required(&self) -> bool {
        self.pending_request && self.fresh_cpu_order_required
    }

    fn reserve(&mut self) -> Result<(), CurrentStatsUnsampledReason> {
        let outstanding = self.terminals.len()
            + self
                .slots
                .iter()
                .filter(|slot| {
                    slot.state.load(Ordering::Acquire) != SLOT_IDLE && !slot.terminal_reported
                })
                .count();
        if outstanding >= RING_CAPACITY {
            return Err(CurrentStatsUnsampledReason::Busy);
        }
        let Some(slot_index) = (0..self.slots.len())
            .map(|offset| (self.next_slot + offset) % self.slots.len())
            .find(|&index| self.slots[index].state.load(Ordering::Acquire) == SLOT_IDLE)
        else {
            return Err(CurrentStatsUnsampledReason::Busy);
        };
        let Some(next_ticket) = self.next_ticket.checked_add(1) else {
            return Err(CurrentStatsUnsampledReason::TicketExhausted);
        };
        let ticket = CurrentStatsTicket(self.next_ticket);
        self.next_ticket = next_ticket;
        self.next_slot = (slot_index + 1) % self.slots.len();

        let slot = &mut self.slots[slot_index];
        slot.ticket = ticket;
        slot.descriptor = None;
        slot.submission = None;
        slot.terminal_reported = false;
        slot.publication
            .store(PUBLICATION_UNPUBLISHED, Ordering::Release);
        slot.completion_ms_bits
            .store(COMPLETION_PENDING_BITS, Ordering::Release);
        slot.state.store(SLOT_RESERVED, Ordering::Release);
        self.reserved = Some(slot_index);
        Ok(())
    }

    /// Returns true only when this requested observer lane owns a slot for the
    /// next Exact frame. Callers use this guard before encoding any
    /// count-production work, preserving the zero-work no-request path.
    pub(super) fn prepare_encode(&mut self, device: &wgpu::Device) -> bool {
        if !self.pending_request {
            return false;
        }
        if self.reserved.is_none() {
            // Presentation abandonment publishes no ticket, so there may be
            // no caller-visible terminal to motivate a poll. While the same
            // request is still active, one non-blocking poll is permitted to
            // recycle only completed abandoned attempts before retrying.
            self.poll_device(device);
            self.recycle_abandoned();
        }
        if self.reserved.is_none() && self.reserve().is_err() {
            // The request remains pending and will retry on a later eligible
            // frame after non-blocking polling recycles an older attempt.
            return false;
        }
        true
    }

    fn recycle_abandoned(&mut self) {
        for slot in &mut self.slots {
            if slot.publication.load(Ordering::Acquire) != PUBLICATION_ABANDONED {
                continue;
            }
            if matches!(
                slot.state.load(Ordering::Acquire),
                SLOT_MAPPED | SLOT_MAP_ERROR
            ) {
                slot.readback.unmap();
                recycle(slot);
            }
        }
    }

    pub(super) fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        counts: CurrentStatsFrameCounts<'_>,
    ) -> Option<StagedCurrentStats> {
        debug_assert!(self.pending_request);
        let slot_index = self.reserved.take()?;
        let slot = &mut self.slots[slot_index];
        debug_assert_eq!(slot.state.load(Ordering::Acquire), SLOT_RESERVED);

        let host_visible = match counts.visible {
            CurrentStatsVisibleSource::Host(visible) => Some(visible),
            CurrentStatsVisibleSource::Gpu(source) => {
                copy_count(encoder, source, &slot.readback, VISIBLE_OFFSET);
                #[cfg(test)]
                {
                    self.encoded_copy_count += 1;
                }
                None
            }
        };
        copy_count(
            encoder,
            counts.contributor,
            &slot.readback,
            CONTRIBUTOR_OFFSET,
        );
        #[cfg(test)]
        {
            self.encoded_copy_count += 1;
        }
        slot.descriptor = Some(EncodedDescriptor {
            frame: counts.frame,
            plan: counts.plan,
            order_generation: counts.order_generation,
            source_count: counts.source_count,
            host_visible,
            count_semantics: counts.count_semantics,
            cpu_preprocess_ms_bits: counts.cpu_preprocess_ms.map(f32::to_bits),
            cpu_sort_ms_bits: counts.cpu_sort_ms.map(f32::to_bits),
        });
        slot.state.store(SLOT_ENCODED, Ordering::Release);
        #[cfg(test)]
        {
            self.encoded_observer_count += 1;
        }
        Some(StagedCurrentStats {
            slot: slot_index,
            ticket: slot.ticket,
            state: Arc::clone(&slot.state),
        })
    }

    pub(super) fn arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        staged: StagedCurrentStats,
        completion_started: crate::TimerInstant,
    ) -> ArmedCurrentStats {
        let slot = &mut self.slots[staged.slot];
        debug_assert_eq!(slot.ticket, staged.ticket);
        debug_assert_eq!(slot.state.load(Ordering::Acquire), SLOT_ENCODED);
        slot.state.store(SLOT_SUBMITTED, Ordering::Release);
        let callback_completion = Arc::clone(&slot.completion_ms_bits);
        command_buffer.on_submitted_work_done(move || {
            callback_completion.store(
                crate::timer_elapsed_ms(completion_started).to_bits(),
                Ordering::Release,
            );
        });
        let callback_state = Arc::clone(&slot.state);
        command_buffer.map_buffer_on_submit(
            &slot.readback,
            wgpu::MapMode::Read,
            0..READBACK_BYTES,
            move |result| {
                let terminal = if result.is_ok() {
                    SLOT_MAPPED
                } else {
                    SLOT_MAP_ERROR
                };
                // Expiry, test-injected failure, abandonment cleanup or a
                // future reuse may already have moved this slot. A late map
                // callback may complete only its own submitted generation.
                let _ = callback_state.compare_exchange(
                    SLOT_SUBMITTED,
                    terminal,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            },
        );
        #[cfg(test)]
        {
            self.map_arm_count += 1;
        }
        let armed = ArmedCurrentStats {
            slot: staged.slot,
            ticket: staged.ticket,
            publication: Arc::clone(&slot.publication),
            committed: false,
        };
        drop(staged);
        armed
    }

    pub(super) fn commit(
        &mut self,
        mut armed: ArmedCurrentStats,
        encode_attempt: u64,
        presentation_sequence: u64,
    ) -> CurrentStatsSubmission {
        let slot = &mut self.slots[armed.slot];
        debug_assert_eq!(slot.ticket, armed.ticket);
        let descriptor = slot
            .descriptor
            .expect("armed current-stats slot has an encoded descriptor");
        let submission = CurrentStatsSubmissionReceipt {
            ticket: armed.ticket,
            join: CurrentStatsJoinIdentity {
                frame: descriptor.frame,
                plan: descriptor.plan,
                order_generation: descriptor.order_generation,
                // CanonicalRaster is prepared and published atomically with
                // this PlanSet generation.
                raster_generation: descriptor.frame.plan_set_generation(),
                encode_attempt,
                presentation_sequence,
            },
        };
        slot.submission = Some(submission);
        slot.publication
            .store(PUBLICATION_COMMITTED, Ordering::Release);
        self.pending_request = false;
        self.fresh_cpu_order_required = false;
        armed.committed = true;
        CurrentStatsSubmission::Issued(submission)
    }

    pub(super) fn accepts_armed(
        &self,
        armed: &ArmedCurrentStats,
        frame: FrameIdentity,
        plan: PlanId,
    ) -> bool {
        let Some(slot) = self.slots.get(armed.slot) else {
            return false;
        };
        let state = slot.state.load(Ordering::Acquire);
        slot.ticket == armed.ticket
            && slot.submission.is_none()
            && slot
                .descriptor
                .is_some_and(|descriptor| descriptor.frame == frame && descriptor.plan == plan)
            && matches!(state, SLOT_SUBMITTED | SLOT_MAPPED | SLOT_MAP_ERROR)
            && slot.publication.load(Ordering::Acquire) == PUBLICATION_UNPUBLISHED
    }

    pub(super) fn resolve_request_unsampled(
        &mut self,
        reason: CurrentStatsUnsampledReason,
    ) -> bool {
        if !self.pending_request || self.request_unsampled.is_some() {
            return false;
        }
        if let Some(slot_index) = self.reserved.take() {
            let slot = &mut self.slots[slot_index];
            if slot.state.load(Ordering::Acquire) == SLOT_RESERVED {
                recycle(slot);
            }
        }
        self.pending_request = false;
        self.fresh_cpu_order_required = false;
        self.request_unsampled = Some(reason);
        true
    }

    pub(super) fn poll(&mut self, device: Option<&wgpu::Device>) -> CurrentStatsPoll {
        if self.slots.iter().any(|slot| {
            matches!(
                slot.state.load(Ordering::Acquire),
                SLOT_SUBMITTED | SLOT_MAPPED | SLOT_MAP_ERROR
            )
        }) && let Some(device) = device
        {
            self.poll_device(device);
        }

        for slot in &mut self.slots {
            match slot.state.load(Ordering::Acquire) {
                SLOT_MAPPED => {
                    if slot.submission.is_none()
                        && slot.publication.load(Ordering::Acquire) == PUBLICATION_UNPUBLISHED
                    {
                        continue;
                    }
                    if slot.terminal_reported
                        || slot.publication.load(Ordering::Acquire) == PUBLICATION_ABANDONED
                    {
                        slot.readback.unmap();
                        recycle(slot);
                        continue;
                    }
                    let completion_ms_bits = slot.completion_ms_bits.load(Ordering::Acquire);
                    if completion_ms_bits == COMPLETION_PENDING_BITS {
                        // Mapping and command-completion callbacks are both
                        // fired by Device::poll, but their relative order is
                        // intentionally not part of the API. Keep the mapped
                        // slot intact until the queue-completion timestamp is
                        // visible so the compatibility receipt never reports
                        // map latency as FrameCompletion.
                        continue;
                    }
                    let bytes = slot.readback.slice(0..READBACK_BYTES).get_mapped_range();
                    let gpu_visible = read_u32(&bytes[0..4]);
                    let contributor = read_u32(&bytes[4..8]);
                    drop(bytes);
                    slot.readback.unmap();
                    let descriptor = slot
                        .descriptor
                        .expect("published current-stats slot has a descriptor");
                    let submission = slot
                        .submission
                        .expect("published current-stats slot has a submission");
                    let visible = descriptor.host_visible.unwrap_or(gpu_visible);
                    let drawn = match descriptor.count_semantics {
                        PlanCountSemantics::DirectDrawEqualsVisible
                        | PlanCountSemantics::IndirectDrawEqualsVisible => visible,
                        PlanCountSemantics::IndirectDrawEqualsContributor => contributor,
                    };
                    let counts = CurrentStatsCounts {
                        source: descriptor.source_count,
                        visible,
                        contributor,
                        drawn,
                    };
                    let terminal = if counts_are_valid(counts, descriptor.count_semantics) {
                        CurrentStatsTerminal::Ready(CurrentStatsReceipt {
                            submission,
                            counts,
                            count_semantics: descriptor.count_semantics,
                            frame_complete_ms_bits: completion_ms_bits,
                            cpu_preprocess_ms_bits: descriptor.cpu_preprocess_ms_bits,
                            cpu_sort_ms_bits: descriptor.cpu_sort_ms_bits,
                        })
                    } else {
                        CurrentStatsTerminal::Dropped(CurrentStatsFailure { submission })
                    };
                    slot.terminal_reported = true;
                    self.terminals.push_back(terminal);
                    recycle(slot);
                }
                SLOT_MAP_ERROR => {
                    if slot.submission.is_none()
                        && slot.publication.load(Ordering::Acquire) == PUBLICATION_UNPUBLISHED
                    {
                        continue;
                    }
                    if !slot.terminal_reported
                        && let Some(submission) = slot.submission
                    {
                        slot.terminal_reported = true;
                        self.terminals.push_back(CurrentStatsTerminal::MapFailure(
                            CurrentStatsFailure { submission },
                        ));
                    }
                    slot.readback.unmap();
                    recycle(slot);
                }
                _ => {}
            }
        }
        self.terminals
            .make_contiguous()
            .sort_by_key(|terminal| terminal.ticket());
        if let Some(reason) = self.request_unsampled.take() {
            CurrentStatsPoll::Unsampled(reason)
        } else if let Some(terminal) = self.terminals.pop_front() {
            CurrentStatsPoll::Terminal(terminal)
        } else {
            CurrentStatsPoll::Empty
        }
    }

    pub(super) fn expire(&mut self, ticket: CurrentStatsTicket) -> bool {
        let Some(slot) = self.slots.iter_mut().find(|slot| {
            slot.ticket == ticket && slot.submission.is_some() && !slot.terminal_reported
        }) else {
            return false;
        };
        let submission = slot.submission.expect("issued ticket has submission");
        slot.terminal_reported = true;
        self.terminals
            .push_back(CurrentStatsTerminal::Expired(CurrentStatsFailure {
                submission,
            }));
        if slot.state.load(Ordering::Acquire) == SLOT_MAPPED {
            slot.readback.unmap();
            recycle(slot);
        }
        true
    }

    pub(super) fn replacement_handoff(&self, observer_since_formal: bool) -> CurrentStatsHandoff {
        let mut terminals = self.terminals.clone();
        for slot in &self.slots {
            if !slot.terminal_reported
                && let Some(submission) = slot.submission
            {
                terminals.push_back(CurrentStatsTerminal::GenerationInvalidated(
                    CurrentStatsFailure { submission },
                ));
            }
        }
        terminals
            .make_contiguous()
            .sort_by_key(|terminal| terminal.ticket());
        debug_assert!(terminals.len() <= RING_CAPACITY);
        let mut inherited_queue_barriers = self.inherited_queue_barriers.clone();
        inherited_queue_barriers.extend(
            self.slots
                .iter()
                .filter(|slot| slot.state.load(Ordering::Acquire) == SLOT_SUBMITTED)
                .map(|slot| Arc::clone(&slot.state)),
        );
        CurrentStatsHandoff {
            next_ticket: self.next_ticket,
            terminals,
            pending_request: self.pending_request,
            fresh_cpu_order_required: self.fresh_cpu_order_required,
            request_unsampled: self.request_unsampled,
            inherited_queue_barriers,
            observer_since_formal,
        }
    }

    pub(super) fn import_handoff(&mut self, handoff: CurrentStatsHandoff) -> bool {
        debug_assert!(self.slots.is_empty());
        debug_assert!(self.terminals.is_empty());
        self.next_ticket = handoff.next_ticket;
        self.terminals = handoff.terminals;
        self.pending_request = handoff.pending_request;
        self.fresh_cpu_order_required = handoff.fresh_cpu_order_required;
        self.request_unsampled = handoff.request_unsampled;
        self.inherited_queue_barriers = handoff.inherited_queue_barriers;
        handoff.observer_since_formal
    }

    /// Captures queue safety before Renderer performs its existing mandatory
    /// sampler progress poll. A callback fired by that later poll cannot make
    /// the current frame retrospectively safe; only a subsequent frame may
    /// start a formal interval.
    pub(super) fn formal_queue_safe_at_frame_entry(&mut self) -> bool {
        self.inherited_queue_barriers
            .retain(|state| state.load(Ordering::Acquire) == SLOT_SUBMITTED);
        self.inherited_queue_barriers.is_empty()
            && !self
                .slots
                .iter()
                .any(|slot| slot.state.load(Ordering::Acquire) == SLOT_SUBMITTED)
    }

    #[cfg(test)]
    pub(super) const fn encoded_copy_count(&self) -> u64 {
        self.encoded_copy_count
    }

    #[cfg(test)]
    pub(super) fn allocated_readback_bytes(&self) -> u64 {
        self.slots.iter().map(|slot| slot.readback.size()).sum()
    }

    #[cfg(test)]
    pub(super) const fn encoded_observer_count(&self) -> u64 {
        self.encoded_observer_count
    }

    #[cfg(test)]
    pub(super) const fn map_arm_count(&self) -> u64 {
        self.map_arm_count
    }

    #[cfg(test)]
    pub(super) const fn device_poll_count(&self) -> u64 {
        self.device_poll_count
    }

    pub(super) const fn request_pending(&self) -> bool {
        self.pending_request
    }

    #[cfg(test)]
    pub(super) fn force_map_failure(&mut self, ticket: CurrentStatsTicket) -> bool {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.ticket == ticket && slot.submission.is_some())
        else {
            return false;
        };
        slot.state.store(SLOT_MAP_ERROR, Ordering::Release);
        true
    }

    #[cfg(test)]
    pub(super) fn hold_callback_for_test(&mut self, ticket: CurrentStatsTicket) -> bool {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.ticket == ticket && slot.submission.is_some())
        else {
            return false;
        };
        // The callback CAS accepts only SUBMITTED, so this deterministic hold
        // lets the test deliver a later terminal first.
        slot.state.store(SLOT_ENCODED, Ordering::Release);
        true
    }

    fn poll_device(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        #[cfg(test)]
        {
            self.device_poll_count += 1;
        }
    }
}

fn copy_count(
    encoder: &mut wgpu::CommandEncoder,
    source: GpuCountSource<'_>,
    destination: &wgpu::Buffer,
    destination_offset: u64,
) {
    encoder.copy_buffer_to_buffer(
        source.buffer(),
        source.offset(),
        destination,
        destination_offset,
        size_of::<u32>() as u64,
    );
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("current-stats u32 slice width"))
}

fn counts_are_valid(counts: CurrentStatsCounts, semantics: PlanCountSemantics) -> bool {
    counts.contributor <= counts.visible
        && counts.visible <= counts.source
        && match semantics {
            PlanCountSemantics::DirectDrawEqualsVisible
            | PlanCountSemantics::IndirectDrawEqualsVisible => counts.drawn == counts.visible,
            PlanCountSemantics::IndirectDrawEqualsContributor => counts.drawn == counts.contributor,
        }
}

fn recycle(slot: &mut CurrentStatsSlot) {
    slot.descriptor = None;
    slot.submission = None;
    slot.state.store(SLOT_IDLE, Ordering::Release);
}
