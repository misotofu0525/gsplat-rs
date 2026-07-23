#[cfg(not(target_arch = "wasm32"))]
use gsplat_core::Vec3f;
use gsplat_core::{Camera, FrameStats};
#[cfg(not(target_arch = "wasm32"))]
use gsplat_sort::CpuSortBackend;
use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
};

pub use crate::api::SurfaceOrderBackendUsed;
use crate::evidence::BoundedEvidenceRing;
use crate::gpu_telemetry::{SurfaceCpuOrderMeasurement, TelemetrySubmission};
use crate::surface_presenter::{CpuCompletionSampleRequest, ProjectedDrawSampleRequest};
use crate::{
    GeometryPath, Renderer, RendererError, SurfaceGpuOrderProducer, SurfaceGpuProducerMeasurement,
    SurfaceGpuProducerMeasurementFailure, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfacePresenter, SurfacePresenterError, SurfaceProjectedDrawExecution,
    SurfaceProjectedDrawMeasurement, SurfaceProjectedDrawMeasurementFailure,
    SurfaceRasterExecutionPlan, SurfaceTimingSource, timer_elapsed_ms, timer_now,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{OwnedCpuOrderInput, SurfaceFrameCapture};

const DEFAULT_SURFACE_SORT_INTERVAL: u32 = 1;
/// Maximum number of camera revisions an asynchronously produced order may lag
/// behind the frame that consumes it. Older results are dropped; if the
/// displayed order reaches this bound before a fresh result is ready, the
/// session performs a synchronous refresh rather than allowing unbounded lag.
#[cfg(not(target_arch = "wasm32"))]
const MAX_ASYNC_SORT_REVISION_LAG: u64 = 2;
#[cfg(not(target_arch = "wasm32"))]
const MAX_ASYNC_SORT_ROTATION_DELTA_RADIANS: f32 = 0.01;
#[cfg(not(target_arch = "wasm32"))]
const MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION: f32 = 0.02;
const ADAPTIVE_CPU_BOOTSTRAP_SAMPLES: u32 = 6;
const ADAPTIVE_INITIAL_PROBE_DELAY: u32 = 4;
const ADAPTIVE_PROBE_SAMPLES_PER_BACKEND: u8 = 8;
const ADAPTIVE_PROBE_SEQUENCE_LEN: u8 = ADAPTIVE_PROBE_SAMPLES_PER_BACKEND * 2;
const ADAPTIVE_REPROBE_INTERVAL: u32 = 48;
const ADAPTIVE_GPU_PROMOTION_RATIO: f32 = 0.88;
const ADAPTIVE_CPU_PROMOTION_RATIO: f32 = 0.92;
const ADAPTIVE_GPU_FAILURE_COOLDOWN: u32 = 96;
const PROJECTED_COMPACT_PROMOTION_RATIO: f32 = 0.95;
const PROJECTED_CANDIDATE_PROMOTION_RATIO: f32 = 0.97;
const PROJECTED_TELEMETRY_FAILURE_COOLDOWN: u32 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceSortSchedule {
    Interval(u32),
    AsyncLatest { interval: u32 },
}

/// Selects where a required Direct order refresh is computed. This is
/// independent from [`SurfaceSortSchedule`], which decides *when* to refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceOrderBackend {
    #[default]
    Cpu,
    Gpu,
    Adaptive,
}

/// Selects the exact projected draw execution independently from CPU/GPU
/// ordering. Forced modes are deterministic experiment controls; Adaptive
/// keeps separate learned lanes for orders produced by each backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceProjectedDrawPolicy {
    Candidate,
    Compact,
    #[default]
    Adaptive,
}

fn validate_projected_draw_policy_transition(
    current: SurfaceProjectedDrawPolicy,
    next: SurfaceProjectedDrawPolicy,
    compact_available: bool,
) -> Result<bool, SurfacePresenterError> {
    if current == next {
        return Ok(false);
    }
    if next == SurfaceProjectedDrawPolicy::Compact && !compact_available {
        return Err(SurfacePresenterError::ProjectedCompactionUnsupported);
    }
    Ok(true)
}

fn validate_gpu_order_producer_transition(
    current: SurfaceGpuOrderProducer,
    next: SurfaceGpuOrderProducer,
    geometry_path: GeometryPath,
    raster_plan: SurfaceRasterExecutionPlan,
    projected_policy: SurfaceProjectedDrawPolicy,
) -> Result<bool, SurfacePresenterError> {
    if current == next {
        return Ok(false);
    }
    if next == SurfaceGpuOrderProducer::Preproject
        && (geometry_path != GeometryPath::PackedAtlas
            || raster_plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact
            || projected_policy != SurfaceProjectedDrawPolicy::Compact)
    {
        return Err(SurfacePresenterError::PreprojectProducerIncompatible);
    }
    Ok(true)
}

fn gpu_producer_measurement_context_is_valid(
    geometry_path: GeometryPath,
    raster_plan: SurfaceRasterExecutionPlan,
    projected_policy: SurfaceProjectedDrawPolicy,
) -> bool {
    geometry_path == GeometryPath::PackedAtlas
        && raster_plan == SurfaceRasterExecutionPlan::ProjectedQuadsExact
        && projected_policy == SurfaceProjectedDrawPolicy::Compact
}

/// Why a requested order measurement did not reserve a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceOrderMeasurementUnsampledReason {
    /// Every non-blocking telemetry slot was still owned by earlier work.
    RingBusy,
    /// The platform Surface did not provide a drawable for this frame.
    SurfaceUnavailable,
}

/// Exact submission identity for the optional order measurement on a frame.
/// Only `Issued` creates a ticket that must later receive one terminal success
/// or failure receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceOrderMeasurementSubmission {
    #[default]
    NotRequested,
    Issued {
        backend: SurfaceOrderBackendUsed,
        ticket: u64,
    },
    Unsampled {
        backend: SurfaceOrderBackendUsed,
        reason: SurfaceOrderMeasurementUnsampledReason,
    },
}

impl SurfaceOrderMeasurementSubmission {
    pub const fn backend(self) -> Option<SurfaceOrderBackendUsed> {
        match self {
            Self::NotRequested => None,
            Self::Issued { backend, .. } | Self::Unsampled { backend, .. } => Some(backend),
        }
    }

    pub const fn ticket(self) -> Option<u64> {
        match self {
            Self::Issued { ticket, .. } => Some(ticket),
            Self::NotRequested | Self::Unsampled { .. } => None,
        }
    }

