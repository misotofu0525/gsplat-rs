//! Bounded native initialization calibration for exact packed Scalar order.

use std::time::Duration;

use super::preprocess::packed::PackedScalarExecution;

pub(crate) const CALIBRATION_INPUT_CAP: usize = 500_000;
const CALIBRATION_MEASURED_SAMPLES: usize = 3;
const CALIBRATION_TOTAL_BUDGET: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub(crate) struct CalibrationCandidates {
    executions: [PackedScalarExecution; 3],
    len: usize,
}

impl CalibrationCandidates {
    fn for_capabilities(available_parallelism: usize, rayon_threads: usize) -> Self {
        Self::for_parallelism(available_parallelism.max(1).min(rayon_threads.max(1)))
    }

    fn for_parallelism(available_parallelism: usize) -> Self {
        let available_parallelism = available_parallelism.max(1);
        let mut candidates = Self {
            executions: [PackedScalarExecution::serial(); 3],
            len: 0,
        };
        candidates.push(PackedScalarExecution::serial());
        if available_parallelism >= 2 {
            candidates.push(PackedScalarExecution::new(2));
        }
        candidates.push(PackedScalarExecution::new(available_parallelism.min(4)));
        candidates
    }

    fn push(&mut self, execution: PackedScalarExecution) {
        if self.as_slice().contains(&execution) {
            return;
        }
        self.executions[self.len] = execution;
        self.len += 1;
    }

    pub(crate) fn as_slice(&self) -> &[PackedScalarExecution] {
        &self.executions[..self.len]
    }

    pub(crate) fn static_fallback(&self) -> PackedScalarExecution {
        self.as_slice()
            .last()
            .copied()
            .expect("calibration candidates always contain serial Scalar")
    }

    pub(crate) fn singleton_decision(&self) -> Option<CalibrationDecision> {
        let [execution] = self.as_slice() else {
            return None;
        };
        Some(CalibrationDecision::static_choice(*execution))
    }
}

