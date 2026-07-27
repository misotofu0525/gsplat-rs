import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { parseArguments } from "../scripts/collect-web-depth-precision-paired-timing.mjs";
import {
  WEB_DEPTH_PAIRED_TIMING,
  classifyPairs,
  collectLaneTiming,
  joinCurrentStatsTerminal,
  schedulePairs,
  validateCollectedRuns,
  validateK1dAdmission,
} from "../src/depth-precision-paired-timing.mjs";

const COMMIT = "a".repeat(40);
const DIGEST = "b".repeat(64);
const DATASET = Object.freeze({
  splat_count: WEB_DEPTH_PAIRED_TIMING.sourceCount,
  sh_degree: WEB_DEPTH_PAIRED_TIMING.shDegree,
  sha256: "d".repeat(64),
});

function packageHashes() {
  return Object.fromEntries([
    "exact_js_sha256",
    "exact_wasm_sha256",
    "candidate_js_sha256",
    "candidate_wasm_sha256",
    "exact_build_receipt_sha256",
    "candidate_build_receipt_sha256",
  ].map((field) => [field, DIGEST]));
}

function buildReceipt(profile) {
  return {
    schema: "gsplat-web-diagnostic-build/v1",
    repository_commit: COMMIT,
    dirty: false,
    profile,
    js_sha256: DIGEST,
    wasm_sha256: DIGEST,
  };
}

function qualitySuite() {
  const frame = {
    presented: true,
    exact: {
      depth_precision: { profile: WEB_DEPTH_PAIRED_TIMING.exactProfile },
      projected_cache_precision: { profile: "ExactAxes32" },
      resident_sh: { codec_profile: "ExactSigned11BandScale5" },
    },
    candidate: {
      depth_precision: { profile: WEB_DEPTH_PAIRED_TIMING.candidateProfile },
      projected_cache_precision: { profile: "ExactAxes32" },
      resident_sh: { codec_profile: "ExactSigned11BandScale5" },
    },
  };
  return {
    schema: "gsplat-balanced-image-gate/v1",
    evidence_class: "balanced_quality_candidate",
    experiment: { name: "b1-depth-key-candidate20", changed_receipt: "depth_precision" },
    inputs: { commit: COMMIT, ...packageHashes() },
    exactness: {
      source_membership: "all",
      sampling: "disabled",
      lod: "disabled",
      render_mode: "sorted_alpha",
      partial_scene_published: false,
      full_membership_full_resolution: true,
      quality_candidate: true,
    },
    resolution: Object.fromEntries(
      ["requested", "surface", "internal_render", "presented"].flatMap((stage) => [
        [`${stage}_width`, WEB_DEPTH_PAIRED_TIMING.width],
        [`${stage}_height`, WEB_DEPTH_PAIRED_TIMING.height],
      ]),
    ),
    camera: { trace_frame_indices: [0, 1, 0] },
    frames: [structuredClone(frame), structuredClone(frame), structuredClone(frame)],
    transitions: [{}, {}],
  };
}

function identity(ticket, cameraRevision) {
  return {
    ticket,
    plan: WEB_DEPTH_PAIRED_TIMING.plan,
    sceneGeneration: 2,
    cameraRevision,
    viewportGeneration: 3,
    contractGeneration: 4,
    planSetGeneration: 5,
    orderGeneration: cameraRevision + 10,
    rasterGeneration: cameraRevision + 20,
    encodeAttempt: cameraRevision + 30,
    presentationSequence: cameraRevision + 40,
  };
}

function exactFrame(cameraRevision, ticket = null) {
  const issued = ticket !== null;
  const receipt = issued ? identity(ticket, cameraRevision) : null;
  return {
    framePresented: true,
    orderBackend: "gpu",
    projectedPolicy: "compact",
    projectedExecution: "compact",
    gpuOrderProducer: "preproject",
    rasterExecutionPlan: "projected_quads_exact",
    gpuSortFallback: false,
    refreshSort: true,
    surfaceWidth: 1920,
    surfaceHeight: 1080,
    internalRenderWidth: 1920,
    internalRenderHeight: 1080,
    presentedWidth: 1920,
    presentedHeight: 1080,
    cameraRevision,
    visibleCount: 2_000_000,
    frameMs: 4,
    frameWallMs: 5,
    currentStatsSubmission: issued ? "issued" : "not_requested",
    currentStatsTicket: receipt?.ticket ?? null,
    currentStatsPlan: receipt?.plan ?? null,
    currentStatsSceneGeneration: receipt?.sceneGeneration ?? null,
    currentStatsCameraRevision: receipt?.cameraRevision ?? null,
    currentStatsViewportGeneration: receipt?.viewportGeneration ?? null,
    currentStatsContractGeneration: receipt?.contractGeneration ?? null,
    currentStatsPlanSetGeneration: receipt?.planSetGeneration ?? null,
    currentStatsOrderGeneration: receipt?.orderGeneration ?? null,
    currentStatsRasterGeneration: receipt?.rasterGeneration ?? null,
    currentStatsEncodeAttempt: receipt?.encodeAttempt ?? null,
    currentStatsPresentationSequence: receipt?.presentationSequence ?? null,
  };
}

