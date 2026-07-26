//! Candidate/Compact Adaptive projected-draw policy for Surface rendering.
//!
//! This module owns the two independent CPU/GPU-order policy lanes and their
//! pure transitions. The composing Surface session still executes the chosen
//! plan, reserves and polls telemetry tickets, publishes evidence/current
//! stats, and arbitrates projected probes against the order controller.

use crate::{
    SurfaceOrderBackendUsed, SurfaceProjectedDrawExecution, SurfaceProjectedDrawMeasurement,
    SurfaceProjectedDrawMeasurementFailure, SurfaceProjectedDrawMeasurementFailureReason,
};

const PROJECTED_CANDIDATE_BOOTSTRAP_SAMPLES: u32 = 6;
const PROJECTED_INITIAL_PROBE_DELAY: u32 = 4;
const PROJECTED_PROBE_SAMPLES_PER_EXECUTION: u8 = 8;
const PROJECTED_PROBE_SEQUENCE_LEN: u8 = PROJECTED_PROBE_SAMPLES_PER_EXECUTION * 2;
const PROJECTED_REPROBE_INTERVAL: u32 = 48;
const PROJECTED_COMPACT_PROMOTION_RATIO: f32 = 0.95;
const PROJECTED_CANDIDATE_PROMOTION_RATIO: f32 = 0.97;
const PROJECTED_TELEMETRY_FAILURE_COOLDOWN: u32 = 96;

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
pub(crate) enum ProjectedAdaptiveSampleKind {
    CandidateBootstrap,
    Probe(u8),
    TransitionWarmup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectedAdaptiveChoice {
    pub(crate) execution: SurfaceProjectedDrawExecution,
    pub(crate) sample: Option<ProjectedAdaptiveSampleKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectedAdaptivePendingSample {
    order_backend: SurfaceOrderBackendUsed,
    execution: SurfaceProjectedDrawExecution,
    ticket: u64,
    kind: ProjectedAdaptiveSampleKind,
}

#[derive(Debug, Clone, Copy)]
struct ProjectedRollingEstimate {
    samples: [f32; 16],
    len: usize,
    cursor: usize,
}

impl Default for ProjectedRollingEstimate {
    fn default() -> Self {
        Self {
            samples: [0.0; 16],
            len: 0,
            cursor: 0,
        }
    }
}

impl ProjectedRollingEstimate {
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

    #[cfg(test)]
    const fn sample_count(&self) -> usize {
        self.len
    }
}

#[derive(Debug)]
pub(crate) struct AdaptiveProjectedDrawPolicy {
    phase: ProjectedAdaptivePhase,
    candidate_baseline: ProjectedRollingEstimate,
    probe_candidate: ProjectedRollingEstimate,
    probe_compact: ProjectedRollingEstimate,
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
            candidate_baseline: ProjectedRollingEstimate::default(),
            probe_candidate: ProjectedRollingEstimate::default(),
            probe_compact: ProjectedRollingEstimate::default(),
            pending: None,
            frames_since_probe: 0,
            next_probe_after: PROJECTED_INITIAL_PROBE_DELAY,
            initial_selection_complete: false,
            incumbent_changed: false,
        }
    }
}

impl AdaptiveProjectedDrawPolicy {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Stops an in-flight formal sample without erasing stable evidence. The
    /// eventual terminal telemetry receipt is still exposed to callers, but
    /// cannot mutate policy while a deterministic forced mode is selected.
    /// Returning to Adaptive repeats the interrupted phase from the same
    /// sample index.
    pub(crate) fn suspend_learning(&mut self) {
        self.pending = None;
        self.incumbent_changed = false;
    }