pub(crate) fn native_candidates() -> CalibrationCandidates {
    let available_parallelism = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    CalibrationCandidates::for_capabilities(available_parallelism, rayon::current_num_threads())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalibrationFallbackReason {
    DeadlineExpired,
    ProbeFailed,
    InvalidTiming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CalibrationDecision {
    execution: PackedScalarExecution,
    fallback_reason: Option<CalibrationFallbackReason>,
}

impl CalibrationDecision {
    fn selected(execution: PackedScalarExecution) -> Self {
        Self {
            execution,
            fallback_reason: None,
        }
    }

    fn fallback(execution: PackedScalarExecution, reason: CalibrationFallbackReason) -> Self {
        Self {
            execution,
            fallback_reason: Some(reason),
        }
    }

    pub(crate) fn static_choice(execution: PackedScalarExecution) -> Self {
        Self::selected(execution)
    }

    pub(crate) const fn execution(self) -> PackedScalarExecution {
        self.execution
    }

    pub(crate) const fn fallback_reason(self) -> Option<CalibrationFallbackReason> {
        self.fallback_reason
    }
}

/// Runs one warmup and three measured terminal-order probes per candidate.
///
/// `elapsed` is time since the start of the complete calibration transaction.
/// The caller owns probe scratch; this protocol never receives or publishes an
/// authoritative order buffer.
pub(crate) fn calibrate_bounded<E>(
    candidates: CalibrationCandidates,
    mut elapsed: impl FnMut() -> Duration,
    mut probe: impl FnMut(PackedScalarExecution) -> Result<Duration, E>,
) -> CalibrationDecision {
    let mut medians = [(PackedScalarExecution::serial(), Duration::ZERO); 3];
    let mut median_count = 0;
    let (&static_fallback, other_candidates) = candidates
        .as_slice()
        .split_last()
        .expect("calibration candidates always contain serial Scalar");

    if elapsed() >= CALIBRATION_TOTAL_BUDGET {
        return CalibrationDecision::fallback(
            static_fallback,
            CalibrationFallbackReason::DeadlineExpired,
        );
    }

    // Measure the accepted static fallback first. If a later candidate probe
    // fails or the deadline expires, the frozen fallback is never an
    // execution outside the candidate set or one the transaction did not try.
    for candidate in std::iter::once(static_fallback).chain(other_candidates.iter().copied()) {
        if elapsed() >= CALIBRATION_TOTAL_BUDGET {
            return CalibrationDecision::fallback(
                static_fallback,
                CalibrationFallbackReason::DeadlineExpired,
            );
        }
        if probe(candidate).is_err() {
            return CalibrationDecision::fallback(
                static_fallback,
                CalibrationFallbackReason::ProbeFailed,
            );
        }
        if elapsed() >= CALIBRATION_TOTAL_BUDGET {
            return CalibrationDecision::fallback(
                static_fallback,
                CalibrationFallbackReason::DeadlineExpired,
            );
        }

        let mut samples = [Duration::ZERO; CALIBRATION_MEASURED_SAMPLES];
        for sample in &mut samples {
            if elapsed() >= CALIBRATION_TOTAL_BUDGET {
                return CalibrationDecision::fallback(
                    static_fallback,
                    CalibrationFallbackReason::DeadlineExpired,
                );
            }
            let Ok(duration) = probe(candidate) else {
                return CalibrationDecision::fallback(
                    static_fallback,
                    CalibrationFallbackReason::ProbeFailed,
                );
            };
            if duration.is_zero() {
                return CalibrationDecision::fallback(
                    static_fallback,
                    CalibrationFallbackReason::InvalidTiming,
                );
            }
            *sample = duration;
            if elapsed() >= CALIBRATION_TOTAL_BUDGET {
                return CalibrationDecision::fallback(
                    static_fallback,
                    CalibrationFallbackReason::DeadlineExpired,
                );
            }
        }
        samples.sort_unstable();
        medians[median_count] = (candidate, samples[1]);
        median_count += 1;
    }

    let fastest = medians[..median_count]
        .iter()
        .map(|(_, median)| *median)
        .min()
        .expect("non-empty candidate set produced a median");
    let noise_ceiling_ns = fastest
        .as_nanos()
        .saturating_add(fastest.as_nanos().div_ceil(20));
    let selected = medians[..median_count]
        .iter()
        .filter(|(_, median)| median.as_nanos() <= noise_ceiling_ns)
        .min_by_key(|(execution, _)| execution.chunk_count())
        .map(|(execution, _)| *execution)
        .unwrap_or(static_fallback);
    CalibrationDecision::selected(selected)
}

#[derive(Debug, Default)]
pub(crate) struct NativeCalibration {
    decision: Option<CalibrationDecision>,
    #[cfg(test)]
    attempts: usize,
}

impl NativeCalibration {
    pub(crate) const fn decision(&self) -> Option<CalibrationDecision> {
        self.decision
    }

    fn freeze(&mut self, decision: CalibrationDecision) {
        if self.decision.is_some() {
            return;
        }
        self.decision = Some(decision);
    }

    pub(crate) fn freeze_static(&mut self, decision: CalibrationDecision) {
        self.freeze(decision);
    }

    pub(crate) fn freeze_after_probe(&mut self, decision: CalibrationDecision) {
        if self.decision.is_some() {
            return;
        }
        self.freeze(decision);
        #[cfg(test)]
        {
            self.attempts += 1;
        }
    }

    #[cfg(test)]
    pub(crate) const fn attempts(&self) -> usize {
        self.attempts
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use super::{
        CALIBRATION_TOTAL_BUDGET, CalibrationCandidates, CalibrationDecision,
        CalibrationFallbackReason, NativeCalibration, calibrate_bounded,
    };
    use crate::cpu::preprocess::packed::PackedScalarExecution;

    fn chunk_counts(candidates: CalibrationCandidates) -> Vec<usize> {
        candidates
            .as_slice()
            .iter()
            .map(|candidate| candidate.chunk_count())
            .collect()
    }

    #[test]
    fn candidate_chunks_are_supported_ordered_and_deduplicated() {
        assert_eq!(chunk_counts(CalibrationCandidates::for_parallelism(0)), [1]);
        assert_eq!(chunk_counts(CalibrationCandidates::for_parallelism(1)), [1]);
        assert_eq!(
            chunk_counts(CalibrationCandidates::for_parallelism(2)),
            [1, 2]
        );
        assert_eq!(
            chunk_counts(CalibrationCandidates::for_parallelism(3)),
            [1, 2, 3]
        );
        assert_eq!(
            chunk_counts(CalibrationCandidates::for_parallelism(8)),
            [1, 2, 4]
        );

        let os_one_rayon_many = CalibrationCandidates::for_capabilities(1, 8);
        assert_eq!(chunk_counts(os_one_rayon_many), [1]);
        assert_eq!(
            os_one_rayon_many.static_fallback(),
            PackedScalarExecution::serial()
        );
        let decision = os_one_rayon_many
            .singleton_decision()
            .expect("effective serial capability has one static decision");
        let mut calibration = NativeCalibration::default();
        calibration.freeze_static(decision);
        assert_eq!(calibration.attempts(), 0);
        assert_eq!(decision.execution(), PackedScalarExecution::serial());

        let os_two_rayon_four = CalibrationCandidates::for_capabilities(2, 4);
        assert_eq!(chunk_counts(os_two_rayon_four), [1, 2]);
        assert_eq!(os_two_rayon_four.static_fallback().chunk_count(), 2);
        assert_eq!(os_two_rayon_four.singleton_decision(), None);
    }

    #[test]
    fn median_noise_band_prefers_fewer_scalar_chunks() {
        let elapsed = Cell::new(Duration::ZERO);
        let calls = Cell::new(0_usize);
        let decision = calibrate_bounded(
            CalibrationCandidates::for_parallelism(8),
            || elapsed.get(),
            |candidate| {
                calls.set(calls.get() + 1);
                elapsed.set(elapsed.get() + Duration::from_millis(1));
                let nanos = match candidate.chunk_count() {
                    1 => 100,
                    2 => 96,
                    4 => 92,
                    _ => unreachable!(),
                };
                Ok::<_, ()>(Duration::from_nanos(nanos))
            },
        );

        assert_eq!(calls.get(), 12);
        assert_eq!(decision.execution().chunk_count(), 2);
        assert_eq!(decision.fallback_reason(), None);
    }

    #[test]
    fn failed_invalid_and_expired_measurements_use_static_fallback() {
        let candidates = CalibrationCandidates::for_capabilities(2, 4);
        let fallback = candidates.static_fallback();
        let mut probed_chunks = Vec::new();
        let failed = calibrate_bounded(
            candidates,
            || Duration::ZERO,
            |candidate| {
                probed_chunks.push(candidate.chunk_count());
                if candidate.chunk_count() == 1 {
                    Err(())
                } else {
                    Ok(Duration::from_nanos(1))
                }
            },
        );
        assert_eq!(
            failed.fallback_reason(),
            Some(CalibrationFallbackReason::ProbeFailed)
        );
        assert_eq!(failed.execution(), fallback);
        assert_eq!(probed_chunks, [2, 2, 2, 2, 1]);

        let candidates = CalibrationCandidates::for_parallelism(8);
        let fallback = candidates.static_fallback();
        let invalid = calibrate_bounded(
            candidates,
            || Duration::ZERO,
            |_| Ok::<_, ()>(Duration::ZERO),
        );
        assert_eq!(
            invalid.fallback_reason(),
            Some(CalibrationFallbackReason::InvalidTiming)
        );
        assert_eq!(invalid.execution(), fallback);

        let elapsed = Cell::new(Duration::ZERO);
        let expired = calibrate_bounded(
            candidates,
            || elapsed.get(),
            |_| {
                elapsed.set(CALIBRATION_TOTAL_BUDGET);
                Ok::<_, ()>(Duration::from_nanos(1))
            },
        );
        assert_eq!(
            expired.fallback_reason(),
            Some(CalibrationFallbackReason::DeadlineExpired)
        );
        assert_eq!(expired.execution(), fallback);

        let first_probe = Cell::new(None);
        let preexpired = calibrate_bounded(
            candidates,
            || CALIBRATION_TOTAL_BUDGET,
            |candidate| {
                first_probe.set(Some(candidate.chunk_count()));
                Ok::<_, ()>(Duration::from_nanos(1))
            },
        );
        assert_eq!(first_probe.get(), None);
        assert_eq!(
            preexpired.fallback_reason(),
            Some(CalibrationFallbackReason::DeadlineExpired)
        );
        assert_eq!(preexpired.execution(), fallback);
    }

    #[test]
    fn calibration_state_distinguishes_serial_capability_from_one_probe_transaction() {
        let serial = PackedScalarExecution::serial();
        let mut serial_only = NativeCalibration::default();
        serial_only.freeze_static(CalibrationDecision::static_choice(serial));
        serial_only.freeze_after_probe(CalibrationDecision::fallback(
            PackedScalarExecution::new(4),
            CalibrationFallbackReason::DeadlineExpired,
        ));
        assert_eq!(serial_only.attempts(), 0);
        assert_eq!(
            serial_only.decision().map(CalibrationDecision::execution),
            Some(serial)
        );

        let fallback = PackedScalarExecution::new(4);
        let mut failed_probe = NativeCalibration::default();
        failed_probe.freeze_after_probe(CalibrationDecision::fallback(
            fallback,
            CalibrationFallbackReason::ProbeFailed,
        ));
        failed_probe.freeze_after_probe(CalibrationDecision::static_choice(serial));
        assert_eq!(failed_probe.attempts(), 1);
        assert_eq!(
            failed_probe
                .decision()
                .and_then(CalibrationDecision::fallback_reason),
            Some(CalibrationFallbackReason::ProbeFailed)
        );
        assert_eq!(
            failed_probe.decision().map(CalibrationDecision::execution),
            Some(fallback)
        );
    }
}
