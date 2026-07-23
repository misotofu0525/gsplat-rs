//! Non-blocking receipts for the Packed GPU order producer A/B path.
//!
//! This ring is deliberately separate from projected Candidate/Compact
//! telemetry. A producer receipt describes the complete GPU-lane graph that
//! reached one presented frame, including the current-camera contributor
//! count and whether the issued order was refreshed or intentionally stale.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

pub use crate::api::SurfaceGpuOrderProducer;
pub use crate::evidence::{
    SurfaceGpuProducerDrawScope, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceGpuProducerMeasurementFailureReason,
};
use crate::{TimerInstant, timer_elapsed_ms, wgpu_label};

const RING_SLOTS: usize = 8;
const CONTRIBUTOR_OFFSET: u64 = 0;
const DRAWN_OFFSET: u64 = CONTRIBUTOR_OFFSET + std::mem::size_of::<u32>() as u64;
const READBACK_BYTES: u64 = 4 * std::mem::size_of::<u32>() as u64;

const SLOT_IDLE: u8 = 0;
const SLOT_ENCODING: u8 = 1;
const SLOT_SUBMITTED: u8 = 2;
const SLOT_MAPPED: u8 = 3;
const SLOT_ERROR: u8 = 4;
const COMPLETION_PENDING_BITS: u32 = u32::MAX;
const MAX_VALID_GPU_PRODUCER_MS: f32 = 60_000.0;

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
const FIRST_GPU_PRODUCER_TICKET: u64 = 1_u64 << 51;
const LAST_GPU_PRODUCER_TICKET: u64 = (1_u64 << 52) - 1;
const _: () = assert!(FIRST_GPU_PRODUCER_TICKET > ((1_u64 << 51) - 1));
const _: () = assert!(LAST_GPU_PRODUCER_TICKET < (1_u64 << 52));
const _: () = assert!(LAST_GPU_PRODUCER_TICKET <= MAX_JAVASCRIPT_SAFE_INTEGER);

fn next_gpu_producer_ticket(current: u64) -> u64 {
    current
        .checked_add(1)
        .filter(|next| *next <= LAST_GPU_PRODUCER_TICKET)
        .unwrap_or(FIRST_GPU_PRODUCER_TICKET)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuProducerSampleMetadata {
    pub(crate) camera_revision: u64,
    pub(crate) producer: SurfaceGpuOrderProducer,
    pub(crate) order_generation: u64,
    pub(crate) projection_generation: u64,
    pub(crate) source_count: u32,
    pub(crate) order_refreshed: bool,
    pub(crate) draw_scope: SurfaceGpuProducerDrawScope,
}

#[derive(Clone, Copy)]
pub(crate) struct GpuProducerCountSource<'a> {
    buffer: &'a wgpu::Buffer,
    offset: u64,
}

impl<'a> GpuProducerCountSource<'a> {
    pub(crate) const fn indirect_args(buffer: &'a wgpu::Buffer) -> Self {
        Self {
            buffer,
            offset: std::mem::size_of::<u32>() as u64,
        }
    }

    pub(crate) const fn raw(buffer: &'a wgpu::Buffer, offset: u64) -> Self {
        Self { buffer, offset }
    }
}

struct GpuProducerTelemetrySlot {
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    completion_ms_bits: Arc<AtomicU32>,
    ticket: u64,
    generation: u64,
    metadata: GpuProducerSampleMetadata,
    readback_encoded: bool,
    terminal_reported: bool,
}

