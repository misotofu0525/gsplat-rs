import {
  PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER,
  PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA
} from './webgpu-renderer-capture.js';

const EPSILON = 1e-12;
const TRACE_MATRIX_TOLERANCE = 1e-9;
const RUNTIME_ABSOLUTE_TOLERANCE = 2e-4;
const RUNTIME_RELATIVE_TOLERANCE = 2e-5;
const MIN_FOCAL_LENGTH_X_OVER_Y = 2 ** -16;
const MAX_FOCAL_LENGTH_X_OVER_Y = 2 ** 16;
const MIN_PRESENTATION_STABLE_FRAMES = 3;

export const PLAYCANVAS_CAMERA_RECEIPT_SCHEMA =
  'gsplat-playcanvas-runtime-camera-receipt/v1';
export const PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA =
  'gsplat-playcanvas-presentation-capture/v1';
export const PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA =
  'gsplat-playcanvas-screenshot-binding/v1';

function finiteNumber(value, label) {
  if (!Number.isFinite(value)) throw new Error(`${label} must be finite`);
  return Number(value);
}

function finiteVector(value, length, label) {
  if (!Array.isArray(value) || value.length !== length || value.some((item) => !Number.isFinite(item))) {
    throw new Error(`${label} must contain ${length} finite numbers`);
  }
  return value.map(Number);
}

function positiveSafeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return value;
}

function normalizeQuaternion(value) {
  const [x, y, z, w] = finiteVector(value, 4, 'trace quaternion');
  const length = Math.hypot(x, y, z, w);
  if (!(length > EPSILON)) throw new Error('trace quaternion has zero length');
  return [x / length, y / length, z / length, w / length];
}

function rotateVector(quaternion, vector) {
  const [x, y, z, w] = quaternion;
  const [vx, vy, vz] = vector;
  const tx = 2 * (y * vz - z * vy);
  const ty = 2 * (z * vx - x * vz);
  const tz = 2 * (x * vy - y * vx);
  return [
    vx + w * tx + (y * tz - z * ty),
    vy + w * ty + (z * tx - x * tz),
    vz + w * tz + (x * ty - y * tx)
  ];
}

function reflectRufWorldToPlayCanvas([x, y, z]) {
  return [x, y, -z];
}

function dot(a, b) {
  return a.reduce((sum, value, index) => sum + value * b[index], 0);
}

function multiplyRowMajor(a, b) {
  finiteVector(a, 16, 'left matrix');
  finiteVector(b, 16, 'right matrix');
  return Array.from({ length: 16 }, (_, index) => {
    const row = Math.floor(index / 4);
    const column = index % 4;
    let value = 0;
    for (let k = 0; k < 4; k += 1) value += a[row * 4 + k] * b[k * 4 + column];
    return value;
  });
}

function rowMajorToColumnMajor(matrix) {
  finiteVector(matrix, 16, 'row-major matrix');
  return Array.from({ length: 16 }, (_, index) => {
    const column = Math.floor(index / 4);
    const row = index % 4;
    return matrix[row * 4 + column];
  });
}

function maxDifference(actual, expected) {
  let maxAbsoluteError = -Infinity;
  let maxRelativeError = -Infinity;
  let maxIndex = -1;
  actual.forEach((value, index) => {
    const absolute = Math.abs(value - expected[index]);
    const relative = absolute / Math.max(Math.abs(expected[index]), 1);
    if (absolute > maxAbsoluteError) {
      maxAbsoluteError = absolute;
      maxRelativeError = relative;
      maxIndex = index;
    }
  });
  return { maxAbsoluteError, maxRelativeError, maxIndex };
}

function assertCloseVector(
  actualValue,
  expectedValue,
  label,
  absoluteTolerance = RUNTIME_ABSOLUTE_TOLERANCE,
  relativeTolerance = RUNTIME_RELATIVE_TOLERANCE
) {
  const actual = finiteVector(actualValue, expectedValue.length, label);
  const expected = finiteVector(expectedValue, expectedValue.length, `${label} oracle`);
  const mismatchIndex = actual.findIndex((value, index) => {
    const absolute = Math.abs(value - expected[index]);
    return absolute > absoluteTolerance + relativeTolerance * Math.abs(expected[index]);
  });
  if (mismatchIndex !== -1) {
    const difference = maxDifference(actual, expected);
    throw new Error(
      `${label} does not match canonical camera oracle at index ${mismatchIndex}; ` +
      `expected ${expected[mismatchIndex]}, got ${actual[mismatchIndex]}, ` +
      `max absolute error ${difference.maxAbsoluteError}`
    );
  }
  return maxDifference(actual, expected);
}

