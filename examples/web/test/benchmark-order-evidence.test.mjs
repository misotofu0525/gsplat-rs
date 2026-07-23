import assert from "node:assert/strict";
import test from "node:test";

import {
  joinOrderingEvidence,
  monotonicOrderingWindow,
  validateTerminalTicketLedger,
  validateOrderingEvidence,
} from "../src/benchmark-order-evidence.mjs";

function frame(overrides = {}) {
  return {
    frame_index: 0,
    visible: 3,
    drawn: 3,
    sort_refreshed: true,
    order_backend: "gpu",
    gpu_sort_fallback: false,
    submitted_measurement_ticket: 7,
    camera_revision: 4,
    visible_count_revision: 4,
    visible_count_pending: false,
    visible_count_source: "gpu_order_receipt",
    visible_count_ticket: 7,
    count_statistics_eligible: true,
    ...overrides,
  };
}

function measurement(overrides = {}) {
  return {
    actual_backend: "gpu",
    ticket: 7,
    camera_revision: 4,
    timing_source: "timestamp_query",
    gpu_preprocess_ms: 0.5,
    gpu_radix_ms: 1.25,
    gpu_order_ms: 1.75,
    gpu_complete_ms: 2.5,
    count_semantics: "candidate_visible_contributor_issued_v1",
    visible: 3,
    contributor: 3,
    drawn: 3,
    exact_contributor_compaction: true,
    ...overrides,
  };
}

function failure(overrides = {}) {
  return {
    actual_backend: "gpu",
    ticket: 7,
    camera_revision: 4,
    reason: "readback_map",
    ...overrides,
  };
}

function cpuMeasurement(overrides = {}) {
  return {
    actual_backend: "cpu",
    ticket: 6,
    camera_revision: 3,
    preprocess_ms: 0.75,
    sort_ms: 1.5,
    frame_complete_ms: 4.25,
    count_semantics: "candidate_visible_contributor_issued_v1",
    visible: 3,
    contributor: 3,
    drawn: 3,
    exact_contributor_compaction: true,
    ...overrides,
  };
}

test("monotonic ordering window separates submit span from terminal drain tail", () => {
  const window = monotonicOrderingWindow({
    submissions: [
      { phase: "warmup", ticket: 1, submitted_at_monotonic_ms: 5 },
      { phase: "measured", ticket: 3, submitted_at_monotonic_ms: 10 },
      { phase: "measured", ticket: 5, submitted_at_monotonic_ms: 22 },
    ],
    terminals: [
      { ticket: 1, terminal_at_monotonic_ms: 9 },
      { ticket: 3, terminal_at_monotonic_ms: 28 },
      { ticket: 5, terminal_at_monotonic_ms: 37 },
    ],
  });
  assert.deepEqual(window, {
    monotonic_clock: "performance.now",
    first_measured_submit_monotonic_ms: 10,
    last_measured_submit_monotonic_ms: 22,
    last_measured_terminal_monotonic_ms: 37,
    submit_span_ms: 12,
    terminal_tail_ms: 15,
    terminal_window_ms: 27,
  });
});

test("monotonic ordering window rejects missing, reversed, and wall-clock evidence", () => {
  const submission = { phase: "measured", ticket: 3, submitted_at_monotonic_ms: 10 };
  assert.throws(
    () => monotonicOrderingWindow({ submissions: [submission], terminals: [] }),
    /lacks a monotonic terminal timestamp/,
  );
  assert.throws(
    () => monotonicOrderingWindow({
      submissions: [submission],
      terminals: [{ ticket: 3, terminal_at_monotonic_ms: 9 }],
    }),
    /terminates before it was submitted/,
  );
  assert.throws(
    () => monotonicOrderingWindow({
      submissions: [{ phase: "measured", ticket: 3, submitted_at_monotonic_ms: "2026-07-23T00:00:00Z" }],
      terminals: [{ ticket: 3, terminal_at_monotonic_ms: 12 }],
    }),
    /finite non-negative monotonic timestamp/,
  );
});

test("fixed-camera order reuse has an explicit null monotonic submit window", () => {
  assert.deepEqual(monotonicOrderingWindow({ submissions: [], terminals: [] }), {
    monotonic_clock: "performance.now",
    first_measured_submit_monotonic_ms: null,
    last_measured_submit_monotonic_ms: null,
    last_measured_terminal_monotonic_ms: null,
    submit_span_ms: null,
    terminal_tail_ms: null,
    terminal_window_ms: null,
  });
});

test("CPU evidence rejects an all-zero artifact", () => {
  const frames = joinOrderingEvidence({
    requestedBackend: "cpu",
    frames: [frame({ visible: 0, drawn: 0, order_backend: "cpu" })],
    measurements: [],
  });
  assert.throws(
    () => validateOrderingEvidence({
      requestedBackend: "cpu",
      frames,
      measurements: [],
    }),
    /no frame with a non-zero/,
  );
});

