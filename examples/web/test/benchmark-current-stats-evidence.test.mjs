import assert from "node:assert/strict";
import test from "node:test";

import {
  joinCurrentStatsEvidence,
  validateAuxiliaryCurrentStatsFormalLedger,
  validateCurrentStatsAttemptSubmissionJoin,
  validateCurrentStatsEvidence,
  validateCurrentStatsTerminalLedger,
} from "../src/benchmark-current-stats-evidence.mjs";

function auxiliaryFormal(overrides = {}) {
  return {
    run_id: "run-1",
    phase: "warmup",
    logical_submission_index: 1,
    attempt_index: 0,
    ticket: 23,
    camera_revision: 5,
    trace_frame_index: 0,
    actual_backend: "cpu",
    deferred_at_monotonic_ms: 19,
    submitted_at_monotonic_ms: 20,
    ...overrides,
  };
}

function deferredAttempt(overrides = {}) {
  return {
    phase: "warmup",
    logical_submission_index: 1,
    attempt_index: 0,
    camera_revision: 5,
    trace_frame_index: 0,
    observed_at_monotonic_ms: 19,
    ...overrides,
  };
}

function identity(overrides = {}) {
  return {
    ticket: 19,
    plan: "gpu_preproject",
    scene_generation: 3,
    camera_revision: 4,
    viewport_generation: 5,
    contract_generation: 6,
    plan_set_generation: 7,
    order_generation: 8,
    raster_generation: 9,
    encode_attempt: 10,
    presentation_sequence: 11,
    ...overrides,
  };
}

function terminal(overrides = {}) {
  return {
    ...identity(),
    status: "ready",
    count_semantics: "indirect_draw_equals_contributor",
    source_count: 100,
    visible: 81,
    contributor: 64,
    drawn: 64,
    submitted_at_monotonic_ms: 10,
    terminal_at_monotonic_ms: 12,
    ...overrides,
  };
}

function frame(overrides = {}) {
  const id = identity();
  return {
    frame_index: 0,
    raster_execution_plan: "projected_quads_exact",
    order_backend: "gpu",
    projected_execution: "compact",
    camera_revision: 4,
    current_stats_submission: "issued",
    current_stats_ticket: id.ticket,
    current_stats_plan: id.plan,
    current_stats_scene_generation: id.scene_generation,
    current_stats_camera_revision: id.camera_revision,
    current_stats_viewport_generation: id.viewport_generation,
    current_stats_contract_generation: id.contract_generation,
    current_stats_plan_set_generation: id.plan_set_generation,
    current_stats_order_generation: id.order_generation,
    current_stats_raster_generation: id.raster_generation,
    current_stats_encode_attempt: id.encode_attempt,
    current_stats_presentation_sequence: id.presentation_sequence,
    visible: null,
    contributor: null,
    drawn: null,
    exact_contributor_compaction: null,
    visible_count_revision: null,
    visible_count_pending: true,
    submitted_measurement_ticket: null,
    ...overrides,
  };
}

test("collector joins pending Exact counts only through the renderer current-stats terminal", () => {
  const joined = joinCurrentStatsEvidence({
    frames: [frame()],
    submissions: [{ ...identity(), phase: "measured", submitted_at_monotonic_ms: 10 }],
    terminals: [terminal()],
    sourceCount: 100,
  });

  assert.equal(joined[0].visible, 81);
  assert.equal(joined[0].contributor, 64);
  assert.equal(joined[0].drawn, 64);
  assert.equal(joined[0].visible_count_source, "renderer_current_stats_terminal");
  assert.equal(joined[0].visible_count_ticket, 19);
  assert.equal(joined[0].visible_count_pending, false);
  assert.equal(joined[0].submitted_measurement_ticket, null);
  assert.doesNotThrow(() => validateCurrentStatsEvidence({ frames: joined }));
});

test("trace-driven frames join each advancing renderer camera revision", () => {
  const next = identity({
    ticket: 20,
    camera_revision: 5,
    order_generation: 10,
    encode_attempt: 11,
    presentation_sequence: 12,
  });
  const nextFrame = frame({
    frame_index: 1,
    camera_revision: next.camera_revision,
    current_stats_ticket: next.ticket,
    current_stats_plan: next.plan,
    current_stats_scene_generation: next.scene_generation,
    current_stats_camera_revision: next.camera_revision,
    current_stats_viewport_generation: next.viewport_generation,
    current_stats_contract_generation: next.contract_generation,
    current_stats_plan_set_generation: next.plan_set_generation,
    current_stats_order_generation: next.order_generation,
    current_stats_raster_generation: next.raster_generation,
    current_stats_encode_attempt: next.encode_attempt,
    current_stats_presentation_sequence: next.presentation_sequence,
  });
  const joined = joinCurrentStatsEvidence({
    frames: [frame(), nextFrame],
    submissions: [identity(), next],
    terminals: [terminal(), terminal({ ...next })],
    sourceCount: 100,
  });

  assert.deepEqual(joined.map((record) => record.camera_revision), [4, 5]);
  assert.deepEqual(joined.map((record) => record.visible_count_revision), [4, 5]);
  assert.doesNotThrow(() => validateCurrentStatsEvidence({ frames: joined }));
});