function assertCloseScalar(actualValue, expectedValue, label) {
  const actual = finiteNumber(actualValue, label);
  const expected = finiteNumber(expectedValue, `${label} oracle`);
  const absolute = Math.abs(actual - expected);
  if (absolute > RUNTIME_ABSOLUTE_TOLERANCE + RUNTIME_RELATIVE_TOLERANCE * Math.abs(expected)) {
    throw new Error(`${label} does not match canonical camera oracle; expected ${expected}, got ${actual}`);
  }
  return absolute;
}

function validateTraceHeader(trace) {
  if (!trace || typeof trace !== 'object' || Array.isArray(trace)) {
    throw new Error('canonical trace root must be an object');
  }
  if (trace.schema !== 'gsplat-camera-trace/v1') {
    throw new Error('canonical trace schema must be gsplat-camera-trace/v1');
  }
  const coordinate = trace.coordinate_system;
  if (coordinate?.handedness !== 'right' || coordinate?.axes !== 'RUF' ||
      coordinate?.camera_forward !== '+Z') {
    throw new Error('canonical trace coordinate system must be right-handed RUF/+Z-forward');
  }
  const convention = trace.matrix_convention;
  if (convention?.storage_order !== 'row-major' || convention?.vector_convention !== 'column' ||
      convention?.composition !== 'projection * view * world_position' ||
      convention?.ndc_xy !== '[-1,1]' || convention?.ndc_z !== '[0,1]' ||
      convention?.clip_w !== 'camera_z') {
    throw new Error('canonical trace matrix convention does not match gsplat-camera-trace/v1');
  }
  const width = positiveSafeInteger(trace.display?.width, 'canonical trace display width');
  const height = positiveSafeInteger(trace.display?.height, 'canonical trace display height');
  if (!Array.isArray(trace.frames) || trace.frames.length === 0) {
    throw new Error('canonical trace frames must be non-empty');
  }
  return { width, height, aspect: width / height };
}

function canonicalViewMatrix(frame) {
  const position = finiteVector(frame.pose?.position, 3, 'trace position');
  const quaternion = normalizeQuaternion(frame.pose?.rotation_xyzw);
  const right = rotateVector(quaternion, [1, 0, 0]);
  const up = rotateVector(quaternion, [0, 1, 0]);
  const forward = rotateVector(quaternion, [0, 0, 1]);
  return [
    ...right, -dot(right, position),
    ...up, -dot(up, position),
    ...forward, -dot(forward, position),
    0, 0, 0, 1
  ];
}

function focalLengthXOverY(frame) {
  const intrinsics = frame?.intrinsics;
  const value = intrinsics && Object.hasOwn(intrinsics, 'focal_length_x_over_y')
    ? intrinsics.focal_length_x_over_y
    : 1;
  if (!Number.isFinite(value) ||
      value < MIN_FOCAL_LENGTH_X_OVER_Y ||
      value > MAX_FOCAL_LENGTH_X_OVER_Y) {
    throw new Error(
      `trace focal_length_x_over_y must be finite and within ` +
      `[${MIN_FOCAL_LENGTH_X_OVER_Y}, ${MAX_FOCAL_LENGTH_X_OVER_Y}]`
    );
  }
  return Number(value);
}

function canonicalProjectionMatrix(frame, aspect) {
  const fov = finiteNumber(frame.intrinsics?.vertical_fov_radians, 'trace vertical FOV');
  const near = finiteNumber(frame.intrinsics?.near_plane, 'trace near plane');
  const far = finiteNumber(frame.intrinsics?.far_plane, 'trace far plane');
  if (!(fov > 0 && fov < Math.PI) || !(near > 0) || !(far > near)) {
    throw new Error('trace intrinsics are invalid');
  }
  const focal = 1 / Math.tan(fov * 0.5);
  const focalRatio = focalLengthXOverY(frame);
  const depth = far / (far - near);
  return [
    focal * focalRatio / aspect, 0, 0, 0,
    0, focal, 0, 0,
    0, 0, depth, -near * depth,
    0, 0, 1, 0
  ];
}

/**
 * Return the exact centered-pinhole OpenGL projection consumed by the pinned
 * PlayCanvas CameraComponent.calculateProjection hook.
 */