test("collector joins a provisional GPU frame to its own terminal ticket and revision", () => {
  const frames = [frame({
    visible: 91,
    drawn: 91,
    visible_count_revision: 3,
    visible_count_pending: true,
    visible_count_source: undefined,
    visible_count_ticket: undefined,
    gpu_complete_ms: 99,
  })];
  const measurements = [measurement({
    visible: 17,
    contributor: 17,
    drawn: 17,
    gpu_complete_ms: 2.75,
  })];
  const joined = joinOrderingEvidence({ requestedBackend: "gpu", frames, measurements });

  assert.equal(joined[0].visible, 17);
  assert.equal(joined[0].contributor, 17);
  assert.equal(joined[0].drawn, 17);
  assert.equal(joined[0].visible_count_revision, 4);
  assert.equal(joined[0].visible_count_pending, false);
  assert.equal(joined[0].visible_count_source, "gpu_order_receipt");
  assert.equal(joined[0].visible_count_ticket, 7);
  assert.equal(joined[0].count_statistics_eligible, true);
  assert.equal(joined[0].gpu_complete_ms, 2.75);
  assert.doesNotThrow(() => validateOrderingEvidence({
    requestedBackend: "gpu",
    frames: joined,
    measurements,
  }));
});

test("collector joins a CPU frame only to the same terminal ticket and revision", () => {
  const frames = [frame({
    order_backend: "cpu",
    submitted_measurement_ticket: 6,
    camera_revision: 3,
    visible: 91,
    drawn: 91,
    visible_count_revision: 2,
    visible_count_pending: true,
    visible_count_source: undefined,
    visible_count_ticket: undefined,
  })];
  const receipt = cpuMeasurement({
    visible: 17,
    contributor: 12,
    drawn: 12,
  });
  const joined = joinOrderingEvidence({
    requestedBackend: "cpu",
    frames,
    measurements: [],
    cpuMeasurements: [receipt],
  });

  assert.equal(joined[0].visible, 17);
  assert.equal(joined[0].contributor, 12);
  assert.equal(joined[0].drawn, 12);
  assert.equal(joined[0].visible_count_revision, 3);
  assert.equal(joined[0].visible_count_source, "cpu_order_receipt");
  assert.equal(joined[0].visible_count_ticket, 6);
  assert.equal(joined[0].exact_contributor_compaction, true);
  assert.doesNotThrow(() => validateOrderingEvidence({
    requestedBackend: "cpu",
    frames: joined,
    measurements: [],
  }));
});

test("fixed-camera measured frames reuse a terminal warmup GPU count receipt", () => {
  const frames = joinOrderingEvidence({
    requestedBackend: "gpu",
    frames: [frame({
      sort_refreshed: false,
      submitted_measurement_ticket: null,
      visible_count_ticket: null,
      visible_count_source: undefined,
    })],
    measurements: [measurement()],
  });
  assert.equal(frames[0].visible_count_source, "cached_gpu_order_receipt");
  assert.equal(frames[0].visible_count_ticket, 7);
  assert.equal(frames[0].count_statistics_eligible, true);
  assert.doesNotThrow(() => validateOrderingEvidence({
    requestedBackend: "gpu",
    frames,
    measurements: [measurement()],
    fixedCameraReuse: true,
  }));
  assert.throws(() => validateOrderingEvidence({
    requestedBackend: "gpu",
    frames,
    measurements: [measurement()],
  }), /no measured GPU sort submission/);
});

test("fixed-camera Adaptive may reuse a terminal CPU order while moving still requires both", () => {
  const frames = joinOrderingEvidence({
    requestedBackend: "adaptive",
    frames: [frame({
      order_backend: "cpu",
      sort_refreshed: false,
      submitted_measurement_ticket: null,
      visible_count_source: undefined,
      visible_count_ticket: undefined,
    })],
    measurements: [],
  });
  assert.doesNotThrow(() => validateOrderingEvidence({
    requestedBackend: "adaptive",
    frames,
    measurements: [],
    fixedCameraReuse: true,
  }));
  assert.throws(() => validateOrderingEvidence({
    requestedBackend: "adaptive",
    frames,
    measurements: [],
  }), /no completed GPU order measurement/);
});

test("final validation rejects a count claim that disagrees with the joined receipt", () => {
  assert.throws(
    () => validateOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame({ visible: 1, drawn: 1 })],
      measurements: [measurement({ visible: 999, contributor: 999, drawn: 999 })],
    }),
    /V\/C\/D counts do not match/,
  );
});

test("CPU ticket collision cannot satisfy a different GPU submission", () => {
  const frames = [
    frame({
      frame_index: 0,
      order_backend: "cpu",
      submitted_measurement_ticket: 7,
      visible_count_source: undefined,
      visible_count_ticket: undefined,
    }),
    frame({
      frame_index: 1,
      submitted_measurement_ticket: 8,
      camera_revision: 5,
      visible_count_ticket: 8,
      visible_count_revision: 5,
    }),
  ];
  assert.throws(
    () => joinOrderingEvidence({
      requestedBackend: "adaptive",
      frames,
      measurements: [measurement({ ticket: 7 })],
    }),
    /missing measured-frame ticket\(s\): 8/,
  );
});

