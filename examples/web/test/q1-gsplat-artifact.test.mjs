import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalSha256,
  decorateQ1GsplatThroughput,
  q1TerminalWindow,
} from "../scripts/q1-gsplat-artifact.mjs";

const SHA = "a".repeat(64);

test("canonical frame hash matches Python sorted compact JSON ordering", () => {
  assert.equal(
    canonicalSha256({ z: 1, a: { y: 2, x: [3, true] } }),
    "28779f9cf0b8820866dcfbd8f71d3cea49a3148a6232bf2adac7410f570ce603",
  );
});

function throughputParsed() {
  const window = {
    mode: "terminal_queue_throughput_window",
    measured_submit_count: 80,
    draw_count_at_warmup_drain_completion: 20,
    draw_count_at_final_drain_start: 100,
    draw_count_at_completion: 100,
    first_measured_input_monotonic_ms: 10,
    last_measured_terminal_monotonic_ms: 90,
    terminal_window_ms: 80,
  };
  return {
    manifests: [JSON.stringify({
      benchmark_window: window,
      unavailable_fields: [],
      build: {},
      trace: {},
    })],
    frameRecords: Array.from({ length: 80 }, (_, frame_index) => JSON.stringify({
      frame_index,
      visible: 10,
      contributor: 8,
      drawn: 8,
    })),
    summaries: [JSON.stringify({ sample_count: 80 })],
  };
}

test("Q1 throughput retains only control-bound counts and the continuous terminal", () => {
  const parsed = decorateQ1GsplatThroughput({
    parsed: throughputParsed(),
    protocolSha256: SHA,
    controlBindings: [0, 1].map((trace_frame_index) => ({
      trace_frame_index,
      run_id: `control-${trace_frame_index}`,
      manifest_sha256: SHA,
      configuration_sha256: SHA,
    })),
    runContext: {
      pairing: { pair_id: "pair-1" },
      environment: { device: "fixture" },
      build_artifacts: { runtime_js: { path: "fixture", sha256: SHA } },
      configuration_sha256: SHA,
    },
  });
  const manifest = JSON.parse(parsed.manifests[0]);
  const frames = parsed.frameRecords.map(JSON.parse);
  const summary = JSON.parse(parsed.summaries[0]);
  assert.equal(manifest.q1_comparison.artifact_role, "throughput");
  assert.deepEqual(frames[0].visible, null);
  assert.equal(summary.sustained_throughput.mean_frame_ms, 1);
  assert.equal(summary.sustained_throughput.mean_fps, 1000);
});

test("Q1 terminal rejects an extra draw during final drain", () => {
  const parsed = throughputParsed();
  const manifest = JSON.parse(parsed.manifests[0]);
  manifest.benchmark_window.draw_count_at_completion += 1;
  assert.throws(() => q1TerminalWindow(manifest.benchmark_window), /continuous terminal/);
});