export function playCanvasProjectionFromTraceFrame(frame, aspect) {
  const validatedAspect = finiteNumber(aspect, 'trace display aspect');
  if (!(validatedAspect > 0)) throw new Error('trace display aspect must be positive');
  const canonicalProjection = canonicalProjectionMatrix(frame, validatedAspect);
  const canonicalToPlayCanvas = [
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, -1, 0,
    0, 0, 0, 1
  ];
  const ndcZeroOneToOpenGl = [
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 2, -1,
    0, 0, 0, 1
  ];
  const projectionMatrixOpenGlColumnMajor = rowMajorToColumnMajor(multiplyRowMajor(
    ndcZeroOneToOpenGl,
    multiplyRowMajor(canonicalProjection, canonicalToPlayCanvas)
  ));
  return {
    focalLengthXOverY: focalLengthXOverY(frame),
    projectionMatrixOpenGlColumnMajor
  };
}

/**
 * Install one runtime projection owner on a PlayCanvas CameraComponent. The
 * controller caches immutable per-frame matrices so trace playback only swaps
 * the selected matrix inside the measured loop. The exact same official hook
 * is used by PlayCanvas raster uniforms, GSplat projection/culling, and this
 * receipt path.
 */
export function createPlayCanvasCenteredPinholeProjectionController({
  cameraComponent,
  aspect,
  centerView
}) {
  if (!cameraComponent || typeof cameraComponent !== 'object') {
    throw new Error('PlayCanvas camera component is required');
  }
  const validatedAspect = finiteNumber(aspect, 'trace display aspect');
  if (!(validatedAspect > 0)) throw new Error('trace display aspect must be positive');
  if (!Number.isSafeInteger(centerView)) {
    throw new Error('PlayCanvas center view must be a safe integer');
  }
  if (cameraComponent.calculateProjection != null) {
    throw new Error('PlayCanvas camera already has a custom projection owner');
  }

  const projectionCache = new WeakMap();
  let current = null;
  const calculateProjection = (target, view) => {
    if (view !== centerView) {
      throw new Error(`unexpected PlayCanvas projection view ${view}`);
    }
    if (!current) throw new Error('PlayCanvas trace projection was used before frame application');
    if (!target || typeof target.set !== 'function') {
      throw new Error('PlayCanvas projection target must provide Mat4.set');
    }
    target.set(current.projectionMatrixOpenGlColumnMajor);
  };
  cameraComponent.calculateProjection = calculateProjection;

  return Object.freeze({
    applyFrame(frame) {
      let next = projectionCache.get(frame);
      if (!next) {
        next = playCanvasProjectionFromTraceFrame(frame, validatedAspect);
        projectionCache.set(frame, next);
      }
      // Commit only after every source value and matrix has validated.
      current = next;
      cameraComponent.horizontalFov = false;
      cameraComponent.fov = (frame.intrinsics.vertical_fov_radians * 180) / Math.PI;
      cameraComponent.nearClip = frame.intrinsics.near_plane;
      cameraComponent.farClip = frame.intrinsics.far_plane;
      // The pinned GSplat frustum culler reads CameraComponent.projectionMatrix
      // directly. Materialize the custom matrix now as well as retaining the
      // official callback used later by the renderer uniform path.
      const runtimeCamera = cameraComponent.camera;
      if (runtimeCamera) {
        if (runtimeCamera.calculateProjection !== calculateProjection) {
          throw new Error('PlayCanvas runtime camera did not retain the custom projection hook');
        }
        runtimeCamera.calculateProjection(runtimeCamera.projectionMatrix, centerView);
      }
      return next;
    },
    captureRuntimeProjection(runtimeCamera, target) {
      if (!current) throw new Error('PlayCanvas trace projection has no applied frame');
      if (runtimeCamera?.calculateProjection !== calculateProjection) {
        throw new Error('PlayCanvas runtime camera lost the exact custom projection hook');
      }
      runtimeCamera.calculateProjection(target, centerView);
      return {
        focalLengthXOverY: current.focalLengthXOverY,
        customProjectionActive: true,
        customProjectionHook: 'CameraComponent.calculateProjection',
        configuredProjectionMatrixOpenGlColumnMajor:
          [...current.projectionMatrixOpenGlColumnMajor]
      };
    }
  });
}

function assertTraceMatrix(actual, expected, label) {
  assertCloseVector(actual, expected, label, TRACE_MATRIX_TOLERANCE, TRACE_MATRIX_TOLERANCE);
}

/**
 * Recompute a camera frame from pose/intrinsics, verify the committed canonical
 * matrices, then explicitly convert from canonical RUF/+Z/[0,1] to the pinned
 * PlayCanvas column-major RUB/-Z/OpenGL convention and its WebGPU shader form.
 */
