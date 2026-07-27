import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { access, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  TRUCK_1080P_QUALIFICATION,
  TRUCK_1080P_FIXED_GPU_PREPROJECT_COMPACT_QUALIFICATION,
  buildTruck1080pFullQualitySuite,
  claimTruck1080pOutputRoot,
  claimTruck1080pThroughputStage,
  optionalEnvironmentValue,
  publishTruck1080pControlCompletion,
  publishValidatedTruck1080pSuite,
  validateTruck1080pCleanWorkingTree,
  validateTruck1080pCollectorConfig,
  validateTruck1080pExactRasterEvidence,
  truck1080pQualificationByName,
} from "../src/truck-1080p-qualification.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

function validConfig(qualificationStage = "control") {
  const expected = TRUCK_1080P_QUALIFICATION;
  const stage = qualificationStage === expected.control.stage
    ? expected.control
    : expected.throughput;
  return {
    qualificationStage,
    qualificationName: expected.name,
    dataset: expected.dataset.selector,
    geometryPath: expected.geometry_path,
    orderBackend: expected.order_backend,
    projectedPolicy: expected.projected_policy,
    gpuOrderProducer: null,
    sortInterval: expected.sort_interval,
    benchmarkSync: false,
    m4Smoke: false,
    orderCompletionProtocol: stage.order_completion_protocol,
    benchmarkWindowMode: stage.benchmark_window_mode,
    warmup: expected.warmup_frames,
    frames: expected.measured_frames,
    cameraTraceUrl: expected.trace.url,
    cameraTraceSequence: true,
    cameraTraceLoops: 1,
    cameraFrame: null,
    cameraFrameIndices: [...expected.trace.frame_indices],
  };
}

test("optional fixed camera environment treats an explicit empty override as absent", () => {
  assert.equal(optionalEnvironmentValue(undefined), null);
  assert.equal(optionalEnvironmentValue(""), null);
  assert.equal(optionalEnvironmentValue("  "), null);
  assert.equal(optionalEnvironmentValue("0"), "0");
});

test("fixed GPU preproject Compact Truck cell is a separate frozen tuple", () => {
  const expected = TRUCK_1080P_FIXED_GPU_PREPROJECT_COMPACT_QUALIFICATION;
  const config = {
    ...validConfig(),
    qualificationName: expected.name,
    orderBackend: "gpu",
    projectedPolicy: "compact",
    gpuOrderProducer: null,
  };
  assert.equal(truck1080pQualificationByName(expected.name), expected);
  assert.equal(validateTruck1080pCollectorConfig(config, expected), expected);
  assert.throws(
    () => validateTruck1080pCollectorConfig({ ...config, gpuOrderProducer: "post-sort" }, expected),
    /GPU order producer override/,
  );
});

test("Truck 1080p collector admission freezes the full formal configuration", () => {
  assert.equal(
    validateTruck1080pCollectorConfig(validConfig()),
    TRUCK_1080P_QUALIFICATION,
  );
  assert.equal(
    validateTruck1080pCollectorConfig(validConfig("throughput")),
    TRUCK_1080P_QUALIFICATION,
  );

  const mutations = [
    ["dataset", "truck-2000000"],
    ["geometryPath", "paged"],
    ["orderBackend", "gpu"],
    ["projectedPolicy", "compact"],
    ["gpuOrderProducer", "preproject"],
    ["sortInterval", 2],
    ["benchmarkSync", true],
    ["m4Smoke", true],
    ["orderCompletionProtocol", "sustained_window"],
    ["benchmarkWindowMode", "terminal_queue_throughput_window"],
    ["qualificationStage", "missing"],
    ["warmup", 19],
    ["frames", 79],
    ["cameraTraceUrl", "/different.json"],
    ["cameraTraceSequence", false],
    ["cameraTraceLoops", 2],
    ["cameraFrame", 0],
    ["cameraFrameIndices", [0]],
  ];
  for (const [field, value] of mutations) {
    assert.throws(
      () => validateTruck1080pCollectorConfig({ ...validConfig(), [field]: value }),
      /Truck 1080p qualification admission failed/,
      field,
    );
  }
});