    pub(crate) fn state(&self) -> SurfaceProjectedDrawAdaptiveState {
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

    pub(crate) fn choose(&mut self, compact_available: bool) -> ProjectedAdaptiveChoice {
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

    pub(crate) fn held_choice(&self) -> ProjectedAdaptiveChoice {
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

    pub(crate) fn cohort_active(&self) -> bool {
        self.pending.is_some()
            || !self.initial_selection_complete
            || matches!(
                self.phase,
                ProjectedAdaptivePhase::CandidateLearning { .. }
                    | ProjectedAdaptivePhase::Probe { .. }
            )
    }

    pub(crate) fn take_incumbent_changed(&mut self) -> bool {
        std::mem::take(&mut self.incumbent_changed)
    }

    pub(crate) fn complete_synchronous_sample(&mut self, choice: ProjectedAdaptiveChoice) {
        if choice.sample == Some(ProjectedAdaptiveSampleKind::TransitionWarmup)
            && let ProjectedAdaptivePhase::Probe {
                active_execution, ..
            } = &mut self.phase
        {
            *active_execution = choice.execution;
        }
    }

    pub(crate) fn register_pending_sample(
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
        self.pending = Some(ProjectedAdaptivePendingSample {
            order_backend,
            execution: choice.execution,
            ticket,
            kind,
        });
    }

    pub(crate) fn observe_measurement(
        &mut self,
        measurement: SurfaceProjectedDrawMeasurement,
    ) -> bool {
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

    pub(crate) fn observe_failure(
        &mut self,
        failure: SurfaceProjectedDrawMeasurementFailure,
    ) -> bool {
        let matches = self.pending.is_some_and(|pending| {
            pending.ticket == failure.ticket
                && pending.order_backend == failure.order_backend
                && pending.execution == failure.execution
        });
        if matches {
            self.pending = None;
            self.initial_selection_complete = true;
            if failure.execution == SurfaceProjectedDrawExecution::Compact
                && failure.reason
                    == SurfaceProjectedDrawMeasurementFailureReason::InvariantViolation
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

    pub(crate) fn pending_sample(&self) -> Option<SurfaceProjectedDrawAdaptivePendingSample> {
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
                    if u32::from(*completed) >= PROJECTED_CANDIDATE_BOOTSTRAP_SAMPLES {
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
                if *next_sample >= PROJECTED_PROBE_SEQUENCE_LEN {
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
            if winner != incumbent || self.next_probe_after < PROJECTED_REPROBE_INTERVAL {
                PROJECTED_REPROBE_INTERVAL
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_abba_sequence_is_independent_from_order_backend() {
        for incumbent in [
            SurfaceProjectedDrawExecution::Candidate,
            SurfaceProjectedDrawExecution::Compact,
        ] {
            let challenger = match incumbent {
                SurfaceProjectedDrawExecution::Candidate => SurfaceProjectedDrawExecution::Compact,
                SurfaceProjectedDrawExecution::Compact => SurfaceProjectedDrawExecution::Candidate,
            };
            assert_eq!(
                (0..4)
                    .map(|index| projected_probe_sequence_execution(incumbent, index))
                    .collect::<Vec<_>>(),
                [incumbent, challenger, challenger, incumbent],
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
        assert_eq!(cpu.candidate_baseline.sample_count(), 1);
        assert_eq!(gpu.candidate_baseline.sample_count(), 0);
    }

    #[test]
    fn projected_transition_warmup_updates_only_the_active_execution() {
        let mut policy = AdaptiveProjectedDrawPolicy {
            phase: ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                next_sample: 1,
                active_execution: SurfaceProjectedDrawExecution::Candidate,
            },
            ..AdaptiveProjectedDrawPolicy::default()
        };
        let choice = policy.choose(true);
        assert_eq!(
            choice,
            ProjectedAdaptiveChoice {
                execution: SurfaceProjectedDrawExecution::Compact,
                sample: Some(ProjectedAdaptiveSampleKind::TransitionWarmup),
            }
        );
        policy.complete_synchronous_sample(choice);
        assert_eq!(
            policy.phase,
            ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                next_sample: 1,
                active_execution: SurfaceProjectedDrawExecution::Compact,
            }
        );
    }

    #[test]
    fn initial_candidate_cohort_remains_active_until_first_abba_selection() {
        let mut policy = AdaptiveProjectedDrawPolicy::default();
        for index in 0..PROJECTED_CANDIDATE_BOOTSTRAP_SAMPLES {
            policy.complete_sample(
                ProjectedAdaptiveSampleKind::CandidateBootstrap,
                SurfaceProjectedDrawExecution::Candidate,
                10.0 + index as f32,
            );
        }
        assert!(policy.cohort_active());

        policy.phase = ProjectedAdaptivePhase::Probe {
            incumbent: SurfaceProjectedDrawExecution::Candidate,
            next_sample: 0,
            active_execution: SurfaceProjectedDrawExecution::Candidate,
        };
        for _ in 0..8 {
            policy.probe_candidate.push(100.0);
            policy.probe_compact.push(85.0);
        }
        policy.finish_probe(SurfaceProjectedDrawExecution::Candidate);
        assert_eq!(
            policy.state(),
            SurfaceProjectedDrawAdaptiveState::CompactStable
        );
        assert!(!policy.cohort_active());
    }

    #[test]
    fn forced_mode_suspends_pending_learning_but_preserves_lane_history() {
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
        assert_eq!(policy.candidate_baseline.sample_count(), 1);
        assert_eq!(
            policy.phase,
            ProjectedAdaptivePhase::Probe {
                incumbent: SurfaceProjectedDrawExecution::Candidate,
                next_sample: 3,
                active_execution: SurfaceProjectedDrawExecution::Compact,
            }
        );
    }
}
