import assert from "node:assert/strict";
import test from "node:test";

import {
  appendBenchmarkSample,
  COUNT_SEMANTICS,
  benchmarkCountEvidence,
  benchmarkResolutionEvidence,
  benchmarkSortTelemetry,
  benchmarkSummary,
  benchmarkSummaryFromFrameRecords,
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

test("terminal-queue frames may leave counts null for their bound control artifact", () => {
  const collector = createBenchmarkCollector({
    runId: "throughput-no-readback",
    warmupCount: 20,
    frameBudgetMs: 1000 / 60,
  });
  assert.doesNotThrow(() => appendBenchmarkSample(collector, {
    elapsed_ns: 1,
    call_ms: 1,
    frame_wall_ms: 2,
    renderer_frame_ms: 1,
    preprocess_ms: 0,
    sort_ms: 0,
    geometry_submit_ms: 1,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: null,
    drawn: null,
    sort_refreshed: true,
  }));
  assert.equal(frameRecords(collector)[0].visible, null);
  assert.throws(() => appendBenchmarkSample(collector, {
    elapsed_ns: 2,
    call_ms: 1,
    frame_wall_ms: 2,
    renderer_frame_ms: 1,
    preprocess_ms: 0,
    sort_ms: 0,
    geometry_submit_ms: 1,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: 1,
    drawn: null,
    sort_refreshed: true,
  }), /availability must match/);
});

test("collector rebuilds summary from post-join frame timings", () => {
  const collector = createBenchmarkCollector({ runId: "joined", warmupCount: 1, frameBudgetMs: 16.67 });
  [1, 2].forEach((value, index) => append(collector, value, index));
  const emitted = benchmarkSummary(collector);
  const records = frameRecords(collector).map((frame, index) => ({
    ...frame,
    gpu_complete_ms: index === 0 ? 4 : 8,
    order_backend: index === 0 ? "cpu" : "gpu",
    gpu_sort_fallback: false,
  }));

  const rebuilt = benchmarkSummaryFromFrameRecords(records, emitted);
  assert.deepEqual(rebuilt.distributions.gpu_complete_ms, {
    count: 2,
    mean: 6,
    p50: 4,
    p90: 8,
    p95: 8,
    p99: 8,
    max: 8,
  });
  assert.deepEqual(rebuilt.sort_telemetry, {
    cpu_frame_count: 1,
    gpu_frame_count: 1,
    gpu_sort_fallback_count: 0,
  });
});

test("sort telemetry is serialized only from complete per-frame receipts", () => {
  assert.deepEqual(benchmarkSortTelemetry([
    { order_backend: "cpu", gpu_sort_fallback: false },
    { order_backend: "gpu", gpu_sort_fallback: false },
    { order_backend: "cpu", gpu_sort_fallback: true },
  ]), {
    cpu_frame_count: 2,
    gpu_frame_count: 1,
    gpu_sort_fallback_count: 1,
  });
  assert.throws(
    () => benchmarkSortTelemetry([{ gpu_sort_fallback: false }]),
    /actual order backend/,
  );
  assert.throws(
    () => benchmarkSortTelemetry([{ order_backend: "cpu" }]),
    /boolean GPU sort fallback receipt/,
  );
});

test("count evidence excludes frames without a current CPU or joined GPU count", () => {
  const evidence = benchmarkCountEvidence([
    { visible: 5, drawn: 5, count_statistics_eligible: true },
    { visible: 999, drawn: 999, count_statistics_eligible: false },
    { visible: 7, drawn: 7, count_statistics_eligible: true },
  ]);
  assert.equal(evidence.eligible_frame_count, 2);
  assert.equal(evidence.excluded_frame_count, 1);
  assert.equal(evidence.count_semantics, "legacy_visible_drawn_v1");
  assert.equal(evidence.contributor, null);
  assert.equal(evidence.visible.mean, 6);
  assert.equal(evidence.drawn.max, 7);
  assert.throws(
    () => benchmarkCountEvidence([{ visible: 3, drawn: 3, count_statistics_eligible: false }]),
    /no post-join frame count eligible/,
  );
});

test("collector and summary preserve exact V/C/D contributor evidence", () => {
  const collector = createBenchmarkCollector({
    runId: "contributors",
    warmupCount: 0,
    frameBudgetMs: 16.67,
  });
  appendBenchmarkSample(collector, {
    elapsed_ns: 0,
    call_ms: 1,
    frame_wall_ms: 1,
    preprocess_ms: 0.1,
    sort_ms: 0.2,
    geometry_submit_ms: 0.3,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: 7,
    contributor: 5,
    drawn: 5,
    exact_contributor_compaction: true,
    sort_refreshed: true,
  });
  const [record] = frameRecords(collector);
  record.count_statistics_eligible = true;
  assert.equal(record.contributor, 5);
  assert.equal(record.exact_contributor_compaction, true);
  assert.deepEqual(benchmarkCountEvidence([record]), {
    source: "post_join_eligible_frames_only",
    count_semantics: COUNT_SEMANTICS,
    eligible_frame_count: 1,
    excluded_frame_count: 0,
    visible: distribution([7]),
    contributor: distribution([5]),
    drawn: distribution([5]),
    exact_contributor_compaction_frame_count: 1,
  });
});

test("collector rejects an incomplete or inconsistent contributor contract", () => {
  const collector = createBenchmarkCollector({
    runId: "bad-contributors",
    warmupCount: 0,
    frameBudgetMs: 16.67,
  });
  const base = {
    elapsed_ns: 0,
    call_ms: 1,
    frame_wall_ms: 1,
    preprocess_ms: 0.1,
    sort_ms: 0.2,
    geometry_submit_ms: 0.3,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: 7,
    drawn: 5,
    sort_refreshed: true,
  };
  assert.throws(
    () => appendBenchmarkSample(collector, { ...base, contributor: 5 }),
    /provided together/,
  );
  assert.throws(
    () => appendBenchmarkSample(collector, {
      ...base,
      contributor: 4,
      exact_contributor_compaction: true,
    }),
    /drawn == contributor/,
  );
  assert.throws(
    () => appendBenchmarkSample(collector, {
      ...base,
      contributor: 5,
      exact_contributor_compaction: false,
    }),
    /drawn == visible/,
  );
});

test("resolution evidence requires every actual stage to equal the requested backing size", () => {
  const receipt = {
    surfaceWidth: 640,
    surfaceHeight: 480,
    internalRenderWidth: 640,
    internalRenderHeight: 480,
    presentedWidth: 640,
    presentedHeight: 480,
  };
  assert.deepEqual(benchmarkResolutionEvidence([receipt, receipt], 640, 480), {
    requested_width: 640,
    requested_height: 480,
    surface_width: 640,
    surface_height: 480,
    internal_render_width: 640,
    internal_render_height: 480,
    presented_width: 640,
    presented_height: 480,
    dynamic_resolution: "disabled",
    upscaling: "disabled",
    full_resolution: true,
  });
});

test("resolution evidence rejects hidden downscale, upscale, missing, and drifting stages", () => {
  const exact = {
    surfaceWidth: 640,
    surfaceHeight: 480,
    internalRenderWidth: 640,
    internalRenderHeight: 480,
    presentedWidth: 640,
    presentedHeight: 480,
  };
  assert.throws(
    () => benchmarkResolutionEvidence([{ ...exact, internalRenderWidth: 320 }], 640, 480),
    /rejected internal_render 320x480/,
  );
  assert.throws(
    () => benchmarkResolutionEvidence([{ ...exact, presentedWidth: 1280 }], 640, 480),
    /rejected presented 1280x480/,
  );
  assert.throws(
    () => benchmarkResolutionEvidence([{ ...exact, presentedWidth: null }], 640, 480),
    /presentedWidth must be a positive integer/,
  );
  assert.throws(
    () => benchmarkResolutionEvidence([exact, { ...exact, surfaceHeight: 479 }], 640, 480),
    /rejected surface 640x479/,
  );
});
