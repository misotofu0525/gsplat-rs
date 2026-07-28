import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';
import {
  canonicalPlayCanvasCameraOracle,
  createPlayCanvasCenteredPinholeProjectionController,
  createPlayCanvasCameraReceipt,
  PLAYCANVAS_CAMERA_RECEIPT_SCHEMA,
  PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA,
  PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA,
  playCanvasProjectionFromTraceFrame,
  traceFrameIndexForPhase,
  traceFrameToPlayCanvasPose,
  validatePlayCanvasCameraReceipt,
  validatePlayCanvasCaptureEvidence,
  validatePlayCanvasQualityOnlyCaptureEvidence,
  validatePlayCanvasScreenshotBinding
} from '../public/trace-camera.js';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..');
const truckTrace = JSON.parse(await readFile(resolve(
  root,
  'tests/perf/trace/fixtures/quality/candidate-truck-quality-2412x1080-v1.json'
), 'utf8'));
const formalTruckTrace = JSON.parse(await readFile(resolve(
  root,
  'tests/perf/trace/fixtures/quality/formal-truck-product-quality-979x546-v1/camera-trace.json'
), 'utf8'));

function close(actual, expected, tolerance = 1e-12) {
  assert.equal(actual.length, expected.length);
  actual.forEach((value, index) => assert.ok(Math.abs(value - expected[index]) <= tolerance,
    `component ${index}: expected ${expected[index]}, got ${value}`));
}

function multiplyRowMajor(a, b) {
  return Array.from({ length: 16 }, (_, index) => {
    const row = Math.floor(index / 4);
    const column = index % 4;
    let value = 0;
    for (let k = 0; k < 4; k += 1) value += a[row * 4 + k] * b[k * 4 + column];
    return value;
  });
}

