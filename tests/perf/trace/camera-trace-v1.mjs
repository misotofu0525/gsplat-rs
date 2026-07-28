const SCHEMA = "gsplat-camera-trace/v1";
const MATRIX_TOLERANCE = 1e-12;
const MIN_FOCAL_LENGTH_X_OVER_Y = 2 ** -16;
const MAX_FOCAL_LENGTH_X_OVER_Y = 2 ** 16;

function fail(message) {
  throw new TypeError(`invalid ${SCHEMA}: ${message}`);
}

function finiteVector(value, length, field) {
  if (!Array.isArray(value) || value.length !== length) fail(`${field} must contain ${length} numbers`);
  value.forEach((number, index) => {
    if (!Number.isFinite(number)) fail(`${field}[${index}] must be finite`);
  });
  return value;
}

function closeMatrix(actual, expected, field) {
  finiteVector(actual, 16, field);
  actual.forEach((value, index) => {
    if (Math.abs(value - expected[index]) > MATRIX_TOLERANCE) {
      fail(`${field}[${index}] mismatch`);
    }
  });
}

function focalLengthXOverY(intrinsics) {
  return intrinsics && Object.hasOwn(intrinsics, "focal_length_x_over_y")
    ? intrinsics.focal_length_x_over_y
    : 1;
}

function viewMatrix(position, rotation) {
  let [x, y, z, w] = rotation;
  x = -x;
  y = -y;
  z = -z;
  const r = [
    1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y),
    2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x),
    2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y),
  ];
  return [
    r[0], r[1], r[2], -(r[0] * position[0] + r[1] * position[1] + r[2] * position[2]),
    r[3], r[4], r[5], -(r[3] * position[0] + r[4] * position[1] + r[5] * position[2]),
    r[6], r[7], r[8], -(r[6] * position[0] + r[7] * position[1] + r[8] * position[2]),
    0, 0, 0, 1,
  ];
}

function projectionMatrix(intrinsics, aspect) {
  const f = 1 / Math.tan(intrinsics.vertical_fov_radians * 0.5);
  const focalRatio = focalLengthXOverY(intrinsics);
  const depth = intrinsics.far_plane / (intrinsics.far_plane - intrinsics.near_plane);
  return [
    f * focalRatio / aspect, 0, 0, 0,
    0, f, 0, 0,
    0, 0, depth, -intrinsics.near_plane * depth,
    0, 0, 1, 0,
  ];
}

function multiplyMat4(a, b) {
  return Array.from({ length: 16 }, (_, index) => {
    const row = Math.floor(index / 4);
    const column = index % 4;
    let value = 0;
    for (let k = 0; k < 4; k += 1) value += a[row * 4 + k] * b[k * 4 + column];
    return value;
  });
}

export function validateCameraTraceV1(trace) {
  if (!trace || typeof trace !== "object" || Array.isArray(trace)) fail("root must be an object");
  if (trace.schema !== SCHEMA) fail(`schema must equal ${SCHEMA}`);
  if (typeof trace.trace_id !== "string" || trace.trace_id.length === 0) fail("trace_id must be non-empty");
  if (!/^[0-9a-f]{64}$/.test(trace.content_sha256 ?? "")) fail("content_sha256 must be lowercase SHA-256 hex");
  const coordinate = trace.coordinate_system;
  if (!coordinate || coordinate.handedness !== "right" || coordinate.axes !== "RUF" || coordinate.camera_forward !== "+Z") {
    fail("coordinate_system does not match v1");
  }
  const convention = trace.matrix_convention;
  if (!convention || convention.storage_order !== "row-major" || convention.vector_convention !== "column"
      || convention.composition !== "projection * view * world_position" || convention.ndc_xy !== "[-1,1]"
      || convention.ndc_z !== "[0,1]" || convention.clip_w !== "camera_z") {
    fail("matrix_convention does not match v1");
  }
  const width = trace.display?.width;
  const height = trace.display?.height;
  if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) fail("display must be positive integers");
  if (!Array.isArray(trace.frames) || trace.frames.length === 0) fail("frames must be non-empty");

  let previousTimestamp = -1;
  trace.frames.forEach((frame, index) => {
    if (frame?.frame_index !== index) fail(`frames[${index}].frame_index must equal ${index}`);
    if (!Number.isSafeInteger(frame.timestamp_ns) || frame.timestamp_ns < 0 || (index > 0 && frame.timestamp_ns <= previousTimestamp)) {
      fail("frame timestamps must be non-negative safe integers and strictly increasing");
    }
    previousTimestamp = frame.timestamp_ns;
    const position = finiteVector(frame.pose?.position, 3, `frames[${index}].pose.position`);
    const rotation = finiteVector(frame.pose?.rotation_xyzw, 4, `frames[${index}].pose.rotation_xyzw`);
    const norm2 = rotation.reduce((sum, value) => sum + value * value, 0);
    if (Math.abs(norm2 - 1) > MATRIX_TOLERANCE) fail(`frames[${index}] quaternion must be normalized`);
    const intrinsics = frame.intrinsics;
    const focalRatio = focalLengthXOverY(intrinsics);
    if (!intrinsics || !Number.isFinite(intrinsics.vertical_fov_radians)
        || !Number.isFinite(intrinsics.near_plane) || !Number.isFinite(intrinsics.far_plane)
        || !Number.isFinite(focalRatio)
        || intrinsics.vertical_fov_radians <= 0 || intrinsics.vertical_fov_radians >= Math.PI
        || intrinsics.near_plane <= 0 || intrinsics.far_plane <= intrinsics.near_plane
        || focalRatio < MIN_FOCAL_LENGTH_X_OVER_Y
        || focalRatio > MAX_FOCAL_LENGTH_X_OVER_Y) {
      fail(`frames[${index}].intrinsics are invalid`);
    }
    const view = viewMatrix(position, rotation);
    const projection = projectionMatrix(intrinsics, width / height);
    closeMatrix(frame.view_matrix, view, `frames[${index}].view_matrix`);
    closeMatrix(frame.projection_matrix, projection, `frames[${index}].projection_matrix`);
    closeMatrix(frame.view_projection_matrix, multiplyMat4(projection, view), `frames[${index}].view_projection_matrix`);
  });
  return trace;
}