export function canonicalPlayCanvasCameraOracle(trace, traceFrameIndex) {
  const display = validateTraceHeader(trace);
  if (!Number.isSafeInteger(traceFrameIndex) || traceFrameIndex < 0 ||
      traceFrameIndex >= trace.frames.length) {
    throw new Error(`trace frame index ${traceFrameIndex} is out of range`);
  }
  const frame = trace.frames[traceFrameIndex];
  if (frame?.frame_index !== traceFrameIndex) {
    throw new Error(`trace frame ${traceFrameIndex} has a mismatched frame_index`);
  }
  const quaternion = normalizeQuaternion(frame.pose?.rotation_xyzw);
  const quaternionNorm = Math.hypot(...finiteVector(frame.pose.rotation_xyzw, 4, 'trace quaternion'));
  if (Math.abs(quaternionNorm - 1) > TRACE_MATRIX_TOLERANCE) {
    throw new Error(`trace frame ${traceFrameIndex} quaternion is not normalized`);
  }
  const canonicalView = canonicalViewMatrix(frame);
  const canonicalProjection = canonicalProjectionMatrix(frame, display.aspect);
  const canonicalViewProjection = multiplyRowMajor(canonicalProjection, canonicalView);
  assertTraceMatrix(frame.view_matrix, canonicalView, `trace frame ${traceFrameIndex} view_matrix`);
  assertTraceMatrix(
    frame.projection_matrix,
    canonicalProjection,
    `trace frame ${traceFrameIndex} projection_matrix`
  );
  assertTraceMatrix(
    frame.view_projection_matrix,
    canonicalViewProjection,
    `trace frame ${traceFrameIndex} view_projection_matrix`
  );

  // C reflects canonical +Z-forward world/camera coordinates into PlayCanvas
  // -Z-forward coordinates. D maps canonical NDC z [0,1] to OpenGL [-1,1].
  const c = [
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, -1, 0,
    0, 0, 0, 1
  ];
  const ndcZeroOneToOpenGl = [
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 2, -1,
    0, 0, 0, 1
  ];
  const playCanvasView = multiplyRowMajor(c, multiplyRowMajor(canonicalView, c));
  const playCanvasProjectionOpenGl = multiplyRowMajor(
    ndcZeroOneToOpenGl,
    multiplyRowMajor(canonicalProjection, c)
  );
  const playCanvasViewProjectionOpenGl = multiplyRowMajor(
    playCanvasProjectionOpenGl,
    playCanvasView
  );
  // Camera.applyShaderProjectionTransform(..., flipY=false, webgpu=true)
  // maps OpenGL z back to WebGPU [0,1]. Algebraically this is Pcanonical*C.
  const playCanvasProjectionWebGpu = multiplyRowMajor(canonicalProjection, c);
  const playCanvasViewProjectionWebGpu = multiplyRowMajor(
    playCanvasProjectionWebGpu,
    playCanvasView
  );
  const sourcePosition = finiteVector(frame.pose.position, 3, 'trace position');
  const sourceForward = rotateVector(quaternion, [0, 0, 1]);
  const sourceUp = rotateVector(quaternion, [0, 1, 0]);

  return {
    traceFrameIndex,
    aspect: display.aspect,
    focalLengthXOverY: focalLengthXOverY(frame),
    position: reflectRufWorldToPlayCanvas(sourcePosition),
    forward: reflectRufWorldToPlayCanvas(sourceForward),
    up: reflectRufWorldToPlayCanvas(sourceUp),
    verticalFovRadians: frame.intrinsics.vertical_fov_radians,
    nearPlane: frame.intrinsics.near_plane,
    farPlane: frame.intrinsics.far_plane,
    canonical: {
      viewMatrixRowMajor: canonicalView,
      projectionMatrixRowMajor: canonicalProjection,
      viewProjectionMatrixRowMajor: canonicalViewProjection
    },
    playCanvas: {
      viewMatrixColumnMajor: rowMajorToColumnMajor(playCanvasView),
      projectionMatrixOpenGlColumnMajor: rowMajorToColumnMajor(playCanvasProjectionOpenGl),
      viewProjectionMatrixOpenGlColumnMajor: rowMajorToColumnMajor(playCanvasViewProjectionOpenGl),
      shaderProjectionMatrixWebGpuColumnMajor: rowMajorToColumnMajor(playCanvasProjectionWebGpu),
      shaderViewProjectionMatrixWebGpuColumnMajor:
        rowMajorToColumnMajor(playCanvasViewProjectionWebGpu)
    }
  };
}

/**
 * Convert a gsplat-camera-trace/v1 RUF/+Z-forward pose into PlayCanvas'
 * +Y-up/-Z-forward camera convention. The PLY entity is reflected by
 * (1,-1,-1), so this applies the matching RUF-world Z reflection to the
 * camera without discarding the trace rotation.
 */
