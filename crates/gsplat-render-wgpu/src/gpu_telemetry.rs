//! Non-blocking GPU ordering measurements tied to the command buffer that
//! produced them.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

pub use crate::evidence::{
    SurfaceCpuOrderMeasurement, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfaceOrderMeasurementFailureReason, SurfaceTimingSource,
};
use crate::{TimerInstant, timer_elapsed_ms, wgpu_label};

const RING_SLOTS: usize = 8;
const QUERY_COUNT: u32 = 4;
const QUERY_BYTES: u64 = QUERY_COUNT as u64 * std::mem::size_of::<u64>() as u64;
const CANDIDATE_VISIBLE_OFFSET: u64 = QUERY_BYTES;
const CONTRIBUTOR_OFFSET: u64 = CANDIDATE_VISIBLE_OFFSET + std::mem::size_of::<u32>() as u64;
const DRAWN_OFFSET: u64 = CONTRIBUTOR_OFFSET + std::mem::size_of::<u32>() as u64;
const READBACK_BYTES: u64 = 48;
const SLOT_IDLE: u8 = 0;
const SLOT_ENCODING: u8 = 1;
const SLOT_SUBMITTED: u8 = 2;
const SLOT_MAPPED: u8 = 3;
const SLOT_ERROR: u8 = 4;
const COMPLETION_PENDING_BITS: u32 = u32::MAX;
const MAX_VALID_GPU_ORDER_MS: f32 = 60_000.0;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
const FIRST_GPU_TICKET: u64 = 1;
const FIRST_CPU_TICKET: u64 = 2;
// Keep order receipts in the low namespace. Producer A/B receipts own
// [2^51, 2^52), and projected-draw receipts own [2^52, 2^53).
const LAST_ORDER_TICKET: u64 = (1_u64 << 51) - 1;
const _: () = assert!(LAST_ORDER_TICKET < (1_u64 << 51));
const _: () = assert!(LAST_ORDER_TICKET <= MAX_JAVASCRIPT_SAFE_INTEGER);

fn next_namespaced_ticket(current: u64, first: u64) -> u64 {
    current
        .checked_add(2)
        .filter(|next| *next <= LAST_ORDER_TICKET)
        .unwrap_or(first)
}

pub(crate) struct GpuOrderTelemetryPoll {
    pub(crate) completed: Vec<SurfaceOrderMeasurement>,
    pub(crate) failures: Vec<SurfaceOrderMeasurementFailure>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrameInstanceCounts {
    pub(crate) candidate_visible: u32,
    pub(crate) contributor: u32,
    pub(crate) drawn: u32,
    pub(crate) exact_contributor_compaction: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct InstanceCountSource<'a> {
    buffer: &'a wgpu::Buffer,
    offset: u64,
}

impl<'a> InstanceCountSource<'a> {
    pub(crate) const fn indirect_args(buffer: &'a wgpu::Buffer) -> Self {
        Self {
            buffer,
            offset: std::mem::size_of::<u32>() as u64,
        }
    }
}

struct TelemetrySlot {
    query_set: Option<wgpu::QuerySet>,
    query_resolve: Option<wgpu::Buffer>,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    completion_ms_bits: Arc<AtomicU32>,
    ticket: u64,
    generation: u64,
    camera_revision: u64,
    has_timestamps: bool,
    exact_contributor_compaction: bool,
    terminal_reported: bool,
}

pub(crate) struct GpuTelemetryTicket {
    slot: usize,
    pub(crate) ticket: u64,
    pub(crate) query_set: Option<wgpu::QuerySet>,
}

pub(crate) struct GpuOrderTelemetry {
    slots: Vec<TelemetrySlot>,
    next_slot: usize,
    next_ticket: u64,
    generation: u64,
    timestamp_period_ns: Option<f32>,
    pending_failures: VecDeque<SurfaceOrderMeasurementFailure>,
}

struct CpuCompletionSlot {
    state: Arc<AtomicU8>,
    completion_ms_bits: Arc<AtomicU32>,
    ticket: u64,
    generation: u64,
    camera_revision: u64,
    preprocess_ms: f32,
    sort_ms: f32,
    counts: FrameInstanceCounts,
    terminal_reported: bool,
}

pub(crate) struct CpuOrderTelemetryPoll {
    pub(crate) completed: Vec<SurfaceCpuOrderMeasurement>,
    pub(crate) failures: Vec<SurfaceOrderMeasurementFailure>,
}

/// Presenter-level result of an optional telemetry reservation. The Surface
/// session attaches the CPU/GPU backend before exposing it publicly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TelemetrySubmission {
    NotRequested,
    Issued(u64),
    RingBusy,
    SurfaceUnavailable,
    /// GPU ordering is being prepared without publishing a drawable. This is
    /// used by browser-only lazy pipeline warmup. No measurement identity may
    /// be exposed until a later call submits the corresponding presented frame.
    GpuOrderPreparationPending,
}

