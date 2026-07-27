import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

import {
  parseArguments,
  rgba8Png,
  writeSuite,
} from "../scripts/collect-web-depth-precision-quality.mjs";
import {
  WEB_DEPTH_QUALITY,
  cameraValues,
  computeFrameMetrics,
  computeTemporalMetric,
  normalizeAndValidateCapture,
  validateMatchedPair,
} from "../src/depth-precision-quality.mjs";

function identity(profile) {
  return {
    sceneGeneration: 1,
    cameraRevision: 2,
    viewportGeneration: 1,
    contractGeneration: 1,
    planSetGeneration: 1,
    planId: "GpuPreproject",
    orderGeneration: 2,
    presentationSequence: 2,
    width: WEB_DEPTH_QUALITY.width,
    height: WEB_DEPTH_QUALITY.height,
    rgba8Sha256: "0".repeat(64),
    profile,
  };
}

function capture(profile) {
  const rgba8 = new Uint8Array(WEB_DEPTH_QUALITY.width * WEB_DEPTH_QUALITY.height * 4);
  const common = {
    ...identity(profile),
    rgba8Sha256: createHash("sha256").update(rgba8).digest("hex"),
  };
  return {
    rgba8,
    identity: common,
    depthPrecision: common,
    projectedCachePrecision: { ...common, profile: "ExactAxes32", axisRecordBytes: 16 },
    residentSh: {
      ...common,
      profile: "ExactSigned11BandScale5",
      mantissaBits: 11,
      symmetricMaxCode: 1023,
      pointScaleBits: 5,
      pointScaleMaxCode: 31,
      rangeChunkSplats: 256,
      sourceCount: 279199,
      encodedCount: 279199,
      residentCount: 279199,
      addressableCount: 279199,
      sourceShDegree: 3,
      residentShDegree: 3,
      residualCoefficientsPerSource: 45,
      planeCount: 4,
      bytesPerSource: 64,
    },
  };
}

const frame = {
  framePresented: true,
  orderBackend: "gpu",
  projectedPolicy: "compact",
  projectedExecution: "compact",
  rasterExecutionPlan: "projected_quads_exact",
  gpuOrderProducer: "preproject",
  gpuSortFallback: false,
  cameraRevision: 2,
  frameMs: 1,
  frameWallMs: 1,
};

const terminal = {
  status: "ready",
  ticket: 2,
  plan: "gpu_preproject",
  countSemantics: "indirect_draw_equals_contributor",
  sceneGeneration: 1,
  cameraRevision: 2,
  viewportGeneration: 1,
  contractGeneration: 1,
  planSetGeneration: 1,
  orderGeneration: 2,
  presentationSequence: 2,
  sourceCount: 279199,
  visibleCount: 200000,
  contributorCount: 150000,
  drawnCount: 150000,
};

const loadReceipt = {
  sourceCount: 279199,
  decodedCount: 279199,
  encodedCount: 279199,
  residentCount: 279199,
  addressableCount: 279199,
  sourceShDegree: 3,
  residentShDegree: 3,
  fullQuality: true,
  sourceMembership: "all",
  samplingEnabled: false,
  lodEnabled: false,
  partialScenePublished: false,
};

test("collector requires separate Exact/Candidate package inputs and a fresh output", () => {
  assert.deepEqual(parseArguments([
    "--exact-pkg", "exact", "--candidate-pkg", "candidate", "--chrome", "chrome", "--output", "out",
  ]), {
    exactPkg: resolve("exact"),
    candidatePkg: resolve("candidate"),
    chrome: resolve("chrome"),
    output: resolve("out"),
  });
  assert.throws(() => parseArguments(["--exact-pkg", "only"]), /usage:/);
});

test("camera trace pose and intrinsics map to the ten-value native contract", () => {
  const values = cameraValues({
    pose: { position: [1, 2, 3], rotation_xyzw: [0, 0, 0, 1] },
    intrinsics: { vertical_fov_radians: 0.5, near_plane: 0.1, far_plane: 100 },
  });
  assert.deepEqual(Array.from(values), [1, 2, 3, 0, 0, 0, 1, 0.5, 0.10000000149011612, 100]);
});

test("RGBA8 PNG encoder emits canonical non-interlaced color type six", () => {
  const png = rgba8Png(2, 1, Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]));
  assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  assert.equal(png.readUInt32BE(16), 2);
  assert.equal(png.readUInt32BE(20), 1);
  assert.deepEqual(Array.from(png.subarray(24, 29)), [8, 6, 0, 0, 0]);
});