    fn from_presenter(backend: SurfaceOrderBackendUsed, submission: TelemetrySubmission) -> Self {
        match submission {
            TelemetrySubmission::NotRequested => Self::NotRequested,
            TelemetrySubmission::Issued(ticket) => Self::Issued { backend, ticket },
            TelemetrySubmission::RingBusy => Self::Unsampled {
                backend,
                reason: SurfaceOrderMeasurementUnsampledReason::RingBusy,
            },
            TelemetrySubmission::SurfaceUnavailable => Self::Unsampled {
                backend,
                reason: SurfaceOrderMeasurementUnsampledReason::SurfaceUnavailable,
            },
            TelemetrySubmission::GpuOrderPreparationPending => Self::NotRequested,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceProjectedDrawMeasurementUnsampledReason {
    RingBusy,
    SurfaceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceProjectedDrawMeasurementSubmission {
    #[default]
    NotRequested,
    Issued {
        execution: SurfaceProjectedDrawExecution,
        ticket: u64,
    },
    Unsampled {
        execution: SurfaceProjectedDrawExecution,
        reason: SurfaceProjectedDrawMeasurementUnsampledReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceGpuProducerMeasurementUnsampledReason {
    RingBusy,
    SurfaceUnavailable,
}

/// Ticket identity for the independent Packed GPU-producer A/B receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceGpuProducerMeasurementSubmission {
    #[default]
    NotRequested,
    Issued {
        producer: SurfaceGpuOrderProducer,
        ticket: u64,
    },
    Unsampled {
        producer: SurfaceGpuOrderProducer,
        reason: SurfaceGpuProducerMeasurementUnsampledReason,
    },
}

impl SurfaceGpuProducerMeasurementSubmission {
    pub const fn ticket(self) -> Option<u64> {
        match self {
            Self::Issued { ticket, .. } => Some(ticket),
            Self::NotRequested | Self::Unsampled { .. } => None,
        }
    }

    fn from_presenter(
        producer: Option<SurfaceGpuOrderProducer>,
        submission: TelemetrySubmission,
    ) -> Self {
        let Some(producer) = producer else {
            return Self::NotRequested;
        };
        match submission {
            TelemetrySubmission::NotRequested | TelemetrySubmission::GpuOrderPreparationPending => {
                Self::NotRequested
            }
            TelemetrySubmission::Issued(ticket) => Self::Issued { producer, ticket },
            TelemetrySubmission::RingBusy => Self::Unsampled {
                producer,
                reason: SurfaceGpuProducerMeasurementUnsampledReason::RingBusy,
            },
            TelemetrySubmission::SurfaceUnavailable => Self::Unsampled {
                producer,
                reason: SurfaceGpuProducerMeasurementUnsampledReason::SurfaceUnavailable,
            },
        }
    }
}

impl SurfaceProjectedDrawMeasurementSubmission {
    pub const fn ticket(self) -> Option<u64> {
        match self {
            Self::Issued { ticket, .. } => Some(ticket),
            Self::NotRequested | Self::Unsampled { .. } => None,
        }
    }

    fn from_presenter(
        execution: SurfaceProjectedDrawExecution,
        submission: TelemetrySubmission,
    ) -> Self {
        match submission {
            TelemetrySubmission::NotRequested | TelemetrySubmission::GpuOrderPreparationPending => {
                Self::NotRequested
            }
            TelemetrySubmission::Issued(ticket) => Self::Issued { execution, ticket },
            TelemetrySubmission::RingBusy => Self::Unsampled {
                execution,
                reason: SurfaceProjectedDrawMeasurementUnsampledReason::RingBusy,
            },
            TelemetrySubmission::SurfaceUnavailable => Self::Unsampled {
                execution,
                reason: SurfaceProjectedDrawMeasurementUnsampledReason::SurfaceUnavailable,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceProjectedDrawAdaptivePendingSample {
    pub order_backend: SurfaceOrderBackendUsed,
    pub execution: SurfaceProjectedDrawExecution,
    pub ticket: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceProjectedDrawAdaptiveState {
    #[default]
    Disabled,
    CandidateLearning,
    CandidateStable,
    CompactProbe,
    CompactStable,
    CandidateProbe,
    CandidateOnly,
    Cooldown,
}

/// Formal Adaptive sample waiting for its terminal asynchronous order receipt
/// (GPU timestamp/readback, or queue completion on the fallback metric).
/// Benchmarks use this to keep pumping without losing the ticket identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceAdaptivePendingSample {
    pub backend: SurfaceOrderBackendUsed,
    pub ticket: u64,
}

/// Coarse policy state retained in benchmark telemetry. Thresholds are
/// exploration controls rather than fixed CPU/GPU crossover decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceAdaptiveState {
    #[default]
    Disabled,
    CpuLearning,
    CpuStable,
    GpuProbe,
    GpuStable,
    CpuProbe,
    Cooldown,
}

/// Structured reason why Adaptive rejected the GPU ordering path and entered
/// a CPU cooldown. This is separate from a policy choice: experiments can
/// distinguish "CPU measured faster" from "GPU ordering was unavailable".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAdaptiveGpuFailureReason {
    Unsupported,
    Initialization,
    OutOfMemory,
    Validation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdaptivePhase {
    CpuLearning {
        completed: u8,
    },
    Stable {
        incumbent: SurfaceOrderBackendUsed,
    },
    Probe {
        incumbent: SurfaceOrderBackendUsed,
        next_sample: u8,
        active_backend: SurfaceOrderBackendUsed,
    },
    Cooldown {
        remaining: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AdaptiveMetric {
    #[default]
    OrderOnly,
    FrameCompletion,
}

const fn adaptive_primary_metric() -> AdaptiveMetric {
    AdaptiveMetric::FrameCompletion
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdaptiveSampleKind {
    CpuBootstrap,
    Probe(u8),
    TransitionWarmup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdaptiveRefreshChoice {
    backend: SurfaceOrderBackendUsed,
    sample: Option<AdaptiveSampleKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdaptivePendingSample {
    backend: SurfaceOrderBackendUsed,
    ticket: u64,
    kind: AdaptiveSampleKind,
}

#[derive(Debug, Clone, Copy)]
struct RollingEstimate {
    samples: [f32; 16],
    len: usize,
    cursor: usize,
}

impl Default for RollingEstimate {
    fn default() -> Self {
        Self {
            samples: [0.0; 16],
            len: 0,
            cursor: 0,
        }
    }
}

impl RollingEstimate {
    fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
    }

    fn push(&mut self, sample_ms: f32) {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return;
        }
        self.samples[self.cursor] = sample_ms;
        self.cursor = (self.cursor + 1) % self.samples.len();
        self.len = self.len.saturating_add(1).min(self.samples.len());
    }

    fn p75(&self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.samples;
        sorted[..self.len].sort_by(f32::total_cmp);
        let index = (self.len - 1) * 3 / 4;
        Some(sorted[index])
    }
}

#[derive(Debug, Default)]
struct AdaptiveOrderPolicy {
    phase: Option<AdaptivePhase>,
    metric: AdaptiveMetric,
    cpu_baseline: RollingEstimate,
    probe_cpu: RollingEstimate,
    probe_gpu: RollingEstimate,
    pending: Option<AdaptivePendingSample>,
    refreshes_since_probe: u32,
    next_gpu_probe_after: u32,
}

impl AdaptiveOrderPolicy {
    fn reset(&mut self, metric: AdaptiveMetric) {
        *self = Self {
            phase: Some(AdaptivePhase::CpuLearning { completed: 0 }),
            metric,
            next_gpu_probe_after: ADAPTIVE_INITIAL_PROBE_DELAY,
            ..Self::default()
        };
    }

    fn state(&self) -> SurfaceAdaptiveState {
        match self.phase {
            None => SurfaceAdaptiveState::Disabled,
            Some(AdaptivePhase::CpuLearning { .. }) => SurfaceAdaptiveState::CpuLearning,
            Some(AdaptivePhase::Stable {
                incumbent: SurfaceOrderBackendUsed::Cpu,
            }) => SurfaceAdaptiveState::CpuStable,
            Some(AdaptivePhase::Stable {
                incumbent: SurfaceOrderBackendUsed::Gpu,
            }) => SurfaceAdaptiveState::GpuStable,
            Some(AdaptivePhase::Probe {
                incumbent: SurfaceOrderBackendUsed::Cpu,
                ..
            }) => SurfaceAdaptiveState::GpuProbe,
            Some(AdaptivePhase::Probe {
                incumbent: SurfaceOrderBackendUsed::Gpu,
                ..
            }) => SurfaceAdaptiveState::CpuProbe,
            Some(AdaptivePhase::Cooldown { .. }) => SurfaceAdaptiveState::Cooldown,
        }
    }

    fn metric(&self) -> AdaptiveMetric {
        self.metric
    }

    fn choose_refresh_backend(&mut self) -> AdaptiveRefreshChoice {
        let phase = self
            .phase
            .get_or_insert(AdaptivePhase::CpuLearning { completed: 0 });
        match *phase {
            AdaptivePhase::CpuLearning { .. } => AdaptiveRefreshChoice {
                backend: SurfaceOrderBackendUsed::Cpu,
                sample: self
                    .pending
                    .is_none()
                    .then_some(AdaptiveSampleKind::CpuBootstrap),
            },
            AdaptivePhase::Stable { incumbent } => {
                if self.refreshes_since_probe < self.next_gpu_probe_after {
                    self.refreshes_since_probe = self.refreshes_since_probe.saturating_add(1);
                    return AdaptiveRefreshChoice {
                        backend: incumbent,
                        sample: None,
                    };
                }
                self.probe_cpu.clear();
                self.probe_gpu.clear();
                *phase = AdaptivePhase::Probe {
                    incumbent,
                    next_sample: 0,
                    active_backend: incumbent,
                };
                self.choose_refresh_backend()
            }
            AdaptivePhase::Probe {
                incumbent,
                next_sample,
                active_backend,
            } => {
                let target_backend = probe_sequence_backend(incumbent, next_sample);
                if self.pending.is_some() {
                    // A timestamp/readback receipt can arrive many presented
                    // frames after the sampled command. Keep serving the
                    // incumbent while that one formal sample is pending so
                    // telemetry latency does not turn a probe into a long
                    // residency on the challenger.
                    AdaptiveRefreshChoice {
                        backend: incumbent,
                        sample: None,
                    }
                } else if active_backend != target_backend {
                    AdaptiveRefreshChoice {
                        backend: target_backend,
                        sample: Some(AdaptiveSampleKind::TransitionWarmup),
                    }
                } else {
                    debug_assert!(next_sample < ADAPTIVE_PROBE_SEQUENCE_LEN);
                    AdaptiveRefreshChoice {
                        backend: target_backend,
                        sample: Some(AdaptiveSampleKind::Probe(next_sample)),
                    }
                }
            }
            AdaptivePhase::Cooldown { mut remaining } => {
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    self.refreshes_since_probe = 0;
                    self.next_gpu_probe_after = 0;
                    *phase = AdaptivePhase::Stable {
                        incumbent: SurfaceOrderBackendUsed::Cpu,
                    };
                } else {
                    *phase = AdaptivePhase::Cooldown { remaining };
                }
                AdaptiveRefreshChoice {
                    backend: SurfaceOrderBackendUsed::Cpu,
                    sample: None,
                }
            }
        }
    }

    fn held_refresh_choice(&self) -> AdaptiveRefreshChoice {
        let backend = match self.phase {
            None
            | Some(AdaptivePhase::CpuLearning { .. })
            | Some(AdaptivePhase::Cooldown { .. }) => SurfaceOrderBackendUsed::Cpu,
            Some(AdaptivePhase::Stable { incumbent })
            | Some(AdaptivePhase::Probe { incumbent, .. }) => incumbent,
        };
        AdaptiveRefreshChoice {
            backend,
            sample: None,
        }
    }

    fn cohort_active(&self) -> bool {
        self.pending.is_some()
            || matches!(
                self.phase,
                Some(AdaptivePhase::CpuLearning { .. } | AdaptivePhase::Probe { .. })
            )
    }

    fn complete_synchronous_sample(&mut self, choice: AdaptiveRefreshChoice, sample_ms: f32) {
        let Some(kind) = choice.sample else {
            return;
        };
        self.complete_sample(choice.backend, kind, sample_ms);
    }

    fn register_pending_sample(&mut self, choice: AdaptiveRefreshChoice, ticket: u64) {
        let Some(kind) = choice.sample else {
            return;
        };
        debug_assert!(self.pending.is_none());
        self.pending = Some(AdaptivePendingSample {
            backend: choice.backend,
            ticket,
            kind,
        });
    }

    fn complete_pending_sample(
        &mut self,
        backend: SurfaceOrderBackendUsed,
        ticket: u64,
        sample_ms: f32,
    ) -> bool {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return false;
        }
        let Some(pending) = self.pending else {
            return false;
        };
        if pending.backend != backend || pending.ticket != ticket {
            return false;
        }
        self.pending = None;
        self.complete_sample(backend, pending.kind, sample_ms);
        true
    }

    fn pending_sample(&self) -> Option<SurfaceAdaptivePendingSample> {
        self.pending.map(|pending| SurfaceAdaptivePendingSample {
            backend: pending.backend,
            ticket: pending.ticket,
        })
    }

    fn observe_cpu_measurement_failure(&mut self, failure: SurfaceOrderMeasurementFailure) -> bool {
        let matches_pending_cpu_ticket = self.pending.is_some_and(|pending| {
            pending.backend == SurfaceOrderBackendUsed::Cpu && pending.ticket == failure.ticket
        });
        if matches_pending_cpu_ticket {
            // A generation change invalidates only the measurement evidence,
            // not the CPU sorter. Leave the phase intact so the next exact
            // refresh retries the same formal sample.
            self.pending = None;
        }
        matches_pending_cpu_ticket
    }

    /// Applies asynchronous GPU evidence only when it belongs to the formal
    /// probe sample currently awaited by the policy. Warmup and steady-state
    /// GPU draws still publish telemetry for diagnostics, but an incomplete
    /// interval from either must not change the adaptive metric source or
    /// restart learning.
    fn observe_gpu_measurement(&mut self, measurement: SurfaceOrderMeasurement) {
        let Some(pending) = self.pending else {
            return;
        };
        if pending.backend != SurfaceOrderBackendUsed::Gpu || pending.ticket != measurement.ticket {
            return;
        }

        if pending.kind == AdaptiveSampleKind::TransitionWarmup {
            self.complete_pending_sample(
                SurfaceOrderBackendUsed::Gpu,
                measurement.ticket,
                measurement.gpu_complete_ms,
            );
            return;
        }
        if !matches!(pending.kind, AdaptiveSampleKind::Probe(_)) {
            return;
        }

        match self.metric {
            AdaptiveMetric::OrderOnly => {
                if measurement.timing_source == SurfaceTimingSource::TimestampQuery
                    && let Some(order_ms) = measurement.gpu_order_ms
                {
                    self.complete_pending_sample(
                        SurfaceOrderBackendUsed::Gpu,
                        measurement.ticket,
                        order_ms,
                    );
                    return;
                }
                // Only a matching formal probe may establish that timestamp
                // evidence is unusable and move the ABBA comparison to paired
                // queue-completion timing.
                self.reset(AdaptiveMetric::FrameCompletion);
            }
            AdaptiveMetric::FrameCompletion => {
                self.complete_pending_sample(
                    SurfaceOrderBackendUsed::Gpu,
                    measurement.ticket,
                    measurement.gpu_complete_ms,
                );
            }
        }
    }

    fn observe_gpu_measurement_failure(&mut self, failure: SurfaceOrderMeasurementFailure) -> bool {
        let matches_pending_gpu_ticket = self.pending.is_some_and(|pending| {
            pending.backend == SurfaceOrderBackendUsed::Gpu && pending.ticket == failure.ticket
        });
        if matches_pending_gpu_ticket {
            self.gpu_failed();
        }
        matches_pending_gpu_ticket
    }

    fn complete_sample(
        &mut self,
        backend: SurfaceOrderBackendUsed,
        kind: AdaptiveSampleKind,
        sample_ms: f32,
    ) {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return;
        }
        match kind {
            AdaptiveSampleKind::CpuBootstrap => {
                self.cpu_baseline.push(sample_ms);
                if let Some(AdaptivePhase::CpuLearning { completed }) = &mut self.phase {
                    *completed = completed.saturating_add(1);
                    if u32::from(*completed) >= ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
                        self.phase = Some(AdaptivePhase::Stable {
                            incumbent: SurfaceOrderBackendUsed::Cpu,
                        });
                        self.refreshes_since_probe = 0;
                    }
                }
            }
            AdaptiveSampleKind::Probe(sample_index) => {
                match backend {
                    SurfaceOrderBackendUsed::Cpu => self.probe_cpu.push(sample_ms),
                    SurfaceOrderBackendUsed::Gpu => self.probe_gpu.push(sample_ms),
                }
                let Some(AdaptivePhase::Probe {
                    incumbent,
                    next_sample,
                    ..
                }) = &mut self.phase
                else {
                    return;
                };
                if sample_index != *next_sample {
                    return;
                }
                *next_sample = next_sample.saturating_add(1);
                if *next_sample >= ADAPTIVE_PROBE_SEQUENCE_LEN {
                    let incumbent = *incumbent;
                    self.finish_probe(incumbent);
                }
            }
            AdaptiveSampleKind::TransitionWarmup => {
                if let Some(AdaptivePhase::Probe { active_backend, .. }) = &mut self.phase {
                    *active_backend = backend;
                }
            }
        }
    }

    fn finish_probe(&mut self, incumbent: SurfaceOrderBackendUsed) {
        let winner = match (self.probe_cpu.p75(), self.probe_gpu.p75()) {
            (Some(cpu), Some(gpu)) => match incumbent {
                SurfaceOrderBackendUsed::Cpu if gpu < cpu * ADAPTIVE_GPU_PROMOTION_RATIO => {
                    SurfaceOrderBackendUsed::Gpu
                }
                SurfaceOrderBackendUsed::Gpu if cpu < gpu * ADAPTIVE_CPU_PROMOTION_RATIO => {
                    SurfaceOrderBackendUsed::Cpu
                }
                _ => incumbent,
            },
            _ => incumbent,
        };
        self.next_gpu_probe_after =
            if winner != incumbent || self.next_gpu_probe_after < ADAPTIVE_REPROBE_INTERVAL {
                ADAPTIVE_REPROBE_INTERVAL
            } else {
                self.next_gpu_probe_after.saturating_mul(2).min(384)
            };
        self.phase = Some(AdaptivePhase::Stable { incumbent: winner });
        self.refreshes_since_probe = 0;
        self.probe_cpu.clear();
        self.probe_gpu.clear();
    }

    fn gpu_failed(&mut self) {
        self.phase = Some(AdaptivePhase::Cooldown {
            remaining: ADAPTIVE_GPU_FAILURE_COOLDOWN,
        });
        self.refreshes_since_probe = 0;
        self.next_gpu_probe_after = ADAPTIVE_GPU_FAILURE_COOLDOWN;
        self.pending = None;
        self.probe_cpu.clear();
        self.probe_gpu.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectedAdaptivePhase {
    CandidateLearning {
        completed: u8,
    },
    Stable {
        incumbent: SurfaceProjectedDrawExecution,
    },
    Probe {
        incumbent: SurfaceProjectedDrawExecution,
        next_sample: u8,
        active_execution: SurfaceProjectedDrawExecution,
    },
    CandidateOnly,
    Cooldown {
        incumbent: SurfaceProjectedDrawExecution,
        remaining: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectedAdaptiveSampleKind {
    CandidateBootstrap,
    Probe(u8),
    TransitionWarmup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectedAdaptiveChoice {
    execution: SurfaceProjectedDrawExecution,
    sample: Option<ProjectedAdaptiveSampleKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectedAdaptivePendingSample {
    order_backend: SurfaceOrderBackendUsed,
    execution: SurfaceProjectedDrawExecution,
    ticket: u64,
    kind: ProjectedAdaptiveSampleKind,
}

#[derive(Debug)]
struct AdaptiveProjectedDrawPolicy {
    phase: ProjectedAdaptivePhase,
    candidate_baseline: RollingEstimate,
    probe_candidate: RollingEstimate,
    probe_compact: RollingEstimate,
    pending: Option<ProjectedAdaptivePendingSample>,
    frames_since_probe: u32,
    next_probe_after: u32,
    initial_selection_complete: bool,
    incumbent_changed: bool,
}

impl Default for AdaptiveProjectedDrawPolicy {
    fn default() -> Self {
        Self {
            phase: ProjectedAdaptivePhase::CandidateLearning { completed: 0 },
            candidate_baseline: RollingEstimate::default(),
            probe_candidate: RollingEstimate::default(),
            probe_compact: RollingEstimate::default(),
            pending: None,
            frames_since_probe: 0,
            next_probe_after: ADAPTIVE_INITIAL_PROBE_DELAY,
            initial_selection_complete: false,
            incumbent_changed: false,
        }
    }
}

impl AdaptiveProjectedDrawPolicy {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Stops an in-flight formal sample without erasing stable evidence. The
    /// eventual terminal telemetry receipt is still exposed to callers, but
    /// cannot mutate policy while a deterministic forced mode is selected.
    /// Returning to Adaptive repeats the interrupted phase from the same
    /// sample index.
    fn suspend_learning(&mut self) {
        self.pending = None;
        // This is an unconsumed owner-boundary notification, not historical
        // timing evidence. Carrying it across an order-backend or forced draw
        // transition could reset a newly learned order cohort later.
        self.incumbent_changed = false;
    }

    fn state(&self) -> SurfaceProjectedDrawAdaptiveState {
        match self.phase {
            ProjectedAdaptivePhase::CandidateLearning { .. } => {
                SurfaceProjectedDrawAdaptiveState::CandidateLearning
            }
            ProjectedAdaptivePhase::Stable {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
            } => SurfaceProjectedDrawAdaptiveState::CandidateStable,
            ProjectedAdaptivePhase::Stable {
                incumbent: SurfaceProjectedDrawExecution::Compact,
            } => SurfaceProjectedDrawAdaptiveState::CompactStable,
            ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                ..
            } => SurfaceProjectedDrawAdaptiveState::CompactProbe,
            ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Compact,
                ..
            } => SurfaceProjectedDrawAdaptiveState::CandidateProbe,
            ProjectedAdaptivePhase::CandidateOnly => {
                SurfaceProjectedDrawAdaptiveState::CandidateOnly
            }
            ProjectedAdaptivePhase::Cooldown { .. } => SurfaceProjectedDrawAdaptiveState::Cooldown,
        }
    }

    fn choose(&mut self, compact_available: bool) -> ProjectedAdaptiveChoice {
        if !compact_available {
            self.phase = ProjectedAdaptivePhase::CandidateOnly;
            self.pending = None;
            self.initial_selection_complete = true;
            return self.held_choice();
        }
        match self.phase {
            ProjectedAdaptivePhase::CandidateLearning { .. } => ProjectedAdaptiveChoice {
                execution: SurfaceProjectedDrawExecution::Candidate,
                sample: self
                    .pending
                    .is_none()
                    .then_some(ProjectedAdaptiveSampleKind::CandidateBootstrap),
            },
            ProjectedAdaptivePhase::Stable { incumbent } => {
                if self.frames_since_probe < self.next_probe_after {
                    self.frames_since_probe = self.frames_since_probe.saturating_add(1);
                    return ProjectedAdaptiveChoice {
                        execution: incumbent,
                        sample: None,
                    };
                }
                self.probe_candidate.clear();
                self.probe_compact.clear();
                self.phase = ProjectedAdaptivePhase::Probe {
                    incumbent,
                    next_sample: 0,
                    active_execution: incumbent,
                };
                self.choose(compact_available)
            }
            ProjectedAdaptivePhase::Probe {
                incumbent,
                next_sample,
                active_execution,
            } => {
                let target = projected_probe_sequence_execution(incumbent, next_sample);
                if self.pending.is_some() {
                    ProjectedAdaptiveChoice {
                        execution: incumbent,
                        sample: None,
                    }
                } else if active_execution != target {
                    ProjectedAdaptiveChoice {
                        execution: target,
                        sample: Some(ProjectedAdaptiveSampleKind::TransitionWarmup),
                    }
                } else {
                    ProjectedAdaptiveChoice {
                        execution: target,
                        sample: Some(ProjectedAdaptiveSampleKind::Probe(next_sample)),
                    }
                }
            }
            ProjectedAdaptivePhase::CandidateOnly => ProjectedAdaptiveChoice {
                execution: SurfaceProjectedDrawExecution::Candidate,
                sample: None,
            },
            ProjectedAdaptivePhase::Cooldown {
                incumbent,
                mut remaining,
            } => {
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    self.phase = ProjectedAdaptivePhase::Stable { incumbent };
                    self.frames_since_probe = 0;
                    self.next_probe_after = self
                        .next_probe_after
                        .max(PROJECTED_TELEMETRY_FAILURE_COOLDOWN);
                } else {
                    self.phase = ProjectedAdaptivePhase::Cooldown {
                        incumbent,
                        remaining,
                    };
                }
                ProjectedAdaptiveChoice {
                    execution: incumbent,
                    sample: None,
                }
            }
        }
    }

    fn held_choice(&self) -> ProjectedAdaptiveChoice {
        let execution = match self.phase {
            ProjectedAdaptivePhase::CandidateLearning { .. }
            | ProjectedAdaptivePhase::CandidateOnly => SurfaceProjectedDrawExecution::Candidate,
            ProjectedAdaptivePhase::Stable { incumbent }
            | ProjectedAdaptivePhase::Probe { incumbent, .. }
            | ProjectedAdaptivePhase::Cooldown { incumbent, .. } => incumbent,
        };
        ProjectedAdaptiveChoice {
            execution,
            sample: None,
        }
    }

    fn cohort_active(&self) -> bool {
        self.pending.is_some()
            || !self.initial_selection_complete
            || matches!(
                self.phase,
                ProjectedAdaptivePhase::CandidateLearning { .. }
                    | ProjectedAdaptivePhase::Probe { .. }
            )
    }

    fn take_incumbent_changed(&mut self) -> bool {
        std::mem::take(&mut self.incumbent_changed)
    }

    fn complete_synchronous_sample(&mut self, choice: ProjectedAdaptiveChoice) {
        if choice.sample == Some(ProjectedAdaptiveSampleKind::TransitionWarmup)
            && let ProjectedAdaptivePhase::Probe {
                active_execution, ..
            } = &mut self.phase
        {
            *active_execution = choice.execution;
        }
    }

    fn register_pending_sample(
        &mut self,
        order_backend: SurfaceOrderBackendUsed,
        choice: ProjectedAdaptiveChoice,
        ticket: u64,
    ) {
        let Some(kind) = choice.sample else {
            return;
        };
        debug_assert_ne!(kind, ProjectedAdaptiveSampleKind::TransitionWarmup);
        debug_assert!(self.pending.is_none());
        // Adaptive is a live-workload policy, so one ABBA cohort may span
        // camera revisions. Each telemetry ticket still records its exact
        // camera revision for diagnostics. The admission boundary is instead
        // strict about order identity: a frame that refreshes, uploads, or
        // applies a different order is deferred before a ticket is reserved.
        self.pending = Some(ProjectedAdaptivePendingSample {
            order_backend,
            execution: choice.execution,
            ticket,
            kind,
        });
    }

    fn observe_measurement(&mut self, measurement: SurfaceProjectedDrawMeasurement) -> bool {
        let Some(pending) = self.pending else {
            return false;
        };
        if pending.ticket != measurement.ticket
            || pending.order_backend != measurement.order_backend
            || pending.execution != measurement.execution
        {
            return false;
        }
        self.pending = None;
        self.complete_sample(
            pending.kind,
            pending.execution,
            measurement.frame_complete_ms,
        );
        true
    }

    fn observe_failure(&mut self, failure: SurfaceProjectedDrawMeasurementFailure) -> bool {
        let matches = self.pending.is_some_and(|pending| {
            pending.ticket == failure.ticket
                && pending.order_backend == failure.order_backend
                && pending.execution == failure.execution
        });
        if matches {
            self.pending = None;
            // A broken measurement path must not hold the order arbiter for
            // an entire cooldown. Candidate remains the safe incumbent and a
            // later reprobe can refine it without blocking useful work.
            self.initial_selection_complete = true;
            if failure.execution == SurfaceProjectedDrawExecution::Compact
                && failure.reason
                    == crate::SurfaceProjectedDrawMeasurementFailureReason::InvariantViolation
            {
                self.phase = ProjectedAdaptivePhase::CandidateOnly;
            } else {
                let incumbent = match self.phase {
                    ProjectedAdaptivePhase::Stable { incumbent }
                    | ProjectedAdaptivePhase::Probe { incumbent, .. }
                    | ProjectedAdaptivePhase::Cooldown { incumbent, .. } => incumbent,
                    ProjectedAdaptivePhase::CandidateLearning { .. }
                    | ProjectedAdaptivePhase::CandidateOnly => {
                        SurfaceProjectedDrawExecution::Candidate
                    }
                };
                self.phase = ProjectedAdaptivePhase::Cooldown {
                    incumbent,
                    remaining: PROJECTED_TELEMETRY_FAILURE_COOLDOWN,
                };
            }
        }
        matches
    }

    fn pending_sample(&self) -> Option<SurfaceProjectedDrawAdaptivePendingSample> {
        self.pending
            .map(|pending| SurfaceProjectedDrawAdaptivePendingSample {
                order_backend: pending.order_backend,
                execution: pending.execution,
                ticket: pending.ticket,
            })
    }

    fn complete_sample(
        &mut self,
        kind: ProjectedAdaptiveSampleKind,
        execution: SurfaceProjectedDrawExecution,
        sample_ms: f32,
    ) {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return;
        }
        match kind {
            ProjectedAdaptiveSampleKind::CandidateBootstrap => {
                self.candidate_baseline.push(sample_ms);
                if let ProjectedAdaptivePhase::CandidateLearning { completed } = &mut self.phase {
                    *completed = completed.saturating_add(1);
                    if u32::from(*completed) >= ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
                        self.phase = ProjectedAdaptivePhase::Stable {
                            incumbent: SurfaceProjectedDrawExecution::Candidate,
                        };
                        self.frames_since_probe = 0;
                    }
                }
            }
            ProjectedAdaptiveSampleKind::Probe(sample_index) => {
                match execution {
                    SurfaceProjectedDrawExecution::Candidate => {
                        self.probe_candidate.push(sample_ms)
                    }
                    SurfaceProjectedDrawExecution::Compact => self.probe_compact.push(sample_ms),
                }
                let ProjectedAdaptivePhase::Probe {
                    incumbent,
                    next_sample,
                    ..
                } = &mut self.phase
                else {
                    return;
                };
                if sample_index != *next_sample {
                    return;
                }
                *next_sample = next_sample.saturating_add(1);
                if *next_sample >= ADAPTIVE_PROBE_SEQUENCE_LEN {
                    let incumbent = *incumbent;
                    self.finish_probe(incumbent);
                }
            }
            ProjectedAdaptiveSampleKind::TransitionWarmup => unreachable!(),
        }
    }

    fn finish_probe(&mut self, incumbent: SurfaceProjectedDrawExecution) {
        let winner = match (self.probe_candidate.p75(), self.probe_compact.p75()) {
            (Some(candidate), Some(compact)) => match incumbent {
                SurfaceProjectedDrawExecution::Candidate
                    if compact < candidate * PROJECTED_COMPACT_PROMOTION_RATIO =>
                {
                    SurfaceProjectedDrawExecution::Compact
                }
                SurfaceProjectedDrawExecution::Compact
                    if candidate < compact * PROJECTED_CANDIDATE_PROMOTION_RATIO =>
                {
                    SurfaceProjectedDrawExecution::Candidate
                }
                _ => incumbent,
            },
            _ => incumbent,
        };
        self.incumbent_changed |= winner != incumbent;
        self.initial_selection_complete = true;
        self.next_probe_after =
            if winner != incumbent || self.next_probe_after < ADAPTIVE_REPROBE_INTERVAL {
                ADAPTIVE_REPROBE_INTERVAL
            } else {
                self.next_probe_after.saturating_mul(2).min(384)
            };
        self.phase = ProjectedAdaptivePhase::Stable { incumbent: winner };
        self.frames_since_probe = 0;
        self.probe_candidate.clear();
        self.probe_compact.clear();
    }
}

fn projected_probe_sequence_execution(
    incumbent: SurfaceProjectedDrawExecution,
    sample_index: u8,
) -> SurfaceProjectedDrawExecution {
    let challenger = match incumbent {
        SurfaceProjectedDrawExecution::Candidate => SurfaceProjectedDrawExecution::Compact,
        SurfaceProjectedDrawExecution::Compact => SurfaceProjectedDrawExecution::Candidate,
    };
    match sample_index % 4 {
        0 | 3 => incumbent,
        1 | 2 => challenger,
        _ => unreachable!(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdaptiveProbeOwner {
    Order,
    ProjectedCpu,
    ProjectedGpu,
}

fn reset_adaptive_for_gpu_producer_measurement_transition(
    adaptive_policy: &mut AdaptiveOrderPolicy,
    projected_cpu: &mut AdaptiveProjectedDrawPolicy,
    projected_gpu: &mut AdaptiveProjectedDrawPolicy,
    owner: &mut Option<AdaptiveProbeOwner>,
    blocked_order_choice: &mut Option<AdaptiveRefreshChoice>,
) {
    adaptive_policy.reset(adaptive_primary_metric());
    projected_cpu.suspend_learning();
    projected_gpu.suspend_learning();
    *owner = None;
    *blocked_order_choice = None;
}

fn retain_gpu_producer_terminal<T>(queue: &mut VecDeque<T>, terminal: T) {
    queue.push_back(terminal);
}

fn projected_policy_can_sample(
    owner: Option<AdaptiveProbeOwner>,
    lane_owner: AdaptiveProbeOwner,
    order_emits_sample: bool,
) -> bool {
    !order_emits_sample
        && match owner {
            None => true,
            Some(active) => active == lane_owner,
        }
}

const fn arbitrate_new_probe_owner(
    current: Option<AdaptiveProbeOwner>,
    order_wants_formal_sample: bool,
    projected_wants_formal_sample: bool,
    projected_owner: AdaptiveProbeOwner,
) -> Option<AdaptiveProbeOwner> {
    match current {
        Some(owner) => Some(owner),
        None if projected_wants_formal_sample => Some(projected_owner),
        None if order_wants_formal_sample => Some(AdaptiveProbeOwner::Order),
        None => None,
    }
}

const fn projected_formal_sample_requested(
    choice: ProjectedAdaptiveChoice,
    order_changed: bool,
) -> bool {
    !order_changed
        && matches!(
            choice.sample,
            Some(
                ProjectedAdaptiveSampleKind::CandidateBootstrap
                    | ProjectedAdaptiveSampleKind::Probe(_)
            )
        )
}

const fn projected_order_changed(
    refresh_sort: bool,
    upload_order: bool,
    actual_sort_refreshed: bool,
) -> bool {
    refresh_sort || upload_order || actual_sort_refreshed
}

const fn gpu_projected_order_changed(refresh_sort: bool, actual_sort_refreshed: bool) -> bool {
    // `SurfaceFramePlan::upload_order` is the deferred CPU upload dirty bit.
    // A GPU-presented frame neither consumes nor changes that CPU order, so
    // carrying the bit into the GPU identity would permanently block formal
    // projected sampling while GPU remains selected.
    projected_order_changed(refresh_sort, false, actual_sort_refreshed)
}

const fn defer_projected_formal_choice(
    choice: ProjectedAdaptiveChoice,
    order_changed: bool,
) -> bool {
    order_changed
        && matches!(
            choice.sample,
            Some(
                ProjectedAdaptiveSampleKind::CandidateBootstrap
                    | ProjectedAdaptiveSampleKind::Probe(_)
            )
        )
}

const fn projected_probe_claims_owner(
    choice: ProjectedAdaptiveChoice,
    order_changed: bool,
    choice_was_pending: bool,
) -> bool {
    projected_formal_sample_requested(choice, order_changed)
        || (defer_projected_formal_choice(choice, order_changed) && !choice_was_pending)
}

const fn order_probe_owner_should_yield(
    owner: Option<AdaptiveProbeOwner>,
    order_pending: bool,
    refresh_sort: bool,
    order_wants_formal_sample: bool,
) -> bool {
    matches!(owner, Some(AdaptiveProbeOwner::Order))
        && !order_pending
        && !refresh_sort
        && !order_wants_formal_sample
}

const fn should_reset_order_for_projected_incumbent_change(
    order_backend: SurfaceOrderBackend,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    metric: AdaptiveMetric,
    order_pending: bool,
    projected_owner_finished: bool,
    projected_incumbent_changed: bool,
) -> bool {
    projected_owner_finished
        && projected_incumbent_changed
        && matches!(order_backend, SurfaceOrderBackend::Adaptive)
        && matches!(projected_draw_policy, SurfaceProjectedDrawPolicy::Adaptive)
        && matches!(metric, AdaptiveMetric::FrameCompletion)
        && !order_pending
}

/// Resets learned timings only when the raster workload actually changes.
/// Bindings commonly re-apply their current configuration; treating that as a
/// transition would discard valid Adaptive evidence and force needless CPU
/// bootstrap samples.
fn reset_adaptive_for_raster_transition(
    policy: &mut AdaptiveOrderPolicy,
    previous: SurfaceRasterExecutionPlan,
    next: SurfaceRasterExecutionPlan,
) -> bool {
    if previous == next {
        return false;
    }
    policy.reset(adaptive_primary_metric());
    true
}

fn probe_sequence_backend(
    incumbent: SurfaceOrderBackendUsed,
    sample_index: u8,
) -> SurfaceOrderBackendUsed {
    let challenger = match incumbent {
        SurfaceOrderBackendUsed::Cpu => SurfaceOrderBackendUsed::Gpu,
        SurfaceOrderBackendUsed::Gpu => SurfaceOrderBackendUsed::Cpu,
    };
    match sample_index % 4 {
        0 | 3 => incumbent,
        1 | 2 => challenger,
        _ => unreachable!(),
    }
}

impl SurfaceSortSchedule {
    pub const fn interval(self) -> u32 {
        match self {
            Self::Interval(interval) | Self::AsyncLatest { interval } => interval,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn async_schedule_threshold(sort_interval: u32) -> u32 {
    sort_interval.saturating_sub(1).max(1)
}

fn adaptive_gpu_order_failure_reason(
    error: &RendererError,
) -> Option<SurfaceAdaptiveGpuFailureReason> {
    match error {
        RendererError::SurfacePresenter(crate::SurfacePresenterError::GpuOrderUnsupported)
        | RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::StorageBindingCountUnsupported(_)
            | crate::ResidentGpuError::DispatchLimitExceeded,
        )) => Some(SurfaceAdaptiveGpuFailureReason::Unsupported),
        RendererError::SurfacePresenter(crate::SurfacePresenterError::DirectScene(
            crate::DirectSceneError::GpuOrderInitialization(_),
        ))
        | RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::GpuOrderInitialization(_),
        )) => Some(SurfaceAdaptiveGpuFailureReason::Initialization),
        RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::GpuOrderOutOfMemory(_),
        )) => Some(SurfaceAdaptiveGpuFailureReason::OutOfMemory),
        RendererError::SurfacePresenter(crate::SurfacePresenterError::ResidentGpu(
            crate::ResidentGpuError::GpuOrderValidation(_),
        )) => Some(SurfaceAdaptiveGpuFailureReason::Validation),
        _ => None,
    }
}

fn should_measure_cpu_refresh(
    plan: SurfaceFramePlan,
    requested_backend: SurfaceOrderBackendUsed,
    geometry_path: GeometryPath,
) -> bool {
    plan.refresh_sort
        && requested_backend == SurfaceOrderBackendUsed::Cpu
        && geometry_path != GeometryPath::PagedActiveAtlas
}

#[cfg(not(target_arch = "wasm32"))]
struct SurfaceAsyncSorter {
    request_tx: SyncSender<Option<(Camera, u64)>>,
    result_rx: Receiver<Result<AsyncSortResult, RendererError>>,
    worker: Option<JoinHandle<()>>,
    in_flight: bool,
}

#[cfg(not(target_arch = "wasm32"))]
struct AsyncSortResult {
    indices: Vec<u32>,
    preprocess_ms: f32,
    sort_ms: f32,
    camera_revision: u64,
    camera: Camera,
}

#[cfg(not(target_arch = "wasm32"))]
impl SurfaceAsyncSorter {
    fn new(renderer: &Renderer) -> Result<Self, RendererError> {
        let positions: Arc<[Vec3f]> = if let Some(scene) = renderer.resident_scene() {
            Arc::clone(&scene.positions)
        } else {
            let scene = renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
            Arc::from(scene.positions.clone().into_boxed_slice())
        };
        let (request_tx, request_rx) = sync_channel::<Option<(Camera, u64)>>(1);
        let (result_tx, result_rx) = sync_channel(1);
        let worker = thread::spawn(move || {
            let mut positions = positions;
            while let Ok(request) = request_rx.recv() {
                let Some((camera, camera_revision)) = request else {
                    break;
                };
                let input = OwnedCpuOrderInput::new(positions, camera);
                let result = sort_positions_for_camera(&input, camera_revision);
                positions = input.into_positions();
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            request_tx,
            result_rx,
            worker: Some(worker),
            in_flight: false,
        })
    }

    fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    fn poll_result(&mut self) -> Option<Result<AsyncSortResult, RendererError>> {
        if !self.in_flight {
            return None;
        }
        match self.result_rx.try_recv() {
            Ok(result) => {
                self.in_flight = false;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.in_flight = false;
                Some(Err(RendererError::SurfaceWorker))
            }
        }
    }

    fn start(&mut self, camera: Camera, camera_revision: u64) {
        if self.in_flight {
            return;
        }
        if self
            .request_tx
            .try_send(Some((camera, camera_revision)))
            .is_ok()
        {
            self.in_flight = true;
        }
    }

    fn drain(&mut self) {
        let _ = self.request_tx.send(None);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.in_flight = false;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SurfaceAsyncSorter {
    fn drop(&mut self) {
        self.drain();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFrameTimings {
    /// Retained for API compatibility. Direct rendering performs no CPU geometry expansion.
    pub cpu_geometry_ms: f32,
    /// CPU wall time spent updating GPU resources, encoding, submitting, and presenting.
    pub render_submit_ms: f32,
    /// End-to-end call wall time for the shared session frame.
    pub frame_wall_ms: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFrameOutput {
    /// CPU ordering phases are populated synchronously for CPU frames. GPU
    /// timing/count evidence is asynchronous and carries its own revision.
    pub stats: FrameStats,
    pub timings: SurfaceFrameTimings,
    /// True only when this call submitted the final raster/blit work and
    /// presented a drawable. GPU order preparation returns false while its
    /// hidden submission or asynchronous count readback is pending.
    pub frame_presented: bool,
    /// Generic retry state for GPU ordering preparation. Callers must retry
    /// the same camera revision after yielding to the event loop and must not
    /// count this call as a rendered frame or a measured order submission.
    pub gpu_order_preparation_pending: bool,
    /// Compatibility alias for `gpu_order_preparation_pending`. New callers
    /// should use the generic field because preparation is not tiled-only.
    ///
    /// This mirrors the generic value so older Web clients also retry the
    /// ProjectedQuadsExact lazy-pipeline preparation turn correctly.
    /// Callers must retry the same camera revision after yielding to the event
    /// loop and must not count this call as a rendered frame.
    pub tiled_preparation_pending: bool,
    pub raster_execution_plan: SurfaceRasterExecutionPlan,
    pub sort_refreshed: bool,
    pub order_uploaded: bool,
    /// Camera-revision lag of an async result observed on this frame.
    pub async_sort_revision_lag: Option<u32>,
    /// True when a completed async result exceeded the bounded-lag policy.
    pub stale_async_sort_dropped: bool,
    /// True when a new background sort was launched after this frame.
    pub async_sort_scheduled: bool,
    pub camera_revision: u64,
    pub applied_order_revision: u64,
    pub presented_order_revision_lag: u32,
    pub async_sort_scheduled_revision: Option<u64>,
    pub async_sort_completed_revision: Option<u64>,
    pub async_sort_result_applied: bool,
    pub sync_sort_fallback: bool,
    pub order_backend: SurfaceOrderBackendUsed,
    /// True when a requested GPU refresh failed and the same frame recovered
    /// through the deterministic CPU path.
    pub gpu_sort_fallback: bool,
    pub adaptive_state: SurfaceAdaptiveState,
    pub adaptive_gpu_failure: Option<SurfaceAdaptiveGpuFailureReason>,
    /// Independent exact projected draw strategy used by this frame.
    pub projected_draw_policy: SurfaceProjectedDrawPolicy,
    pub projected_draw_execution: SurfaceProjectedDrawExecution,
    pub projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState,
    pub projected_draw_measurement_submission: SurfaceProjectedDrawMeasurementSubmission,
    pub completed_projected_draw_measurement: Option<SurfaceProjectedDrawMeasurement>,
    pub completed_projected_draw_measurement_failure:
        Option<SurfaceProjectedDrawMeasurementFailure>,
    /// Actual Packed GPU producer used by this presented frame. CPU, Direct,
    /// Paged, and preparation-only calls report `None`.
    pub gpu_order_producer: Option<SurfaceGpuOrderProducer>,
    pub gpu_producer_measurement_submission: SurfaceGpuProducerMeasurementSubmission,
    pub completed_gpu_producer_measurement: Option<SurfaceGpuProducerMeasurement>,
    pub completed_gpu_producer_measurement_failure: Option<SurfaceGpuProducerMeasurementFailure>,
    /// Complete requested/issued/unsampled identity for this frame.
    pub order_measurement_submission: SurfaceOrderMeasurementSubmission,
    /// Measurement scheduled by this frame, if a ring slot was available.
    /// Kept as a compatibility mirror of
    /// [`SurfaceOrderMeasurementSubmission::ticket`].
    pub submitted_measurement_ticket: Option<u64>,
    /// Newest result harvested at the start of this frame. It may describe an
    /// earlier camera revision and must be joined by ticket/revision.
    pub completed_order_measurement: Option<SurfaceOrderMeasurement>,
    /// Newest terminal failure harvested at the start of this frame. Issued
    /// GPU tickets never disappear silently on readback/context invalidation.
    pub completed_order_measurement_failure: Option<SurfaceOrderMeasurementFailure>,
    pub visible_count_revision: Option<u64>,
    pub visible_count_pending: bool,
    pub gpu_timestamp_queries_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceFramePlan {
    refresh_sort: bool,
    upload_order: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceFrameState {
    camera_dirty: bool,
    camera_changed_this_frame: bool,
    force_sort: bool,
    order_upload_dirty: bool,
    camera_changes_since_sort: u32,
}

impl Default for SurfaceFrameState {
    fn default() -> Self {
        Self {
            camera_dirty: true,
            camera_changed_this_frame: true,
            force_sort: true,
            order_upload_dirty: true,
            camera_changes_since_sort: 0,
        }
    }
}

impl SurfaceFrameState {
    fn mark_camera_changed(&mut self) {
        self.camera_dirty = true;
        self.camera_changed_this_frame = true;
        self.camera_changes_since_sort = self.camera_changes_since_sort.saturating_add(1);
    }

    fn force_sort(&mut self) {
        self.force_sort = true;
        self.order_upload_dirty = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn mark_external_order(&mut self, camera_changes_since_sort: u32) {
        self.camera_dirty = camera_changes_since_sort > 0;
        self.camera_changed_this_frame = false;
        self.force_sort = false;
        self.order_upload_dirty = true;
        self.camera_changes_since_sort = camera_changes_since_sort;
    }

    fn plan(self, has_order: bool, sort_interval: u32) -> SurfaceFramePlan {
        let interval = sort_interval.max(1);
        let refresh_sort = self.force_sort
            || !has_order
            || (self.camera_dirty
                && (self.camera_changes_since_sort >= interval || !self.camera_changed_this_frame));
        SurfaceFramePlan {
            refresh_sort,
            upload_order: self.order_upload_dirty || refresh_sort,
        }
    }

    fn finish_frame(&mut self, plan: SurfaceFramePlan, order_uploaded: bool) {
        self.force_sort = false;
        self.camera_changed_this_frame = false;
        if plan.refresh_sort {
            self.camera_dirty = false;
            self.camera_changes_since_sort = 0;
        }
        if order_uploaded {
            self.order_upload_dirty = false;
        }
    }
}

/// Owns the ordering + direct GPU draw lifecycle shared by every Surface client.
///
/// PLY-derived scene attributes stay GPU-resident. CPU refreshes upload compact
/// source IDs, while GPU refreshes keep stable `(depth_key, source_id)` pairs on
/// the renderer device and draw their source IDs directly. The vertex shader
/// fetches and projects the corresponding Gaussian for Web, desktop, Android,
/// and iOS.
pub struct SurfaceRenderSession {
    renderer: Renderer,
    presenter: SurfacePresenter,
    camera: Camera,
    sort_interval: u32,
    order_backend: SurfaceOrderBackend,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    gpu_producer_measurement_enabled: bool,
    presented_order_backend: SurfaceOrderBackendUsed,
    gpu_order_initialized: bool,
    adaptive_policy: AdaptiveOrderPolicy,
    adaptive_projected_cpu: AdaptiveProjectedDrawPolicy,
    adaptive_projected_gpu: AdaptiveProjectedDrawPolicy,
    adaptive_probe_owner: Option<AdaptiveProbeOwner>,
    blocked_order_choice: Option<AdaptiveRefreshChoice>,
    adaptive_gpu_failure: Option<SurfaceAdaptiveGpuFailureReason>,
    pending_tiled_backend: Option<SurfaceOrderBackendUsed>,
    pending_tiled_adaptive_choice: Option<AdaptiveRefreshChoice>,
    pending_projected_choice: Option<ProjectedAdaptiveChoice>,
    camera_revision: u64,
    applied_order_revision: u64,
    applied_order_camera: Camera,
    #[cfg(not(target_arch = "wasm32"))]
    async_sort_translation_limit: f32,
    frame_state: SurfaceFrameState,
    last_stats: FrameStats,
    latest_cpu_order_measurement: Option<SurfaceCpuOrderMeasurement>,
    latest_gpu_order_measurement: Option<SurfaceOrderMeasurement>,
    completed_cpu_order_measurements: BoundedEvidenceRing<SurfaceCpuOrderMeasurement>,
    completed_order_measurements: BoundedEvidenceRing<SurfaceOrderMeasurement>,
    completed_order_measurement_failures: BoundedEvidenceRing<SurfaceOrderMeasurementFailure>,
    completed_projected_draw_measurements: BoundedEvidenceRing<SurfaceProjectedDrawMeasurement>,
    completed_projected_draw_measurement_failures:
        BoundedEvidenceRing<SurfaceProjectedDrawMeasurementFailure>,
    completed_gpu_producer_measurements: VecDeque<SurfaceGpuProducerMeasurement>,
    completed_gpu_producer_measurement_failures: VecDeque<SurfaceGpuProducerMeasurementFailure>,
    #[cfg(not(target_arch = "wasm32"))]
    async_sort_enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    async_sorter: Option<SurfaceAsyncSorter>,
}

fn try_switch_renderer_geometry_path<Error>(
    renderer: &mut Renderer,
    target: GeometryPath,
    prepare_presenter: impl FnOnce(&Renderer) -> Result<(), Error>,
) -> Result<bool, Error> {
    let previous = renderer.geometry_path();
    if previous == target {
        return Ok(false);
    }

    renderer.set_geometry_path(target);
    if let Err(error) = prepare_presenter(renderer) {
        renderer.set_geometry_path(previous);
        return Err(error);
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceGeometrySwitchEntry {
    AlreadyActive,
    Synchronous,
    AsyncPreparationRequired,
    Unsupported,
}

fn surface_geometry_switch_entry(
    web: bool,
    current: GeometryPath,
    target: GeometryPath,
) -> SurfaceGeometrySwitchEntry {
    if current == target {
        SurfaceGeometrySwitchEntry::AlreadyActive
    } else if web
        && (current == GeometryPath::PagedActiveAtlas || target == GeometryPath::PagedActiveAtlas)
    {
        SurfaceGeometrySwitchEntry::Unsupported
    } else if web {
        SurfaceGeometrySwitchEntry::AsyncPreparationRequired
    } else {
        SurfaceGeometrySwitchEntry::Synchronous
    }
}

impl SurfaceRenderSession {
    pub fn new(
        mut renderer: Renderer,
        presenter: SurfacePresenter,
        camera: Camera,
    ) -> Result<Self, RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if !renderer.has_scene() {
            return Err(RendererError::SceneNotLoaded);
        }
        if renderer.geometry_path() != presenter.geometry_path() {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let async_sort_translation_limit = {
            let positions = renderer.positions().ok_or(RendererError::SceneNotLoaded)?;
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for position in positions {
                min[0] = min[0].min(position.x);
                min[1] = min[1].min(position.y);
                min[2] = min[2].min(position.z);
                max[0] = max[0].max(position.x);
                max[1] = max[1].max(position.y);
                max[2] = max[2].max(position.z);
            }
            let diagonal =
                ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2))
                    .sqrt();
            (diagonal * MAX_ASYNC_SORT_TRANSLATION_DIAGONAL_FRACTION).max(1e-4)
        };
        // Prepare the only eagerly allocated session-owned collection before
        // staging release. Nothing after the handoff below allocates or can
        // fail before the completed session value is returned.
        let completed_order_measurements = BoundedEvidenceRing::new();
        let completed_cpu_order_measurements = BoundedEvidenceRing::new();
        let completed_order_measurement_failures = BoundedEvidenceRing::new();
        let completed_projected_draw_measurements = BoundedEvidenceRing::new();
        let completed_projected_draw_measurement_failures = BoundedEvidenceRing::new();
        let completed_gpu_producer_measurements = VecDeque::with_capacity(64);
        let completed_gpu_producer_measurement_failures = VecDeque::with_capacity(64);

        // All fallible session validation is complete and the presenter
        // already owns its durable GPU scene. Packed upload planes can now be
        // discarded without affecting exact positions or either sort backend.
        // This is deliberately the final fallible operation before ownership
        // moves into the live session.
        renderer.finish_surface_upload_handoff(presenter.geometry_path())?;
        Ok(Self {
            renderer,
            presenter,
            camera,
            sort_interval: DEFAULT_SURFACE_SORT_INTERVAL,
            order_backend: SurfaceOrderBackend::Cpu,
            projected_draw_policy: SurfaceProjectedDrawPolicy::Adaptive,
            gpu_producer_measurement_enabled: false,
            presented_order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_order_initialized: false,
            adaptive_policy: AdaptiveOrderPolicy::default(),
            adaptive_projected_cpu: AdaptiveProjectedDrawPolicy::default(),
            adaptive_projected_gpu: AdaptiveProjectedDrawPolicy::default(),
            adaptive_probe_owner: None,
            blocked_order_choice: None,
            adaptive_gpu_failure: None,
            pending_tiled_backend: None,
            pending_tiled_adaptive_choice: None,
            pending_projected_choice: None,
            camera_revision: 0,
            applied_order_revision: 0,
            applied_order_camera: camera,
            #[cfg(not(target_arch = "wasm32"))]
            async_sort_translation_limit,
            frame_state: SurfaceFrameState::default(),
            last_stats: FrameStats::zero(),
            latest_cpu_order_measurement: None,
            latest_gpu_order_measurement: None,
            completed_cpu_order_measurements,
            completed_order_measurements,
            completed_order_measurement_failures,
            completed_projected_draw_measurements,
            completed_projected_draw_measurement_failures,
            completed_gpu_producer_measurements,
            completed_gpu_producer_measurement_failures,
            #[cfg(not(target_arch = "wasm32"))]
            async_sort_enabled: false,
            #[cfg(not(target_arch = "wasm32"))]
            async_sorter: None,
        })
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Requests an exact readback of the next native Surface frame. This is a
    /// diagnostic operation: it may reconfigure the swapchain for `COPY_SRC`,
    /// but ordinary sessions never pay that cost unless explicitly requested.
    /// Resize and a second request fail while this capture remains armed. If a
    /// frame cannot be presented, retry rendering or call
    /// [`Self::cancel_surface_capture`] before resize/re-request.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_surface_capture(&mut self) -> Result<(), RendererError> {
        self.presenter.request_surface_capture()?;
        Ok(())
    }

    /// Cancels a requested native Surface capture and releases its readback
    /// buffer. This also discards a presented capture that was not taken.
    /// Returns false when no capture was armed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_surface_capture(&mut self) -> bool {
        self.presenter.cancel_surface_capture()
    }

    /// Blocks until the requested presented frame is readable and returns
    /// canonical RGBA8 bytes. Calling before a frame presents fails closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_surface_capture(&mut self) -> Result<SurfaceFrameCapture, RendererError> {
        Ok(self.presenter.take_surface_capture()?)
    }

    pub fn geometry_path(&self) -> GeometryPath {
        self.renderer.geometry_path()
    }

    pub fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        self.presenter.raster_execution_plan()
    }

    pub const fn projected_draw_policy(&self) -> SurfaceProjectedDrawPolicy {
        self.projected_draw_policy
    }

    pub const fn gpu_order_producer(&self) -> SurfaceGpuOrderProducer {
        self.presenter.gpu_order_producer()
    }

    /// Prepares a complete dormant producer graph without changing the
    /// selected producer or scheduling state. This is the browser-safe first
    /// half of the transactional A/B switch.
    pub async fn prepare_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        if producer == SurfaceGpuOrderProducer::Preproject
            && !gpu_producer_measurement_context_is_valid(
                self.geometry_path(),
                self.raster_execution_plan(),
                self.projected_draw_policy,
            )
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        self.presenter.prepare_gpu_order_producer(producer).await?;
        Ok(())
    }

    /// Transactionally selects the Packed GPU producer. The CPU lane and the
    /// outer CPU/GPU/Adaptive policy remain unchanged.
    pub fn set_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        if !validate_gpu_order_producer_transition(
            self.gpu_order_producer(),
            producer,
            self.geometry_path(),
            self.raster_execution_plan(),
            self.projected_draw_policy,
        )? {
            return Ok(());
        }
        self.presenter.set_gpu_order_producer(producer)?;
        self.finish_gpu_order_producer_transition();
        Ok(())
    }

    /// Browser-safe all-or-nothing producer switch. The complete candidate is
    /// scoped and published before the infallible selector/state transition.
    #[cfg(target_arch = "wasm32")]
    pub async fn set_gpu_order_producer_async(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        if !validate_gpu_order_producer_transition(
            self.gpu_order_producer(),
            producer,
            self.geometry_path(),
            self.raster_execution_plan(),
            self.projected_draw_policy,
        )? {
            return Ok(());
        }
        self.presenter.prepare_gpu_order_producer(producer).await?;
        self.presenter.set_gpu_order_producer(producer)?;
        self.finish_gpu_order_producer_transition();
        Ok(())
    }

    fn finish_gpu_order_producer_transition(&mut self) {
        self.gpu_order_initialized = false;
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.latest_gpu_order_measurement = None;
        self.reset_adaptive_policy();
        self.adaptive_projected_cpu.suspend_learning();
        self.adaptive_projected_gpu.suspend_learning();
        self.adaptive_probe_owner = None;
        self.blocked_order_choice = None;
        self.frame_state.force_sort();
    }

    /// Enables the independent per-GPU-frame producer receipt ring. It is a
    /// diagnostic control and is admitted only under forced Compact so no
    /// Phase1 Candidate/Adaptive ticket can share the experiment.
    pub fn set_gpu_producer_measurement_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<(), RendererError> {
        if self.gpu_producer_measurement_enabled == enabled {
            return Ok(());
        }
        if enabled
            && !gpu_producer_measurement_context_is_valid(
                self.geometry_path(),
                self.raster_execution_plan(),
                self.projected_draw_policy,
            )
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        self.presenter.set_gpu_producer_measurement_enabled(enabled);
        self.gpu_producer_measurement_enabled = enabled;
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.latest_gpu_order_measurement = None;
        reset_adaptive_for_gpu_producer_measurement_transition(
            &mut self.adaptive_policy,
            &mut self.adaptive_projected_cpu,
            &mut self.adaptive_projected_gpu,
            &mut self.adaptive_probe_owner,
            &mut self.blocked_order_choice,
        );
        self.frame_state.force_sort();
        Ok(())
    }

    /// Transactionally changes only the projected draw strategy. A rejected
    /// forced Compact request leaves the previous policy and learned lanes
    /// untouched; repeated requests are no-ops.
    pub fn set_projected_draw_policy(
        &mut self,
        policy: SurfaceProjectedDrawPolicy,
    ) -> Result<(), RendererError> {
        if policy != SurfaceProjectedDrawPolicy::Compact
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement_enabled)
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        if !validate_projected_draw_policy_transition(
            self.projected_draw_policy,
            policy,
            self.presenter.projected_contributor_indirect_draw_enabled(),
        )? {
            return Ok(());
        }
        // Candidate and Compact have different FrameCompletion workloads.
        // No order sample, projected sample, or owner chosen under the old
        // execution may cross this accepted transition. Stable projected
        // history remains intact so returning to Adaptive does not relearn
        // from zero; stale terminal tickets remain diagnostic-only.
        if self.adaptive_policy.metric() == AdaptiveMetric::FrameCompletion {
            self.adaptive_policy.reset(AdaptiveMetric::FrameCompletion);
        }
        self.adaptive_projected_cpu.suspend_learning();
        self.adaptive_projected_gpu.suspend_learning();
        self.adaptive_probe_owner = None;
        self.blocked_order_choice = None;
        self.projected_draw_policy = policy;
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.frame_state.force_sort();
        Ok(())
    }

    /// Selects the exact Packed raster implementation without changing the
    /// CPU/GPU/Adaptive ordering policy or the source scene contract.
    pub fn set_raster_execution_plan(
        &mut self,
        plan: SurfaceRasterExecutionPlan,
    ) -> Result<(), RendererError> {
        // A repeated setter call is not a strategy transition. In particular,
        // do not erase Adaptive's measured history or force a redundant sort
        // when bindings re-apply their current configuration.
        let previous = self.presenter.raster_execution_plan();
        if previous == plan {
            return Ok(());
        }
        if plan != SurfaceRasterExecutionPlan::ProjectedQuadsExact
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement_enabled)
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        self.presenter.set_raster_execution_plan(plan)?;
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        self.pending_projected_choice = None;
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        // Projection/raster queue pressure is part of the FrameCompletion
        // metric. Measurements learned under the previous raster plan are not
        // comparable, even though both plans consume the same exact order.
        let adaptive_reset =
            reset_adaptive_for_raster_transition(&mut self.adaptive_policy, previous, plan);
        debug_assert!(adaptive_reset);
        self.reset_projected_draw_policies();
        self.frame_state.force_sort();
        Ok(())
    }

    /// Switches the shared renderer and presenter to a different geometry
    /// path (experimental A/B benchmark knob; default remains
    /// [`GeometryPath::SortedIndexDirect`]).
    pub fn set_geometry_path(&mut self, path: GeometryPath) -> Result<(), RendererError> {
        if path != GeometryPath::PackedAtlas
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement_enabled)
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        match surface_geometry_switch_entry(
            cfg!(target_arch = "wasm32"),
            self.geometry_path(),
            path,
        ) {
            SurfaceGeometrySwitchEntry::AlreadyActive => return Ok(()),
            SurfaceGeometrySwitchEntry::AsyncPreparationRequired => {
                return Err(SurfacePresenterError::SurfaceGeometryPreparationRequired.into());
            }
            SurfaceGeometrySwitchEntry::Unsupported => {
                return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported.into());
            }
            SurfaceGeometrySwitchEntry::Synchronous => {}
        }
        if path == GeometryPath::PagedActiveAtlas && self.order_backend != SurfaceOrderBackend::Cpu
        {
            return Err(RendererError::InvalidConfig);
        }
        let changed = try_switch_renderer_geometry_path(&mut self.renderer, path, |renderer| {
            self.presenter.set_geometry_path(path, renderer)
        })?;
        if !changed {
            return Ok(());
        }
        self.finish_geometry_path_switch(path);
        Ok(())
    }

    /// Browser-only two-phase geometry switch. The renderer's target CPU
    /// derivations and the presenter's complete GPU graph remain unpublished
    /// while validation/OOM/internal scopes are pending. Direct and Packed
    /// are the full-quality runtime pair; Paged remains constructor-time-only.
    #[cfg(target_arch = "wasm32")]
    pub async fn set_geometry_path_async(
        &mut self,
        path: GeometryPath,
    ) -> Result<(), RendererError> {
        if path != GeometryPath::PackedAtlas
            && (self.gpu_order_producer() == SurfaceGpuOrderProducer::Preproject
                || self.gpu_producer_measurement_enabled)
        {
            return Err(SurfacePresenterError::PreprojectProducerIncompatible.into());
        }
        match surface_geometry_switch_entry(true, self.geometry_path(), path) {
            SurfaceGeometrySwitchEntry::AlreadyActive => return Ok(()),
            SurfaceGeometrySwitchEntry::Unsupported => {
                return Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported.into());
            }
            SurfaceGeometrySwitchEntry::AsyncPreparationRequired => {}
            SurfaceGeometrySwitchEntry::Synchronous => unreachable!(),
        }

        let prepared_renderer = self
            .renderer
            .prepare_geometry_path_candidate(path)?
            .expect("changed geometry path must produce a renderer candidate");
        let prepare_gpu_order = self.order_backend != SurfaceOrderBackend::Cpu;
        let require_projected_compaction =
            self.projected_draw_policy == SurfaceProjectedDrawPolicy::Compact;
        let prepared_presenter = self
            .presenter
            .prepare_geometry_path_async(
                path,
                &self.renderer,
                &prepared_renderer,
                prepare_gpu_order,
                require_projected_compaction,
            )
            .await?;

        // Both candidates are complete and every following operation is an
        // infallible assignment. No live session field changed before here.
        self.renderer
            .publish_geometry_path_candidate(prepared_renderer);
        self.presenter
            .publish_geometry_path_candidate(prepared_presenter);
        self.finish_geometry_path_switch(path);
        Ok(())
    }

    fn finish_geometry_path_switch(&mut self, _path: GeometryPath) {
        #[cfg(not(target_arch = "wasm32"))]
        if _path == GeometryPath::PagedActiveAtlas {
            self.disable_async_sort();
        }
        self.gpu_order_initialized = false;
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        self.reset_adaptive_policy();
        self.reset_projected_draw_policies();
        self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
        self.frame_state.force_sort();
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Monotonic identity of the camera state currently owned by this session.
    ///
    /// Native benchmark receipts use this read-only value to prove that the
    /// pose/intrinsics they report belong to the same revision that was
    /// submitted and presented. Camera scheduling remains owned by the shared
    /// Surface session.
    pub const fn camera_revision(&self) -> u64 {
        self.camera_revision
    }

    pub fn set_camera(&mut self, camera: Camera) -> Result<(), RendererError> {
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;
        if self.camera != camera {
            self.camera = camera;
            self.camera_revision = self.camera_revision.wrapping_add(1);
            self.frame_state.mark_camera_changed();
        }
        Ok(())
    }

    pub fn surface_size(&self) -> (u32, u32) {
        self.presenter.surface_size()
    }

    /// Actual raster target dimensions before any Surface presentation.
    pub fn internal_render_size(&self) -> (u32, u32) {
        self.presenter.internal_render_size()
    }

    pub fn last_presented_size(&self) -> Option<(u32, u32)> {
        self.presenter.last_presented_size()
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let previous_size = self.presenter.surface_size();
        self.presenter.resize(width, height)?;
        let (surface_width, surface_height) = self.presenter.surface_size();
        self.renderer.set_size(surface_width, surface_height)?;
        if previous_size != (surface_width, surface_height) {
            self.pending_tiled_backend = None;
            self.pending_tiled_adaptive_choice = None;
            self.latest_gpu_order_measurement = None;
            self.latest_cpu_order_measurement = None;
            self.reset_adaptive_policy();
            self.reset_projected_draw_policies();
            self.frame_state.force_sort();
        }
        Ok(())
    }

    /// Browser-only transactional resize for the production Packed +
    /// Projected Surface. No renderer/session size or scheduling state is
    /// published until the presenter's async configure transaction succeeds.
    #[cfg(target_arch = "wasm32")]
    pub async fn resize_async(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let candidate_config = gsplat_core::RendererConfig {
            width,
            height,
            ..self.renderer.config()
        };
        candidate_config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;
        let previous_size = self.presenter.surface_size();
        self.presenter.resize_async(width, height).await?;
        // On wasm, `Renderer::set_size` performs exactly the validation above
        // and then publishes the copyable config; it creates no GPU resource.
        self.renderer.set_size(width, height)?;
        let (surface_width, surface_height) = self.presenter.surface_size();
        if previous_size != (surface_width, surface_height) {
            self.pending_tiled_backend = None;
            self.pending_tiled_adaptive_choice = None;
            self.latest_gpu_order_measurement = None;
            self.latest_cpu_order_measurement = None;
            self.reset_adaptive_policy();
            self.reset_projected_draw_policies();
            self.frame_state.force_sort();
        }
        Ok(())
    }

    pub fn sort_interval(&self) -> u32 {
        self.sort_interval
    }

    pub const fn order_backend(&self) -> SurfaceOrderBackend {
        self.order_backend
    }

    /// Transactionally prepares the complete GPU-order resource graph without
    /// changing the selected backend, frame state, or Adaptive evidence.
    /// Browser callers must await this before selecting GPU or Adaptive.
    pub async fn prepare_gpu_order(&mut self) -> Result<(), RendererError> {
        self.presenter.prepare_gpu_order().await?;
        Ok(())
    }

    pub fn set_order_backend(&mut self, backend: SurfaceOrderBackend) -> Result<(), RendererError> {
        if self.order_backend == backend {
            return Ok(());
        }
        if backend != SurfaceOrderBackend::Cpu
            && self.geometry_path() == GeometryPath::PagedActiveAtlas
        {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if backend != SurfaceOrderBackend::Cpu && self.async_sort_enabled {
            return Err(RendererError::InvalidConfig);
        }
        let gpu_prepare_error = if backend == SurfaceOrderBackend::Cpu {
            None
        } else {
            self.presenter.prepare_direct_gpu_order().err()
        };
        let gpu_prepare_failed = match (backend, gpu_prepare_error) {
            (SurfaceOrderBackend::Gpu, Some(error)) => return Err(error.into()),
            (SurfaceOrderBackend::Adaptive, Some(error)) => {
                let error = RendererError::from(error);
                if let Some(reason) = adaptive_gpu_order_failure_reason(&error) {
                    self.adaptive_gpu_failure = Some(reason);
                    true
                } else {
                    return Err(error);
                }
            }
            _ => false,
        };
        self.order_backend = backend;
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        self.pending_projected_choice = None;
        if backend != SurfaceOrderBackend::Adaptive || !gpu_prepare_failed {
            self.adaptive_gpu_failure = None;
        }
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        self.reset_adaptive_policy();
        // Backend-specific projected lanes keep their completed history, but
        // neither lane may retain an in-flight ticket or arbitration owner
        // across a backend transition. An eventual old receipt is still
        // exposed in diagnostics and cannot mutate either policy.
        self.adaptive_projected_cpu.suspend_learning();
        self.adaptive_projected_gpu.suspend_learning();
        self.adaptive_probe_owner = None;
        self.blocked_order_choice = None;
        if backend == SurfaceOrderBackend::Adaptive && gpu_prepare_failed {
            self.adaptive_policy.gpu_failed();
        }
        // A later GPU frame must build a same-context prefix even if the
        // previous GPU producer had once published a valid one. `force_sort`
        // below is the scheduler contract; this presenter invalidation is the
        // independent fail-closed guard.
        self.presenter.invalidate_gpu_order_producer_prefix();
        self.frame_state.force_sort();
        Ok(())
    }

    pub fn sort_schedule(&self) -> SurfaceSortSchedule {
        #[cfg(not(target_arch = "wasm32"))]
        if self.async_sort_enabled {
            return SurfaceSortSchedule::AsyncLatest {
                interval: self.sort_interval,
            };
        }
        SurfaceSortSchedule::Interval(self.sort_interval)
    }

    pub fn set_sort_schedule(
        &mut self,
        schedule: SurfaceSortSchedule,
    ) -> Result<(), RendererError> {
        if schedule.interval() == 0 {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(target_arch = "wasm32")]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. }) {
            return Err(RendererError::InvalidConfig);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(schedule, SurfaceSortSchedule::AsyncLatest { .. })
            && self.order_backend != SurfaceOrderBackend::Cpu
        {
            return Err(RendererError::InvalidConfig);
        }
        match schedule {
            SurfaceSortSchedule::Interval(interval) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.set_async_sort_enabled(false)?;
                self.set_sort_interval(interval)
            }
            SurfaceSortSchedule::AsyncLatest { interval } => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.set_async_sort_enabled(true)?;
                    self.set_sort_interval(interval)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = interval;
                    unreachable!("async schedules are rejected before mutation")
                }
            }
        }
    }

    pub fn set_sort_interval(&mut self, interval: u32) -> Result<(), RendererError> {
        if interval == 0 {
            return Err(RendererError::InvalidConfig);
        }
        if self.sort_interval != interval {
            self.sort_interval = interval;
            self.reset_adaptive_policy();
            self.frame_state.force_sort();
        }
        Ok(())
    }

    pub fn set_frame_latency(&mut self, latency: u32) {
        self.presenter.set_frame_latency(latency);
        self.latest_gpu_order_measurement = None;
        self.latest_cpu_order_measurement = None;
        self.reset_adaptive_policy();
        self.reset_projected_draw_policies();
        self.frame_state.force_sort();
    }

    fn reset_adaptive_policy(&mut self) {
        // Select the backend on the user-visible frame boundary. A GPU radix
        // timestamp can beat CPU preprocess+sort while still reducing total
        // throughput by contending with projection and raster work on the same
        // queue. The paired ABBA completion probe keeps one incumbent while a
        // formal receipt is pending, so both backends include their real queue
        // pressure without turning readback latency into challenger residency.
        self.adaptive_policy.reset(adaptive_primary_metric());
        if self.adaptive_probe_owner == Some(AdaptiveProbeOwner::Order) {
            self.adaptive_probe_owner = None;
        }
        self.blocked_order_choice = None;
    }

    fn reset_projected_draw_policies(&mut self) {
        self.adaptive_projected_cpu.reset();
        self.adaptive_projected_gpu.reset();
        self.adaptive_probe_owner = None;
        self.pending_projected_choice = None;
        self.blocked_order_choice = None;
    }

    fn projected_policy(&self, backend: SurfaceOrderBackendUsed) -> &AdaptiveProjectedDrawPolicy {
        match backend {
            SurfaceOrderBackendUsed::Cpu => &self.adaptive_projected_cpu,
            SurfaceOrderBackendUsed::Gpu => &self.adaptive_projected_gpu,
        }
    }

    fn projected_policy_mut(
        &mut self,
        backend: SurfaceOrderBackendUsed,
    ) -> &mut AdaptiveProjectedDrawPolicy {
        match backend {
            SurfaceOrderBackendUsed::Cpu => &mut self.adaptive_projected_cpu,
            SurfaceOrderBackendUsed::Gpu => &mut self.adaptive_projected_gpu,
        }
    }

    fn refresh_adaptive_probe_owner(&mut self) {
        let finished = match self.adaptive_probe_owner {
            Some(AdaptiveProbeOwner::Order) => !self.adaptive_policy.cohort_active(),
            Some(AdaptiveProbeOwner::ProjectedCpu) => !self.adaptive_projected_cpu.cohort_active(),
            Some(AdaptiveProbeOwner::ProjectedGpu) => !self.adaptive_projected_gpu.cohort_active(),
            None => false,
        };
        if finished {
            let projected_owner_finished = matches!(
                self.adaptive_probe_owner,
                Some(AdaptiveProbeOwner::ProjectedCpu | AdaptiveProbeOwner::ProjectedGpu)
            );
            let projected_incumbent_changed = match self.adaptive_probe_owner {
                Some(AdaptiveProbeOwner::ProjectedCpu) => {
                    self.adaptive_projected_cpu.take_incumbent_changed()
                }
                Some(AdaptiveProbeOwner::ProjectedGpu) => {
                    self.adaptive_projected_gpu.take_incumbent_changed()
                }
                Some(AdaptiveProbeOwner::Order) | None => false,
            };
            if should_reset_order_for_projected_incumbent_change(
                self.order_backend,
                self.projected_draw_policy,
                self.adaptive_policy.metric(),
                self.adaptive_policy.pending.is_some(),
                projected_owner_finished,
                projected_incumbent_changed,
            ) {
                // FrameCompletion includes raster queue pressure. Publish the
                // projected winner first, then discard only order evidence at
                // this owner boundary; neither projected lane is reset.
                self.adaptive_policy.reset(adaptive_primary_metric());
                self.blocked_order_choice = None;
                self.frame_state.force_sort();
            }
            if projected_owner_finished && self.blocked_order_choice.is_some() {
                self.frame_state.force_sort();
            }
            self.adaptive_probe_owner = None;
        }
    }

    fn projected_probe_owner(backend: SurfaceOrderBackendUsed) -> AdaptiveProbeOwner {
        match backend {
            SurfaceOrderBackendUsed::Cpu => AdaptiveProbeOwner::ProjectedCpu,
            SurfaceOrderBackendUsed::Gpu => AdaptiveProbeOwner::ProjectedGpu,
        }
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    /// Formal Adaptive sample awaiting its asynchronous terminal receipt.
    pub fn adaptive_pending_sample(&self) -> Option<SurfaceAdaptivePendingSample> {
        (self.order_backend == SurfaceOrderBackend::Adaptive)
            .then(|| self.adaptive_policy.pending_sample())
            .flatten()
    }

    /// Current Adaptive policy state, including transitions completed by an
    /// explicit receipt poll between rendered frames.
    pub fn adaptive_state(&self) -> SurfaceAdaptiveState {
        if self.order_backend == SurfaceOrderBackend::Adaptive {
            self.adaptive_policy.state()
        } else {
            SurfaceAdaptiveState::Disabled
        }
    }

    pub fn projected_draw_adaptive_state(
        &self,
        backend: SurfaceOrderBackendUsed,
    ) -> SurfaceProjectedDrawAdaptiveState {
        if self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive
            && self.raster_execution_plan() == SurfaceRasterExecutionPlan::ProjectedQuadsExact
        {
            self.projected_policy(backend).state()
        } else {
            SurfaceProjectedDrawAdaptiveState::Disabled
        }
    }

    pub fn projected_draw_adaptive_pending_sample(
        &self,
        backend: SurfaceOrderBackendUsed,
    ) -> Option<SurfaceProjectedDrawAdaptivePendingSample> {
        (self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive)
            .then(|| self.projected_policy(backend).pending_sample())
            .flatten()
    }

    /// Polls CPU/GPU completion callbacks and publishes terminal receipts
    /// without acquiring a Surface texture, encoding a draw, or submitting
    /// more queue work. Benchmarks use this to isolate one formal sample from
    /// artificial drain-frame backlog.
    pub fn poll_order_measurement_receipts(&mut self) {
        let _ = self.collect_order_measurements();
        let _ = self.collect_projected_draw_measurements();
        let _ = self.collect_gpu_producer_measurements();
    }

    /// Drains completed CPU order measurements in ticket order.
    pub fn drain_cpu_order_measurements(&mut self) -> Vec<SurfaceCpuOrderMeasurement> {
        self.completed_cpu_order_measurements.drain().collect()
    }

    /// Drains exact asynchronous GPU timing/count receipts in ticket order.
    pub fn drain_order_measurements(&mut self) -> Vec<SurfaceOrderMeasurement> {
        self.completed_order_measurements.drain().collect()
    }

    /// Drains terminal failure receipts for issued GPU measurement tickets.
    pub fn drain_order_measurement_failures(&mut self) -> Vec<SurfaceOrderMeasurementFailure> {
        self.completed_order_measurement_failures.drain().collect()
    }

    pub fn drain_projected_draw_measurements(&mut self) -> Vec<SurfaceProjectedDrawMeasurement> {
        self.completed_projected_draw_measurements.drain().collect()
    }

    pub fn drain_projected_draw_measurement_failures(
        &mut self,
    ) -> Vec<SurfaceProjectedDrawMeasurementFailure> {
        self.completed_projected_draw_measurement_failures
            .drain()
            .collect()
    }

    pub fn drain_gpu_producer_measurements(&mut self) -> Vec<SurfaceGpuProducerMeasurement> {
        self.completed_gpu_producer_measurements.drain(..).collect()
    }

    pub fn drain_gpu_producer_measurement_failures(
        &mut self,
    ) -> Vec<SurfaceGpuProducerMeasurementFailure> {
        self.completed_gpu_producer_measurement_failures
            .drain(..)
            .collect()
    }

    pub fn force_sort_refresh(&mut self) {
        self.frame_state.force_sort();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_async_sort_enabled(&mut self, enabled: bool) -> Result<(), RendererError> {
        if self.async_sort_enabled == enabled {
            return Ok(());
        }
        if enabled {
            if self.order_backend != SurfaceOrderBackend::Cpu {
                return Err(RendererError::InvalidConfig);
            }
            self.async_sorter = Some(SurfaceAsyncSorter::new(&self.renderer)?);
        } else {
            self.disable_async_sort();
            return Ok(());
        }
        self.async_sort_enabled = enabled;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn disable_async_sort(&mut self) {
        self.async_sorter = None;
        self.async_sort_enabled = false;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn async_sort_enabled(&self) -> bool {
        self.async_sort_enabled
    }

    pub fn render_frame(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.async_sort_enabled
            && self.order_backend == SurfaceOrderBackend::Cpu
            && self.geometry_path() != GeometryPath::PagedActiveAtlas
        {
            return self.render_frame_async_sort();
        }
        self.render_frame_sync()
    }

    fn render_frame_sync(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let (completed_measurement, completed_measurement_failure) =
            self.collect_order_measurements();
        let (completed_projected_measurement, completed_projected_measurement_failure) =
            self.collect_projected_draw_measurements();
        let (completed_gpu_producer_measurement, completed_gpu_producer_measurement_failure) =
            self.collect_gpu_producer_measurements();
        self.refresh_adaptive_probe_owner();
        let has_order = match self.presented_order_backend {
            SurfaceOrderBackendUsed::Cpu => !self.renderer.current_sorted_indices().is_empty(),
            SurfaceOrderBackendUsed::Gpu => self.gpu_order_initialized,
        };
        let plan = self.frame_state.plan(has_order, self.sort_interval);
        let mut adaptive_choice = self.pending_tiled_adaptive_choice.or_else(|| {
            if self.order_backend != SurfaceOrderBackend::Adaptive {
                return None;
            }
            if let Some(blocked) = self.blocked_order_choice {
                return Some(blocked);
            }
            plan.refresh_sort.then(|| {
                if matches!(
                    self.adaptive_probe_owner,
                    Some(AdaptiveProbeOwner::ProjectedCpu | AdaptiveProbeOwner::ProjectedGpu)
                ) {
                    self.adaptive_policy.held_refresh_choice()
                } else {
                    self.adaptive_policy.choose_refresh_backend()
                }
            })
        });
        let order_wants_formal_sample =
            adaptive_choice.is_some_and(|choice| choice.sample.is_some());
        if order_probe_owner_should_yield(
            self.adaptive_probe_owner,
            self.adaptive_policy.pending.is_some(),
            plan.refresh_sort,
            order_wants_formal_sample,
        ) {
            // Order cohorts advance only on actual refreshes. Once their last
            // ticket is terminal, a stable frame would otherwise leave Order
            // owning an idle cohort and starve the projected learner. Yield
            // only the owner; phase/history resume on the next refresh.
            self.adaptive_probe_owner = None;
        }
        let planned_backend = match adaptive_choice {
            Some(choice) => choice.backend,
            None if !plan.refresh_sort => self.presented_order_backend,
            None => match self.order_backend {
                SurfaceOrderBackend::Cpu => SurfaceOrderBackendUsed::Cpu,
                SurfaceOrderBackend::Gpu => SurfaceOrderBackendUsed::Gpu,
                SurfaceOrderBackend::Adaptive => unreachable!("adaptive refresh has a choice"),
            },
        };
        let requested_backend = self.pending_tiled_backend.unwrap_or(planned_backend);
        let compact_available = self.presenter.projected_contributor_indirect_draw_enabled();
        let projected_choice_was_pending = self.pending_projected_choice.is_some();
        let mut projected_choice = self.pending_projected_choice.unwrap_or_else(|| {
            match self.projected_draw_policy {
                SurfaceProjectedDrawPolicy::Candidate => {
                    return ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Candidate,
                        sample: None,
                    };
                }
                SurfaceProjectedDrawPolicy::Compact => {
                    return ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Compact,
                        sample: None,
                    };
                }
                SurfaceProjectedDrawPolicy::Adaptive => {}
            }
            if self.presenter.raster_execution_plan()
                != SurfaceRasterExecutionPlan::ProjectedQuadsExact
            {
                return ProjectedAdaptiveChoice {
                    execution: SurfaceProjectedDrawExecution::Candidate,
                    sample: None,
                };
            }
            let owner = Self::projected_probe_owner(requested_backend);
            if !projected_policy_can_sample(
                self.adaptive_probe_owner,
                owner,
                self.adaptive_probe_owner == Some(AdaptiveProbeOwner::Order),
            ) {
                self.projected_policy(requested_backend).held_choice()
            } else {
                self.projected_policy_mut(requested_backend)
                    .choose(compact_available)
            }
        });
        let projected_owner = Self::projected_probe_owner(requested_backend);
        let projected_order_changed_this_frame = match requested_backend {
            SurfaceOrderBackendUsed::Cpu => {
                projected_order_changed(plan.refresh_sort, plan.upload_order, plan.refresh_sort)
            }
            SurfaceOrderBackendUsed::Gpu => {
                gpu_projected_order_changed(plan.refresh_sort, plan.refresh_sort)
            }
        };
        let projected_formal_sample_deferred =
            defer_projected_formal_choice(projected_choice, projected_order_changed_this_frame);
        let projected_claims_owner = projected_probe_claims_owner(
            projected_choice,
            projected_order_changed_this_frame,
            projected_choice_was_pending,
        );
        let projected_wants_transition_warmup = matches!(
            projected_choice.sample,
            Some(ProjectedAdaptiveSampleKind::TransitionWarmup)
        );
        if self.adaptive_probe_owner == Some(projected_owner)
            && projected_formal_sample_deferred
            && projected_choice_was_pending
        {
            // The first changed-order frame gives a new projected formal
            // choice one grace turn: the choice becomes pending so a following
            // cached-order frame can issue it. A second changed-order frame
            // must release ownership, otherwise continuous camera motion can
            // starve the order learner forever. Clear only the session ticket
            // request; the projected policy has not consumed its sample index
            // and will reissue it after Order finishes. Preserve execution so
            // a warmed challenger is not changed mid-frame.
            self.adaptive_probe_owner = None;
            self.pending_projected_choice = None;
            projected_choice.sample = None;
        }
        if matches!(
            self.adaptive_probe_owner,
            Some(AdaptiveProbeOwner::ProjectedCpu | AdaptiveProbeOwner::ProjectedGpu)
        ) && self.blocked_order_choice.is_some()
        {
            adaptive_choice = adaptive_choice.map(|choice| AdaptiveRefreshChoice {
                backend: choice.backend,
                sample: None,
            });
        } else if self.adaptive_probe_owner.is_none()
            && order_wants_formal_sample
            && projected_wants_transition_warmup
        {
            // A transition warmup may run while the order changes, but it is
            // not a formal projected ticket and must not retain arbitration
            // ownership. Delay the order sample for only this one frame so its
            // FrameCompletion evidence is not polluted by a raster-lane
            // transition.
            self.blocked_order_choice = adaptive_choice;
            adaptive_choice = adaptive_choice.map(|choice| AdaptiveRefreshChoice {
                backend: choice.backend,
                sample: None,
            });
        } else if self.adaptive_probe_owner.is_none()
            && order_wants_formal_sample
            && projected_claims_owner
        {
            // Stabilize the exact raster lane for the target order backend
            // before timing that order choice. The order policy has not
            // consumed evidence and will return this same formal choice after
            // the projected cohort reaches its owner boundary.
            self.blocked_order_choice = adaptive_choice;
            adaptive_choice = adaptive_choice.map(|choice| AdaptiveRefreshChoice {
                backend: choice.backend,
                sample: None,
            });
            self.adaptive_probe_owner =
                arbitrate_new_probe_owner(self.adaptive_probe_owner, true, true, projected_owner);
        } else if self.adaptive_probe_owner.is_none() && order_wants_formal_sample {
            self.blocked_order_choice = None;
            self.adaptive_probe_owner =
                arbitrate_new_probe_owner(self.adaptive_probe_owner, true, false, projected_owner);
        } else if self.adaptive_probe_owner.is_none() && projected_claims_owner {
            self.adaptive_probe_owner =
                arbitrate_new_probe_owner(self.adaptive_probe_owner, false, true, projected_owner);
        }
        // Every exact CPU refresh publishes the same frame-start -> queue-done
        // interval as GPU telemetry. Adaptive consumes only its matching
        // formal ticket, while forced-CPU benchmarks retain comparable
        // completion evidence instead of submit-wall timing.
        let track_cpu_completion =
            should_measure_cpu_refresh(plan, requested_backend, self.geometry_path());
        let mut gpu_failed = false;
        let mut output = if requested_backend == SurfaceOrderBackendUsed::Gpu
            && self.geometry_path() != GeometryPath::PagedActiveAtlas
        {
            match self.render_gpu_with_plan(plan, projected_choice) {
                Ok(output) => output,
                Err(error) if self.order_backend == SurfaceOrderBackend::Adaptive => {
                    let Some(reason) = adaptive_gpu_order_failure_reason(&error) else {
                        return Err(error);
                    };
                    gpu_failed = true;
                    self.adaptive_gpu_failure = Some(reason);
                    self.frame_state.force_sort();
                    let fallback_plan = self.frame_state.plan(
                        !self.renderer.current_sorted_indices().is_empty(),
                        self.sort_interval,
                    );
                    projected_choice = self
                        .projected_policy(SurfaceOrderBackendUsed::Cpu)
                        .held_choice();
                    if self.adaptive_probe_owner == Some(AdaptiveProbeOwner::ProjectedGpu) {
                        self.adaptive_probe_owner = None;
                    }
                    let mut output = self.render_with_plan(
                        fallback_plan,
                        true,
                        fallback_plan.refresh_sort,
                        projected_choice,
                    )?;
                    output.gpu_sort_fallback = true;
                    output
                }
                Err(error) => return Err(error),
            }
        } else {
            self.render_with_plan(
                plan,
                plan.refresh_sort,
                track_cpu_completion,
                projected_choice,
            )?
        };
        // Keep one outer wall clock so a failed GPU attempt plus CPU fallback
        // is measured as the frame the caller actually experienced.
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        output.timings.frame_wall_ms = frame_wall_ms;
        output.stats.frame_ms = frame_wall_ms;
        if !output.frame_presented {
            // Exact WebGPU count/allocation is a preparation turn, not a
            // rendered frame. Preserve the frame-state plan, adaptive sample,
            // applied revision, and measurement ledger until the matching
            // scatter/raster/present submission exists.
            output.completed_order_measurement = completed_measurement;
            output.completed_order_measurement_failure = completed_measurement_failure;
            output.completed_projected_draw_measurement = completed_projected_measurement;
            output.completed_projected_draw_measurement_failure =
                completed_projected_measurement_failure;
            output.completed_gpu_producer_measurement = completed_gpu_producer_measurement;
            output.completed_gpu_producer_measurement_failure =
                completed_gpu_producer_measurement_failure;
            output.gpu_timestamp_queries_enabled = self.presenter.gpu_order_timestamps_enabled();
            self.pending_tiled_backend = Some(output.order_backend);
            self.pending_tiled_adaptive_choice = adaptive_choice;
            self.pending_projected_choice = Some(projected_choice);
            return Ok(output);
        }
        self.pending_tiled_backend = None;
        self.pending_tiled_adaptive_choice = None;
        let defer_projected_choice = defer_projected_formal_choice(
            projected_choice,
            output.sort_refreshed || output.order_uploaded,
        );
        // A formal Candidate/Compact ticket is comparable only when the
        // presented order stayed unchanged. Keep the exact requested choice
        // for the next stable-order frame instead of consuming its sample
        // index or silently turning it into an untimed frame.
        self.pending_projected_choice = defer_projected_choice.then_some(projected_choice);
        self.last_stats = output.stats;
        if self.order_backend == SurfaceOrderBackend::Adaptive {
            if gpu_failed {
                self.adaptive_policy.gpu_failed();
            } else if let Some(choice) = adaptive_choice {
                if choice.backend == SurfaceOrderBackendUsed::Gpu {
                    self.adaptive_gpu_failure = None;
                }
                match (choice.sample, self.adaptive_policy.metric()) {
                    (None, _) => {}
                    (Some(AdaptiveSampleKind::TransitionWarmup), AdaptiveMetric::OrderOnly) => {
                        // A successful submission is sufficient: queue order
                        // guarantees the next timed GPU command executes after
                        // this untimed transition. Waiting for readback here
                        // would multiply exploration cost without adding
                        // timing evidence.
                        self.adaptive_policy
                            .complete_synchronous_sample(choice, 0.0);
                    }
                    (Some(_), AdaptiveMetric::OrderOnly)
                        if choice.backend == SurfaceOrderBackendUsed::Cpu =>
                    {
                        self.adaptive_policy.complete_synchronous_sample(
                            choice,
                            output.stats.preprocess_ms + output.stats.sort_ms,
                        );
                    }
                    (Some(_), _) => {
                        if let Some(ticket) = output.order_measurement_submission.ticket() {
                            self.adaptive_policy.register_pending_sample(choice, ticket);
                        } else {
                            self.blocked_order_choice = Some(choice);
                            self.frame_state.force_sort();
                        }
                    }
                }
            }
        }
        if !defer_projected_choice {
            match projected_choice.sample {
                None => {}
                Some(ProjectedAdaptiveSampleKind::TransitionWarmup) => self
                    .projected_policy_mut(output.order_backend)
                    .complete_synchronous_sample(projected_choice),
                Some(
                    ProjectedAdaptiveSampleKind::CandidateBootstrap
                    | ProjectedAdaptiveSampleKind::Probe(_),
                ) => {
                    if let Some(ticket) = output.projected_draw_measurement_submission.ticket() {
                        self.projected_policy_mut(output.order_backend)
                            .register_pending_sample(
                                output.order_backend,
                                projected_choice,
                                ticket,
                            );
                    }
                }
            }
        }
        output.completed_order_measurement = completed_measurement;
        output.completed_order_measurement_failure = completed_measurement_failure;
        output.completed_projected_draw_measurement = completed_projected_measurement;
        output.completed_projected_draw_measurement_failure =
            completed_projected_measurement_failure;
        output.completed_gpu_producer_measurement = completed_gpu_producer_measurement;
        output.completed_gpu_producer_measurement_failure =
            completed_gpu_producer_measurement_failure;
        output.gpu_timestamp_queries_enabled = self.presenter.gpu_order_timestamps_enabled();
        if output.order_backend == SurfaceOrderBackendUsed::Gpu {
            if let Some(measurement) = self.latest_gpu_order_measurement {
                output.stats.visible_count = measurement.visible_count;
                output.stats.drawn_count = measurement.drawn_count;
                output.visible_count_revision = Some(measurement.camera_revision);
                output.visible_count_pending = measurement.camera_revision != self.camera_revision;
            } else {
                output.stats.visible_count = 0;
                output.stats.drawn_count = 0;
                output.visible_count_revision = None;
                output.visible_count_pending = true;
            }
        } else if self.presenter.projected_contributor_indirect_draw_enabled() {
            if let Some(measurement) = self.latest_cpu_order_measurement {
                output.stats.visible_count = measurement.visible_count;
                output.stats.drawn_count = measurement.drawn_count;
                output.visible_count_revision = Some(measurement.camera_revision);
                output.visible_count_pending = measurement.camera_revision != self.camera_revision;
            } else {
                output.stats.visible_count = 0;
                output.stats.drawn_count = 0;
                output.visible_count_revision = None;
                output.visible_count_pending = true;
            }
        }
        if let Some(measurement) = completed_projected_measurement
            && measurement.order_backend == output.order_backend
        {
            output.stats.visible_count = measurement.visible_count;
            output.stats.drawn_count = measurement.drawn_count;
            output.visible_count_revision = Some(measurement.camera_revision);
            output.visible_count_pending = measurement.camera_revision != self.camera_revision;
        }
        output.adaptive_state = if self.order_backend == SurfaceOrderBackend::Adaptive {
            self.adaptive_policy.state()
        } else {
            SurfaceAdaptiveState::Disabled
        };
        output.adaptive_gpu_failure = self.adaptive_gpu_failure;
        output.projected_draw_adaptive_state =
            self.projected_draw_adaptive_state(output.order_backend);
        self.last_stats = output.stats;
        if output.sort_refreshed {
            self.applied_order_revision = self.camera_revision;
            self.applied_order_camera = self.camera;
            output.applied_order_revision = self.applied_order_revision;
            output.presented_order_revision_lag = 0;
        }
        Ok(output)
    }

    fn collect_order_measurements(
        &mut self,
    ) -> (
        Option<SurfaceOrderMeasurement>,
        Option<SurfaceOrderMeasurementFailure>,
    ) {
        let cpu_telemetry = self.presenter.poll_cpu_order_completion_telemetry();
        for measurement in cpu_telemetry.completed {
            self.observe_cpu_completion_measurement(measurement);
            self.latest_cpu_order_measurement = Some(measurement);
            self.completed_cpu_order_measurements.push(measurement);
        }
        let mut newest_failure = None;
        for failure in cpu_telemetry.failures {
            if self.order_backend == SurfaceOrderBackend::Adaptive {
                self.adaptive_policy
                    .observe_cpu_measurement_failure(failure);
            }
            self.completed_order_measurement_failures.push(failure);
            newest_failure = Some(failure);
        }
        let telemetry = self.presenter.poll_gpu_order_telemetry();
        let mut newest = None;
        for measurement in telemetry.completed {
            if self.order_backend == SurfaceOrderBackend::Adaptive {
                self.adaptive_policy.observe_gpu_measurement(measurement);
            }
            self.latest_gpu_order_measurement = Some(measurement);
            self.completed_order_measurements.push(measurement);
            newest = Some(measurement);
        }
        for failure in telemetry.failures {
            if self.order_backend == SurfaceOrderBackend::Adaptive {
                self.adaptive_policy
                    .observe_gpu_measurement_failure(failure);
            }
            self.completed_order_measurement_failures.push(failure);
            newest_failure = Some(failure);
        }
        (newest, newest_failure)
    }

    fn collect_projected_draw_measurements(
        &mut self,
    ) -> (
        Option<SurfaceProjectedDrawMeasurement>,
        Option<SurfaceProjectedDrawMeasurementFailure>,
    ) {
        let telemetry = self.presenter.poll_projected_draw_telemetry();
        let mut newest = None;
        for measurement in telemetry.completed {
            if self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive
                && self.gpu_order_producer() == SurfaceGpuOrderProducer::PostSort
            {
                self.projected_policy_mut(measurement.order_backend)
                    .observe_measurement(measurement);
            }
            self.completed_projected_draw_measurements.push(measurement);
            newest = Some(measurement);
        }
        let mut newest_failure = None;
        for failure in telemetry.failures {
            if self.projected_draw_policy == SurfaceProjectedDrawPolicy::Adaptive
                && self.gpu_order_producer() == SurfaceGpuOrderProducer::PostSort
            {
                self.projected_policy_mut(failure.order_backend)
                    .observe_failure(failure);
            }
            self.completed_projected_draw_measurement_failures
                .push(failure);
            newest_failure = Some(failure);
        }
        self.refresh_adaptive_probe_owner();
        (newest, newest_failure)
    }

    fn collect_gpu_producer_measurements(
        &mut self,
    ) -> (
        Option<SurfaceGpuProducerMeasurement>,
        Option<SurfaceGpuProducerMeasurementFailure>,
    ) {
        let telemetry = self.presenter.poll_gpu_producer_telemetry();
        let mut newest = None;
        for measurement in telemetry.completed {
            retain_gpu_producer_terminal(
                &mut self.completed_gpu_producer_measurements,
                measurement,
            );
            newest = Some(measurement);
        }
        let mut newest_failure = None;
        for failure in telemetry.failures {
            retain_gpu_producer_terminal(
                &mut self.completed_gpu_producer_measurement_failures,
                failure,
            );
            newest_failure = Some(failure);
        }
        (newest, newest_failure)
    }

    fn observe_cpu_completion_measurement(&mut self, measurement: SurfaceCpuOrderMeasurement) {
        if self.order_backend == SurfaceOrderBackend::Adaptive
            && self.adaptive_policy.metric() == AdaptiveMetric::FrameCompletion
        {
            self.adaptive_policy.complete_pending_sample(
                SurfaceOrderBackendUsed::Cpu,
                measurement.ticket,
                measurement.frame_complete_ms,
            );
        }
    }

    fn render_gpu_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
        projected_choice: ProjectedAdaptiveChoice,
    ) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let order_changed = gpu_projected_order_changed(plan.refresh_sort, plan.refresh_sort);
        let projected_formal_sample =
            projected_formal_sample_requested(projected_choice, order_changed);
        self.presenter.set_projected_draw_execution(
            projected_choice.execution,
            projected_choice.sample.is_some(),
            projected_formal_sample.then_some(ProjectedDrawSampleRequest {
                camera_revision: self.camera_revision,
                started: frame_start,
                order_backend: SurfaceOrderBackendUsed::Gpu,
                order_refreshed: order_changed,
            }),
        );
        let render_start = timer_now();
        let presenter_submission = self.presenter.render_direct_gpu_order(
            &self.camera,
            plan.refresh_sort,
            self.camera_revision,
            frame_start,
        )?;
        let gpu_order_preparation_pending =
            presenter_submission == TelemetrySubmission::GpuOrderPreparationPending;
        let tiled_preparation_pending = gpu_order_preparation_pending;
        let frame_presented = self.presenter.last_frame_presented();
        let order_measurement_submission = SurfaceOrderMeasurementSubmission::from_presenter(
            SurfaceOrderBackendUsed::Gpu,
            presenter_submission,
        );
        let projected_draw_execution = self.presenter.resolved_projected_draw_execution();
        let projected_draw_measurement_submission =
            SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                projected_draw_execution,
                self.presenter.take_projected_draw_submission(),
            );
        let gpu_order_producer = self.presenter.take_actual_gpu_order_producer();
        let gpu_producer_submission = self.presenter.take_gpu_producer_submission();
        let gpu_producer_measurement_submission =
            SurfaceGpuProducerMeasurementSubmission::from_presenter(
                gpu_order_producer.or_else(|| {
                    self.gpu_producer_measurement_enabled
                        .then_some(self.gpu_order_producer())
                }),
                gpu_producer_submission,
            );
        let submitted_measurement_ticket = order_measurement_submission.ticket();
        let render_submit_ms = timer_elapsed_ms(render_start);
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        let stats = FrameStats {
            frame_ms: frame_wall_ms,
            preprocess_ms: 0.0,
            sort_ms: 0.0,
            raster_ms: 0.0,
            // The exact count arrives asynchronously from the same indirect
            // buffer used by this draw; zero here means pending, not sampled.
            visible_count: 0,
            drawn_count: 0,
        };
        if frame_presented {
            self.last_stats = stats;
            self.gpu_order_initialized |= plan.refresh_sort;
            self.presented_order_backend = SurfaceOrderBackendUsed::Gpu;
            self.frame_state.finish_frame(plan, false);
        }
        Ok(SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms,
                frame_wall_ms,
            },
            frame_presented,
            gpu_order_preparation_pending,
            tiled_preparation_pending,
            raster_execution_plan: self.presenter.raster_execution_plan(),
            sort_refreshed: frame_presented && plan.refresh_sort,
            order_uploaded: false,
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision: self.camera_revision,
            applied_order_revision: self.applied_order_revision,
            presented_order_revision_lag: u32::try_from(
                self.camera_revision
                    .saturating_sub(self.applied_order_revision),
            )
            .unwrap_or(u32::MAX),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend: SurfaceOrderBackendUsed::Gpu,
            gpu_sort_fallback: false,
            adaptive_state: SurfaceAdaptiveState::Disabled,
            adaptive_gpu_failure: None,
            projected_draw_policy: self.projected_draw_policy,
            projected_draw_execution,
            projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
            projected_draw_measurement_submission,
            completed_projected_draw_measurement: None,
            completed_projected_draw_measurement_failure: None,
            gpu_order_producer,
            gpu_producer_measurement_submission,
            completed_gpu_producer_measurement: None,
            completed_gpu_producer_measurement_failure: None,
            order_measurement_submission,
            submitted_measurement_ticket,
            completed_order_measurement: None,
            completed_order_measurement_failure: None,
            visible_count_revision: None,
            visible_count_pending: submitted_measurement_ticket.is_some(),
            gpu_timestamp_queries_enabled: self.presenter.gpu_order_timestamps_enabled(),
        })
    }

    fn render_with_plan(
        &mut self,
        plan: SurfaceFramePlan,
        sort_refreshed: bool,
        track_cpu_completion: bool,
        projected_choice: ProjectedAdaptiveChoice,
    ) -> Result<SurfaceFrameOutput, RendererError> {
        let frame_start = timer_now();
        let order_changed =
            projected_order_changed(plan.refresh_sort, plan.upload_order, sort_refreshed);
        let projected_formal_sample =
            projected_formal_sample_requested(projected_choice, order_changed);
        self.presenter.set_projected_draw_execution(
            projected_choice.execution,
            projected_choice.sample.is_some(),
            projected_formal_sample.then_some(ProjectedDrawSampleRequest {
                camera_revision: self.camera_revision,
                started: frame_start,
                order_backend: SurfaceOrderBackendUsed::Cpu,
                order_refreshed: order_changed,
            }),
        );
        let paged = self.geometry_path() == GeometryPath::PagedActiveAtlas;
        let mut stats = if paged {
            FrameStats::zero()
        } else {
            self.renderer
                .build_surface_sorted_indices_with_sort_refresh(&self.camera, plan.refresh_sort)?
        };
        let render_start = timer_now();
        let presenter_submission = if paged {
            let scene = self.renderer.scene().ok_or(RendererError::SceneNotLoaded)?;
            self.presenter
                .render_sorted_indices(scene, &[], &self.camera, true)?;
            let (visible_count, drawn_count) =
                paged_surface_counts(scene.len(), self.presenter.instance_count());
            stats.visible_count = visible_count;
            stats.drawn_count = drawn_count;
            TelemetrySubmission::NotRequested
        } else {
            let completion = track_cpu_completion.then_some(CpuCompletionSampleRequest {
                camera_revision: self.camera_revision,
                started: frame_start,
                preprocess_ms: stats.preprocess_ms,
                sort_ms: stats.sort_ms,
            });
            self.presenter.render_cpu_sorted_indices_tracked(
                self.renderer.current_sorted_indices(),
                &self.camera,
                plan.upload_order,
                completion,
            )?
        };
        let gpu_order_preparation_pending =
            presenter_submission == TelemetrySubmission::GpuOrderPreparationPending;
        let tiled_preparation_pending = gpu_order_preparation_pending;
        let frame_presented = self.presenter.last_frame_presented();
        let order_measurement_submission = SurfaceOrderMeasurementSubmission::from_presenter(
            SurfaceOrderBackendUsed::Cpu,
            presenter_submission,
        );
        let projected_draw_execution = self.presenter.resolved_projected_draw_execution();
        let projected_draw_measurement_submission =
            SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                projected_draw_execution,
                self.presenter.take_projected_draw_submission(),
            );
        let submitted_measurement_ticket = order_measurement_submission.ticket();
        let render_submit_ms = timer_elapsed_ms(render_start);
        let frame_wall_ms = timer_elapsed_ms(frame_start);
        stats.frame_ms = frame_wall_ms;
        if frame_presented {
            self.last_stats = stats;
            self.presented_order_backend = SurfaceOrderBackendUsed::Cpu;
            self.frame_state
                .finish_frame(plan, paged || plan.upload_order);
        }
        Ok(SurfaceFrameOutput {
            stats,
            timings: SurfaceFrameTimings {
                cpu_geometry_ms: 0.0,
                render_submit_ms,
                frame_wall_ms,
            },
            frame_presented,
            gpu_order_preparation_pending,
            tiled_preparation_pending,
            raster_execution_plan: self.presenter.raster_execution_plan(),
            sort_refreshed: frame_presented && (paged || sort_refreshed),
            order_uploaded: frame_presented && (paged || plan.upload_order),
            async_sort_revision_lag: None,
            stale_async_sort_dropped: false,
            async_sort_scheduled: false,
            camera_revision: self.camera_revision,
            applied_order_revision: self.applied_order_revision,
            presented_order_revision_lag: u32::try_from(
                self.camera_revision
                    .saturating_sub(self.applied_order_revision),
            )
            .unwrap_or(u32::MAX),
            async_sort_scheduled_revision: None,
            async_sort_completed_revision: None,
            async_sort_result_applied: false,
            sync_sort_fallback: false,
            order_backend: SurfaceOrderBackendUsed::Cpu,
            gpu_sort_fallback: false,
            adaptive_state: SurfaceAdaptiveState::Disabled,
            adaptive_gpu_failure: None,
            projected_draw_policy: self.projected_draw_policy,
            projected_draw_execution,
            projected_draw_adaptive_state: SurfaceProjectedDrawAdaptiveState::Disabled,
            projected_draw_measurement_submission,
            completed_projected_draw_measurement: None,
            completed_projected_draw_measurement_failure: None,
            gpu_order_producer: None,
            gpu_producer_measurement_submission:
                SurfaceGpuProducerMeasurementSubmission::NotRequested,
            completed_gpu_producer_measurement: None,
            completed_gpu_producer_measurement_failure: None,
            order_measurement_submission,
            submitted_measurement_ticket,
            completed_order_measurement: None,
            completed_order_measurement_failure: None,
            visible_count_revision: Some(if paged || plan.refresh_sort {
                self.camera_revision
            } else {
                self.applied_order_revision
            }),
            visible_count_pending: !paged
                && !plan.refresh_sort
                && self.applied_order_revision != self.camera_revision,
            gpu_timestamp_queries_enabled: self.presenter.gpu_order_timestamps_enabled(),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_frame_async_sort(&mut self) -> Result<SurfaceFrameOutput, RendererError> {
        let mut completed_timing = None;
        let mut applied_order = false;
        let mut observed_revision_lag = None;
        let mut stale_result_dropped = false;
        let mut completed_revision = None;
        let polled_result = self
            .async_sorter
            .as_mut()
            .ok_or(RendererError::SurfaceWorker)?
            .poll_result();
        if let Some(result) = polled_result {
            let result = result?;
            completed_revision = Some(result.camera_revision);
            let revision_delta = self.camera_revision.saturating_sub(result.camera_revision);
            let revision_lag = u32::try_from(revision_delta).unwrap_or(u32::MAX);
            observed_revision_lag = Some(revision_lag);
            completed_timing = Some((result.preprocess_ms, result.sort_ms));
            if result.camera_revision >= self.applied_order_revision
                && revision_delta <= MAX_ASYNC_SORT_REVISION_LAG
                && async_order_pose_compatible(
                    &result.camera,
                    &self.camera,
                    self.async_sort_translation_limit,
                )
            {
                self.renderer
                    .replace_surface_sorted_indices(result.indices)?;
                self.frame_state.mark_external_order(revision_lag);
                self.applied_order_revision = result.camera_revision;
                self.applied_order_camera = result.camera;
                applied_order = true;
            } else {
                stale_result_dropped = true;
            }
        }

        if self.renderer.current_sorted_indices().is_empty() || self.frame_state.force_sort {
            return self.render_frame_sync();
        }

        let displayed_order_lag = self
            .camera_revision
            .saturating_sub(self.applied_order_revision);
        if displayed_order_lag > MAX_ASYNC_SORT_REVISION_LAG
            || !async_order_pose_compatible(
                &self.applied_order_camera,
                &self.camera,
                self.async_sort_translation_limit,
            )
        {
            let mut output = self.render_frame_sync()?;
            output.async_sort_revision_lag = observed_revision_lag;
            output.stale_async_sort_dropped = stale_result_dropped;
            output.async_sort_completed_revision = completed_revision;
            output.async_sort_result_applied = applied_order;
            output.sync_sort_fallback = true;
            return Ok(output);
        }

        let should_schedule = self.frame_state.camera_dirty
            && self.frame_state.camera_changes_since_sort
                >= async_schedule_threshold(self.sort_interval)
            && !self
                .async_sorter
                .as_ref()
                .is_some_and(SurfaceAsyncSorter::is_in_flight);
        let schedule_camera = self.camera;
        let schedule_revision = self.camera_revision;
        let (completed_projected_measurement, completed_projected_measurement_failure) =
            self.collect_projected_draw_measurements();
        let (completed_gpu_producer_measurement, completed_gpu_producer_measurement_failure) =
            self.collect_gpu_producer_measurements();
        self.refresh_adaptive_probe_owner();
        let compact_available = self.presenter.projected_contributor_indirect_draw_enabled();
        let projected_choice =
            self.pending_projected_choice
                .unwrap_or_else(|| match self.projected_draw_policy {
                    SurfaceProjectedDrawPolicy::Candidate => ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Candidate,
                        sample: None,
                    },
                    SurfaceProjectedDrawPolicy::Compact => ProjectedAdaptiveChoice {
                        execution: SurfaceProjectedDrawExecution::Compact,
                        sample: None,
                    },
                    SurfaceProjectedDrawPolicy::Adaptive => {
                        self.adaptive_projected_cpu.choose(compact_available)
                    }
                });
        if self.adaptive_probe_owner.is_none() && projected_choice.sample.is_some() {
            self.adaptive_probe_owner = Some(AdaptiveProbeOwner::ProjectedCpu);
        }
        let plan = SurfaceFramePlan {
            refresh_sort: false,
            upload_order: self.frame_state.order_upload_dirty,
        };
        let mut output = self.render_with_plan(plan, applied_order, false, projected_choice)?;
        output.completed_projected_draw_measurement = completed_projected_measurement;
        output.completed_projected_draw_measurement_failure =
            completed_projected_measurement_failure;
        output.completed_gpu_producer_measurement = completed_gpu_producer_measurement;
        output.completed_gpu_producer_measurement_failure =
            completed_gpu_producer_measurement_failure;
        if output.frame_presented {
            let defer_projected_choice = defer_projected_formal_choice(
                projected_choice,
                output.sort_refreshed || output.order_uploaded,
            );
            self.pending_projected_choice = defer_projected_choice.then_some(projected_choice);
            if !defer_projected_choice {
                match projected_choice.sample {
                    None => {}
                    Some(ProjectedAdaptiveSampleKind::TransitionWarmup) => self
                        .adaptive_projected_cpu
                        .complete_synchronous_sample(projected_choice),
                    Some(
                        ProjectedAdaptiveSampleKind::CandidateBootstrap
                        | ProjectedAdaptiveSampleKind::Probe(_),
                    ) => {
                        if let Some(ticket) = output.projected_draw_measurement_submission.ticket()
                        {
                            self.adaptive_projected_cpu.register_pending_sample(
                                SurfaceOrderBackendUsed::Cpu,
                                projected_choice,
                                ticket,
                            );
                        }
                    }
                }
            }
        } else {
            self.pending_projected_choice = Some(projected_choice);
        }
        output.projected_draw_adaptive_state =
            self.projected_draw_adaptive_state(SurfaceOrderBackendUsed::Cpu);

        if let Some((preprocess_ms, sort_ms)) = completed_timing {
            output.stats.preprocess_ms = preprocess_ms;
            output.stats.sort_ms = sort_ms;
            self.last_stats = output.stats;
        }
        if should_schedule {
            self.async_sorter
                .as_mut()
                .ok_or(RendererError::SurfaceWorker)?
                .start(schedule_camera, schedule_revision);
        }
        output.async_sort_revision_lag = observed_revision_lag;
        output.stale_async_sort_dropped = stale_result_dropped;
        output.async_sort_scheduled = should_schedule;
        output.camera_revision = self.camera_revision;
        output.applied_order_revision = self.applied_order_revision;
        output.presented_order_revision_lag = u32::try_from(
            self.camera_revision
                .saturating_sub(self.applied_order_revision),
        )
        .unwrap_or(u32::MAX);
        output.async_sort_scheduled_revision = should_schedule.then_some(schedule_revision);
        output.async_sort_completed_revision = completed_revision;
        output.async_sort_result_applied = applied_order;
        Ok(output)
    }
}

fn paged_surface_counts(source_count: usize, drawn_count: u32) -> (u32, u32) {
    (u32::try_from(source_count).unwrap_or(u32::MAX), drawn_count)
}

#[cfg(not(target_arch = "wasm32"))]
fn sort_positions_for_camera(
    input: &OwnedCpuOrderInput,
    camera_revision: u64,
) -> Result<AsyncSortResult, RendererError> {
    let positions = input.positions();
    let camera = input.camera();
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    let preprocess_start = std::time::Instant::now();
    let view_rotation = crate::quat_to_mat3(crate::quat_inverse(camera.pose.rotation_xyzw));
    let depth_row = view_rotation[2];
    let camera_position = camera.pose.position;
    let mut depth_keys = Vec::with_capacity(positions.len());
    let mut indices = Vec::with_capacity(positions.len());

    for (index, position) in positions.iter().enumerate() {
        let depth =
            crate::world_to_camera_depth_with_view_row(*position, camera_position, depth_row);
        if depth >= camera.intrinsics.near_plane && depth <= camera.intrinsics.far_plane {
            indices.push(index as u32);
            depth_keys.push(depth.max(0.0).to_bits());
        }
    }
    let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

    let sort_start = std::time::Instant::now();
    CpuSortBackend::default().sort_values_by_keys(&depth_keys, &mut indices)?;
    let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

    Ok(AsyncSortResult {
        indices,
        preprocess_ms,
        sort_ms,
        camera_revision,
        camera,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn async_order_pose_compatible(
    order_camera: &Camera,
    current_camera: &Camera,
    translation_limit: f32,
) -> bool {
    let dx = current_camera.pose.position.x - order_camera.pose.position.x;
    let dy = current_camera.pose.position.y - order_camera.pose.position.y;
    let dz = current_camera.pose.position.z - order_camera.pose.position.z;
    if dx * dx + dy * dy + dz * dz > translation_limit * translation_limit {
        return false;
    }
    let a = order_camera.pose.rotation_xyzw;
    let b = current_camera.pose.rotation_xyzw;
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3])
        .abs()
        .clamp(0.0, 1.0);
    2.0 * dot.acos() <= MAX_ASYNC_SORT_ROTATION_DELTA_RADIANS
}

#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_GPU_FAILURE_COOLDOWN, ADAPTIVE_INITIAL_PROBE_DELAY, ADAPTIVE_PROBE_SEQUENCE_LEN,
        ADAPTIVE_REPROBE_INTERVAL, AdaptiveMetric, AdaptiveOrderPolicy, AdaptiveProbeOwner,
        AdaptiveProjectedDrawPolicy, AdaptiveRefreshChoice, AdaptiveSampleKind,
        MAX_ASYNC_SORT_REVISION_LAG, PROJECTED_TELEMETRY_FAILURE_COOLDOWN,
        ProjectedAdaptivePendingSample, ProjectedAdaptivePhase, ProjectedAdaptiveSampleKind,
        SurfaceAdaptiveState, SurfaceFrameState, SurfaceGeometrySwitchEntry, SurfaceOrderBackend,
        SurfaceOrderBackendUsed, SurfaceOrderMeasurementSubmission,
        SurfaceProjectedDrawAdaptiveState, SurfaceProjectedDrawMeasurementSubmission,
        SurfaceProjectedDrawPolicy, SurfaceSortSchedule, TelemetrySubmission,
        adaptive_gpu_order_failure_reason, adaptive_primary_metric, arbitrate_new_probe_owner,
        async_order_pose_compatible, async_schedule_threshold, defer_projected_formal_choice,
        gpu_producer_measurement_context_is_valid, gpu_projected_order_changed,
        order_probe_owner_should_yield, paged_surface_counts, probe_sequence_backend,
        projected_formal_sample_requested, projected_order_changed, projected_policy_can_sample,
        projected_probe_claims_owner, projected_probe_sequence_execution,
        reset_adaptive_for_gpu_producer_measurement_transition,
        reset_adaptive_for_raster_transition, retain_gpu_producer_terminal,
        should_measure_cpu_refresh, should_reset_order_for_projected_incumbent_change,
        surface_geometry_switch_entry, try_switch_renderer_geometry_path,
        validate_gpu_order_producer_transition, validate_projected_draw_policy_transition,
    };
    use crate::{
        GeometryPath, Renderer, RendererError, ResidentGpuError, SurfaceGpuOrderProducer,
        SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
        SurfaceOrderMeasurementFailureReason, SurfacePresenterError, SurfaceProjectedDrawExecution,
        SurfaceProjectedDrawMeasurementFailure, SurfaceProjectedDrawMeasurementFailureReason,
        SurfaceRasterExecutionPlan, SurfaceTimingSource,
    };
    use gsplat_core::{Camera, RendererConfig, SceneBuffers, Vec3f};
    use std::collections::VecDeque;

    #[test]
    fn sort_schedule_exposes_interval_for_sync_and_async_policies() {
        assert_eq!(SurfaceSortSchedule::Interval(2).interval(), 2);
        assert_eq!(
            SurfaceSortSchedule::AsyncLatest { interval: 3 }.interval(),
            3
        );
    }

    #[test]
    fn browser_geometry_switch_contract_is_idempotent_async_and_paged_fail_closed() {
        assert_eq!(
            surface_geometry_switch_entry(
                true,
                GeometryPath::PagedActiveAtlas,
                GeometryPath::PagedActiveAtlas,
            ),
            SurfaceGeometrySwitchEntry::AlreadyActive,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                true,
                GeometryPath::SortedIndexDirect,
                GeometryPath::PackedAtlas,
            ),
            SurfaceGeometrySwitchEntry::AsyncPreparationRequired,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                true,
                GeometryPath::PackedAtlas,
                GeometryPath::SortedIndexDirect,
            ),
            SurfaceGeometrySwitchEntry::AsyncPreparationRequired,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                true,
                GeometryPath::SortedIndexDirect,
                GeometryPath::PagedActiveAtlas,
            ),
            SurfaceGeometrySwitchEntry::Unsupported,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                true,
                GeometryPath::PagedActiveAtlas,
                GeometryPath::PackedAtlas,
            ),
            SurfaceGeometrySwitchEntry::Unsupported,
        );
        assert_eq!(
            surface_geometry_switch_entry(
                false,
                GeometryPath::SortedIndexDirect,
                GeometryPath::PagedActiveAtlas,
            ),
            SurfaceGeometrySwitchEntry::Synchronous,
        );
    }

    #[test]
    fn every_non_paged_cpu_refresh_uses_completion_telemetry() {
        let refresh = super::SurfaceFramePlan {
            refresh_sort: true,
            upload_order: true,
        };
        let reuse = super::SurfaceFramePlan {
            refresh_sort: false,
            upload_order: false,
        };
        assert!(should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::SortedIndexDirect,
        ));
        assert!(should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::PackedAtlas,
        ));
        assert!(!should_measure_cpu_refresh(
            reuse,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::PackedAtlas,
        ));
        assert!(!should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Gpu,
            GeometryPath::PackedAtlas,
        ));
        assert!(!should_measure_cpu_refresh(
            refresh,
            SurfaceOrderBackendUsed::Cpu,
            GeometryPath::PagedActiveAtlas,
        ));
    }

    #[test]
    fn gpu_order_preparation_exposes_no_formal_measurement_identity() {
        let submission = SurfaceOrderMeasurementSubmission::from_presenter(
            SurfaceOrderBackendUsed::Gpu,
            TelemetrySubmission::GpuOrderPreparationPending,
        );
        assert_eq!(submission, SurfaceOrderMeasurementSubmission::NotRequested);
        assert_eq!(submission.ticket(), None);
    }

    #[test]
    fn adaptive_fallback_only_accepts_gpu_order_capability_failures() {
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::GpuOrderUnsupported
            )),
            Some(super::SurfaceAdaptiveGpuFailureReason::Unsupported)
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::GpuOrderPreparationRequired
            )),
            None
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::ResidentGpu(ResidentGpuError::GpuOrderOutOfMemory(
                    "injected".into()
                ))
            )),
            Some(super::SurfaceAdaptiveGpuFailureReason::OutOfMemory)
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::SurfaceOutOfMemory
            )),
            None
        );
        assert_eq!(
            adaptive_gpu_order_failure_reason(&RendererError::SurfacePresenter(
                SurfacePresenterError::ResidentGpu(ResidentGpuError::GpuOrderInternal(
                    "injected".into()
                ))
            )),
            None
        );
    }

    #[test]
    fn paged_surface_counts_report_source_total_and_active_drawn() {
        assert_eq!(paged_surface_counts(279_199, 262_144), (279_199, 262_144));
    }

    #[test]
    fn failed_presenter_prepare_rolls_renderer_back_to_working_path() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0), Vec3f::new(0.1, 0.0, 1.2)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[-3.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.load_scene(scene).unwrap();
        assert_eq!(renderer.world_covariances.as_ref().map(Vec::len), Some(2));

        let result = try_switch_renderer_geometry_path(
            &mut renderer,
            GeometryPath::PagedActiveAtlas,
            |prepared| {
                assert_eq!(prepared.geometry_path(), GeometryPath::PagedActiveAtlas);
                assert!(prepared.world_covariances.is_none());
                assert!(prepared.spatial_pages.is_some());
                Err::<(), _>("injected presenter allocation failure")
            },
        );

        assert_eq!(result, Err("injected presenter allocation failure"));
        assert_eq!(renderer.geometry_path(), GeometryPath::SortedIndexDirect);
        assert_eq!(renderer.world_covariances.as_ref().map(Vec::len), Some(2));
        assert!(renderer.spatial_pages.is_none());
    }

    #[test]
    fn released_packed_scene_rejects_wide_path_switches_and_rolls_back() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 1.0), Vec3f::new(0.1, 0.0, 1.2)],
            opacity: vec![1.0; 2],
            scale_xyz: vec![[-3.0; 3]; 2],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
            color_dc: vec![[0.0; 3]; 2],
            sh_degree: 0,
            sh_rest: None,
        };
        let expected_positions = scene.positions.clone();
        let mut renderer = Renderer::with_config_for_surface(RendererConfig::default()).unwrap();
        renderer.set_geometry_path(GeometryPath::PackedAtlas);
        renderer.load_scene(scene).unwrap();
        renderer
            .finish_surface_upload_handoff(GeometryPath::PackedAtlas)
            .unwrap();

        for target in [
            GeometryPath::SortedIndexDirect,
            GeometryPath::PagedActiveAtlas,
        ] {
            let result = try_switch_renderer_geometry_path(&mut renderer, target, |prepared| {
                prepared
                    .scene()
                    .map(|_| ())
                    .ok_or(SurfacePresenterError::GeometrySourceUnavailable { path: target })
            });

            assert!(matches!(
                result,
                Err(SurfacePresenterError::GeometrySourceUnavailable { path }) if path == target
            ));
            assert_eq!(renderer.geometry_path(), GeometryPath::PackedAtlas);
            assert_eq!(renderer.positions(), Some(expected_positions.as_slice()));
            assert!(renderer.has_scene());
            assert!(!renderer.resident_scene().unwrap().has_upload_staging());
        }
    }

    #[test]
    fn first_frame_forces_sort_and_order_upload() {
        let plan = SurfaceFrameState::default().plan(false, 2);

        assert!(plan.refresh_sort);
        assert!(plan.upload_order);
    }

    #[test]
    fn stationary_frame_reuses_order_without_resorting() {
        let mut state = SurfaceFrameState::default();
        let first = state.plan(false, 2);
        state.finish_frame(first, true);

        let stationary = state.plan(true, 2);
        assert!(!stationary.refresh_sort);
        assert!(!stationary.upload_order);
    }

    #[test]
    fn deferred_camera_change_catches_up_on_the_next_stationary_frame() {
        let mut state = SurfaceFrameState::default();
        let first = state.plan(false, 2);
        state.finish_frame(first, true);

        state.mark_camera_changed();
        let first_change = state.plan(true, 2);
        assert!(!first_change.refresh_sort);
        state.finish_frame(first_change, false);

        let stationary = state.plan(true, 2);
        assert!(stationary.refresh_sort);
        assert!(stationary.upload_order);
        state.finish_frame(stationary, true);

        let caught_up = state.plan(true, 2);
        assert!(!caught_up.refresh_sort);
        assert!(!caught_up.upload_order);
    }

    #[test]
    fn continuous_camera_changes_refresh_at_the_requested_interval() {
        let mut state = SurfaceFrameState::default();
        let first = state.plan(false, 2);
        state.finish_frame(first, true);

        state.mark_camera_changed();
        let first_change = state.plan(true, 2);
        assert!(!first_change.refresh_sort);
        state.finish_frame(first_change, false);

        state.mark_camera_changed();
        let second_change = state.plan(true, 2);
        assert!(second_change.refresh_sort);
        assert!(second_change.upload_order);
    }

    #[test]
    fn async_sort_revision_lag_is_explicitly_bounded() {
        assert_eq!(MAX_ASYNC_SORT_REVISION_LAG, 2);
    }

    #[test]
    fn async_sort_pose_envelope_accepts_slow_motion_and_rejects_jumps() {
        let order = Camera::default();
        let mut current = order;
        current.pose.position = Vec3f::new(0.001, 0.0, 0.0);
        current.pose.rotation_xyzw = [0.0, -(0.002_f32 * 0.5).sin(), 0.0, (0.002_f32 * 0.5).cos()];
        assert!(async_order_pose_compatible(&order, &current, 0.01));

        current.pose.position = Vec3f::new(0.02, 0.0, 0.0);
        assert!(!async_order_pose_compatible(&order, &current, 0.01));
        current.pose.position = order.pose.position;
        current.pose.rotation_xyzw = [0.0, -(0.02_f32 * 0.5).sin(), 0.0, (0.02_f32 * 0.5).cos()];
        assert!(!async_order_pose_compatible(&order, &current, 0.01));
    }

    #[test]
    fn async_sort_starts_one_revision_before_interval_boundary() {
        assert_eq!(async_schedule_threshold(1), 1);
        assert_eq!(async_schedule_threshold(2), 1);
        assert_eq!(async_schedule_threshold(3), 2);
    }

    fn feed_cpu_bootstrap(policy: &mut AdaptiveOrderPolicy, sample_ms: f32) {
        policy.reset(AdaptiveMetric::OrderOnly);
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            let choice = policy.choose_refresh_backend();
            assert_eq!(choice.backend, SurfaceOrderBackendUsed::Cpu);
            assert_eq!(choice.sample, Some(AdaptiveSampleKind::CpuBootstrap));
            policy.complete_synchronous_sample(choice, sample_ms);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        for _ in 0..ADAPTIVE_INITIAL_PROBE_DELAY {
            let choice = policy.choose_refresh_backend();
            assert_eq!(choice.backend, SurfaceOrderBackendUsed::Cpu);
            assert_eq!(choice.sample, None);
        }
    }

    #[test]
    fn reapplying_the_same_raster_plan_preserves_adaptive_history() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        assert!(!reset_adaptive_for_raster_transition(
            &mut policy,
            crate::SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            crate::SurfaceRasterExecutionPlan::ProjectedQuadsExact,
        ));
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        assert!(reset_adaptive_for_raster_transition(
            &mut policy,
            crate::SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            crate::SurfaceRasterExecutionPlan::GlobalQuads,
        ));
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuLearning);
        assert_eq!(policy.metric(), adaptive_primary_metric());
    }

    fn complete_probe(policy: &mut AdaptiveOrderPolicy, cpu_ms: f32, gpu_ms: f32) {
        let mut sample_index = 0;
        while sample_index < ADAPTIVE_PROBE_SEQUENCE_LEN {
            let choice = policy.choose_refresh_backend();
            match choice.sample {
                Some(AdaptiveSampleKind::TransitionWarmup) => {
                    policy.complete_synchronous_sample(choice, 0.0);
                }
                Some(AdaptiveSampleKind::Probe(index)) => {
                    assert_eq!(index, sample_index);
                    let sample_ms = match choice.backend {
                        SurfaceOrderBackendUsed::Cpu => cpu_ms,
                        SurfaceOrderBackendUsed::Gpu => gpu_ms,
                    };
                    policy.complete_synchronous_sample(choice, sample_ms);
                    sample_index += 1;
                }
                other => panic!("unexpected probe choice: {other:?}"),
            }
        }
    }

    fn incomplete_timestamp_measurement(ticket: u64) -> SurfaceOrderMeasurement {
        SurfaceOrderMeasurement {
            ticket,
            camera_revision: 7,
            timing_source: SurfaceTimingSource::TimestampQuery,
            gpu_preprocess_ms: Some(0.25),
            gpu_radix_ms: None,
            gpu_order_ms: None,
            gpu_complete_ms: 4.0,
            timestamp_period_ns: Some(1.0),
            below_timestamp_resolution: false,
            visible_count: 3,
            contributor_count: 3,
            drawn_count: 3,
            exact_contributor_compaction: false,
        }
    }

    fn complete_timestamp_measurement(ticket: u64, order_ms: f32) -> SurfaceOrderMeasurement {
        SurfaceOrderMeasurement {
            ticket,
            camera_revision: 7,
            timing_source: SurfaceTimingSource::TimestampQuery,
            gpu_preprocess_ms: Some(order_ms * 0.25),
            gpu_radix_ms: Some(order_ms * 0.75),
            gpu_order_ms: Some(order_ms),
            gpu_complete_ms: order_ms + 100.0,
            timestamp_period_ns: Some(1.0),
            below_timestamp_resolution: false,
            visible_count: 3,
            contributor_count: 3,
            drawn_count: 3,
            exact_contributor_compaction: false,
        }
    }

    #[test]
    fn adaptive_contexts_start_with_end_to_end_completion_timing() {
        assert_eq!(adaptive_primary_metric(), AdaptiveMetric::FrameCompletion);
    }

    #[test]
    fn adaptive_probe_is_four_repeated_abba_blocks_with_eight_samples_per_backend() {
        let expected = [
            SurfaceOrderBackendUsed::Cpu,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceOrderBackendUsed::Gpu,
            SurfaceOrderBackendUsed::Cpu,
        ];
        let mut cpu = 0;
        let mut gpu = 0;
        for sample_index in 0..ADAPTIVE_PROBE_SEQUENCE_LEN {
            let backend = probe_sequence_backend(SurfaceOrderBackendUsed::Cpu, sample_index);
            assert_eq!(backend, expected[usize::from(sample_index % 4)]);
            match backend {
                SurfaceOrderBackendUsed::Cpu => cpu += 1,
                SurfaceOrderBackendUsed::Gpu => gpu += 1,
            }
        }
        assert_eq!((cpu, gpu), (8, 8));
    }

    #[test]
    fn transition_warmup_waits_for_completion_without_contaminating_the_metric() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);

        let cpu_probe = policy.choose_refresh_backend();
        assert_eq!(cpu_probe.backend, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(cpu_probe.sample, Some(AdaptiveSampleKind::Probe(0)));
        policy.complete_synchronous_sample(cpu_probe, 10.0);

        let warmup = policy.choose_refresh_backend();
        assert_eq!(warmup.backend, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(warmup.sample, Some(AdaptiveSampleKind::TransitionWarmup));
        policy.register_pending_sample(warmup, 77);
        assert_eq!(policy.metric(), AdaptiveMetric::OrderOnly);
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuProbe);
        assert_eq!(policy.pending.map(|pending| pending.ticket), Some(77));
        let while_pending = policy.choose_refresh_backend();
        assert_eq!(while_pending.backend, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(while_pending.sample, None);

        policy.observe_gpu_measurement(incomplete_timestamp_measurement(76));
        assert_eq!(policy.metric(), AdaptiveMetric::OrderOnly);
        assert_eq!(policy.pending.map(|pending| pending.ticket), Some(77));

        // Transition samples are queue barriers, not timing evidence, so a
        // missing timestamp interval cannot reset metric learning.
        policy.observe_gpu_measurement(incomplete_timestamp_measurement(77));
        assert_eq!(policy.metric(), AdaptiveMetric::OrderOnly);
        assert!(policy.pending.is_none());

        let gpu_probe = policy.choose_refresh_backend();
        assert_eq!(gpu_probe.backend, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(gpu_probe.sample, Some(AdaptiveSampleKind::Probe(1)));
        policy.register_pending_sample(gpu_probe, 78);

        // Even while a formal GPU probe is pending, a late warmup receipt is
        // diagnostic only because its ticket does not match.
        policy.observe_gpu_measurement(incomplete_timestamp_measurement(77));
        assert_eq!(policy.metric(), AdaptiveMetric::OrderOnly);
        assert_eq!(policy.pending.map(|pending| pending.ticket), Some(78));

        // The matching formal probe is the only receipt allowed to establish
        // that timestamp intervals are unusable on this context.
        policy.observe_gpu_measurement(incomplete_timestamp_measurement(78));
        assert_eq!(policy.metric(), AdaptiveMetric::FrameCompletion);
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuLearning);
        assert!(policy.pending.is_none());
    }

    #[test]
    fn complete_formal_gpu_timestamp_keeps_order_only_metric() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);

        let cpu_probe = policy.choose_refresh_backend();
        policy.complete_synchronous_sample(cpu_probe, 10.0);

        let warmup = policy.choose_refresh_backend();
        policy.complete_synchronous_sample(warmup, 0.0);

        let gpu_probe = policy.choose_refresh_backend();
        assert_eq!(gpu_probe.backend, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(gpu_probe.sample, Some(AdaptiveSampleKind::Probe(1)));
        policy.register_pending_sample(gpu_probe, 78);
        let while_pending = policy.choose_refresh_backend();
        assert_eq!(while_pending.backend, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(while_pending.sample, None);
        policy.observe_gpu_measurement(complete_timestamp_measurement(78, 5.0));

        assert_eq!(policy.metric(), AdaptiveMetric::OrderOnly);
        assert!(policy.pending.is_none());
        assert_eq!(policy.probe_gpu.p75(), Some(5.0));
        let next_probe = policy.choose_refresh_backend();
        assert_eq!(next_probe.backend, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(next_probe.sample, Some(AdaptiveSampleKind::Probe(2)));
    }

    #[test]
    fn adaptive_policy_keeps_cpu_when_gpu_probe_loses_and_backs_off() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        complete_probe(&mut policy, 10.0, 20.0);
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        for _ in 0..ADAPTIVE_REPROBE_INTERVAL {
            let choice = policy.choose_refresh_backend();
            assert_eq!(choice.backend, SurfaceOrderBackendUsed::Cpu);
            assert_eq!(choice.sample, None);
        }
        assert_eq!(
            policy.choose_refresh_backend().sample,
            Some(AdaptiveSampleKind::Probe(0))
        );
    }

    #[test]
    fn adaptive_policy_promotes_gpu_only_after_sustained_hysteresis_win() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 20.0);
        complete_probe(&mut policy, 20.0, 15.0);
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        assert_eq!(
            policy.choose_refresh_backend().backend,
            SurfaceOrderBackendUsed::Gpu
        );
    }

    #[test]
    fn adaptive_hysteresis_keeps_incumbent_for_a_small_nominal_win() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 20.0);
        complete_probe(&mut policy, 20.0, 18.5);
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
    }

    #[test]
    fn completion_probe_waits_for_the_matching_ticket_and_uses_incumbent_while_pending() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::FrameCompletion);
        for ticket in 1..=super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            let choice = policy.choose_refresh_backend();
            policy.register_pending_sample(choice, u64::from(ticket));
            assert_eq!(
                policy.pending_sample(),
                Some(super::SurfaceAdaptivePendingSample {
                    backend: SurfaceOrderBackendUsed::Cpu,
                    ticket: u64::from(ticket),
                })
            );
            let continuation = policy.choose_refresh_backend();
            assert_eq!(continuation.backend, SurfaceOrderBackendUsed::Cpu);
            assert_eq!(continuation.sample, None);
            assert!(!policy.complete_pending_sample(
                SurfaceOrderBackendUsed::Cpu,
                u64::from(ticket) + 100,
                10.0,
            ));
            assert!(policy.complete_pending_sample(
                SurfaceOrderBackendUsed::Cpu,
                u64::from(ticket),
                10.0,
            ));
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
    }

    #[test]
    fn invalid_completion_value_cannot_consume_a_formal_sample() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::FrameCompletion);
        let choice = policy.choose_refresh_backend();
        policy.register_pending_sample(choice, 2);

        assert!(!policy.complete_pending_sample(SurfaceOrderBackendUsed::Cpu, 2, f32::NAN,));
        assert_eq!(policy.pending_sample().map(|sample| sample.ticket), Some(2));
        assert!(policy.complete_pending_sample(SurfaceOrderBackendUsed::Cpu, 2, 10.0));
        assert!(policy.pending_sample().is_none());
    }

    #[test]
    fn cpu_terminal_failure_retries_the_same_formal_phase_without_sticking_pending() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::FrameCompletion);
        let choice = policy.choose_refresh_backend();
        policy.register_pending_sample(choice, 2);

        assert!(
            policy.observe_cpu_measurement_failure(SurfaceOrderMeasurementFailure {
                ticket: 2,
                camera_revision: 3,
                reason: SurfaceOrderMeasurementFailureReason::GenerationInvalidated,
            })
        );
        assert!(policy.pending_sample().is_none());
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuLearning);
        assert_eq!(
            policy.choose_refresh_backend().sample,
            Some(AdaptiveSampleKind::CpuBootstrap)
        );
    }

    #[test]
    fn adaptive_gpu_failure_enters_cpu_cooldown() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::OrderOnly);
        policy.gpu_failed();
        assert_eq!(policy.state(), SurfaceAdaptiveState::Cooldown);
        assert_eq!(
            policy.choose_refresh_backend().backend,
            SurfaceOrderBackendUsed::Cpu
        );
    }

    #[test]
    fn matching_gpu_terminal_failure_clears_pending_probe_and_enters_cooldown() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::FrameCompletion);
        policy.phase = Some(super::AdaptivePhase::Probe {
            incumbent: SurfaceOrderBackendUsed::Cpu,
            next_sample: 1,
            active_backend: SurfaceOrderBackendUsed::Gpu,
        });
        let choice = AdaptiveRefreshChoice {
            backend: SurfaceOrderBackendUsed::Gpu,
            sample: Some(AdaptiveSampleKind::Probe(1)),
        };
        policy.register_pending_sample(choice, 91);

        assert!(
            !policy.observe_gpu_measurement_failure(SurfaceOrderMeasurementFailure {
                ticket: 90,
                camera_revision: 7,
                reason: SurfaceOrderMeasurementFailureReason::ReadbackMap,
            })
        );
        assert!(policy.pending.is_some());
        assert!(
            policy.observe_gpu_measurement_failure(SurfaceOrderMeasurementFailure {
                ticket: 91,
                camera_revision: 7,
                reason: SurfaceOrderMeasurementFailureReason::ReadbackMap,
            })
        );
        assert!(policy.pending.is_none());
        assert_eq!(policy.state(), SurfaceAdaptiveState::Cooldown);
    }

    #[test]
    fn adaptive_gpu_failure_retries_after_one_cooldown_period() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::OrderOnly);
        policy.gpu_failed();
        for _ in 0..ADAPTIVE_GPU_FAILURE_COOLDOWN {
            assert_eq!(
                policy.choose_refresh_backend().backend,
                SurfaceOrderBackendUsed::Cpu
            );
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        let cpu_probe = policy.choose_refresh_backend();
        assert_eq!(cpu_probe.backend, SurfaceOrderBackendUsed::Cpu);
        assert_eq!(cpu_probe.sample, Some(AdaptiveSampleKind::Probe(0)));
        policy.complete_synchronous_sample(cpu_probe, 1.0);
        let gpu_transition = policy.choose_refresh_backend();
        assert_eq!(gpu_transition.backend, SurfaceOrderBackendUsed::Gpu);
        assert_eq!(
            gpu_transition.sample,
            Some(AdaptiveSampleKind::TransitionWarmup)
        );
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuProbe);
    }

    #[test]
    fn context_reset_changes_metric_and_discards_pending_probe() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset(AdaptiveMetric::FrameCompletion);
        let choice = policy.choose_refresh_backend();
        policy.register_pending_sample(choice, 7);
        assert!(policy.pending.is_some());

        policy.reset(AdaptiveMetric::OrderOnly);
        assert_eq!(policy.metric(), AdaptiveMetric::OrderOnly);
        assert!(policy.pending.is_none());
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuLearning);
    }

    #[test]
    fn projected_abba_sequence_is_independent_from_order_backend() {
        for incumbent in [
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceProjectedDrawExecution::Compact,
        ] {
            assert_eq!(
                (0..4)
                    .map(|index| projected_probe_sequence_execution(incumbent, index))
                    .collect::<Vec<_>>(),
                [
                    incumbent,
                    match incumbent {
                        SurfaceProjectedDrawExecution::Candidate => {
                            SurfaceProjectedDrawExecution::Compact
                        }
                        SurfaceProjectedDrawExecution::Compact => {
                            SurfaceProjectedDrawExecution::Candidate
                        }
                    },
                    match incumbent {
                        SurfaceProjectedDrawExecution::Candidate => {
                            SurfaceProjectedDrawExecution::Compact
                        }
                        SurfaceProjectedDrawExecution::Compact => {
                            SurfaceProjectedDrawExecution::Candidate
                        }
                    },
                    incumbent,
                ],
            );
        }
    }

    #[test]
    fn projected_hysteresis_promotes_an_eight_percent_compact_gain() {
        let mut policy = AdaptiveProjectedDrawPolicy {
            phase: ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                next_sample: 0,
                active_execution: SurfaceProjectedDrawExecution::Candidate,
            },
            ..AdaptiveProjectedDrawPolicy::default()
        };
        for _ in 0..8 {
            policy.probe_candidate.push(100.0);
            policy.probe_compact.push(91.2);
        }
        policy.finish_probe(SurfaceProjectedDrawExecution::Candidate);
        assert_eq!(
            policy.state(),
            SurfaceProjectedDrawAdaptiveState::CompactStable,
        );
    }

    #[test]
    fn projected_telemetry_failure_backs_off_instead_of_retrying_each_frame() {
        let mut policy = AdaptiveProjectedDrawPolicy::default();
        let choice = policy.choose(true);
        assert_eq!(
            choice.sample,
            Some(ProjectedAdaptiveSampleKind::CandidateBootstrap),
        );
        policy.register_pending_sample(SurfaceOrderBackendUsed::Cpu, choice, 101);
        assert!(
            policy.observe_failure(SurfaceProjectedDrawMeasurementFailure {
                ticket: 101,
                camera_revision: 4,
                execution: SurfaceProjectedDrawExecution::Candidate,
                order_backend: SurfaceOrderBackendUsed::Cpu,
                projection_generation: 9,
                probe_generation: 3,
                reason: SurfaceProjectedDrawMeasurementFailureReason::ReadbackMap,
            })
        );
        assert_eq!(policy.state(), SurfaceProjectedDrawAdaptiveState::Cooldown);
        for _ in 0..PROJECTED_TELEMETRY_FAILURE_COOLDOWN - 1 {
            assert!(policy.choose(true).sample.is_none());
        }
        assert!(policy.choose(true).sample.is_none());
        assert_eq!(
            policy.state(),
            SurfaceProjectedDrawAdaptiveState::CandidateStable,
        );
    }

    #[test]
    fn projected_cpu_and_gpu_lanes_keep_separate_evidence() {
        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let gpu = AdaptiveProjectedDrawPolicy::default();
        cpu.complete_sample(
            ProjectedAdaptiveSampleKind::CandidateBootstrap,
            SurfaceProjectedDrawExecution::Candidate,
            12.0,
        );
        assert_eq!(cpu.candidate_baseline.len, 1);
        assert_eq!(gpu.candidate_baseline.len, 0);
    }

    #[test]
    fn first_changed_then_cached_order_preserves_one_projected_grace_turn() {
        let mut lane = AdaptiveProjectedDrawPolicy::default();
        let formal = lane.choose(true);
        let projected_owner = AdaptiveProbeOwner::ProjectedCpu;

        // On the first required sort, the formal projected choice cannot be
        // measured yet. It nevertheless owns one grace turn and is retained
        // as pending so a static second frame can issue the exact ticket.
        assert!(!projected_formal_sample_requested(formal, true));
        assert!(defer_projected_formal_choice(formal, true));
        assert!(projected_probe_claims_owner(formal, true, false));
        let owner = arbitrate_new_probe_owner(None, true, true, projected_owner);
        assert_eq!(owner, Some(projected_owner));

        // The retained choice becomes eligible when the next frame reuses the
        // exact same order. Projected remains owner and the blocked order
        // sample cannot contaminate its timing cohort.
        assert!(projected_formal_sample_requested(formal, false));
        assert!(projected_probe_claims_owner(formal, false, true));
        assert_eq!(owner, Some(projected_owner));
        assert!(!projected_policy_can_sample(
            Some(AdaptiveProbeOwner::Order),
            projected_owner,
            true,
        ));
    }

    #[test]
    fn repeated_changed_orders_release_projected_grace_and_advance_order_learning() {
        let mut projected = AdaptiveProjectedDrawPolicy::default();
        let mut formal = projected.choose(true);
        let projected_owner = AdaptiveProbeOwner::ProjectedCpu;
        let mut pending_projected_choice = Some(formal);

        let mut owner = arbitrate_new_probe_owner(
            None,
            true,
            projected_probe_claims_owner(formal, true, false),
            projected_owner,
        );
        assert_eq!(owner, Some(projected_owner));

        // The same pending formal choice sees a second order change. Its one
        // grace turn is exhausted, so production releases Projected and Order
        // receives the still-unconsumed bootstrap sample.
        assert!(defer_projected_formal_choice(formal, true));
        if owner == Some(projected_owner) && pending_projected_choice.is_some() {
            owner = None;
            pending_projected_choice = None;
            formal.sample = None;
        }
        assert!(pending_projected_choice.is_none());
        assert!(formal.sample.is_none());
        assert!(!projected_probe_claims_owner(formal, true, true));
        owner = arbitrate_new_probe_owner(owner, true, false, projected_owner);
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));

        // Once Order owns the cohort, a finite number of successful bootstrap
        // receipts necessarily leaves CpuLearning instead of livelocking.
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            let choice = order.choose_refresh_backend();
            assert_eq!(choice.sample, Some(AdaptiveSampleKind::CpuBootstrap));
            order.complete_synchronous_sample(choice, 10.0);
        }
        assert_eq!(order.state(), SurfaceAdaptiveState::CpuStable);
    }

    #[test]
    fn yielded_projected_choice_stays_unsampled_until_order_cohort_releases() {
        let mut projected = AdaptiveProjectedDrawPolicy::default();
        let formal = projected.choose(true);
        let projected_owner = AdaptiveProbeOwner::ProjectedCpu;

        // refresh -> refresh: the first frame grants grace; the second yields
        // to Order and removes the session-level pending ticket.
        let mut owner = Some(projected_owner);
        let mut pending_projected_choice = Some(formal);
        let mut yielded = pending_projected_choice.expect("grace choice");
        if owner == Some(projected_owner)
            && defer_projected_formal_choice(yielded, true)
            && pending_projected_choice.is_some()
        {
            owner = None;
            pending_projected_choice = None;
            yielded.sample = None;
        }
        owner = arbitrate_new_probe_owner(owner, true, false, projected_owner);
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));
        assert!(pending_projected_choice.is_none());
        assert!(yielded.sample.is_none());

        // A following stable frame cannot steal ownership while the Order
        // ticket from the second refresh is still pending.
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        let order_choice = order.choose_refresh_backend();
        order.register_pending_sample(order_choice, 77);
        assert!(!order_probe_owner_should_yield(
            owner,
            order.pending.is_some(),
            false,
            false,
        ));
        let held = projected.held_choice();
        assert!(held.sample.is_none());
        assert!(!projected_probe_claims_owner(held, false, false));
        assert_eq!(owner, Some(AdaptiveProbeOwner::Order));
        assert!(projected.pending.is_none());

        // Once that ticket is terminal, Order has no work on a stable frame
        // and yields without losing its CpuLearning phase or first sample.
        assert!(order.complete_pending_sample(SurfaceOrderBackendUsed::Cpu, 77, 10.0,));
        assert!(order_probe_owner_should_yield(
            owner,
            order.pending.is_some(),
            false,
            false,
        ));
        owner = None;
        assert_eq!(order.state(), SurfaceAdaptiveState::CpuLearning);

        // Projected now reissues the exact same unconsumed bootstrap kind and
        // the cached order makes its formal ticket eligible.
        let reissued = projected.choose(true);
        assert_eq!(reissued.sample, formal.sample);
        assert!(projected_formal_sample_requested(reissued, false));
        owner = arbitrate_new_probe_owner(
            owner,
            false,
            projected_probe_claims_owner(reissued, false, false),
            projected_owner,
        );
        assert_eq!(owner, Some(projected_owner));
    }

    #[test]
    fn projected_transition_warmup_delays_order_once_without_claiming_owner() {
        let mut lane = AdaptiveProjectedDrawPolicy {
            phase: ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                next_sample: 1,
                active_execution: SurfaceProjectedDrawExecution::Candidate,
            },
            ..AdaptiveProjectedDrawPolicy::default()
        };
        let transition_warmup = lane.choose(true);
        assert!(!projected_formal_sample_requested(transition_warmup, false));
        assert!(!projected_probe_claims_owner(
            transition_warmup,
            false,
            false,
        ));
        assert_eq!(
            arbitrate_new_probe_owner(None, true, false, AdaptiveProbeOwner::ProjectedCpu),
            Some(AdaptiveProbeOwner::Order),
        );
    }

    #[test]
    fn projected_formal_ticket_requires_a_cached_order_and_forced_reprojection() {
        let mut lane = AdaptiveProjectedDrawPolicy::default();
        let formal = lane.choose(true);
        for (refresh_sort, upload_order, actual_sort_refreshed) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let changed =
                projected_order_changed(refresh_sort, upload_order, actual_sort_refreshed);
            assert!(changed);
            assert!(!projected_formal_sample_requested(formal, changed));
            assert!(defer_projected_formal_choice(formal, changed));
        }
        let cached = projected_order_changed(false, false, false);
        assert!(projected_formal_sample_requested(formal, cached));
        assert!(!defer_projected_formal_choice(formal, cached));

        lane.phase = ProjectedAdaptivePhase::Probe {
            incumbent: SurfaceProjectedDrawExecution::Candidate,
            next_sample: 1,
            active_execution: SurfaceProjectedDrawExecution::Candidate,
        };
        let transition_warmup = lane.choose(true);
        assert_eq!(
            transition_warmup.sample,
            Some(ProjectedAdaptiveSampleKind::TransitionWarmup)
        );
        assert!(!projected_formal_sample_requested(transition_warmup, false));
        assert!(!defer_projected_formal_choice(transition_warmup, true));
    }

    #[test]
    fn deferred_cpu_upload_does_not_block_cached_gpu_projected_ticket() {
        let second_gpu_plan = super::SurfaceFramePlan {
            refresh_sort: false,
            // The first GPU frame intentionally leaves this dirty for a
            // future CPU switch; it is not part of the GPU order identity.
            upload_order: true,
        };
        let mut lane = AdaptiveProjectedDrawPolicy::default();
        let formal = lane.choose(true);
        let changed = gpu_projected_order_changed(second_gpu_plan.refresh_sort, false);
        assert!(!changed);
        assert!(projected_formal_sample_requested(formal, changed));
        assert!(!defer_projected_formal_choice(formal, changed));
    }

    #[test]
    fn cpu_and_gpu_projected_lanes_stabilize_before_their_order_samples() {
        let order_choice = AdaptiveRefreshChoice {
            backend: SurfaceOrderBackendUsed::Cpu,
            sample: Some(AdaptiveSampleKind::CpuBootstrap),
        };
        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let cpu_choice = cpu.choose(true);
        assert!(order_choice.sample.is_some());
        assert!(cpu_choice.sample.is_some());
        assert_eq!(
            arbitrate_new_probe_owner(None, true, true, AdaptiveProbeOwner::ProjectedCpu,),
            Some(AdaptiveProbeOwner::ProjectedCpu),
        );
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            cpu.complete_sample(
                ProjectedAdaptiveSampleKind::CandidateBootstrap,
                SurfaceProjectedDrawExecution::Candidate,
                10.0,
            );
        }
        assert!(
            cpu.cohort_active(),
            "candidate bootstrap alone must not release the order owner"
        );
        cpu.phase = ProjectedAdaptivePhase::Probe {
            incumbent: SurfaceProjectedDrawExecution::Candidate,
            next_sample: 0,
            active_execution: SurfaceProjectedDrawExecution::Candidate,
        };
        for _ in 0..8 {
            cpu.probe_candidate.push(100.0);
            cpu.probe_compact.push(85.0);
        }
        cpu.finish_probe(SurfaceProjectedDrawExecution::Candidate);
        assert_eq!(
            cpu.state(),
            SurfaceProjectedDrawAdaptiveState::CompactStable
        );
        assert!(!cpu.cohort_active());

        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        let gpu_choice = gpu.choose(true);
        assert!(gpu_choice.sample.is_some());
        assert_eq!(
            arbitrate_new_probe_owner(None, true, true, AdaptiveProbeOwner::ProjectedGpu,),
            Some(AdaptiveProbeOwner::ProjectedGpu),
        );
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            gpu.complete_sample(
                ProjectedAdaptiveSampleKind::CandidateBootstrap,
                SurfaceProjectedDrawExecution::Candidate,
                18.0,
            );
        }
        assert!(
            gpu.cohort_active(),
            "the GPU-order lane also owns through its initial Compact probe"
        );
        gpu.phase = ProjectedAdaptivePhase::Probe {
            incumbent: SurfaceProjectedDrawExecution::Candidate,
            next_sample: 0,
            active_execution: SurfaceProjectedDrawExecution::Candidate,
        };
        for _ in 0..8 {
            gpu.probe_candidate.push(140.0);
            gpu.probe_compact.push(110.0);
        }
        gpu.finish_probe(SurfaceProjectedDrawExecution::Candidate);
        assert_eq!(
            gpu.state(),
            SurfaceProjectedDrawAdaptiveState::CompactStable
        );
        assert!(!gpu.cohort_active());
        assert_eq!(
            arbitrate_new_probe_owner(None, true, false, AdaptiveProbeOwner::ProjectedGpu,),
            Some(AdaptiveProbeOwner::Order),
            "order gets a formal ticket only after the target lane is stable",
        );
    }

    #[test]
    fn projected_unsampled_submission_does_not_advance_the_policy() {
        let mut policy = AdaptiveProjectedDrawPolicy::default();
        let first = policy.choose(true);
        assert_eq!(
            first.sample,
            Some(ProjectedAdaptiveSampleKind::CandidateBootstrap),
        );
        for submission in [
            TelemetrySubmission::RingBusy,
            TelemetrySubmission::SurfaceUnavailable,
        ] {
            assert!(matches!(
                SurfaceProjectedDrawMeasurementSubmission::from_presenter(
                    first.execution,
                    submission,
                ),
                SurfaceProjectedDrawMeasurementSubmission::Unsampled { .. }
            ));
            assert_eq!(policy.choose(true), first);
        }
    }

    #[test]
    fn projected_incumbent_change_resets_order_only_at_a_clear_owner_boundary() {
        assert!(should_reset_order_for_projected_incumbent_change(
            SurfaceOrderBackend::Adaptive,
            SurfaceProjectedDrawPolicy::Adaptive,
            AdaptiveMetric::FrameCompletion,
            false,
            true,
            true,
        ));
        assert!(!should_reset_order_for_projected_incumbent_change(
            SurfaceOrderBackend::Adaptive,
            SurfaceProjectedDrawPolicy::Adaptive,
            AdaptiveMetric::FrameCompletion,
            true,
            true,
            true,
        ));
        assert!(!should_reset_order_for_projected_incumbent_change(
            SurfaceOrderBackend::Adaptive,
            SurfaceProjectedDrawPolicy::Adaptive,
            AdaptiveMetric::FrameCompletion,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn forced_projected_policy_validation_is_transactional_and_idempotent() {
        assert!(matches!(
            validate_projected_draw_policy_transition(
                SurfaceProjectedDrawPolicy::Adaptive,
                SurfaceProjectedDrawPolicy::Adaptive,
                false,
            ),
            Ok(false),
        ));
        assert!(matches!(
            validate_projected_draw_policy_transition(
                SurfaceProjectedDrawPolicy::Candidate,
                SurfaceProjectedDrawPolicy::Compact,
                false,
            ),
            Err(SurfacePresenterError::ProjectedCompactionUnsupported),
        ));
        assert!(matches!(
            validate_projected_draw_policy_transition(
                SurfaceProjectedDrawPolicy::Candidate,
                SurfaceProjectedDrawPolicy::Adaptive,
                false,
            ),
            Ok(true),
        ));
    }

    #[test]
    fn preproject_selector_is_idempotent_and_rejects_every_incompatible_context() {
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::Preproject,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::SortedIndexDirect,
                SurfaceRasterExecutionPlan::GlobalQuads,
                SurfaceProjectedDrawPolicy::Candidate,
            ),
            Ok(false),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::SortedIndexDirect,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Err(SurfacePresenterError::PreprojectProducerIncompatible),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::GlobalQuads,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Err(SurfacePresenterError::PreprojectProducerIncompatible),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Adaptive,
            ),
            Err(SurfacePresenterError::PreprojectProducerIncompatible),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::PostSort,
                SurfaceGpuOrderProducer::Preproject,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Ok(true),
        ));
        assert!(matches!(
            validate_gpu_order_producer_transition(
                SurfaceGpuOrderProducer::Preproject,
                SurfaceGpuOrderProducer::PostSort,
                GeometryPath::PackedAtlas,
                SurfaceRasterExecutionPlan::ProjectedQuadsExact,
                SurfaceProjectedDrawPolicy::Compact,
            ),
            Ok(true),
        ));
    }

    #[test]
    fn producer_measurements_require_the_isolated_forced_compact_context() {
        assert!(gpu_producer_measurement_context_is_valid(
            GeometryPath::PackedAtlas,
            SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            SurfaceProjectedDrawPolicy::Compact,
        ));
        assert!(!gpu_producer_measurement_context_is_valid(
            GeometryPath::PackedAtlas,
            SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            SurfaceProjectedDrawPolicy::Adaptive,
        ));
        assert!(!gpu_producer_measurement_context_is_valid(
            GeometryPath::PackedAtlas,
            SurfaceRasterExecutionPlan::GlobalQuads,
            SurfaceProjectedDrawPolicy::Compact,
        ));
    }

    #[test]
    fn forced_projected_mode_suspends_pending_learning_but_preserves_lane_history() {
        let mut policy = AdaptiveProjectedDrawPolicy::default();
        policy.candidate_baseline.push(11.0);
        policy.phase = ProjectedAdaptivePhase::Probe {
            incumbent: SurfaceProjectedDrawExecution::Candidate,
            next_sample: 3,
            active_execution: SurfaceProjectedDrawExecution::Compact,
        };
        policy.pending = Some(ProjectedAdaptivePendingSample {
            order_backend: SurfaceOrderBackendUsed::Cpu,
            execution: SurfaceProjectedDrawExecution::Compact,
            ticket: 17,
            kind: ProjectedAdaptiveSampleKind::Probe(3),
        });
        policy.incumbent_changed = true;

        policy.suspend_learning();

        assert!(policy.pending.is_none());
        assert!(!policy.incumbent_changed);
        assert_eq!(policy.candidate_baseline.len, 1);
        assert_eq!(
            policy.phase,
            ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                next_sample: 3,
                active_execution: SurfaceProjectedDrawExecution::Compact,
            }
        );
    }

    #[test]
    fn resetting_order_policy_does_not_clear_either_projected_lane() {
        let mut order = AdaptiveOrderPolicy::default();
        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        cpu.candidate_baseline.push(11.0);
        gpu.candidate_baseline.push(19.0);
        order.reset(AdaptiveMetric::FrameCompletion);
        assert_eq!(cpu.candidate_baseline.len, 1);
        assert_eq!(gpu.candidate_baseline.len, 1);
    }

    #[test]
    fn gpu_producer_measurement_graph_transition_discards_all_pending_learning() {
        let mut order = AdaptiveOrderPolicy::default();
        order.reset(AdaptiveMetric::FrameCompletion);
        let order_choice = order.choose_refresh_backend();
        order.register_pending_sample(order_choice, 41);

        let mut cpu = AdaptiveProjectedDrawPolicy::default();
        cpu.candidate_baseline.push(11.0);
        let cpu_choice = cpu.choose(true);
        cpu.register_pending_sample(SurfaceOrderBackendUsed::Cpu, cpu_choice, 42);
        let mut gpu = AdaptiveProjectedDrawPolicy::default();
        gpu.candidate_baseline.push(19.0);
        let gpu_choice = gpu.choose(true);
        gpu.register_pending_sample(SurfaceOrderBackendUsed::Gpu, gpu_choice, 43);
        let mut owner = Some(AdaptiveProbeOwner::Order);
        let mut blocked = Some(order_choice);

        reset_adaptive_for_gpu_producer_measurement_transition(
            &mut order,
            &mut cpu,
            &mut gpu,
            &mut owner,
            &mut blocked,
        );

        assert_eq!(order.state(), SurfaceAdaptiveState::CpuLearning);
        assert!(order.pending.is_none());
        assert!(cpu.pending.is_none());
        assert!(gpu.pending.is_none());
        assert_eq!(cpu.candidate_baseline.len, 1);
        assert_eq!(gpu.candidate_baseline.len, 1);
        assert!(owner.is_none());
        assert!(blocked.is_none());
    }

    #[test]
    fn gpu_producer_terminal_queue_retains_the_sixty_fifth_receipt() {
        let mut terminals = VecDeque::with_capacity(64);
        for ticket in 0_u64..65 {
            retain_gpu_producer_terminal(&mut terminals, ticket);
        }
        assert_eq!(terminals.len(), 65);
        assert_eq!(
            terminals.into_iter().collect::<Vec<_>>(),
            (0..65).collect::<Vec<_>>()
        );
    }
}