pub(crate) struct CpuCompletionTicket {
    slot: usize,
    pub(crate) ticket: u64,
}

/// Small callback-only ring for CPU refresh completion. No readback buffer is
/// needed because the CPU phase values are already known synchronously.
pub(crate) struct CpuOrderCompletionTelemetry {
    slots: Vec<CpuCompletionSlot>,
    next_slot: usize,
    next_ticket: u64,
    generation: u64,
    pending_failures: VecDeque<SurfaceOrderMeasurementFailure>,
}

impl Default for CpuOrderCompletionTelemetry {
    fn default() -> Self {
        let slots = (0..RING_SLOTS)
            .map(|_| CpuCompletionSlot {
                state: Arc::new(AtomicU8::new(SLOT_IDLE)),
                completion_ms_bits: Arc::new(AtomicU32::new(COMPLETION_PENDING_BITS)),
                ticket: 0,
                generation: 0,
                camera_revision: 0,
                preprocess_ms: 0.0,
                sort_ms: 0.0,
                counts: FrameInstanceCounts::default(),
                terminal_reported: false,
            })
            .collect();
        Self {
            slots,
            next_slot: 0,
            next_ticket: FIRST_CPU_TICKET,
            generation: 1,
            pending_failures: VecDeque::with_capacity(RING_SLOTS),
        }
    }
}

impl CpuOrderCompletionTelemetry {
    pub(crate) fn invalidate_generation(&mut self) {
        for slot in &mut self.slots {
            let state = slot.state.load(Ordering::Acquire);
            if state != SLOT_IDLE && !slot.terminal_reported {
                self.pending_failures
                    .push_back(SurfaceOrderMeasurementFailure {
                        ticket: slot.ticket,
                        camera_revision: slot.camera_revision,
                        reason: SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
                    });
                slot.terminal_reported = true;
            }
            // Encoding means no command-buffer callback owns these atomics
            // yet. Recycle it immediately; submitted slots must instead wait
            // for their callback before reuse.
            if state == SLOT_ENCODING {
                slot.state.store(SLOT_IDLE, Ordering::Release);
            }
        }
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    #[cfg(test)]
    pub(crate) fn begin_sample(
        &mut self,
        camera_revision: u64,
        preprocess_ms: f32,
        sort_ms: f32,
    ) -> Option<CpuCompletionTicket> {
        self.begin_sample_with_counts(
            camera_revision,
            preprocess_ms,
            sort_ms,
            FrameInstanceCounts::default(),
        )
    }

    #[cfg(test)]
    pub(crate) fn begin_submitted_sample_for_test(
        &mut self,
        camera_revision: u64,
        preprocess_ms: f32,
        sort_ms: f32,
        counts: FrameInstanceCounts,
    ) -> Option<u64> {
        let ticket =
            self.begin_sample_with_counts(camera_revision, preprocess_ms, sort_ms, counts)?;
        self.slots[ticket.slot]
            .state
            .store(SLOT_SUBMITTED, Ordering::Release);
        Some(ticket.ticket)
    }

    #[cfg(test)]
    pub(crate) fn complete_submitted_sample_for_test(
        &mut self,
        ticket: u64,
        frame_complete_ms: f32,
    ) -> bool {
        let Some(slot) = self.slots.iter_mut().find(|slot| {
            slot.ticket == ticket && slot.state.load(Ordering::Acquire) == SLOT_SUBMITTED
        }) else {
            return false;
        };
        slot.completion_ms_bits
            .store(frame_complete_ms.to_bits(), Ordering::Release);
        slot.state.store(SLOT_MAPPED, Ordering::Release);
        true
    }

    pub(crate) fn begin_sample_with_counts(
        &mut self,
        camera_revision: u64,
        preprocess_ms: f32,
        sort_ms: f32,
        counts: FrameInstanceCounts,
    ) -> Option<CpuCompletionTicket> {
        let slot_index = (0..self.slots.len())
            .map(|offset| (self.next_slot + offset) % self.slots.len())
            .find(|&index| self.slots[index].state.load(Ordering::Acquire) == SLOT_IDLE)?;
        self.next_slot = (slot_index + 1) % self.slots.len();
        let ticket = self.next_ticket;
        self.next_ticket = next_namespaced_ticket(self.next_ticket, FIRST_CPU_TICKET);

        let slot = &mut self.slots[slot_index];
        slot.ticket = ticket;
        slot.generation = self.generation;
        slot.camera_revision = camera_revision;
        slot.preprocess_ms = preprocess_ms;
        slot.sort_ms = sort_ms;
        slot.counts = counts;
        slot.terminal_reported = false;
        slot.completion_ms_bits
            .store(COMPLETION_PENDING_BITS, Ordering::Release);
        slot.state.store(SLOT_ENCODING, Ordering::Release);
        Some(CpuCompletionTicket {
            slot: slot_index,
            ticket,
        })
    }

    pub(crate) fn arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        ticket: CpuCompletionTicket,
        completion_started: TimerInstant,
    ) {
        let slot = &mut self.slots[ticket.slot];
        debug_assert_eq!(slot.ticket, ticket.ticket);
        slot.state.store(SLOT_SUBMITTED, Ordering::Release);
        let completion_bits = Arc::clone(&slot.completion_ms_bits);
        let state = Arc::clone(&slot.state);
        command_buffer.on_submitted_work_done(move || {
            completion_bits.store(
                timer_elapsed_ms(completion_started).to_bits(),
                Ordering::Release,
            );
            state.store(SLOT_MAPPED, Ordering::Release);
        });
    }

