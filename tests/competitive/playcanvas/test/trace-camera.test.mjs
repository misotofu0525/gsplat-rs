import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';
import {
  canonicalPlayCanvasCameraOracle,
  createPlayCanvasCameraReceipt,
  PLAYCANVAS_CAMERA_RECEIPT_SCHEMA,
  PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA,
  PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA,
  traceFrameIndexForPhase,
  traceFrameToPlayCanvasPose,
  validatePlayCanvasCameraReceipt,
  validatePlayCanvasCaptureEvidence,
  validatePlayCanvasScreenshotBinding
} from '../public/trace-camera.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..');
const truckTrace = JSON.parse(await readFile(resolve(
  root,
  'tests/perf/trace/fixtures/quality/candidate-truck-quality-2412x1080-v1.json'
), 'utf8'));

function close(actual, expected, tolerance = 1e-12) {
  assert.equal(actual.length, expected.length);
  actual.forEach((value, index) => assert.ok(Math.abs(value - expected[index]) <= tolerance,
    `component ${index}: expected ${expected[index]}, got ${value}`));
}

test('identity trace camera becomes PlayCanvas -Z forward', () => {
  const pose = traceFrameToPlayCanvasPose({
    pose: { position: [1, 2, 3], rotation_xyzw: [0, 0, 0, 1] }
  });
  close(pose.position, [1, 2, -3]);
  close(pose.forward, [0, 0, -1]);
  close(pose.up, [0, 1, 0]);
  close(pose.target, [1, 2, -4]);
});

test('trace rotation is preserved instead of replaced by a fixed look direction', () => {
  const half = Math.sqrt(0.5);
  const pose = traceFrameToPlayCanvasPose({
    pose: { position: [0, 0, 0], rotation_xyzw: [0, half, 0, half] }
  });
  close(pose.forward, [1, 0, 0]);
  close(pose.up, [0, 1, 0]);
});

test('official Truck trace produces an orthonormal PlayCanvas pose', async () => {
  for (const frame of truckTrace.frames) {
    const pose = traceFrameToPlayCanvasPose(frame);
    const forwardLength = Math.hypot(...pose.forward);
    const upLength = Math.hypot(...pose.up);
    const dot = pose.forward.reduce((sum, value, index) => sum + value * pose.up[index], 0);
    assert.ok(Math.abs(forwardLength - 1) < 1e-12);
    assert.ok(Math.abs(upLength - 1) < 1e-12);
    assert.ok(Math.abs(dot) < 1e-12);
  }
});

test('measurement sequence restarts at trace frame zero after a non-integral warmup cycle', () => {
  const warmup = Array.from({ length: 20 }, (_, phaseFrameIndex) =>
    traceFrameIndexForPhase('sequence', 0, 3, phaseFrameIndex));
  const measured = Array.from({ length: 6 }, (_, phaseFrameIndex) =>
    traceFrameIndexForPhase('sequence', 0, 3, phaseFrameIndex));
  assert.deepEqual(warmup.slice(-3), [2, 0, 1]);
  assert.deepEqual(measured, [0, 1, 2, 0, 1, 2]);
});

test('static trace selection ignores the phase frame index', () => {
  assert.equal(traceFrameIndexForPhase('static', 4, 0, 99), 4);
});

function observationFromOracle(oracle) {
  return {
    position: oracle.position,
    forward: oracle.forward,
    up: oracle.up,
    verticalFovRadians: oracle.verticalFovRadians,
    nearPlane: oracle.nearPlane,
    farPlane: oracle.farPlane,
    aspect: oracle.aspect,
    horizontalFov: false,
    renderTargetFlipY: false,
    webGpuDepthRangeApplied: true,
    viewMatrixColumnMajor: oracle.playCanvas.viewMatrixColumnMajor,
    projectionMatrixOpenGlColumnMajor: oracle.playCanvas.projectionMatrixOpenGlColumnMajor,
    viewProjectionMatrixOpenGlColumnMajor:
      oracle.playCanvas.viewProjectionMatrixOpenGlColumnMajor,
    shaderProjectionMatrixWebGpuColumnMajor:
      oracle.playCanvas.shaderProjectionMatrixWebGpuColumnMajor,
    shaderViewProjectionMatrixWebGpuColumnMajor:
      oracle.playCanvas.shaderViewProjectionMatrixWebGpuColumnMajor
  };
}

