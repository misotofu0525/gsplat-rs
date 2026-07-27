export const WEB_DEPTH_PAIRED_TIMING = Object.freeze({
  schema: "gsplat-web-depth-paired-timing/v1",
  width: 1920,
  height: 1080,
  warmupFrames: 20,
  measuredFrames: 80,
  minimumPairs: 3,
  defaultPairs: 3,
  defaultSeed: 20260728,
  sourceCount: 2541226,
  shDegree: 3,
  exactProfile: "ExactFull32",
  candidateProfile: "CandidateStable20",
  plan: "gpu_preproject",
});

function fail(message) {
  throw new Error(`Web Candidate20 paired timing rejected: ${message}`);
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

function fullSha(value, label) {
  const sha = string(value, label);
  if (!/^[0-9a-f]{40}$/.test(sha)) fail(`${label} must be a full lowercase Git SHA`);
  return sha;
}

function sha256(value, label) {
  const digest = string(value, label);
  if (!/^[0-9a-f]{64}$/.test(digest)) fail(`${label} must be lowercase SHA-256`);
  return digest;
}

function seededRandom(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

export function schedulePairs(pairs, seed) {
  integer(pairs, "pairs", { positive: true });
  integer(seed, "seed");
  if (pairs < WEB_DEPTH_PAIRED_TIMING.minimumPairs) {
    fail(`pairs must be at least ${WEB_DEPTH_PAIRED_TIMING.minimumPairs}`);
  }
  const forward = Object.freeze(["exact", "candidate"]);
  const reverse = Object.freeze(["candidate", "exact"]);
  const schedule = [];
  for (let index = 0; index < Math.floor(pairs / 2); index += 1) {
    schedule.push(forward, reverse);
  }
  if (pairs % 2 !== 0) schedule.push((seed & 1) === 0 ? forward : reverse);
  const random = seededRandom(seed);
  for (let index = schedule.length - 1; index > 0; index -= 1) {
    const swap = Math.floor(random() * (index + 1));
    [schedule[index], schedule[swap]] = [schedule[swap], schedule[index]];
  }
  const forwardCount = schedule.filter((order) => order === forward).length;
  const reverseCount = schedule.filter((order) => order === reverse).length;
  if (Math.abs(forwardCount - reverseCount) > 1) fail("counterbalanced schedule is invalid");
  return schedule.map((order, pairIndex) => Object.freeze({
    pair_index: pairIndex,
    order: [...order],
  }));
}

export function validateK1dAdmission({
  suite,
  expectedCommit,
  exactBuild,
  candidateBuild,
  packageHashes,
}) {
  const value = object(suite, "K1d suite");
  const commit = fullSha(expectedCommit, "expected commit");
  if (value.schema !== "gsplat-balanced-image-gate/v1"
      || value.evidence_class !== "balanced_quality_candidate"
      || value.experiment?.name !== "b1-depth-key-candidate20"
      || value.experiment?.changed_receipt !== "depth_precision") {
    fail("quality suite is not the retained K1d Candidate20 gate");
  }
  if (value.inputs?.commit !== commit) fail("K1d suite commit differs from the timing commit");
  const exactness = object(value.exactness, "K1d exactness");
  if (exactness.source_membership !== "all"
      || exactness.sampling !== "disabled"
      || exactness.lod !== "disabled"
      || exactness.render_mode !== "sorted_alpha"
      || exactness.partial_scene_published !== false
      || exactness.full_membership_full_resolution !== true
      || exactness.quality_candidate !== true) {
    fail("K1d suite does not preserve its full-membership quality-candidate contract");
  }
  const resolution = object(value.resolution, "K1d resolution");
  for (const stage of ["requested", "surface", "internal_render", "presented"]) {
    if (resolution[`${stage}_width`] !== WEB_DEPTH_PAIRED_TIMING.width
        || resolution[`${stage}_height`] !== WEB_DEPTH_PAIRED_TIMING.height) {
      fail(`K1d ${stage} resolution is not 1920x1080`);
    }
  }
  if (JSON.stringify(value.camera?.trace_frame_indices) !== JSON.stringify([0, 1, 0])
      || !Array.isArray(value.frames) || value.frames.length !== 3
      || !Array.isArray(value.transitions) || value.transitions.length !== 2) {
    fail("K1d suite lacks the complete moving 0 -> 1 -> 0 gate");
  }
  for (const [index, frame] of value.frames.entries()) {
    if (frame?.presented !== true
        || frame?.exact?.depth_precision?.profile !== WEB_DEPTH_PAIRED_TIMING.exactProfile
        || frame?.candidate?.depth_precision?.profile
          !== WEB_DEPTH_PAIRED_TIMING.candidateProfile
        || frame?.exact?.projected_cache_precision?.profile !== "ExactAxes32"
        || frame?.candidate?.projected_cache_precision?.profile !== "ExactAxes32"
        || frame?.exact?.resident_sh?.codec_profile !== "ExactSigned11BandScale5"
        || frame?.candidate?.resident_sh?.codec_profile !== "ExactSigned11BandScale5") {
      fail(`K1d frame ${index} lacks the admitted precision receipts`);
    }
  }

  const hashes = object(packageHashes, "timing package hashes");
  const inputs = object(value.inputs, "K1d inputs");
  for (const field of ["exact_js_sha256", "exact_wasm_sha256",
    "candidate_js_sha256", "candidate_wasm_sha256", "exact_build_receipt_sha256",
    "candidate_build_receipt_sha256"]) {
    if (sha256(hashes[field], `package ${field}`) !== inputs[field]) {
      fail(`timing package ${field} differs from the K1d-qualified binary`);
    }
  }
  validateBuildReceipt(exactBuild, "quality-exact", commit, hashes, "exact");
  validateBuildReceipt(candidateBuild, "quality-candidate20", commit, hashes, "candidate");
  return Object.freeze({
    experiment: value.experiment.name,
    commit,
    exact_profile: WEB_DEPTH_PAIRED_TIMING.exactProfile,
    candidate_profile: WEB_DEPTH_PAIRED_TIMING.candidateProfile,
  });
}

function validateBuildReceipt(receipt, profile, commit, hashes, lane) {
  const value = object(receipt, `${lane} build receipt`);
  const expected = {
    schema: "gsplat-web-diagnostic-build/v1",
    repository_commit: commit,
    dirty: false,
    profile,
    js_sha256: hashes[`${lane}_js_sha256`],
    wasm_sha256: hashes[`${lane}_wasm_sha256`],
  };
  for (const [field, wanted] of Object.entries(expected)) {
    if (value[field] !== wanted) fail(`${lane} build receipt ${field} mismatch`);
  }
}

export function cameraFromTraceFrame(frame) {
  const value = object(frame, "trace frame");
  const pose = object(value.pose, "trace frame.pose");
  const intrinsics = object(value.intrinsics, "trace frame.intrinsics");
  if (!Array.isArray(pose.position) || pose.position.length !== 3
      || !Array.isArray(pose.rotation_xyzw) || pose.rotation_xyzw.length !== 4) {
    fail("trace pose dimensions are invalid");
  }
  const camera = {
    position: pose.position.map(Number),
    rotationXyzw: pose.rotation_xyzw.map(Number),
    intrinsics: {
      verticalFovRadians: Number(intrinsics.vertical_fov_radians),
      nearPlane: Number(intrinsics.near_plane),
      farPlane: Number(intrinsics.far_plane),
    },
  };
  const numbers = [...camera.position, ...camera.rotationXyzw,
    ...Object.values(camera.intrinsics)];
  if (numbers.some((item) => !Number.isFinite(item))) fail("trace camera is non-finite");
  return camera;
}

function validateLoadReceipt(receipt, dataset) {
  const value = object(receipt, "load receipt");
  const count = integer(dataset.splat_count, "dataset splat count", { positive: true });
  const degree = integer(dataset.sh_degree, "dataset SH degree");
  for (const field of ["sourceCount", "decodedCount", "encodedCount", "residentCount",
    "addressableCount"]) {
    if (value[field] !== count) fail(`load receipt ${field} is not complete Truck`);
  }
  if (value.sourceShDegree !== degree || value.residentShDegree !== degree
      || value.inputSha256 !== sha256(dataset.sha256, "dataset SHA-256")
      || value.fullQuality !== true || value.sourceMembership !== "all"
      || value.samplingEnabled !== false || value.lodEnabled !== false
      || value.partialScenePublished !== false || value.streamed !== true) {
    fail("load receipt is not streamed full-membership Truck SH3");
  }
}

function validateFrame(frame, dataset, expectTerminal) {
  const value = object(frame, "rendered frame");
  if (value.framePresented !== true || value.orderBackend !== "gpu"
      || value.projectedPolicy !== "compact" || value.projectedExecution !== "compact"
      || value.gpuOrderProducer !== "preproject"
      || value.rasterExecutionPlan !== "projected_quads_exact"
      || value.gpuSortFallback !== false || value.refreshSort !== true) {
    fail("timing frame did not execute refreshed GPU Preproject Compact Exact");
  }
  for (const [field, expected] of [["surfaceWidth", WEB_DEPTH_PAIRED_TIMING.width],
    ["surfaceHeight", WEB_DEPTH_PAIRED_TIMING.height],
    ["internalRenderWidth", WEB_DEPTH_PAIRED_TIMING.width],
    ["internalRenderHeight", WEB_DEPTH_PAIRED_TIMING.height],
    ["presentedWidth", WEB_DEPTH_PAIRED_TIMING.width],
    ["presentedHeight", WEB_DEPTH_PAIRED_TIMING.height]]) {
    if (value[field] !== expected) fail(`timing frame ${field} mismatch`);
  }
  if (expectTerminal) {
    if (value.currentStatsSubmission !== "issued") fail("terminal frame did not issue current stats");
  } else if (value.currentStatsSubmission !== "not_requested") {
    fail("ordinary timing frame unexpectedly requested current stats");
  }
  if (!Number.isSafeInteger(value.cameraRevision) || value.cameraRevision <= 0) {
    fail("timing frame lacks renderer camera revision");
  }
  if (value.visibleCount !== null && value.visibleCount > dataset.splat_count) {
    fail("timing frame visible count exceeds Truck source count");
  }
}

function frameSubmission(frame) {
  return {
    ticket: frame.currentStatsTicket,
    plan: frame.currentStatsPlan,
    sceneGeneration: frame.currentStatsSceneGeneration,
    cameraRevision: frame.currentStatsCameraRevision,
    viewportGeneration: frame.currentStatsViewportGeneration,
    contractGeneration: frame.currentStatsContractGeneration,
    planSetGeneration: frame.currentStatsPlanSetGeneration,
    orderGeneration: frame.currentStatsOrderGeneration,
    rasterGeneration: frame.currentStatsRasterGeneration,
    encodeAttempt: frame.currentStatsEncodeAttempt,
    presentationSequence: frame.currentStatsPresentationSequence,
  };
}

export function joinCurrentStatsTerminal(frame, terminal, dataset) {
  validateFrame(frame, dataset, true);
  const value = object(terminal, "current-stats terminal");
  if (value.status !== "ready" || value.plan !== WEB_DEPTH_PAIRED_TIMING.plan
      || value.countSemantics !== "indirect_draw_equals_contributor"
      || value.sourceCount !== dataset.splat_count
      || value.contributorCount > value.visibleCount
      || value.visibleCount > value.sourceCount
      || value.drawnCount !== value.contributorCount) {
    fail("current-stats terminal is not exact current Truck contributor evidence");
  }
  const submission = frameSubmission(frame);
  for (const field of Object.keys(submission)) {
    if (submission[field] !== value[field]) fail(`current-stats ${field} identity mismatch`);
  }
  return Object.freeze({ submission: Object.freeze(submission), terminal: Object.freeze({ ...value }) });
}

async function pollTerminal(renderer, nextFrame, attempts) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const terminal = renderer.pollCurrentStats();
    if (terminal.status !== "empty") return terminal;
    await nextFrame();
  }
  fail("current-stats terminal timed out without rendering a retry frame");
}

