import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  isQ1CaptureTraceStep,
  normalizeQ1SurfaceCapture,
  q1CaptureMeasuredFrame,
  q1PresentationIdentity,
  validateQ1SamePresentCapture,
} from "../src/q1-gsplat-producer.mjs";
import {
  createCameraTraceSequence,
  validateCameraTraceV1,
} from "../../../tests/perf/trace/camera-trace-v1.mjs";

const SHA = "a".repeat(64);
const EXAMPLE_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function rawCapture() {
  return {
    rgba8: new Uint8Array(8),
    identity: {
      sceneGeneration: 1,
      cameraRevision: 2,
      viewportGeneration: 0,
      contractGeneration: 3,
      planSetGeneration: 4,
      planId: "GpuPreproject",
      orderGeneration: 5,
      presentationSequence: 6,
      width: 2,
      height: 1,
      rgba8Sha256: SHA,
    },
    depthPrecision: { profile: "ExactFull32" },
  };
}

function frame(trace = 0) {
  return {
    run_id: "run-1",
    frame_index: q1CaptureMeasuredFrame(trace),
    trace_frame_index: trace,
    camera_revision: 2,
    current_stats_camera_revision: 2,
    current_stats_scene_generation: 1,
    current_stats_viewport_generation: 0,
    current_stats_contract_generation: 3,
    current_stats_plan_set_generation: 4,
    current_stats_order_generation: 5,
    current_stats_presentation_sequence: 6,
    presentation_sequence: 6,
    order_backend: "gpu",
    gpu_order_producer: "preproject",
    projected_execution: "compact",
    raster_execution_plan: "projected_quads_exact",
    gpu_sort_fallback: false,
  };
}

test("Q1 controls place one capture on alternating terminal frames", () => {
  assert.equal(q1CaptureMeasuredFrame(0), 78);
  assert.equal(q1CaptureMeasuredFrame(1), 79);
  assert.throws(() => q1CaptureMeasuredFrame(2), /must be 0 or 1/);
});

test("Q1 capture gate uses the camera trace measure-phase contract", async () => {
  const trace = validateCameraTraceV1(JSON.parse(await readFile(
    resolve(EXAMPLE_ROOT, "../../tests/perf/trace/fixtures/camera-trace-v1.json"),
    "utf8",
  )));
  const sequence = createCameraTraceSequence(trace, {
    frameIndices: [0, 1],
    warmupFrames: 20,
    measuredFrames: 80,
  });
  const trace0 = sequence.step(98);
  const trace1 = sequence.step(99);

  assert.deepEqual(
    [trace0.phase, trace0.phaseFrameIndex, trace0.traceFrameIndex],
    ["measure", 78, 0],
  );
  assert.deepEqual(
    [trace1.phase, trace1.phaseFrameIndex, trace1.traceFrameIndex],
    ["measure", 79, 1],
  );
  assert.equal(isQ1CaptureTraceStep(trace0, 0), true);
  assert.equal(isQ1CaptureTraceStep(trace1, 1), true);
  assert.equal(isQ1CaptureTraceStep(sequence.step(19), 0), false);
  assert.equal(isQ1CaptureTraceStep(sequence.step(97), 0), false);
  assert.equal(isQ1CaptureTraceStep(trace0, 1), false);
});

test("Q1 capture retains only the frozen renderer-owned receipt", () => {
  const capture = normalizeQ1SurfaceCapture(rawCapture());
  assert.equal(capture.receipt.plan_id, "GpuPreproject");
  assert.equal(capture.receipt.profile, "ExactFull32");
  assert.equal(capture.rgba8.byteLength, 8);
  assert.throws(
    () => normalizeQ1SurfaceCapture({ ...rawCapture(), rgba8: new Uint8Array(4) }),
    /byte length/,
  );
});

test("Q1 same-present join rejects camera and presentation drift", () => {
  const capture = normalizeQ1SurfaceCapture(rawCapture());
  const terminal = {
    status: "ready",
    plan: "gpu_preproject",
    count_semantics: "indirect_draw_equals_contributor",
    camera_revision: 2,
    presentation_sequence: 6,
  };
  assert.equal(validateQ1SamePresentCapture({
    capture,
    frame: frame(0),
    terminal,
    traceFrameIndex: 0,
  }), capture.receipt);
  assert.throws(() => validateQ1SamePresentCapture({
    capture,
    frame: { ...frame(0), current_stats_presentation_sequence: 7 },
    terminal,
    traceFrameIndex: 0,
  }), /presentation sequence split/);
});

test("Q1 presentation identity binds the frozen trace and terminal frame", () => {
  const value = q1PresentationIdentity({
    trace: { trace_id: "trace", content_sha256: "b".repeat(64) },
    traceFrameIndex: 1,
    frame: frame(1),
    frameSha256: SHA,
    dimensions: { requested_width: 1920 },
  });
  assert.equal(value.camera.pose_intrinsics_sha256,
    "4b1d63381a662226712fd58cf5b3ea120fe5378beb73c228a509f55ec393f265");
  assert.equal(value.terminal_identity.frame_index, 79);
});

test("Q1 collector uses the renderer-selected device and atomic publication", async () => {
  const collector = await readFile(
    resolve(EXAMPLE_ROOT, "scripts/collect-web-benchmark-artifact.mjs"),
    "utf8",
  );
  const main = await readFile(resolve(EXAMPLE_ROOT, "src/main.js"), "utf8");
  assert.doesNotMatch(collector, /navigator\.gpu\?*\.requestAdapter|requestAdapter\s*\(/);
  assert.match(collector, /GSPLAT_Q1_SURFACE_DEVICE_PRE/);
  assert.match(collector, /Q1ArtifactTransaction\.claim/);
  assert.ok(
    collector.indexOf("Q1ArtifactTransaction.claim") < collector.indexOf("server = await startHttpServer"),
  );
  assert.ok(
    collector.indexOf("await cleanupBrowserAndServer") < collector.indexOf("q1ArtifactTransaction.publish"),
  );
  assert.match(main, /diagnosticSurfaceDeviceReceipt\(\)/);
  assert.equal(main.match(/isQ1CaptureTraceStep\(/g)?.length, 2);
});
