export const WEB_DEPTH_QUALITY = Object.freeze({
  width: 1920,
  height: 1080,
  traceFrameIndices: Object.freeze([0, 1, 0]),
  sourceCount: 279199,
  shDegree: 3,
  geometryPathId: 1,
  orderBackendId: 1,
  projectedPolicyId: 1,
  gpuProducerId: 1,
  exactProfile: "ExactFull32",
  candidateProfile: "CandidateStable20",
});

const FRAME_LIMITS = Object.freeze({
  ssim_luma_srgb_window8: ["min", 0.99],
  rgb_mae_normalized: ["max", 0.005],
  rgb_bad_pixel_fraction_over_3: ["max", 0.02],
  alpha_mae_normalized: ["max", 0.001],
  alpha_bad_pixel_fraction_over_1: ["max", 0.005],
});

function fail(message) {
  throw new Error(`Web Candidate20 quality rejected: ${message}`);
}

function object(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
  return value;
}

function integer(value, label, { positive = false } = {}) {
  if (!Number.isSafeInteger(value) || value < 0 || (positive && value === 0)) {
    fail(`${label} must be a ${positive ? "positive" : "non-negative"} safe integer`);
  }
  return value;
}

function finite(value, label) {
  if (!Number.isFinite(value) || value < 0) fail(`${label} must be finite and non-negative`);
  return value;
}

function string(value, label) {
  if (typeof value !== "string" || value.length === 0) fail(`${label} must be non-empty`);
  return value;
}

function sha256Text(value, label) {
  const text = string(value, label);
  if (!/^[0-9a-f]{64}$/.test(text)) fail(`${label} must be lowercase SHA-256`);
  return text;
}

export async function sha256Bytes(bytes) {
  if (!(bytes instanceof Uint8Array)) fail("hash input must be Uint8Array");
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, "0")).join("");
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function cameraValues(traceFrame) {
  const frame = object(traceFrame, "trace frame");
  const pose = object(frame.pose, "trace frame.pose");
  const intrinsics = object(frame.intrinsics, "trace frame.intrinsics");
  const position = pose.position;
  const rotation = pose.rotation_xyzw;
  if (!Array.isArray(position) || position.length !== 3 ||
      !Array.isArray(rotation) || rotation.length !== 4) {
    fail("trace pose dimensions are invalid");
  }
  const values = [
    ...position,
    ...rotation,
    intrinsics.vertical_fov_radians,
    intrinsics.near_plane,
    intrinsics.far_plane,
  ].map(Number);
  if (values.some((value) => !Number.isFinite(value))) fail("trace camera contains non-finite data");
  return new Float32Array(values);
}

function normalizeIdentity(raw, label) {
  const value = object(raw, label);
  return {
    scene_generation: integer(value.sceneGeneration, `${label}.sceneGeneration`, { positive: true }),
    camera_revision: integer(value.cameraRevision, `${label}.cameraRevision`, { positive: true }),
    viewport_generation: integer(value.viewportGeneration, `${label}.viewportGeneration`),
    contract_generation: integer(value.contractGeneration, `${label}.contractGeneration`, { positive: true }),
    plan_set_generation: integer(value.planSetGeneration, `${label}.planSetGeneration`, { positive: true }),
    plan_id: string(value.planId, `${label}.planId`),
    order_generation: integer(value.orderGeneration, `${label}.orderGeneration`, { positive: true }),
    presentation_sequence: integer(value.presentationSequence, `${label}.presentationSequence`, { positive: true }),
    width: integer(value.width, `${label}.width`, { positive: true }),
    height: integer(value.height, `${label}.height`, { positive: true }),
    rgba8_sha256: sha256Text(value.rgba8Sha256, `${label}.rgba8Sha256`),
  };
}