    pub(crate) fn poll(&mut self) -> CpuOrderTelemetryPoll {
        let mut completed = Vec::new();
        let mut failures: Vec<_> = self.pending_failures.drain(..).collect();
        for slot in &mut self.slots {
            if slot.state.load(Ordering::Acquire) == SLOT_MAPPED {
                let completion_bits = slot.completion_ms_bits.load(Ordering::Acquire);
                if completion_bits == COMPLETION_PENDING_BITS {
                    continue;
                }
                if slot.generation == self.generation && !slot.terminal_reported {
                    completed.push(SurfaceCpuOrderMeasurement {
                        ticket: slot.ticket,
                        camera_revision: slot.camera_revision,
                        preprocess_ms: slot.preprocess_ms,
                        sort_ms: slot.sort_ms,
                        frame_complete_ms: f32::from_bits(completion_bits),
                        visible_count: slot.counts.candidate_visible,
                        contributor_count: slot.counts.contributor,
                        drawn_count: slot.counts.drawn,
                        exact_contributor_compaction: slot.counts.exact_contributor_compaction,
                    });
                    slot.terminal_reported = true;
                }
                slot.state.store(SLOT_IDLE, Ordering::Release);
            }
        }
        completed.sort_by_key(|sample| sample.ticket);
        failures.sort_by_key(|failure| failure.ticket);
        CpuOrderTelemetryPoll {
            completed,
            failures,
        }
    }
}

