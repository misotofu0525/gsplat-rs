export const BENCHMARK_SCHEMA = "gsplat-benchmark/v1";
export const COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1";

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
  const contributorPresent = sample.contributor !== undefined && sample.contributor !== null;
  const compactionPresent = sample.exact_contributor_compaction !== undefined
    && sample.exact_contributor_compaction !== null;
  if (contributorPresent !== compactionPresent) {
    throw new TypeError("contributor and exact_contributor_compaction must be provided together");
  }
  if (contributorPresent) {
    frame.contributor = sample.contributor;
    frame.exact_contributor_compaction = sample.exact_contributor_compaction;
  }
  if (!Number.isSafeInteger(frame.elapsed_ns) || frame.elapsed_ns < 0) throw new TypeError("elapsed_ns is invalid");
  if (!Number.isSafeInteger(frame.visible) || frame.visible < 0) throw new TypeError("visible is invalid");
  if (!Number.isSafeInteger(frame.drawn) || frame.drawn < 0) throw new TypeError("drawn is invalid");
  if (contributorPresent) {
    if (!Number.isSafeInteger(frame.contributor) || frame.contributor < 0) {
      throw new TypeError("contributor is invalid");
    }
    if (typeof frame.exact_contributor_compaction !== "boolean") {
      throw new TypeError("exact_contributor_compaction must be boolean");
    }
    if (frame.contributor > frame.visible) {
      throw new TypeError("contributor must not exceed visible candidates");
    }
    if (frame.exact_contributor_compaction && frame.drawn !== frame.contributor) {
      throw new TypeError("exact contributor draw requires drawn == contributor");
    }
    if (!frame.exact_contributor_compaction && frame.drawn !== frame.visible) {
      throw new TypeError("non-compacted draw requires drawn == visible");
    }
  }
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

export function benchmarkSummaryFromFrameRecords(records, emittedSummary) {
  if (!Array.isArray(records) || records.length === 0) {
    throw new TypeError("frame records are required");
  }
  if (!emittedSummary || emittedSummary.schema !== BENCHMARK_SCHEMA
      || emittedSummary.record_type !== "summary") {
    throw new TypeError("emitted summary metadata is invalid");
  }
  const runId = records[0].run_id;
  if (typeof runId !== "string" || runId.length === 0 || emittedSummary.run_id !== runId) {
    throw new TypeError("frame/summary run_id mismatch");
  }
  const collector = createBenchmarkCollector({
    runId,
    warmupCount: emittedSummary.warmup_count,
    frameBudgetMs: emittedSummary.frame_budget_ms,
  });
  for (const [index, record] of records.entries()) {
    if (record.schema !== BENCHMARK_SCHEMA || record.record_type !== "frame"
        || record.run_id !== runId || record.frame_index !== index) {
      throw new TypeError(`invalid frame record at index ${index}`);
    }
    appendBenchmarkSample(collector, record);
  }
  return benchmarkSummary(collector);
}

export function benchmarkCountEvidence(records) {
  if (!Array.isArray(records) || records.length === 0) {
    throw new TypeError("frame records are required");
  }
  const eligible = records.filter((frame) => frame.count_statistics_eligible === true);
  if (eligible.length === 0) {
    throw new TypeError("benchmark has no post-join frame count eligible for statistics");
  }
  for (const frame of eligible) {
    if (!Number.isSafeInteger(frame.visible) || frame.visible < 0
        || !Number.isSafeInteger(frame.drawn) || frame.drawn < 0) {
      throw new TypeError("eligible frame has invalid visible/drawn counts");
    }
  }
  const contributorFrames = eligible.filter(
    (frame) => Number.isSafeInteger(frame.contributor)
      && typeof frame.exact_contributor_compaction === "boolean",
  );
  if (contributorFrames.length !== 0 && contributorFrames.length !== eligible.length) {
    throw new TypeError("eligible frames mix legacy and contributor count contracts");
  }
  return {
    source: "post_join_eligible_frames_only",
    count_semantics: contributorFrames.length === eligible.length
      ? COUNT_SEMANTICS
      : "legacy_visible_drawn_v1",
    eligible_frame_count: eligible.length,
    excluded_frame_count: records.length - eligible.length,
    visible: distribution(eligible.map((frame) => frame.visible)),
    contributor: contributorFrames.length === 0
      ? null
      : distribution(contributorFrames.map((frame) => frame.contributor)),
    drawn: distribution(eligible.map((frame) => frame.drawn)),
    exact_contributor_compaction_frame_count: contributorFrames.filter(
      (frame) => frame.exact_contributor_compaction,
    ).length,
  };
}

function positiveInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new TypeError(`${name} must be a positive integer`);
  }
  return value;
}

/**
 * Builds fail-closed full-resolution evidence from actual presented frames.
 * Browser CSS size and configured intent are deliberately insufficient: every
 * Surface, internal raster, and presented dimension must equal the requested
 * canvas backing size on every measured frame.
 */
export function benchmarkResolutionEvidence(receipts, requestedWidth, requestedHeight) {
  const width = positiveInteger(requestedWidth, "requested_width");
  const height = positiveInteger(requestedHeight, "requested_height");
  if (!Array.isArray(receipts) || receipts.length === 0) {
    throw new TypeError("full-resolution evidence requires presented frame receipts");
  }
  const stages = [
    ["surface", "surfaceWidth", "surfaceHeight"],
    ["internal_render", "internalRenderWidth", "internalRenderHeight"],
    ["presented", "presentedWidth", "presentedHeight"],
  ];
  for (const [index, receipt] of receipts.entries()) {
    for (const [stage, widthKey, heightKey] of stages) {
      const actualWidth = positiveInteger(receipt?.[widthKey], `frames[${index}].${widthKey}`);
      const actualHeight = positiveInteger(receipt?.[heightKey], `frames[${index}].${heightKey}`);
      if (actualWidth !== width || actualHeight !== height) {
        throw new TypeError(
          `full-resolution gate rejected ${stage} ${actualWidth}x${actualHeight}; ` +
          `requested ${width}x${height}`,
        );
      }
    }
  }
  return {
    requested_width: width,
    requested_height: height,
    surface_width: width,
    surface_height: height,
    internal_render_width: width,
    internal_render_height: height,
    presented_width: width,
    presented_height: height,
    dynamic_resolution: "disabled",
    upscaling: "disabled",
    full_resolution: true,
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