test("Truck control completion binds one atomically claimed throughput stage", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "gsplat-truck-two-stage-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const outputRoot = join(parent, "attempt-1");
  await claimTruck1080pOutputRoot(outputRoot);
  const controlArtifact = join(
    outputRoot,
    TRUCK_1080P_QUALIFICATION.control.artifact_name,
  );
  await mkdir(controlArtifact);
  const controlManifestPath = join(controlArtifact, "manifest.json");
  const suitePath = join(outputRoot, TRUCK_1080P_QUALIFICATION.suite_name);
  await writeFile(controlManifestPath, "{}\n");
  await writeFile(suitePath, "{}\n");
  const completion = await publishTruck1080pControlCompletion({
    outputRoot,
    controlManifestPath,
    controlRunId: "control-run",
    configurationSha256: "b".repeat(64),
    suitePath,
  });
  assert.equal(completion.marker.control_manifest, "control-current-stats/manifest.json");
  assert.equal(
    completion.marker.control_manifest_sha256,
    createHash("sha256").update("{}\n").digest("hex"),
  );

  const claims = await Promise.allSettled([
    claimTruck1080pThroughputStage({ outputRoot, controlManifestPath }),
    claimTruck1080pThroughputStage({ outputRoot, controlManifestPath }),
  ]);
  assert.equal(claims.filter((result) => result.status === "fulfilled").length, 1);
  assert.equal(claims.filter((result) => result.status === "rejected").length, 1);
  assert.match(
    claims.find((result) => result.status === "rejected").reason.message,
    /throughput stage is already claimed; retry is forbidden/,
  );
});

test("Truck throughput cannot start before a validated control completion", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "gsplat-truck-no-control-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const outputRoot = join(parent, "attempt-1");
  await claimTruck1080pOutputRoot(outputRoot);
  const controlArtifact = join(
    outputRoot,
    TRUCK_1080P_QUALIFICATION.control.artifact_name,
  );
  await mkdir(controlArtifact);
  const controlManifestPath = join(controlArtifact, "manifest.json");
  await writeFile(controlManifestPath, "{}\n");
  await assert.rejects(
    claimTruck1080pThroughputStage({ outputRoot, controlManifestPath }),
    /validated control completion is unavailable/,
  );
});

test("Truck throughput rejects a control manifest changed after validation", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "gsplat-truck-control-drift-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const outputRoot = join(parent, "attempt-1");
  await claimTruck1080pOutputRoot(outputRoot);
  const controlArtifact = join(
    outputRoot,
    TRUCK_1080P_QUALIFICATION.control.artifact_name,
  );
  await mkdir(controlArtifact);
  const controlManifestPath = join(controlArtifact, "manifest.json");
  const suitePath = join(outputRoot, TRUCK_1080P_QUALIFICATION.suite_name);
  await writeFile(controlManifestPath, "{}\n");
  await writeFile(suitePath, "{}\n");
  await publishTruck1080pControlCompletion({
    outputRoot,
    controlManifestPath,
    controlRunId: "control-run",
    configurationSha256: "b".repeat(64),
    suitePath,
  });
  await writeFile(controlManifestPath, '{"changed":true}\n');
  await assert.rejects(
    claimTruck1080pThroughputStage({ outputRoot, controlManifestPath }),
    /control manifest content drifted after validation/,
  );
});

test("Truck 1080p collector admission rejects a dirty working tree", () => {
  assert.doesNotThrow(() => validateTruck1080pCleanWorkingTree(""));
  assert.throws(
    () => validateTruck1080pCleanWorkingTree(" M examples/web/src/main.js\n"),
    /working tree must be clean before Chrome starts/,
  );
});

test("Truck 1080p output root has exactly one atomic claimant", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "gsplat-truck-claim-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const outputRoot = join(parent, "attempt-1");
  const results = await Promise.allSettled([
    claimTruck1080pOutputRoot(outputRoot),
    claimTruck1080pOutputRoot(outputRoot),
  ]);
  assert.equal(results.filter((result) => result.status === "fulfilled").length, 1);
  assert.equal(results.filter((result) => result.status === "rejected").length, 1);
  assert.match(
    results.find((result) => result.status === "rejected").reason.message,
    /output root is already claimed/,
  );
  assert.equal((await stat(outputRoot)).isDirectory(), true);
});

