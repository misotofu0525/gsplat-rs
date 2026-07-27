import assert from "node:assert/strict";
import test from "node:test";

import {
  createTerminalQueueThroughputWindow,
  currentStatsEvidenceWindowIdentity,
  currentStatsEvidenceWindowManifest,
  validateBenchmarkWindowManifest,
} from "../src/benchmark-window-mode.mjs";

const CONFIGURATION_SHA256 = "a".repeat(64);

function controlIdentity() {
  return currentStatsEvidenceWindowIdentity({
    runId: "control-a",
    configurationSha256: CONFIGURATION_SHA256,
  });
}

function draw(overrides = {}) {
  return {
    currentStatsSubmission: "not_requested",
    currentStatsTicket: null,
    submittedAtMonotonicMs: 1,
    projectedAdaptiveState: "candidate_learning",
    projectedExecution: "candidate",
    exactAdaptiveState: "cpu_learning",
    actualPlan: "cpu_post_sort",
    ...overrides,
  };
}

test("current-stats control window is explicitly excluded from performance evidence", () => {
  const window = currentStatsEvidenceWindowManifest({
    runId: "control-a",
    configurationSha256: CONFIGURATION_SHA256,
  });
  assert.equal(window.performance_evidence, false);
  assert.equal(window.evidence_role, "renderer_exact_current_stats_control");
  assert.doesNotThrow(() => validateBenchmarkWindowManifest({
    window,
    currentStatsSubmissionCount: 100,
    expectedLogicalFrameCount: 100,
    expectedConfigurationSha256: CONFIGURATION_SHA256,
  }));
  assert.throws(
    () => validateBenchmarkWindowManifest({
      window: { ...window, performance_evidence: true },
      currentStatsSubmissionCount: 100,
      expectedLogicalFrameCount: 100,
      expectedConfigurationSha256: CONFIGURATION_SHA256,
    }),
    /cannot be admitted as cross-implementation performance evidence/,
  );
});

test("N>1 drains warmup before input and measures input through final terminal", () => {
  const window = createTerminalQueueThroughputWindow({
    warmupFrames: 2,
    measuredFrames: 3,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
    directQueueCompletion: true,
  });

  assert.equal(window.noteDraw(draw({ submittedAtMonotonicMs: 1 })), "warmup");
  assert.equal(window.action, "request_warmup_receipt");
  assert.throws(() => window.noteDraw(draw({ submittedAtMonotonicMs: 2 })), /forbids draw/);
  window.beginWarmupReceipt({ requestedAtMonotonicMs: 2 });
  assert.equal(window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 11,
    submittedAtMonotonicMs: 3,
  })), "warmup");
  assert.equal(window.action, "poll_warmup_receipt");
  assert.throws(
    () => window.beginMeasuredInput({ acceptedAtMonotonicMs: 4 }),
    /unexpected first measured input/,
  );
  assert.throws(() => window.noteDraw(draw({ submittedAtMonotonicMs: 4 })), /forbids draw/);
  window.recordWarmupReceipt({
    ticket: 11,
    status: "ready",
    plan: "cpu_post_sort",
    terminalAtMonotonicMs: 5,
  });
  assert.equal(window.action, "accept_measured_input");
  window.beginMeasuredInput({ acceptedAtMonotonicMs: 10 });

  assert.equal(window.noteDraw(draw({ submittedAtMonotonicMs: 14 })), "measured");
  window.noteDraw(draw({
    submittedAtMonotonicMs: 15,
    projectedAdaptiveState: "compact_probe",
    projectedExecution: "compact",
    exactAdaptiveState: "gpu_probe",
    actualPlan: "gpu_preproject",
  }));
  assert.equal(window.action, "request_final_receipt");
  window.beginFinalReceipt({ requestedAtMonotonicMs: 16 });
  window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 27,
    submittedAtMonotonicMs: 17,
    projectedAdaptiveState: "compact_stable",
    projectedExecution: "compact",
    exactAdaptiveState: "gpu_stable",
    actualPlan: "gpu_preproject",
  }));
  assert.equal(window.action, "poll_final_receipt");
  assert.throws(() => window.noteDraw(draw({ submittedAtMonotonicMs: 18 })), /forbids draw/);
  window.recordTerminalReceipt({
    ticket: 27,
    status: "ready",
    plan: "gpu_preproject",
    terminalAtMonotonicMs: 20,
  });
  const evidence = window.evidence();

  assert.equal(evidence.first_measured_input_monotonic_ms, 10);
  assert.equal(evidence.first_measured_submit_monotonic_ms, 14);
  assert.equal(evidence.last_measured_submit_monotonic_ms, 17);
  assert.equal(evidence.input_to_first_submit_ms, 4);
  assert.equal(evidence.submit_span_ms, 3);
  assert.equal(evidence.terminal_tail_ms, 3);
  assert.equal(evidence.terminal_window_ms, 10);
  assert.equal(evidence.warmup_terminal_receipt.ticket, 11);
  assert.equal(evidence.terminal_receipt.ticket, 27);
  assert.notEqual(
    evidence.warmup_boundary_current_stats_ticket,
    evidence.final_measured_current_stats_ticket,
  );
  assert.equal(
    evidence.draw_count_at_warmup_drain_start,
    evidence.draw_count_at_warmup_drain_completion,
  );
  assert.equal(evidence.draw_count_at_final_drain_start, evidence.draw_count_at_completion);
  assert.equal(
    evidence.terminal_receipt_overhead.warmup_terminal_boundary,
    "same_submission_result_ready_before_first_measured_input",
  );
  assert.equal(evidence.terminal_receipt_overhead.residual_warmup_queue_tail, "excluded");
  assert.equal(evidence.completion_primitive, "gpu_queue_on_submitted_work_done");
  assert.equal(
    evidence.queue_completion_timestamp_source,
    "wgpu_queue_callback_performance_now",
  );
  assert.equal(evidence.terminal_receipt_overhead.queue_completion_callback, true);
  assert.equal(
    evidence.terminal_receipt_overhead.fairness_assessment,
    "same_queue_completion_primitive",
  );
  assert.doesNotThrow(() => validateBenchmarkWindowManifest({
    window: evidence,
    currentStatsSubmissionCount: 2,
    expectedLogicalFrameCount: 3,
    expectedWarmupFrameCount: 2,
    expectedConfigurationSha256: CONFIGURATION_SHA256,
  }));
  assert.throws(
    () => validateBenchmarkWindowManifest({
      window: {
        ...evidence,
        queue_completion_timestamp_source: "raf_current_stats_poll_performance_now",
      },
      currentStatsSubmissionCount: 2,
      expectedLogicalFrameCount: 3,
      expectedWarmupFrameCount: 2,
      expectedConfigurationSha256: CONFIGURATION_SHA256,
    }),
    /lacks continuous submits/,
  );
  for (const invalidCount of [0, 1, 3]) {
    assert.throws(
      () => validateBenchmarkWindowManifest({
        window: evidence,
        currentStatsSubmissionCount: invalidCount,
        expectedLogicalFrameCount: 3,
        expectedWarmupFrameCount: 2,
        expectedConfigurationSha256: CONFIGURATION_SHA256,
      }),
      /requires its untimed warmup boundary and final receipt/,
    );
  }
});

