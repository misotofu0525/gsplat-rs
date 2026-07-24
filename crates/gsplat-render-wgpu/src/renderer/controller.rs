//! One measured controller over complete Exact execution plans.

use thiserror::Error;

use crate::evidence::{PlanComparisonKey, PlanSample, PlanSampleTicket};
use crate::plans::PlanId;

const BOOTSTRAP_SAMPLES: u8 = 6;
const INITIAL_PROBE_DELAY: u32 = 4;
const SAMPLES_PER_PLAN: u8 = 8;
const REPROBE_INTERVAL: u32 = 48;
const CHALLENGER_PROMOTION_RATIO: f32 = 0.90;
const FAILURE_COOLDOWN: u32 = 96;
const MINIMUM_RESIDENCY: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub(super) struct ControllerConfig {
    bootstrap_samples: u8,
    initial_probe_delay: u32,
    samples_per_plan: u8,
    reprobe_interval: u32,
    challenger_promotion_ratio: f32,
    failure_cooldown: u32,
    minimum_residency: u32,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            bootstrap_samples: BOOTSTRAP_SAMPLES,
            initial_probe_delay: INITIAL_PROBE_DELAY,
            samples_per_plan: SAMPLES_PER_PLAN,
            reprobe_interval: REPROBE_INTERVAL,
            challenger_promotion_ratio: CHALLENGER_PROMOTION_RATIO,
            failure_cooldown: FAILURE_COOLDOWN,
            minimum_residency: MINIMUM_RESIDENCY,
        }
    }
}

