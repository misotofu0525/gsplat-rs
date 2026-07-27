export const Q1_GSPLAT_CONTROL = Object.freeze({
  measuredFrames: 80,
  captureFrameByTrace: Object.freeze({ 0: 78, 1: 79 }),
  poseIntrinsicsSha256: Object.freeze({
    0: "a008beb20adfdc24af484b25112503edb636e03010028aaeae812c3454527b46",
    1: "4b1d63381a662226712fd58cf5b3ea120fe5378beb73c228a509f55ec393f265",
  }),
});

function fail(message) {
  throw new TypeError(`Q1 gsplat-rs producer rejected: ${message}`);
}

function positiveInteger(value, label, { allowZero = false } = {}) {
  if (!Number.isSafeInteger(value) || value < (allowZero ? 0 : 1)) {
    fail(`${label} must be ${allowZero ? "a non-negative" : "a positive"} safe integer`);
  }
  return value;
}

function sha256(value, label) {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    fail(`${label} must be lowercase SHA-256`);
  }
  return value;
}

export function q1CaptureMeasuredFrame(traceFrameIndex) {
  const result = Q1_GSPLAT_CONTROL.captureFrameByTrace[traceFrameIndex];
  if (result === undefined) fail("capture trace frame must be 0 or 1");
  return result;
}

export function normalizeQ1SurfaceCapture(raw) {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    fail("capture must be an object");
  }
  if (!(raw.rgba8 instanceof Uint8Array)) fail("capture lacks renderer-owned RGBA8 bytes");
  const identity = raw.identity;
  const depth = raw.depthPrecision;
  if (identity === null || typeof identity !== "object"
      || depth === null || typeof depth !== "object") {
    fail("capture lacks renderer identity or depth precision");
  }
  const receipt = {
    scene_generation: positiveInteger(identity.sceneGeneration, "scene generation"),
    camera_revision: positiveInteger(identity.cameraRevision, "camera revision"),
    viewport_generation: positiveInteger(
      identity.viewportGeneration,
      "viewport generation",
      { allowZero: true },
    ),
    contract_generation: positiveInteger(identity.contractGeneration, "contract generation"),
    plan_set_generation: positiveInteger(identity.planSetGeneration, "plan-set generation"),
    plan_id: String(identity.planId ?? ""),
    order_generation: positiveInteger(identity.orderGeneration, "order generation"),
    presentation_sequence: positiveInteger(
      identity.presentationSequence,
      "presentation sequence",
    ),
    width: positiveInteger(identity.width, "capture width"),
    height: positiveInteger(identity.height, "capture height"),
    rgba8_sha256: sha256(identity.rgba8Sha256, "capture RGBA8 digest"),
    profile: String(depth.profile ?? ""),
  };
  if (receipt.plan_id !== "GpuPreproject" || receipt.profile !== "ExactFull32") {
    fail("capture is not ExactFull32 GpuPreproject");
  }
  if (raw.rgba8.byteLength !== receipt.width * receipt.height * 4) {
    fail("capture RGBA8 byte length does not match its dimensions");
  }
  return Object.freeze({ receipt: Object.freeze(receipt), rgba8: raw.rgba8 });
}

export function validateQ1SamePresentCapture({ capture, frame, terminal, traceFrameIndex }) {
  const target = q1CaptureMeasuredFrame(traceFrameIndex);
  if (frame?.trace_frame_index !== traceFrameIndex || frame?.frame_index !== target) {
    fail("capture is not attached to the frozen terminal trace frame");
  }
  const receipt = capture.receipt;
  const pairs = [
    [frame.camera_revision, receipt.camera_revision, "frame camera revision"],
    [frame.current_stats_camera_revision, receipt.camera_revision, "current-stats camera revision"],
    [frame.current_stats_scene_generation, receipt.scene_generation, "scene generation"],
    [frame.current_stats_viewport_generation, receipt.viewport_generation, "viewport generation"],
    [frame.current_stats_contract_generation, receipt.contract_generation, "contract generation"],
    [frame.current_stats_plan_set_generation, receipt.plan_set_generation, "plan-set generation"],
    [frame.current_stats_order_generation, receipt.order_generation, "order generation"],
    [frame.current_stats_presentation_sequence, receipt.presentation_sequence, "presentation sequence"],
    [terminal?.camera_revision, receipt.camera_revision, "terminal camera revision"],
    [terminal?.presentation_sequence, receipt.presentation_sequence, "terminal presentation sequence"],
  ];
  for (const [actual, expected, label] of pairs) {
    if (actual !== expected) fail(`${label} split from renderer capture`);
  }
  if (frame.order_backend !== "gpu" || frame.gpu_order_producer !== "preproject"
      || frame.projected_execution !== "compact"
      || frame.raster_execution_plan !== "projected_quads_exact"
      || frame.gpu_sort_fallback !== false
      || terminal?.status !== "ready" || terminal?.plan !== "gpu_preproject"
      || terminal?.count_semantics !== "indirect_draw_equals_contributor") {
    fail("capture frame is not a successful GPU Preproject Compact Exact presentation");
  }
  return receipt;
}

export function q1PresentationIdentity({
  trace,
  traceFrameIndex,
  frame,
  frameSha256,
  dimensions,
}) {
  const target = q1CaptureMeasuredFrame(traceFrameIndex);
  if (frame?.frame_index !== target || frame?.trace_frame_index !== traceFrameIndex) {
    fail("presentation terminal frame mismatch");
  }
  sha256(frameSha256, "terminal frame digest");
  return {
    trace_frame_index: traceFrameIndex,
    camera: {
      trace_id: trace.trace_id,
      trace_content_sha256: trace.content_sha256,
      trace_frame_index: traceFrameIndex,
      pose_intrinsics_sha256: Q1_GSPLAT_CONTROL.poseIntrinsicsSha256[traceFrameIndex],
      camera_revision: frame.camera_revision,
    },
    terminal_identity: {
      run_id: frame.run_id,
      frame_index: frame.frame_index,
      frame_sha256: frameSha256,
      presentation_sequence: frame.presentation_sequence,
    },
    successful_present: true,
    queue_terminal_complete: true,
    captured_after_terminal: true,
    dimensions,
  };
}