function terminalFor(frame) {
  return {
    status: "ready",
    ...identity(frame.currentStatsTicket, frame.cameraRevision),
    countSemantics: "indirect_draw_equals_contributor",
    sourceCount: DATASET.splat_count,
    visibleCount: 2_000_000,
    contributorCount: 1_500_000,
    drawnCount: 1_500_000,
  };
}

function fakeRenderer() {
  let cameraRevision = 0;
  let ticket = 0;
  let requested = false;
  const terminals = [];
  return {
    renderCount: 0,
    requestCount: 0,
    loadReceipt: () => ({
      sourceCount: DATASET.splat_count,
      decodedCount: DATASET.splat_count,
      encodedCount: DATASET.splat_count,
      residentCount: DATASET.splat_count,
      addressableCount: DATASET.splat_count,
      sourceShDegree: DATASET.sh_degree,
      residentShDegree: DATASET.sh_degree,
      fullQuality: true,
      sourceMembership: "all",
      samplingEnabled: false,
      lodEnabled: false,
      partialScenePublished: false,
      streamed: true,
      inputSha256: DATASET.sha256,
    }),
    rasterPath: () => "packed_atlas",
    setCamera: () => { cameraRevision += 1; },
    requestCurrentStats() {
      this.requestCount += 1;
      requested = true;
      return { status: "requested" };
    },
    renderFrame() {
      this.renderCount += 1;
      const frame = exactFrame(cameraRevision, requested ? ++ticket : null);
      if (requested) terminals.push(terminalFor(frame));
      requested = false;
      return frame;
    },
    pollCurrentStats: () => terminals.shift() ?? { status: "empty" },
  };
}

function trace() {
  const frame = (index) => ({
    frame_index: index,
    pose: { position: [index, 0, 1], rotation_xyzw: [0, 0, 0, 1] },
    intrinsics: { vertical_fov_radians: 1, near_plane: 0.1, far_plane: 100 },
  });
  return { frames: [frame(0), frame(1)] };
}

function runReceipt(pairIndex, pairPosition, lane, time) {
  return {
    pair_index: pairIndex,
    pair_position: pairPosition,
    lane,
    warmup_frames: 20,
    measured_frames: 80,
    capture_requested: false,
    capture_api_calls: 0,
    warmup_evidence: {},
    measured_evidence: {},
    terminal_window_ms: time * 80,
    terminal_ms_per_frame: time,
    frame_wall_mean_ms: time,
    frames: Array.from({ length: 80 }, () => ({})),
  };
}

test("CLI defaults to three seeded pairs and requires a full commit", () => {
  const args = parseArguments([
    "--exact-pkg", "exact", "--candidate-pkg", "candidate",
    "--quality-suite", "quality.json", "--expected-commit", COMMIT,
    "--chrome", "chrome", "--output", "fresh",
  ]);
  assert.equal(args.pairs, 3);
  assert.equal(args.seed, WEB_DEPTH_PAIRED_TIMING.defaultSeed);
  assert.throws(() => parseArguments([
    "--exact-pkg", "exact", "--candidate-pkg", "candidate",
    "--quality-suite", "quality.json", "--expected-commit", "short",
    "--chrome", "chrome", "--output", "fresh",
  ]), /full SHA/);
  assert.throws(() => parseArguments([
    "--exact-pkg", "exact", "--candidate-pkg", "candidate",
    "--quality-suite", "quality.json", "--expected-commit", COMMIT,
    "--chrome", "chrome", "--output", "fresh", "--pairs", "2",
  ]), /at least 3/);
});

test("schedule is deterministic and counterbalanced", () => {
  const first = schedulePairs(5, 17);
  assert.deepEqual(first, schedulePairs(5, 17));
  assert.equal(first.length, 5);
  const signatures = first.map((pair) => pair.order.join("/"));
  const forward = signatures.filter((value) => value === "exact/candidate").length;
  const reverse = signatures.filter((value) => value === "candidate/exact").length;
  assert.ok(Math.abs(forward - reverse) <= 1);
});