#[cfg(test)]
impl ControllerConfig {
    pub(super) const fn accelerated() -> Self {
        Self {
            bootstrap_samples: 1,
            initial_probe_delay: 0,
            samples_per_plan: 2,
            reprobe_interval: 1,
            challenger_promotion_ratio: 0.90,
            failure_cooldown: 2,
            minimum_residency: 1,
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WholePlanControllerError {
    #[error("whole-plan probe generation is exhausted")]
    ProbeGenerationExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FormalSampleKind {
    Bootstrap,
    TransitionWarmup,
    Probe { index: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlanDecision {
    plan: PlanId,
    comparison: PlanComparisonKey,
    probe_generation: u64,
    formal: Option<FormalSampleKind>,
    adaptive: bool,
}

impl PlanDecision {
    pub(super) const fn plan(self) -> PlanId {
        self.plan
    }

    pub(super) const fn comparison(self) -> PlanComparisonKey {
        self.comparison
    }

    pub(super) const fn probe_generation(self) -> u64 {
        self.probe_generation
    }

    pub(super) const fn formal_kind(self) -> Option<FormalSampleKind> {
        self.formal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Learning {
        completed: u8,
    },
    Stable,
    Probe {
        challenger: PlanId,
        next_sample: u8,
        active_plan: PlanId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingSample {
    ticket: PlanSampleTicket,
    kind: FormalSampleKind,
}

#[derive(Debug, Clone, Copy)]
struct RollingEstimate {
    samples: [f32; 16],
    len: usize,
}

impl Default for RollingEstimate {
    fn default() -> Self {
        Self {
            samples: [0.0; 16],
            len: 0,
        }
    }
}

impl RollingEstimate {
    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, sample: f32) {
        if self.len < self.samples.len() {
            self.samples[self.len] = sample;
            self.len += 1;
        }
    }

    fn p75(&self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.samples;
        sorted[..self.len].sort_by(f32::total_cmp);
        Some(sorted[(self.len - 1) * 3 / 4])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SampleDisposition {
    Accepted,
    Rejected,
}

/// Sole policy owner for the shadow Exact renderer. It sees only closed
/// complete-plan identities and queue-terminal samples.
#[derive(Clone)]
pub(super) struct WholePlanController {
    config: ControllerConfig,
    comparison: PlanComparisonKey,
    eligible: Box<[PlanId]>,
    fallback: PlanId,
    incumbent: PlanId,
    phase: Phase,
    pending: Option<PendingSample>,
    probe_generation: u64,
    frames_since_probe: u32,
    next_probe_after: u32,
    minimum_residency: u32,
    challenger_cursor: usize,
    cooldowns: [u32; 3],
    incumbent_samples: RollingEstimate,
    challenger_samples: RollingEstimate,
}

impl WholePlanController {
    pub(super) fn new(
        fallback: PlanId,
        eligible: &[PlanId],
        comparison: PlanComparisonKey,
    ) -> Self {
        Self::with_config(fallback, eligible, comparison, ControllerConfig::default())
    }

    fn with_config(
        fallback: PlanId,
        eligible: &[PlanId],
        comparison: PlanComparisonKey,
        config: ControllerConfig,
    ) -> Self {
        debug_assert!(eligible.contains(&fallback));
        debug_assert!(config.bootstrap_samples > 0);
        debug_assert!(config.samples_per_plan > 0 && config.samples_per_plan <= 8);
        debug_assert!(config.samples_per_plan.is_multiple_of(2));
        debug_assert!((0.0..1.0).contains(&config.challenger_promotion_ratio));
        let phase = if eligible.len() > 1 {
            Phase::Learning { completed: 0 }
        } else {
            Phase::Stable
        };
        Self {
            config,
            comparison,
            eligible: eligible.to_vec().into_boxed_slice(),
            fallback,
            incumbent: fallback,
            phase,
            pending: None,
            probe_generation: 1,
            frames_since_probe: 0,
            next_probe_after: config.initial_probe_delay,
            minimum_residency: 0,
            challenger_cursor: 0,
            cooldowns: [0; 3],
            incumbent_samples: RollingEstimate::default(),
            challenger_samples: RollingEstimate::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn set_config_for_test(&mut self, config: ControllerConfig) {
        let fallback = self.fallback;
        let eligible = self.eligible.to_vec();
        let comparison = self.comparison;
        *self = Self::with_config(fallback, &eligible, comparison, config);
    }

    /// Returns true when identity/eligibility changed and outstanding sampler
    /// work must be invalidated before another formal ticket can be armed.
    pub(super) fn synchronize(
        &mut self,
        fallback: PlanId,
        eligible: &[PlanId],
        comparison: PlanComparisonKey,
    ) -> bool {
        if self.fallback == fallback
            && self.eligible.as_ref() == eligible
            && self.comparison == comparison
        {
            return false;
        }
        let config = self.config;
        *self = Self::with_config(fallback, eligible, comparison, config);
        true
    }

    pub(super) fn choose_adaptive(&mut self) -> Result<PlanDecision, WholePlanControllerError> {
        if self.pending.is_some() {
            return Ok(self.decision(self.incumbent, None, true));
        }

        match self.phase {
            Phase::Learning { .. } => {
                Ok(self.decision(self.incumbent, Some(FormalSampleKind::Bootstrap), true))
            }
            Phase::Stable => {
                if self.minimum_residency > 0 || self.frames_since_probe < self.next_probe_after {
                    return Ok(self.decision(self.incumbent, None, true));
                }
                let Some(challenger) = self.next_challenger() else {
                    return Ok(self.decision(self.incumbent, None, true));
                };
                self.probe_generation = self
                    .probe_generation
                    .checked_add(1)
                    .ok_or(WholePlanControllerError::ProbeGenerationExhausted)?;
                self.incumbent_samples.clear();
                self.challenger_samples.clear();
                self.phase = Phase::Probe {
                    challenger,
                    next_sample: 0,
                    active_plan: self.incumbent,
                };
                self.choose_adaptive()
            }
            Phase::Probe {
                challenger,
                next_sample,
                active_plan,
            } => {
                let target = probe_sequence_plan(self.incumbent, challenger, next_sample);
                let formal = if target == active_plan {
                    FormalSampleKind::Probe { index: next_sample }
                } else {
                    FormalSampleKind::TransitionWarmup
                };
                Ok(self.decision(target, Some(formal), true))
            }
        }
    }

    pub(super) fn choose_forced(&self, plan: PlanId) -> PlanDecision {
        self.decision(plan, None, false)
    }

    fn decision(
        &self,
        plan: PlanId,
        formal: Option<FormalSampleKind>,
        adaptive: bool,
    ) -> PlanDecision {
        PlanDecision {
            plan,
            comparison: self.comparison,
            probe_generation: self.probe_generation,
            formal,
            adaptive,
        }
    }

    pub(super) fn can_arm(&self, decision: PlanDecision) -> bool {
        decision.adaptive
            && decision.formal.is_some()
            && self.pending.is_none()
            && decision.comparison == self.comparison
            && self.eligible.contains(&decision.plan)
    }

    /// Called only after the sampler successfully attaches its callback to the
    /// command buffer that is about to be submitted.
    pub(super) fn register_pending(
        &mut self,
        decision: PlanDecision,
        ticket: PlanSampleTicket,
    ) -> bool {
        let Some(kind) = decision.formal else {
            return false;
        };
        if !self.can_arm(decision)
            || ticket.probe_generation() != decision.probe_generation
            || ticket.comparison() != decision.comparison
            || ticket.plan() != decision.plan
        {
            return false;
        }
        self.pending = Some(PendingSample { ticket, kind });
        true
    }

    pub(super) fn submitted_without_sample(&mut self, decision: PlanDecision) {
        if !decision.adaptive || decision.formal.is_some() || self.pending.is_some() {
            return;
        }
        if matches!(self.phase, Phase::Stable) {
            self.frames_since_probe = self.frames_since_probe.saturating_add(1);
            self.minimum_residency = self.minimum_residency.saturating_sub(1);
            for cooldown in &mut self.cooldowns {
                *cooldown = cooldown.saturating_sub(1);
            }
        }
    }

    pub(super) fn observe(&mut self, sample: PlanSample) -> SampleDisposition {
        let Some(pending) = self.pending else {
            return SampleDisposition::Rejected;
        };
        let ticket = sample.sample_ticket();
        if ticket != pending.ticket {
            return SampleDisposition::Rejected;
        }
        if !sample.is_comparable()
            || sample.comparison() != self.comparison
            || !self.eligible.contains(&sample.plan_id())
        {
            // A terminal callback carrying the exact pending ticket is owned
            // by this probe. Invalid duration/count/frame data cannot teach
            // policy, but it must terminate the slot and enter bounded
            // cooldown instead of leaving the sole formal sample wedged.
            self.pending = None;
            self.enter_failure_cooldown(pending.ticket.plan());
            return SampleDisposition::Rejected;
        }

        self.pending = None;
        match pending.kind {
            FormalSampleKind::Bootstrap => {
                let Phase::Learning { completed } = self.phase else {
                    return SampleDisposition::Rejected;
                };
                let completed = completed.saturating_add(1);
                self.phase = if completed >= self.config.bootstrap_samples {
                    self.frames_since_probe = 0;
                    self.next_probe_after = self.config.initial_probe_delay;
                    Phase::Stable
                } else {
                    Phase::Learning { completed }
                };
            }
            FormalSampleKind::TransitionWarmup => {
                let Phase::Probe {
                    challenger,
                    next_sample,
                    ..
                } = self.phase
                else {
                    return SampleDisposition::Rejected;
                };
                self.phase = Phase::Probe {
                    challenger,
                    next_sample,
                    active_plan: sample.plan_id(),
                };
            }
            FormalSampleKind::Probe { index } => {
                let Phase::Probe {
                    challenger,
                    next_sample,
                    active_plan,
                } = self.phase
                else {
                    return SampleDisposition::Rejected;
                };
                if index != next_sample
                    || sample.plan_id()
                        != probe_sequence_plan(self.incumbent, challenger, next_sample)
                {
                    return SampleDisposition::Rejected;
                }
                if sample.plan_id() == self.incumbent {
                    self.incumbent_samples.push(sample.frame_complete_ms());
                } else {
                    self.challenger_samples.push(sample.frame_complete_ms());
                }
                let next_sample = next_sample.saturating_add(1);
                if next_sample >= self.config.samples_per_plan.saturating_mul(2) {
                    self.finish_probe(challenger);
                } else {
                    self.phase = Phase::Probe {
                        challenger,
                        next_sample,
                        active_plan,
                    };
                }
            }
        }
        SampleDisposition::Accepted
    }

    fn finish_probe(&mut self, challenger: PlanId) {
        let incumbent = self.incumbent_samples.p75();
        let challenger_metric = self.challenger_samples.p75();
        if let (Some(incumbent), Some(challenger_metric)) = (incumbent, challenger_metric)
            && challenger_metric < incumbent * self.config.challenger_promotion_ratio
        {
            self.incumbent = challenger;
            self.minimum_residency = self.config.minimum_residency;
        }
        if let Some(index) = self.eligible.iter().position(|plan| *plan == challenger) {
            self.challenger_cursor = (index + 1) % self.eligible.len();
        }
        self.phase = Phase::Stable;
        self.frames_since_probe = 0;
        self.next_probe_after = self.config.reprobe_interval;
        self.incumbent_samples.clear();
        self.challenger_samples.clear();
    }

    pub(super) fn execution_failed(&mut self, decision: PlanDecision) {
        if !decision.adaptive || self.pending.is_some() {
            return;
        }
        self.enter_failure_cooldown(decision.plan);
    }

    fn enter_failure_cooldown(&mut self, failed: PlanId) {
        self.cooldowns[plan_index(failed)] = self.config.failure_cooldown;
        if failed == self.incumbent {
            self.incumbent = self.fallback;
        }
        self.phase = Phase::Stable;
        self.frames_since_probe = 0;
        self.next_probe_after = self.config.initial_probe_delay;
        self.minimum_residency = self.config.minimum_residency;
        self.incumbent_samples.clear();
        self.challenger_samples.clear();
    }

    fn next_challenger(&self) -> Option<PlanId> {
        (0..self.eligible.len())
            .map(|offset| (self.challenger_cursor + offset) % self.eligible.len())
            .map(|index| self.eligible[index])
            .find(|plan| *plan != self.incumbent && self.cooldowns[plan_index(*plan)] == 0)
    }

    #[cfg(test)]
    pub(super) const fn incumbent_for_test(&self) -> PlanId {
        self.incumbent
    }

    #[cfg(test)]
    pub(super) fn pending_for_test(&self) -> Option<PlanSampleTicket> {
        self.pending.map(|pending| pending.ticket)
    }
}

const fn plan_index(plan: PlanId) -> usize {
    match plan {
        PlanId::CpuPostSort => 0,
        PlanId::GpuPostSort => 1,
        PlanId::GpuPreproject => 2,
    }
}

fn probe_sequence_plan(incumbent: PlanId, challenger: PlanId, index: u8) -> PlanId {
    match index % 4 {
        0 | 3 => incumbent,
        1 | 2 => challenger,
        _ => unreachable!(),
    }
}