export function traceFrameToPlayCanvasPose(frame) {
  if (!frame || typeof frame !== 'object' || !frame.pose) {
    throw new Error('trace frame is missing pose');
  }
  const sourcePosition = finiteVector(frame.pose.position, 3, 'trace position');
  const quaternion = normalizeQuaternion(frame.pose.rotation_xyzw);
  const sourceForward = rotateVector(quaternion, [0, 0, 1]);
  const sourceUp = rotateVector(quaternion, [0, 1, 0]);
  const position = reflectRufWorldToPlayCanvas(sourcePosition);
  const forward = reflectRufWorldToPlayCanvas(sourceForward);
  const up = reflectRufWorldToPlayCanvas(sourceUp);
  const target = position.map((value, index) => value + forward[index]);
  return { position, target, forward, up };
}

export function createPlayCanvasCameraReceipt({
  trace,
  traceFrameIndex,
  phase,
  observation
}) {
  const receipt = {
    schema: PLAYCANVAS_CAMERA_RECEIPT_SCHEMA,
    trace_frame_index: traceFrameIndex,
    phase,
    runtime_source: 'live PlayCanvas Entity and Camera matrices after trace application',
    position: finiteVector(observation.position, 3, 'runtime camera position'),
    forward: finiteVector(observation.forward, 3, 'runtime camera forward'),
    up: finiteVector(observation.up, 3, 'runtime camera up'),
    vertical_fov_radians: finiteNumber(
      observation.verticalFovRadians,
      'runtime camera vertical FOV'
    ),
    near_plane: finiteNumber(observation.nearPlane, 'runtime camera near plane'),
    far_plane: finiteNumber(observation.farPlane, 'runtime camera far plane'),
    aspect: finiteNumber(observation.aspect, 'runtime camera aspect'),
    focal_length_x_over_y: finiteNumber(
      observation.focalLengthXOverY,
      'runtime camera focal length x/y ratio'
    ),
    horizontal_fov: observation.horizontalFov,
    render_target_flip_y: observation.renderTargetFlipY,
    webgpu_depth_range_applied: observation.webGpuDepthRangeApplied,
    custom_projection: {
      active: observation.customProjectionActive,
      hook: observation.customProjectionHook,
      configured_projection_matrix_opengl_column_major: finiteVector(
        observation.configuredProjectionMatrixOpenGlColumnMajor,
        16,
        'configured PlayCanvas custom projection matrix'
      )
    },
    view_matrix_column_major: finiteVector(
      observation.viewMatrixColumnMajor,
      16,
      'runtime PlayCanvas view matrix'
    ),
    projection_matrix_opengl_column_major: finiteVector(
      observation.projectionMatrixOpenGlColumnMajor,
      16,
      'runtime PlayCanvas OpenGL projection matrix'
    ),
    view_projection_matrix_opengl_column_major: finiteVector(
      observation.viewProjectionMatrixOpenGlColumnMajor,
      16,
      'runtime PlayCanvas OpenGL view-projection matrix'
    ),
    shader_projection_matrix_webgpu_column_major: finiteVector(
      observation.shaderProjectionMatrixWebGpuColumnMajor,
      16,
      'runtime PlayCanvas WebGPU shader projection matrix'
    ),
    shader_view_projection_matrix_webgpu_column_major: finiteVector(
      observation.shaderViewProjectionMatrixWebGpuColumnMajor,
      16,
      'runtime PlayCanvas WebGPU shader view-projection matrix'
    ),
    conversion: {
      world: 'canonical RUF +Z-forward to PlayCanvas RUB -Z-forward by diag(1,1,-1)',
      projection:
        'centered f*x/y/aspect canonical row-major +Z/[0,1] -> PlayCanvas column-major -Z/OpenGL[-1,1] -> WebGPU shader [0,1]',
      shader_flip_y: false
    },
    validation: {
      oracle: 'pose/intrinsics-recomputed canonical trace oracle; trace matrices verified independently',
      absolute_tolerance: RUNTIME_ABSOLUTE_TOLERANCE,
      relative_tolerance: RUNTIME_RELATIVE_TOLERANCE,
      passed: true
    }
  };
  // Construction is intentionally cheap enough to run at the measured frame
  // boundary. The independent oracle validation runs after the terminal
  // measurement drain, outside the timed interval, and again in the runner.
  // Keep `trace` in the signature so call sites cannot accidentally construct
  // an unbound receipt, while still deferring the expensive matrix oracle.
  validateTraceHeader(trace);
  return receipt;
}

