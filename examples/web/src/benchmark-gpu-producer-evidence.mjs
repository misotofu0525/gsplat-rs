const MIN_GPU_PRODUCER_TICKET = 2 ** 51;
const MAX_GPU_PRODUCER_TICKET = (2 ** 52) - 1;
const GPU_PRODUCERS = new Set(["post-sort", "preproject"]);
const GPU_PRODUCER_FAILURE_REASONS = new Set([
  "readback_map",
  "generation_invalidated",
  "invariant_violation",
]);

function safeInteger(value, name, { positive = false, ticket = false } = {}) {
  if (!Number.isSafeInteger(value) || value < (positive ? 1 : 0)) {
    throw new TypeError(`${name} is not a JS-safe integer`);
  }
  if (ticket && (value < MIN_GPU_PRODUCER_TICKET || value > MAX_GPU_PRODUCER_TICKET)) {
    throw new TypeError(`${name} is outside the GPU-producer ticket namespace`);
  }
  return value;
}

function validateProducer(value, name) {
  if (!GPU_PRODUCERS.has(value)) {
    throw new TypeError(`${name} is not post-sort or preproject`);
  }
  return value;
}

function validateIdentity(record, name) {
  safeInteger(record?.ticket, `${name}.ticket`, { positive: true, ticket: true });
  safeInteger(record?.camera_revision, `${name}.camera_revision`);
  validateProducer(record?.producer, `${name}.producer`);
}

/**
 * Validate the requested/actual producer and one issued formal ticket on each
 * retained A/B frame. An omitted request proves GPU frames retain the PostSort
 * product default, while CPU frames report no GPU producer, and does not
 * enable the diagnostic receipt stream.
 */
export function validateGpuProducerFrameEvidence({ requestedProducer, frames }) {
  if (requestedProducer !== null) {
    validateProducer(requestedProducer, "requestedProducer");
  }
  if (!Array.isArray(frames) || frames.length === 0) {
    throw new TypeError("GPU-producer frame evidence is empty");
  }

  for (const [index, frame] of frames.entries()) {
    const name = `frames[${index}]`;
    const actual = frame?.gpu_order_producer;
    const submission = frame?.gpu_producer_measurement_submission;
    const ticket = frame?.gpu_producer_measurement_ticket;
    const measuredProducer = frame?.gpu_producer_measurement_producer;
    const unsampled = frame?.gpu_producer_measurement_unsampled_reason;

    if (requestedProducer === null) {
      if (actual !== null) validateProducer(actual, `${name}.gpu_order_producer`);
      if ((frame?.order_backend === "gpu" && actual !== "post-sort")
          || (frame?.order_backend === "cpu" && frame?.gpu_order_producer !== null)) {
        throw new TypeError(`${name} changed or misreported the default GPU producer`);
      }
      if (submission !== "not_requested" || ticket !== null
          || measuredProducer !== null || unsampled !== null) {
        throw new TypeError(`${name} enabled GPU-producer telemetry without a request`);
      }
      continue;
    }

    validateProducer(actual, `${name}.gpu_order_producer`);

    if (actual !== requestedProducer
        || frame?.order_backend !== "gpu"
        || frame?.raster_execution_plan !== "projected_quads_exact"
        || frame?.projected_policy !== "compact"
        || frame?.projected_execution !== "compact"
        || frame?.projected_adaptive_state !== "disabled"
        || frame?.sort_refreshed !== true) {
      throw new TypeError(`${name} is outside the strict GPU-producer experiment context`);
    }
    if (submission !== "issued") {
      throw new TypeError(`${name} lacks an issued GPU-producer measurement`);
    }
    safeInteger(ticket, `${name}.gpu_producer_measurement_ticket`, {
      positive: true,
      ticket: true,
    });
    if (measuredProducer !== requestedProducer || unsampled !== null) {
      throw new TypeError(`${name} issued GPU-producer identity does not match the request`);
    }
  }
}