async function renderTraceFrame(renderer, traceFrame, nextFrame, dataset, expectTerminal) {
  await nextFrame();
  renderer.setCamera(cameraFromTraceFrame(traceFrame));
  if (expectTerminal) {
    const request = renderer.requestCurrentStats();
    if (request?.status !== "requested") fail("current-stats request was not admitted");
  }
  const frame = renderer.renderFrame();
  validateFrame(frame, dataset, expectTerminal);
  return frame;
}

export async function collectLaneTiming({
  renderer,
  lane,
  pairIndex,
  position,
  trace,
  dataset,
  warmupFrames = WEB_DEPTH_PAIRED_TIMING.warmupFrames,
  measuredFrames = WEB_DEPTH_PAIRED_TIMING.measuredFrames,
  nextFrame = () => new Promise((resolve) => requestAnimationFrame(() => resolve())),
  now = () => performance.now(),
  terminalPollAttempts = 600,
}) {
  if (!["exact", "candidate"].includes(lane)) fail(`unknown lane ${lane}`);
  integer(pairIndex, "pair index");
  integer(position, "pair position", { positive: true });
  if (position > 2) fail("pair position must be one or two");
  integer(warmupFrames, "warmup frames", { positive: true });
  integer(measuredFrames, "measured frames", { positive: true });
  const frames = object(trace, "Truck trace").frames;
  if (!Array.isArray(frames) || frames.length < 2) fail("Truck timing trace is not moving");
  validateLoadReceipt(renderer.loadReceipt(), dataset);
  if (renderer.rasterPath() !== "packed_atlas") fail("timing renderer is not Packed Exact");

  let terminalFrame = null;
  for (let index = 0; index < warmupFrames; index += 1) {
    terminalFrame = await renderTraceFrame(
      renderer,
      frames[index % frames.length],
      nextFrame,
      dataset,
      index === warmupFrames - 1,
    );
  }
  const warmupTerminal = await pollTerminal(renderer, nextFrame, terminalPollAttempts);
  const warmupEvidence = joinCurrentStatsTerminal(terminalFrame, warmupTerminal, dataset);

  const measured = [];
  let started = null;
  for (let index = 0; index < measuredFrames; index += 1) {
    await nextFrame();
    if (started === null) started = now();
    renderer.setCamera(cameraFromTraceFrame(frames[index % frames.length]));
    const last = index === measuredFrames - 1;
    if (last) {
      const request = renderer.requestCurrentStats();
      if (request?.status !== "requested") fail("measured current-stats request was not admitted");
    }
    const frame = renderer.renderFrame();
    validateFrame(frame, dataset, last);
    measured.push({
      frame_index: index,
      trace_frame_index: index % frames.length,
      frame_ms: finite(frame.frameMs, `frame ${index}.frameMs`),
      frame_wall_ms: finite(frame.frameWallMs, `frame ${index}.frameWallMs`),
      camera_revision: frame.cameraRevision,
      order_generation: frame.currentStatsOrderGeneration ?? null,
      presentation_sequence: frame.currentStatsPresentationSequence ?? null,
      terminal_issued: last,
    });
    terminalFrame = frame;
  }
  const measuredTerminal = await pollTerminal(renderer, nextFrame, terminalPollAttempts);
  const terminalAt = now();
  const measuredEvidence = joinCurrentStatsTerminal(terminalFrame, measuredTerminal, dataset);
  const terminalWindowMs = finite(terminalAt - started, "terminal window");
  return Object.freeze({
    lane,
    pair_index: pairIndex,
    pair_position: position,
    warmup_frames: warmupFrames,
    measured_frames: measuredFrames,
    capture_requested: false,
    capture_api_calls: 0,
    warmup_evidence: warmupEvidence,
    measured_evidence: measuredEvidence,
    terminal_window_ms: terminalWindowMs,
    terminal_ms_per_frame: terminalWindowMs / measuredFrames,
    frame_wall_mean_ms: measured.reduce((sum, frame) => sum + frame.frame_wall_ms, 0)
      / measuredFrames,
    frames: measured,
  });
}