test("Truck 1080p suite is a single non-performance full-quality prerequisite cell", () => {
  const expected = TRUCK_1080P_QUALIFICATION;
  const suite = buildTruck1080pFullQualitySuite({
    manifest: {
      build: {
        repository_commit: "a".repeat(40),
        dirty: false,
      },
      renderer: {
        path: expected.renderer_path,
        backend: "webgpu",
        count_semantics: "candidate_visible_contributor_issued_v1",
        raster_execution_plan: "projected_quads_exact",
      },
      ordering_evidence: {
        terminal_model: "renderer_current_stats",
      },
    },
    frames: Array.from({ length: expected.measured_frames }, () => ({
      raster_execution_plan: "projected_quads_exact",
    })),
    imageSha256: "b".repeat(64),
  });

  assert.equal(suite.status, "complete");
  assert.deepEqual(suite.quality_contract, {
    blend_mode: "sorted_alpha",
    source_membership: "all",
    sampling: "disabled",
    lod: "disabled",
    sh_degree: "source",
    resolution_scale: 1.0,
    capacity_failure: "reject_before_publish",
  });
  assert.deepEqual(suite.datasets, [{
    id: "truck-full",
    role: "full_scene",
    local_path: "tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
    sha256: "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c",
    bytes: 630225580,
    splat_count: 2541226,
    sh_degree: 3,
  }]);
  assert.equal(suite.endpoints[0].performance_evidence, false);
  assert.deepEqual(suite.protocols[0].display, { width: 1920, height: 1080 });
  assert.deepEqual(suite.protocols[0].camera.frame_indices, [0, 1]);
  assert.equal(suite.protocols[0].warmup_frames, 20);
  assert.equal(suite.protocols[0].measured_frames, 80);
  assert.equal(suite.protocols[0].sort_policies[0], "adaptive");
  assert.equal(suite.runs[0].artifact, "control-current-stats");
});

function constantDistribution(count, value) {
  return {
    count,
    mean: value,
    p50: value,
    p90: value,
    p95: value,
    p99: value,
    max: value,
  };
}