function withFocalRatio(trace, ratio) {
  const calibrated = structuredClone(trace);
  for (const frame of calibrated.frames) {
    frame.intrinsics.focal_length_x_over_y = ratio;
    frame.projection_matrix[0] *= ratio;
    frame.view_projection_matrix = multiplyRowMajor(frame.projection_matrix, frame.view_matrix);
  }
  return calibrated;
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
    focalLengthXOverY: oracle.focalLengthXOverY,
    horizontalFov: false,
    renderTargetFlipY: false,
    webGpuDepthRangeApplied: true,
    customProjectionActive: true,
    customProjectionHook: 'CameraComponent.calculateProjection',
    configuredProjectionMatrixOpenGlColumnMajor:
      oracle.playCanvas.projectionMatrixOpenGlColumnMajor,
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

function formalReceipt(phase) {
  const oracle = canonicalPlayCanvasCameraOracle(formalTruckTrace, 0);
  return createPlayCanvasCameraReceipt({
    trace: formalTruckTrace,
    traceFrameIndex: 0,
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

test('legacy trace defaults the exact centered-pinhole focal ratio to one', () => {
  const oracle = canonicalPlayCanvasCameraOracle(truckTrace, 0);
  assert.equal(oracle.focalLengthXOverY, 1);
  const projection = playCanvasProjectionFromTraceFrame(
    truckTrace.frames[0],
    truckTrace.display.width / truckTrace.display.height
  );
  assert.equal(projection.focalLengthXOverY, 1);
  close(
    projection.projectionMatrixOpenGlColumnMajor,
    oracle.playCanvas.projectionMatrixOpenGlColumnMajor
  );
});

test('formal Truck Product Quality views preserve the exact calibrated projection', () => {
  assert.deepEqual(formalTruckTrace.derivation.view_ids, ['000001', '000009']);
  assert.deepEqual(formalTruckTrace.display, { width: 979, height: 546 });
  for (let index = 0; index < formalTruckTrace.frames.length; index += 1) {
    const oracle = canonicalPlayCanvasCameraOracle(formalTruckTrace, index);
    const projection = playCanvasProjectionFromTraceFrame(
      formalTruckTrace.frames[index],
      formalTruckTrace.display.width / formalTruckTrace.display.height
    );
    assert.equal(oracle.focalLengthXOverY, 1.005624011459175);
    assert.equal(projection.focalLengthXOverY, oracle.focalLengthXOverY);
    close(
      projection.projectionMatrixOpenGlColumnMajor,
      oracle.playCanvas.projectionMatrixOpenGlColumnMajor
    );
  }
});

test('calibrated focal ratio reaches the pinned PlayCanvas custom projection hook', () => {
  const ratio = 581.9245675736333 / 578.6701201866216;
  const trace = withFocalRatio(truckTrace, ratio);
  const oracle = canonicalPlayCanvasCameraOracle(trace, 0);
  const target = {
    data: null,
    set(values) {
      this.data = [...values];
    }
  };
  const runtime = { calculateProjection: null, projectionMatrix: target };
  const cameraComponent = {
    camera: runtime,
    horizontalFov: true,
    fov: 0,
    nearClip: 0,
    farClip: 0,
    get calculateProjection() {
      return runtime.calculateProjection;
    },
    set calculateProjection(value) {
      runtime.calculateProjection = value;
    }
  };
  const controller = createPlayCanvasCenteredPinholeProjectionController({
    cameraComponent,
    aspect: trace.display.width / trace.display.height,
    centerView: 0
  });
  controller.applyFrame(trace.frames[0]);
  close(target.data, oracle.playCanvas.projectionMatrixOpenGlColumnMajor);
  const capture = controller.captureRuntimeProjection(runtime, target);
  assert.equal(capture.focalLengthXOverY, ratio);
  assert.equal(capture.customProjectionActive, true);
  assert.equal(capture.customProjectionHook, 'CameraComponent.calculateProjection');
  assert.equal(cameraComponent.horizontalFov, false);
  close(target.data, oracle.playCanvas.projectionMatrixOpenGlColumnMajor);
  close(
    capture.configuredProjectionMatrixOpenGlColumnMajor,
    oracle.playCanvas.projectionMatrixOpenGlColumnMajor
  );
  const calibratedReceipt = createPlayCanvasCameraReceipt({
    trace,
    traceFrameIndex: 0,
    phase: 'calibrated_test',
    observation: observationFromOracle(oracle)
  });
  assert.equal(
    validatePlayCanvasCameraReceipt(calibratedReceipt, trace, 0)
      .receipt.focal_length_x_over_y,
    ratio
  );
});

test('centered-pinhole controller fails closed on ratio bounds and hook replacement', () => {
  for (const ratio of [2 ** -16, 2 ** 16]) {
    assert.doesNotThrow(() => playCanvasProjectionFromTraceFrame(
      withFocalRatio(truckTrace, ratio).frames[0],
      16 / 9
    ));
  }
  for (const ratio of [0, 2 ** -17, 2 ** 17, NaN, Infinity, null, '1']) {
    const cameraComponent = { calculateProjection: null };
    const controller = createPlayCanvasCenteredPinholeProjectionController({
      cameraComponent,
      aspect: 16 / 9,
      centerView: 0
    });
    const trace = withFocalRatio(truckTrace, 1);
    trace.frames[0].intrinsics.focal_length_x_over_y = ratio;
    assert.throws(() => controller.applyFrame(trace.frames[0]), /focal_length_x_over_y/);
  }

  const runtime = { calculateProjection: null, projectionMatrix: { set() {} } };
  const cameraComponent = {
    camera: runtime,
    get calculateProjection() {
      return runtime.calculateProjection;
    },
    set calculateProjection(value) {
      runtime.calculateProjection = value;
    }
  };
  const controller = createPlayCanvasCenteredPinholeProjectionController({
    cameraComponent,
    aspect: 16 / 9,
    centerView: 0
  });
  controller.applyFrame(truckTrace.frames[0]);
  assert.throws(
    () => controller.captureRuntimeProjection(
      { calculateProjection() {} },
      { set() {} }
    ),
    /lost the exact custom projection hook/
  );
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

  const wrongRatio = structuredClone(valid);
  wrongRatio.focal_length_x_over_y = 1.25;
  assert.throws(
    () => validatePlayCanvasCameraReceipt(wrongRatio, truckTrace, 1),
    /focal length x\/y ratio/
  );

  const missingHook = structuredClone(valid);
  missingHook.custom_projection.active = false;
  assert.throws(
    () => validatePlayCanvasCameraReceipt(missingHook, truckTrace, 1),
    /custom projection hook/
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
      minimum_stable_frame_count: 3,
      measurement_terminal_submit_version: 12,
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

test('quality-only presentation validates without benchmark samples or timing', () => {
  const presentationFrames = [0, 1, 2].map((frameIndex) => ({
    trace_frame_index: 0,
    camera_receipt: formalReceipt(`presentation_frame_${frameIndex}`),
    submit_version_before: 20 + frameIndex,
    submit_version_after: 21 + frameIndex,
    queue_submit_call_count: 1
  }));
  presentationFrames.at(-1).renderer_capture_copy = { submit_version_after: 24 };
  const capture = {
    lifecycle: 'quality_only',
    qualityStartDrain: {
      phase: 'quality_only_start',
      frameLoopStopped: true,
      submitVersionStable: true,
      submitVersionAfter: 20
    },
    presentationCapture: {
      schema: PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA,
      ready_for_external_capture: true,
      excluded_from_performance: true,
      capture_trace_frame_index: 0,
      capture_trace_frame_source: 'explicit_quality_only_formal_view',
      stable_frame_count: 3,
      minimum_stable_frame_count: 3,
      measurement_terminal_submit_version: 20,
      frames: presentationFrames,
      renderer_capture: {
        schema: 'gsplat-playcanvas-webgpu-renderer-capture/v1',
        producer: 'playcanvas_webgpu_copy_texture_to_buffer',
        status: 'terminal',
        renderer_submit_version: 23,
        copy_submit_version_before: 23,
        copy_submit_version_after: 24,
        copy_map_complete: true,
        queue_terminal_complete: true,
        camera_receipt: presentationFrames.at(-1).camera_receipt
      },
      queue_drain: {
        phase: 'post_capture_presentation',
        frameLoopStopped: true,
        submitVersionStable: true,
        submitVersionBefore: 24,
        submitVersionAfter: 24
      },
      terminal_camera_receipt: formalReceipt('external_capture_terminal')
    }
  };
  const validation = validatePlayCanvasQualityOnlyCaptureEvidence({
    trace: formalTruckTrace,
    capture
  });
  assert.equal(validation.expectedCaptureIndex, 0);
  assert.equal('samples' in capture, false);
  assert.equal('timing' in capture, false);

  const wrongLifecycle = structuredClone(capture);
  wrongLifecycle.qualityStartDrain.phase = 'post_measurement_terminal';
  assert.throws(
    () => validatePlayCanvasQualityOnlyCaptureEvidence({
      trace: formalTruckTrace,
      capture: wrongLifecycle
    }),
    /quality-only capture lacks/
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