export function validateCollectedRuns(runs, schedule, {
  warmupFrames = WEB_DEPTH_PAIRED_TIMING.warmupFrames,
  measuredFrames = WEB_DEPTH_PAIRED_TIMING.measuredFrames,
} = {}) {
  if (!Array.isArray(runs)) fail("timing runs are missing");
  if (!Array.isArray(schedule) || schedule.length < WEB_DEPTH_PAIRED_TIMING.minimumPairs) {
    fail("timing schedule is missing or too short");
  }
  integer(warmupFrames, "warmup frames", { positive: true });
  integer(measuredFrames, "measured frames", { positive: true });
  if (runs.length !== schedule.length * 2) {
    fail("timing run count differs from the admitted schedule");
  }
  for (let index = 0; index < runs.length; index += 1) {
    const run = object(runs[index], `timing run ${index}`);
    const pair = object(schedule[Math.floor(index / 2)], `schedule pair ${Math.floor(index / 2)}`);
    const position = (index % 2) + 1;
    const lane = pair.order?.[position - 1];
    if (pair.pair_index !== Math.floor(index / 2)
        || run.pair_index !== pair.pair_index
        || run.pair_position !== position
        || run.lane !== lane) {
      fail(`timing run ${index} differs from the seeded schedule`);
    }
    if (run.warmup_frames !== warmupFrames || run.measured_frames !== measuredFrames
        || !Array.isArray(run.frames) || run.frames.length !== measuredFrames) {
      fail(`timing run ${index} frame counts differ from the protocol`);
    }
    if (run.capture_requested !== false || run.capture_api_calls !== 0) {
      fail(`timing run ${index} used capture or blit work`);
    }
    object(run.warmup_evidence, `timing run ${index} warmup evidence`);
    object(run.measured_evidence, `timing run ${index} measured evidence`);
    finite(run.terminal_window_ms, `timing run ${index} terminal window`);
    finite(run.terminal_ms_per_frame, `timing run ${index} terminal time per frame`);
    finite(run.frame_wall_mean_ms, `timing run ${index} frame-wall mean`);
  }
  return runs;
}