impl GpuOrderTelemetry {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        timestamps_enabled: bool,
    ) -> Self {
        let slots = (0..RING_SLOTS)
            .map(|_| {
                let query_set = timestamps_enabled.then(|| {
                    device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: wgpu_label("gsplat-gpu-order-telemetry-query-set"),
                        ty: wgpu::QueryType::Timestamp,
                        count: QUERY_COUNT,
                    })
                });
                let query_resolve = timestamps_enabled.then(|| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: wgpu_label("gsplat-gpu-order-telemetry-query-resolve"),
                        size: QUERY_BYTES,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    })
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: wgpu_label("gsplat-gpu-order-telemetry-readback"),
                    size: READBACK_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                TelemetrySlot {
                    query_set,
                    query_resolve,
                    readback,
                    state: Arc::new(AtomicU8::new(SLOT_IDLE)),
                    completion_ms_bits: Arc::new(AtomicU32::new(COMPLETION_PENDING_BITS)),
                    ticket: 0,
                    generation: 0,
                    camera_revision: 0,
                    has_timestamps: false,
                    exact_contributor_compaction: false,
                    terminal_reported: false,
                }
            })
            .collect();
        Self {
            slots,
            next_slot: 0,
            next_ticket: FIRST_GPU_TICKET,
            generation: 1,
            timestamp_period_ns: timestamps_enabled.then(|| queue.get_timestamp_period()),
            pending_failures: VecDeque::with_capacity(RING_SLOTS),
        }
    }

    pub(crate) fn timestamps_enabled(&self) -> bool {
        self.timestamp_period_ns.is_some()
    }

    pub(crate) fn invalidate_generation(&mut self) {
        for slot in &mut self.slots {
            let state = slot.state.load(Ordering::Acquire);
            if state != SLOT_IDLE && !slot.terminal_reported {
                self.pending_failures
                    .push_back(SurfaceOrderMeasurementFailure {
                        ticket: slot.ticket,
                        camera_revision: slot.camera_revision,
                        reason: SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
                    });
                slot.terminal_reported = true;
            }
            // An encoding reservation has no late map/completion callback and
            // can be reused immediately after its terminal invalidation.
            if state == SLOT_ENCODING {
                slot.state.store(SLOT_IDLE, Ordering::Release);
            }
        }
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    pub(crate) fn begin_sample(
        &mut self,
        camera_revision: u64,
        allow_timestamps: bool,
    ) -> Option<GpuTelemetryTicket> {
        let slot_index = (0..self.slots.len())
            .map(|offset| (self.next_slot + offset) % self.slots.len())
            .find(|&index| self.slots[index].state.load(Ordering::Acquire) == SLOT_IDLE)?;
        self.next_slot = (slot_index + 1) % self.slots.len();
        let ticket = self.next_ticket;
        self.next_ticket = next_namespaced_ticket(self.next_ticket, FIRST_GPU_TICKET);

        let slot = &mut self.slots[slot_index];
        slot.ticket = ticket;
        slot.generation = self.generation;
        slot.camera_revision = camera_revision;
        slot.has_timestamps = allow_timestamps && slot.query_set.is_some();
        slot.exact_contributor_compaction = false;
        slot.terminal_reported = false;
        slot.completion_ms_bits
            .store(COMPLETION_PENDING_BITS, Ordering::Release);
        slot.state.store(SLOT_ENCODING, Ordering::Release);

        Some(GpuTelemetryTicket {
            slot: slot_index,
            ticket,
            query_set: slot.has_timestamps.then(|| {
                slot.query_set
                    .as_ref()
                    .expect("timestamp slot has a query set")
                    .clone()
            }),
        })
    }

    pub(crate) fn encode_readback(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        ticket: &GpuTelemetryTicket,
        indirect_args: &wgpu::Buffer,
    ) {
        let count = InstanceCountSource::indirect_args(indirect_args);
        self.encode_instance_count_readback(encoder, ticket, count, count, count, false);
    }

    pub(crate) fn encode_instance_count_readback(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        ticket: &GpuTelemetryTicket,
        candidate_visible: InstanceCountSource<'_>,
        contributor: InstanceCountSource<'_>,
        drawn: InstanceCountSource<'_>,
        exact_contributor_compaction: bool,
    ) {
        let slot = &mut self.slots[ticket.slot];
        slot.exact_contributor_compaction = exact_contributor_compaction;
        if slot.has_timestamps {
            let query_set = slot
                .query_set
                .as_ref()
                .expect("timestamp telemetry slot has query set");
            let query_resolve = slot
                .query_resolve
                .as_ref()
                .expect("timestamp telemetry slot has resolve buffer");
            encoder.resolve_query_set(query_set, 0..QUERY_COUNT, query_resolve, 0);
            encoder.copy_buffer_to_buffer(query_resolve, 0, &slot.readback, 0, QUERY_BYTES);
        } else {
            encoder.clear_buffer(&slot.readback, 0, Some(QUERY_BYTES));
        }
        copy_count(
            encoder,
            candidate_visible,
            &slot.readback,
            CANDIDATE_VISIBLE_OFFSET,
        );
        copy_count(encoder, contributor, &slot.readback, CONTRIBUTOR_OFFSET);
        copy_count(encoder, drawn, &slot.readback, DRAWN_OFFSET);
        encoder.clear_buffer(
            &slot.readback,
            DRAWN_OFFSET + std::mem::size_of::<u32>() as u64,
            Some(std::mem::size_of::<u32>() as u64),
        );
    }

    pub(crate) fn arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        ticket: GpuTelemetryTicket,
        completion_started: TimerInstant,
    ) {
        let slot = &mut self.slots[ticket.slot];
        debug_assert_eq!(slot.ticket, ticket.ticket);
        slot.state.store(SLOT_SUBMITTED, Ordering::Release);

        let map_state = Arc::clone(&slot.state);
        command_buffer.map_buffer_on_submit(
            &slot.readback,
            wgpu::MapMode::Read,
            0..READBACK_BYTES,
            move |result| {
                map_state.store(
                    if result.is_ok() {
                        SLOT_MAPPED
                    } else {
                        SLOT_ERROR
                    },
                    Ordering::Release,
                );
            },
        );
        let completion_bits = Arc::clone(&slot.completion_ms_bits);
        command_buffer.on_submitted_work_done(move || {
            completion_bits.store(
                timer_elapsed_ms(completion_started).to_bits(),
                Ordering::Release,
            );
        });
    }

    pub(crate) fn poll(&mut self, device: &wgpu::Device) -> GpuOrderTelemetryPoll {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut completed = Vec::new();
        let mut failures: Vec<_> = self.pending_failures.drain(..).collect();
        for slot in &mut self.slots {
            match slot.state.load(Ordering::Acquire) {
                SLOT_ERROR => {
                    if !slot.terminal_reported {
                        failures.push(SurfaceOrderMeasurementFailure {
                            ticket: slot.ticket,
                            camera_revision: slot.camera_revision,
                            reason: SurfaceOrderMeasurementFailureReason::ReadbackMap,
                        });
                        slot.terminal_reported = true;
                    }
                    // Do not recycle the slot until both callbacks belonging
                    // to this command buffer have run. Otherwise a late
                    // completion callback could overwrite a newer sample that
                    // reused the same atomics after a mapping error.
                    if slot.completion_ms_bits.load(Ordering::Acquire) != COMPLETION_PENDING_BITS {
                        slot.state.store(SLOT_IDLE, Ordering::Release);
                    }
                }
                SLOT_MAPPED => {
                    let completion_bits = slot.completion_ms_bits.load(Ordering::Acquire);
                    if completion_bits == COMPLETION_PENDING_BITS {
                        continue;
                    }
                    let bytes = slot.readback.slice(0..READBACK_BYTES).get_mapped_range();
                    let timestamps = [
                        read_u64(&bytes[0..8]),
                        read_u64(&bytes[8..16]),
                        read_u64(&bytes[16..24]),
                        read_u64(&bytes[24..32]),
                    ];
                    let visible_count = read_u32(
                        &bytes[CANDIDATE_VISIBLE_OFFSET as usize..CONTRIBUTOR_OFFSET as usize],
                    );
                    let contributor_count =
                        read_u32(&bytes[CONTRIBUTOR_OFFSET as usize..DRAWN_OFFSET as usize]);
                    let drawn_count = read_u32(
                        &bytes[DRAWN_OFFSET as usize
                            ..DRAWN_OFFSET as usize + std::mem::size_of::<u32>()],
                    );
                    drop(bytes);
                    slot.readback.unmap();

                    if slot.generation == self.generation && !slot.terminal_reported {
                        let (gpu_preprocess_ms, gpu_radix_ms, gpu_order_ms) = if slot.has_timestamps
                        {
                            let period = self.timestamp_period_ns.unwrap_or(0.0);
                            (
                                timestamp_delta_ms(timestamps[0], timestamps[1], period),
                                timestamp_delta_ms(timestamps[2], timestamps[3], period),
                                timestamp_delta_ms(timestamps[0], timestamps[3], period),
                            )
                        } else {
                            (None, None, None)
                        };
                        let below_timestamp_resolution = slot.has_timestamps
                            && matches!(gpu_order_ms, Some(value) if value == 0.0);
                        completed.push(SurfaceOrderMeasurement {
                            ticket: slot.ticket,
                            camera_revision: slot.camera_revision,
                            timing_source: if slot.has_timestamps {
                                SurfaceTimingSource::TimestampQuery
                            } else {
                                SurfaceTimingSource::CompletionOnly
                            },
                            gpu_preprocess_ms,
                            gpu_radix_ms,
                            gpu_order_ms,
                            gpu_complete_ms: f32::from_bits(completion_bits),
                            timestamp_period_ns: slot
                                .has_timestamps
                                .then_some(self.timestamp_period_ns.unwrap_or(0.0)),
                            below_timestamp_resolution,
                            visible_count,
                            contributor_count,
                            drawn_count,
                            exact_contributor_compaction: slot.exact_contributor_compaction,
                        });
                        slot.terminal_reported = true;
                    }
                    slot.state.store(SLOT_IDLE, Ordering::Release);
                }
                _ => {}
            }
        }
        completed.sort_by_key(|sample| sample.ticket);
        failures.sort_by_key(|failure| failure.ticket);
        GpuOrderTelemetryPoll {
            completed,
            failures,
        }
    }
}