test("frame and temporal metrics reproduce identity exactly", () => {
  const pixels = new Uint8Array([10, 20, 30, 255, 40, 50, 60, 200]);
  assert.deepEqual(computeFrameMetrics(pixels, pixels, 2, 1), {
    ssim_luma_srgb_window8: 1,
    rgb_mae_normalized: 0,
    rgb_bad_pixel_fraction_over_3: 0,
    alpha_mae_normalized: 0,
    alpha_bad_pixel_fraction_over_1: 0,
  });
  assert.equal(computeTemporalMetric(pixels, pixels, pixels, pixels), 0);
});

test("renderer-owned receipt rejects wrong profile before publication", async () => {
  const invalid = capture("CandidateStable20");
  await assert.rejects(() => normalizeAndValidateCapture({
    lane: "exact",
    captureIndex: 0,
    capture: invalid,
    frame,
    terminal,
    loadReceipt,
    dataset: { splat_count: 279199, sh_degree: 3 },
  }), /changed more than the depth-key profile/);
});

test("matched pair fails closed on plan or lifecycle drift", () => {
  const exact = { identity: { plan_id: "GpuPreproject", order_generation: 2,
    scene_generation: 1, camera_revision: 2, viewport_generation: 1,
    contract_generation: 1, plan_set_generation: 1, presentation_sequence: 2 } };
  const candidate = structuredClone(exact);
  validateMatchedPair(exact, candidate);
  candidate.identity.order_generation = 3;
  assert.throws(() => validateMatchedPair(exact, candidate), /order_generation mismatch/);
});

function normalizedCapture(lane, captureIndex, rgbaHash) {
  const identity = {
    scene_generation: 1,
    camera_revision: captureIndex + 1,
    viewport_generation: 1,
    contract_generation: 1,
    plan_set_generation: 1,
    plan_id: "GpuPreproject",
    order_generation: captureIndex + 1,
    presentation_sequence: captureIndex + 1,
    width: WEB_DEPTH_QUALITY.width,
    height: WEB_DEPTH_QUALITY.height,
    rgba8_sha256: rgbaHash,
  };
  return {
    identity,
    depth_precision: {
      ...identity,
      profile: lane === "exact" ? "ExactFull32" : "CandidateStable20",
    },
    projected_cache_precision: { ...identity, profile: "ExactAxes32", axis_record_bytes: 16 },
    resident_sh: {
      ...identity,
      codec_profile: "ExactSigned11BandScale5",
      mantissa_bits: 11,
      symmetric_max_code: 1023,
      point_scale_bits: 5,
      point_scale_max_code: 31,
      range_chunk_splats: 256,
      source_count: 279199,
      encoded_count: 279199,
      resident_count: 279199,
      addressable_count: 279199,
      source_sh_degree: 3,
      resident_sh_degree: 3,
      residual_coefficients_per_source: 45,
      plane_count: 4,
      bytes_per_source: 64,
    },
    frame: { frameMs: 1, frameWallMs: 1 },
    counts: {
      ticket: captureIndex + 1,
      visibleCount: 200000,
      contributorCount: 150000,
      drawnCount: 150000,
    },
  };
}

test("synthetic renderer-owned receipts produce a canonical Balanced artifact", async () => {
  const root = resolve(import.meta.dirname, "../../..");
  const datasetPath = resolve(root, "tests/perf/datasets/kitsune.json");
  const tracePath = resolve(
    root,
    "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json",
  );
  const [datasetBytes, traceBytes] = await Promise.all([readFile(datasetPath), readFile(tracePath)]);
  const dataset = JSON.parse(datasetBytes);
  const trace = JSON.parse(traceBytes);
  const rgba = Buffer.alloc(WEB_DEPTH_QUALITY.width * WEB_DEPTH_QUALITY.height * 4);
  const rgbaHash = createHash("sha256").update(rgba).digest("hex");
  const uploaded = new Map();
  const outcome = { lanes: { exact: { captures: [] }, candidate: { captures: [] } } };
  for (const lane of ["exact", "candidate"]) {
    for (let index = 0; index < 3; index += 1) {
      uploaded.set(`${lane}:${index}`, rgba);
      outcome.lanes[lane].captures.push(normalizedCapture(lane, index, rgbaHash));
    }
  }
  const output = await mkdtemp(resolve(tmpdir(), "gsplat-web-depth-quality-"));
  try {
    const suite = await writeSuite({
      output,
      outcome,
      uploaded,
      commit: "1".repeat(40),
      dataset,
      trace,
      inputs: {
        dataset_manifest_sha256: createHash("sha256").update(datasetBytes).digest("hex"),
        trace_file_sha256: createHash("sha256").update(traceBytes).digest("hex"),
      },
      browserReceipt: { platform: "fixture", userAgent: "fixture" },
    });
    assert.equal(resolve(suite), resolve(output, "suite.json"));
  } finally {
    await rm(output, { recursive: true, force: true });
  }
});