test("N=1 accepts input before render and keeps warmup/final tickets separate", () => {
  const window = createTerminalQueueThroughputWindow({
    warmupFrames: 1,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  assert.equal(window.action, "request_warmup_receipt");
  window.beginWarmupReceipt({ requestedAtMonotonicMs: 1 });
  window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 3,
    submittedAtMonotonicMs: 2,
  }));
  window.recordWarmupReceipt({
    ticket: 3,
    status: "ready",
    plan: "cpu_post_sort",
    terminalAtMonotonicMs: 4,
  });
  window.beginMeasuredInput({ acceptedAtMonotonicMs: 10 });
  assert.equal(window.action, "request_final_receipt");
  window.beginFinalReceipt({ requestedAtMonotonicMs: 11 });
  window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 4,
    submittedAtMonotonicMs: 14,
  }));
  window.recordTerminalReceipt({
    ticket: 4,
    status: "ready",
    plan: "cpu_post_sort",
    terminalAtMonotonicMs: 18,
  });
  const evidence = window.evidence();
  assert.equal(evidence.measured_submit_count, 1);
  assert.equal(evidence.submit_span_ms, 0);
  assert.equal(evidence.input_to_first_submit_ms, 4);
  assert.equal(evidence.terminal_window_ms, 8);
  assert.deepEqual(
    [evidence.warmup_terminal_receipt.ticket, evidence.terminal_receipt.ticket],
    [3, 4],
  );
});

test("warmup backlog and boundary identity failures remain fail closed", () => {
  const window = createTerminalQueueThroughputWindow({
    warmupFrames: 1,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  window.beginWarmupReceipt({ requestedAtMonotonicMs: 1 });
  window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 5,
    submittedAtMonotonicMs: 2,
  }));
  assert.throws(
    () => window.recordWarmupReceipt({
      ticket: 6,
      status: "ready",
      plan: "cpu_post_sort",
      terminalAtMonotonicMs: 4,
    }),
    /warmup receipt identity drift/,
  );
  window.recordWarmupReceipt({
    ticket: 5,
    status: "ready",
    plan: "cpu_post_sort",
    terminalAtMonotonicMs: 5,
  });
  assert.throws(
    () => window.beginMeasuredInput({ acceptedAtMonotonicMs: 4 }),
    /precedes the warmup queue terminal/,
  );
  window.beginMeasuredInput({ acceptedAtMonotonicMs: 6 });
  window.beginFinalReceipt({ requestedAtMonotonicMs: 7 });
  assert.throws(
    () => window.noteDraw(draw({
      currentStatsSubmission: "issued",
      currentStatsTicket: 5,
      submittedAtMonotonicMs: 8,
    })),
    /reused one current-stats ticket/,
  );
});