export function classifyPairs(runs) {
  if (!Array.isArray(runs)) fail("timing runs are missing");
  const pairs = new Map();
  for (const run of runs) {
    if (!run || !["exact", "candidate"].includes(run.lane)) fail("timing run lane is invalid");
    const pair = pairs.get(run.pair_index) ?? {};
    if (pair[run.lane]) fail(`pair ${run.pair_index} duplicates ${run.lane}`);
    pair[run.lane] = run;
    pairs.set(run.pair_index, pair);
  }
  if (pairs.size < WEB_DEPTH_PAIRED_TIMING.minimumPairs) fail("timing has fewer than three pairs");
  const observations = [...pairs.entries()].sort(([left], [right]) => left - right)
    .map(([pairIndex, pair]) => {
      if (!pair.exact || !pair.candidate) fail(`pair ${pairIndex} is incomplete`);
      const exact = finite(pair.exact.terminal_ms_per_frame, `pair ${pairIndex} exact timing`);
      const candidate = finite(
        pair.candidate.terminal_ms_per_frame,
        `pair ${pairIndex} candidate timing`,
      );
      if (exact === 0) fail(`pair ${pairIndex} exact timing is zero`);
      return {
        pair_index: pairIndex,
        exact_terminal_ms_per_frame: exact,
        candidate_terminal_ms_per_frame: candidate,
        candidate_minus_exact_ms: candidate - exact,
        candidate_minus_exact_percent: ((candidate / exact) - 1) * 100,
      };
    });
  const candidateWins = observations.every((item) => item.candidate_minus_exact_ms < 0);
  const exactWins = observations.every((item) => item.candidate_minus_exact_ms > 0);
  return Object.freeze({
    outcome: candidateWins ? "candidate" : exactWins ? "exact" : "inconclusive",
    metric: "first_measured_input_to_final_renderer_terminal_ms_per_frame",
    performance_percentage_is_observation: true,
    hard_percentage_gate: null,
    observations,
  });
}