function normalizePrecision(raw, kind, label) {
  const value = object(raw, label);
  const identity = normalizeIdentity(value, label);
  if (kind === "depth") return { ...identity, profile: string(value.profile, `${label}.profile`) };
  if (kind === "projected") {
    return {
      ...identity,
      profile: string(value.profile, `${label}.profile`),
      axis_record_bytes: integer(value.axisRecordBytes, `${label}.axisRecordBytes`, { positive: true }),
    };
  }
  return {
    ...identity,
    codec_profile: string(value.profile, `${label}.profile`),
    mantissa_bits: integer(value.mantissaBits, `${label}.mantissaBits`, { positive: true }),
    symmetric_max_code: integer(value.symmetricMaxCode, `${label}.symmetricMaxCode`, { positive: true }),
    point_scale_bits: integer(value.pointScaleBits, `${label}.pointScaleBits`, { positive: true }),
    point_scale_max_code: integer(value.pointScaleMaxCode, `${label}.pointScaleMaxCode`, { positive: true }),
    range_chunk_splats: integer(value.rangeChunkSplats, `${label}.rangeChunkSplats`, { positive: true }),
    source_count: integer(value.sourceCount, `${label}.sourceCount`, { positive: true }),
    encoded_count: integer(value.encodedCount, `${label}.encodedCount`, { positive: true }),
    resident_count: integer(value.residentCount, `${label}.residentCount`, { positive: true }),
    addressable_count: integer(value.addressableCount, `${label}.addressableCount`, { positive: true }),
    source_sh_degree: integer(value.sourceShDegree, `${label}.sourceShDegree`),
    resident_sh_degree: integer(value.residentShDegree, `${label}.residentShDegree`),
    residual_coefficients_per_source: integer(
      value.residualCoefficientsPerSource,
      `${label}.residualCoefficientsPerSource`,
    ),
    plane_count: integer(value.planeCount, `${label}.planeCount`),
    bytes_per_source: integer(value.bytesPerSource, `${label}.bytesPerSource`),
  };
}

function commonIdentity(receipt) {
  const result = { ...receipt };
  for (const key of Object.keys(result)) {
    if (!["scene_generation", "camera_revision", "viewport_generation", "contract_generation",
      "plan_set_generation", "plan_id", "order_generation", "presentation_sequence", "width",
      "height", "rgba8_sha256"].includes(key)) delete result[key];
  }
  return result;
}

export async function normalizeAndValidateCapture({
  lane,
  captureIndex,
  capture,
  frame,
  terminal,
  loadReceipt,
  dataset,
}) {
  if (!['exact', 'candidate'].includes(lane)) fail(`unknown lane ${lane}`);
  integer(captureIndex, "capture index");
  const raw = object(capture, `${lane} capture`);
  if (!(raw.rgba8 instanceof Uint8Array)) fail(`${lane} capture lacks renderer-owned RGBA8`);
  const identity = normalizeIdentity(raw.identity, `${lane}.identity`);
  const depth = normalizePrecision(raw.depthPrecision, "depth", `${lane}.depthPrecision`);
  const projected = normalizePrecision(
    raw.projectedCachePrecision,
    "projected",
    `${lane}.projectedCachePrecision`,
  );
  const resident = normalizePrecision(raw.residentSh, "resident", `${lane}.residentSh`);
  for (const receipt of [depth, projected, resident]) {
    if (!sameJson(commonIdentity(receipt), identity)) fail(`${lane} precision identity split`);
  }
  if (identity.width !== WEB_DEPTH_QUALITY.width || identity.height !== WEB_DEPTH_QUALITY.height) {
    fail(`${lane} capture is not internal 1920x1080`);
  }
  if (raw.rgba8.byteLength !== identity.width * identity.height * 4) {
    fail(`${lane} RGBA8 byte length mismatch`);
  }
  if (await sha256Bytes(raw.rgba8) !== identity.rgba8_sha256) fail(`${lane} RGBA8 hash mismatch`);
  const expectedDepth = lane === "exact"
    ? WEB_DEPTH_QUALITY.exactProfile
    : WEB_DEPTH_QUALITY.candidateProfile;
  if (depth.profile !== expectedDepth || projected.profile !== "ExactAxes32" ||
      projected.axis_record_bytes !== 16 || resident.codec_profile !== "ExactSigned11BandScale5") {
    fail(`${lane} changed more than the depth-key profile`);
  }
  const sourceCount = integer(dataset.splat_count, "dataset.splat_count", { positive: true });
  const shDegree = integer(dataset.sh_degree, "dataset.sh_degree");
  const expectedResidual = (((shDegree + 1) ** 2) - 1) * 3;
  const expectedPlaneCount = [0, 1, 3, 4][shDegree];
  const residentFields = [resident.source_count, resident.encoded_count, resident.resident_count,
    resident.addressable_count];
  if (residentFields.some((value) => value !== sourceCount) ||
      resident.source_sh_degree !== shDegree || resident.resident_sh_degree !== shDegree ||
      resident.residual_coefficients_per_source !== expectedResidual ||
      resident.plane_count !== expectedPlaneCount || resident.bytes_per_source !== expectedPlaneCount * 16) {
    fail(`${lane} Resident SH does not preserve full Kitsune SH3 membership`);
  }

  const load = object(loadReceipt, `${lane}.loadReceipt`);
  for (const key of ["sourceCount", "decodedCount", "encodedCount", "residentCount", "addressableCount"]) {
    if (load[key] !== sourceCount) fail(`${lane} loadReceipt.${key} is incomplete`);
  }
  if (load.sourceShDegree !== shDegree || load.residentShDegree !== shDegree || load.fullQuality !== true ||
      load.sourceMembership !== "all" || load.samplingEnabled !== false || load.lodEnabled !== false ||
      load.partialScenePublished !== false) fail(`${lane} load receipt is not complete full-membership SH3`);

  const rendered = object(frame, `${lane}.frame`);
  if (rendered.framePresented !== true || rendered.orderBackend !== "gpu" ||
      rendered.projectedPolicy !== "compact" || rendered.projectedExecution !== "compact" ||
      rendered.rasterExecutionPlan !== "projected_quads_exact" || rendered.gpuOrderProducer !== "preproject" ||
      rendered.gpuSortFallback !== false) fail(`${lane} frame did not execute GPU Preproject Compact Exact`);
  if (rendered.cameraRevision !== identity.camera_revision) fail(`${lane} frame/capture camera mismatch`);

  const counts = object(terminal, `${lane}.currentStats`);
  if (counts.status !== "ready" || counts.plan !== "gpu_preproject" ||
      counts.countSemantics !== "indirect_draw_equals_contributor") {
    fail(`${lane} current-stats is not a ready GpuPreproject contributor receipt`);
  }
  for (const [camel, snake] of [["sceneGeneration", "scene_generation"],
    ["cameraRevision", "camera_revision"], ["viewportGeneration", "viewport_generation"],
    ["contractGeneration", "contract_generation"], ["planSetGeneration", "plan_set_generation"],
    ["orderGeneration", "order_generation"], ["presentationSequence", "presentation_sequence"]]) {
    if (counts[camel] !== identity[snake]) fail(`${lane} current-stats/capture ${camel} mismatch`);
  }
  if (counts.sourceCount !== sourceCount || counts.visibleCount > sourceCount ||
      counts.contributorCount > counts.visibleCount || counts.drawnCount !== counts.contributorCount) {
    fail(`${lane} current-stats violates source >= V >= C == D`);
  }
  return {
    rgba8: raw.rgba8,
    identity,
    depth_precision: depth,
    projected_cache_precision: projected,
    resident_sh: resident,
    frame: rendered,
    counts,
  };
}

