import assert from "node:assert/strict";
import test from "node:test";

import {
  appendBenchmarkSample,
  benchmarkSummary,
  createBenchmarkCollector,
  distribution,
  frameRecords,
} from "../src/benchmark-artifact.mjs";

function append(collector, value, index) {
  appendBenchmarkSample(collector, {
    elapsed_ns: index * 1_000,
    call_ms: value,
    frame_wall_ms: value,
    preprocess_ms: value / 10,
    sort_ms: value / 5,
    geometry_submit_ms: value / 4,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: 2,
    drawn: 2,
    sort_refreshed: null,
  });
}

test("nearest-rank distributions match the five-frame golden vector", () => {
  assert.deepEqual(distribution([1, 2, 3, 4, 5]), {
    count: 5,
    mean: 3,
    p50: 3,
    p90: 5,
    p95: 5,
    p99: 5,
    max: 5,
  });
});

test("summary preserves unavailable GPU metrics as null", () => {
  const collector = createBenchmarkCollector({ runId: "web-test", warmupCount: 2, frameBudgetMs: 3.5 });
  [1, 2, 3, 4, 5].forEach((value, index) => append(collector, value, index));
  const summary = benchmarkSummary(collector);

  assert.equal(summary.sample_count, 5);
  assert.equal(summary.missed_frame_count, 2);
  assert.equal(summary.distributions.gpu_wait_ms, null);
  assert.equal(summary.distributions.gpu_complete_ms, null);
  assert.equal(summary.distributions.frame_wall_ms.p95, 5);
  assert.equal(frameRecords(collector)[4].frame_index, 4);
});

test("collector rejects non-finite values and non-monotonic timestamps", () => {
  const collector = createBenchmarkCollector({ runId: "web-test", warmupCount: 0, frameBudgetMs: 16.67 });
  append(collector, 1, 1);
  assert.throws(() => append(collector, 2, 0), /monotonic/);
  assert.throws(() => append(collector, Number.NaN, 2), /finite non-negative/);
});
