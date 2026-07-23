import assert from "node:assert/strict";
import test from "node:test";

import {
  validateProjectedFrameEvidence,
  validateProjectedTerminalLedger,
} from "../src/benchmark-projected-evidence.mjs";

const ticket = 2 ** 52;

test("forced projected frame evidence has execution but no ticket", () => {
  validateProjectedFrameEvidence({
    requestedPolicy: "compact",
    frames: [{
      projected_policy: "compact",
      projected_execution: "compact",
      projected_adaptive_state: "disabled",
      projected_measurement_submission: "not_requested",
      projected_measurement_ticket: null,
      projected_measurement_execution: null,
      projected_measurement_unsampled_reason: null,
    }],
  });
  assert.throws(
    () => validateProjectedFrameEvidence({
      requestedPolicy: "candidate",
      frames: [{
        projected_policy: "candidate",
        projected_execution: "candidate",
        projected_adaptive_state: "disabled",
        projected_measurement_submission: "issued",
        projected_measurement_ticket: ticket,
        projected_measurement_execution: "candidate",
        projected_measurement_unsampled_reason: null,
      }],
    }),
    /manufactures telemetry/,
  );
});

test("adaptive projected terminal ledger accepts one exact terminal per high ticket", () => {
  const submission = {
    ticket,
    camera_revision: 11,
    execution: "compact",
    order_backend: "gpu",
  };
  assert.deepEqual(validateProjectedTerminalLedger({
    submissions: [submission],
    measurements: [{
      ...submission,
      projection_generation: 19,
      probe_generation: 3,
      frame_complete_ms: 4.5,
      visible: 100,
      contributor: 76,
      drawn: 76,
    }],
    failures: [],
  }), {
    issued_count: 1,
    success_count: 1,
    failure_count: 0,
  });
});

test("adaptive projected evidence preserves unsampled backpressure without a ticket", () => {
  validateProjectedFrameEvidence({
    requestedPolicy: "adaptive",
    frames: [{
      projected_policy: "adaptive",
      projected_execution: "candidate",
      projected_adaptive_state: "candidate_learning",
      projected_measurement_submission: "unsampled",
      projected_measurement_ticket: null,
      projected_measurement_execution: "candidate",
      projected_measurement_unsampled_reason: "ring_busy",
    }],
  });
  assert.throws(
    () => validateProjectedFrameEvidence({
      requestedPolicy: "adaptive",
      frames: [{
        projected_policy: "adaptive",
        projected_execution: "compact",
        projected_adaptive_state: "compact_probe",
        projected_measurement_submission: "unsampled",
        projected_measurement_ticket: null,
        projected_measurement_execution: "candidate",
        projected_measurement_unsampled_reason: "ring_busy",
      }],
    }),
    /invalid identity/,
  );
});

test("projected terminal ledger rejects contradictory, missing, and unsafe tickets", () => {
  const submission = {
    ticket,
    camera_revision: 11,
    execution: "candidate",
    order_backend: "cpu",
  };
  const success = {
    ...submission,
    projection_generation: 19,
    probe_generation: 3,
    frame_complete_ms: 4.5,
    visible: 100,
    contributor: 76,
    drawn: 100,
  };
  assert.throws(
    () => validateProjectedTerminalLedger({
      submissions: [submission],
      measurements: [success],
      failures: [{
        ...submission,
        projection_generation: 19,
        probe_generation: 3,
        reason: "readback_map",
      }],
    }),
    /more than one terminal/,
  );
  assert.throws(
    () => validateProjectedTerminalLedger({
      submissions: [submission],
      measurements: [],
      failures: [],
    }),
    /missing ticket/,
  );
  assert.throws(
    () => validateProjectedTerminalLedger({
      submissions: [{ ...submission, ticket: Number.MAX_SAFE_INTEGER + 1 }],
      measurements: [],
      failures: [],
    }),
    /JS-safe integer/,
  );
});