async function pollCurrentStats(renderer, attempts = 240) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const receipt = renderer.pollCurrentStats();
    if (receipt.status === "ready") return receipt;
    if (!["empty"].includes(receipt.status)) fail(`current-stats terminal is ${receipt.status}`);
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  fail("current-stats did not terminalize");
}

export async function captureQualityLane({ module, wasmUrl, canvas, plyBytes, trace, dataset, lane }) {
  if (!navigator.gpu) fail("navigator.gpu is unavailable");
  await module.default({ module_or_path: wasmUrl });
  const renderer = await module.createRendererWithGeometryPath(
    canvas,
    plyBytes,
    WEB_DEPTH_QUALITY.width,
    WEB_DEPTH_QUALITY.height,
    WEB_DEPTH_QUALITY.geometryPathId,
  );
  try {
    renderer.setSortInterval(1);
    renderer.setProjectedPolicy(WEB_DEPTH_QUALITY.projectedPolicyId);
    await renderer.prepareGpuOrder();
    renderer.setOrderBackend(WEB_DEPTH_QUALITY.orderBackendId);
    await renderer.setGpuOrderProducerAsync(WEB_DEPTH_QUALITY.gpuProducerId);
    const loadReceipt = renderer.loadReceipt();
    const captures = [];
    for (let captureIndex = 0; captureIndex < WEB_DEPTH_QUALITY.traceFrameIndices.length; captureIndex += 1) {
      const traceFrameIndex = WEB_DEPTH_QUALITY.traceFrameIndices[captureIndex];
      renderer.setCamera(cameraValues(trace.frames[traceFrameIndex]));
      const request = renderer.requestCurrentStats();
      if (request.status !== "requested") fail(`${lane} current-stats request was ${request.status}`);
      await renderer.requestDiagnosticSurfaceCapture();
      const frame = renderer.renderFrame();
      if (frame.framePresented !== true) fail(`${lane} capture frame was not presented`);
      const capture = await renderer.takeDiagnosticSurfaceCapture();
      const terminal = await pollCurrentStats(renderer);
      captures.push(await normalizeAndValidateCapture({
        lane,
        captureIndex,
        capture,
        frame,
        terminal,
        loadReceipt,
        dataset,
      }));
    }
    return { loadReceipt, captures };
  } finally {
    renderer.free();
  }
}

