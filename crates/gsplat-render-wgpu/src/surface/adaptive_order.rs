//! CPU/GPU Adaptive ordering policy for Surface rendering.
//!
//! This module owns policy state and transitions only. The composing Surface
//! session still executes sorting, submits and polls tickets, publishes
//! evidence/current-stats, and arbitrates order probes against projected-draw
//! probes.

use crate::{
    SurfaceOrderBackendUsed, SurfaceOrderMeasurement, SurfaceOrderMeasurementFailure,
    SurfaceTimingSource,
};

pub(crate) const ADAPTIVE_CPU_BOOTSTRAP_SAMPLES: u32 = 6;
pub(crate) const ADAPTIVE_INITIAL_PROBE_DELAY: u32 = 4;
const ADAPTIVE_PROBE_SAMPLES_PER_BACKEND: u8 = 8;
pub(crate) const ADAPTIVE_PROBE_SEQUENCE_LEN: u8 = ADAPTIVE_PROBE_SAMPLES_PER_BACKEND * 2;
pub(crate) const ADAPTIVE_REPROBE_INTERVAL: u32 = 48;
const ADAPTIVE_GPU_PROMOTION_RATIO: f32 = 0.88;
const ADAPTIVE_CPU_PROMOTION_RATIO: f32 = 0.92;
const ADAPTIVE_GPU_FAILURE_COOLDOWN: u32 = 96;

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
pub(crate) enum AdaptiveMetric {
    #[default]
    OrderOnly,
    FrameCompletion,
}

pub(crate) const fn adaptive_primary_metric() -> AdaptiveMetric {
    AdaptiveMetric::FrameCompletion
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdaptiveSampleKind {
    CpuBootstrap,
    Probe(u8),
    TransitionWarmup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdaptiveRefreshChoice {
    pub(crate) backend: SurfaceOrderBackendUsed,
    pub(crate) sample: Option<AdaptiveSampleKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AdaptivePendingSample {
    backend: SurfaceOrderBackendUsed,
    ticket: u64,
    kind: AdaptiveSampleKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RollingEstimate {
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
    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
    }

    pub(crate) fn push(&mut self, sample_ms: f32) {
        if !sample_ms.is_finite() || sample_ms < 0.0 {
            return;
        }
        self.samples[self.cursor] = sample_ms;
        self.cursor = (self.cursor + 1) % self.samples.len();
        self.len = self.len.saturating_add(1).min(self.samples.len());
    }

    pub(crate) fn p75(&self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.samples;
        sorted[..self.len].sort_by(f32::total_cmp);
        let index = (self.len - 1) * 3 / 4;
        Some(sorted[index])
    }

    #[cfg(test)]
    pub(crate) const fn sample_count(&self) -> usize {
        self.len
    }
}

#[derive(Debug, Default)]
pub(crate) struct AdaptiveOrderPolicy {
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
    pub(crate) fn reset(&mut self, metric: AdaptiveMetric) {
        *self = Self {
            phase: Some(AdaptivePhase::CpuLearning { completed: 0 }),
            metric,
            next_gpu_probe_after: ADAPTIVE_INITIAL_PROBE_DELAY,
            ..Self::default()
        };
    }

    pub(crate) fn state(&self) -> SurfaceAdaptiveState {
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

    pub(crate) fn metric(&self) -> AdaptiveMetric {
        self.metric
    }

    pub(crate) fn choose_refresh_backend(&mut self) -> AdaptiveRefreshChoice {
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

    pub(crate) fn held_refresh_choice(&self) -> AdaptiveRefreshChoice {
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

    pub(crate) fn cohort_active(&self) -> bool {
        self.pending.is_some()
            || matches!(
                self.phase,
                Some(AdaptivePhase::CpuLearning { .. } | AdaptivePhase::Probe { .. })
            )
    }

    pub(crate) fn complete_synchronous_sample(
        &mut self,
        choice: AdaptiveRefreshChoice,
        sample_ms: f32,
    ) {
        let Some(kind) = choice.sample else {
            return;
        };
        self.complete_sample(choice.backend, kind, sample_ms);
    }

    pub(crate) fn register_pending_sample(&mut self, choice: AdaptiveRefreshChoice, ticket: u64) {
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

    pub(crate) fn complete_pending_sample(
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

    pub(crate) fn pending_sample(&self) -> Option<SurfaceAdaptivePendingSample> {
        self.pending.map(|pending| SurfaceAdaptivePendingSample {
            backend: pending.backend,
            ticket: pending.ticket,
        })
    }

    pub(crate) fn has_pending_sample(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn observe_cpu_measurement_failure(
        &mut self,
        failure: SurfaceOrderMeasurementFailure,
    ) -> bool {
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
    pub(crate) fn observe_gpu_measurement(&mut self, measurement: SurfaceOrderMeasurement) {
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

    pub(crate) fn observe_gpu_measurement_failure(
        &mut self,
        failure: SurfaceOrderMeasurementFailure,
    ) -> bool {
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

    pub(crate) fn gpu_failed(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SurfaceOrderMeasurementFailureReason;

    fn feed_cpu_bootstrap(policy: &mut AdaptiveOrderPolicy, sample_ms: f32) {
        policy.reset(AdaptiveMetric::OrderOnly);
        for _ in 0..ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
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
}