export function validatePlayCanvasCameraReceipt(receipt, trace, expectedTraceFrameIndex) {
  if (receipt?.schema !== PLAYCANVAS_CAMERA_RECEIPT_SCHEMA) {
    throw new Error(`camera receipt schema must be ${PLAYCANVAS_CAMERA_RECEIPT_SCHEMA}`);
  }
  if (!Number.isSafeInteger(receipt.trace_frame_index) || receipt.trace_frame_index < 0 ||
      receipt.trace_frame_index !== expectedTraceFrameIndex) {
    throw new Error(
      `camera receipt trace frame index mismatch; expected ${expectedTraceFrameIndex}, ` +
      `got ${receipt.trace_frame_index}`
    );
  }
  if (typeof receipt.phase !== 'string' || receipt.phase.length === 0) {
    throw new Error('camera receipt phase must be non-empty');
  }
  if (receipt.horizontal_fov !== false || receipt.render_target_flip_y !== false ||
      receipt.webgpu_depth_range_applied !== true) {
    throw new Error('camera receipt projection mode is not the qualified vertical-FOV WebGPU path');
  }
  if (receipt.custom_projection?.active !== true ||
      receipt.custom_projection?.hook !== 'CameraComponent.calculateProjection') {
    throw new Error('camera receipt did not prove the PlayCanvas custom projection hook');
  }
  if (receipt.conversion?.shader_flip_y !== false || receipt.validation?.passed !== true) {
    throw new Error('camera receipt conversion/validation declaration is incomplete');
  }
  const oracle = canonicalPlayCanvasCameraOracle(trace, expectedTraceFrameIndex);
  const errors = [
    assertCloseVector(receipt.position, oracle.position, 'runtime camera position'),
    assertCloseVector(receipt.forward, oracle.forward, 'runtime camera forward'),
    assertCloseVector(receipt.up, oracle.up, 'runtime camera up'),
    assertCloseVector(
      receipt.view_matrix_column_major,
      oracle.playCanvas.viewMatrixColumnMajor,
      'runtime PlayCanvas view matrix'
    ),
    assertCloseVector(
      receipt.custom_projection.configured_projection_matrix_opengl_column_major,
      oracle.playCanvas.projectionMatrixOpenGlColumnMajor,
      'configured PlayCanvas custom projection matrix'
    ),
    assertCloseVector(
      receipt.projection_matrix_opengl_column_major,
      oracle.playCanvas.projectionMatrixOpenGlColumnMajor,
      'runtime PlayCanvas OpenGL projection matrix'
    ),
    assertCloseVector(
      receipt.view_projection_matrix_opengl_column_major,
      oracle.playCanvas.viewProjectionMatrixOpenGlColumnMajor,
      'runtime PlayCanvas OpenGL view-projection matrix'
    ),
    assertCloseVector(
      receipt.shader_projection_matrix_webgpu_column_major,
      oracle.playCanvas.shaderProjectionMatrixWebGpuColumnMajor,
      'runtime PlayCanvas WebGPU shader projection matrix'
    ),
    assertCloseVector(
      receipt.shader_view_projection_matrix_webgpu_column_major,
      oracle.playCanvas.shaderViewProjectionMatrixWebGpuColumnMajor,
      'runtime PlayCanvas WebGPU shader view-projection matrix'
    )
  ];
  assertCloseScalar(
    receipt.vertical_fov_radians,
    oracle.verticalFovRadians,
    'runtime camera vertical FOV'
  );
  assertCloseScalar(receipt.near_plane, oracle.nearPlane, 'runtime camera near plane');
  assertCloseScalar(receipt.far_plane, oracle.farPlane, 'runtime camera far plane');
  assertCloseScalar(receipt.aspect, oracle.aspect, 'runtime camera aspect');
  assertCloseScalar(
    receipt.focal_length_x_over_y,
    oracle.focalLengthXOverY,
    'runtime camera focal length x/y ratio'
  );
  return {
    receipt,
    oracle,
    max_absolute_error: Math.max(...errors.map((error) => error.maxAbsoluteError))
  };
}