function receipt(traceFrameIndex, phase = 'test') {
  const oracle = canonicalPlayCanvasCameraOracle(truckTrace, traceFrameIndex);
  return createPlayCanvasCameraReceipt({
    trace: truckTrace,
    traceFrameIndex,
    phase,
    observation: observationFromOracle(oracle)
  });
}

test('canonical oracle explicitly converts +Z/[0,1] row-major into PlayCanvas -Z matrices', () => {
  const oracle = canonicalPlayCanvasCameraOracle(truckTrace, 0);
  assert.equal(oracle.traceFrameIndex, 0);
  close(oracle.position, [
    truckTrace.frames[0].pose.position[0],
    truckTrace.frames[0].pose.position[1],
    -truckTrace.frames[0].pose.position[2]
  ]);
  // PlayCanvas' raw matrix uses OpenGL -Z / [-1,1], while its WebGPU shader
  // transform maps the same near/far projection back to [0,1].
  assert.equal(oracle.playCanvas.projectionMatrixOpenGlColumnMajor[11], -1);
  assert.equal(oracle.playCanvas.shaderProjectionMatrixWebGpuColumnMajor[11], -1);
  assert.notEqual(
    oracle.playCanvas.projectionMatrixOpenGlColumnMajor[10],
    oracle.playCanvas.shaderProjectionMatrixWebGpuColumnMajor[10]
  );
  assert.equal(validatePlayCanvasCameraReceipt(receipt(0), truckTrace, 0).receipt.schema,
    PLAYCANVAS_CAMERA_RECEIPT_SCHEMA);
});

test('camera receipt validator fails closed on trace index, runtime matrix, and FOV mutation', () => {
  const valid = receipt(1, 'measurement_frame_1');

  const wrongIndex = structuredClone(valid);
  wrongIndex.trace_frame_index = 0;
  assert.throws(
    () => validatePlayCanvasCameraReceipt(wrongIndex, truckTrace, 1),
    /trace frame index mismatch/
  );

  const wrongMatrix = structuredClone(valid);
  wrongMatrix.shader_view_projection_matrix_webgpu_column_major[7] += 0.25;
  assert.throws(
    () => validatePlayCanvasCameraReceipt(wrongMatrix, truckTrace, 1),
    /WebGPU shader view-projection matrix/
  );

  const wrongFov = structuredClone(valid);
  wrongFov.vertical_fov_radians += 0.01;
  assert.throws(
    () => validatePlayCanvasCameraReceipt(wrongFov, truckTrace, 1),
    /vertical FOV/
  );
});

function cameraCaptureFixture() {
  const samples = [0, 1].map((traceFrameIndex, sampleIndex) => ({
    traceFrameIndex,
    cameraReceipt: receipt(traceFrameIndex, `measurement_frame_${sampleIndex}`)
  }));
  const presentationFrames = [0, 1, 2].map((frameIndex) => ({
    trace_frame_index: 1,
    camera_receipt: receipt(1, `presentation_frame_${frameIndex}`),
    submit_version_before: 12 + frameIndex,
    submit_version_after: 13 + frameIndex,
    queue_submit_call_count: 1
  }));
  presentationFrames.at(-1).renderer_capture_copy = { submit_version_after: 16 };
  const rendererCapture = {
    schema: 'gsplat-playcanvas-webgpu-renderer-capture/v1',
    producer: 'playcanvas_webgpu_copy_texture_to_buffer',
    status: 'terminal',
    renderer_submit_version: 15,
    copy_submit_version_before: 15,
    copy_submit_version_after: 16,
    copy_map_complete: true,
    queue_terminal_complete: true,
    camera_receipt: presentationFrames.at(-1).camera_receipt
  };
  return {
    samples,
    measurementDrain: { submitVersionAfter: 12 },
    presentationCapture: {
      schema: PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA,
      ready_for_external_capture: true,
      excluded_from_performance: true,
      capture_trace_frame_index: 1,
      capture_trace_frame_source: 'last_measured_trace_frame',
      stable_frame_count: 3,
      frames: presentationFrames,
      renderer_capture: rendererCapture,
      queue_drain: {
        phase: 'post_capture_presentation',
        frameLoopStopped: true,
        submitVersionStable: true,
        submitVersionBefore: 16,
        submitVersionAfter: 16
      },
      terminal_camera_receipt: receipt(1, 'external_capture_terminal')
    }
  };
}