#[must_use = "a GPU producer telemetry reservation must be armed or cancelled"]
pub(crate) struct GpuProducerTelemetryReservation {
    slot: usize,
    ticket: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuProducerTelemetryTicket {
    pub(crate) ticket: u64,
}

pub(crate) struct GpuProducerTelemetryPoll {
    pub(crate) completed: Vec<SurfaceGpuProducerMeasurement>,
    pub(crate) failures: Vec<SurfaceGpuProducerMeasurementFailure>,
}

pub(crate) struct GpuProducerTelemetry {
    slots: Vec<GpuProducerTelemetrySlot>,
    next_slot: usize,
    next_ticket: u64,
    generation: u64,
    pending_failures: VecDeque<SurfaceGpuProducerMeasurementFailure>,
}

impl GpuProducerTelemetry {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let empty_metadata = GpuProducerSampleMetadata {
            camera_revision: 0,
            producer: SurfaceGpuOrderProducer::PostSort,
            order_generation: 0,
            projection_generation: 0,
            source_count: 0,
            order_refreshed: false,
            draw_scope: SurfaceGpuProducerDrawScope::StaleOrderCandidates,
        };
        let slots = (0..RING_SLOTS)
            .map(|_| GpuProducerTelemetrySlot {
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: wgpu_label("gsplat-gpu-producer-telemetry-readback"),
                    size: READBACK_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(SLOT_IDLE)),
                completion_ms_bits: Arc::new(AtomicU32::new(COMPLETION_PENDING_BITS)),
                ticket: 0,
                generation: 0,
                metadata: empty_metadata,
                readback_encoded: false,
                terminal_reported: false,
            })
            .collect();
        Self {
            slots,
            next_slot: 0,
            next_ticket: FIRST_GPU_PRODUCER_TICKET,
            generation: 1,
            pending_failures: VecDeque::with_capacity(RING_SLOTS),
        }
    }

    pub(crate) fn invalidate_generation(&mut self) {
        for slot in &mut self.slots {
            let state = slot.state.load(Ordering::Acquire);
            match state {
                SLOT_IDLE => {}
                SLOT_ENCODING => {
                    slot.terminal_reported = true;
                    slot.state.store(SLOT_IDLE, Ordering::Release);
                }
                _ if !slot.terminal_reported => {
                    self.pending_failures
                        .push_back(SurfaceGpuProducerMeasurementFailure {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            producer: slot.metadata.producer,
                            order_generation: slot.metadata.order_generation,
                            projection_generation: slot.metadata.projection_generation,
                            reason: invalidation_failure_reason(state),
                        });
                    slot.terminal_reported = true;
                }
                _ => {}
            }
        }
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    pub(crate) fn begin_sample(
        &mut self,
        metadata: GpuProducerSampleMetadata,
    ) -> Option<GpuProducerTelemetryReservation> {
        let slot_index = (0..self.slots.len())
            .map(|offset| (self.next_slot + offset) % self.slots.len())
            .find(|&index| self.slots[index].state.load(Ordering::Acquire) == SLOT_IDLE)?;
        self.next_slot = (slot_index + 1) % self.slots.len();

        let ticket = self.next_ticket;
        self.next_ticket = next_gpu_producer_ticket(self.next_ticket);
        let slot = &mut self.slots[slot_index];
        slot.ticket = ticket;
        slot.generation = self.generation;
        slot.metadata = metadata;
        slot.readback_encoded = false;
        slot.terminal_reported = false;
        slot.completion_ms_bits
            .store(COMPLETION_PENDING_BITS, Ordering::Release);
        slot.state.store(SLOT_ENCODING, Ordering::Release);
        Some(GpuProducerTelemetryReservation {
            slot: slot_index,
            ticket,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cancel(&mut self, reservation: GpuProducerTelemetryReservation) -> bool {
        let Some(slot) = self.slots.get_mut(reservation.slot) else {
            return false;
        };
        if slot.ticket != reservation.ticket || slot.state.load(Ordering::Acquire) != SLOT_ENCODING
        {
            return false;
        }
        slot.terminal_reported = true;
        slot.state.store(SLOT_IDLE, Ordering::Release);
        true
    }

    pub(crate) fn encode_count_readback(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        reservation: &GpuProducerTelemetryReservation,
        contributor: GpuProducerCountSource<'_>,
        drawn: GpuProducerCountSource<'_>,
    ) -> bool {
        let Some(slot) = self.slots.get_mut(reservation.slot) else {
            return false;
        };
        if slot.ticket != reservation.ticket || slot.state.load(Ordering::Acquire) != SLOT_ENCODING
        {
            return false;
        }
        copy_count(encoder, contributor, &slot.readback, CONTRIBUTOR_OFFSET);
        copy_count(encoder, drawn, &slot.readback, DRAWN_OFFSET);
        encoder.clear_buffer(
            &slot.readback,
            DRAWN_OFFSET + std::mem::size_of::<u32>() as u64,
            Some(2 * std::mem::size_of::<u32>() as u64),
        );
        slot.readback_encoded = true;
        true
    }

    pub(crate) fn arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        reservation: GpuProducerTelemetryReservation,
        completion_started: TimerInstant,
    ) -> Option<GpuProducerTelemetryTicket> {
        let slot = self.slots.get_mut(reservation.slot)?;
        if slot.ticket != reservation.ticket || slot.state.load(Ordering::Acquire) != SLOT_ENCODING
        {
            return None;
        }
        if !slot.readback_encoded {
            slot.terminal_reported = true;
            slot.state.store(SLOT_IDLE, Ordering::Release);
            return None;
        }

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
        let completion_ms_bits = Arc::clone(&slot.completion_ms_bits);
        command_buffer.on_submitted_work_done(move || {
            completion_ms_bits.store(
                timer_elapsed_ms(completion_started).to_bits(),
                Ordering::Release,
            );
        });
        Some(GpuProducerTelemetryTicket {
            ticket: reservation.ticket,
        })
    }

    pub(crate) fn poll(&mut self, device: &wgpu::Device) -> GpuProducerTelemetryPoll {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut completed = Vec::new();
        let mut failures: Vec<_> = self.pending_failures.drain(..).collect();
        for slot in &mut self.slots {
            match slot.state.load(Ordering::Acquire) {
                SLOT_ERROR => {
                    if !slot.terminal_reported {
                        failures.push(failure(
                            slot,
                            SurfaceGpuProducerMeasurementFailureReason::ReadbackMap,
                        ));
                        slot.terminal_reported = true;
                    }
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
                    let contributor_count = read_u32(&bytes[0..4]);
                    let drawn_count = read_u32(&bytes[4..8]);
                    drop(bytes);
                    slot.readback.unmap();
                    let frame_complete_ms = f32::from_bits(completion_bits);
                    if slot.generation == self.generation
                        && !slot.terminal_reported
                        && producer_measurement_is_valid(
                            slot.metadata,
                            frame_complete_ms,
                            contributor_count,
                            drawn_count,
                        )
                    {
                        completed.push(SurfaceGpuProducerMeasurement {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            producer: slot.metadata.producer,
                            order_generation: slot.metadata.order_generation,
                            projection_generation: slot.metadata.projection_generation,
                            source_count: slot.metadata.source_count,
                            contributor_count,
                            drawn_count,
                            order_refreshed: slot.metadata.order_refreshed,
                            draw_scope: slot.metadata.draw_scope,
                            frame_complete_ms,
                        });
                        slot.terminal_reported = true;
                    } else if slot.generation == self.generation && !slot.terminal_reported {
                        failures.push(failure(
                            slot,
                            SurfaceGpuProducerMeasurementFailureReason::InvariantViolation,
                        ));
                        slot.terminal_reported = true;
                    } else if slot.generation != self.generation && !slot.terminal_reported {
                        failures.push(failure(
                            slot,
                            SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated,
                        ));
                        slot.terminal_reported = true;
                    }
                    slot.state.store(SLOT_IDLE, Ordering::Release);
                }
                _ => {}
            }
        }
        completed.sort_by_key(|sample| sample.ticket);
        failures.sort_by_key(|failure| failure.ticket);
        GpuProducerTelemetryPoll {
            completed,
            failures,
        }
    }
}

fn invalidation_failure_reason(state: u8) -> SurfaceGpuProducerMeasurementFailureReason {
    if state == SLOT_ERROR {
        SurfaceGpuProducerMeasurementFailureReason::ReadbackMap
    } else {
        SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated
    }
}

fn failure(
    slot: &GpuProducerTelemetrySlot,
    reason: SurfaceGpuProducerMeasurementFailureReason,
) -> SurfaceGpuProducerMeasurementFailure {
    SurfaceGpuProducerMeasurementFailure {
        ticket: slot.ticket,
        camera_revision: slot.metadata.camera_revision,
        producer: slot.metadata.producer,
        order_generation: slot.metadata.order_generation,
        projection_generation: slot.metadata.projection_generation,
        reason,
    }
}

fn producer_measurement_is_valid(
    metadata: GpuProducerSampleMetadata,
    frame_complete_ms: f32,
    contributor_count: u32,
    drawn_count: u32,
) -> bool {
    let valid_scope = match metadata.draw_scope {
        SurfaceGpuProducerDrawScope::ExactCurrentContributors => {
            metadata.order_refreshed && drawn_count == contributor_count
        }
        SurfaceGpuProducerDrawScope::StaleOrderCandidates => !metadata.order_refreshed,
    };
    frame_complete_ms.is_finite()
        && (0.0..=MAX_VALID_GPU_PRODUCER_MS).contains(&frame_complete_ms)
        && contributor_count <= metadata.source_count
        && drawn_count <= metadata.source_count
        && valid_scope
}

fn copy_count(
    encoder: &mut wgpu::CommandEncoder,
    source: GpuProducerCountSource<'_>,
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

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("GPU producer telemetry u32 width"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use wgpu::util::DeviceExt;

    fn metadata(
        refreshed: bool,
        draw_scope: SurfaceGpuProducerDrawScope,
    ) -> GpuProducerSampleMetadata {
        GpuProducerSampleMetadata {
            camera_revision: 7,
            producer: SurfaceGpuOrderProducer::Preproject,
            order_generation: 3,
            projection_generation: 9,
            source_count: 100,
            order_refreshed: refreshed,
            draw_scope,
        }
    }

    #[test]
    fn ticket_namespace_is_javascript_safe_and_disjoint() {
        assert_eq!(
            next_gpu_producer_ticket(LAST_GPU_PRODUCER_TICKET),
            FIRST_GPU_PRODUCER_TICKET
        );
    }

    #[test]
    fn refreshed_and_stale_receipts_have_distinct_count_contracts() {
        assert!(producer_measurement_is_valid(
            metadata(true, SurfaceGpuProducerDrawScope::ExactCurrentContributors),
            16.0,
            60,
            60,
        ));
        assert!(!producer_measurement_is_valid(
            metadata(true, SurfaceGpuProducerDrawScope::ExactCurrentContributors),
            16.0,
            60,
            59,
        ));
        assert!(producer_measurement_is_valid(
            metadata(false, SurfaceGpuProducerDrawScope::StaleOrderCandidates),
            16.0,
            75,
            60,
        ));
        assert!(producer_measurement_is_valid(
            metadata(false, SurfaceGpuProducerDrawScope::StaleOrderCandidates),
            16.0,
            40,
            60,
        ));
        assert!(!producer_measurement_is_valid(
            metadata(false, SurfaceGpuProducerDrawScope::ExactCurrentContributors),
            16.0,
            60,
            60,
        ));
    }

    #[test]
    fn count_and_completion_bounds_fail_closed() {
        let current = metadata(true, SurfaceGpuProducerDrawScope::ExactCurrentContributors);
        assert!(!producer_measurement_is_valid(current, f32::NAN, 60, 60));
        assert!(!producer_measurement_is_valid(current, 16.0, 101, 101));
        assert!(!producer_measurement_is_valid(current, 16.0, 60, 101));
    }

    #[test]
    fn map_error_remains_the_terminal_reason_during_generation_invalidation() {
        assert_eq!(
            invalidation_failure_reason(SLOT_ERROR),
            SurfaceGpuProducerMeasurementFailureReason::ReadbackMap,
        );
        assert_eq!(
            invalidation_failure_reason(SLOT_SUBMITTED),
            SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated,
        );
        assert_eq!(
            invalidation_failure_reason(SLOT_MAPPED),
            SurfaceGpuProducerMeasurementFailureReason::GenerationInvalidated,
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
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
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("GPU producer telemetry test device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn count_buffer(device: &wgpu::Device, value: u32) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GPU producer telemetry count"),
            contents: bytemuck::bytes_of(&value),
            usage: wgpu::BufferUsages::COPY_SRC,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn queue_terminal_receipt_reports_actual_counts_and_generations() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping GPU producer telemetry test; adapter unavailable");
            return;
        };
        let contributor = count_buffer(&device, 60);
        let drawn = count_buffer(&device, 60);
        let mut telemetry = GpuProducerTelemetry::new(&device);

        let cancelled = telemetry
            .begin_sample(metadata(
                true,
                SurfaceGpuProducerDrawScope::ExactCurrentContributors,
            ))
            .expect("cancellable slot");
        assert!(telemetry.cancel(cancelled));

        let reservation = telemetry
            .begin_sample(metadata(
                true,
                SurfaceGpuProducerDrawScope::ExactCurrentContributors,
            ))
            .expect("producer telemetry slot");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GPU producer telemetry encoder"),
        });
        assert!(telemetry.encode_count_readback(
            &mut encoder,
            &reservation,
            GpuProducerCountSource::raw(&contributor, 0),
            GpuProducerCountSource::raw(&drawn, 0),
        ));
        let command_buffer = encoder.finish();
        let issued = telemetry
            .arm(&command_buffer, reservation, crate::timer_now())
            .expect("encoded producer reservation");
        queue.submit(Some(command_buffer));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for producer receipt");

        let poll = telemetry.poll(&device);
        assert!(poll.failures.is_empty());
        assert_eq!(poll.completed.len(), 1);
        let sample = poll.completed[0];
        assert_eq!(sample.ticket, issued.ticket);
        assert_eq!(sample.producer, SurfaceGpuOrderProducer::Preproject);
        assert_eq!(
            (sample.order_generation, sample.projection_generation),
            (3, 9)
        );
        assert_eq!(
            (
                sample.source_count,
                sample.contributor_count,
                sample.drawn_count
            ),
            (100, 60, 60)
        );
        assert!(sample.exact_current_contributor_draw());
        assert!(!sample.stale_order());
        assert!(sample.frame_complete_ms.is_finite());
    }
}