fn copy_count(
    encoder: &mut wgpu::CommandEncoder,
    source: InstanceCountSource<'_>,
    destination: &wgpu::Buffer,
    destination_offset: u64,
) {
    encoder.copy_buffer_to_buffer(
        source.buffer,
        source.offset,
        destination,
        destination_offset,
        std::mem::size_of::<u32>() as u64,
    );
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().expect("telemetry u64 slice width"))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("telemetry u32 slice width"))
}

fn timestamp_delta_ms(begin: u64, end: u64, period_ns: f32) -> Option<f32> {
    if !period_ns.is_finite() || period_ns <= 0.0 {
        return None;
    }
    let millis = end.wrapping_sub(begin) as f64 * f64::from(period_ns) / 1_000_000.0;
    let millis = millis as f32;
    (millis.is_finite() && millis <= MAX_VALID_GPU_ORDER_MS).then_some(millis)
}

#[cfg(test)]
mod tests {
    use super::{
        COMPLETION_PENDING_BITS, CpuOrderCompletionTelemetry, GpuOrderTelemetry,
        InstanceCountSource, LAST_ORDER_TICKET, SLOT_ERROR, SLOT_IDLE,
        SurfaceOrderMeasurementFailureReason, SurfaceTimingSource, next_namespaced_ticket,
        timestamp_delta_ms,
    };

