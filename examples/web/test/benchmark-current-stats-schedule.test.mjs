import assert from "node:assert/strict";
import test from "node:test";

import {
  createCurrentStatsSchedule,
  validateCurrentStatsScheduleEvidence,
} from "../src/benchmark-current-stats-schedule.mjs";

function issue(schedule, ticket, nowMs, priming = false) {
  schedule.noteDraw(nowMs);
  schedule.beginRequest({ nowMs, priming, traceStep: { ticket } });
  return schedule.recordIssued({
    ticket,
    stats: { ticket },
    submittedAtMonotonicMs: nowMs + 0.25,
  });
}

function terminate(schedule, ticket, nowMs, status = "ready") {
  const pending = schedule.pendingForTerminal({ ticket, status });
  schedule.recordTerminal({
    pending,
    joined: { ticket },
    terminalAtMonotonicMs: nowMs,
  });
  return schedule.takeReadyInSubmissionOrder();
}

test("sustained current-stats submits consecutive frames without awaiting terminals", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 1,
    measuredFrames: 3,
  });

  issue(schedule, 11, 1);
  assert.equal(schedule.action, "draw");
  issue(schedule, 12, 2);
  assert.equal(schedule.action, "draw");
  issue(schedule, 13, 3);
  assert.equal(schedule.action, "draw");
  issue(schedule, 14, 4);

  assert.equal(schedule.submittedCount, 4);
  assert.equal(schedule.pendingCount, 4);
  assert.equal(schedule.action, "drain");
});

test("sustained schedule retains the complete 20 warmup plus 80 measured ledger", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 20,
    measuredFrames: 80,
  });
  const pendingTickets = [];
  const phases = [];
  for (let index = 0; index < 100; index += 1) {
    if (pendingTickets.length === 4) {
      terminate(schedule, pendingTickets.shift(), 1_000 + index);
    }
    const ticket = 100 + index;
    phases.push(issue(schedule, ticket, index + 1).phase);
    pendingTickets.push(ticket);
  }
  while (pendingTickets.length > 0) {
    terminate(schedule, pendingTickets.shift(), 2_000 + pendingTickets.length);
  }

  const evidence = schedule.evidence();
  assert.deepEqual(phases.slice(0, 20), Array(20).fill("warmup"));
  assert.deepEqual(phases.slice(20), Array(80).fill("measured"));
  assert.equal(evidence.submitted_logical_count, 100);
  assert.equal(evidence.terminal_logical_count, 100);
  assert.equal(evidence.peak_pending_count, 4);
});

test("out-of-order terminals recycle samples in original submission order", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 0,
    measuredFrames: 3,
  });
  issue(schedule, 21, 1);
  issue(schedule, 22, 2);
  issue(schedule, 23, 3);

  assert.deepEqual(terminate(schedule, 22, 4), []);
  assert.deepEqual(
    terminate(schedule, 21, 5).map((entry) => entry.ticket),
    [21, 22],
  );
  assert.deepEqual(
    terminate(schedule, 23, 6).map((entry) => entry.ticket),
    [23],
  );
  assert.equal(schedule.complete, true);
  assert.doesNotThrow(() => schedule.evidence());
});

test("busy ring, missing issue, unknown, duplicate, and failed terminals fail closed", () => {
  const busy = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 0,
    measuredFrames: 3,
    pendingCapacity: 2,
  });
  issue(busy, 31, 1);
  issue(busy, 32, 2);
  assert.equal(busy.action, "fail_capacity");
  assert.throws(() => busy.noteDraw(3), /ring remained busy/);

  const failures = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 0,
    measuredFrames: 2,
  });
  assert.throws(
    () => failures.recordIssued({ ticket: 41, stats: {}, submittedAtMonotonicMs: 1 }),
    /without a request/,
  );
  issue(failures, 41, 1);
  assert.throws(
    () => failures.pendingForTerminal({ ticket: 99, status: "ready" }),
    /unknown renderer current-stats terminal/,
  );
  assert.throws(
    () => failures.pendingForTerminal({ ticket: 41, status: "map_failure" }),
    /terminated with map_failure/,
  );
  terminate(failures, 41, 2);
  assert.throws(
    () => failures.pendingForTerminal({ ticket: 41, status: "ready" }),
    /duplicate renderer current-stats terminal/,
  );
});

test("final drain forbids new draws and times out without retry", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 0,
    measuredFrames: 2,
    finalDrainTimeoutMs: 10,
  });
  issue(schedule, 51, 1);
  issue(schedule, 52, 2);

  assert.equal(schedule.action, "drain");
  assert.throws(() => schedule.noteDraw(3), /forbids draw while drain/);
  assert.doesNotThrow(() => schedule.noteEmptyPoll(11.9));
  assert.throws(() => schedule.noteEmptyPoll(12.25), /final drain timed out/);

  terminate(schedule, 51, 13);
  terminate(schedule, 52, 14);
  const evidence = schedule.evidence();
  assert.equal(
    evidence.draw_count_at_final_drain_start,
    evidence.draw_count_at_completion,
  );
});

