import assert from "node:assert/strict";
import test from "node:test";

import {
  validateGpuProducerFrameEvidence,
  validateGpuProducerMeasuredSubmissions,
  validateGpuProducerTerminalLedger,
} from "../src/benchmark-gpu-producer-evidence.mjs";

const ticket = 2 ** 51;

function producerFrame(overrides = {}) {
  return {
    gpu_order_producer: "preproject",
    gpu_producer_measurement_submission: "issued",
    gpu_producer_measurement_ticket: ticket,
    gpu_producer_measurement_producer: "preproject",
    gpu_producer_measurement_unsampled_reason: null,
    camera_revision: 7,
    order_backend: "gpu",
    raster_execution_plan: "projected_quads_exact",
    projected_policy: "compact",
    projected_execution: "compact",
    projected_adaptive_state: "disabled",
    // Exact D=C is proved by the terminal receipt, not the submission frame.
    exact_contributor_compaction: null,
    sort_refreshed: true,
    ...overrides,
  };
}

test("unrequested producer evidence preserves the PostSort product default", () => {
  validateGpuProducerFrameEvidence({
    requestedProducer: null,
    frames: [producerFrame({
      gpu_order_producer: "post-sort",
      gpu_producer_measurement_submission: "not_requested",
      gpu_producer_measurement_ticket: null,
      gpu_producer_measurement_producer: null,
    })],
  });
  validateGpuProducerFrameEvidence({
    requestedProducer: null,
    frames: [producerFrame({
      order_backend: "cpu",
      gpu_order_producer: null,
      gpu_producer_measurement_submission: "not_requested",
      gpu_producer_measurement_ticket: null,
      gpu_producer_measurement_producer: null,
    })],
  });
  assert.throws(
    () => validateGpuProducerFrameEvidence({
      requestedProducer: null,
      frames: [producerFrame({
        gpu_producer_measurement_submission: "not_requested",
        gpu_producer_measurement_ticket: null,
        gpu_producer_measurement_producer: null,
      })],
    }),
    /changed or misreported/,
  );
});

test("requested producer requires exact forced-GPU context and an issued ticket", () => {
  validateGpuProducerFrameEvidence({
    requestedProducer: "preproject",
    frames: [producerFrame()],
  });
  assert.throws(
    () => validateGpuProducerFrameEvidence({
      requestedProducer: "preproject",
      frames: [producerFrame({ order_backend: "cpu" })],
    }),
    /outside the strict/,
  );
  assert.throws(
    () => validateGpuProducerFrameEvidence({
      requestedProducer: "preproject",
      frames: [producerFrame({
        gpu_producer_measurement_submission: "unsampled",
        gpu_producer_measurement_ticket: null,
        gpu_producer_measurement_unsampled_reason: "ring_busy",
      })],
    }),
    /lacks an issued/,
  );
});

test("retained frames join exactly to measured producer submissions", () => {
  const frame = producerFrame();
  const submission = {
    ticket,
    camera_revision: 7,
    producer: "preproject",
    phase: "measured",
  };
  validateGpuProducerMeasuredSubmissions({
    frames: [frame],
    submissions: [
      { ...submission, ticket: ticket + 1, phase: "warmup" },
      submission,
    ],
  });
  assert.throws(
    () => validateGpuProducerMeasuredSubmissions({
      frames: [frame],
      submissions: [{ ...submission, camera_revision: 8 }],
    }),
    /lacks a matching measured submission/,
  );
  assert.throws(
    () => validateGpuProducerMeasuredSubmissions({
      frames: [frame],
      submissions: [submission, submission],
    }),
    /duplicated/,
  );
});

test("producer terminal ledger accepts one exact-current terminal", () => {
  const identity = {
    ticket,
    camera_revision: 7,
    producer: "post-sort",
  };
  assert.deepEqual(validateGpuProducerTerminalLedger({
    requestedProducer: "post-sort",
    sourceCount: 100,
    submissions: [identity],
    measurements: [{
      ...identity,
      order_generation: 4,
      projection_generation: 9,
      queue_complete_ms: 3.5,
      count_semantics: "source_contributor_issued_v1",
      source: 100,
      contributor: 72,
      drawn: 72,
      order_refreshed: true,
      draw_scope: "exact_current_contributors",
      exact_current_contributor_draw: true,
      stale_order: false,
    }],
    failures: [],
  }), {
    issued_count: 1,
    success_count: 1,
    failure_count: 0,
  });
});

test("producer ledger rejects stale, incomplete, and duplicate terminals", () => {
  const identity = { ticket, camera_revision: 7, producer: "post-sort" };
  const success = {
    ...identity,
    order_generation: 4,
    projection_generation: 9,
    queue_complete_ms: 3.5,
    count_semantics: "source_contributor_issued_v1",
    source: 100,
    contributor: 72,
    drawn: 72,
    order_refreshed: true,
    draw_scope: "exact_current_contributors",
    exact_current_contributor_draw: true,
    stale_order: false,
  };
  assert.throws(
    () => validateGpuProducerTerminalLedger({
      requestedProducer: "post-sort",
      sourceCount: 100,
      submissions: [],
      measurements: [],
      failures: [],
    }),
    /issued no tickets/,
  );
  assert.throws(
    () => validateGpuProducerTerminalLedger({
      requestedProducer: "post-sort",
      sourceCount: 100,
      submissions: [identity],
      measurements: [{ ...success, stale_order: true }],
      failures: [],
    }),
    /exact-current/,
  );
  assert.throws(
    () => validateGpuProducerTerminalLedger({
      requestedProducer: "post-sort",
      sourceCount: 100,
      submissions: [identity],
      measurements: [],
      failures: [],
    }),
    /missing ticket/,
  );
  assert.throws(
    () => validateGpuProducerTerminalLedger({
      requestedProducer: "post-sort",
      sourceCount: 100,
      submissions: [identity],
      measurements: [success],
      failures: [{
        ...identity,
        order_generation: 4,
        projection_generation: 9,
        reason: "readback_map",
      }],
    }),
    /more than one terminal/,
  );
});