/** Join every retained measured frame to its independently logged submission. */
export function validateGpuProducerMeasuredSubmissions({ frames, submissions }) {
  if (!Array.isArray(frames) || frames.length === 0 || !Array.isArray(submissions)) {
    throw new TypeError("GPU-producer measured submission inputs are invalid");
  }
  const measured = submissions.filter((submission) => submission?.phase === "measured");
  const byTicket = new Map();
  for (const [index, submission] of measured.entries()) {
    validateIdentity(submission, `measured_submissions[${index}]`);
    if (byTicket.has(submission.ticket)) {
      throw new TypeError(`GPU-producer measured ticket ${submission.ticket} was duplicated`);
    }
    byTicket.set(submission.ticket, submission);
  }
  if (measured.length !== frames.length) {
    throw new TypeError(
      `GPU-producer measured submission count mismatch: frames=${frames.length} ` +
      `submissions=${measured.length}`,
    );
  }
  for (const [index, frame] of frames.entries()) {
    const ticket = frame?.gpu_producer_measurement_ticket;
    const submission = byTicket.get(ticket);
    if (!submission
        || submission.camera_revision !== frame?.camera_revision
        || submission.producer !== frame?.gpu_producer_measurement_producer) {
      throw new TypeError(
        `frames[${index}] GPU-producer ticket ${ticket} lacks a matching measured submission`,
      );
    }
  }
}

/** Require exactly one exact-current terminal for every issued producer ticket. */
export function validateGpuProducerTerminalLedger({
  requestedProducer,
  sourceCount,
  submissions,
  measurements,
  failures,
}) {
  validateProducer(requestedProducer, "requestedProducer");
  safeInteger(sourceCount, "sourceCount");
  if (!Array.isArray(submissions) || !Array.isArray(measurements) || !Array.isArray(failures)) {
    throw new TypeError("GPU-producer terminal ledger inputs must be arrays");
  }

  const issued = new Map();
  for (const [index, submission] of submissions.entries()) {
    const name = `submissions[${index}]`;
    validateIdentity(submission, name);
    if (submission.producer !== requestedProducer) {
      throw new TypeError(`${name}.producer does not match the request`);
    }
    if (issued.has(submission.ticket)) {
      throw new TypeError(`GPU-producer ticket ${submission.ticket} was issued more than once`);
    }
    issued.set(submission.ticket, submission);
  }
  if (issued.size === 0) {
    throw new TypeError("requested GPU-producer evidence issued no tickets");
  }

  const terminals = new Map();
  const addTerminal = (terminal, kind, index) => {
    const name = `${kind}[${index}]`;
    validateIdentity(terminal, name);
    if (terminals.has(terminal.ticket)) {
      throw new TypeError(`GPU-producer ticket ${terminal.ticket} has more than one terminal`);
    }
    const submission = issued.get(terminal.ticket);
    if (!submission) {
      throw new TypeError(`GPU-producer terminal ticket ${terminal.ticket} was never issued`);
    }
    if (submission.camera_revision !== terminal.camera_revision
        || submission.producer !== terminal.producer) {
      throw new TypeError(`GPU-producer ticket ${terminal.ticket} changed terminal identity`);
    }
    terminals.set(terminal.ticket, kind);
  };

  for (const [index, measurement] of measurements.entries()) {
    addTerminal(measurement, "measurements", index);
    safeInteger(measurement.order_generation, `measurements[${index}].order_generation`);
    safeInteger(
      measurement.projection_generation,
      `measurements[${index}].projection_generation`,
    );
    if (!Number.isFinite(measurement.queue_complete_ms)
        || measurement.queue_complete_ms < 0) {
      throw new TypeError(`measurements[${index}].queue_complete_ms is invalid`);
    }
    const { source, contributor, drawn } = measurement;
    if (![source, contributor, drawn].every(
      (value) => Number.isSafeInteger(value) && value >= 0,
    ) || source !== sourceCount || contributor > source || drawn !== contributor
        || measurement.count_semantics !== "source_contributor_issued_v1"
        || measurement.order_refreshed !== true
        || measurement.draw_scope !== "exact_current_contributors"
        || measurement.exact_current_contributor_draw !== true
        || measurement.stale_order !== false) {
      throw new TypeError(
        `GPU-producer ticket ${measurement.ticket} lacks exact-current S/C/D evidence`,
      );
    }
  }

  for (const [index, failure] of failures.entries()) {
    addTerminal(failure, "failures", index);
    safeInteger(failure.order_generation, `failures[${index}].order_generation`);
    safeInteger(failure.projection_generation, `failures[${index}].projection_generation`);
    if (!GPU_PRODUCER_FAILURE_REASONS.has(failure.reason)) {
      throw new TypeError(`failures[${index}].reason is invalid`);
    }
  }

  const missing = [...issued.keys()].filter((ticket) => !terminals.has(ticket));
  if (missing.length > 0) {
    throw new TypeError(`GPU-producer terminal ledger is missing ticket(s): ${missing.join(",")}`);
  }
  return {
    issued_count: issued.size,
    success_count: measurements.length,
    failure_count: failures.length,
  };
}