test("K1d admission binds timing packages and commit to retained quality", () => {
  assert.equal(validateK1dAdmission({
    suite: qualitySuite(),
    expectedCommit: COMMIT,
    exactBuild: buildReceipt("quality-exact"),
    candidateBuild: buildReceipt("quality-candidate20"),
    packageHashes: packageHashes(),
  }).commit, COMMIT);
  const wrong = qualitySuite();
  wrong.inputs.commit = "c".repeat(40);
  assert.throws(() => validateK1dAdmission({
    suite: wrong,
    expectedCommit: COMMIT,
    exactBuild: buildReceipt("quality-exact"),
    candidateBuild: buildReceipt("quality-candidate20"),
    packageHashes: packageHashes(),
  }), /commit differs/);
});

test("current-stats join rejects a mismatched renderer identity", () => {
  const frame = exactFrame(7, 3);
  assert.equal(joinCurrentStatsTerminal(frame, terminalFor(frame), DATASET).terminal.drawnCount,
    1_500_000);
  assert.throws(
    () => joinCurrentStatsTerminal(frame, { ...terminalFor(frame), orderGeneration: 999 }, DATASET),
    /orderGeneration identity mismatch/,
  );
});

test("lane timing renders once per scheduled frame and uses only two terminal fences", async () => {
  const renderer = fakeRenderer();
  let clock = 0;
  const receipt = await collectLaneTiming({
    renderer,
    lane: "candidate",
    pairIndex: 0,
    position: 1,
    trace: trace(),
    dataset: DATASET,
    warmupFrames: 2,
    measuredFrames: 3,
    nextFrame: async () => {},
    now: () => { clock += 5; return clock; },
  });
  assert.equal(renderer.renderCount, 5);
  assert.equal(renderer.requestCount, 2);
  assert.equal(receipt.frames.length, 3);
  assert.equal(receipt.capture_requested, false);
  assert.equal(receipt.capture_api_calls, 0);
  assert.equal(receipt.terminal_window_ms, 5);
});

test("collected runs must exactly match the seeded schedule and forbid capture", () => {
  const schedule = schedulePairs(3, 4);
  const runs = schedule.flatMap((pair) => pair.order.map((lane, index) =>
    runReceipt(pair.pair_index, index + 1, lane, lane === "candidate" ? 4 : 5)));
  assert.equal(validateCollectedRuns(runs, schedule), runs);
  assert.equal(classifyPairs(runs).outcome, "candidate");
  assert.throws(
    () => validateCollectedRuns(runs.map((run, index) => index === 0
      ? { ...run, capture_api_calls: 1 }
      : run), schedule),
    /capture or blit/,
  );
  assert.throws(
    () => validateCollectedRuns([runs[1], runs[0], ...runs.slice(2)], schedule),
    /seeded schedule/,
  );
});

test("classification stays three-state and percentages are observations", () => {
  const candidate = classifyPairs([
    runReceipt(0, 1, "exact", 5), runReceipt(0, 2, "candidate", 4),
    runReceipt(1, 1, "candidate", 4), runReceipt(1, 2, "exact", 5),
    runReceipt(2, 1, "exact", 5), runReceipt(2, 2, "candidate", 4),
  ]);
  assert.equal(candidate.outcome, "candidate");
  assert.equal(candidate.performance_percentage_is_observation, true);
  assert.equal(candidate.hard_percentage_gate, null);
  const mixed = structuredClone(candidate.observations);
  const runs = mixed.flatMap((_, pairIndex) => [
    runReceipt(pairIndex, 1, "exact", 5),
    runReceipt(pairIndex, 2, "candidate", pairIndex === 1 ? 6 : 4),
  ]);
  assert.equal(classifyPairs(runs).outcome, "inconclusive");
});

test("timing implementation contains no diagnostic capture API call", async () => {
  const [source, collector] = await Promise.all([
    readFile(new URL("../src/depth-precision-paired-timing.mjs", import.meta.url), "utf8"),
    readFile(new URL("../scripts/collect-web-depth-precision-paired-timing.mjs", import.meta.url),
      "utf8"),
  ]);
  for (const text of [source, collector]) {
    assert.doesNotMatch(text, /\.requestDiagnosticSurfaceCapture\s*\(/);
    assert.doesNotMatch(text, /\.takeDiagnosticSurfaceCapture\s*\(/);
  }
});