async function installTemporaryTruckArtifact(root) {
  const expected = TRUCK_1080P_QUALIFICATION;
  const artifact = join(root, expected.control.artifact_name);
  await mkdir(artifact, { recursive: true });
  const runId = "truck-1080p-validator-fixture";
  const commit = "a".repeat(40);
  const manifest = {
    schema: "gsplat-benchmark/v1",
    record_type: "manifest",
    run_id: runId,
    identity: {
      series_id: "web-camera-trace-sequence-v1",
      started_at_utc: "2026-07-27T00:00:00Z",
      ended_at_utc: "2026-07-27T00:00:01Z",
      measurement_started_at_utc: "2026-07-27T00:00:00.100Z",
      measurement_ended_at_utc: "2026-07-27T00:00:00.900Z",
    },
    build: {
      repository_commit: commit,
      dirty: false,
      profile: "browser",
      package_version: "0.1.3",
    },
    dataset: {
      id: "truck.ply",
      logical_id: expected.dataset.selector,
      source_path: `/${expected.dataset.local_path}`,
      sha256: expected.dataset.sha256,
      bytes: expected.dataset.bytes,
      splat_count: expected.dataset.splat_count,
      sh_degree: expected.dataset.sh_degree,
    },
    exactness: {
      source_splat_count: expected.dataset.splat_count,
      decoded_splat_count: expected.dataset.splat_count,
      encoded_splat_count: expected.dataset.splat_count,
      resident_splat_count: expected.dataset.splat_count,
      addressable_splat_count: expected.dataset.splat_count,
      source_sh_degree: expected.dataset.sh_degree,
      resident_sh_degree: expected.dataset.sh_degree,
      source_membership: "all",
      sampling: "disabled",
      lod: "disabled",
      sh_degree_policy: "source",
      partial_scene_published: false,
      full_quality: true,
    },
    trace: {
      id: expected.trace.id,
      sha256: expected.trace.content_sha256,
      reference_width: expected.trace.width,
      reference_height: expected.trace.height,
      require_display_match: true,
      display_policy: "trace_display_exact",
      quality_comparable: true,
      frame_indices: [...expected.trace.frame_indices],
    },
    renderer: {
      implementation: "gsplat-rs",
      path: expected.renderer_path,
      backend: "webgpu",
      sort_policy: "interval_1",
      order_backend_requested: expected.order_backend,
      sort_interval: expected.sort_interval,
      count_semantics: "candidate_visible_contributor_issued_v1",
      raster_execution_plan: "projected_quads_exact",
    },
    ordering_evidence: {
      terminal_model: "renderer_current_stats",
    },
    display: {
      width: expected.trace.width,
      height: expected.trace.height,
      dpr: 1,
      refresh_hz: 60,
      frame_budget_ms: 16.666667,
      refresh_hz_source: "configured",
      frame_budget_source: "configured",
    },
    resolution: {
      requested_width: expected.trace.width,
      requested_height: expected.trace.height,
      surface_width: expected.trace.width,
      surface_height: expected.trace.height,
      internal_render_width: expected.trace.width,
      internal_render_height: expected.trace.height,
      presented_width: expected.trace.width,
      presented_height: expected.trace.height,
      dynamic_resolution: "disabled",
      upscaling: "disabled",
      full_resolution: true,
    },
    environment: {
      platform: "web",
      os: "fixture-os",
      device: "fixture-device",
      browser: "fixture-chrome",
      adapter: "fixture-webgpu-adapter",
      driver: "fixture-driver",
    },
    unavailable_fields: [
      "frames[*].gpu_wait_ms",
      "frames[*].gpu_complete_ms",
    ],
  };
  const frames = Array.from({ length: expected.measured_frames }, (_, frameIndex) => ({
    schema: "gsplat-benchmark/v1",
    record_type: "frame",
    run_id: runId,
    frame_index: frameIndex,
    elapsed_ns: (frameIndex + 1) * 1_000_000,
    call_ms: 1,
    frame_wall_ms: 1,
    preprocess_ms: 0.1,
    sort_ms: 0.2,
    geometry_submit_ms: 0.3,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: expected.dataset.splat_count,
    contributor: expected.dataset.splat_count,
    drawn: expected.dataset.splat_count,
    exact_contributor_compaction: true,
    sort_refreshed: true,
    raster_execution_plan: "projected_quads_exact",
  }));
  const summary = {
    schema: "gsplat-benchmark/v1",
    record_type: "summary",
    run_id: runId,
    sample_count: expected.measured_frames,
    warmup_count: expected.warmup_frames,
    frame_budget_ms: 16.666667,
    missed_frame_count: 0,
    distributions: {
      call_ms: constantDistribution(expected.measured_frames, 1),
      frame_wall_ms: constantDistribution(expected.measured_frames, 1),
      preprocess_ms: constantDistribution(expected.measured_frames, 0.1),
      sort_ms: constantDistribution(expected.measured_frames, 0.2),
      geometry_submit_ms: constantDistribution(expected.measured_frames, 0.3),
      gpu_wait_ms: null,
      gpu_complete_ms: null,
    },
    sort_telemetry: {
      cpu_frame_count: 0,
      gpu_frame_count: expected.measured_frames,
      gpu_sort_fallback_count: 0,
    },
  };
  await writeFile(join(artifact, "manifest.json"), `${JSON.stringify(manifest)}\n`);
  await writeFile(
    join(artifact, "frames.jsonl"),
    `${frames.map((frame) => JSON.stringify(frame)).join("\n")}\n`,
  );
  await writeFile(join(artifact, "summary.json"), `${JSON.stringify(summary)}\n`);

  const png = Buffer.alloc(24);
  Buffer.from("89504e470d0a1a0a0000000d49484452", "hex").copy(png);
  png.writeUInt32BE(expected.trace.width, 16);
  png.writeUInt32BE(expected.trace.height, 20);
  const imagePath = join(artifact, "final-frame.png");
  await writeFile(imagePath, png);
  return {
    artifact,
    manifest,
    frames,
    imagePath,
    imageSha256: createHash("sha256").update(png).digest("hex"),
  };
}

function runValidator(script, ...args) {
  return spawnSync(
    process.env.PYTHON ?? "python3",
    [resolve(REPO_ROOT, script), ...args],
    { cwd: REPO_ROOT, encoding: "utf8" },
  );
}

