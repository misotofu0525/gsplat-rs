//! Adaptive CPU/GPU order policy, isolated from the default CPU frame path.

use super::surface_session::{SurfaceAdaptiveState, SurfaceOrderBackendUsed};

const ADAPTIVE_CPU_BOOTSTRAP_SAMPLES: u32 = 6;
const ADAPTIVE_INITIAL_PROBE_DELAY: u32 = 4;
const ADAPTIVE_PROBE_SAMPLES: u32 = 4;
const ADAPTIVE_REPROBE_INTERVAL: u32 = 48;
const ADAPTIVE_GPU_PROMOTION_RATIO: f32 = 0.88;
const ADAPTIVE_CPU_PROMOTION_RATIO: f32 = 0.92;
const ADAPTIVE_EMERGENCY_DEMOTION_RATIO: f32 = 1.25;
const ADAPTIVE_GPU_FAILURE_COOLDOWN: u32 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdaptivePhase {
    CpuLearning,
    CpuStable,
    GpuProbe { remaining: u32, skip_warmup: bool },
    GpuStable,
    CpuProbe { remaining: u32 },
    Cooldown { remaining: u32 },
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
struct AdaptiveTimingEstimate {
    refresh: RollingEstimate,
    reuse: RollingEstimate,
}

impl AdaptiveTimingEstimate {
    fn clear(&mut self) {
        self.refresh.clear();
        self.reuse.clear();
    }

    fn push(&mut self, sample_ms: f32, refresh: bool) {
        if refresh {
            self.refresh.push(sample_ms);
        } else {
            self.reuse.push(sample_ms);
        }
    }

    /// Estimates the end-to-end cost at the configured cadence. Keeping the
    /// refresh and reuse tails separate prevents a costly refresh from falling
    /// below p75 merely because the sort interval is four or greater.
    fn cadence_score(&self, sort_interval: u32) -> Option<f32> {
        match (self.refresh.p75(), self.reuse.p75()) {
            (Some(refresh), Some(reuse)) if sort_interval > 1 => {
                let reuse_weight = sort_interval.saturating_sub(1) as f32;
                Some((refresh + reuse * reuse_weight) / sort_interval as f32)
            }
            (Some(refresh), _) => Some(refresh),
            (None, Some(reuse)) => Some(reuse),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct AdaptiveOrderPolicy {
    phase: Option<AdaptivePhase>,
    cpu: AdaptiveTimingEstimate,
    gpu: AdaptiveTimingEstimate,
    refreshes_since_probe: u32,
    cpu_bootstrap_refreshes: u32,
    next_gpu_probe_after: u32,
    pub(crate) gpu_initialized: bool,
}

impl AdaptiveOrderPolicy {
    pub(crate) fn reset(&mut self) {
        *self = Self {
            phase: Some(AdaptivePhase::CpuLearning),
            next_gpu_probe_after: ADAPTIVE_INITIAL_PROBE_DELAY,
            ..Self::default()
        };
    }

    pub(crate) fn state(&self) -> SurfaceAdaptiveState {
        match self.phase {
            None => SurfaceAdaptiveState::Disabled,
            Some(AdaptivePhase::CpuLearning) => SurfaceAdaptiveState::CpuLearning,
            Some(AdaptivePhase::CpuStable) => SurfaceAdaptiveState::CpuStable,
            Some(AdaptivePhase::GpuProbe { .. }) => SurfaceAdaptiveState::GpuProbe,
            Some(AdaptivePhase::GpuStable) => SurfaceAdaptiveState::GpuStable,
            Some(AdaptivePhase::CpuProbe { .. }) => SurfaceAdaptiveState::CpuProbe,
            Some(AdaptivePhase::Cooldown { .. }) => SurfaceAdaptiveState::Cooldown,
        }
    }

    pub(crate) fn choose_refresh_backend(&mut self) -> SurfaceOrderBackendUsed {
        let phase = self.phase.get_or_insert(AdaptivePhase::CpuLearning);
        match *phase {
            AdaptivePhase::CpuLearning | AdaptivePhase::Cooldown { .. } => {
                SurfaceOrderBackendUsed::Cpu
            }
            AdaptivePhase::CpuStable => {
                if self.cpu.refresh.len >= ADAPTIVE_CPU_BOOTSTRAP_SAMPLES as usize
                    && self.refreshes_since_probe >= self.next_gpu_probe_after
                {
                    self.gpu.clear();
                    *phase = AdaptivePhase::GpuProbe {
                        remaining: ADAPTIVE_PROBE_SAMPLES,
                        skip_warmup: !self.gpu_initialized,
                    };
                    SurfaceOrderBackendUsed::Gpu
                } else {
                    SurfaceOrderBackendUsed::Cpu
                }
            }
            AdaptivePhase::GpuProbe { .. } | AdaptivePhase::GpuStable => {
                if matches!(*phase, AdaptivePhase::GpuStable)
                    && self.refreshes_since_probe >= ADAPTIVE_REPROBE_INTERVAL
                {
                    self.cpu.clear();
                    *phase = AdaptivePhase::CpuProbe {
                        remaining: ADAPTIVE_PROBE_SAMPLES,
                    };
                    SurfaceOrderBackendUsed::Cpu
                } else {
                    SurfaceOrderBackendUsed::Gpu
                }
            }
            AdaptivePhase::CpuProbe { .. } => SurfaceOrderBackendUsed::Cpu,
        }
    }

    pub(crate) fn observe_frame(
        &mut self,
        backend: SurfaceOrderBackendUsed,
        frame_ms: f32,
        refresh: bool,
        sort_interval: u32,
    ) {
        let Some(phase) = self.phase else {
            return;
        };
        let skip_sample = matches!(
            phase,
            AdaptivePhase::GpuProbe {
                skip_warmup: true,
                ..
            }
        );
        if !skip_sample {
            match backend {
                SurfaceOrderBackendUsed::Cpu => self.cpu.push(frame_ms, refresh),
                SurfaceOrderBackendUsed::Gpu => self.gpu.push(frame_ms, refresh),
            }
        }
        match phase {
            AdaptivePhase::CpuLearning => {
                if refresh {
                    self.cpu_bootstrap_refreshes = self.cpu_bootstrap_refreshes.saturating_add(1);
                }
                if self.cpu_bootstrap_refreshes >= ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
                    self.phase = Some(AdaptivePhase::CpuStable);
                    self.refreshes_since_probe = 0;
                }
            }
            AdaptivePhase::CpuStable => {
                if refresh {
                    self.refreshes_since_probe = self.refreshes_since_probe.saturating_add(1);
                }
            }
            AdaptivePhase::GpuProbe {
                mut remaining,
                skip_warmup,
            } => {
                self.gpu_initialized = true;
                if skip_warmup && refresh {
                    self.phase = Some(AdaptivePhase::GpuProbe {
                        remaining,
                        skip_warmup: false,
                    });
                    return;
                }
                if skip_warmup {
                    return;
                }
                if !refresh {
                    return;
                }
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    let gpu_wins = match (
                        self.cpu.cadence_score(sort_interval),
                        self.gpu.cadence_score(sort_interval),
                    ) {
                        (Some(cpu), Some(gpu)) => gpu < cpu * ADAPTIVE_GPU_PROMOTION_RATIO,
                        _ => false,
                    };
                    self.phase = Some(if gpu_wins {
                        self.next_gpu_probe_after = ADAPTIVE_REPROBE_INTERVAL;
                        AdaptivePhase::GpuStable
                    } else {
                        self.next_gpu_probe_after =
                            if self.next_gpu_probe_after < ADAPTIVE_REPROBE_INTERVAL {
                                ADAPTIVE_REPROBE_INTERVAL
                            } else {
                                self.next_gpu_probe_after.saturating_mul(2).min(384)
                            };
                        AdaptivePhase::CpuStable
                    });
                    self.refreshes_since_probe = 0;
                } else {
                    self.phase = Some(AdaptivePhase::GpuProbe {
                        remaining,
                        skip_warmup: false,
                    });
                }
            }
            AdaptivePhase::GpuStable => {
                if refresh {
                    self.refreshes_since_probe = self.refreshes_since_probe.saturating_add(1);
                }
                if backend == SurfaceOrderBackendUsed::Gpu
                    && matches!(
                        (
                            self.cpu.cadence_score(sort_interval),
                            self.gpu.cadence_score(sort_interval),
                        ),
                        (Some(cpu), Some(gpu))
                            if gpu > cpu * ADAPTIVE_EMERGENCY_DEMOTION_RATIO
                    )
                {
                    self.phase = Some(AdaptivePhase::CpuStable);
                    self.refreshes_since_probe = 0;
                }
            }
            AdaptivePhase::CpuProbe { mut remaining } => {
                if !refresh {
                    return;
                }
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    let cpu_wins = match (
                        self.cpu.cadence_score(sort_interval),
                        self.gpu.cadence_score(sort_interval),
                    ) {
                        (Some(cpu), Some(gpu)) => cpu < gpu * ADAPTIVE_CPU_PROMOTION_RATIO,
                        _ => true,
                    };
                    self.phase = Some(if cpu_wins {
                        AdaptivePhase::CpuStable
                    } else {
                        AdaptivePhase::GpuStable
                    });
                    self.refreshes_since_probe = 0;
                } else {
                    self.phase = Some(AdaptivePhase::CpuProbe { remaining });
                }
            }
            AdaptivePhase::Cooldown { mut remaining } => {
                if !refresh {
                    return;
                }
                remaining = remaining.saturating_sub(1);
                self.phase = Some(if remaining == 0 {
                    self.refreshes_since_probe = 0;
                    self.next_gpu_probe_after = 0;
                    AdaptivePhase::CpuStable
                } else {
                    AdaptivePhase::Cooldown { remaining }
                });
            }
        }
    }

    pub(crate) fn gpu_failed(&mut self) {
        self.phase = Some(AdaptivePhase::Cooldown {
            remaining: ADAPTIVE_GPU_FAILURE_COOLDOWN,
        });
        self.refreshes_since_probe = 0;
        self.next_gpu_probe_after = ADAPTIVE_GPU_FAILURE_COOLDOWN;
        self.gpu.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ADAPTIVE_GPU_FAILURE_COOLDOWN, ADAPTIVE_INITIAL_PROBE_DELAY, ADAPTIVE_PROBE_SAMPLES,
        ADAPTIVE_REPROBE_INTERVAL, AdaptiveOrderPolicy, AdaptivePhase, AdaptiveTimingEstimate,
    };
    use crate::surface_session::{SurfaceAdaptiveState, SurfaceOrderBackendUsed};

    fn feed_cpu_bootstrap(policy: &mut AdaptiveOrderPolicy, frame_ms: f32) {
        policy.reset();
        for _ in 0..super::ADAPTIVE_CPU_BOOTSTRAP_SAMPLES {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, frame_ms, true, 1);
        }
        for _ in 0..ADAPTIVE_INITIAL_PROBE_DELAY {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, frame_ms, true, 1);
        }
    }

    #[test]
    fn adaptive_policy_keeps_cpu_when_gpu_probe_loses() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 30.0, true, 1); // compile/warmup sample
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Gpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 20.0, true, 1);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
    }

    #[test]
    fn adaptive_policy_samples_presented_backend_during_interval_reuse() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 30.0, true, 2);
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 20.0, true, 2);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);

        let cpu_refresh_samples = policy.cpu.refresh.len;
        let cpu_reuse_samples = policy.cpu.reuse.len;
        let gpu_reuse_samples = policy.gpu.reuse.len;
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 21.0, false, 2);

        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        assert_eq!(policy.cpu.refresh.len, cpu_refresh_samples);
        assert_eq!(policy.cpu.reuse.len, cpu_reuse_samples);
        assert_eq!(policy.gpu.reuse.len, gpu_reuse_samples + 1);
    }

    #[test]
    fn adaptive_policy_backs_off_after_losing_gpu_probe() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 10.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 30.0, true, 1);
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 20.0, true, 1);
        }

        for _ in 0..ADAPTIVE_REPROBE_INTERVAL {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 10.0, true, 1);
        }
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
    }

    #[test]
    fn adaptive_policy_does_not_misattribute_cpu_reuse_after_cpu_probe() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset();
        policy.cpu.push(12.0, true);
        policy.gpu.push(10.0, true);
        policy.phase = Some(AdaptivePhase::CpuProbe { remaining: 1 });

        policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 12.0, true, 2);
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        let cpu_reuse_samples = policy.cpu.reuse.len;
        let gpu_reuse_samples = policy.gpu.reuse.len;

        policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 12.0, false, 2);
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        assert_eq!(policy.cpu.reuse.len, cpu_reuse_samples + 1);
        assert_eq!(policy.gpu.reuse.len, gpu_reuse_samples);
    }

    #[test]
    fn adaptive_cadence_score_keeps_expensive_refresh_visible_at_long_intervals() {
        let mut cpu = AdaptiveTimingEstimate::default();
        let mut gpu = AdaptiveTimingEstimate::default();
        for _ in 0..4 {
            cpu.push(12.0, true);
            gpu.push(80.0, true);
        }
        for _ in 0..16 {
            cpu.push(10.0, false);
            gpu.push(5.0, false);
        }

        assert!(
            gpu.cadence_score(8).unwrap() > cpu.cadence_score(8).unwrap(),
            "the cheaper GPU reuse frames must not hide its much slower refresh"
        );
    }

    #[test]
    fn adaptive_policy_promotes_gpu_only_after_sustained_win() {
        let mut policy = AdaptiveOrderPolicy::default();
        feed_cpu_bootstrap(&mut policy, 20.0);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
        policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 40.0, true, 1); // ignored warmup
        for _ in 0..ADAPTIVE_PROBE_SAMPLES {
            policy.observe_frame(SurfaceOrderBackendUsed::Gpu, 15.0, true, 1);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::GpuStable);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
    }

    #[test]
    fn adaptive_gpu_failure_enters_cpu_cooldown() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset();
        policy.gpu_failed();
        assert_eq!(policy.state(), SurfaceAdaptiveState::Cooldown);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Cpu
        );
    }

    #[test]
    fn adaptive_gpu_failure_retries_after_one_cooldown_period() {
        let mut policy = AdaptiveOrderPolicy::default();
        policy.reset();
        policy.gpu_failed();
        for _ in 0..ADAPTIVE_GPU_FAILURE_COOLDOWN {
            assert_eq!(
                policy.choose_refresh_backend(),
                SurfaceOrderBackendUsed::Cpu
            );
            policy.observe_frame(SurfaceOrderBackendUsed::Cpu, 10.0, true, 1);
        }
        assert_eq!(policy.state(), SurfaceAdaptiveState::CpuStable);
        assert_eq!(
            policy.choose_refresh_backend(),
            SurfaceOrderBackendUsed::Gpu
        );
    }
}