test("every measured GPU sort refresh requires a positive ticket", () => {
  assert.throws(
    () => joinOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame(), frame({ frame_index: 1, submitted_measurement_ticket: null })],
      measurements: [measurement()],
    }),
    /lacks a positive submission ticket/,
  );
});

test("a GPU ticket terminates as success or structured failure, never both", () => {
  assert.throws(
    () => joinOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame()],
      measurements: [],
      failures: [failure()],
    }),
    /terminated with structured failure readback_map/,
  );
  assert.throws(
    () => joinOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame()],
      measurements: [measurement()],
      failures: [failure()],
    }),
    /contradictory success and failure receipts/,
  );
  assert.throws(
    () => joinOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame()],
      measurements: [],
      failures: [],
    }),
    /terminal receipts are incomplete/,
  );
});

test("issued-ticket ledger includes warmup tickets and requires one matching terminal receipt", () => {
  const submissions = [
    { actual_backend: "gpu", ticket: 5, camera_revision: 2, phase: "warmup" },
    { actual_backend: "gpu", ticket: 7, camera_revision: 4, phase: "measured" },
  ];
  const measurements = [
    measurement({ ticket: 5, camera_revision: 2 }),
    measurement({ ticket: 7, camera_revision: 4 }),
  ];
  assert.doesNotThrow(() => validateTerminalTicketLedger({
    submissions,
    measurements,
    failures: [],
  }));
  assert.throws(
    () => validateTerminalTicketLedger({
      submissions,
      measurements: measurements.slice(1),
      failures: [],
    }),
    /issued order ticket 5 has no terminal receipt/,
  );
  assert.throws(
    () => validateTerminalTicketLedger({
      submissions,
      measurements: measurements.slice(1),
      failures: [failure({ ticket: 5, camera_revision: 2 })],
    }),
    /issued order ticket 5 terminated with structured failure readback_map/,
  );
});

test("issued-ticket ledger validates CPU and GPU terminal receipts together", () => {
  const submissions = [
    { actual_backend: "cpu", ticket: 6, camera_revision: 3, phase: "warmup" },
    { actual_backend: "gpu", ticket: 7, camera_revision: 4, phase: "measured" },
  ];
  assert.doesNotThrow(() => validateTerminalTicketLedger({
    submissions,
    cpuMeasurements: [cpuMeasurement()],
    measurements: [measurement()],
    failures: [],
  }));
  assert.throws(
    () => validateTerminalTicketLedger({
      submissions,
      cpuMeasurements: [],
      measurements: [measurement()],
      failures: [],
    }),
    /issued order ticket 6 has no terminal receipt/,
  );
});

test("strict Adaptive evidence rejects an explicit GPU fallback reason", () => {
  assert.throws(
    () => validateOrderingEvidence({
      requestedBackend: "adaptive",
      frames: [
        frame({ adaptive_gpu_failure: "out_of_memory" }),
        frame({ frame_index: 1, order_backend: "cpu" }),
      ],
      measurements: [measurement()],
    }),
    /adaptive GPU failure out_of_memory/,
  );
});

test("GPU evidence rejects missing receipts, fallbacks, zero counts, and revision mismatch", () => {
  assert.throws(
    () => validateOrderingEvidence({ requestedBackend: "gpu", frames: [frame()], measurements: [] }),
    /no completed GPU order measurement/,
  );
  assert.throws(
    () => validateOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame({ gpu_sort_fallback: true })],
      measurements: [measurement()],
    }),
    /silently fell back/,
  );
  assert.throws(
    () => validateOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame({ visible: 0, drawn: 0 })],
      measurements: [measurement({ visible: 0, contributor: 0, drawn: 0 })],
    }),
    /lacks an exact non-zero/,
  );
  assert.throws(
    () => joinOrderingEvidence({
      requestedBackend: "gpu",
      frames: [frame({ camera_revision: 9 })],
      measurements: [measurement({ camera_revision: 8 })],
    }),
    /does not match submitted frame revision/,
  );
});

test("Adaptive evidence must exercise both backends and joins only GPU refreshes", () => {
  const gpuOnlyFrames = joinOrderingEvidence({
    requestedBackend: "adaptive",
    frames: [frame()],
    measurements: [measurement()],
  });
  assert.throws(
    () => validateOrderingEvidence({
      requestedBackend: "adaptive",
      frames: gpuOnlyFrames,
      measurements: [measurement()],
    }),
    /both CPU and GPU/,
  );
  const frames = joinOrderingEvidence({
    requestedBackend: "adaptive",
    frames: [frame(), frame({
      frame_index: 1,
      order_backend: "cpu",
      submitted_measurement_ticket: 7,
      visible_count_source: undefined,
      visible_count_ticket: undefined,
    })],
    measurements: [measurement()],
  });
  assert.doesNotThrow(() => validateOrderingEvidence({
    requestedBackend: "adaptive",
    frames,
    measurements: [measurement()],
  }));
});