    #[cfg(not(target_arch = "wasm32"))]
    use crate::timer_now;
    #[cfg(not(target_arch = "wasm32"))]
    use wgpu::util::DeviceExt;

    #[test]
    fn timestamp_delta_accepts_quantized_zero_and_wraparound() {
        assert_eq!(timestamp_delta_ms(10, 10, 1.0), Some(0.0));
        assert_eq!(timestamp_delta_ms(u64::MAX - 2, 3, 1.0), Some(0.000_006));
    }

    #[test]
    fn timestamp_delta_rejects_invalid_period_and_implausible_duration() {
        assert_eq!(timestamp_delta_ms(0, 1, 0.0), None);
        assert_eq!(timestamp_delta_ms(0, 1, f32::NAN), None);
        assert_eq!(timestamp_delta_ms(0, u64::MAX / 2, 1.0), None);
    }

    #[test]
    fn backend_ticket_namespaces_stay_exact_in_javascript_numbers() {
        assert_eq!(next_namespaced_ticket(1, 1), 3);
        assert_eq!(next_namespaced_ticket(2, 2), 4);
        assert_eq!(next_namespaced_ticket(LAST_ORDER_TICKET, 1), 1);
        assert_eq!(next_namespaced_ticket(LAST_ORDER_TICKET - 1, 2), 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn test_device(timestamps: bool) -> Option<(wgpu::Device, wgpu::Queue, bool)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok()?;
            let downlevel = adapter.get_downlevel_capabilities();
            let timestamp_supported = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY)
                && downlevel
                    .flags
                    .contains(wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE);
            if timestamps && !timestamp_supported {
                return None;
            }
            let required_features = if timestamps {
                wgpu::Features::TIMESTAMP_QUERY
            } else {
                wgpu::Features::empty()
            };
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("gpu-telemetry-test-device"),
                    required_features,
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()?;
            Some((device, queue, timestamp_supported))
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn indirect_args(device: &wgpu::Device, instance_count: u32) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gpu-telemetry-test-indirect-args"),
            contents: bytemuck::cast_slice(&[6_u32, instance_count, 0, 0]),
            usage: wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cpu_completion_ring_reports_command_completion_not_submit_wall() {
        let Some((device, queue, _)) = test_device(false) else {
            eprintln!("skipping CPU completion telemetry test; adapter unavailable");
            return;
        };
        let mut telemetry = CpuOrderCompletionTelemetry::default();
        let ticket = telemetry
            .begin_sample(17, 1.25, 2.5)
            .expect("fresh completion ring slot");
        assert_eq!(ticket.ticket % 2, 0);
        let started = timer_now();
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("cpu-completion-telemetry-test"),
        });
        let command_buffer = encoder.finish();
        telemetry.arm(&command_buffer, ticket, started);
        queue.submit(Some(command_buffer));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for CPU-order draw completion");