export function validatePlayCanvasCaptureEvidence({
  trace,
  capture,
  expectedMeasuredFrames,
  requestedCaptureTraceFrameIndex = null
}) {
  positiveSafeInteger(expectedMeasuredFrames, 'expected measured frame count');
  if (!capture || !Array.isArray(capture.samples) || capture.samples.length !== expectedMeasuredFrames) {
    throw new Error('capture does not contain the expected measured samples');
  }
  capture.samples.forEach((sample, sampleIndex) => {
    if (!Number.isSafeInteger(sample.traceFrameIndex) || sample.traceFrameIndex < 0) {
      throw new Error(`measured sample ${sampleIndex} omitted its trace frame index`);
    }
    validatePlayCanvasCameraReceipt(sample.cameraReceipt, trace, sample.traceFrameIndex);
  });
  const lastMeasuredTraceFrameIndex = capture.samples.at(-1).traceFrameIndex;
  const expectedCaptureIndex = requestedCaptureTraceFrameIndex ?? lastMeasuredTraceFrameIndex;
  const expectedSelectionSource = requestedCaptureTraceFrameIndex === null
    ? 'last_measured_trace_frame'
    : 'explicit_capture_trace_frame';
  return validatePresentationCapture({
    trace,
    capture,
    expectedCaptureIndex,
    expectedSelectionSource,
    startSubmitVersion: capture.measurementDrain?.submitVersionAfter
  });
}

export function validatePlayCanvasQualityOnlyCaptureEvidence({ trace, capture }) {
  if (capture?.lifecycle !== 'quality_only' ||
      capture.qualityStartDrain?.phase !== 'quality_only_start' ||
      capture.qualityStartDrain?.frameLoopStopped !== true ||
      capture.qualityStartDrain?.submitVersionStable !== true) {
    throw new Error('quality-only capture lacks its stopped-loop start drain');
  }
  return validatePresentationCapture({
    trace,
    capture,
    expectedCaptureIndex: 0,
    expectedSelectionSource: 'explicit_quality_only_formal_view',
    startSubmitVersion: capture.qualityStartDrain.submitVersionAfter
  });
}

function validatePresentationCapture({
  trace,
  capture,
  expectedCaptureIndex,
  expectedSelectionSource,
  startSubmitVersion
}) {
  const presentation = capture.presentationCapture;
  if (presentation?.schema !== PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA ||
      presentation.ready_for_external_capture !== true ||
      presentation.excluded_from_performance !== true) {
    throw new Error('presentation capture did not reach the external-capture terminal state');
  }
  if (presentation.capture_trace_frame_index !== expectedCaptureIndex) {
    throw new Error(
      `presentation capture trace frame mismatch; expected ${expectedCaptureIndex}, ` +
      `got ${presentation.capture_trace_frame_index}`
    );
  }
  if (presentation.capture_trace_frame_source !== expectedSelectionSource) {
    throw new Error('presentation capture trace frame source is inconsistent');
  }
  if (!Number.isSafeInteger(presentation.stable_frame_count) ||
      presentation.stable_frame_count < MIN_PRESENTATION_STABLE_FRAMES ||
      presentation.minimum_stable_frame_count !== MIN_PRESENTATION_STABLE_FRAMES ||
      presentation.frames?.length !== presentation.stable_frame_count) {
    throw new Error(`presentation capture requires at least ${MIN_PRESENTATION_STABLE_FRAMES} stable frames`);
  }
  let expectedSubmitVersion = startSubmitVersion;
  if (!Number.isSafeInteger(expectedSubmitVersion) ||
      presentation.measurement_terminal_submit_version !== expectedSubmitVersion) {
    throw new Error('presentation capture start submit version is invalid');
  }
  presentation.frames.forEach((frame, frameIndex) => {
    if (frame.trace_frame_index !== expectedCaptureIndex) {
      throw new Error(`presentation frame ${frameIndex} used the wrong trace frame`);
    }
    validatePlayCanvasCameraReceipt(frame.camera_receipt, trace, expectedCaptureIndex);
    if (!Number.isSafeInteger(frame.submit_version_before) ||
        !Number.isSafeInteger(frame.submit_version_after) ||
        frame.submit_version_before !== expectedSubmitVersion ||
        frame.submit_version_after <= frame.submit_version_before ||
        frame.queue_submit_call_count !== frame.submit_version_after - frame.submit_version_before) {
      throw new Error(`presentation frame ${frameIndex} has an invalid submit receipt`);
    }
    expectedSubmitVersion = frame.submit_version_after;
  });
  const rendererCapture = presentation.renderer_capture;
  const finalPresentationFrame = presentation.frames.at(-1);
  if (rendererCapture?.schema !== PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA ||
      rendererCapture.producer !== PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER ||
      rendererCapture.status !== 'terminal' || rendererCapture.copy_map_complete !== true ||
      rendererCapture.queue_terminal_complete !== true ||
      rendererCapture.renderer_submit_version !== expectedSubmitVersion ||
      rendererCapture.copy_submit_version_before !== expectedSubmitVersion ||
      rendererCapture.copy_submit_version_after !== expectedSubmitVersion + 1 ||
      finalPresentationFrame?.renderer_capture_copy?.submit_version_after !==
        rendererCapture.copy_submit_version_after ||
      rendererCapture.camera_receipt?.trace_frame_index !== expectedCaptureIndex ||
      JSON.stringify(rendererCapture.camera_receipt) !==
        JSON.stringify(finalPresentationFrame?.camera_receipt)) {
    throw new Error('presentation terminal lacks a same-frame WebGPU renderer capture');
  }
  expectedSubmitVersion = rendererCapture.copy_submit_version_after;
  const drain = presentation.queue_drain;
  if (drain?.phase !== 'post_capture_presentation' || drain.frameLoopStopped !== true ||
      drain.submitVersionStable !== true || drain.submitVersionBefore !== expectedSubmitVersion ||
      drain.submitVersionAfter !== expectedSubmitVersion) {
    throw new Error('presentation terminal queue drain is invalid');
  }
  validatePlayCanvasCameraReceipt(
    presentation.terminal_camera_receipt,
    trace,
    expectedCaptureIndex
  );
  return { capture, presentation, expectedCaptureIndex };
}

