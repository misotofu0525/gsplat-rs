export const BENCHMARK_SCHEMA = "gsplat-benchmark/v1";

const METRICS = [
  "call_ms",
  "frame_wall_ms",
  "preprocess_ms",
  "sort_ms",
  "geometry_submit_ms",
  "gpu_wait_ms",
  "gpu_complete_ms",
];

function finiteNonNegative(value, name, nullable = false) {
  if (value === null && nullable) return null;
  if (!Number.isFinite(value) || value < 0) {
    throw new TypeError(`${name} must be a finite non-negative number${nullable ? " or null" : ""}`);
  }
  return value;
}

export function createBenchmarkCollector({ runId, warmupCount, frameBudgetMs }) {
  if (typeof runId !== "string" || runId.length === 0) throw new TypeError("runId is required");
  if (!Number.isInteger(warmupCount) || warmupCount < 0) throw new TypeError("warmupCount is invalid");
  if (!Number.isFinite(frameBudgetMs) || frameBudgetMs <= 0) throw new TypeError("frameBudgetMs is invalid");
  return { runId, warmupCount, frameBudgetMs, samples: [] };
}

export function appendBenchmarkSample(collector, sample) {
  const frame = {
    elapsed_ns: sample.elapsed_ns,
    call_ms: finiteNonNegative(sample.call_ms, "call_ms"),
    frame_wall_ms: finiteNonNegative(sample.frame_wall_ms, "frame_wall_ms"),
    renderer_frame_ms: finiteNonNegative(sample.renderer_frame_ms ?? sample.frame_wall_ms, "renderer_frame_ms"),
    preprocess_ms: finiteNonNegative(sample.preprocess_ms, "preprocess_ms"),
    sort_ms: finiteNonNegative(sample.sort_ms, "sort_ms"),
    geometry_submit_ms: finiteNonNegative(sample.geometry_submit_ms, "geometry_submit_ms"),
    gpu_wait_ms: finiteNonNegative(sample.gpu_wait_ms, "gpu_wait_ms", true),
    gpu_complete_ms: finiteNonNegative(sample.gpu_complete_ms, "gpu_complete_ms", true),
    visible: sample.visible,
    drawn: sample.drawn,
    sort_refreshed: sample.sort_refreshed ?? null,
  };
  if (!Number.isSafeInteger(frame.elapsed_ns) || frame.elapsed_ns < 0) throw new TypeError("elapsed_ns is invalid");
  if (!Number.isSafeInteger(frame.visible) || frame.visible < 0) throw new TypeError("visible is invalid");
  if (!Number.isSafeInteger(frame.drawn) || frame.drawn < 0) throw new TypeError("drawn is invalid");
  if (frame.sort_refreshed !== null && typeof frame.sort_refreshed !== "boolean") {
    throw new TypeError("sort_refreshed must be boolean or null");
  }
  const previous = collector.samples.at(-1);
  if (previous && frame.elapsed_ns < previous.elapsed_ns) throw new TypeError("elapsed_ns must be monotonic");
  collector.samples.push(frame);
}

export function nearestRank(values, percentile) {
  if (values.length === 0) throw new TypeError("nearestRank requires values");
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.max(Math.ceil(percentile * sorted.length) - 1, 0);
  return sorted[Math.min(index, sorted.length - 1)];
}

export function distribution(values) {
  const available = values.filter((value) => value !== null);
  if (available.length === 0) return null;
  let total = 0;
  for (const value of available) total += finiteNonNegative(value, "distribution value");
  return {
    count: available.length,
    mean: total / available.length,
    p50: nearestRank(available, 0.50),
    p90: nearestRank(available, 0.90),
    p95: nearestRank(available, 0.95),
    p99: nearestRank(available, 0.99),
    max: Math.max(...available),
  };
}

export function frameRecords(collector) {
  return collector.samples.map((sample, frameIndex) => ({
    schema: BENCHMARK_SCHEMA,
    record_type: "frame",
    run_id: collector.runId,
    frame_index: frameIndex,
    ...sample,
  }));
}

export function benchmarkSummary(collector) {
  if (collector.samples.length === 0) throw new TypeError("benchmark requires at least one sample");
  const distributions = Object.fromEntries(
    METRICS.map((metric) => [metric, distribution(collector.samples.map((sample) => sample[metric]))]),
  );
  return {
    schema: BENCHMARK_SCHEMA,
    record_type: "summary",
    run_id: collector.runId,
    sample_count: collector.samples.length,
    warmup_count: collector.warmupCount,
    frame_budget_ms: collector.frameBudgetMs,
    missed_frame_count: collector.samples.filter((sample) => sample.frame_wall_ms > collector.frameBudgetMs).length,
    distributions,
  };
}

export function legacyAverages(collector) {
  const count = collector.samples.length;
  if (count === 0) return { count: 0 };
  const average = (key) => collector.samples.reduce((total, sample) => total + sample[key], 0) / count;
  return {
    count,
    callMs: average("call_ms"),
    frameMs: average("renderer_frame_ms"),
    preprocessMs: average("preprocess_ms"),
    sortMs: average("sort_ms"),
    geometrySubmitMs: average("geometry_submit_ms"),
    visible: average("visible"),
    drawn: average("drawn"),
  };
}
