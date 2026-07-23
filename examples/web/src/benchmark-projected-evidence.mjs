const MIN_PROJECTED_TICKET = 2 ** 52;
const PROJECTED_EXECUTIONS = new Set(["candidate", "compact"]);
const PROJECTED_FAILURE_REASONS = new Set([
  "readback_map",
  "generation_invalidated",
  "invariant_violation",
]);
const PROJECTED_ADAPTIVE_STATES = new Set([
  "disabled",
  "candidate_learning",
  "candidate_stable",
  "compact_probe",
  "compact_stable",
  "candidate_probe",
  "candidate_only",
  "cooldown",
]);

function safeInteger(value, name, { positive = false, projectedTicket = false } = {}) {
  if (!Number.isSafeInteger(value) || value < (positive ? 1 : 0)) {
    throw new TypeError(`${name} is not a JS-safe integer`);
  }
  if (projectedTicket && value < MIN_PROJECTED_TICKET) {
    throw new TypeError(`${name} is outside the projected ticket namespace`);
  }
  return value;
}

function validateProjectedIdentity(record, name) {
  safeInteger(record?.ticket, `${name}.ticket`, { positive: true, projectedTicket: true });
  safeInteger(record?.camera_revision, `${name}.camera_revision`);
  if (!PROJECTED_EXECUTIONS.has(record?.execution)) {
    throw new TypeError(`${name}.execution is invalid`);
  }
  if (record?.order_backend !== "cpu" && record?.order_backend !== "gpu") {
    throw new TypeError(`${name}.order_backend is invalid`);
  }
}

/** Validate requested/actual/submission evidence on every retained frame. */
export function validateProjectedFrameEvidence({ requestedPolicy, frames }) {
  if (!["candidate", "compact", "adaptive"].includes(requestedPolicy)) {
    throw new TypeError("requested projected policy is invalid");
  }
  if (!Array.isArray(frames) || frames.length === 0) {
    throw new TypeError("projected frame evidence is empty");
  }
  for (const [index, frame] of frames.entries()) {
    const name = `frames[${index}]`;
    if (frame?.projected_policy !== requestedPolicy) {
      throw new TypeError(`${name}.projected_policy does not match the requested policy`);
    }
    if (!PROJECTED_EXECUTIONS.has(frame?.projected_execution)) {
      throw new TypeError(`${name}.projected_execution is invalid`);
    }
    if (!PROJECTED_ADAPTIVE_STATES.has(frame?.projected_adaptive_state)) {
      throw new TypeError(`${name}.projected_adaptive_state is invalid`);
    }
    const submission = frame?.projected_measurement_submission;
    const ticket = frame?.projected_measurement_ticket;
    const execution = frame?.projected_measurement_execution;
    const unsampled = frame?.projected_measurement_unsampled_reason;
    if (requestedPolicy !== "adaptive") {
      if (frame.projected_execution !== requestedPolicy
          || frame.projected_adaptive_state !== "disabled"
          || submission !== "not_requested"
          || ticket !== null || execution !== null || unsampled !== null) {
        throw new TypeError(`${name} manufactures telemetry for a forced projected policy`);
      }
      continue;
    }
    if (submission === "not_requested") {
      if (ticket !== null || execution !== null || unsampled !== null) {
        throw new TypeError(`${name} not-requested projected sample exposes identity`);
      }
    } else if (submission === "issued") {
      safeInteger(ticket, `${name}.projected_measurement_ticket`, {
        positive: true,
        projectedTicket: true,
      });
      if (!PROJECTED_EXECUTIONS.has(execution)
          || execution !== frame.projected_execution || unsampled !== null) {
        throw new TypeError(`${name} issued projected sample has inconsistent execution`);
      }
    } else if (submission === "unsampled") {
      if (ticket !== null || !PROJECTED_EXECUTIONS.has(execution)
          || execution !== frame.projected_execution
          || !["ring_busy", "surface_unavailable"].includes(unsampled)) {
        throw new TypeError(`${name} unsampled projected request has invalid identity`);
      }
    } else {
      throw new TypeError(`${name}.projected_measurement_submission is invalid`);
    }
  }
}

/**
 * Require exactly one terminal success or structured failure for every issued
 * Adaptive projected ticket. Unissued terminal records are rejected too.
 */
export function validateProjectedTerminalLedger({ submissions, measurements, failures }) {
  if (!Array.isArray(submissions) || !Array.isArray(measurements) || !Array.isArray(failures)) {
    throw new TypeError("projected terminal ledger inputs must be arrays");
  }
  const issued = new Map();
  for (const [index, submission] of submissions.entries()) {
    const name = `submissions[${index}]`;
    validateProjectedIdentity(submission, name);
    if (issued.has(submission.ticket)) {
      throw new TypeError(`projected ticket ${submission.ticket} was issued more than once`);
    }
    issued.set(submission.ticket, submission);
  }

  const terminals = new Map();
  const addTerminal = (terminal, kind, index) => {
    const name = `${kind}[${index}]`;
    validateProjectedIdentity(terminal, name);
    if (terminals.has(terminal.ticket)) {
      throw new TypeError(`projected ticket ${terminal.ticket} has more than one terminal`);
    }
    const submission = issued.get(terminal.ticket);
    if (!submission) {
      throw new TypeError(`projected terminal ticket ${terminal.ticket} was never issued`);
    }
    if (submission.camera_revision !== terminal.camera_revision
        || submission.execution !== terminal.execution
        || submission.order_backend !== terminal.order_backend) {
      throw new TypeError(`projected ticket ${terminal.ticket} changed terminal identity`);
    }
    terminals.set(terminal.ticket, kind);
  };

  for (const [index, measurement] of measurements.entries()) {
    addTerminal(measurement, "measurements", index);
    safeInteger(measurement.projection_generation, `measurements[${index}].projection_generation`);
    safeInteger(measurement.probe_generation, `measurements[${index}].probe_generation`);
    if (!Number.isFinite(measurement.frame_complete_ms) || measurement.frame_complete_ms < 0) {
      throw new TypeError(`measurements[${index}].frame_complete_ms is invalid`);
    }
    const { visible, contributor, drawn, execution } = measurement;
    if (![visible, contributor, drawn].every((value) => Number.isSafeInteger(value) && value >= 0)
        || contributor > visible
        || (execution === "candidate" && drawn !== visible)
        || (execution === "compact" && drawn !== contributor)) {
      throw new TypeError(`projected ticket ${measurement.ticket} has invalid V/C/D evidence`);
    }
  }
  for (const [index, failure] of failures.entries()) {
    addTerminal(failure, "failures", index);
    safeInteger(failure.projection_generation, `failures[${index}].projection_generation`);
    safeInteger(failure.probe_generation, `failures[${index}].probe_generation`);
    if (!PROJECTED_FAILURE_REASONS.has(failure.reason)) {
      throw new TypeError(`failures[${index}].reason is invalid`);
    }
  }
  const missing = [...issued.keys()].filter((ticket) => !terminals.has(ticket));
  if (missing.length > 0) {
    throw new TypeError(`projected terminal ledger is missing ticket(s): ${missing.join(",")}`);
  }
  return {
    issued_count: issued.size,
    success_count: measurements.length,
    failure_count: failures.length,
  };
}