function windowSsim(exact, candidate) {
  const count = exact.length;
  const meanExact = exact.reduce((sum, value) => sum + value, 0) / count;
  const meanCandidate = candidate.reduce((sum, value) => sum + value, 0) / count;
  let varianceExact = 0;
  let varianceCandidate = 0;
  let covariance = 0;
  for (let index = 0; index < count; index += 1) {
    const exactDelta = exact[index] - meanExact;
    const candidateDelta = candidate[index] - meanCandidate;
    varianceExact += exactDelta * exactDelta;
    varianceCandidate += candidateDelta * candidateDelta;
    covariance += exactDelta * candidateDelta;
  }
  const denominator = Math.max(count - 1, 1);
  varianceExact /= denominator;
  varianceCandidate /= denominator;
  covariance /= denominator;
  const c1 = (0.01 * 255) ** 2;
  const c2 = (0.03 * 255) ** 2;
  return ((2 * meanExact * meanCandidate + c1) * (2 * covariance + c2)) /
    ((meanExact ** 2 + meanCandidate ** 2 + c1) * (varianceExact + varianceCandidate + c2));
}

export function computeFrameMetrics(exact, candidate, width, height) {
  if (!(exact instanceof Uint8Array) || !(candidate instanceof Uint8Array) ||
      exact.length !== width * height * 4 || candidate.length !== exact.length) fail("metric RGBA shape mismatch");
  let rgbAbsolute = 0;
  let alphaAbsolute = 0;
  let rgbBad = 0;
  let alphaBad = 0;
  for (let offset = 0; offset < exact.length; offset += 4) {
    const errors = [0, 1, 2].map((channel) => Math.abs(exact[offset + channel] - candidate[offset + channel]));
    const alphaError = Math.abs(exact[offset + 3] - candidate[offset + 3]);
    rgbAbsolute += errors[0] + errors[1] + errors[2];
    alphaAbsolute += alphaError;
    if (errors.some((value) => value > 3)) rgbBad += 1;
    if (alphaError > 1) alphaBad += 1;
  }
  const scores = [];
  for (let top = 0; top < height; top += 8) {
    for (let left = 0; left < width; left += 8) {
      const exactLuma = [];
      const candidateLuma = [];
      for (let y = top; y < Math.min(top + 8, height); y += 1) {
        for (let x = left; x < Math.min(left + 8, width); x += 1) {
          const offset = (y * width + x) * 4;
          exactLuma.push(0.2126 * exact[offset] + 0.7152 * exact[offset + 1] + 0.0722 * exact[offset + 2]);
          candidateLuma.push(0.2126 * candidate[offset] + 0.7152 * candidate[offset + 1] + 0.0722 * candidate[offset + 2]);
        }
      }
      scores.push(windowSsim(exactLuma, candidateLuma));
    }
  }
  const pixels = width * height;
  const result = {
    ssim_luma_srgb_window8: scores.reduce((sum, value) => sum + value, 0) / scores.length,
    rgb_mae_normalized: rgbAbsolute / (255 * 3 * pixels),
    rgb_bad_pixel_fraction_over_3: rgbBad / pixels,
    alpha_mae_normalized: alphaAbsolute / (255 * pixels),
    alpha_bad_pixel_fraction_over_1: alphaBad / pixels,
  };
  for (const [key, [kind, limit]] of Object.entries(FRAME_LIMITS)) {
    if ((kind === "min" && result[key] < limit) || (kind === "max" && result[key] > limit)) {
      fail(`${key} failed the Balanced gate`);
    }
  }
  return result;
}

export function computeTemporalMetric(previousExact, currentExact, previousCandidate, currentCandidate) {
  if (![currentExact, previousCandidate, currentCandidate].every((value) =>
    value instanceof Uint8Array && value.length === previousExact.length)) fail("temporal RGBA shape mismatch");
  let absolute = 0;
  for (let offset = 0; offset < previousExact.length; offset += 4) {
    for (let channel = 0; channel < 3; channel += 1) {
      const exactDelta = currentExact[offset + channel] - previousExact[offset + channel];
      const candidateDelta = currentCandidate[offset + channel] - previousCandidate[offset + channel];
      absolute += Math.abs(candidateDelta - exactDelta);
    }
  }
  const result = absolute / (255 * 3 * (previousExact.length / 4));
  if (result > 0.005) fail("temporal_rgb_residual_mae_normalized failed the Balanced gate");
  return result;
}

export function validateMatchedPair(exact, candidate) {
  for (const field of ["plan_id", "order_generation"]) {
    if (exact.identity[field] !== candidate.identity[field]) fail(`Exact/Candidate ${field} mismatch`);
  }
  for (const field of ["scene_generation", "camera_revision", "viewport_generation",
    "contract_generation", "plan_set_generation", "presentation_sequence"]) {
    if (exact.identity[field] !== candidate.identity[field]) fail(`Exact/Candidate lifecycle ${field} mismatch`);
  }
}

export function timingFromFrame(frame) {
  return {
    call_ms: finite(frame.frameMs, "frame.frameMs"),
    frame_wall_ms: finite(frame.frameWallMs, "frame.frameWallMs"),
  };
}