test("Truck suite publication passes the real benchmark and full-quality validators", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "gsplat-truck-suite-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const fixture = await installTemporaryTruckArtifact(root);
  const benchmark = runValidator(
    "tests/perf/validate-benchmark-artifacts.py",
    fixture.artifact,
  );
  assert.equal(benchmark.status, 0, benchmark.stderr || benchmark.stdout);
  assert.equal(
    validateTruck1080pExactRasterEvidence({
      manifest: fixture.manifest,
      frames: fixture.frames,
    }).frame_count,
    TRUCK_1080P_QUALIFICATION.measured_frames,
  );

  const suitePath = join(root, TRUCK_1080P_QUALIFICATION.suite_name);
  await publishValidatedTruck1080pSuite({
    suitePath,
    manifest: fixture.manifest,
    frames: fixture.frames,
    imagePath: fixture.imagePath,
    imageSha256: fixture.imageSha256,
    validate: async (staging) => {
      const result = runValidator(
        "tests/perf/validate-full-quality-experiment.py",
        staging,
      );
      assert.equal(result.status, 0, result.stderr || result.stdout);
    },
  });
  assert.equal(JSON.parse(await readFile(suitePath, "utf8")).status, "complete");
  await assert.rejects(access(join(root, ".suite.json.staging")), { code: "ENOENT" });
});

test("Truck raster admission rejects missing, one-frame legacy, and global plans", async (t) => {
  const cases = [
    {
      name: "missing plan",
      mutate: ({ frames }) => {
        delete frames[0].raster_execution_plan;
      },
      message: /retained measured frame 0.*observed missing/,
    },
    {
      name: "one-frame legacy mutation",
      mutate: ({ frames }) => {
        frames[37].raster_execution_plan = "legacy_quads";
      },
      message: /retained measured frame 37.*observed legacy_quads/,
    },
    {
      name: "global plan",
      mutate: ({ manifest, frames }) => {
        manifest.renderer.raster_execution_plan = "global_quads";
        for (const frame of frames) frame.raster_execution_plan = "global_quads";
      },
      message: /manifest renderer\.raster_execution_plan.*observed global_quads/,
    },
  ];

  for (const testCase of cases) {
    await t.test(testCase.name, async (subtest) => {
      const root = await mkdtemp(join(tmpdir(), "gsplat-truck-raster-reject-"));
      subtest.after(() => rm(root, { recursive: true, force: true }));
      const fixture = await installTemporaryTruckArtifact(root);
      testCase.mutate(fixture);
      await writeFile(
        join(fixture.artifact, "manifest.json"),
        `${JSON.stringify(fixture.manifest)}\n`,
      );
      await writeFile(
        join(fixture.artifact, "frames.jsonl"),
        `${fixture.frames.map((frame) => JSON.stringify(frame)).join("\n")}\n`,
      );
      const benchmark = runValidator(
        "tests/perf/validate-benchmark-artifacts.py",
        fixture.artifact,
      );
      assert.equal(benchmark.status, 0, benchmark.stderr || benchmark.stdout);

      const suitePath = join(root, TRUCK_1080P_QUALIFICATION.suite_name);
      await assert.rejects(
        publishValidatedTruck1080pSuite({
          suitePath,
          manifest: fixture.manifest,
          frames: fixture.frames,
          imagePath: fixture.imagePath,
          imageSha256: fixture.imageSha256,
          validate: async () => {
            assert.fail("raster admission must fail before the suite validator");
          },
        }),
        testCase.message,
      );
      await assert.rejects(access(suitePath), { code: "ENOENT" });
      await assert.rejects(access(join(root, ".suite.json.staging")), { code: "ENOENT" });
    });
  }
});

test("Truck suite validator failure never publishes suite.json", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "gsplat-truck-suite-failure-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const fixture = await installTemporaryTruckArtifact(root);
  const suitePath = join(root, TRUCK_1080P_QUALIFICATION.suite_name);
  await assert.rejects(
    publishValidatedTruck1080pSuite({
      suitePath,
      manifest: fixture.manifest,
      frames: fixture.frames,
      imagePath: fixture.imagePath,
      imageSha256: fixture.imageSha256,
      validate: async () => {
        throw new Error("validator rejected fixture");
      },
    }),
    /validator rejected fixture/,
  );
  await assert.rejects(access(suitePath), { code: "ENOENT" });
  await access(join(root, ".suite.json.staging"));
});