test('sequence presentation capture is bound to the last measured frame by default', () => {
  const capture = cameraCaptureFixture();
  const validation = validatePlayCanvasCaptureEvidence({
    trace: truckTrace,
    capture,
    expectedMeasuredFrames: 2
  });
  assert.equal(validation.expectedCaptureIndex, 1);

  capture.presentationCapture.capture_trace_frame_index = 0;
  assert.throws(
    () => validatePlayCanvasCaptureEvidence({
      trace: truckTrace,
      capture,
      expectedMeasuredFrames: 2
    }),
    /presentation capture trace frame mismatch/
  );
});

test('explicit static presentation capture accepts its declared trace frame', () => {
  const capture = cameraCaptureFixture();
  capture.samples = [capture.samples[0], structuredClone(capture.samples[0])];
  capture.samples[1].cameraReceipt.phase = 'measurement_frame_1';
  capture.presentationCapture.capture_trace_frame_index = 1;
  capture.presentationCapture.capture_trace_frame_source = 'explicit_capture_trace_frame';
  const validation = validatePlayCanvasCaptureEvidence({
    trace: truckTrace,
    capture,
    expectedMeasuredFrames: 2,
    requestedCaptureTraceFrameIndex: 1
  });
  assert.equal(validation.expectedCaptureIndex, 1);
});

test('screenshot binding validator rejects trace-index and image-hash mutations', () => {
  const cameraReceipt = receipt(1, 'external_capture_terminal');
  const image = (file, hash) => ({
    file,
    sha256: hash,
    width: 2412,
    height: 1080,
    capture_trace_frame_index: 1,
    captured_after_presentation_terminal: true
  });
  const binding = {
    schema: PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA,
    presentation_state: 'ready_for_external_capture',
    capture_trace_frame_index: 1,
    camera_receipt_schema: PLAYCANVAS_CAMERA_RECEIPT_SCHEMA,
    camera_receipt_sha256: 'a'.repeat(64),
    canvas: image('final-frame.png', 'b'.repeat(64)),
    adb: image('device-screen.png', 'c'.repeat(64)),
    content_comparison: {
      file: 'canvas-device-ssim.json',
      schema: 'gsplat-image-parity/v1',
      metric: 'ssim-luma-srgb-window8',
      width: 2412,
      height: 1080,
      score: 0.999,
      threshold: 0.99,
      pass: true,
      canvas_sha256: 'b'.repeat(64),
      adb_sha256: 'c'.repeat(64)
    }
  };
  assert.equal(validatePlayCanvasScreenshotBinding({
    binding,
    cameraReceipt,
    expectedWidth: 2412,
    expectedHeight: 1080,
    requireAdb: true
  }), binding);

  const wrongIndex = structuredClone(binding);
  wrongIndex.adb.capture_trace_frame_index = 0;
  assert.throws(
    () => validatePlayCanvasScreenshotBinding({
      binding: wrongIndex,
      cameraReceipt,
      expectedWidth: 2412,
      expectedHeight: 1080,
      requireAdb: true
    }),
    /ADB screenshot binding/
  );

  const wrongHash = structuredClone(binding);
  wrongHash.canvas.sha256 = 'not-a-sha';
  assert.throws(
    () => validatePlayCanvasScreenshotBinding({
      binding: wrongHash,
      cameraReceipt,
      expectedWidth: 2412,
      expectedHeight: 1080,
      requireAdb: true
    }),
    /canvas screenshot binding/
  );

  const wrongContent = structuredClone(binding);
  wrongContent.content_comparison.score = 0.5;
  assert.throws(
    () => validatePlayCanvasScreenshotBinding({
      binding: wrongContent,
      cameraReceipt,
      expectedWidth: 2412,
      expectedHeight: 1080,
      requireAdb: true
    }),
    /content comparison/
  );
});