export function cameraTraceFrame(trace, index = 0) {
  validateCameraTraceV1(trace);
  if (!Number.isInteger(index) || index < 0 || index >= trace.frames.length) {
    fail(`frame ${index} is out of range`);
  }
  const frame = trace.frames[index];
  return {
    index,
    position: [...frame.pose.position],
    rotationXyzw: [...frame.pose.rotation_xyzw],
    intrinsics: {
      verticalFovRadians: frame.intrinsics.vertical_fov_radians,
      nearPlane: frame.intrinsics.near_plane,
      farPlane: frame.intrinsics.far_plane,
      focalLengthXOverY: focalLengthXOverY(frame.intrinsics),
    },
  };
}

export function createCameraTraceSequence(trace, options = {}) {
  validateCameraTraceV1(trace);
  const frameIndices = options.frameIndices == null
    ? trace.frames.map((_, index) => index)
    : [...options.frameIndices];
  if (frameIndices.length === 0) fail("frame_indices must be non-empty");
  if (frameIndices.length < 2) fail("trace sequence requires at least two frame indices");
  const seen = new Set();
  frameIndices.forEach((index, position) => {
    if (!Number.isSafeInteger(index) || index < 0 || index >= trace.frames.length) {
      fail(`frame_indices[${position}] is out of range`);
    }
    if (seen.has(index)) fail(`frame_indices contains duplicate ${index}`);
    seen.add(index);
  });

  const warmupFrames = options.warmupFrames ?? 0;
  const measuredFrames = options.measuredFrames ?? frameIndices.length;
  const loops = options.loops ?? 1;
  if (!Number.isSafeInteger(warmupFrames) || warmupFrames < 0) {
    fail("warmup_frames must be a non-negative safe integer");
  }
  if (!Number.isSafeInteger(measuredFrames) || measuredFrames <= 0) {
    fail("measured_frames must be a positive safe integer");
  }
  if (!Number.isSafeInteger(loops) || loops <= 0) {
    fail("loops must be a positive safe integer");
  }
  const measuredSampleCount = measuredFrames * loops;
  const totalFrames = warmupFrames + measuredSampleCount;
  if (!Number.isSafeInteger(measuredSampleCount) || !Number.isSafeInteger(totalFrames)) {
    fail("sequence length exceeds JavaScript safe integer range");
  }

  const step = (playbackIndex) => {
    if (!Number.isSafeInteger(playbackIndex) || playbackIndex < 0 || playbackIndex >= totalFrames) {
      fail(`playback frame ${playbackIndex} is out of range`);
    }
    const warmup = playbackIndex < warmupFrames;
    const measuredSampleIndex = warmup ? null : playbackIndex - warmupFrames;
    const loopIndex = warmup ? 0 : Math.floor(measuredSampleIndex / measuredFrames);
    const phaseFrameIndex = warmup
      ? playbackIndex
      : measuredSampleIndex % measuredFrames;
    const traceFrameIndex = frameIndices[phaseFrameIndex % frameIndices.length];
    const frame = trace.frames[traceFrameIndex];
    return {
      phase: warmup ? "warmup" : "measure",
      loopIndex,
      phaseFrameIndex,
      measuredSampleIndex,
      traceFrameIndex,
      timestampNs: frame.timestamp_ns,
      camera: cameraTraceFrame(trace, traceFrameIndex),
    };
  };

  return Object.freeze({
    frameIndices: Object.freeze(frameIndices),
    warmupFrames,
    measuredFrames,
    loops,
    measuredSampleCount,
    totalFrames,
    step,
  });
}

export function cameraBasisFromTraceFrame(camera) {
  const [x, y, z, w] = camera.rotationXyzw;
  const rotate = ([vx, vy, vz]) => {
    const tx = 2 * (y * vz - z * vy);
    const ty = 2 * (z * vx - x * vz);
    const tz = 2 * (x * vy - y * vx);
    return [
      vx + w * tx + (y * tz - z * ty),
      vy + w * ty + (z * tx - x * tz),
      vz + w * tz + (x * ty - y * tx),
    ];
  };
  return {
    eye: [...camera.position],
    right: rotate([1, 0, 0]),
    up: rotate([0, 1, 0]),
    forward: rotate([0, 0, 1]),
  };
}
