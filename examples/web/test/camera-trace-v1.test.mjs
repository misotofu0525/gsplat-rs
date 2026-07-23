import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  cameraBasisFromTraceFrame,
  cameraTraceFrame,
  createCameraTraceSequence,
  validateCameraTraceV1,
} from "../../../tests/perf/trace/camera-trace-v1.mjs";

const fixtureUrl = new URL("../../../tests/perf/trace/fixtures/camera-trace-v1.json", import.meta.url);

test("camera trace fixture validates and selects an exact frame", async () => {
  const trace = validateCameraTraceV1(JSON.parse(await readFile(fixtureUrl, "utf8")));
  const camera = cameraTraceFrame(trace, 2);
  const basis = cameraBasisFromTraceFrame(camera);

  assert.deepEqual(camera.position, [0.5, 0.125, -3]);
  assert.deepEqual(basis, {
    eye: [0.5, 0.125, -3],
    right: [1, 0, 0],
    up: [0, 1, 0],
    forward: [0, 0, 1],
  });
});

test("camera trace validation rejects matrix drift and bad frame selection", async () => {
  const trace = JSON.parse(await readFile(fixtureUrl, "utf8"));
  trace.frames[0].view_matrix[0] += 0.01;
  assert.throws(() => validateCameraTraceV1(trace), /view_matrix/);

  const valid = JSON.parse(await readFile(fixtureUrl, "utf8"));
  assert.throws(() => cameraTraceFrame(valid, 99), /out of range/);
});

test("camera trace sequence defaults to every changing revision once", async () => {
  const trace = JSON.parse(await readFile(fixtureUrl, "utf8"));
  const sequence = createCameraTraceSequence(trace);

  assert.deepEqual(sequence.frameIndices, [0, 1, 2]);
  assert.equal(sequence.warmupFrames, 0);
  assert.equal(sequence.measuredFrames, 3);
  assert.equal(sequence.loops, 1);
  assert.deepEqual(
    Array.from({ length: sequence.totalFrames }, (_, index) => sequence.step(index)),
    [
      { phase: "measure", loopIndex: 0, phaseFrameIndex: 0, measuredSampleIndex: 0,
        traceFrameIndex: 0, timestampNs: 0, camera: cameraTraceFrame(trace, 0) },
      { phase: "measure", loopIndex: 0, phaseFrameIndex: 1, measuredSampleIndex: 1,
        traceFrameIndex: 1, timestampNs: 16_666_667, camera: cameraTraceFrame(trace, 1) },
      { phase: "measure", loopIndex: 0, phaseFrameIndex: 2, measuredSampleIndex: 2,
        traceFrameIndex: 2, timestampNs: 33_333_334, camera: cameraTraceFrame(trace, 2) },
    ],
  );
});

test("camera trace sequence cycles selected indices through warmup and loops", async () => {
  const trace = JSON.parse(await readFile(fixtureUrl, "utf8"));
  const sequence = createCameraTraceSequence(trace, {
    frameIndices: [2, 0],
    warmupFrames: 3,
    measuredFrames: 3,
    loops: 2,
  });

  assert.deepEqual(
    Array.from({ length: sequence.totalFrames }, (_, index) => {
      const step = sequence.step(index);
      return [step.phase, step.loopIndex, step.phaseFrameIndex, step.traceFrameIndex];
    }),
    [
      ["warmup", 0, 0, 2], ["warmup", 0, 1, 0], ["warmup", 0, 2, 2],
      ["measure", 0, 0, 2], ["measure", 0, 1, 0], ["measure", 0, 2, 2],
      ["measure", 1, 0, 2], ["measure", 1, 1, 0], ["measure", 1, 2, 2],
    ],
  );
  assert.throws(
    () => createCameraTraceSequence(trace, { frameIndices: [1, 1] }),
    /duplicate/,
  );
  assert.throws(
    () => createCameraTraceSequence(trace, { frameIndices: [1] }),
    /at least two/,
  );
});
