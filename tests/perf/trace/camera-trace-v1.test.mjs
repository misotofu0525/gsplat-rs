import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { cameraTraceFrame, validateCameraTraceV1 } from "./camera-trace-v1.mjs";

const fixtureUrl = new URL("./fixtures/camera-trace-v1.json", import.meta.url);
const fixture = JSON.parse(await readFile(fixtureUrl, "utf8"));

function multiplyMat4(a, b) {
  return Array.from({ length: 16 }, (_, index) => {
    const row = Math.floor(index / 4);
    const column = index % 4;
    let value = 0;
    for (let k = 0; k < 4; k += 1) value += a[row * 4 + k] * b[k * 4 + column];
    return value;
  });
}

function withFocalRatio(ratio) {
  const trace = structuredClone(fixture);
  for (const frame of trace.frames) {
    frame.intrinsics.focal_length_x_over_y = ratio;
    frame.projection_matrix[0] *= ratio;
    frame.view_projection_matrix = multiplyMat4(frame.projection_matrix, frame.view_matrix);
  }
  return trace;
}

test("legacy traces strictly default the centered focal ratio to one", () => {
  const validated = validateCameraTraceV1(structuredClone(fixture));
  assert.equal(cameraTraceFrame(validated, 0).intrinsics.focalLengthXOverY, 1);
  assert.equal(Object.hasOwn(validated.frames[0].intrinsics, "focal_length_x_over_y"), false);
});

test("exact centered focal ratio drives projection and camera playback", () => {
  const ratio = 581.9245675736333 / 578.6701201866216;
  const trace = withFocalRatio(ratio);
  validateCameraTraceV1(trace);
  assert.equal(cameraTraceFrame(trace, 1).intrinsics.focalLengthXOverY, ratio);
  assert.equal(trace.frames[1].projection_matrix[0], fixture.frames[1].projection_matrix[0] * ratio);
});

test("focal-ratio matrix mutation fails closed", () => {
  const trace = withFocalRatio(1.125);
  trace.frames[0].projection_matrix[0] += 1e-6;
  assert.throws(() => validateCameraTraceV1(trace), /projection_matrix\[0\] mismatch/);
});

test("focal-ratio bounds are inclusive and invalid values fail closed", () => {
  for (const ratio of [2 ** -16, 2 ** 16]) validateCameraTraceV1(withFocalRatio(ratio));
  for (const ratio of [0, -(2 ** -16), 2 ** -17, 2 ** 17, NaN, Infinity, null, "1"]) {
    const trace = withFocalRatio(1);
    trace.frames[0].intrinsics.focal_length_x_over_y = ratio;
    assert.throws(() => validateCameraTraceV1(trace), /intrinsics are invalid/);
  }
});
