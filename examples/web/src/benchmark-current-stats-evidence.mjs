const IDENTITY_FIELDS = [
  "ticket",
  "plan",
  "scene_generation",
  "camera_revision",
  "viewport_generation",
  "contract_generation",
  "plan_set_generation",
  "order_generation",
  "raster_generation",
  "encode_attempt",
  "presentation_sequence",
];

function safeNonNegativeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} must be a non-negative safe integer`);
  }
  return value;
}

function validateIdentity(record, label) {
  for (const field of IDENTITY_FIELDS) {
    if (field === "plan") {
      if (!["cpu_post_sort", "gpu_post_sort", "gpu_preproject"].includes(record[field])) {
        throw new Error(`${label} has an invalid Exact plan`);
      }
    } else {
      safeNonNegativeInteger(record[field], `${label}.${field}`);
    }
  }
}
function validateReadyCounts(terminal, label, sourceCount) {
  if (!["draw_equals_visible", "indirect_draw_equals_visible", "indirect_draw_equals_contributor"]
    .includes(terminal.count_semantics)) {
    throw new Error(`${label} has invalid count semantics`);
  }
  for (const field of ["source_count", "visible", "contributor", "drawn"]) {
    safeNonNegativeInteger(terminal[field], `${label}.${field}`);
  }
  if (sourceCount != null && terminal.source_count !== sourceCount) {
    throw new Error(`${label} source count does not match the exact scene`);
  }
  if (terminal.contributor > terminal.visible || terminal.visible > terminal.source_count) {
    throw new Error(`${label} violates C <= V <= S`);
  }
  const expectedDrawn = terminal.count_semantics === "indirect_draw_equals_contributor"
    ? terminal.contributor
    : terminal.visible;
  if (terminal.drawn !== expectedDrawn) {
    throw new Error(`${label} drawn count disagrees with its Exact semantics`);
  }
}

function expectedPlan(frame) {
  if (frame.order_backend === "cpu") return "cpu_post_sort";
  if (frame.order_backend === "gpu" && frame.projected_execution === "compact") {
    return "gpu_preproject";
  }
  if (frame.order_backend === "gpu") return "gpu_post_sort";
  throw new Error(`frame ${frame.frame_index ?? "unknown"} has an invalid Exact backend`);
}

function frameIdentity(frame) {
  return {
    ticket: frame.current_stats_ticket,
    plan: frame.current_stats_plan,
    scene_generation: frame.current_stats_scene_generation,
    camera_revision: frame.current_stats_camera_revision,
    viewport_generation: frame.current_stats_viewport_generation,
    contract_generation: frame.current_stats_contract_generation,
    plan_set_generation: frame.current_stats_plan_set_generation,
    order_generation: frame.current_stats_order_generation,
    raster_generation: frame.current_stats_raster_generation,
    encode_attempt: frame.current_stats_encode_attempt,
    presentation_sequence: frame.current_stats_presentation_sequence,
  };
}

function assertSameIdentity(expected, actual, label) {
  for (const field of IDENTITY_FIELDS) {
    if (expected[field] !== actual[field]) {
      throw new Error(
        `${label} identity mismatch for ${field}: ${expected[field]} vs ${actual[field]}`,
      );
    }
  }
}

export function validateCurrentStatsTerminalLedger({ submissions, terminals, sourceCount = null }) {
  if (!Array.isArray(submissions) || submissions.length === 0) {
    throw new Error("Exact benchmark has no renderer current-stats submissions");
  }
  if (!Array.isArray(terminals)) {
    throw new Error("Exact benchmark current-stats terminals are missing");
  }
  const terminalByTicket = new Map();
  for (const terminal of terminals) {
    validateIdentity(terminal, `current-stats terminal ${terminal.ticket ?? "unknown"}`);
    if (terminalByTicket.has(terminal.ticket)) {
      throw new Error(`current-stats ticket ${terminal.ticket} has multiple terminals`);
    }
    terminalByTicket.set(terminal.ticket, terminal);
  }

  const seen = new Set();
  for (const submission of submissions) {
    validateIdentity(submission, `current-stats submission ${submission.ticket ?? "unknown"}`);
    if (seen.has(submission.ticket)) {
      throw new Error(`current-stats submission ticket ${submission.ticket} is duplicated`);
    }
    seen.add(submission.ticket);
    const terminal = terminalByTicket.get(submission.ticket);
    if (!terminal) {
      throw new Error(`current-stats ticket ${submission.ticket} has no terminal`);
    }
    assertSameIdentity(submission, terminal, `current-stats ticket ${submission.ticket}`);
    if (terminal.status !== "ready") {
      throw new Error(
        `current-stats ticket ${submission.ticket} terminated with ${terminal.status}`,
      );
    }
    validateReadyCounts(terminal, `current-stats terminal ${submission.ticket}`, sourceCount);
  }
  for (const ticket of terminalByTicket.keys()) {
    if (!seen.has(ticket)) {
      throw new Error(`current-stats terminal ticket ${ticket} has no submission`);
    }
  }
}

export function validateAuxiliaryCurrentStatsFormalLedger({
  runId,
  expectedLogicalFrameCount,
  deferredPresentationCount,
  deferredPresentations,
  submissions,
  terminals,
  terminalQueueThroughput = false,
}) {
  if (!Array.isArray(deferredPresentations)
      || !Array.isArray(submissions) || !Array.isArray(terminals)) {
    throw new Error("renderer-owned Exact artifact lacks its auxiliary formal ledger");
  }
  if (terminalQueueThroughput) {
    if (deferredPresentations.length !== 0
        || submissions.length !== 0 || terminals.length !== 0) {
      throw new Error("terminal throughput emitted auxiliary control formal work");
    }
    return;
  }
  safeNonNegativeInteger(
    deferredPresentationCount,
    "deferred current-stats presentation count",
  );
  safeNonNegativeInteger(expectedLogicalFrameCount, "expected logical frame count");
  if (submissions.length > deferredPresentationCount
      || submissions.length !== terminals.length) {
    throw new Error("auxiliary formal ledger disagrees with deferred control presentations");
  }
  const deferredByAttempt = new Map();
  for (const record of deferredPresentations) {
    const key = `${record.phase}:${record.logical_submission_index}:${record.attempt_index}`;
    if (deferredByAttempt.has(key)) {
      throw new Error("auxiliary formal ledger saw a duplicate deferred attempt identity");
    }
    deferredByAttempt.set(key, record);
  }
  const submissionByTicket = new Map();
  for (const record of submissions) {
    const attempt = deferredByAttempt.get(
      `${record.phase}:${record.logical_submission_index}:${record.attempt_index}`,
    );
    if (record.run_id !== runId
        || !["warmup", "measured", "preflight"].includes(record.phase)
        || !Number.isSafeInteger(record.logical_submission_index)
        || record.logical_submission_index < 0
        || record.logical_submission_index >= expectedLogicalFrameCount
        || !Number.isSafeInteger(record.ticket) || record.ticket <= 0
        || !Number.isSafeInteger(record.camera_revision) || record.camera_revision < 0
        || !Number.isSafeInteger(record.attempt_index) || record.attempt_index < 0
        || (record.trace_frame_index !== null
          && (!Number.isSafeInteger(record.trace_frame_index)
            || record.trace_frame_index < 0))
        || !Number.isFinite(record.deferred_at_monotonic_ms)
        || !["cpu", "gpu"].includes(record.actual_backend)
        || !Number.isFinite(record.submitted_at_monotonic_ms)
        || submissionByTicket.has(record.ticket)
        || !attempt
        || attempt.camera_revision !== record.camera_revision
        || attempt.trace_frame_index !== record.trace_frame_index
        || attempt.observed_at_monotonic_ms !== record.deferred_at_monotonic_ms
        || record.submitted_at_monotonic_ms < record.deferred_at_monotonic_ms) {
      throw new Error("auxiliary formal submission has invalid or duplicate identity");
    }
    submissionByTicket.set(record.ticket, record);
  }
  const terminalTickets = new Set();
  for (const terminal of terminals) {
    const submission = submissionByTicket.get(terminal.ticket);
    if (!submission || terminalTickets.has(terminal.ticket)
        || terminal.outcome !== "success" || terminal.reason !== null
        || terminal.run_id !== submission.run_id
        || terminal.phase !== submission.phase
        || terminal.logical_submission_index !== submission.logical_submission_index
        || terminal.camera_revision !== submission.camera_revision
        || terminal.actual_backend !== submission.actual_backend
        || terminal.submitted_at_monotonic_ms !== submission.submitted_at_monotonic_ms
        || !Number.isFinite(terminal.terminal_at_monotonic_ms)
        || terminal.terminal_at_monotonic_ms < submission.submitted_at_monotonic_ms) {
      throw new Error("auxiliary formal terminal lacks an exact successful submission join");
    }
    terminalTickets.add(terminal.ticket);
  }
}

export function joinCurrentStatsEvidence({ frames, submissions, terminals, sourceCount = null }) {
  validateCurrentStatsTerminalLedger({ submissions, terminals, sourceCount });
  const submissionByTicket = new Map(submissions.map((submission) => [submission.ticket, submission]));
  const terminalByTicket = new Map(terminals.map((terminal) => [terminal.ticket, terminal]));

  return frames.map((frame) => {
    if (frame.raster_execution_plan !== "projected_quads_exact") {
      throw new Error(`frame ${frame.frame_index ?? "unknown"} is not renderer-owned Exact`);
    }
    if (frame.current_stats_submission !== "issued") {
      throw new Error(`frame ${frame.frame_index ?? "unknown"} has no current-stats submission`);
    }
    const identity = frameIdentity(frame);
    validateIdentity(identity, `frame ${frame.frame_index ?? "unknown"} current-stats`);
    const submission = submissionByTicket.get(identity.ticket);
    const terminal = terminalByTicket.get(identity.ticket);
    if (!submission || !terminal) {
      throw new Error(`frame current-stats ticket ${identity.ticket} has no ledger join`);
    }
    assertSameIdentity(identity, submission, `frame current-stats ticket ${identity.ticket}`);
    assertSameIdentity(identity, terminal, `frame current-stats ticket ${identity.ticket}`);
    if (identity.camera_revision !== frame.camera_revision
        || identity.plan !== expectedPlan(frame)) {
      throw new Error(`frame current-stats ticket ${identity.ticket} has stale or mismatched runtime identity`);
    }
    for (const [field, value] of [
      ["visible", terminal.visible],
      ["contributor", terminal.contributor],
      ["drawn", terminal.drawn],
    ]) {
      if (frame[field] != null && frame[field] !== value) {
        throw new Error(`frame current-stats ticket ${identity.ticket} disagrees on ${field}`);
      }
    }
    const exactContributorCompaction = terminal.count_semantics === "indirect_draw_equals_contributor";
    if (frame.exact_contributor_compaction != null
        && frame.exact_contributor_compaction !== exactContributorCompaction) {
      throw new Error(
        `frame current-stats ticket ${identity.ticket} disagrees on contributor compaction`,
      );
    }
    return {
      ...frame,
      count_semantics: "candidate_visible_contributor_issued_v1",
      visible: terminal.visible,
      contributor: terminal.contributor,
      drawn: terminal.drawn,
      exact_contributor_compaction: exactContributorCompaction,
      visible_count_revision: terminal.camera_revision,
      visible_count_pending: false,
      visible_count_source: "renderer_current_stats_terminal",
      visible_count_ticket: terminal.ticket,
      count_statistics_eligible: true,
      gpu_complete_ms: null,
      gpu_preprocess_ms: null,
      gpu_radix_ms: null,
      gpu_order_ms: null,
    };
  });
}

export function validateCurrentStatsEvidence({ frames }) {
  if (!Array.isArray(frames) || frames.length === 0) {
    throw new Error("Exact benchmark has no measured frames");
  }
  for (const frame of frames) {
    if (frame.visible_count_source !== "renderer_current_stats_terminal"
        || frame.count_statistics_eligible !== true
        || frame.visible_count_pending !== false
        || frame.visible_count_ticket !== frame.current_stats_ticket
        || frame.visible_count_revision !== frame.camera_revision
        || frame.visible <= 0) {
      throw new Error(
        `frame ${frame.frame_index ?? "unknown"} lacks a current renderer terminal count join`,
      );
    }
  }
}
