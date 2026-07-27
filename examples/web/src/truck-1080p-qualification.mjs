import { access, mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";

import { canonicalFormalDatasetIdentity } from "./dataset-identity.mjs";

const QUALIFICATION_NAME = "truck-quality-1080p-v1";
const TRUCK_IDENTITY = canonicalFormalDatasetIdentity("truck");
const EXACT_RASTER_PLAN = "projected_quads_exact";
const EXACT_COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1";
const EXACT_TERMINAL_MODEL = "renderer_current_stats";
const CONTROL_COMPLETE_SCHEMA = "gsplat-truck-1080p-control-complete/v1";

export const TRUCK_1080P_QUALIFICATION = Object.freeze({
  name: QUALIFICATION_NAME,
  dataset: Object.freeze({
    selector: TRUCK_IDENTITY.logical_id,
    suite_id: "truck-full",
    local_path: TRUCK_IDENTITY.source_path.slice(1),
    sha256: TRUCK_IDENTITY.sha256,
    bytes: TRUCK_IDENTITY.bytes,
    splat_count: TRUCK_IDENTITY.splat_count,
    sh_degree: TRUCK_IDENTITY.sh_degree,
  }),
  trace: Object.freeze({
    id: "candidate-truck-quality-2view-1920x1080-v1",
    local_path: "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
    url: "/tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
    file_sha256: "13081183bf2d1c6b6ec165324f53185cc30af2f304db3fefa7aa3d9044ac7c5a",
    content_sha256: "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3",
    pose_intrinsics_sha256: "f2bebe5dcee7b41ef957cf6041d3ac9a9f06e19296a08ff128947c03a78d4db3",
    camera_family: "candidate-truck-quality-2view-v1",
    width: 1920,
    height: 1080,
    frame_count: 2,
    frame_indices: Object.freeze([0, 1]),
  }),
  warmup_frames: 20,
  measured_frames: 80,
  geometry_path: "packed",
  order_backend: "adaptive",
  projected_policy: "adaptive",
  sort_interval: 1,
  renderer_path: "wasm_packed_atlas",
  control: Object.freeze({
    stage: "control",
    artifact_name: "control-current-stats",
    benchmark_window_mode: "current_stats_evidence_window",
    order_completion_protocol: "isolated_terminal",
    completion_name: ".control-stage-complete.json",
  }),
  throughput: Object.freeze({
    stage: "throughput",
    artifact_name: "throughput-terminal-queue",
    benchmark_window_mode: "terminal_queue_throughput_window",
    order_completion_protocol: "sustained_window",
    claim_name: ".throughput-stage-claimed",
  }),
  suite_name: "suite.json",
});

export const TRUCK_1080P_FIXED_GPU_PREPROJECT_COMPACT_QUALIFICATION = Object.freeze({
  ...TRUCK_1080P_QUALIFICATION,
  name: "truck-quality-1080p-fixed-gpu-preproject-compact-v1",
  order_backend: "gpu",
  projected_policy: "compact",
  gpu_order_producer: null,
});

export function truck1080pQualificationByName(name) {
  if (name === TRUCK_1080P_QUALIFICATION.name) return TRUCK_1080P_QUALIFICATION;
  if (name === TRUCK_1080P_FIXED_GPU_PREPROJECT_COMPACT_QUALIFICATION.name) {
    return TRUCK_1080P_FIXED_GPU_PREPROJECT_COMPACT_QUALIFICATION;
  }
  return null;
}

function fail(message) {
  throw new TypeError(`Truck 1080p qualification admission failed: ${message}`);
}

function requireExact(actual, expected, field) {
  if (actual !== expected) fail(`${field} must equal ${JSON.stringify(expected)}, observed ${JSON.stringify(actual)}`);
}

export function optionalEnvironmentValue(value) {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

export function validateTruck1080pCollectorConfig(
  config,
  expected = TRUCK_1080P_QUALIFICATION,
) {
  const stage = config.qualificationStage === expected.control.stage
    ? expected.control
    : config.qualificationStage === expected.throughput.stage
      ? expected.throughput
      : null;
  if (stage === null) {
    fail(
      `qualification stage must be ${expected.control.stage} or ` +
      `${expected.throughput.stage}, observed ${JSON.stringify(config.qualificationStage)}`,
    );
  }
  requireExact(config.qualificationName, expected.name, "qualification name");
  requireExact(config.dataset, expected.dataset.selector, "dataset");
  requireExact(config.geometryPath, expected.geometry_path, "geometry path");
  requireExact(config.orderBackend, expected.order_backend, "order backend");
  requireExact(config.projectedPolicy, expected.projected_policy, "projected policy");
  requireExact(config.gpuOrderProducer, expected.gpu_order_producer ?? null, "GPU order producer override");
  requireExact(config.sortInterval, expected.sort_interval, "sort interval");
  requireExact(config.benchmarkSync, false, "benchmark sync");
  requireExact(config.m4Smoke, false, "M4 smoke mode");
  requireExact(
    config.orderCompletionProtocol,
    stage.order_completion_protocol,
    "order completion protocol",
  );
  requireExact(config.benchmarkWindowMode, stage.benchmark_window_mode, "benchmark window mode");
  requireExact(config.warmup, expected.warmup_frames, "warmup frames");
  requireExact(config.frames, expected.measured_frames, "measured frames");
  requireExact(config.cameraTraceUrl, expected.trace.url, "camera trace URL");
  requireExact(config.cameraTraceSequence, true, "camera trace sequence");
  requireExact(config.cameraTraceLoops, 1, "camera trace loops");
  requireExact(config.cameraFrame, null, "fixed camera frame override");
  if (!Array.isArray(config.cameraFrameIndices)
      || config.cameraFrameIndices.length !== expected.trace.frame_indices.length
      || config.cameraFrameIndices.some(
        (value, index) => value !== expected.trace.frame_indices[index],
      )) {
    fail(`camera frame indices must equal [${expected.trace.frame_indices.join(",")}], observed ${JSON.stringify(config.cameraFrameIndices)}`);
  }
  return expected;
}

export function validateTruck1080pCleanWorkingTree(porcelain) {
  if (typeof porcelain !== "string") fail("Git porcelain status is unavailable");
  if (porcelain.trim() !== "") fail("working tree must be clean before Chrome starts");
}

export async function claimTruck1080pOutputRoot(outputRoot) {
  await mkdir(dirname(outputRoot), { recursive: true });
  try {
    await mkdir(outputRoot);
  } catch (error) {
    if (error?.code === "EEXIST") {
      fail(`output root is already claimed: ${outputRoot}`);
    }
    throw error;
  }
  return outputRoot;
}

function requireSha256(value, field) {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    fail(`${field} must be a lowercase SHA-256 digest`);
  }
}

export async function publishTruck1080pControlCompletion({
  outputRoot,
  controlManifestPath,
  controlRunId,
  configurationSha256,
  suitePath,
  expected = TRUCK_1080P_QUALIFICATION,
}) {
  const expectedManifest = resolve(
    outputRoot,
    expected.control.artifact_name,
    "manifest.json",
  );
  const expectedSuite = resolve(outputRoot, expected.suite_name);
  requireExact(controlManifestPath, expectedManifest, "control manifest path");
  requireExact(suitePath, expectedSuite, "control suite path");
  requireSha256(configurationSha256, "control configuration SHA-256");
  if (typeof controlRunId !== "string" || controlRunId.length === 0) {
    fail("control run ID must be a non-empty string");
  }
  const [manifestStatus, suiteStatus] = await Promise.all([
    stat(controlManifestPath),
    stat(suitePath),
  ]);
  if (!manifestStatus.isFile()) fail("control manifest is not a file");
  if (!suiteStatus.isFile()) fail("control full-quality suite is not a file");
  const controlManifestSha256 = createHash("sha256")
    .update(await readFile(controlManifestPath))
    .digest("hex");
  const fullQualitySuiteSha256 = createHash("sha256")
    .update(await readFile(suitePath))
    .digest("hex");
  const marker = {
    schema: CONTROL_COMPLETE_SCHEMA,
    control_artifact: expected.control.artifact_name,
    control_manifest: `${expected.control.artifact_name}/manifest.json`,
    control_manifest_sha256: controlManifestSha256,
    control_run_id: controlRunId,
    configuration_sha256: configurationSha256,
    full_quality_suite: expected.suite_name,
    full_quality_suite_sha256: fullQualitySuiteSha256,
  };
  const markerPath = resolve(outputRoot, expected.control.completion_name);
  await writeFile(markerPath, `${JSON.stringify(marker, null, 2)}\n`, { flag: "wx" });
  return Object.freeze({ markerPath, marker: Object.freeze(marker) });
}

export async function claimTruck1080pThroughputStage({
  outputRoot,
  controlManifestPath,
  expected = TRUCK_1080P_QUALIFICATION,
}) {
  const expectedManifest = resolve(
    outputRoot,
    expected.control.artifact_name,
    "manifest.json",
  );
  requireExact(controlManifestPath, expectedManifest, "throughput control manifest path");
  const markerPath = resolve(outputRoot, expected.control.completion_name);
  let marker;
  try {
    marker = JSON.parse(await readFile(markerPath, "utf8"));
  } catch (error) {
    fail(`validated control completion is unavailable: ${error.message}`);
  }
  if (marker?.schema !== CONTROL_COMPLETE_SCHEMA
      || marker.control_artifact !== expected.control.artifact_name
      || marker.control_manifest !== `${expected.control.artifact_name}/manifest.json`
      || marker.full_quality_suite !== expected.suite_name) {
    fail("validated control completion identity is invalid");
  }
  requireSha256(marker.control_manifest_sha256, "retained control manifest SHA-256");
  requireSha256(marker.configuration_sha256, "retained control configuration SHA-256");
  requireSha256(marker.full_quality_suite_sha256, "retained full-quality suite SHA-256");
  if (typeof marker.control_run_id !== "string" || marker.control_run_id.length === 0) {
    fail("retained control run ID is invalid");
  }
  const [manifestStatus, suiteStatus] = await Promise.all([
    stat(controlManifestPath),
    stat(resolve(outputRoot, expected.suite_name)),
  ]);
  if (!manifestStatus.isFile() || !suiteStatus.isFile()) {
    fail("validated control artifact or suite is not retained");
  }
  const retainedManifestSha256 = createHash("sha256")
    .update(await readFile(controlManifestPath))
    .digest("hex");
  const retainedSuiteSha256 = createHash("sha256")
    .update(await readFile(resolve(outputRoot, expected.suite_name)))
    .digest("hex");
  if (retainedManifestSha256 !== marker.control_manifest_sha256) {
    fail("retained control manifest content drifted after validation");
  }
  if (retainedSuiteSha256 !== marker.full_quality_suite_sha256) {
    fail("retained full-quality suite content drifted after validation");
  }
  try {
    await mkdir(resolve(outputRoot, expected.throughput.claim_name));
  } catch (error) {
    if (error?.code === "EEXIST") {
      fail("throughput stage is already claimed; retry is forbidden");
    }
    throw error;
  }
  return Object.freeze(marker);
}

export function validateTruck1080pExactRasterEvidence({
  manifest,
  frames,
  expected = TRUCK_1080P_QUALIFICATION,
}) {
  if (!Array.isArray(frames) || frames.length !== expected.measured_frames) {
    fail(
      `retained measured frame count must equal ${expected.measured_frames}, ` +
      `observed ${Array.isArray(frames) ? frames.length : "invalid"}`,
    );
  }
  const renderer = manifest?.renderer;
  const requiredRendererFields = {
    path: expected.renderer_path,
    backend: "webgpu",
    count_semantics: EXACT_COUNT_SEMANTICS,
    raster_execution_plan: EXACT_RASTER_PLAN,
  };
  for (const [field, value] of Object.entries(requiredRendererFields)) {
    if (renderer?.[field] !== value) {
      fail(
        `manifest renderer.${field} must equal ${value}, ` +
        `observed ${renderer?.[field] ?? "missing"}`,
      );
    }
  }
  if (manifest?.ordering_evidence?.terminal_model !== EXACT_TERMINAL_MODEL) {
    fail(
      `manifest ordering_evidence.terminal_model must equal ${EXACT_TERMINAL_MODEL}, ` +
      `observed ${manifest?.ordering_evidence?.terminal_model ?? "missing"}`,
    );
  }
  for (const [index, frame] of frames.entries()) {
    if (frame?.raster_execution_plan !== EXACT_RASTER_PLAN) {
      fail(
        `retained measured frame ${index} raster_execution_plan must equal ` +
        `${EXACT_RASTER_PLAN}, observed ${frame?.raster_execution_plan ?? "missing"}`,
      );
    }
  }
  return Object.freeze({
    frame_count: frames.length,
    raster_execution_plan: EXACT_RASTER_PLAN,
    terminal_model: EXACT_TERMINAL_MODEL,
  });
}

export function buildTruck1080pFullQualitySuite({
  manifest,
  frames,
  imageSha256,
  expected = TRUCK_1080P_QUALIFICATION,
}) {
  validateTruck1080pExactRasterEvidence({ manifest, frames, expected });
  return {
    schema: "gsplat-full-quality-experiment/v1",
    suite_id: `q1-webgpu-truck-1080p-${manifest.build.repository_commit.slice(0, 12)}`,
    status: "complete",
    pre_run_requirements: [],
    renderer_path: expected.renderer_path,
    build: {
      repository_commit: manifest.build.repository_commit,
      working_tree_dirty: manifest.build.dirty,
    },
    quality_contract: {
      blend_mode: "sorted_alpha",
      source_membership: "all",
      sampling: "disabled",
      lod: "disabled",
      sh_degree: "source",
      resolution_scale: 1.0,
      capacity_failure: "reject_before_publish",
    },
    traces: [{
      id: expected.trace.id,
      local_path: expected.trace.local_path,
      sha256: expected.trace.content_sha256,
      width: expected.trace.width,
      height: expected.trace.height,
      frame_count: expected.trace.frame_count,
      evidence_class: "formal_full_quality",
      dataset_id: expected.dataset.suite_id,
      camera_family: expected.trace.camera_family,
      pose_intrinsics_sha256: expected.trace.pose_intrinsics_sha256,
    }],
    datasets: [{
      id: expected.dataset.suite_id,
      role: "full_scene",
      local_path: expected.dataset.local_path,
      sha256: expected.dataset.sha256,
      bytes: expected.dataset.bytes,
      splat_count: expected.dataset.splat_count,
      sh_degree: expected.dataset.sh_degree,
    }],
    endpoints: [{
      id: "chrome-m4-webgpu",
      availability: "available",
      artifact_platform: "web",
      execution_class: "browser",
      performance_evidence: false,
      formal_display: {
        width: expected.trace.width,
        height: expected.trace.height,
        source: "trace_display_exact",
      },
    }],
    protocols: [{
      id: "q1-webgpu-truck-1080p-prerequisite",
      evidence_class: "formal_full_quality",
      dataset_ids: [expected.dataset.suite_id],
      endpoint_ids: ["chrome-m4-webgpu"],
      sort_policies: [expected.order_backend],
      repetitions: 1,
      warmup_frames: expected.warmup_frames,
      measured_frames: expected.measured_frames,
      sort_interval: expected.sort_interval,
      randomization_seed: 0,
      randomize_policy_order: false,
      sort_refresh: "every_camera_revision",
      require_image: true,
      display: { width: expected.trace.width, height: expected.trace.height },
      camera: {
        mode: "trace_sequence",
        trace_id: expected.trace.id,
        frame_indices: [...expected.trace.frame_indices],
        require_display_match: true,
        display_policy: "trace_display_exact",
        quality_comparable: true,
      },
    }],
    capacity_rejections: [],
    runs: [{
      protocol_id: "q1-webgpu-truck-1080p-prerequisite",
      dataset_id: expected.dataset.suite_id,
      endpoint_id: "chrome-m4-webgpu",
      sort_policy: expected.order_backend,
      camera_case: "sequence",
      repetition: 1,
      schedule_index: 1,
      policy_position: 1,
      artifact: expected.control.artifact_name,
      image: {
        path: `${expected.control.artifact_name}/final-frame.png`,
        sha256: imageSha256,
        width: expected.trace.width,
        height: expected.trace.height,
      },
    }],
  };
}

export async function publishValidatedTruck1080pSuite({
  suitePath,
  manifest,
  frames,
  imagePath,
  imageSha256,
  validate,
  expected = TRUCK_1080P_QUALIFICATION,
}) {
  validateTruck1080pExactRasterEvidence({ manifest, frames, expected });
  const outputRoot = dirname(suitePath);
  const artifactPath = resolve(outputRoot, expected.control.artifact_name);
  const expectedImagePath = resolve(artifactPath, "final-frame.png");
  if (imagePath !== expectedImagePath) {
    fail(`final image must be ${expectedImagePath}, observed ${imagePath}`);
  }
  const [artifactStatus, imageStatus] = await Promise.all([
    stat(artifactPath),
    stat(imagePath),
  ]);
  if (!artifactStatus.isDirectory()) fail(`${artifactPath} is not a published artifact directory`);
  if (!imageStatus.isFile()) fail(`${imagePath} is not a retained PNG file`);
  try {
    await access(suitePath);
    fail(`suite destination already exists: ${suitePath}`);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }

  const suite = buildTruck1080pFullQualitySuite({ manifest, frames, imageSha256, expected });
  const staging = resolve(outputRoot, ".suite.json.staging");
  await writeFile(staging, `${JSON.stringify(suite, null, 2)}\n`, { flag: "wx" });
  await validate(staging);
  await rename(staging, suitePath);
  return suitePath;
}