export function validatePlayCanvasScreenshotBinding({
  binding,
  cameraReceipt,
  expectedWidth,
  expectedHeight,
  requireAdb
}) {
  if (binding?.schema !== PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA ||
      binding.presentation_state !== 'ready_for_external_capture') {
    throw new Error('screenshot binding schema/state is invalid');
  }
  if (binding.capture_trace_frame_index !== cameraReceipt?.trace_frame_index ||
      binding.camera_receipt_schema !== PLAYCANVAS_CAMERA_RECEIPT_SCHEMA) {
    throw new Error('screenshot binding is not attached to its terminal camera receipt');
  }
  if (!/^[0-9a-f]{64}$/.test(binding.camera_receipt_sha256 ?? '')) {
    throw new Error('screenshot binding camera receipt hash is invalid');
  }
  const validateImage = (image, label) => {
    if (!image || image.capture_trace_frame_index !== binding.capture_trace_frame_index ||
        image.width !== expectedWidth || image.height !== expectedHeight ||
        !/^[0-9a-f]{64}$/.test(image.sha256 ?? '') ||
        typeof image.file !== 'string' || image.file.length === 0 ||
        image.captured_after_presentation_terminal !== true) {
      throw new Error(`${label} screenshot binding is invalid`);
    }
  };
  validateImage(binding.canvas, 'canvas');
  if (requireAdb) {
    validateImage(binding.adb, 'ADB');
    const comparison = binding.content_comparison;
    if (!comparison || comparison.schema !== 'gsplat-image-parity/v1' ||
        comparison.metric !== 'ssim-luma-srgb-window8' ||
        comparison.width !== expectedWidth || comparison.height !== expectedHeight ||
        !Number.isFinite(comparison.score) || !Number.isFinite(comparison.threshold) ||
        comparison.score < comparison.threshold || comparison.pass !== true ||
        comparison.canvas_sha256 !== binding.canvas.sha256 ||
        comparison.adb_sha256 !== binding.adb.sha256 ||
        typeof comparison.file !== 'string' || comparison.file.length === 0) {
      throw new Error('canvas/ADB screenshot content comparison is invalid');
    }
  } else {
    if (binding.adb !== null) validateImage(binding.adb, 'ADB');
    if (binding.content_comparison !== null) {
      throw new Error('local screenshot binding cannot claim an ADB content comparison');
    }
  }
  return binding;
}

export function traceFrameIndexForPhase(
  cameraMode,
  requestedTraceFrameIndex,
  traceFrameCount,
  phaseFrameIndex
) {
  if (!['static', 'sequence'].includes(cameraMode)) {
    throw new Error('camera mode must be static or sequence');
  }
  if (!Number.isSafeInteger(requestedTraceFrameIndex) || requestedTraceFrameIndex < 0) {
    throw new Error('requested trace frame index must be a non-negative safe integer');
  }
  if (!Number.isSafeInteger(phaseFrameIndex) || phaseFrameIndex < 0) {
    throw new Error('phase frame index must be a non-negative safe integer');
  }
  if (cameraMode === 'static') return requestedTraceFrameIndex;
  if (!Number.isSafeInteger(traceFrameCount) || traceFrameCount <= 0) {
    throw new Error('sequence trace frame count must be a positive safe integer');
  }
  return phaseFrameIndex % traceFrameCount;
}

export const PLAYCANVAS_MIN_PRESENTATION_STABLE_FRAMES = MIN_PRESENTATION_STABLE_FRAMES;
