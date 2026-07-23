function isPositiveTicket(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isNonNegativeRevision(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

export const COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1";

function validateCountReceipt(receipt, label) {
  if (receipt.count_semantics !== COUNT_SEMANTICS) {
    throw new Error(`${label} lacks count_semantics=${COUNT_SEMANTICS}`);
  }
  for (const field of ["visible", "contributor", "drawn"]) {
    if (!Number.isSafeInteger(receipt[field]) || receipt[field] < 0) {
      throw new Error(`${label} has invalid ${field} count`);
    }
  }
  if (typeof receipt.exact_contributor_compaction !== "boolean") {
    throw new Error(`${label} lacks exact_contributor_compaction`);
  }
  if (receipt.contributor > receipt.visible) {
    throw new Error(`${label} violates contributor <= visible`);
  }
  if (receipt.exact_contributor_compaction && receipt.drawn !== receipt.contributor) {
    throw new Error(`${label} exact contributor execution requires drawn == contributor`);
  }
  if (!receipt.exact_contributor_compaction && receipt.drawn !== receipt.visible) {
    throw new Error(`${label} legacy/downlevel execution requires drawn == visible`);
  }
}

function finiteMonotonicTimestamp(value, label) {
  if (!Number.isFinite(value) || value < 0) {
    throw new Error(`${label} must be a finite non-negative monotonic timestamp`);
  }
  return value;
}

/**
 * Derive the ordering window exclusively from performance.now() timestamps
 * emitted by the page. UTC strings remain identity metadata and are never
 * subtracted for performance evidence.
 */
export function monotonicOrderingWindow({ submissions, terminals }) {
  if (!Array.isArray(submissions) || !Array.isArray(terminals)) {
    throw new Error("monotonic ordering window requires submission and terminal ledgers");
  }
  const measured = submissions.filter((submission) => submission.phase === "measured");
  if (measured.length === 0) {
    return {
      monotonic_clock: "performance.now",
      first_measured_submit_monotonic_ms: null,
      last_measured_submit_monotonic_ms: null,
      last_measured_terminal_monotonic_ms: null,
      submit_span_ms: null,
      terminal_tail_ms: null,
      terminal_window_ms: null,
    };
  }

  let previousSubmit = -Infinity;
  const measuredTickets = new Set();
  for (const submission of measured) {
    const timestamp = finiteMonotonicTimestamp(
      submission.submitted_at_monotonic_ms,
      `submission ${submission.ticket}`,
    );
    if (timestamp < previousSubmit) {
      throw new Error("measured submission timestamps are not monotonic");
    }
    if (measuredTickets.has(submission.ticket)) {
      throw new Error(`measured submission ticket ${submission.ticket} is duplicated`);
    }
    measuredTickets.add(submission.ticket);
    previousSubmit = timestamp;
  }

  const terminalsByTicket = new Map();
  for (const terminal of terminals) {
    if (!measuredTickets.has(terminal.ticket)) continue;
    if (terminalsByTicket.has(terminal.ticket)) {
      throw new Error(`measured ticket ${terminal.ticket} has multiple terminal timestamps`);
    }
    terminalsByTicket.set(terminal.ticket, terminal);
  }

  let lastTerminal = -Infinity;
  for (const submission of measured) {
    const terminal = terminalsByTicket.get(submission.ticket);
    if (!terminal) {
      throw new Error(`measured ticket ${submission.ticket} lacks a monotonic terminal timestamp`);
    }
    const terminalTimestamp = finiteMonotonicTimestamp(
      terminal.terminal_at_monotonic_ms,
      `terminal ${terminal.ticket}`,
    );
    if (terminalTimestamp < submission.submitted_at_monotonic_ms) {
      throw new Error(`measured ticket ${submission.ticket} terminates before it was submitted`);
    }
    lastTerminal = Math.max(lastTerminal, terminalTimestamp);
  }

  const firstSubmit = measured[0].submitted_at_monotonic_ms;
  const lastSubmit = measured.at(-1).submitted_at_monotonic_ms;
  if (lastTerminal < lastSubmit) {
    throw new Error("last measured terminal precedes the last measured submission");
  }
  return {
    monotonic_clock: "performance.now",
    first_measured_submit_monotonic_ms: firstSubmit,
    last_measured_submit_monotonic_ms: lastSubmit,
    last_measured_terminal_monotonic_ms: lastTerminal,
    submit_span_ms: lastSubmit - firstSubmit,
    terminal_tail_ms: lastTerminal - lastSubmit,
    terminal_window_ms: lastTerminal - firstSubmit,
  };
}

function gpuSubmissionFrames(frames) {
  return frames.filter((frame) => frame.order_backend === "gpu" && frame.sort_refreshed === true);
}

function indexGpuMeasurements(measurements) {
  const byTicket = new Map();
  for (const measurement of measurements) {
    if (measurement.actual_backend !== "gpu") {
      throw new Error("order measurement does not identify GPU as its producing backend");
    }
    if (!isPositiveTicket(measurement.ticket)) {
      throw new Error("order measurement has an invalid ticket");
    }
    if (byTicket.has(measurement.ticket)) {
      throw new Error(`duplicate order measurement ticket ${measurement.ticket}`);
    }
    if (!isNonNegativeRevision(measurement.camera_revision)) {
      throw new Error(`order measurement ${measurement.ticket} has an invalid camera revision`);
    }
    if (!Number.isFinite(measurement.gpu_complete_ms) || measurement.gpu_complete_ms < 0) {
      throw new Error(`order measurement ${measurement.ticket} has invalid completion timing`);
    }
    validateCountReceipt(measurement, `order measurement ${measurement.ticket}`);
    byTicket.set(measurement.ticket, measurement);
  }
  return byTicket;
}

function indexCpuMeasurements(measurements) {
  const byTicket = new Map();
  for (const measurement of measurements) {
    if (measurement.actual_backend !== "cpu") {
      throw new Error("CPU order measurement does not identify CPU as its producing backend");
    }
    if (!isPositiveTicket(measurement.ticket) || measurement.ticket % 2 !== 0) {
      throw new Error("CPU order measurement has an invalid ticket");
    }
    if (byTicket.has(measurement.ticket)) {
      throw new Error(`duplicate CPU order measurement ticket ${measurement.ticket}`);
    }
    if (!isNonNegativeRevision(measurement.camera_revision)) {
      throw new Error(`CPU order measurement ${measurement.ticket} has an invalid camera revision`);
    }
    for (const field of ["preprocess_ms", "sort_ms", "frame_complete_ms"]) {
      if (!Number.isFinite(measurement[field]) || measurement[field] < 0) {
        throw new Error(`CPU order measurement ${measurement.ticket} has invalid ${field}`);
      }
    }
    validateCountReceipt(measurement, `CPU order measurement ${measurement.ticket}`);
    byTicket.set(measurement.ticket, measurement);
  }
  return byTicket;
}

function indexOrderMeasurementFailures(failures) {
  const byTicket = new Map();
  const allowedReasons = new Set(["readback_map", "generation_invalidated"]);
  for (const failure of failures) {
    if (failure.actual_backend !== "cpu" && failure.actual_backend !== "gpu") {
      throw new Error("order measurement failure has an invalid producing backend");
    }
    if (!isPositiveTicket(failure.ticket)
        || (failure.actual_backend === "cpu") !== (failure.ticket % 2 === 0)) {
      throw new Error("order measurement failure has an invalid namespaced ticket");
    }
    if (byTicket.has(failure.ticket)) {
      throw new Error(`duplicate order measurement failure ticket ${failure.ticket}`);
    }
    if (!isNonNegativeRevision(failure.camera_revision)) {
      throw new Error(`order measurement failure ${failure.ticket} has an invalid camera revision`);
    }
    if (!allowedReasons.has(failure.reason)) {
      throw new Error(`order measurement failure ${failure.ticket} has unknown reason ${failure.reason}`);
    }
    byTicket.set(failure.ticket, failure);
  }
  return byTicket;
}

function indexGpuMeasurementFailures(failures) {
  const byTicket = new Map();
  const allowedReasons = new Set(["readback_map", "generation_invalidated"]);
  for (const failure of failures) {
    if (failure.actual_backend !== "gpu") {
      throw new Error("order measurement failure does not identify GPU as its producing backend");
    }
    if (!isPositiveTicket(failure.ticket)) {
      throw new Error("order measurement failure has an invalid ticket");
    }
    if (byTicket.has(failure.ticket)) {
      throw new Error(`duplicate order measurement failure ticket ${failure.ticket}`);
    }
    if (!isNonNegativeRevision(failure.camera_revision)) {
      throw new Error(`order measurement failure ${failure.ticket} has an invalid camera revision`);
    }
    if (!allowedReasons.has(failure.reason)) {
      throw new Error(`order measurement failure ${failure.ticket} has unknown reason ${failure.reason}`);
    }
    byTicket.set(failure.ticket, failure);
  }
  return byTicket;
}

export function validateTerminalTicketLedger({
  submissions,
  measurements,
  cpuMeasurements = [],
  failures,
}) {
  if (!Array.isArray(submissions) || submissions.length === 0) {
    throw new Error("strict order evidence has no issued-ticket ledger");
  }
  const successByTicket = indexGpuMeasurements(measurements);
  for (const [ticket, measurement] of indexCpuMeasurements(cpuMeasurements)) {
    if (successByTicket.has(ticket)) {
      throw new Error(`order ticket ${ticket} has more than one success receipt`);
    }
    successByTicket.set(ticket, measurement);
  }
  const failureByTicket = indexOrderMeasurementFailures(failures);
  const seenSubmissions = new Set();
  for (const submission of submissions) {
    const backend = submission.actual_backend;
    if ((backend !== "cpu" && backend !== "gpu") || !isPositiveTicket(submission.ticket)
        || !isNonNegativeRevision(submission.camera_revision)
        || (backend === "cpu") !== (submission.ticket % 2 === 0)) {
      throw new Error("issued order ticket ledger contains an invalid submission");
    }
    if (seenSubmissions.has(submission.ticket)) {
      throw new Error(`issued order ticket ledger repeats ticket ${submission.ticket}`);
    }
    seenSubmissions.add(submission.ticket);
    const success = successByTicket.get(submission.ticket);
    const failure = failureByTicket.get(submission.ticket);
    if (success && failure) {
      throw new Error(
        `issued order ticket ${submission.ticket} has contradictory success and failure receipts`,
      );
    }
    const terminal = success ?? failure;
    if (!terminal) {
      throw new Error(`issued order ticket ${submission.ticket} has no terminal receipt`);
    }
    if (terminal.actual_backend !== backend) {
      throw new Error(
        `issued order ticket ${submission.ticket} backend ${backend} does not match terminal ` +
        `${terminal.actual_backend}`,
      );
    }
    if (terminal.camera_revision !== submission.camera_revision) {
      throw new Error(
        `issued order ticket ${submission.ticket} revision ${submission.camera_revision} ` +
        `does not match terminal revision ${terminal.camera_revision}`,
      );
    }
    if (failure) {
      throw new Error(
        `issued order ticket ${submission.ticket} terminated with structured failure ${failure.reason}`,
      );
    }
  }
}

function gpuSubmissionJoins(frames, measurements, failures = []) {
  const submissions = gpuSubmissionFrames(frames);
  const receiptByTicket = indexGpuMeasurements(measurements);
  const failureByTicket = indexGpuMeasurementFailures(failures);
  const seenSubmissionTickets = new Set();
  const joins = new Map();

  for (const ticket of receiptByTicket.keys()) {
    if (failureByTicket.has(ticket)) {
      throw new Error(`GPU measurement ticket ${ticket} has contradictory success and failure receipts`);
    }
  }

  for (const frame of submissions) {
    const ticket = frame.submitted_measurement_ticket;
    if (!isPositiveTicket(ticket)) {
      throw new Error(
        `GPU sort refresh frame ${frame.frame_index ?? "unknown"} lacks a positive submission ticket`,
      );
    }
    if (seenSubmissionTickets.has(ticket)) {
      throw new Error(`duplicate measured-frame GPU submission ticket ${ticket}`);
    }
    seenSubmissionTickets.add(ticket);
    if (!isNonNegativeRevision(frame.camera_revision)) {
      throw new Error(`GPU submission ticket ${ticket} has an invalid frame camera revision`);
    }
    const failure = failureByTicket.get(ticket);
    if (failure) {
      if (failure.camera_revision !== frame.camera_revision) {
        throw new Error(
          `GPU failure ticket ${ticket} revision ${failure.camera_revision} ` +
          `does not match submitted frame revision ${frame.camera_revision}`,
        );
      }
      throw new Error(
        `GPU measurement ticket ${ticket} terminated with structured failure ${failure.reason}`,
      );
    }
    const measurement = receiptByTicket.get(ticket);
    if (!measurement) {
      throw new Error(`GPU terminal receipts are incomplete; missing measured-frame ticket(s): ${ticket}`);
    }
    if (measurement.camera_revision !== frame.camera_revision) {
      throw new Error(
        `GPU receipt ticket ${ticket} revision ${measurement.camera_revision} ` +
        `does not match submitted frame revision ${frame.camera_revision}`,
      );
    }
    joins.set(frame, measurement);
  }

  return { submissions, receiptByTicket, failureByTicket, joins };
}

/** Join provisional counts to the terminal receipt from the same backend,
 * ticket, and camera revision. CPU and GPU ticket namespaces stay disjoint.
 */
export function joinOrderingEvidence({
  requestedBackend,
  frames,
  measurements,
  cpuMeasurements = [],
  failures = [],
}) {
  const joins = requestedBackend === "cpu"
    ? new Map()
    : gpuSubmissionJoins(frames, measurements, failures).joins;
  const cpuMeasurementsByTicket = indexCpuMeasurements(cpuMeasurements);
  const cpuJoins = new Map();
  for (const frame of frames) {
    if (frame.order_backend !== "cpu" || frame.sort_refreshed !== true) continue;
    const ticket = frame.submitted_measurement_ticket;
    const measurement = cpuMeasurementsByTicket.get(ticket);
    if (!measurement) continue;
    if (measurement.camera_revision !== frame.camera_revision) {
      throw new Error(
        `CPU receipt ticket ${ticket} revision ${measurement.camera_revision} ` +
        `does not match submitted frame revision ${frame.camera_revision}`,
      );
    }
    cpuJoins.set(frame, measurement);
  }
  const gpuMeasurementsByRevision = new Map();
  for (const measurement of measurements) {
    const previous = gpuMeasurementsByRevision.get(measurement.camera_revision);
    if (!previous || measurement.ticket > previous.ticket) {
      gpuMeasurementsByRevision.set(measurement.camera_revision, measurement);
    }
  }
  const cpuMeasurementsByRevision = new Map();
  for (const measurement of cpuMeasurements) {
    const previous = cpuMeasurementsByRevision.get(measurement.camera_revision);
    if (!previous || measurement.ticket > previous.ticket) {
      cpuMeasurementsByRevision.set(measurement.camera_revision, measurement);
    }
  }
  if (requestedBackend !== "cpu" && failures.length > 0) {
    const failure = failures[0];
    throw new Error(
      `${requestedBackend} benchmark observed structured GPU measurement failure ` +
      `ticket=${failure.ticket} revision=${failure.camera_revision} reason=${failure.reason}`,
    );
  }

  return frames.map((frame) => {
    const measurement = joins.get(frame) ?? cpuJoins.get(frame);
    if (!measurement) {
      const legacyCpuCountIsCurrent = frame.order_backend === "cpu"
        && cpuMeasurements.length === 0
        && isNonNegativeRevision(frame.camera_revision)
        && frame.visible_count_revision === frame.camera_revision
        && frame.visible_count_pending === false;
      const cachedMeasurement = frame.sort_refreshed === false
        && isNonNegativeRevision(frame.camera_revision)
        && frame.visible_count_revision === frame.camera_revision
        && frame.visible_count_pending === false
        ? frame.order_backend === "gpu"
          ? gpuMeasurementsByRevision.get(frame.camera_revision)
          : cpuMeasurementsByRevision.get(frame.camera_revision)
        : null;
      const cachedCountIsCurrent = cachedMeasurement != null
        && frame.visible === cachedMeasurement.visible
        && frame.drawn === cachedMeasurement.drawn;
      return {
        ...frame,
        ...(cachedCountIsCurrent ? {
          count_semantics: cachedMeasurement.count_semantics,
          visible: cachedMeasurement.visible,
          contributor: cachedMeasurement.contributor,
          drawn: cachedMeasurement.drawn,
          exact_contributor_compaction: cachedMeasurement.exact_contributor_compaction,
        } : {}),
        visible_count_source: legacyCpuCountIsCurrent
          ? "synchronous_cpu_order"
          : cachedCountIsCurrent
            ? frame.order_backend === "gpu"
              ? "cached_gpu_order_receipt"
              : "cached_cpu_order_receipt"
            : "excluded_unjoined_frame_count",
        visible_count_ticket: cachedCountIsCurrent ? cachedMeasurement.ticket : null,
        count_statistics_eligible: legacyCpuCountIsCurrent || cachedCountIsCurrent,
        // Completion fields harvested on a non-submitting frame belong to a
        // different ticket. The standalone receipt remains in
        // order-measurements.jsonl; do not double-count it in frame summary.
        gpu_complete_ms: null,
        gpu_preprocess_ms: null,
        gpu_radix_ms: null,
        gpu_order_ms: null,
      };
    }
    return {
      ...frame,
      count_semantics: measurement.count_semantics,
      visible: measurement.visible,
      contributor: measurement.contributor,
      drawn: measurement.drawn,
      exact_contributor_compaction: measurement.exact_contributor_compaction,
      visible_count_revision: measurement.camera_revision,
      visible_count_pending: false,
      visible_count_source: frame.order_backend === "gpu"
        ? "gpu_order_receipt"
        : "cpu_order_receipt",
      visible_count_ticket: measurement.ticket,
      count_statistics_eligible: true,
      gpu_complete_ms: frame.order_backend === "gpu" ? measurement.gpu_complete_ms : null,
      gpu_preprocess_ms: frame.order_backend === "gpu"
        ? measurement.gpu_preprocess_ms ?? null
        : null,
      gpu_radix_ms: frame.order_backend === "gpu" ? measurement.gpu_radix_ms ?? null : null,
      gpu_order_ms: frame.order_backend === "gpu" ? measurement.gpu_order_ms ?? null : null,
      completed_measurement_ticket: measurement.ticket,
      completed_measurement_revision: measurement.camera_revision,
      completed_measurement_timing_source: measurement.timing_source ?? null,
      completed_visible: measurement.visible,
      completed_contributor: measurement.contributor,
      completed_drawn: measurement.drawn,
      completed_exact_contributor_compaction: measurement.exact_contributor_compaction,
    };
  });
}

/** Validate the final, post-join artifact. */
export function validateOrderingEvidence({
  requestedBackend,
  frames,
  measurements,
  failures = [],
  fixedCameraReuse = false,
}) {
  const nonzeroCpuFrames = frames.filter(
    (frame) => frame.order_backend === "cpu"
      && frame.count_statistics_eligible === true
      && [
        "synchronous_cpu_order",
        "cpu_order_receipt",
        "cached_cpu_order_receipt",
      ].includes(frame.visible_count_source)
      && frame.visible_count_revision === frame.camera_revision
      && frame.visible_count_pending === false
      && frame.visible > 0
      && (frame.exact_contributor_compaction === true
        ? frame.drawn === frame.contributor
        : frame.drawn === frame.visible),
  );

  if (requestedBackend === "cpu") {
    if (nonzeroCpuFrames.length === 0) {
      throw new Error("CPU benchmark has no frame with a non-zero exact visible/drawn count");
    }
    return;
  }

  const fallbackFrames = frames.filter((frame) => frame.gpu_sort_fallback === true);
  if (requestedBackend === "gpu" && fallbackFrames.length > 0) {
    throw new Error(`GPU benchmark silently fell back on ${fallbackFrames.length} measured frame(s)`);
  }
  if (requestedBackend === "gpu" && frames.some((frame) => frame.order_backend !== "gpu")) {
    throw new Error("GPU benchmark contains a measured frame that did not use the GPU backend");
  }
  const adaptiveFailureFrame = frames.find((frame) => frame.adaptive_gpu_failure != null);
  if (adaptiveFailureFrame) {
    throw new Error(
      `${requestedBackend} benchmark observed adaptive GPU failure ${adaptiveFailureFrame.adaptive_gpu_failure}`,
    );
  }
  const nonzeroCachedGpuFrames = frames.filter(
    (frame) => frame.order_backend === "gpu"
      && frame.sort_refreshed === false
      && frame.count_statistics_eligible === true
      && frame.visible_count_source === "cached_gpu_order_receipt"
      && frame.visible_count_revision === frame.camera_revision
      && frame.visible_count_pending === false
      && frame.visible > 0
      && (frame.exact_contributor_compaction
        ? frame.drawn === frame.contributor
        : frame.drawn === frame.visible),
  );

  if (measurements.length === 0 && !(fixedCameraReuse && nonzeroCpuFrames.length > 0)) {
    throw new Error(`${requestedBackend} benchmark has no completed GPU order measurement`);
  }

  const { submissions, joins } = gpuSubmissionJoins(frames, measurements, failures);
  if (failures.length > 0) {
    const failure = failures[0];
    throw new Error(
      `${requestedBackend} benchmark observed structured GPU measurement failure ` +
      `ticket=${failure.ticket} revision=${failure.camera_revision} reason=${failure.reason}`,
    );
  }
  if (submissions.length === 0
      && !(fixedCameraReuse && (nonzeroCpuFrames.length > 0 || nonzeroCachedGpuFrames.length > 0))) {
    throw new Error(`${requestedBackend} benchmark has no measured GPU sort submission`);
  }
  for (const frame of submissions) {
    const measurement = joins.get(frame);
    if (!measurement) throw new Error("internal GPU receipt join failure");
    if (measurement.visible <= 0) {
      throw new Error(`order measurement ${measurement.ticket} lacks an exact non-zero count receipt`);
    }
    if (frame.visible !== measurement.visible
        || frame.contributor !== measurement.contributor
        || frame.drawn !== measurement.drawn
        || frame.exact_contributor_compaction !== measurement.exact_contributor_compaction) {
      throw new Error(
        `GPU frame ticket ${measurement.ticket} V/C/D counts do not match its terminal receipt`,
      );
    }
    if (frame.visible_count_ticket !== measurement.ticket
        || frame.visible_count_source !== "gpu_order_receipt"
        || frame.count_statistics_eligible !== true
        || frame.visible_count_revision !== measurement.camera_revision
        || frame.visible_count_pending !== false) {
      throw new Error(
        `GPU frame ticket ${measurement.ticket} lacks a terminal ticket+revision count join`,
      );
    }
  }

  if (nonzeroCpuFrames.length === 0
      && nonzeroCachedGpuFrames.length === 0
      && submissions.length === 0) {
    throw new Error("all measured frames are zero and no exact measured-frame GPU receipt completed");
  }

  if (requestedBackend === "adaptive" && !fixedCameraReuse) {
    const usedBackends = new Set(frames.map((frame) => frame.order_backend));
    if (!usedBackends.has("cpu") || !usedBackends.has("gpu")) {
      throw new Error("Adaptive benchmark did not exercise both CPU and GPU ordering");
    }
  }
}
