export const DEPTH_PRECISION_DIAGNOSTIC_SCHEMA =
  "gsplat-web-depth-precision-diagnostic/v1";
export const DEPTH_PRECISION_CONSOLE_PREFIX =
  "GSPLAT_DIAGNOSTIC_PRESENTED_DEPTH_PRECISION ";

const RECEIPT_INTEGER_FIELDS = Object.freeze([
  "scene_generation",
  "camera_revision",
  "viewport_generation",
  "contract_generation",
  "plan_set_generation",
  "order_generation",
  "presentation_sequence",
]);
const STABLE_GENERATION_FIELDS = Object.freeze([
  "scene_generation",
  "viewport_generation",
  "contract_generation",
  "plan_set_generation",
]);

function fail(message) {
  throw new Error(`Candidate20 diagnostic rejected: ${message}`);
}

function requireObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
  return value;
}

function requireSafeInteger(value, label, { positive = false } = {}) {
  if (!Number.isSafeInteger(value) || value < (positive ? 1 : 0)) {
    fail(`${label} must be a ${positive ? "positive" : "non-negative"} safe integer`);
  }
  return value;
}

function requireLiteral(value, expected, label) {
  if (value !== expected) fail(`${label} must equal ${JSON.stringify(expected)}`);
}

export function parsePresentedDepthPrecisionConsoleLine(line) {
  if (typeof line !== "string") fail("console line must be a string");
  const offset = line.indexOf(DEPTH_PRECISION_CONSOLE_PREFIX);
  if (offset < 0) return null;
  const payload = line.slice(offset + DEPTH_PRECISION_CONSOLE_PREFIX.length);
  try {
    return JSON.parse(payload);
  } catch (error) {
    fail(`console receipt is not JSON: ${error.message}`);
  }
}

export function validateDepthPrecisionDiagnostic({ frames, receipts, runtime }) {
  if (!Array.isArray(frames) || frames.length < 3) {
    fail("at least three successful presented frames are required");
  }
  if (!Array.isArray(receipts) || receipts.length !== frames.length) {
    fail("each successful presented frame requires exactly one console receipt");
  }
  requireLiteral(requireObject(runtime, "runtime").raster_path, "packed_atlas", "runtime.raster_path");

  let previousPresentation = 0;
  let previousOrderGeneration = 0;
  let stableGenerations = null;
  const normalizedFrames = [];
  const normalizedReceipts = [];
  for (let index = 0; index < frames.length; index += 1) {
    const frame = requireObject(frames[index], `frames[${index}]`);
    const receipt = requireObject(receipts[index], `receipts[${index}]`);
    requireLiteral(frame.frame_presented, true, `frames[${index}].frame_presented`);
    requireLiteral(frame.order_backend, "gpu", `frames[${index}].order_backend`);
    requireLiteral(frame.projected_execution, "compact", `frames[${index}].projected_execution`);
    requireLiteral(frame.gpu_order_producer, "preproject", `frames[${index}].gpu_order_producer`);
    requireLiteral(frame.raster_execution_plan, "projected_quads_exact", `frames[${index}].raster_execution_plan`);
    const cameraRevision = requireSafeInteger(
      frame.camera_revision,
      `frames[${index}].camera_revision`,
    );
    const appliedOrderRevision = requireSafeInteger(
      frame.applied_order_revision,
      `frames[${index}].applied_order_revision`,
    );
    requireSafeInteger(
      frame.presented_order_revision_lag,
      `frames[${index}].presented_order_revision_lag`,
    );
    if (appliedOrderRevision !== cameraRevision || frame.presented_order_revision_lag !== 0) {
      fail(`frames[${index}] does not present the current camera order`);
    }

    requireLiteral(
      receipt.record_type,
      "presented_depth_precision",
      `receipts[${index}].record_type`,
    );
    requireLiteral(
      receipt.depth_precision_profile,
      "CandidateStable20",
      `receipts[${index}].depth_precision_profile`,
    );
    requireLiteral(receipt.plan_id, "GpuPreproject", `receipts[${index}].plan_id`);
    for (const field of RECEIPT_INTEGER_FIELDS) {
      requireSafeInteger(receipt[field], `receipts[${index}].${field}`, {
        positive: field === "presentation_sequence",
      });
    }
    if (stableGenerations === null) {
      stableGenerations = Object.fromEntries(
        STABLE_GENERATION_FIELDS.map((field) => [field, receipt[field]]),
      );
    } else {
      for (const field of STABLE_GENERATION_FIELDS) {
        if (receipt[field] !== stableGenerations[field]) {
          fail(`${field} must remain stable across the diagnostic session`);
        }
      }
    }
    if (receipt.camera_revision !== cameraRevision) {
      fail(`receipts[${index}].camera_revision does not match its presented frame`);
    }
    if (receipt.presentation_sequence <= previousPresentation) {
      fail("presentation_sequence must be unique and strictly increasing");
    }
    if (receipt.order_generation < previousOrderGeneration) {
      fail("order_generation must not move backwards");
    }
    previousPresentation = receipt.presentation_sequence;
    previousOrderGeneration = receipt.order_generation;
    normalizedFrames.push({ ...frame });
    normalizedReceipts.push({ ...receipt });
  }

  return Object.freeze({
    frames: Object.freeze(normalizedFrames),
    receipts: Object.freeze(normalizedReceipts),
  });
}

export function createDepthPrecisionDiagnosticArtifact({
  identity,
  inputs,
  runtime,
  frames,
  receipts,
}) {
  const validated = validateDepthPrecisionDiagnostic({ frames, receipts, runtime });
  return {
    schema: DEPTH_PRECISION_DIAGNOSTIC_SCHEMA,
    evidence_class: "diagnostic",
    full_quality_eligible: false,
    validator: "diagnostic_only",
    identity: requireObject(identity, "identity"),
    inputs: requireObject(inputs, "inputs"),
    configuration: {
      geometry_path: "packed",
      depth_precision_profile: "CandidateStable20",
      order_backend: "gpu",
      projected_policy: "compact",
      gpu_order_producer: "preproject",
      successful_present_count: validated.frames.length,
    },
    runtime: requireObject(runtime, "runtime"),
    frames: validated.frames,
    presented_depth_precision_receipts: validated.receipts,
  };
}
