export const CURRENT_STATS_RING_CAPACITY = 4;
export const CURRENT_STATS_FINAL_DRAIN_TIMEOUT_MS = 60_000;

const PROTOCOL_POLICY = Object.freeze({
  isolated_terminal: Object.freeze({
    submissionMode: "isolated_current_stats_terminal",
    submissionGate: "previous_terminal",
    frameWallSource: "isolated_terminal_progression",
  }),
  sustained_window: Object.freeze({
    submissionMode: "overlapped_bounded_current_stats",
    submissionGate: "bounded_pending_capacity",
    frameWallSource: "request_animation_frame_interval",
  }),
});

function positiveInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new TypeError(`${name} must be a positive integer`);
  }
  return value;
}

function nonNegativeInteger(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative integer`);
  }
  return value;
}

function finiteMonotonicMs(value, name) {
  if (!Number.isFinite(value) || value < 0) {
    throw new TypeError(`${name} must be a finite non-negative monotonic timestamp`);
  }
  return value;
}

function policyFor(protocol) {
  const policy = PROTOCOL_POLICY[protocol];
  if (!policy) {
    throw new TypeError(
      "current-stats protocol must be isolated_terminal or sustained_window",
    );
  }
  return policy;
}

export function validateCurrentStatsScheduleEvidence({
  evidence,
  protocol,
  frameWallSource,
  expectedLogicalFrameCount,
  expectedWarmupFrameCount,
}) {
  const policy = policyFor(protocol);
  positiveInteger(expectedLogicalFrameCount, "expected logical frame count");
  const expectedWarmup = nonNegativeInteger(
    expectedWarmupFrameCount,
    "expected warmup frame count",
  );
  if (expectedWarmup > expectedLogicalFrameCount) {
    throw new Error("expected warmup frame count exceeds the logical frame count");
  }
  if (!evidence || typeof evidence !== "object" || Array.isArray(evidence)) {
    throw new TypeError("renderer current-stats schedule evidence is required");
  }
  if (evidence.protocol !== protocol
      || evidence.submission_mode !== policy.submissionMode
      || evidence.submission_gate !== policy.submissionGate
      || evidence.frame_wall_source !== policy.frameWallSource
      || frameWallSource !== policy.frameWallSource) {
    throw new Error(
      `current-stats schedule labels do not prove ${protocol} behavior`,
    );
  }
  if (evidence.state !== "complete") {
    throw new Error(`current-stats schedule ended in ${evidence.state ?? "missing"}`);
  }
  const capacity = positiveInteger(
    evidence.pending_capacity,
    "current-stats pending capacity",
  );
  const submitted = nonNegativeInteger(
    evidence.submitted_logical_count,
    "submitted logical count",
  );
  const terminal = nonNegativeInteger(
    evidence.terminal_logical_count,
    "terminal logical count",
  );
  const issued = nonNegativeInteger(evidence.issued_count, "issued count");
  const terminalCount = nonNegativeInteger(evidence.terminal_count, "terminal count");
  const peakPending = nonNegativeInteger(evidence.peak_pending_count, "peak pending count");
  const drainAfter = nonNegativeInteger(
    evidence.final_drain_started_after_logical_submit_count,
    "final drain submit count",
  );
  const drawAtDrain = nonNegativeInteger(
    evidence.draw_count_at_final_drain_start,
    "draw count at final drain start",
  );
  const drawAtCompletion = nonNegativeInteger(
    evidence.draw_count_at_completion,
    "draw count at completion",
  );
  const deferredPresentations = nonNegativeInteger(
    evidence.deferred_presentation_count,
    "deferred current-stats presentation count",
  );
  const peakDeferredBeforeIssue = nonNegativeInteger(
    evidence.peak_deferred_presentations_before_issue,
    "peak deferred current-stats presentations before issue",
  );
  const deferredAttempts = evidence.deferred_presentations;
  const issuedAttempts = evidence.issued_presentations;
  if (!Array.isArray(deferredAttempts) || !Array.isArray(issuedAttempts)) {
    throw new Error("current-stats schedule lacks its per-presentation attempt ledger");
  }
  positiveInteger(evidence.final_drain_timeout_ms, "final drain timeout");
  const presentedAttempts = nonNegativeInteger(
    evidence.presented_attempt_count,
    "presented current-stats attempt count",
  );

  if (submitted !== expectedLogicalFrameCount
      || terminal !== expectedLogicalFrameCount
      || drainAfter !== expectedLogicalFrameCount) {
    throw new Error(
      `current-stats logical ledger mismatch: submitted=${submitted} terminal=${terminal} ` +
      `drain_after=${drainAfter} expected=${expectedLogicalFrameCount}`,
    );
  }
  if (issued !== terminalCount || issued < expectedLogicalFrameCount) {
    throw new Error(
      `current-stats issued/terminal ledger mismatch: issued=${issued} terminal=${terminalCount}`,
    );
  }
  if (peakPending < 1 || peakPending > capacity) {
    throw new Error(
      `current-stats peak pending count ${peakPending} exceeds capacity ${capacity}`,
    );
  }
  if (protocol === "isolated_terminal" && peakPending !== 1) {
    throw new Error("isolated current-stats schedule admitted overlapping tickets");
  }
  if (drawAtDrain !== drawAtCompletion) {
    throw new Error(
      `current-stats final drain submitted a new draw: ${drawAtDrain} -> ${drawAtCompletion}`,
    );
  }
  if (peakDeferredBeforeIssue > deferredPresentations) {
    throw new Error(
      "current-stats peak deferred presentations exceeds the total deferred count",
    );
  }
  if (deferredAttempts.length !== deferredPresentations
      || issuedAttempts.length !== issued
      || presentedAttempts !== deferredAttempts.length + issuedAttempts.length) {
    throw new Error("current-stats presentation attempt totals are inconsistent");
  }
  const validAttemptIdentity = (record, logicalIndex) => (
    ["preflight", "warmup", "measured"].includes(record.phase)
    && ((logicalIndex === null && record.phase === "preflight")
      || (logicalIndex !== null && record.phase !== "preflight"))
    && (record.trace_frame_index === null
      || (Number.isSafeInteger(record.trace_frame_index)
        && record.trace_frame_index >= 0))
  );
  const expectedPhase = (logicalIndex) => (
    logicalIndex === null
      ? "preflight"
      : logicalIndex < expectedWarmup ? "warmup" : "measured"
  );
  const deferredByMember = new Map();
  for (const record of deferredAttempts) {
    const logicalIndex = record.logical_submission_index;
    const formalFields = [
      record.auxiliary_formal_ticket,
      record.auxiliary_formal_backend,
      record.auxiliary_formal_submitted_at_monotonic_ms,
    ];
    const hasNoFormal = formalFields.every((value) => value === null);
    const hasValidFormal = protocol === "isolated_terminal"
      && Number.isSafeInteger(record.auxiliary_formal_ticket)
      && record.auxiliary_formal_ticket > 0
      && ["cpu", "gpu"].includes(record.auxiliary_formal_backend)
      && Number.isFinite(record.auxiliary_formal_submitted_at_monotonic_ms)
      && record.auxiliary_formal_submitted_at_monotonic_ms
        >= record.observed_at_monotonic_ms;
    if (!Number.isSafeInteger(record.attempt_index) || record.attempt_index < 0
        || !Number.isSafeInteger(record.camera_revision) || record.camera_revision < 0
        || !Number.isFinite(record.observed_at_monotonic_ms)
        || record.observed_at_monotonic_ms < 0
        || !validAttemptIdentity(record, logicalIndex)
        || record.phase !== expectedPhase(logicalIndex)
        || (!hasNoFormal && !hasValidFormal)
        || (logicalIndex !== null
          && (!Number.isSafeInteger(logicalIndex) || logicalIndex < 0
            || logicalIndex >= expectedLogicalFrameCount))) {
      throw new Error("current-stats deferred attempt has invalid identity");
    }
    const key = `${record.phase}:${logicalIndex ?? "preflight"}`;
    const group = deferredByMember.get(key) ?? [];
    if (record.attempt_index !== group.length) {
      throw new Error(`current-stats member ${key} has non-contiguous deferred attempts`);
    }
    if (group.length > 0
        && (group[0].camera_revision !== record.camera_revision
          || group[0].trace_frame_index !== record.trace_frame_index)) {
      throw new Error(`current-stats member ${key} changed camera or trace identity`);
    }
    group.push(record);
    deferredByMember.set(key, group);
  }
  const logicalIssued = new Set();
  const issuedMembers = new Set();
  const issuedTickets = new Set();
  for (const record of issuedAttempts) {
    const logicalIndex = record.logical_submission_index;
    if (!Number.isSafeInteger(record.attempt_index) || record.attempt_index < 0
        || !Number.isSafeInteger(record.ticket) || record.ticket <= 0
        || !Number.isSafeInteger(record.camera_revision) || record.camera_revision < 0
        || !Number.isFinite(record.submitted_at_monotonic_ms)
        || record.submitted_at_monotonic_ms < 0
        || !validAttemptIdentity(record, logicalIndex)
        || record.phase !== expectedPhase(logicalIndex)
        || issuedTickets.has(record.ticket)
        || (logicalIndex !== null
          && (!Number.isSafeInteger(logicalIndex) || logicalIndex < 0
            || logicalIndex >= expectedLogicalFrameCount))) {
      throw new Error("current-stats issued attempt has invalid identity");
    }
    const key = `${record.phase}:${logicalIndex ?? "preflight"}`;
    const deferred = deferredByMember.get(key) ?? [];
    if (record.attempt_index !== deferred.length
        || (deferred.length > 0
          && (deferred[0].camera_revision !== record.camera_revision
            || deferred[0].trace_frame_index !== record.trace_frame_index
            || deferred.at(-1).observed_at_monotonic_ms
              > record.submitted_at_monotonic_ms))) {
      throw new Error(`current-stats member ${key} did not end with its issued presentation`);
    }
    if (issuedMembers.has(key)) {
      throw new Error(`current-stats member ${key} issued more than once`);
    }
    issuedMembers.add(key);
    issuedTickets.add(record.ticket);
    if (logicalIndex !== null) {
      if (logicalIssued.has(logicalIndex)) {
        throw new Error(`current-stats logical member ${logicalIndex} issued more than once`);
      }
      logicalIssued.add(logicalIndex);
    }
  }
  for (const key of deferredByMember.keys()) {
    if (!issuedMembers.has(key)) {
      throw new Error(`current-stats member ${key} lacks its final issued presentation`);
    }
  }
  if (logicalIssued.size !== expectedLogicalFrameCount) {
    throw new Error("current-stats logical member attempt ledger is incomplete");
  }
  return evidence;
}

export function createCurrentStatsSchedule({
  protocol,
  warmupFrames,
  measuredFrames,
  pendingCapacity = CURRENT_STATS_RING_CAPACITY,
  finalDrainTimeoutMs = CURRENT_STATS_FINAL_DRAIN_TIMEOUT_MS,
}) {
  const policy = policyFor(protocol);
  const warmup = nonNegativeInteger(warmupFrames, "warmup frame count");
  const measured = positiveInteger(measuredFrames, "measured frame count");
  const capacity = positiveInteger(pendingCapacity, "current-stats pending capacity");
  const drainTimeout = positiveInteger(finalDrainTimeoutMs, "final drain timeout");
  const expectedLogicalFrameCount = warmup + measured;

  let scheduleState = "submitting";
  let request = null;
  let nextLogicalSubmissionIndex = 0;
  let nextIssueOrdinal = 0;
  let nextRecycleOrdinal = 0;
  let submittedLogicalCount = 0;
  let terminalLogicalCount = 0;
  let terminalCount = 0;
  let peakPendingCount = 0;
  let deferredPresentationCount = 0;
  let peakDeferredPresentationsBeforeIssue = 0;
  let drawCount = 0;
  let lastDrawAtMonotonicMs = null;
  let drawCountAtFinalDrainStart = null;
  let drawCountAtCompletion = null;
  const issuedTickets = new Set();
  const terminalTickets = new Set();
  const pendingByTicket = new Map();
  const completedByOrdinal = new Map();
  const deferredPresentationRecords = [];
  const issuedPresentationRecords = [];

  function traceFrameIndex(traceStep) {
    const value = traceStep?.traceFrameIndex ?? traceStep?.frame ?? null;
    if (value !== null && (!Number.isSafeInteger(value) || value < 0)) {
      throw new Error("current-stats trace frame index is invalid");
    }
    return value;
  }

  function pendingPreflight() {
    return [...pendingByTicket.values()].some((pending) => pending.priming);
  }

  function frameAction() {
    if (scheduleState === "draining" || scheduleState === "complete") return "drain";
    if (request !== null) return "draw";
    if (protocol === "isolated_terminal" && pendingByTicket.size > 0) {
      return "wait_terminal";
    }
    if (pendingPreflight()) return "wait_terminal";
    if (pendingByTicket.size >= capacity) return "fail_capacity";
    return "draw";
  }

  function completeIfDrained() {
    if (scheduleState !== "draining"
        || request !== null
        || pendingByTicket.size !== 0
        || completedByOrdinal.size !== 0
        || nextRecycleOrdinal !== nextIssueOrdinal) {
      return;
    }
    scheduleState = "complete";
    drawCountAtCompletion = drawCount;
  }

  function requireRequestWithinDeadline(nowMs) {
    finiteMonotonicMs(nowMs, "current-stats request progress timestamp");
    if (request === null) {
      throw new Error("current-stats request progress has no outstanding request");
    }
    if (nowMs - request.requestedAtMonotonicMs >= drainTimeout) {
      throw new Error(
        `renderer current-stats request did not issue within ${drainTimeout}ms`,
      );
    }
  }

  return {
    protocol,
    submissionMode: policy.submissionMode,
    submissionGate: policy.submissionGate,
    frameWallSource: policy.frameWallSource,

    get state() {
      return scheduleState;
    },
    get action() {
      return frameAction();
    },
    get requestOutstanding() {
      return request !== null;
    },
    get requestPhase() {
      return request?.phase ?? null;
    },
    get pendingCount() {
      return pendingByTicket.size;
    },
    get submittedCount() {
      return submittedLogicalCount;
    },
    get nextSubmissionIndex() {
      return nextLogicalSubmissionIndex;
    },
    get complete() {
      return scheduleState === "complete";
    },

    requireRequestWithinDeadline(nowMs) {
      requireRequestWithinDeadline(nowMs);
    },

    noteDraw(nowMs) {
      finiteMonotonicMs(nowMs, "animation-frame timestamp");
      const action = frameAction();
      if (action === "fail_capacity") {
        throw new Error(
          `renderer current-stats ring remained busy at bounded capacity ${capacity}`,
        );
      }
      if (action !== "draw") {
        throw new Error(`current-stats schedule forbids draw while ${action}`);
      }
      drawCount += 1;
      lastDrawAtMonotonicMs = nowMs;
    },

    beginRequest({ nowMs, priming = false, traceStep = null }) {
      finiteMonotonicMs(nowMs, "current-stats request timestamp");
      if (request !== null) {
        throw new Error("renderer current-stats request is already outstanding");
      }
      const action = frameAction();
      if (action === "fail_capacity") {
        throw new Error(
          `renderer current-stats ring remained busy at bounded capacity ${capacity}`,
        );
      }
      if (action !== "draw") {
        throw new Error(`current-stats schedule cannot issue while ${action}`);
      }
      const phase = priming
        ? "preflight"
        : nextLogicalSubmissionIndex < warmup ? "warmup" : "measured";
      request = {
        phase,
        priming: Boolean(priming),
        logicalSubmissionIndex: priming ? null : nextLogicalSubmissionIndex,
        requestedAtMonotonicMs: nowMs,
        traceStep,
        deferredPresentations: 0,
        presentationCameraRevision: null,
      };
      return { ...request };
    },

    recordDeferredPresentation({
      submission,
      ticket,
      cameraRevision,
      traceStep,
      observedAtMonotonicMs,
    }) {
      finiteMonotonicMs(
        observedAtMonotonicMs,
        "deferred current-stats presentation timestamp",
      );
      if (request === null) {
        throw new Error(
          "renderer deferred a current-stats presentation without an outstanding request",
        );
      }
      if (submission !== "not_requested" || ticket !== null) {
        throw new Error(
          "deferred current-stats presentation exposed a ticket or non-empty submission",
        );
      }
      if (!Number.isSafeInteger(cameraRevision) || cameraRevision < 0) {
        throw new Error("deferred current-stats presentation has an invalid camera revision");
      }
      if (traceStep !== request.traceStep) {
        throw new Error("deferred current-stats presentation changed its logical trace step");
      }
      if (request.presentationCameraRevision === null) {
        request.presentationCameraRevision = cameraRevision;
      } else if (request.presentationCameraRevision !== cameraRevision) {
        throw new Error("deferred current-stats presentation changed its camera revision");
      }
      requireRequestWithinDeadline(observedAtMonotonicMs);
      request.deferredPresentations += 1;
      deferredPresentationCount += 1;
      peakDeferredPresentationsBeforeIssue = Math.max(
        peakDeferredPresentationsBeforeIssue,
        request.deferredPresentations,
      );
      const record = {
        phase: request.phase,
        logical_submission_index: request.logicalSubmissionIndex,
        attempt_index: request.deferredPresentations - 1,
        camera_revision: cameraRevision,
        trace_frame_index: traceFrameIndex(traceStep),
        observed_at_monotonic_ms: observedAtMonotonicMs,
        auxiliary_formal_ticket: null,
        auxiliary_formal_backend: null,
        auxiliary_formal_submitted_at_monotonic_ms: null,
      };
      deferredPresentationRecords.push(record);
      return record;
    },

    recordAuxiliaryFormal({
      deferredPresentation,
      ticket,
      backend,
      submittedAtMonotonicMs,
    }) {
      finiteMonotonicMs(
        submittedAtMonotonicMs,
        "auxiliary formal submit timestamp",
      );
      if (protocol !== "isolated_terminal"
          || request === null
          || deferredPresentation !== deferredPresentationRecords.at(-1)
          || deferredPresentation.phase !== request.phase
          || deferredPresentation.logical_submission_index
            !== request.logicalSubmissionIndex
          || deferredPresentation.auxiliary_formal_ticket !== null
          || !Number.isSafeInteger(ticket) || ticket <= 0
          || !["cpu", "gpu"].includes(backend)
          || submittedAtMonotonicMs < deferredPresentation.observed_at_monotonic_ms) {
        throw new Error("deferred current-stats attempt has invalid auxiliary formal identity");
      }
      deferredPresentation.auxiliary_formal_ticket = ticket;
      deferredPresentation.auxiliary_formal_backend = backend;
      deferredPresentation.auxiliary_formal_submitted_at_monotonic_ms =
        submittedAtMonotonicMs;
    },

    recordIssued({ ticket, stats, submittedAtMonotonicMs }) {
      finiteMonotonicMs(submittedAtMonotonicMs, "current-stats submit timestamp");
      if (request === null) {
        throw new Error("renderer current-stats ticket was issued without a request");
      }
      if (request.presentationCameraRevision !== null
          && stats?.cameraRevision !== request.presentationCameraRevision) {
        throw new Error("issued current-stats presentation changed its deferred camera revision");
      }
      if (!Number.isSafeInteger(stats?.cameraRevision) || stats.cameraRevision < 0) {
        throw new Error("issued current-stats presentation has an invalid camera revision");
      }
      if (!Number.isSafeInteger(ticket) || ticket <= 0) {
        throw new Error(`renderer current-stats issued invalid ticket=${ticket}`);
      }
      if (issuedTickets.has(ticket) || terminalTickets.has(ticket)) {
        throw new Error(`duplicate renderer current-stats ticket=${ticket}`);
      }
      if (pendingByTicket.size >= capacity) {
        throw new Error(
          `renderer current-stats ring exceeded bounded capacity ${capacity}`,
        );
      }
      const pending = {
        ...request,
        ticket,
        stats,
        submittedAtMonotonicMs,
        frameProgressAtMonotonicMs: lastDrawAtMonotonicMs,
        ordinal: nextIssueOrdinal,
      };
      nextIssueOrdinal += 1;
      issuedTickets.add(ticket);
      pendingByTicket.set(ticket, pending);
      peakPendingCount = Math.max(peakPendingCount, pendingByTicket.size);
      request = null;
      issuedPresentationRecords.push({
        phase: pending.phase,
        logical_submission_index: pending.logicalSubmissionIndex,
        attempt_index: pending.deferredPresentations,
        ticket,
        camera_revision: stats.cameraRevision,
        trace_frame_index: traceFrameIndex(pending.traceStep),
        submitted_at_monotonic_ms: submittedAtMonotonicMs,
      });

      if (!pending.priming) {
        nextLogicalSubmissionIndex += 1;
        submittedLogicalCount += 1;
        if (submittedLogicalCount === expectedLogicalFrameCount) {
          scheduleState = "draining";
          drawCountAtFinalDrainStart = drawCount;
        }
      }
      return pending;
    },

    pendingForTerminal({ ticket, status }) {
      if (status !== "ready") {
        throw new Error(
          `renderer current-stats ticket=${ticket ?? "missing"} terminated with ${status ?? "missing"}`,
        );
      }
      if (!Number.isSafeInteger(ticket) || ticket <= 0) {
        throw new Error(`renderer current-stats terminal has invalid ticket=${ticket}`);
      }
      if (terminalTickets.has(ticket)) {
        throw new Error(`duplicate renderer current-stats terminal ticket=${ticket}`);
      }
      const pending = pendingByTicket.get(ticket);
      if (!pending) {
        throw new Error(`unknown renderer current-stats terminal ticket=${ticket}`);
      }
      return pending;
    },

    recordTerminal({ pending, joined, terminalAtMonotonicMs }) {
      finiteMonotonicMs(terminalAtMonotonicMs, "current-stats terminal timestamp");
      const current = pendingByTicket.get(pending?.ticket);
      if (current !== pending) {
        throw new Error(
          `renderer current-stats terminal ticket=${pending?.ticket ?? "missing"} is stale`,
        );
      }
      pendingByTicket.delete(pending.ticket);
      terminalTickets.add(pending.ticket);
      terminalCount += 1;
      if (!pending.priming) terminalLogicalCount += 1;
      completedByOrdinal.set(pending.ordinal, {
        ...pending,
        joined,
        terminalAtMonotonicMs,
      });
    },

    takeReadyInSubmissionOrder() {
      const ready = [];
      while (completedByOrdinal.has(nextRecycleOrdinal)) {
        ready.push(completedByOrdinal.get(nextRecycleOrdinal));
        completedByOrdinal.delete(nextRecycleOrdinal);
        nextRecycleOrdinal += 1;
      }
      completeIfDrained();
      return ready;
    },

    noteEmptyPoll(nowMs) {
      finiteMonotonicMs(nowMs, "current-stats poll timestamp");
      const expired = [...pendingByTicket.values()].find(
        (pending) => nowMs - pending.submittedAtMonotonicMs >= drainTimeout,
      );
      if (expired) {
        throw new Error(
          `renderer current-stats terminal ticket=${expired.ticket} timed out after ` +
          `${drainTimeout}ms while ${scheduleState}`,
        );
      }
    },

    evidence() {
      if (scheduleState !== "complete") {
        throw new Error(`current-stats schedule evidence is unavailable while ${scheduleState}`);
      }
      const evidence = {
        state: scheduleState,
        protocol,
        submission_mode: policy.submissionMode,
        submission_gate: policy.submissionGate,
        frame_wall_source: policy.frameWallSource,
        pending_capacity: capacity,
        submitted_logical_count: submittedLogicalCount,
        terminal_logical_count: terminalLogicalCount,
        issued_count: issuedTickets.size,
        terminal_count: terminalCount,
        peak_pending_count: peakPendingCount,
        deferred_presentation_count: deferredPresentationCount,
        peak_deferred_presentations_before_issue: peakDeferredPresentationsBeforeIssue,
        presented_attempt_count:
          deferredPresentationRecords.length + issuedPresentationRecords.length,
        deferred_presentations: deferredPresentationRecords,
        issued_presentations: issuedPresentationRecords,
        final_drain_started_after_logical_submit_count: submittedLogicalCount,
        draw_count_at_final_drain_start: drawCountAtFinalDrainStart,
        draw_count_at_completion: drawCountAtCompletion,
        final_drain_timeout_ms: drainTimeout,
      };
      return validateCurrentStatsScheduleEvidence({
        evidence,
        protocol,
        frameWallSource: policy.frameWallSource,
        expectedLogicalFrameCount,
        expectedWarmupFrameCount: warmup,
      });
    },
  };
}
