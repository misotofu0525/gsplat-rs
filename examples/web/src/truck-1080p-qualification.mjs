import { access, rename, stat, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

import { canonicalFormalDatasetIdentity } from "./dataset-identity.mjs";

const QUALIFICATION_NAME = "truck-quality-1080p-v1";
const TRUCK_IDENTITY = canonicalFormalDatasetIdentity("truck");

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
  order_completion_protocol: "sustained_window",
  renderer_path: "wasm_packed_atlas",
  artifact_name: "run-adaptive",
  suite_name: "suite.json",
});

function fail(message) {
  throw new TypeError(`Truck 1080p qualification admission failed: ${message}`);
}

function requireExact(actual, expected, field) {
  if (actual !== expected) fail(`${field} must equal ${JSON.stringify(expected)}, observed ${JSON.stringify(actual)}`);
}

export function validateTruck1080pCollectorConfig(config) {
  const expected = TRUCK_1080P_QUALIFICATION;
  requireExact(config.qualificationName, expected.name, "qualification name");
  requireExact(config.dataset, expected.dataset.selector, "dataset");
  requireExact(config.geometryPath, expected.geometry_path, "geometry path");
  requireExact(config.orderBackend, expected.order_backend, "order backend");
  requireExact(config.projectedPolicy, expected.projected_policy, "projected policy");
  requireExact(config.gpuOrderProducer, null, "GPU order producer override");
  requireExact(config.sortInterval, expected.sort_interval, "sort interval");
  requireExact(config.benchmarkSync, false, "benchmark sync");
  requireExact(config.m4Smoke, false, "M4 smoke mode");
  requireExact(
    config.orderCompletionProtocol,
    expected.order_completion_protocol,
    "order completion protocol",
  );
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

export function buildTruck1080pFullQualitySuite({ manifest, imageSha256 }) {
  const expected = TRUCK_1080P_QUALIFICATION;
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
      artifact: expected.artifact_name,
      image: {
        path: `${expected.artifact_name}/final-frame.png`,
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
  imagePath,
  imageSha256,
  validate,
}) {
  const expected = TRUCK_1080P_QUALIFICATION;
  const outputRoot = dirname(suitePath);
  const artifactPath = resolve(outputRoot, expected.artifact_name);
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

  const suite = buildTruck1080pFullQualitySuite({ manifest, imageSha256 });
  const staging = resolve(outputRoot, ".suite.json.staging");
  await writeFile(staging, `${JSON.stringify(suite, null, 2)}\n`, { flag: "wx" });
  await validate(staging);
  await rename(staging, suitePath);
  return suitePath;
}