test("current-stats evidence fails closed for missing, stale, mismatched, unsampled, and failed terminals", () => {
  const submission = { ...identity(), phase: "measured", submitted_at_monotonic_ms: 10 };
  assert.throws(
    () => validateCurrentStatsTerminalLedger({ submissions: [submission], terminals: [] }),
    /has no terminal/,
  );
  assert.throws(
    () => joinCurrentStatsEvidence({
      frames: [frame({ current_stats_camera_revision: 3 })],
      submissions: [submission],
      terminals: [terminal()],
    }),
    /identity mismatch/,
  );
  assert.throws(
    () => validateCurrentStatsTerminalLedger({
      submissions: [submission],
      terminals: [terminal({ plan: "gpu_post_sort" })],
    }),
    /identity mismatch/,
  );
  assert.throws(
    () => validateCurrentStatsTerminalLedger({
      submissions: [submission],
      terminals: [terminal({ status: "unsampled" })],
    }),
    /terminated with unsampled/,
  );
  assert.throws(
    () => validateCurrentStatsTerminalLedger({
      submissions: [submission],
      terminals: [terminal({ status: "map_failure" })],
    }),
    /terminated with map_failure/,
  );
});

test("provisional Exact counts remain unavailable and cannot be labeled eligible", () => {
  assert.equal(frame().visible, null);
  assert.equal(frame().drawn, null);
  assert.throws(
    () => validateCurrentStatsEvidence({ frames: [frame()] }),
    /lacks a current renderer terminal count join/,
  );
});

test("auxiliary formal control ledger joins without becoming a logical sample", () => {
  const submission = auxiliaryFormal();
  const terminal = {
    ...submission,
    outcome: "success",
    reason: null,
    terminal_at_monotonic_ms: 22,
  };
  assert.doesNotThrow(() => validateAuxiliaryCurrentStatsFormalLedger({
    runId: "run-1",
    expectedLogicalFrameCount: 100,
    deferredPresentationCount: 2,
    deferredPresentations: [deferredAttempt()],
    submissions: [submission],
    terminals: [terminal],
  }));
});

test("auxiliary formal ledger rejects missing, failed, stale, and throughput terminals", () => {
  const submission = auxiliaryFormal();
  const terminal = {
    ...submission,
    outcome: "success",
    reason: null,
    terminal_at_monotonic_ms: 22,
  };
  const validate = (overrides = {}) => validateAuxiliaryCurrentStatsFormalLedger({
    runId: "run-1",
    expectedLogicalFrameCount: 100,
    deferredPresentationCount: 1,
    deferredPresentations: [deferredAttempt()],
    submissions: [submission],
    terminals: [terminal],
    ...overrides,
  });
  assert.throws(() => validate({ terminals: [] }), /disagrees/);
  assert.throws(
    () => validate({ terminals: [{ ...terminal, outcome: "failure", reason: "map" }] }),
    /exact successful submission join/,
  );
  assert.throws(
    () => validate({ terminals: [{ ...terminal, camera_revision: 6 }] }),
    /exact successful submission join/,
  );
  assert.throws(
    () => validate({ submissions: [{ ...submission, attempt_index: 1 }] }),
    /invalid or duplicate identity/,
  );
  assert.throws(
    () => validate({ submissions: [{ ...submission, submitted_at_monotonic_ms: 18 }] }),
    /invalid or duplicate identity/,
  );
  assert.throws(
    () => validate({ terminalQueueThroughput: true }),
    /throughput emitted auxiliary control formal work/,
  );
});

test("issued presentation attempts join their exact renderer submissions", () => {
  const attempt = {
    phase: "measured",
    logical_submission_index: 4,
    attempt_index: 2,
    ticket: 31,
    camera_revision: 9,
    trace_frame_index: 1,
    submitted_at_monotonic_ms: 42,
  };
  const submission = {
    phase: "measured",
    ticket: 31,
    camera_revision: 9,
    submitted_at_monotonic_ms: 42,
  };
  assert.doesNotThrow(() => validateCurrentStatsAttemptSubmissionJoin({
    issuedPresentations: [attempt],
    submissions: [submission],
  }));
  assert.throws(
    () => validateCurrentStatsAttemptSubmissionJoin({
      issuedPresentations: [attempt],
      submissions: [{ ...submission, camera_revision: 10 }],
    }),
    /lacks its exact renderer submission join/,
  );
  assert.throws(
    () => validateCurrentStatsAttemptSubmissionJoin({
      issuedPresentations: [attempt],
      submissions: [],
    }),
    /disagree/,
  );
});