        let poll = telemetry.poll();
        assert!(poll.failures.is_empty());
        assert_eq!(poll.completed.len(), 1);
        assert_eq!(poll.completed[0].camera_revision, 17);
        assert_eq!(poll.completed[0].preprocess_ms, 1.25);
        assert_eq!(poll.completed[0].sort_ms, 2.5);
        assert!(poll.completed[0].frame_complete_ms.is_finite());
        assert!(poll.completed[0].frame_complete_ms >= 0.0);
    }

    #[test]
    fn cpu_generation_invalidation_terminally_fails_an_issued_ticket_once() {
        let mut telemetry = CpuOrderCompletionTelemetry::default();
        let ticket = telemetry
            .begin_sample(19, 1.0, 2.0)
            .expect("fresh CPU telemetry ring slot");

        telemetry.invalidate_generation();
        let first = telemetry.poll();
        assert!(first.completed.is_empty());
        assert_eq!(first.failures.len(), 1);
        assert_eq!(first.failures[0].ticket, ticket.ticket);
        assert_eq!(first.failures[0].camera_revision, 19);
        assert_eq!(
            first.failures[0].reason,
            SurfaceOrderMeasurementFailureReason::GenerationInvalidated
        );

        let second = telemetry.poll();
        assert!(second.completed.is_empty());
        assert!(second.failures.is_empty());

        // The invalidated ticket was still in Encoding and therefore had no
        // callback that could recycle its ring slot.
        assert_eq!(
            telemetry.slots[ticket.slot]
                .state
                .load(std::sync::atomic::Ordering::Acquire),
            SLOT_IDLE
        );
        assert!(telemetry.begin_sample(20, 3.0, 4.0).is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gpu_completion_only_ring_returns_exact_indirect_count() {
        let Some((device, queue, _)) = test_device(false) else {
            eprintln!("skipping GPU completion telemetry test; adapter unavailable");
            return;
        };
        let candidate = indirect_args(&device, 123_456);
        let contributor = indirect_args(&device, 120_000);
        let drawn = indirect_args(&device, 120_000);
        let mut telemetry = GpuOrderTelemetry::new(&device, &queue, false);
        let ticket = telemetry
            .begin_sample(23, true)
            .expect("fresh GPU telemetry ring slot");
        assert!(ticket.query_set.is_none());
        let started = timer_now();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-completion-only-telemetry-test"),
        });
        telemetry.encode_instance_count_readback(
            &mut encoder,
            &ticket,
            InstanceCountSource::indirect_args(&candidate),
            InstanceCountSource::indirect_args(&contributor),
            InstanceCountSource::indirect_args(&drawn),
            true,
        );
        let command_buffer = encoder.finish();
        telemetry.arm(&command_buffer, ticket, started);
        queue.submit(Some(command_buffer));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for GPU completion telemetry");

        let poll = telemetry.poll(&device);
        assert!(poll.failures.is_empty());
        assert_eq!(poll.completed.len(), 1);
        let sample = poll.completed[0];
        assert_eq!(sample.camera_revision, 23);
        assert_eq!(sample.timing_source, SurfaceTimingSource::CompletionOnly);
        assert_eq!(sample.visible_count, 123_456);
        assert_eq!(sample.contributor_count, 120_000);
        assert_eq!(sample.drawn_count, 120_000);
        assert!(sample.exact_contributor_compaction);
        assert_eq!(sample.gpu_order_ms, None);
        assert!(sample.gpu_complete_ms.is_finite());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn timestamp_ring_resolves_nonblocking_when_adapter_supports_it() {
        let Some((device, queue, _)) = test_device(true) else {
            eprintln!("skipping timestamp telemetry test; feature unavailable");
            return;
        };
        let indirect = indirect_args(&device, 777);
        let mut telemetry = GpuOrderTelemetry::new(&device, &queue, true);
        let ticket = telemetry
            .begin_sample(29, true)
            .expect("fresh timestamp telemetry ring slot");
        let query_set = ticket
            .query_set
            .as_ref()
            .expect("timestamp-enabled ticket")
            .clone();
        let started = timer_now();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-timestamp-telemetry-test"),
        });
        {
            let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu-timestamp-telemetry-keygen-test-pass"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            });
        }
        {
            let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu-timestamp-telemetry-radix-test-pass"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &query_set,
                    beginning_of_pass_write_index: Some(2),
                    end_of_pass_write_index: Some(3),
                }),
            });
        }
        telemetry.encode_readback(&mut encoder, &ticket, &indirect);
        let command_buffer = encoder.finish();
        telemetry.arm(&command_buffer, ticket, started);
        queue.submit(Some(command_buffer));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for timestamp telemetry");

        let poll = telemetry.poll(&device);
        assert!(poll.failures.is_empty());
        assert_eq!(poll.completed.len(), 1);
        let sample = poll.completed[0];
        assert_eq!(sample.timing_source, SurfaceTimingSource::TimestampQuery);
        assert_eq!(
            (
                sample.visible_count,
                sample.contributor_count,
                sample.drawn_count,
            ),
            (777, 777, 777)
        );
        assert!(sample.gpu_preprocess_ms.is_some());
        assert!(sample.gpu_radix_ms.is_some());
        assert!(sample.gpu_order_ms.is_some());
        assert!(sample.timestamp_period_ns.is_some());
        assert!(!sample.exact_contributor_compaction);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn issued_gpu_ticket_gets_exactly_one_readback_failure_terminal_receipt() {
        let Some((device, queue, _)) = test_device(false) else {
            eprintln!("skipping GPU telemetry failure test; adapter unavailable");
            return;
        };
        let mut telemetry = GpuOrderTelemetry::new(&device, &queue, false);
        let ticket = telemetry
            .begin_sample(31, false)
            .expect("fresh GPU telemetry ring slot");
        let slot = &mut telemetry.slots[ticket.slot];
        slot.completion_ms_bits
            .store(1.0_f32.to_bits(), std::sync::atomic::Ordering::Release);
        slot.state
            .store(SLOT_ERROR, std::sync::atomic::Ordering::Release);

        let first = telemetry.poll(&device);
        assert!(first.completed.is_empty());
        assert_eq!(first.failures.len(), 1);
        assert_eq!(first.failures[0].ticket, ticket.ticket);
        assert_eq!(first.failures[0].camera_revision, 31);
        assert_eq!(
            first.failures[0].reason,
            SurfaceOrderMeasurementFailureReason::ReadbackMap
        );

        let second = telemetry.poll(&device);
        assert!(second.completed.is_empty());
        assert!(second.failures.is_empty());
        assert_ne!(COMPLETION_PENDING_BITS, 1.0_f32.to_bits());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn generation_invalidation_terminally_fails_every_outstanding_ticket_once() {
        let Some((device, queue, _)) = test_device(false) else {
            eprintln!("skipping GPU telemetry invalidation test; adapter unavailable");
            return;
        };
        let mut telemetry = GpuOrderTelemetry::new(&device, &queue, false);
        let ticket = telemetry
            .begin_sample(37, false)
            .expect("fresh GPU telemetry ring slot");

        telemetry.invalidate_generation();
        let first = telemetry.poll(&device);
        assert!(first.completed.is_empty());
        assert_eq!(first.failures.len(), 1);
        assert_eq!(first.failures[0].ticket, ticket.ticket);
        assert_eq!(
            first.failures[0].reason,
            SurfaceOrderMeasurementFailureReason::GenerationInvalidated
        );
        let second = telemetry.poll(&device);
        assert!(second.failures.is_empty());
        assert_eq!(
            telemetry.slots[ticket.slot]
                .state
                .load(std::sync::atomic::Ordering::Acquire),
            SLOT_IDLE
        );
    }
}
