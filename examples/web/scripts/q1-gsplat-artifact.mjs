import { createHash } from "node:crypto";

import {
  q1PresentationIdentity,
  validateQ1SamePresentCapture,
} from "../src/q1-gsplat-producer.mjs";

function fail(message) {
  throw new TypeError(`Q1 gsplat-rs artifact rejected: ${message}`);
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value).sort().map(
      (key) => `${JSON.stringify(key)}:${canonical(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

export function canonicalSha256(value) {
  return createHash("sha256").update(canonical(value)).digest("hex");
}

function parseOne(values, label) {
  if (!Array.isArray(values) || values.length !== 1) fail(`${label} must contain one record`);
  return JSON.parse(values[0]);
}

function parseFrames(parsed) {
  if (!Array.isArray(parsed.frameRecords) || parsed.frameRecords.length !== 80) {
    fail("artifact must contain exactly 80 measured frames");
  }
  return parsed.frameRecords.map((value) => JSON.parse(value));
}

function stringify(parsed, manifest, frames, summary) {
  return {
    ...parsed,
    manifests: [JSON.stringify(manifest)],
    frameRecords: frames.map((frame) => JSON.stringify(frame)),
    summaries: [JSON.stringify(summary)],
  };
}

function applyCommonIdentity(manifest, context) {
  if (context === null || typeof context !== "object" || Array.isArray(context)) {
    fail("run context must be an object");
  }
  if (context.pairing === null || typeof context.pairing !== "object"
      || context.environment === null || typeof context.environment !== "object"
      || context.build_artifacts === null || typeof context.build_artifacts !== "object") {
    fail("run context lacks pairing, environment, or build artifacts");
  }
  if (typeof context.configuration_sha256 !== "string"
      || !/^[0-9a-f]{64}$/.test(context.configuration_sha256)) {
    fail("run context lacks the frozen Q1 configuration digest");
  }
  manifest.pairing = context.pairing;
  manifest.environment = context.environment;
  manifest.build.artifacts = context.build_artifacts;
  manifest.trace.camera_mode = "trace_sequence";
  return context.configuration_sha256;
}

export function decorateQ1GsplatControl({
  parsed,
  capture,
  trace,
  traceFrameIndex,
  protocolSha256,
  runContext,
}) {
  const manifest = parseOne(parsed.manifests, "manifest");
  const summary = parseOne(parsed.summaries, "summary");
  const frames = parseFrames(parsed);
  const configurationSha256 = applyCommonIdentity(manifest, runContext);
  const terminalFrame = frames.find((frame) =>
    frame.capture_depth_precision !== undefined
      && frame.trace_frame_index === traceFrameIndex,
  );
  if (!terminalFrame
      || canonical(terminalFrame.capture_depth_precision) !== canonical(capture.receipt)) {
    fail("terminal frame does not retain the exact renderer capture receipt object");
  }
  const terminal = parsed.currentStatsTerminals
    .map((value) => JSON.parse(value))
    .find((value) => value.ticket === terminalFrame.current_stats_ticket);
  validateQ1SamePresentCapture({
    capture,
    frame: terminalFrame,
    terminal,
    traceFrameIndex,
  });
  const dimensions = Object.fromEntries([
    "requested", "surface", "internal_render", "presented",
  ].flatMap((stage) => [
    [`${stage}_width`, manifest.resolution[`${stage}_width`]],
    [`${stage}_height`, manifest.resolution[`${stage}_height`]],
  ]));
  manifest.q1_comparison = {
    artifact_role: "control",
    protocol_sha256: protocolSha256,
    configuration_sha256: configurationSha256,
    performance_evidence: false,
    count_scope: "exact_v_c_d_control_only",
    capture_trace_frame_index: traceFrameIndex,
    presentation_identity: q1PresentationIdentity({
      trace,
      traceFrameIndex,
      frame: terminalFrame,
      frameSha256: canonicalSha256(terminalFrame),
      dimensions,
    }),
  };
  return {
    parsed: stringify(parsed, manifest, frames, summary),
    rgba8: capture.rgba8,
    receipt: capture.receipt,
  };
}

export function q1TerminalWindow(benchmarkWindow) {
  if (benchmarkWindow?.mode !== "terminal_queue_throughput_window"
      || benchmarkWindow.measured_submit_count !== 80
      || benchmarkWindow.draw_count_at_final_drain_start
        !== benchmarkWindow.draw_count_at_completion
      || benchmarkWindow.completion_primitive
        !== "gpu_queue_on_submitted_work_done"
      || benchmarkWindow.queue_completion_timestamp_source
        !== "wgpu_queue_callback_performance_now"
      || benchmarkWindow.terminal_receipt_overhead?.queue_completion_callback !== true
      || benchmarkWindow.terminal_receipt_overhead?.fairness_assessment
        !== "same_queue_completion_primitive") {
    fail("throughput lacks its admitted continuous terminal window");
  }
  return {
    schema: "gsplat-q1-webgpu-terminal-window/v1",
    clock: "performance_now_monotonic",
    start_boundary: "first_measured_camera_input_accepted",
    end_boundary: "final_measured_gpu_queue_completion",
    completion_primitive: benchmarkWindow.completion_primitive,
    completion_timestamp_source: benchmarkWindow.queue_completion_timestamp_source,
    frame_loop_policy: "controlled_presented_raf",
    camera_mutation_point: "before_update_order_project_render",
    warmup_queue_drained: true,
    continuous_submissions: true,
    per_frame_observer_reads: 0,
    extra_submissions_during_terminal_drain: 0,
    measured_camera_input_count: 80,
    measured_submission_count: 80,
    dropped_frame_count: 0,
    submission_counter_stable_during_drain: true,
    submission_counter_before_first:
      benchmarkWindow.draw_count_at_warmup_drain_completion,
    submission_counter_after_last: benchmarkWindow.draw_count_at_final_drain_start,
    started_at_monotonic_ms: benchmarkWindow.first_measured_input_monotonic_ms,
    completed_at_monotonic_ms: benchmarkWindow.last_measured_terminal_monotonic_ms,
    duration_ms: benchmarkWindow.terminal_window_ms,
  };
}

export function decorateQ1GsplatThroughput({
  parsed,
  protocolSha256,
  controlBindings,
  runContext,
}) {
  const manifest = parseOne(parsed.manifests, "manifest");
  const summary = parseOne(parsed.summaries, "summary");
  const frames = parseFrames(parsed);
  const configurationSha256 = applyCommonIdentity(manifest, runContext);
  if (!Array.isArray(controlBindings) || controlBindings.length !== 2
      || controlBindings.some((binding, index) => binding.trace_frame_index !== index)) {
    fail("throughput must bind trace 0 and trace 1 controls exactly once");
  }
  for (const frame of frames) {
    frame.visible = null;
    // Throughput counts are intentionally unavailable and are bound to the
    // exact control captures instead. The canonical artifact schema treats
    // contributor and exact_contributor_compaction as one optional contract,
    // so omit both rather than publishing a half-present null receipt.
    delete frame.contributor;
    delete frame.exact_contributor_compaction;
    frame.drawn = null;
    delete frame.capture_depth_precision;
  }
  const terminal = q1TerminalWindow(manifest.benchmark_window);
  manifest.q1_comparison = {
    artifact_role: "throughput",
    protocol_sha256: protocolSha256,
    configuration_sha256: configurationSha256,
    performance_evidence: true,
    count_scope: "control_bound_v_c_d_unavailable",
    control_bindings: controlBindings,
    terminal_window: terminal,
  };
  manifest.unavailable_fields = [...new Set([
    ...(manifest.unavailable_fields ?? []),
    "frames[*].visible",
    "frames[*].contributor",
    "frames[*].drawn",
  ])];
  summary.sustained_throughput = {
    measured_frame_count: 80,
    terminal_window_ms: terminal.duration_ms,
    mean_frame_ms: terminal.duration_ms / 80,
    mean_fps: 80_000 / terminal.duration_ms,
  };
  return stringify(parsed, manifest, frames, summary);
}
