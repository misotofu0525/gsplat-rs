//! Non-blocking measurements for projected draw execution.
//!
//! This ring is intentionally independent from order-backend telemetry. A
//! projected draw sample measures the command-buffer completion interval and
//! the exact candidate/contributor/drawn counts for one `Candidate` or
//! `Compact` execution. Reserving a slot does not expose its ticket; `arm` is
//! the exposure boundary, so an abandoned encoding reservation can be
//! cancelled without manufacturing a public failure receipt.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

pub use crate::api::SurfaceProjectedDrawExecution;
pub use crate::evidence::{
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
    SurfaceProjectedDrawMeasurementFailureReason,
};
use crate::{SurfaceOrderBackendUsed, TimerInstant, timer_elapsed_ms, wgpu_label};

const RING_SLOTS: usize = 8;
const CANDIDATE_VISIBLE_OFFSET: u64 = 0;
const CONTRIBUTOR_OFFSET: u64 = CANDIDATE_VISIBLE_OFFSET + std::mem::size_of::<u32>() as u64;
const DRAWN_OFFSET: u64 = CONTRIBUTOR_OFFSET + std::mem::size_of::<u32>() as u64;
// Keep the mapped range aligned to COPY_BUFFER_ALIGNMENT while storing three
// u32 counts. The final word is deterministic padding.
const READBACK_BYTES: u64 = 4 * std::mem::size_of::<u32>() as u64;

const SLOT_IDLE: u8 = 0;
const SLOT_ENCODING: u8 = 1;
const SLOT_SUBMITTED: u8 = 2;
const SLOT_MAPPED: u8 = 3;
const SLOT_ERROR: u8 = 4;
const COMPLETION_PENDING_BITS: u32 = u32::MAX;
const MAX_VALID_PROJECTED_DRAW_MS: f32 = 60_000.0;

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
// Order measurements own [1, 2^51), producer A/B receipts own [2^51, 2^52),
// and projected draw measurements use the high JS-safe quarter. Tickets
// remain exactly representable by JavaScript numbers.
const FIRST_PROJECTED_DRAW_TICKET: u64 = 1_u64 << 52;

fn next_projected_draw_ticket(current: u64) -> u64 {
    current
        .checked_add(1)
        .filter(|next| *next <= MAX_JAVASCRIPT_SAFE_INTEGER)
        .unwrap_or(FIRST_PROJECTED_DRAW_TICKET)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectedDrawSampleMetadata {
    pub(crate) camera_revision: u64,
    pub(crate) execution: SurfaceProjectedDrawExecution,
    pub(crate) order_backend: SurfaceOrderBackendUsed,
    pub(crate) projection_generation: u64,
    pub(crate) probe_generation: u64,
    pub(crate) projection_rebuilt: bool,
    pub(crate) order_refreshed: bool,
}

/// Source of one u32 instance count copied into the shared V/C/D readback.
#[derive(Clone, Copy)]
pub(crate) struct ProjectedDrawCountSource<'a> {
    buffer: &'a wgpu::Buffer,
    offset: u64,
}

impl<'a> ProjectedDrawCountSource<'a> {
    /// Read `instance_count` from a standard non-indexed indirect draw record.
    pub(crate) const fn indirect_args(buffer: &'a wgpu::Buffer) -> Self {
        Self {
            buffer,
            offset: std::mem::size_of::<u32>() as u64,
        }
    }

    /// Read a raw u32 at `offset` from any COPY_SRC buffer.
    pub(crate) const fn raw(buffer: &'a wgpu::Buffer, offset: u64) -> Self {
        Self { buffer, offset }
    }
}

struct ProjectedDrawTelemetrySlot {
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    completion_ms_bits: Arc<AtomicU32>,
    ticket: u64,
    generation: u64,
    metadata: ProjectedDrawSampleMetadata,
    readback_encoded: bool,
    terminal_reported: bool,
}

/// An unexposed ring reservation. It may be encoded and armed, or cancelled
/// without producing a terminal receipt.
#[must_use = "a projected draw telemetry reservation must be armed or cancelled"]
pub(crate) struct ProjectedDrawTelemetryReservation {
    slot: usize,
    ticket: u64,
}

/// Identity exposed only after the owning command buffer has been armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectedDrawTelemetryTicket {
    pub(crate) ticket: u64,
}

