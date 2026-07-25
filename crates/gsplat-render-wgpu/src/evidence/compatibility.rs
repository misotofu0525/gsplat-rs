use std::{collections::VecDeque, num::NonZeroU64};

use crate::evidence::BoundedEvidenceRing;
use crate::gpu_telemetry::SurfaceCpuOrderMeasurement;
use crate::{
    SurfaceGpuOrderProducer, SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
    SurfaceOrderBackendUsed, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfaceOrderMeasurementFailureReason, SurfaceProjectedDrawExecution,
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
    SurfaceProjectedDrawMeasurementFailureReason, SurfaceTimingSource,
    surface_session::{
        SurfaceAdaptiveState, SurfaceGpuProducerMeasurementSubmission, SurfaceOrderBackend,
        SurfaceOrderMeasurementSubmission, SurfaceProjectedDrawAdaptiveState,
        SurfaceProjectedDrawMeasurementSubmission, SurfaceProjectedDrawPolicy,
    },
};

const TERMINAL_CAPACITY: usize = 64;
const ORDER_COUNT_CAPACITY: usize = 2 * TERMINAL_CAPACITY;
const PROJECTED_COUNT_CAPACITY: usize = TERMINAL_CAPACITY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCompatibilityChannel {
    Order,
    Projected,
    Producer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceCompatibilitySubmission {
    Order(SurfaceCompatibilityOrderSubmission),
    Projected(SurfaceCompatibilityProjectedSubmission),
    Producer(SurfaceCompatibilityProducerSubmission),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityOrderIssueContext {
    pub requested_backend: SurfaceOrderBackend,
    pub actual_backend: SurfaceOrderBackendUsed,
    pub adaptive_state: SurfaceAdaptiveState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityOrderSubmission {
    pub camera_revision: u64,
    pub requested_backend: SurfaceOrderBackend,
    pub actual_backend: SurfaceOrderBackendUsed,
    pub adaptive_state: SurfaceAdaptiveState,
    pub measurement: SurfaceOrderMeasurementSubmission,
}

impl SurfaceCompatibilityOrderSubmission {
    fn issued_context(self, ticket: u64) -> Option<SurfaceCompatibilityOrderIssueContext> {
        match self.measurement {
            SurfaceOrderMeasurementSubmission::Issued {
                backend,
                ticket: issued_ticket,
            } if issued_ticket == ticket => Some(SurfaceCompatibilityOrderIssueContext {
                requested_backend: self.requested_backend,
                actual_backend: backend,
                adaptive_state: self.adaptive_state,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityProjectedIssueContext {
    pub requested_policy: SurfaceProjectedDrawPolicy,
    pub actual_execution: SurfaceProjectedDrawExecution,
    pub order_backend: SurfaceOrderBackendUsed,
    pub adaptive_state: SurfaceProjectedDrawAdaptiveState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityProjectedSubmission {
    pub camera_revision: u64,
    pub requested_policy: SurfaceProjectedDrawPolicy,
    pub actual_execution: SurfaceProjectedDrawExecution,
    pub order_backend: SurfaceOrderBackendUsed,
    pub adaptive_state: SurfaceProjectedDrawAdaptiveState,
    pub measurement: SurfaceProjectedDrawMeasurementSubmission,
}

impl SurfaceCompatibilityProjectedSubmission {
    fn issued_context(self, ticket: u64) -> Option<SurfaceCompatibilityProjectedIssueContext> {
        match self.measurement {
            SurfaceProjectedDrawMeasurementSubmission::Issued {
                execution,
                ticket: issued_ticket,
            } if issued_ticket == ticket => Some(SurfaceCompatibilityProjectedIssueContext {
                requested_policy: self.requested_policy,
                actual_execution: execution,
                order_backend: self.order_backend,
                adaptive_state: self.adaptive_state,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityProducerIssueContext {
    pub requested_producer: SurfaceGpuOrderProducer,
    pub actual_producer: Option<SurfaceGpuOrderProducer>,
    pub order_backend: SurfaceOrderBackendUsed,
    pub projected_execution: SurfaceProjectedDrawExecution,
    pub measurement_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityProducerSubmission {
    pub camera_revision: u64,
    pub requested_producer: SurfaceGpuOrderProducer,
    pub actual_producer: Option<SurfaceGpuOrderProducer>,
    pub order_backend: SurfaceOrderBackendUsed,
    pub projected_execution: SurfaceProjectedDrawExecution,
    pub measurement_enabled: bool,
    pub measurement: SurfaceGpuProducerMeasurementSubmission,
}

impl SurfaceCompatibilityProducerSubmission {
    fn issued_context(self, ticket: u64) -> Option<SurfaceCompatibilityProducerIssueContext> {
        match self.measurement {
            SurfaceGpuProducerMeasurementSubmission::Issued {
                producer,
                ticket: issued_ticket,
            } if issued_ticket == ticket => Some(SurfaceCompatibilityProducerIssueContext {
                requested_producer: self.requested_producer,
                actual_producer: Some(producer),
                order_backend: self.order_backend,
                projected_execution: self.projected_execution,
                measurement_enabled: self.measurement_enabled,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCompatibilityTerminalSelector {
    OrderCpuSuccess,
    OrderGpuSuccess,
    OrderFailure,
    ProjectedSuccess,
    ProjectedFailure,
    ProducerSuccess,
    ProducerFailure,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceCompatibilityTerminal {
    OrderCpuSuccess(SurfaceCompatibilityOrderCpuSuccess),
    OrderGpuSuccess(SurfaceCompatibilityOrderGpuSuccess),
    OrderFailure(SurfaceCompatibilityOrderFailure),
    ProjectedSuccess(SurfaceCompatibilityProjectedSuccess),
    ProjectedFailure(SurfaceCompatibilityProjectedFailure),
    ProducerSuccess(SurfaceCompatibilityProducerSuccess),
    ProducerFailure(SurfaceCompatibilityProducerFailure),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCompatibilityOrderCpuSuccess {
    pub issue: SurfaceCompatibilityOrderIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub preprocess_ms: f32,
    pub sort_ms: f32,
    pub frame_complete_ms: f32,
    pub contributor_count: u32,
    pub exact_contributor_compaction: bool,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCompatibilityOrderGpuSuccess {
    pub issue: SurfaceCompatibilityOrderIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub timing_source: SurfaceTimingSource,
    pub gpu_preprocess_ms: Option<f32>,
    pub gpu_radix_ms: Option<f32>,
    pub gpu_order_ms: Option<f32>,
    pub gpu_complete_ms: f32,
    pub timestamp_period_ns: Option<f32>,
    pub below_timestamp_resolution: bool,
    pub visible_count: u32,
    pub drawn_count: u32,
    pub exact_contributor_compaction: bool,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityOrderFailure {
    pub issue: SurfaceCompatibilityOrderIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub reason: SurfaceOrderMeasurementFailureReason,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCompatibilityProjectedSuccess {
    pub issue: SurfaceCompatibilityProjectedIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub execution: SurfaceProjectedDrawExecution,
    pub order_backend: SurfaceOrderBackendUsed,
    pub projection_generation: u64,
    pub probe_generation: u64,
    pub projection_rebuilt: bool,
    pub order_refreshed: bool,
    pub frame_complete_ms: f32,
    pub exact_contributor_compaction: bool,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityProjectedFailure {
    pub issue: SurfaceCompatibilityProjectedIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub execution: SurfaceProjectedDrawExecution,
    pub order_backend: SurfaceOrderBackendUsed,
    pub projection_generation: u64,
    pub probe_generation: u64,
    pub reason: SurfaceProjectedDrawMeasurementFailureReason,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCompatibilityProducerSuccess {
    pub issue: SurfaceCompatibilityProducerIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub producer: SurfaceGpuOrderProducer,
    pub order_generation: u64,
    pub projection_generation: u64,
    pub source_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub order_refreshed: bool,
    pub draw_scope: SurfaceGpuProducerDrawScope,
    pub frame_complete_ms: f32,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityProducerFailure {
    pub issue: SurfaceCompatibilityProducerIssueContext,
    pub ticket: u64,
    pub camera_revision: u64,
    pub producer: SurfaceGpuOrderProducer,
    pub order_generation: u64,
    pub projection_generation: u64,
    pub reason: SurfaceGpuProducerMeasurementFailureReason,
    pub dropped_prior: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCompatibilityTerminalUnavailable {
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceCompatibilityTerminalPoll {
    Ready(SurfaceCompatibilityTerminal),
    Unavailable(SurfaceCompatibilityTerminalUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCompatibilityCountFamily {
    Order,
    Projected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityCounts {
    pub family: SurfaceCompatibilityCountFamily,
    pub ticket: u64,
    pub camera_revision: u64,
    pub visible_count: u32,
    pub contributor_count: u32,
    pub drawn_count: u32,
    pub exact_contributor_compaction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCompatibilityCountsUnavailableReason {
    Pending,
    Failed,
    Expired,
    Consumed,
    InvalidTicket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCompatibilityCountsUnavailable {
    pub family: SurfaceCompatibilityCountFamily,
    pub ticket: NonZeroU64,
    pub reason: SurfaceCompatibilityCountsUnavailableReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceCompatibilityCountsTake {
    Ready(SurfaceCompatibilityCounts),
    Unavailable(SurfaceCompatibilityCountsUnavailable),
}

#[derive(Debug, Clone, Copy)]
struct AtomicTerminal<T, C> {
    payload: T,
    issue: C,
    dropped_prior: bool,
}

struct BoundedTerminalQueue<T, C> {
    entries: BoundedEvidenceRing<AtomicTerminal<T, C>>,
    len: usize,
}

impl<T: Copy, C: Copy> BoundedTerminalQueue<T, C> {
    fn new() -> Self {
        Self {
            entries: BoundedEvidenceRing::new(),
            len: 0,
        }
    }

    fn push(&mut self, payload: T, issue: C) {
        let dropped_prior = self.len == TERMINAL_CAPACITY;
        self.entries.push(AtomicTerminal {
            payload,
            issue,
            dropped_prior,
        });
        self.len = (self.len + 1).min(TERMINAL_CAPACITY);
    }

    fn pop(&mut self) -> Option<AtomicTerminal<T, C>> {
        let mut retained = [None; TERMINAL_CAPACITY];
        let mut drained = self.entries.drain();
        let first = drained.next();
        for (slot, entry) in retained.iter_mut().zip(drained) {
            *slot = Some(entry);
        }
        for entry in retained.into_iter().flatten() {
            self.entries.push(entry);
        }
        self.len = self.len.saturating_sub(usize::from(first.is_some()));
        first
    }

    fn drain(&mut self) -> impl Iterator<Item = AtomicTerminal<T, C>> + '_ {
        self.len = 0;
        self.entries.drain()
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CountOutcome {
    Failed,
    Expired,
    Consumed,
}

struct CountLedger {
    family: SurfaceCompatibilityCountFamily,
    capacity: usize,
    ready: VecDeque<SurfaceCompatibilityCounts>,
    outcomes: VecDeque<(u64, CountOutcome)>,
}

impl CountLedger {
    fn new(family: SurfaceCompatibilityCountFamily, capacity: usize) -> Self {
        Self {
            family,
            capacity,
            ready: VecDeque::with_capacity(capacity),
            outcomes: VecDeque::with_capacity(capacity),
        }
    }

    fn record_outcome(&mut self, ticket: u64, outcome: CountOutcome) {
        self.outcomes.retain(|(recorded, _)| *recorded != ticket);
        if self.outcomes.len() == self.capacity {
            self.outcomes.pop_front();
        }
        self.outcomes.push_back((ticket, outcome));
    }

    fn begin(&mut self, ticket: u64) {
        self.ready.retain(|counts| counts.ticket != ticket);
        self.outcomes.retain(|(recorded, _)| *recorded != ticket);
    }

    fn publish(&mut self, counts: SurfaceCompatibilityCounts) {
        debug_assert_eq!(counts.family, self.family);
        self.ready.retain(|entry| entry.ticket != counts.ticket);
        self.outcomes.retain(|(ticket, _)| *ticket != counts.ticket);
        if self.ready.len() == self.capacity
            && let Some(expired) = self.ready.pop_front()
        {
            self.record_outcome(expired.ticket, CountOutcome::Expired);
        }
        self.ready.push_back(counts);
    }

    fn fail(&mut self, ticket: u64) {
        self.ready.retain(|counts| counts.ticket != ticket);
        self.record_outcome(ticket, CountOutcome::Failed);
    }

    fn take(&mut self, ticket: NonZeroU64, pending: bool) -> SurfaceCompatibilityCountsTake {
        if let Some(index) = self
            .ready
            .iter()
            .position(|counts| counts.ticket == ticket.get())
        {
            let counts = self.ready.remove(index).expect("located count receipt");
            self.record_outcome(ticket.get(), CountOutcome::Consumed);
            return SurfaceCompatibilityCountsTake::Ready(counts);
        }
        let reason = self
            .outcomes
            .iter()
            .rev()
            .find_map(|(recorded, outcome)| (*recorded == ticket.get()).then_some(*outcome))
            .map(|outcome| match outcome {
                CountOutcome::Failed => SurfaceCompatibilityCountsUnavailableReason::Failed,
                CountOutcome::Expired => SurfaceCompatibilityCountsUnavailableReason::Expired,
                CountOutcome::Consumed => SurfaceCompatibilityCountsUnavailableReason::Consumed,
            })
            .unwrap_or(if pending {
                SurfaceCompatibilityCountsUnavailableReason::Pending
            } else {
                SurfaceCompatibilityCountsUnavailableReason::InvalidTicket
            });
        SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
            family: self.family,
            ticket,
            reason,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct ProducerEntry<T, C> {
    terminal: AtomicTerminal<T, C>,
    raw_visible: bool,
    compatibility_visible: bool,
}

struct ProducerTerminalQueue<T, C> {
    entries: VecDeque<ProducerEntry<T, C>>,
    compatibility_visible: usize,
}

impl<T: Copy, C: Copy> ProducerTerminalQueue<T, C> {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            compatibility_visible: 0,
        }
    }

    fn push(&mut self, payload: T, issue: C) {
        let dropped_prior = self.compatibility_visible == TERMINAL_CAPACITY;
        if dropped_prior
            && let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.compatibility_visible)
        {
            entry.compatibility_visible = false;
            self.compatibility_visible -= 1;
        }
        self.entries.push_back(ProducerEntry {
            terminal: AtomicTerminal {
                payload,
                issue,
                dropped_prior,
            },
            raw_visible: true,
            compatibility_visible: true,
        });
        self.compatibility_visible += 1;
    }

    fn poll(&mut self) -> Option<AtomicTerminal<T, C>> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.compatibility_visible)?;
        entry.compatibility_visible = false;
        entry.raw_visible = false;
        self.compatibility_visible -= 1;
        let terminal = entry.terminal;
        self.entries
            .retain(|entry| entry.raw_visible || entry.compatibility_visible);
        Some(terminal)
    }

    fn drain_raw(&mut self) -> Vec<T> {
        let mut drained = Vec::with_capacity(self.entries.len());
        for entry in &mut self.entries {
            if entry.raw_visible {
                drained.push(entry.terminal.payload);
                entry.raw_visible = false;
            }
            if entry.compatibility_visible {
                entry.compatibility_visible = false;
                self.compatibility_visible -= 1;
            }
        }
        self.entries.clear();
        drained
    }

    fn compatibility_is_empty(&self) -> bool {
        self.compatibility_visible == 0
    }
}

pub(crate) struct CompatibilityEvidenceStore {
    last_order_submission: Option<SurfaceCompatibilityOrderSubmission>,
    last_projected_submission: Option<SurfaceCompatibilityProjectedSubmission>,
    last_producer_submission: Option<SurfaceCompatibilityProducerSubmission>,
    pending_order: VecDeque<(u64, SurfaceCompatibilityOrderIssueContext)>,
    pending_projected: VecDeque<(u64, SurfaceCompatibilityProjectedIssueContext)>,
    pending_producer: VecDeque<(u64, SurfaceCompatibilityProducerIssueContext)>,
    order_cpu_success:
        BoundedTerminalQueue<SurfaceCpuOrderMeasurement, SurfaceCompatibilityOrderIssueContext>,
    order_gpu_success:
        BoundedTerminalQueue<SurfaceOrderMeasurement, SurfaceCompatibilityOrderIssueContext>,
    order_failure:
        BoundedTerminalQueue<SurfaceOrderMeasurementFailure, SurfaceCompatibilityOrderIssueContext>,
    projected_success: BoundedTerminalQueue<
        SurfaceProjectedDrawMeasurement,
        SurfaceCompatibilityProjectedIssueContext,
    >,
    projected_failure: BoundedTerminalQueue<
        SurfaceProjectedDrawMeasurementFailure,
        SurfaceCompatibilityProjectedIssueContext,
    >,
    producer_success: ProducerTerminalQueue<
        SurfaceGpuProducerMeasurement,
        SurfaceCompatibilityProducerIssueContext,
    >,
    producer_failure: ProducerTerminalQueue<
        SurfaceGpuProducerMeasurementFailure,
        SurfaceCompatibilityProducerIssueContext,
    >,
    order_counts: CountLedger,
    projected_counts: CountLedger,
}

impl CompatibilityEvidenceStore {
    pub(crate) fn new() -> Self {
        Self {
            last_order_submission: None,
            last_projected_submission: None,
            last_producer_submission: None,
            pending_order: VecDeque::new(),
            pending_projected: VecDeque::new(),
            pending_producer: VecDeque::new(),
            order_cpu_success: BoundedTerminalQueue::new(),
            order_gpu_success: BoundedTerminalQueue::new(),
            order_failure: BoundedTerminalQueue::new(),
            projected_success: BoundedTerminalQueue::new(),
            projected_failure: BoundedTerminalQueue::new(),
            producer_success: ProducerTerminalQueue::new(),
            producer_failure: ProducerTerminalQueue::new(),
            order_counts: CountLedger::new(
                SurfaceCompatibilityCountFamily::Order,
                ORDER_COUNT_CAPACITY,
            ),
            projected_counts: CountLedger::new(
                SurfaceCompatibilityCountFamily::Projected,
                PROJECTED_COUNT_CAPACITY,
            ),
        }
    }

    pub(crate) fn observe_submissions(
        &mut self,
        order: SurfaceCompatibilityOrderSubmission,
        projected: SurfaceCompatibilityProjectedSubmission,
        producer: SurfaceCompatibilityProducerSubmission,
    ) {
        self.last_order_submission = Some(order);
        self.last_projected_submission = Some(projected);
        self.last_producer_submission = Some(producer);
        if let Some(ticket) = order.measurement.ticket()
            && let Some(issue) = order.issued_context(ticket)
        {
            self.order_counts.begin(ticket);
            replace_pending(&mut self.pending_order, ticket, issue);
        }
        if let Some(ticket) = projected.measurement.ticket()
            && let Some(issue) = projected.issued_context(ticket)
        {
            self.projected_counts.begin(ticket);
            replace_pending(&mut self.pending_projected, ticket, issue);
        }
        if let Some(ticket) = producer.measurement.ticket()
            && let Some(issue) = producer.issued_context(ticket)
        {
            replace_pending(&mut self.pending_producer, ticket, issue);
        }
    }

    pub(crate) fn submission(
        &self,
        channel: SurfaceCompatibilityChannel,
    ) -> Option<SurfaceCompatibilitySubmission> {
        match channel {
            SurfaceCompatibilityChannel::Order => self
                .last_order_submission
                .map(SurfaceCompatibilitySubmission::Order),
            SurfaceCompatibilityChannel::Projected => self
                .last_projected_submission
                .map(SurfaceCompatibilitySubmission::Projected),
            SurfaceCompatibilityChannel::Producer => self
                .last_producer_submission
                .map(SurfaceCompatibilitySubmission::Producer),
        }
    }

    pub(crate) fn publish_cpu_order(&mut self, measurement: SurfaceCpuOrderMeasurement) {
        let Some(issue) = take_pending(&mut self.pending_order, measurement.ticket) else {
            return;
        };
        debug_assert_eq!(issue.actual_backend, SurfaceOrderBackendUsed::Cpu);
        self.order_counts.publish(order_counts(
            measurement.ticket,
            measurement.camera_revision,
            measurement.visible_count,
            measurement.contributor_count,
            measurement.drawn_count,
            measurement.exact_contributor_compaction,
        ));
        self.order_cpu_success.push(measurement, issue);
    }

    pub(crate) fn publish_gpu_order(&mut self, measurement: SurfaceOrderMeasurement) {
        let Some(issue) = take_pending(&mut self.pending_order, measurement.ticket) else {
            return;
        };
        debug_assert_eq!(issue.actual_backend, SurfaceOrderBackendUsed::Gpu);
        self.order_counts.publish(order_counts(
            measurement.ticket,
            measurement.camera_revision,
            measurement.visible_count,
            measurement.contributor_count,
            measurement.drawn_count,
            measurement.exact_contributor_compaction,
        ));
        self.order_gpu_success.push(measurement, issue);
    }

    pub(crate) fn publish_order_failure(&mut self, failure: SurfaceOrderMeasurementFailure) {
        let Some(issue) = take_pending(&mut self.pending_order, failure.ticket) else {
            return;
        };
        self.order_counts.fail(failure.ticket);
        self.order_failure.push(failure, issue);
    }

    pub(crate) fn publish_projected_success(
        &mut self,
        measurement: SurfaceProjectedDrawMeasurement,
    ) {
        let Some(issue) = take_pending(&mut self.pending_projected, measurement.ticket) else {
            return;
        };
        self.projected_counts.publish(SurfaceCompatibilityCounts {
            family: SurfaceCompatibilityCountFamily::Projected,
            ticket: measurement.ticket,
            camera_revision: measurement.camera_revision,
            visible_count: measurement.visible_count,
            contributor_count: measurement.contributor_count,
            drawn_count: measurement.drawn_count,
            exact_contributor_compaction: measurement.exact_contributor_compaction,
        });
        self.projected_success.push(measurement, issue);
    }

    pub(crate) fn publish_projected_failure(
        &mut self,
        failure: SurfaceProjectedDrawMeasurementFailure,
    ) {
        let Some(issue) = take_pending(&mut self.pending_projected, failure.ticket) else {
            return;
        };
        self.projected_counts.fail(failure.ticket);
        self.projected_failure.push(failure, issue);
    }

    pub(crate) fn publish_producer_success(&mut self, measurement: SurfaceGpuProducerMeasurement) {
        let Some(issue) = take_pending(&mut self.pending_producer, measurement.ticket) else {
            return;
        };
        self.producer_success.push(measurement, issue);
    }

    pub(crate) fn publish_producer_failure(
        &mut self,
        failure: SurfaceGpuProducerMeasurementFailure,
    ) {
        let Some(issue) = take_pending(&mut self.pending_producer, failure.ticket) else {
            return;
        };
        self.producer_failure.push(failure, issue);
    }

    pub(crate) fn terminal_is_empty(&self, selector: SurfaceCompatibilityTerminalSelector) -> bool {
        match selector {
            SurfaceCompatibilityTerminalSelector::OrderCpuSuccess => {
                self.order_cpu_success.is_empty()
            }
            SurfaceCompatibilityTerminalSelector::OrderGpuSuccess => {
                self.order_gpu_success.is_empty()
            }
            SurfaceCompatibilityTerminalSelector::OrderFailure => self.order_failure.is_empty(),
            SurfaceCompatibilityTerminalSelector::ProjectedSuccess => {
                self.projected_success.is_empty()
            }
            SurfaceCompatibilityTerminalSelector::ProjectedFailure => {
                self.projected_failure.is_empty()
            }
            SurfaceCompatibilityTerminalSelector::ProducerSuccess => {
                self.producer_success.compatibility_is_empty()
            }
            SurfaceCompatibilityTerminalSelector::ProducerFailure => {
                self.producer_failure.compatibility_is_empty()
            }
        }
    }

    pub(crate) fn poll_terminal(
        &mut self,
        selector: SurfaceCompatibilityTerminalSelector,
    ) -> SurfaceCompatibilityTerminalPoll {
        let terminal = match selector {
            SurfaceCompatibilityTerminalSelector::OrderCpuSuccess => self
                .order_cpu_success
                .pop()
                .map(|record| SurfaceCompatibilityTerminal::OrderCpuSuccess(record.into())),
            SurfaceCompatibilityTerminalSelector::OrderGpuSuccess => self
                .order_gpu_success
                .pop()
                .map(|record| SurfaceCompatibilityTerminal::OrderGpuSuccess(record.into())),
            SurfaceCompatibilityTerminalSelector::OrderFailure => self
                .order_failure
                .pop()
                .map(|record| SurfaceCompatibilityTerminal::OrderFailure(record.into())),
            SurfaceCompatibilityTerminalSelector::ProjectedSuccess => self
                .projected_success
                .pop()
                .map(|record| SurfaceCompatibilityTerminal::ProjectedSuccess(record.into())),
            SurfaceCompatibilityTerminalSelector::ProjectedFailure => self
                .projected_failure
                .pop()
                .map(|record| SurfaceCompatibilityTerminal::ProjectedFailure(record.into())),
            SurfaceCompatibilityTerminalSelector::ProducerSuccess => self
                .producer_success
                .poll()
                .map(|record| SurfaceCompatibilityTerminal::ProducerSuccess(record.into())),
            SurfaceCompatibilityTerminalSelector::ProducerFailure => self
                .producer_failure
                .poll()
                .map(|record| SurfaceCompatibilityTerminal::ProducerFailure(record.into())),
        };
        terminal.map_or(
            SurfaceCompatibilityTerminalPoll::Unavailable(
                SurfaceCompatibilityTerminalUnavailable::Empty,
            ),
            SurfaceCompatibilityTerminalPoll::Ready,
        )
    }

    pub(crate) fn take_counts(
        &mut self,
        family: SurfaceCompatibilityCountFamily,
        ticket: NonZeroU64,
    ) -> SurfaceCompatibilityCountsTake {
        let pending = match family {
            SurfaceCompatibilityCountFamily::Order => self
                .pending_order
                .iter()
                .any(|(pending, _)| *pending == ticket.get()),
            SurfaceCompatibilityCountFamily::Projected => self
                .pending_projected
                .iter()
                .any(|(pending, _)| *pending == ticket.get()),
        };
        match family {
            SurfaceCompatibilityCountFamily::Order => self.order_counts.take(ticket, pending),
            SurfaceCompatibilityCountFamily::Projected => {
                self.projected_counts.take(ticket, pending)
            }
        }
    }

    pub(crate) fn drain_cpu_order(&mut self) -> Vec<SurfaceCpuOrderMeasurement> {
        self.order_cpu_success
            .drain()
            .map(|record| record.payload)
            .collect()
    }

    pub(crate) fn drain_gpu_order(&mut self) -> Vec<SurfaceOrderMeasurement> {
        self.order_gpu_success
            .drain()
            .map(|record| record.payload)
            .collect()
    }

    pub(crate) fn drain_order_failures(&mut self) -> Vec<SurfaceOrderMeasurementFailure> {
        self.order_failure
            .drain()
            .map(|record| record.payload)
            .collect()
    }

    pub(crate) fn drain_projected_successes(&mut self) -> Vec<SurfaceProjectedDrawMeasurement> {
        self.projected_success
            .drain()
            .map(|record| record.payload)
            .collect()
    }

    pub(crate) fn drain_projected_failures(
        &mut self,
    ) -> Vec<SurfaceProjectedDrawMeasurementFailure> {
        self.projected_failure
            .drain()
            .map(|record| record.payload)
            .collect()
    }

    pub(crate) fn drain_producer_successes(&mut self) -> Vec<SurfaceGpuProducerMeasurement> {
        self.producer_success.drain_raw()
    }

    pub(crate) fn drain_producer_failures(&mut self) -> Vec<SurfaceGpuProducerMeasurementFailure> {
        self.producer_failure.drain_raw()
    }
}

fn replace_pending<C: Copy>(pending: &mut VecDeque<(u64, C)>, ticket: u64, context: C) {
    pending.retain(|(recorded, _)| *recorded != ticket);
    pending.push_back((ticket, context));
}

fn take_pending<C: Copy>(pending: &mut VecDeque<(u64, C)>, ticket: u64) -> Option<C> {
    pending
        .iter()
        .position(|(recorded, _)| *recorded == ticket)
        .and_then(|index| pending.remove(index))
        .map(|(_, context)| context)
}

fn order_counts(
    ticket: u64,
    camera_revision: u64,
    visible_count: u32,
    contributor_count: u32,
    drawn_count: u32,
    exact_contributor_compaction: bool,
) -> SurfaceCompatibilityCounts {
    SurfaceCompatibilityCounts {
        family: SurfaceCompatibilityCountFamily::Order,
        ticket,
        camera_revision,
        visible_count,
        contributor_count,
        drawn_count,
        exact_contributor_compaction,
    }
}

impl From<AtomicTerminal<SurfaceCpuOrderMeasurement, SurfaceCompatibilityOrderIssueContext>>
    for SurfaceCompatibilityOrderCpuSuccess
{
    fn from(
        record: AtomicTerminal<SurfaceCpuOrderMeasurement, SurfaceCompatibilityOrderIssueContext>,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            preprocess_ms: record.payload.preprocess_ms,
            sort_ms: record.payload.sort_ms,
            frame_complete_ms: record.payload.frame_complete_ms,
            contributor_count: record.payload.contributor_count,
            exact_contributor_compaction: record.payload.exact_contributor_compaction,
            dropped_prior: record.dropped_prior,
        }
    }
}

impl From<AtomicTerminal<SurfaceOrderMeasurement, SurfaceCompatibilityOrderIssueContext>>
    for SurfaceCompatibilityOrderGpuSuccess
{
    fn from(
        record: AtomicTerminal<SurfaceOrderMeasurement, SurfaceCompatibilityOrderIssueContext>,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            timing_source: record.payload.timing_source,
            gpu_preprocess_ms: record.payload.gpu_preprocess_ms,
            gpu_radix_ms: record.payload.gpu_radix_ms,
            gpu_order_ms: record.payload.gpu_order_ms,
            gpu_complete_ms: record.payload.gpu_complete_ms,
            timestamp_period_ns: record.payload.timestamp_period_ns,
            below_timestamp_resolution: record.payload.below_timestamp_resolution,
            visible_count: record.payload.visible_count,
            drawn_count: record.payload.drawn_count,
            exact_contributor_compaction: record.payload.exact_contributor_compaction,
            dropped_prior: record.dropped_prior,
        }
    }
}

impl From<AtomicTerminal<SurfaceOrderMeasurementFailure, SurfaceCompatibilityOrderIssueContext>>
    for SurfaceCompatibilityOrderFailure
{
    fn from(
        record: AtomicTerminal<
            SurfaceOrderMeasurementFailure,
            SurfaceCompatibilityOrderIssueContext,
        >,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            reason: record.payload.reason,
            dropped_prior: record.dropped_prior,
        }
    }
}

impl
    From<AtomicTerminal<SurfaceProjectedDrawMeasurement, SurfaceCompatibilityProjectedIssueContext>>
    for SurfaceCompatibilityProjectedSuccess
{
    fn from(
        record: AtomicTerminal<
            SurfaceProjectedDrawMeasurement,
            SurfaceCompatibilityProjectedIssueContext,
        >,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            execution: record.payload.execution,
            order_backend: record.payload.order_backend,
            projection_generation: record.payload.projection_generation,
            probe_generation: record.payload.probe_generation,
            projection_rebuilt: record.payload.projection_rebuilt,
            order_refreshed: record.payload.order_refreshed,
            frame_complete_ms: record.payload.frame_complete_ms,
            exact_contributor_compaction: record.payload.exact_contributor_compaction,
            dropped_prior: record.dropped_prior,
        }
    }
}

impl
    From<
        AtomicTerminal<
            SurfaceProjectedDrawMeasurementFailure,
            SurfaceCompatibilityProjectedIssueContext,
        >,
    > for SurfaceCompatibilityProjectedFailure
{
    fn from(
        record: AtomicTerminal<
            SurfaceProjectedDrawMeasurementFailure,
            SurfaceCompatibilityProjectedIssueContext,
        >,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            execution: record.payload.execution,
            order_backend: record.payload.order_backend,
            projection_generation: record.payload.projection_generation,
            probe_generation: record.payload.probe_generation,
            reason: record.payload.reason,
            dropped_prior: record.dropped_prior,
        }
    }
}

impl From<AtomicTerminal<SurfaceGpuProducerMeasurement, SurfaceCompatibilityProducerIssueContext>>
    for SurfaceCompatibilityProducerSuccess
{
    fn from(
        record: AtomicTerminal<
            SurfaceGpuProducerMeasurement,
            SurfaceCompatibilityProducerIssueContext,
        >,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            producer: record.payload.producer,
            order_generation: record.payload.order_generation,
            projection_generation: record.payload.projection_generation,
            source_count: record.payload.source_count,
            contributor_count: record.payload.contributor_count,
            drawn_count: record.payload.drawn_count,
            order_refreshed: record.payload.order_refreshed,
            draw_scope: record.payload.draw_scope,
            frame_complete_ms: record.payload.frame_complete_ms,
            dropped_prior: record.dropped_prior,
        }
    }
}

impl
    From<
        AtomicTerminal<
            SurfaceGpuProducerMeasurementFailure,
            SurfaceCompatibilityProducerIssueContext,
        >,
    > for SurfaceCompatibilityProducerFailure
{
    fn from(
        record: AtomicTerminal<
            SurfaceGpuProducerMeasurementFailure,
            SurfaceCompatibilityProducerIssueContext,
        >,
    ) -> Self {
        Self {
            issue: record.issue,
            ticket: record.payload.ticket,
            camera_revision: record.payload.camera_revision,
            producer: record.payload.producer,
            order_generation: record.payload.order_generation,
            projection_generation: record.payload.projection_generation,
            reason: record.payload.reason,
            dropped_prior: record.dropped_prior,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_session::{
        SurfaceGpuProducerMeasurementUnsampledReason,
        SurfaceProjectedDrawMeasurementUnsampledReason,
    };

    fn submissions(
        ticket: u64,
        backend: SurfaceOrderBackendUsed,
        execution: SurfaceProjectedDrawExecution,
        producer: SurfaceGpuOrderProducer,
    ) -> (
        SurfaceCompatibilityOrderSubmission,
        SurfaceCompatibilityProjectedSubmission,
        SurfaceCompatibilityProducerSubmission,
    ) {
        (
            SurfaceCompatibilityOrderSubmission {
                camera_revision: ticket + 100,
                requested_backend: SurfaceOrderBackend::Adaptive,
                actual_backend: backend,
                adaptive_state: SurfaceAdaptiveState::GpuProbe,
                measurement: SurfaceOrderMeasurementSubmission::Issued { backend, ticket },
            },
            SurfaceCompatibilityProjectedSubmission {
                camera_revision: ticket + 100,
                requested_policy: SurfaceProjectedDrawPolicy::Adaptive,
                actual_execution: execution,
                order_backend: backend,
                adaptive_state: SurfaceProjectedDrawAdaptiveState::CompactProbe,
                measurement: SurfaceProjectedDrawMeasurementSubmission::Issued {
                    execution,
                    ticket,
                },
            },
            SurfaceCompatibilityProducerSubmission {
                camera_revision: ticket + 100,
                requested_producer: producer,
                actual_producer: Some(producer),
                order_backend: backend,
                projected_execution: execution,
                measurement_enabled: true,
                measurement: SurfaceGpuProducerMeasurementSubmission::Issued { producer, ticket },
            },
        )
    }

    fn observe_ticket(
        store: &mut CompatibilityEvidenceStore,
        ticket: u64,
        backend: SurfaceOrderBackendUsed,
        execution: SurfaceProjectedDrawExecution,
        producer: SurfaceGpuOrderProducer,
    ) {
        let (order, projected, producer) = submissions(ticket, backend, execution, producer);
        store.observe_submissions(order, projected, producer);
    }

    fn cpu_measurement(ticket: u64) -> SurfaceCpuOrderMeasurement {
        SurfaceCpuOrderMeasurement {
            ticket,
            camera_revision: ticket + 100,
            preprocess_ms: 1.0,
            sort_ms: 2.0,
            frame_complete_ms: 3.0,
            visible_count: 90,
            contributor_count: 70,
            drawn_count: 70,
            exact_contributor_compaction: true,
        }
    }

    fn gpu_measurement(ticket: u64) -> SurfaceOrderMeasurement {
        SurfaceOrderMeasurement {
            ticket,
            camera_revision: ticket + 100,
            timing_source: SurfaceTimingSource::TimestampQuery,
            gpu_preprocess_ms: Some(1.0),
            gpu_radix_ms: Some(2.0),
            gpu_order_ms: Some(3.0),
            gpu_complete_ms: 4.0,
            timestamp_period_ns: Some(5.0),
            below_timestamp_resolution: false,
            visible_count: 91,
            contributor_count: 71,
            drawn_count: 91,
            exact_contributor_compaction: false,
        }
    }

    fn projected_measurement(ticket: u64) -> SurfaceProjectedDrawMeasurement {
        SurfaceProjectedDrawMeasurement {
            ticket,
            camera_revision: ticket + 100,
            execution: SurfaceProjectedDrawExecution::Compact,
            order_backend: SurfaceOrderBackendUsed::Gpu,
            projection_generation: ticket + 200,
            probe_generation: ticket + 300,
            projection_rebuilt: true,
            order_refreshed: false,
            frame_complete_ms: 6.0,
            visible_count: 92,
            contributor_count: 72,
            drawn_count: 72,
            exact_contributor_compaction: true,
        }
    }

    fn producer_measurement(ticket: u64) -> SurfaceGpuProducerMeasurement {
        SurfaceGpuProducerMeasurement {
            ticket,
            camera_revision: ticket + 100,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: ticket + 200,
            projection_generation: ticket + 300,
            source_count: 100,
            contributor_count: 72,
            drawn_count: 72,
            order_refreshed: true,
            draw_scope: SurfaceGpuProducerDrawScope::ExactCurrentContributors,
            frame_complete_ms: 7.0,
        }
    }

    fn nonzero(ticket: u64) -> NonZeroU64 {
        NonZeroU64::new(ticket).expect("test ticket is non-zero")
    }

    #[test]
    fn gpu_success_terminal_keeps_ffi_counts_without_consuming_take_once_counts() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            2,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceGpuOrderProducer::PostSort,
        );
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(2)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Order,
                ticket: nonzero(2),
                reason: SurfaceCompatibilityCountsUnavailableReason::Pending,
            })
        );
        store.publish_gpu_order(gpu_measurement(2));
        let SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::OrderGpuSuccess(
            success,
        )) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderGpuSuccess)
        else {
            panic!("GPU success must be ready");
        };
        assert_eq!(success.issue.actual_backend, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(
            success.issue.requested_backend,
            SurfaceOrderBackend::Adaptive
        );
        assert_eq!((success.visible_count, success.drawn_count), (91, 91));
        let SurfaceCompatibilityCountsTake::Ready(counts) =
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(2))
        else {
            panic!("GPU counts must be ready");
        };
        assert_eq!(
            (
                counts.visible_count,
                counts.contributor_count,
                counts.drawn_count
            ),
            (91, 71, 91)
        );
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(2)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Order,
                ticket: nonzero(2),
                reason: SurfaceCompatibilityCountsUnavailableReason::Consumed,
            })
        );
    }

    #[test]
    fn cpu_success_terminal_keeps_contributor_after_take_once_counts_are_consumed() {
        let mut store = CompatibilityEvidenceStore::new();
        // An odd ticket is deliberately used for CPU: backend identity comes
        // from publication context, never ticket parity.
        observe_ticket(
            &mut store,
            3,
            SurfaceOrderBackendUsed::Cpu,
            SurfaceProjectedDrawExecution::Compact,
            SurfaceGpuOrderProducer::PostSort,
        );
        store.publish_cpu_order(cpu_measurement(3));
        let SurfaceCompatibilityCountsTake::Ready(counts) =
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(3))
        else {
            panic!("CPU counts must be ready");
        };
        assert_eq!(
            (
                counts.visible_count,
                counts.contributor_count,
                counts.drawn_count
            ),
            (90, 70, 70)
        );
        let SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::OrderCpuSuccess(
            success,
        )) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess)
        else {
            panic!("CPU success must be ready");
        };
        assert_eq!(success.issue.actual_backend, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(success.contributor_count, 70);
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(3)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Order,
                ticket: nonzero(3),
                reason: SurfaceCompatibilityCountsUnavailableReason::Consumed,
            })
        );
    }

    #[test]
    fn failure_is_terminal_without_usable_counts() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            7,
            SurfaceOrderBackendUsed::Cpu,
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceGpuOrderProducer::PostSort,
        );
        store.publish_order_failure(SurfaceOrderMeasurementFailure {
            ticket: 7,
            camera_revision: 107,
            reason: SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
        });
        let SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::OrderFailure(
            failure,
        )) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderFailure)
        else {
            panic!("failure must be ready");
        };
        assert_eq!(failure.issue.actual_backend, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(7)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Order,
                ticket: nonzero(7),
                reason: SurfaceCompatibilityCountsUnavailableReason::Failed,
            })
        );
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(999)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Order,
                ticket: nonzero(999),
                reason: SurfaceCompatibilityCountsUnavailableReason::InvalidTicket,
            })
        );
    }

    #[test]
    fn projected_and_producer_failures_preserve_identity_without_counts() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            13,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Compact,
            SurfaceGpuOrderProducer::Preproject,
        );
        store.publish_projected_failure(SurfaceProjectedDrawMeasurementFailure {
            ticket: 13,
            camera_revision: 113,
            execution: SurfaceProjectedDrawExecution::Compact,
            order_backend: SurfaceOrderBackendUsed::Gpu,
            projection_generation: 213,
            probe_generation: 313,
            reason: SurfaceProjectedDrawMeasurementFailureReason::InvariantViolation,
        });
        let SurfaceCompatibilityTerminalPoll::Ready(
            SurfaceCompatibilityTerminal::ProjectedFailure(projected),
        ) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProjectedFailure)
        else {
            panic!("projected failure must be ready");
        };
        assert_eq!(projected.projection_generation, 213);
        assert_eq!(projected.probe_generation, 313);
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Projected, nonzero(13)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Projected,
                ticket: nonzero(13),
                reason: SurfaceCompatibilityCountsUnavailableReason::Failed,
            })
        );

        // The three ticket namespaces are independent, so publishing the same
        // numeric producer ticket retains its separately recorded context.
        store.observe_submissions(
            SurfaceCompatibilityOrderSubmission {
                measurement: SurfaceOrderMeasurementSubmission::NotRequested,
                ..submissions(
                    13,
                    SurfaceOrderBackendUsed::Gpu,
                    SurfaceProjectedDrawExecution::Compact,
                    SurfaceGpuOrderProducer::Preproject,
                )
                .0
            },
            SurfaceCompatibilityProjectedSubmission {
                measurement: SurfaceProjectedDrawMeasurementSubmission::NotRequested,
                ..submissions(
                    13,
                    SurfaceOrderBackendUsed::Gpu,
                    SurfaceProjectedDrawExecution::Compact,
                    SurfaceGpuOrderProducer::Preproject,
                )
                .1
            },
            submissions(
                13,
                SurfaceOrderBackendUsed::Gpu,
                SurfaceProjectedDrawExecution::Compact,
                SurfaceGpuOrderProducer::Preproject,
            )
            .2,
        );
        store.publish_producer_failure(SurfaceGpuProducerMeasurementFailure {
            ticket: 13,
            camera_revision: 113,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: 413,
            projection_generation: 513,
            reason: SurfaceGpuProducerMeasurementFailureReason::ReadbackMap,
        });
        let SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::ProducerFailure(
            producer,
        )) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProducerFailure)
        else {
            panic!("producer failure must be ready");
        };
        assert_eq!(producer.order_generation, 413);
        assert_eq!(producer.projection_generation, 513);
        assert_eq!(
            producer.issue.actual_producer,
            Some(SurfaceGpuOrderProducer::Preproject)
        );
    }

    #[test]
    fn duplicate_terminal_callback_is_ignored_after_first_publication() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            17,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceGpuOrderProducer::PostSort,
        );
        store.publish_gpu_order(gpu_measurement(17));
        store.publish_gpu_order(gpu_measurement(17));
        assert!(matches!(
            store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderGpuSuccess),
            SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::OrderGpuSuccess(
                _
            ))
        ));
        assert!(matches!(
            store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderGpuSuccess),
            SurfaceCompatibilityTerminalPoll::Unavailable(
                SurfaceCompatibilityTerminalUnavailable::Empty
            )
        ));
    }

    #[test]
    fn republished_ticket_invalidates_prior_counts_instead_of_returning_stale_data() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            19,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceGpuOrderProducer::PostSort,
        );
        store.publish_gpu_order(gpu_measurement(19));
        observe_ticket(
            &mut store,
            19,
            SurfaceOrderBackendUsed::Cpu,
            SurfaceProjectedDrawExecution::Compact,
            SurfaceGpuOrderProducer::PostSort,
        );
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(19)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Order,
                ticket: nonzero(19),
                reason: SurfaceCompatibilityCountsUnavailableReason::Pending,
            })
        );
    }

    #[test]
    fn projected_sixty_fifth_success_marks_drop_and_expires_oldest_counts() {
        let mut store = CompatibilityEvidenceStore::new();
        for ticket in 1..=65 {
            observe_ticket(
                &mut store,
                ticket,
                SurfaceOrderBackendUsed::Gpu,
                SurfaceProjectedDrawExecution::Compact,
                SurfaceGpuOrderProducer::Preproject,
            );
            store.publish_projected_success(projected_measurement(ticket));
        }
        let SurfaceCompatibilityTerminalPoll::Ready(
            SurfaceCompatibilityTerminal::ProjectedSuccess(first),
        ) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProjectedSuccess)
        else {
            panic!("projected success must be ready");
        };
        assert_eq!(first.ticket, 2);
        let mut last = first;
        for _ in 1..64 {
            let SurfaceCompatibilityTerminalPoll::Ready(
                SurfaceCompatibilityTerminal::ProjectedSuccess(success),
            ) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProjectedSuccess)
            else {
                panic!("all retained projected successes must be ready");
            };
            last = success;
        }
        assert_eq!(last.ticket, 65);
        assert!(last.dropped_prior);
        assert_eq!(last.projection_generation, 265);
        assert_eq!(last.probe_generation, 365);
        assert_eq!(last.execution, SurfaceProjectedDrawExecution::Compact);
        assert_eq!(
            store.take_counts(SurfaceCompatibilityCountFamily::Projected, nonzero(1)),
            SurfaceCompatibilityCountsTake::Unavailable(SurfaceCompatibilityCountsUnavailable {
                family: SurfaceCompatibilityCountFamily::Projected,
                ticket: nonzero(1),
                reason: SurfaceCompatibilityCountsUnavailableReason::Expired,
            })
        );
        let SurfaceCompatibilityCountsTake::Ready(counts) =
            store.take_counts(SurfaceCompatibilityCountFamily::Projected, nonzero(65))
        else {
            panic!("newest projected counts must be ready");
        };
        assert_eq!(
            (
                counts.visible_count,
                counts.contributor_count,
                counts.drawn_count
            ),
            (92, 72, 72)
        );
        assert!(counts.exact_contributor_compaction);
    }

    #[test]
    fn order_count_retention_covers_sixty_four_cpu_plus_sixty_four_gpu() {
        let mut store = CompatibilityEvidenceStore::new();
        for ticket in 1..=64 {
            observe_ticket(
                &mut store,
                ticket,
                SurfaceOrderBackendUsed::Cpu,
                SurfaceProjectedDrawExecution::Compact,
                SurfaceGpuOrderProducer::PostSort,
            );
            store.publish_cpu_order(cpu_measurement(ticket));
        }
        for ticket in 65..=128 {
            observe_ticket(
                &mut store,
                ticket,
                SurfaceOrderBackendUsed::Gpu,
                SurfaceProjectedDrawExecution::Candidate,
                SurfaceGpuOrderProducer::PostSort,
            );
            store.publish_gpu_order(gpu_measurement(ticket));
        }
        assert!(matches!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(1)),
            SurfaceCompatibilityCountsTake::Ready(_)
        ));
        assert!(matches!(
            store.take_counts(SurfaceCompatibilityCountFamily::Order, nonzero(128)),
            SurfaceCompatibilityCountsTake::Ready(_)
        ));
    }

    #[test]
    fn producer_terminal_preserves_generation_scd_and_draw_scope() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            9,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceProjectedDrawExecution::Compact,
            SurfaceGpuOrderProducer::Preproject,
        );
        store.publish_producer_success(producer_measurement(9));
        let SurfaceCompatibilityTerminalPoll::Ready(SurfaceCompatibilityTerminal::ProducerSuccess(
            success,
        )) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProducerSuccess)
        else {
            panic!("producer success must be ready");
        };
        assert_eq!(success.order_generation, 209);
        assert_eq!(success.projection_generation, 309);
        assert_eq!(
            (
                success.source_count,
                success.contributor_count,
                success.drawn_count
            ),
            (100, 72, 72)
        );
        assert_eq!(
            success.draw_scope,
            SurfaceGpuProducerDrawScope::ExactCurrentContributors
        );
        assert!(store.drain_producer_successes().is_empty());
    }

    #[test]
    fn producer_raw_drain_remains_fifo_and_lossless_past_compatibility_capacity() {
        let mut store = CompatibilityEvidenceStore::new();
        for ticket in 1..=65 {
            observe_ticket(
                &mut store,
                ticket,
                SurfaceOrderBackendUsed::Gpu,
                SurfaceProjectedDrawExecution::Compact,
                SurfaceGpuOrderProducer::Preproject,
            );
            store.publish_producer_success(producer_measurement(ticket));
        }
        assert_eq!(
            store
                .drain_producer_successes()
                .into_iter()
                .map(|measurement| measurement.ticket)
                .collect::<Vec<_>>(),
            (1..=65).collect::<Vec<_>>()
        );
        assert!(matches!(
            store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProducerSuccess),
            SurfaceCompatibilityTerminalPoll::Unavailable(
                SurfaceCompatibilityTerminalUnavailable::Empty
            )
        ));
    }

    #[test]
    fn producer_compatibility_window_is_bounded_without_losing_raw_overflow() {
        let mut store = CompatibilityEvidenceStore::new();
        for ticket in 1..=65 {
            observe_ticket(
                &mut store,
                ticket,
                SurfaceOrderBackendUsed::Gpu,
                SurfaceProjectedDrawExecution::Compact,
                SurfaceGpuOrderProducer::Preproject,
            );
            store.publish_producer_success(producer_measurement(ticket));
        }
        let mut successes = Vec::new();
        while let SurfaceCompatibilityTerminalPoll::Ready(
            SurfaceCompatibilityTerminal::ProducerSuccess(success),
        ) = store.poll_terminal(SurfaceCompatibilityTerminalSelector::ProducerSuccess)
        {
            successes.push(success);
        }
        assert_eq!(successes.len(), 64);
        assert_eq!(successes.first().map(|success| success.ticket), Some(2));
        assert_eq!(successes.last().map(|success| success.ticket), Some(65));
        assert!(
            successes
                .last()
                .is_some_and(|success| success.dropped_prior)
        );
        assert_eq!(
            store
                .drain_producer_successes()
                .into_iter()
                .map(|measurement| measurement.ticket)
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn legacy_and_compatibility_success_views_never_double_deliver() {
        let mut store = CompatibilityEvidenceStore::new();
        observe_ticket(
            &mut store,
            11,
            SurfaceOrderBackendUsed::Cpu,
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceGpuOrderProducer::PostSort,
        );
        store.publish_cpu_order(cpu_measurement(11));
        assert_eq!(store.drain_cpu_order().len(), 1);
        assert!(matches!(
            store.poll_terminal(SurfaceCompatibilityTerminalSelector::OrderCpuSuccess),
            SurfaceCompatibilityTerminalPoll::Unavailable(
                SurfaceCompatibilityTerminalUnavailable::Empty
            )
        ));
    }

    #[test]
    fn latest_submissions_preserve_not_requested_and_unsampled_identity() {
        let mut store = CompatibilityEvidenceStore::new();
        let order = SurfaceCompatibilityOrderSubmission {
            camera_revision: 4,
            requested_backend: SurfaceOrderBackend::Cpu,
            actual_backend: SurfaceOrderBackendUsed::Cpu,
            adaptive_state: SurfaceAdaptiveState::Disabled,
            measurement: SurfaceOrderMeasurementSubmission::NotRequested,
        };
        let projected = SurfaceCompatibilityProjectedSubmission {
            camera_revision: 4,
            requested_policy: SurfaceProjectedDrawPolicy::Candidate,
            actual_execution: SurfaceProjectedDrawExecution::Candidate,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
            measurement: SurfaceProjectedDrawMeasurementSubmission::Unsampled {
                execution: SurfaceProjectedDrawExecution::Candidate,
                reason: SurfaceProjectedDrawMeasurementUnsampledReason::SurfaceUnavailable,
            },
        };
        let producer = SurfaceCompatibilityProducerSubmission {
            camera_revision: 4,
            requested_producer: SurfaceGpuOrderProducer::PostSort,
            actual_producer: None,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            projected_execution: SurfaceProjectedDrawExecution::Candidate,
            measurement_enabled: false,
            measurement: SurfaceGpuProducerMeasurementSubmission::Unsampled {
                producer: SurfaceGpuOrderProducer::PostSort,
                reason: SurfaceGpuProducerMeasurementUnsampledReason::RingBusy,
            },
        };
        store.observe_submissions(order, projected, producer);
        assert_eq!(
            store.submission(SurfaceCompatibilityChannel::Order),
            Some(SurfaceCompatibilitySubmission::Order(order))
        );
        assert_eq!(
            store.submission(SurfaceCompatibilityChannel::Projected),
            Some(SurfaceCompatibilitySubmission::Projected(projected))
        );
        assert_eq!(
            store.submission(SurfaceCompatibilityChannel::Producer),
            Some(SurfaceCompatibilitySubmission::Producer(producer))
        );
    }
}