test("isolated protocol preserves one-ticket terminal progression", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "isolated_terminal",
    warmupFrames: 0,
    measuredFrames: 2,
  });
  issue(schedule, 61, 1);
  assert.equal(schedule.action, "wait_terminal");
  assert.throws(() => schedule.noteDraw(2), /wait_terminal/);
  terminate(schedule, 61, 2);
  assert.equal(schedule.action, "draw");
  issue(schedule, 62, 3);
  assert.equal(schedule.action, "drain");
  terminate(schedule, 62, 4);

  const evidence = schedule.evidence();
  assert.equal(evidence.peak_pending_count, 1);
  assert.equal(evidence.submission_gate, "previous_terminal");
  assert.equal(evidence.frame_wall_source, "isolated_terminal_progression");
});

test("isolated control retains one logical trace step across deferred observer presentations", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "isolated_terminal",
    warmupFrames: 0,
    measuredFrames: 1,
  });

  schedule.noteDraw(1);
  const traceStep = { frame: 0 };
  schedule.beginRequest({ nowMs: 1, traceStep });
  schedule.recordDeferredPresentation({
    submission: "not_requested",
    ticket: null,
    cameraRevision: 7,
    traceStep,
    observedAtMonotonicMs: 1.25,
  });
  assert.equal(schedule.nextSubmissionIndex, 0);
  assert.equal(schedule.requestOutstanding, true);
  assert.equal(schedule.action, "draw");

  schedule.noteDraw(2);
  schedule.recordDeferredPresentation({
    submission: "not_requested",
    ticket: null,
    cameraRevision: 7,
    traceStep,
    observedAtMonotonicMs: 2.25,
  });
  assert.equal(schedule.nextSubmissionIndex, 0);

  schedule.noteDraw(3);
  schedule.recordIssued({
    ticket: 63,
    stats: { ticket: 63, cameraRevision: 7 },
    submittedAtMonotonicMs: 3.25,
  });
  terminate(schedule, 63, 4);

  const evidence = schedule.evidence();
  assert.equal(evidence.submitted_logical_count, 1);
  assert.equal(evidence.terminal_logical_count, 1);
  assert.equal(evidence.deferred_presentation_count, 2);
  assert.equal(evidence.peak_deferred_presentations_before_issue, 2);
  assert.equal(evidence.draw_count_at_completion, 3);
});

test("deferred observer presentations fail closed on invalid shape or finite timeout", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "isolated_terminal",
    warmupFrames: 0,
    measuredFrames: 1,
    finalDrainTimeoutMs: 10,
  });
  schedule.noteDraw(1);
  schedule.beginRequest({ nowMs: 1, traceStep: null });

  assert.throws(
    () => schedule.recordDeferredPresentation({
      submission: "issued",
      ticket: 1,
      cameraRevision: 1,
      traceStep: null,
      observedAtMonotonicMs: 2,
    }),
    /exposed a ticket or non-empty submission/,
  );
  assert.throws(
    () => schedule.recordDeferredPresentation({
      submission: "not_requested",
      ticket: null,
      cameraRevision: 1,
      traceStep: null,
      observedAtMonotonicMs: 11,
    }),
    /did not issue within 10ms/,
  );
});

test("deferred observer presentations preserve camera and logical member identity", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "isolated_terminal",
    warmupFrames: 0,
    measuredFrames: 1,
  });
  const traceStep = { frame: 0 };
  schedule.noteDraw(1);
  schedule.beginRequest({ nowMs: 1, traceStep });
  schedule.recordDeferredPresentation({
    submission: "not_requested",
    ticket: null,
    cameraRevision: 9,
    traceStep,
    observedAtMonotonicMs: 2,
  });
  assert.throws(
    () => schedule.recordDeferredPresentation({
      submission: "not_requested",
      ticket: null,
      cameraRevision: 10,
      traceStep,
      observedAtMonotonicMs: 3,
    }),
    /changed its camera revision/,
  );
  assert.throws(
    () => schedule.recordIssued({
      ticket: 64,
      stats: { cameraRevision: 10 },
      submittedAtMonotonicMs: 4,
    }),
    /changed its deferred camera revision/,
  );
});

test("schedule admission rejects sustained or isolated labels that overclaim behavior", () => {
  const schedule = createCurrentStatsSchedule({
    protocol: "sustained_window",
    warmupFrames: 0,
    measuredFrames: 1,
  });
  issue(schedule, 71, 1);
  terminate(schedule, 71, 2);
  const evidence = schedule.evidence();

  assert.throws(
    () => validateCurrentStatsScheduleEvidence({
      evidence: { ...evidence, submission_gate: "previous_terminal" },
      protocol: "sustained_window",
      frameWallSource: "request_animation_frame_interval",
      expectedLogicalFrameCount: 1,
    }),
    /labels do not prove sustained_window behavior/,
  );
  assert.throws(
    () => validateCurrentStatsScheduleEvidence({
      evidence,
      protocol: "sustained_window",
      frameWallSource: "isolated_terminal_progression",
      expectedLogicalFrameCount: 1,
    }),
    /labels do not prove sustained_window behavior/,
  );
  assert.throws(
    () => validateCurrentStatsScheduleEvidence({
      evidence: {
        ...evidence,
        draw_count_at_completion: evidence.draw_count_at_final_drain_start + 1,
      },
      protocol: "sustained_window",
      frameWallSource: "request_animation_frame_interval",
      expectedLogicalFrameCount: 1,
    }),
    /final drain submitted a new draw/,
  );
});