pub(crate) struct ProjectedDrawTelemetryPoll {
    pub(crate) completed: Vec<SurfaceProjectedDrawMeasurement>,
    pub(crate) failures: Vec<SurfaceProjectedDrawMeasurementFailure>,
}

/// Eight-slot callback-driven ring for projected draw completion and V/C/D.
pub(crate) struct ProjectedDrawTelemetry {
    slots: Vec<ProjectedDrawTelemetrySlot>,
    next_slot: usize,
    next_ticket: u64,
    generation: u64,
    pending_failures: VecDeque<SurfaceProjectedDrawMeasurementFailure>,
}

impl ProjectedDrawTelemetry {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let slots = (0..RING_SLOTS)
            .map(|_| ProjectedDrawTelemetrySlot {
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: wgpu_label("gsplat-projected-draw-telemetry-readback"),
                    size: READBACK_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(SLOT_IDLE)),
                completion_ms_bits: Arc::new(AtomicU32::new(COMPLETION_PENDING_BITS)),
                ticket: 0,
                generation: 0,
                metadata: ProjectedDrawSampleMetadata {
                    camera_revision: 0,
                    execution: SurfaceProjectedDrawExecution::Candidate,
                    order_backend: SurfaceOrderBackendUsed::Cpu,
                    projection_generation: 0,
                    probe_generation: 0,
                    projection_rebuilt: false,
                    order_refreshed: false,
                },
                readback_encoded: false,
                terminal_reported: false,
            })
            .collect();
        Self {
            slots,
            next_slot: 0,
            next_ticket: FIRST_PROJECTED_DRAW_TICKET,
            generation: 1,
            pending_failures: VecDeque::with_capacity(RING_SLOTS),
        }
    }

    /// Invalidates measurements tied to the previous presenter generation.
    ///
    /// Encoding reservations have not exposed an identity and are recycled
    /// silently. Armed slots retain their buffers until both map and completion
    /// callbacks finish, while their exposed ticket immediately receives one
    /// terminal failure receipt.
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
                        .push_back(SurfaceProjectedDrawMeasurementFailure {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            execution: slot.metadata.execution,
                            order_backend: slot.metadata.order_backend,
                            projection_generation: slot.metadata.projection_generation,
                            probe_generation: slot.metadata.probe_generation,
                            reason: if state == SLOT_ERROR {
                                SurfaceProjectedDrawMeasurementFailureReason::ReadbackMap
                            } else {
                                SurfaceProjectedDrawMeasurementFailureReason::GenerationInvalidated
                            },
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
        metadata: ProjectedDrawSampleMetadata,
    ) -> Option<ProjectedDrawTelemetryReservation> {
        let slot_index = (0..self.slots.len())
            .map(|offset| (self.next_slot + offset) % self.slots.len())
            .find(|&index| self.slots[index].state.load(Ordering::Acquire) == SLOT_IDLE)?;
        self.next_slot = (slot_index + 1) % self.slots.len();

        let ticket = self.next_ticket;
        self.next_ticket = next_projected_draw_ticket(self.next_ticket);

        let slot = &mut self.slots[slot_index];
        slot.ticket = ticket;
        slot.generation = self.generation;
        slot.metadata = metadata;
        slot.readback_encoded = false;
        slot.terminal_reported = false;
        slot.completion_ms_bits
            .store(COMPLETION_PENDING_BITS, Ordering::Release);
        slot.state.store(SLOT_ENCODING, Ordering::Release);

        Some(ProjectedDrawTelemetryReservation {
            slot: slot_index,
            ticket,
        })
    }

    /// Cancels an unexposed encoding reservation without emitting a failure.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cancel(&mut self, reservation: ProjectedDrawTelemetryReservation) -> bool {
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

    /// Encodes V, C, and D copies into this slot's single mapped readback.
    ///
    /// All sources must include COPY_SRC usage and remain alive through command
    /// submission. Returning false means the reservation was stale or already
    /// left its encoding state; no commands are encoded in that case.
    pub(crate) fn encode_count_readback(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        reservation: &ProjectedDrawTelemetryReservation,
        candidate_visible: ProjectedDrawCountSource<'_>,
        contributor: ProjectedDrawCountSource<'_>,
        drawn: ProjectedDrawCountSource<'_>,
    ) -> bool {
        let Some(slot) = self.slots.get_mut(reservation.slot) else {
            return false;
        };
        if slot.ticket != reservation.ticket || slot.state.load(Ordering::Acquire) != SLOT_ENCODING
        {
            return false;
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
        slot.readback_encoded = true;
        true
    }

    /// Arms callbacks on the command buffer and exposes the ticket identity.
    ///
    /// Returning `None` means the reservation was stale or V/C/D readback was
    /// not encoded. A matching unencoded reservation is recycled silently.
    pub(crate) fn arm(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        reservation: ProjectedDrawTelemetryReservation,
        completion_started: TimerInstant,
    ) -> Option<ProjectedDrawTelemetryTicket> {
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

        Some(ProjectedDrawTelemetryTicket {
            ticket: reservation.ticket,
        })
    }

    pub(crate) fn poll(&mut self, device: &wgpu::Device) -> ProjectedDrawTelemetryPoll {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut completed = Vec::new();
        let mut failures: Vec<_> = self.pending_failures.drain(..).collect();

        for slot in &mut self.slots {
            match slot.state.load(Ordering::Acquire) {
                SLOT_ERROR => {
                    if !slot.terminal_reported {
                        failures.push(SurfaceProjectedDrawMeasurementFailure {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            execution: slot.metadata.execution,
                            order_backend: slot.metadata.order_backend,
                            projection_generation: slot.metadata.projection_generation,
                            probe_generation: slot.metadata.probe_generation,
                            reason: SurfaceProjectedDrawMeasurementFailureReason::ReadbackMap,
                        });
                        slot.terminal_reported = true;
                    }
                    // A map error and command completion are distinct
                    // callbacks. Reusing this slot before completion would let
                    // a late callback overwrite a newer sample's atomics.
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

                    let frame_complete_ms = f32::from_bits(completion_bits);
                    if slot.generation == self.generation
                        && !slot.terminal_reported
                        // Formal projected samples are deliberately isolated
                        // from order work and forced to rebuild their cache.
                        // Treat any wiring regression as a terminal invalid
                        // sample instead of contaminating the adaptive lane.
                        && projected_draw_sample_is_isolated(slot.metadata)
                        && projected_draw_measurement_is_valid(
                            slot.metadata.execution,
                            frame_complete_ms,
                            visible_count,
                            contributor_count,
                            drawn_count,
                        )
                    {
                        completed.push(SurfaceProjectedDrawMeasurement {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            execution: slot.metadata.execution,
                            order_backend: slot.metadata.order_backend,
                            projection_generation: slot.metadata.projection_generation,
                            probe_generation: slot.metadata.probe_generation,
                            projection_rebuilt: slot.metadata.projection_rebuilt,
                            order_refreshed: slot.metadata.order_refreshed,
                            frame_complete_ms,
                            visible_count,
                            contributor_count,
                            drawn_count,
                            exact_contributor_compaction: slot
                                .metadata
                                .execution
                                .exact_contributor_compaction(),
                        });
                        slot.terminal_reported = true;
                    } else if slot.generation == self.generation && !slot.terminal_reported {
                        failures.push(SurfaceProjectedDrawMeasurementFailure {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            execution: slot.metadata.execution,
                            order_backend: slot.metadata.order_backend,
                            projection_generation: slot.metadata.projection_generation,
                            probe_generation: slot.metadata.probe_generation,
                            reason:
                                SurfaceProjectedDrawMeasurementFailureReason::InvariantViolation,
                        });
                        slot.terminal_reported = true;
                    } else if slot.generation != self.generation && !slot.terminal_reported {
                        // Defensive fallback: normal invalidation queues this
                        // receipt eagerly, but never silently discard a stale
                        // exposed ticket if generation state changes elsewhere.
                        failures.push(SurfaceProjectedDrawMeasurementFailure {
                            ticket: slot.ticket,
                            camera_revision: slot.metadata.camera_revision,
                            execution: slot.metadata.execution,
                            order_backend: slot.metadata.order_backend,
                            projection_generation: slot.metadata.projection_generation,
                            probe_generation: slot.metadata.probe_generation,
                            reason:
                                SurfaceProjectedDrawMeasurementFailureReason::GenerationInvalidated,
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
        ProjectedDrawTelemetryPoll {
            completed,
            failures,
        }
    }
}

fn projected_draw_sample_is_isolated(metadata: ProjectedDrawSampleMetadata) -> bool {
    metadata.projection_rebuilt && !metadata.order_refreshed
}

fn projected_draw_measurement_is_valid(
    execution: SurfaceProjectedDrawExecution,
    frame_complete_ms: f32,
    visible_count: u32,
    contributor_count: u32,
    drawn_count: u32,
) -> bool {
    frame_complete_ms.is_finite()
        && (0.0..=MAX_VALID_PROJECTED_DRAW_MS).contains(&frame_complete_ms)
        && contributor_count <= visible_count
        && match execution {
            SurfaceProjectedDrawExecution::Candidate => drawn_count == visible_count,
            SurfaceProjectedDrawExecution::Compact => drawn_count == contributor_count,
        }
}

fn copy_count(
    encoder: &mut wgpu::CommandEncoder,
    source: ProjectedDrawCountSource<'_>,
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
    u32::from_le_bytes(
        bytes
            .try_into()
            .expect("projected draw telemetry u32 width"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        FIRST_PROJECTED_DRAW_TICKET, MAX_JAVASCRIPT_SAFE_INTEGER, ProjectedDrawCountSource,
        ProjectedDrawSampleMetadata, ProjectedDrawTelemetry, SLOT_IDLE,
        SurfaceProjectedDrawExecution, SurfaceProjectedDrawMeasurementFailureReason,
        next_projected_draw_ticket,
    };
    use crate::SurfaceOrderBackendUsed;

    #[cfg(not(target_arch = "wasm32"))]
    use crate::timer_now;
    #[cfg(not(target_arch = "wasm32"))]
    use wgpu::util::DeviceExt;

    #[test]
    fn projected_draw_ticket_namespace_is_javascript_safe_and_wraps() {
        assert_eq!(
            next_projected_draw_ticket(FIRST_PROJECTED_DRAW_TICKET),
            FIRST_PROJECTED_DRAW_TICKET + 1
        );
        assert_eq!(
            next_projected_draw_ticket(MAX_JAVASCRIPT_SAFE_INTEGER),
            FIRST_PROJECTED_DRAW_TICKET
        );
        const {
            assert!(FIRST_PROJECTED_DRAW_TICKET <= MAX_JAVASCRIPT_SAFE_INTEGER);
        }
    }

    #[test]
    fn policy_receipts_reject_invalid_counts_and_completion_times() {
        use super::{projected_draw_measurement_is_valid, projected_draw_sample_is_isolated};

        let isolated = metadata(0, SurfaceProjectedDrawExecution::Candidate);
        assert!(projected_draw_sample_is_isolated(isolated));
        assert!(!projected_draw_sample_is_isolated(
            ProjectedDrawSampleMetadata {
                projection_rebuilt: false,
                ..isolated
            }
        ));
        assert!(!projected_draw_sample_is_isolated(
            ProjectedDrawSampleMetadata {
                order_refreshed: true,
                ..isolated
            }
        ));

        assert!(projected_draw_measurement_is_valid(
            SurfaceProjectedDrawExecution::Candidate,
            16.0,
            100,
            60,
            100,
        ));
        assert!(projected_draw_measurement_is_valid(
            SurfaceProjectedDrawExecution::Compact,
            16.0,
            100,
            60,
            60,
        ));
        assert!(!projected_draw_measurement_is_valid(
            SurfaceProjectedDrawExecution::Candidate,
            16.0,
            100,
            60,
            60,
        ));
        assert!(!projected_draw_measurement_is_valid(
            SurfaceProjectedDrawExecution::Compact,
            16.0,
            100,
            60,
            100,
        ));
        assert!(!projected_draw_measurement_is_valid(
            SurfaceProjectedDrawExecution::Compact,
            f32::NAN,
            100,
            60,
            60,
        ));
        assert!(!projected_draw_measurement_is_valid(
            SurfaceProjectedDrawExecution::Compact,
            16.0,
            59,
            60,
            60,
        ));
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
                    label: Some("projected-draw-telemetry-test-device"),
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
    fn indirect_args(device: &wgpu::Device, instance_count: u32) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("projected-draw-telemetry-test-indirect-args"),
            contents: bytemuck::cast_slice(&[6_u32, instance_count, 0, 0]),
            usage: wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC,
        })
    }

    fn metadata(
        camera_revision: u64,
        execution: SurfaceProjectedDrawExecution,
    ) -> ProjectedDrawSampleMetadata {
        ProjectedDrawSampleMetadata {
            camera_revision,
            execution,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            projection_generation: 7,
            probe_generation: 3,
            projection_rebuilt: true,
            order_refreshed: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn successful_sample_reports_distinct_v_c_d_and_execution() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping projected draw telemetry test; adapter unavailable");
            return;
        };
        let visible = indirect_args(&device, 257);
        let contributor = indirect_args(&device, 193);
        let drawn = indirect_args(&device, 193);
        let mut telemetry = ProjectedDrawTelemetry::new(&device);
        let reservation = telemetry
            .begin_sample(metadata(41, SurfaceProjectedDrawExecution::Compact))
            .expect("fresh projected draw telemetry slot");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-draw-telemetry-test"),
        });
        assert!(telemetry.encode_count_readback(
            &mut encoder,
            &reservation,
            ProjectedDrawCountSource::indirect_args(&visible),
            ProjectedDrawCountSource::indirect_args(&contributor),
            ProjectedDrawCountSource::indirect_args(&drawn),
        ));
        let command_buffer = encoder.finish();
        let issued = telemetry
            .arm(&command_buffer, reservation, timer_now())
            .expect("encoded reservation can be armed");
        queue.submit(Some(command_buffer));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for projected draw telemetry");

        let poll = telemetry.poll(&device);
        assert!(poll.failures.is_empty());
        assert_eq!(poll.completed.len(), 1);
        let sample = poll.completed[0];
        assert_eq!(sample.ticket, issued.ticket);
        assert_eq!(sample.camera_revision, 41);
        assert_eq!(
            (
                sample.visible_count,
                sample.contributor_count,
                sample.drawn_count
            ),
            (257, 193, 193)
        );
        assert_eq!(sample.execution, SurfaceProjectedDrawExecution::Compact);
        assert!(sample.exact_contributor_compaction);
        assert!(sample.frame_complete_ms.is_finite());
        assert!(sample.frame_complete_ms >= 0.0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn invalidation_fails_an_armed_ticket_once_but_silently_recycles_encoding() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping projected draw invalidation test; adapter unavailable");
            return;
        };
        let counts = indirect_args(&device, 11);
        let mut telemetry = ProjectedDrawTelemetry::new(&device);

        let cancelled = telemetry
            .begin_sample(metadata(41, SurfaceProjectedDrawExecution::Candidate))
            .expect("fresh cancellable reservation");
        assert!(telemetry.cancel(cancelled));
        let after_cancel = telemetry.poll(&device);
        assert!(after_cancel.completed.is_empty());
        assert!(after_cancel.failures.is_empty());

        let unexposed = telemetry
            .begin_sample(metadata(42, SurfaceProjectedDrawExecution::Candidate))
            .expect("fresh unexposed reservation");
        telemetry.invalidate_generation();
        assert!(!telemetry.cancel(unexposed));
        let silent = telemetry.poll(&device);
        assert!(silent.completed.is_empty());
        assert!(silent.failures.is_empty());

        let reservation = telemetry
            .begin_sample(metadata(43, SurfaceProjectedDrawExecution::Candidate))
            .expect("fresh exposed reservation");
        let slot = reservation.slot;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-draw-invalidation-test"),
        });
        assert!(telemetry.encode_count_readback(
            &mut encoder,
            &reservation,
            ProjectedDrawCountSource::raw(&counts, std::mem::size_of::<u32>() as u64),
            ProjectedDrawCountSource::indirect_args(&counts),
            ProjectedDrawCountSource::indirect_args(&counts),
        ));
        let command_buffer = encoder.finish();
        let issued = telemetry
            .arm(&command_buffer, reservation, timer_now())
            .expect("encoded reservation can be armed");
        telemetry.invalidate_generation();
        queue.submit(Some(command_buffer));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for invalidated projected draw telemetry");

        let first = telemetry.poll(&device);
        assert!(first.completed.is_empty());
        assert_eq!(first.failures.len(), 1);
        assert_eq!(first.failures[0].ticket, issued.ticket);
        assert_eq!(first.failures[0].camera_revision, 43);
        assert_eq!(
            first.failures[0].reason,
            SurfaceProjectedDrawMeasurementFailureReason::GenerationInvalidated
        );

        let second = telemetry.poll(&device);
        assert!(second.completed.is_empty());
        assert!(second.failures.is_empty());
        assert_eq!(
            telemetry.slots[slot]
                .state
                .load(std::sync::atomic::Ordering::Acquire),
            SLOT_IDLE
        );
    }
}