test("non-final observer work, missing issue, duplicate terminal, and control drift reject", () => {
  assert.throws(
    () => createTerminalQueueThroughputWindow({
      warmupFrames: 0,
      measuredFrames: 1,
      configurationSha256: "b".repeat(64),
      controlArtifactIdentity: controlIdentity(),
    }),
    /same-configuration current-stats control/,
  );

  const premature = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 2,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  premature.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  assert.throws(
    () => premature.noteDraw(draw({
      currentStatsSubmission: "issued",
      currentStatsTicket: 1,
    })),
    /warmup or non-final measured frame requested current stats/,
  );

  const missing = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  missing.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  missing.beginFinalReceipt({ requestedAtMonotonicMs: 0 });
  assert.throws(
    () => missing.noteDraw(draw({ submittedAtMonotonicMs: 1 })),
    /did not issue its terminal current-stats receipt/,
  );
  missing.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 2,
    submittedAtMonotonicMs: 1,
  }));
  missing.recordTerminalReceipt({
    ticket: 2,
    status: "ready",
    plan: "cpu_post_sort",
    terminalAtMonotonicMs: 2,
  });
  assert.throws(
    () => missing.recordTerminalReceipt({
      ticket: 2,
      status: "ready",
      plan: "cpu_post_sort",
      terminalAtMonotonicMs: 3,
    }),
    /unexpected terminal receipt poll/,
  );
});

test("projected-disabled Exact whole-plan state is valid", () => {
  const window = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  window.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  window.beginFinalReceipt({ requestedAtMonotonicMs: 0 });
  window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 3,
    projectedAdaptiveState: "disabled",
  }));
  window.recordTerminalReceipt({
    ticket: 3,
    status: "ready",
    plan: "cpu_post_sort",
    terminalAtMonotonicMs: 3,
  });
  assert.equal(window.evidence().exact_adaptive_measured[0].projected_state, "disabled");
});

test("fixed GPU preproject Compact cell admits only the frozen execution tuple", () => {
  const window = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
    executionCell: "fixed_gpu_preproject_compact",
  });
  window.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  window.beginFinalReceipt({ requestedAtMonotonicMs: 0 });
  window.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 31,
    projectedAdaptiveState: "disabled",
    projectedExecution: "compact",
    exactAdaptiveState: "disabled",
    actualPlan: "gpu_preproject",
  }));
  window.recordTerminalReceipt({
    ticket: 31,
    status: "ready",
    plan: "gpu_preproject",
    terminalAtMonotonicMs: 2,
  });
  const evidence = window.evidence();
  assert.equal(evidence.execution_cell, "fixed_gpu_preproject_compact");
  assert.equal(evidence.exact_adaptive_measured[0].plan, "gpu_preproject");

  const drift = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
    executionCell: "fixed_gpu_preproject_compact",
  });
  drift.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  drift.beginFinalReceipt({ requestedAtMonotonicMs: 0 });
  assert.throws(() => drift.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 32,
    exactAdaptiveState: "disabled",
    actualPlan: "gpu_post_sort",
  })), /execution drifted/);
});

test("inactive Exact state, receipt identity drift, and map failure reject", () => {
  const inactive = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  inactive.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  inactive.beginFinalReceipt({ requestedAtMonotonicMs: 0 });
  assert.throws(() => inactive.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 4,
    exactAdaptiveState: "disabled",
  })), /active Exact adaptive state\/plan/);

  const mismatch = createTerminalQueueThroughputWindow({
    warmupFrames: 0,
    measuredFrames: 1,
    configurationSha256: CONFIGURATION_SHA256,
    controlArtifactIdentity: controlIdentity(),
  });
  mismatch.beginMeasuredInput({ acceptedAtMonotonicMs: 0 });
  mismatch.beginFinalReceipt({ requestedAtMonotonicMs: 0 });
  mismatch.noteDraw(draw({
    currentStatsSubmission: "issued",
    currentStatsTicket: 7,
  }));
  assert.throws(
    () => mismatch.recordTerminalReceipt({
      ticket: 8,
      status: "ready",
      plan: "cpu_post_sort",
      terminalAtMonotonicMs: 2,
    }),
    /identity drift expected=7 observed=8/,
  );
  assert.throws(
    () => mismatch.recordTerminalReceipt({
      ticket: 7,
      status: "map_failure",
      plan: "cpu_post_sort",
      terminalAtMonotonicMs: 2,
    }),
    /terminated with map_failure/,
  );
});
