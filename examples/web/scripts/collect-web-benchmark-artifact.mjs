#!/usr/bin/env node
/**
 * Headless Chrome collector for gsplat-rs Web Phase A baseline artifacts.
 * Emits a validated gsplat-benchmark/v1 directory from console JSON lines.
 */
import { access, copyFile, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { constants, createReadStream } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFile as execFileCallback, spawn } from 'node:child_process';
import process from 'node:process';
import os from 'node:os';
import { promisify } from 'node:util';
import {
  benchmarkCountEvidence,
  benchmarkSummaryFromFrameRecords,
  distribution,
} from '../src/benchmark-artifact.mjs';
import {
  joinOrderingEvidence,
  monotonicOrderingWindow,
  validateTerminalTicketLedger,
  validateOrderingEvidence,
} from '../src/benchmark-order-evidence.mjs';
import {
  joinCurrentStatsEvidence,
  validateAuxiliaryCurrentStatsFormalLedger,
  validateCurrentStatsAttemptSubmissionJoin,
  validateCurrentStatsEvidence,
  validateCurrentStatsTerminalLedger,
} from '../src/benchmark-current-stats-evidence.mjs';
import { validateCurrentStatsScheduleEvidence } from '../src/benchmark-current-stats-schedule.mjs';
import { validateBenchmarkWindowManifest } from '../src/benchmark-window-mode.mjs';
import {
  validateProjectedFrameEvidence,
  validateProjectedTerminalLedger,
} from '../src/benchmark-projected-evidence.mjs';
import {
  validateGpuProducerFrameEvidence,
  validateGpuProducerMeasuredSubmissions,
  validateGpuProducerTerminalLedger,
} from '../src/benchmark-gpu-producer-evidence.mjs';
import {
  validateDatasetEvidenceIdentity,
  validateFormalDatasetEvidenceIdentity,
  validateFormalDatasetLogicalRequest,
} from '../src/dataset-identity.mjs';
import {
  TRUCK_1080P_QUALIFICATION,
  truck1080pQualificationByName,
  claimTruck1080pOutputRoot,
  claimTruck1080pThroughputStage,
  optionalEnvironmentValue,
  publishTruck1080pControlCompletion,
  publishValidatedTruck1080pSuite,
  validateTruck1080pCleanWorkingTree,
  validateTruck1080pCollectorConfig,
  validateTruck1080pExactRasterEvidence,
  validateTruck1080pFixedCompactControlEvidence,
} from '../src/truck-1080p-qualification.mjs';
import {
  decorateQ1GsplatControl,
  decorateQ1GsplatThroughput,
} from './q1-gsplat-artifact.mjs';
import { rgba8Png } from './rgba8-png.mjs';
import {
  assertStableBrowserRuntime,
  assertStableRendererSurfaceDevice,
  browserProcessArgsReceipt,
  observedRunContext,
} from './q1-browser-environment.mjs';
import { Q1ArtifactTransaction } from './q1-artifact-transaction.mjs';
import { cleanupBrowserAndServer, waitForChildExit } from './q1-process-cleanup.mjs';
import { gsplatQ1WebGpuEnvironmentFields } from '../../../tests/perf/q1-webgpu-environment.mjs';
import {
  browserOwnershipConfig,
  publishBrowserOwnershipHandshake,
} from '../../../tests/perf/browser-process-ownership.mjs';

const execFile = promisify(execFileCallback);

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, '../../..');
const playcanvasRoot = resolve(repoRoot, 'tests/competitive/playcanvas');
const Q1_BROWSER_ARGS = Object.freeze([
  '--enable-unsafe-webgpu',
  '--enable-gpu',
  '--ignore-gpu-blocklist',
]);
const chromeCandidates = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium'
].filter(Boolean);

const qualificationName = process.env.GSPLAT_PHASE_E_QUALIFICATION ?? '';
const qualification = qualificationName.length > 0;
const formalDatasetLogicalId = qualificationName === 'kitsune-static-v1'
  ? 'kitsune'
  : qualificationName === 'raster-diagnostic-v1'
    ? 'raster_diagnostic_v1'
    : truck1080pQualificationByName(qualificationName) ? 'truck' : null;
const truckQualification = truck1080pQualificationByName(qualificationName);
const truck1080pQualification = truckQualification !== null;
const truckQualificationStage = optionalEnvironmentValue(
  process.env.GSPLAT_TRUCK_QUALIFICATION_STAGE,
);
const q1ArtifactRole = optionalEnvironmentValue(process.env.GSPLAT_Q1_ARTIFACT_ROLE);
if (q1ArtifactRole !== null && !['control', 'throughput'].includes(q1ArtifactRole)) {
  throw new Error('GSPLAT_Q1_ARTIFACT_ROLE must be control or throughput');
}
const q1CaptureTraceFrame = q1ArtifactRole === 'control'
  ? Number(process.env.GSPLAT_Q1_CAPTURE_TRACE_FRAME)
  : null;
if (q1ArtifactRole === 'control' && ![0, 1].includes(q1CaptureTraceFrame)) {
  throw new Error('Q1 control requires GSPLAT_Q1_CAPTURE_TRACE_FRAME=0 or 1');
}
const q1ProtocolSha256 = optionalEnvironmentValue(process.env.GSPLAT_Q1_PROTOCOL_SHA256);
if (q1ArtifactRole !== null && !/^[0-9a-f]{64}$/.test(q1ProtocolSha256 ?? '')) {
  throw new Error('Q1 producer requires GSPLAT_Q1_PROTOCOL_SHA256');
}
const q1WasmPackageDirectory = optionalEnvironmentValue(
  process.env.GSPLAT_Q1_WASM_PACKAGE_DIR,
);
const q1RunContextPath = optionalEnvironmentValue(process.env.GSPLAT_Q1_RUN_CONTEXT);
const q1RunContext = q1ArtifactRole === null ? null : q1RunContextPath === null
  ? null
  : JSON.parse(await readFile(resolve(repoRoot, q1RunContextPath), 'utf8'));
if (q1ArtifactRole !== null && q1RunContext === null) {
  throw new Error('Q1 producer requires GSPLAT_Q1_RUN_CONTEXT');
}
const q1CollectionSessionId = optionalEnvironmentValue(
  process.env.GSPLAT_Q1_COLLECTION_SESSION_ID,
);
if (q1ArtifactRole !== null && q1CollectionSessionId === null) {
  throw new Error('Q1 producer requires GSPLAT_Q1_COLLECTION_SESSION_ID');
}
const q1SeriesRootText = optionalEnvironmentValue(process.env.GSPLAT_Q1_SERIES_ROOT);
const q1SeriesRoot = q1ArtifactRole === null || q1SeriesRootText === null
  ? null
  : resolve(q1SeriesRootText);
if (q1ArtifactRole !== null && q1SeriesRoot === null) {
  throw new Error('Q1 producer requires GSPLAT_Q1_SERIES_ROOT');
}
let claimedTruckOutputRoot = null;
let truckControlCompletion = null;
let q1ArtifactTransaction = null;
const m4Smoke = process.env.GSPLAT_M4_SMOKE === '1';
const requestedFrameCount = Number(
  process.env.GSPLAT_BENCHMARK_FRAMES ?? (qualification ? 3600 : 30),
);
const warmup = Number(process.env.GSPLAT_BENCHMARK_WARMUP_FRAMES ?? (qualification ? 120 : 5));
const dataset = process.env.GSPLAT_DATASET ?? (
  formalDatasetLogicalId ?? 'minimal'
);
if (formalDatasetLogicalId !== null) {
  validateFormalDatasetLogicalRequest({
    requestedLogicalId: dataset,
    expectedLogicalId: formalDatasetLogicalId,
  });
}
const outDir = resolve(
  process.env.GSPLAT_ARTIFACT_DIR ??
    resolve(
      repoRoot,
      m4Smoke
        ? 'target/benchmarks/m4-webgpu-smoke'
        : truck1080pQualification
          ? `target/qualification/q1-webgpu-truck-1080p/${TRUCK_1080P_QUALIFICATION.control.artifact_name}`
        : qualification
          ? 'target/benchmarks/phase-e/gsplat-web-kitsune-static-v1'
          : 'target/benchmarks/phase-a/web-minimal-v1'
    )
);
if (q1SeriesRoot !== null) {
  const pathFromSeries = relative(q1SeriesRoot, outDir);
  if (pathFromSeries === '' || pathFromSeries === '..' || pathFromSeries.startsWith(`..${sep}`)) {
    throw new Error('Q1 artifact directory must be a fresh child of GSPLAT_Q1_SERIES_ROOT');
  }
}
const fullQualitySuitePath = resolve(
  process.env.GSPLAT_FULL_QUALITY_SUITE ?? resolve(dirname(outDir), 'suite.json'),
);
const port = Number(process.env.GSPLAT_HTTP_PORT ?? 4173);
const geometryPath = process.env.GSPLAT_GEOMETRY_PATH ?? 'packed';
const orderBackend = process.env.GSPLAT_ORDER_BACKEND ?? 'adaptive';
const projectedPolicy = process.env.GSPLAT_PROJECTED_POLICY ?? 'adaptive';
if (!['candidate', 'compact', 'adaptive'].includes(projectedPolicy)) {
  throw new Error('GSPLAT_PROJECTED_POLICY must be candidate, compact, or adaptive');
}
const gpuOrderProducer = process.env.GSPLAT_GPU_ORDER_PRODUCER?.trim() || null;
if (gpuOrderProducer !== null && !['post-sort', 'preproject'].includes(gpuOrderProducer)) {
  throw new Error('GSPLAT_GPU_ORDER_PRODUCER must be post-sort or preproject');
}
const sortInterval = Number(process.env.GSPLAT_SORT_INTERVAL ?? 1);
const navigationTimeoutMs = Number(process.env.GSPLAT_NAVIGATION_TIMEOUT_MS ?? 600_000);
const benchmarkTimeoutMs = Number(process.env.GSPLAT_BENCHMARK_TIMEOUT_MS ?? 600_000);
const benchmarkSync = process.env.GSPLAT_BENCHMARK_SYNC === '1';
const benchmarkSettleMs = Number(process.env.GSPLAT_BENCHMARK_SETTLE_MS ?? 50);
const orderCompletionProtocol = process.env.GSPLAT_ORDER_COMPLETION_PROTOCOL ?? 'isolated_terminal';
if (!['isolated_terminal', 'sustained_window'].includes(orderCompletionProtocol)) {
  throw new Error('GSPLAT_ORDER_COMPLETION_PROTOCOL must be isolated_terminal or sustained_window');
}
const benchmarkWindowMode = process.env.GSPLAT_BENCHMARK_WINDOW_MODE
  ?? 'current_stats_evidence_window';
if (!['current_stats_evidence_window', 'terminal_queue_throughput_window']
  .includes(benchmarkWindowMode)) {
  throw new Error(
    'GSPLAT_BENCHMARK_WINDOW_MODE must be current_stats_evidence_window or '
      + 'terminal_queue_throughput_window',
  );
}
const currentStatsControlArtifact = optionalEnvironmentValue(
  process.env.GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT,
);
const terminalAdaptiveCell = orderBackend === 'adaptive'
  && projectedPolicy === 'adaptive' && gpuOrderProducer === null;
const terminalFixedGpuCompactCell = orderBackend === 'gpu'
  && projectedPolicy === 'compact' && gpuOrderProducer === null;
if (benchmarkWindowMode === 'terminal_queue_throughput_window' && (
  orderCompletionProtocol !== 'sustained_window'
  || geometryPath !== 'packed'
  || (!terminalAdaptiveCell && !terminalFixedGpuCompactCell)
  || benchmarkSync
  || currentStatsControlArtifact === null
)) {
  throw new Error(
    'terminal-queue throughput requires an admitted async Packed Exact sustained_window cell and '
      + 'GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT',
  );
}

async function loadCurrentStatsControlIdentity() {
  if (benchmarkWindowMode !== 'terminal_queue_throughput_window') return null;
  const requested = resolve(repoRoot, currentStatsControlArtifact);
  const manifestPath = (await stat(requested)).isDirectory()
    ? resolve(requested, 'manifest.json')
    : requested;
  await runPythonValidator(
    'tests/perf/validate-benchmark-artifacts.py',
    [dirname(manifestPath)],
    'current-stats control artifact validator',
  );
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  const manifestSha256 = await sha256File(manifestPath);
  const window = manifest.benchmark_window;
  const schedule = manifest.ordering_window?.current_stats_schedule;
  const expectedLogicalFrames = (window?.configuration?.warmup_frames ?? -1)
    + (window?.configuration?.measured_frames ?? -1);
  if (window?.mode !== 'current_stats_evidence_window'
      || window.performance_evidence !== false
      || window.evidence_role !== 'renderer_exact_current_stats_control'
      || window.control_artifact_identity?.run_id !== manifest.run_id
      || window.control_artifact_identity?.configuration_sha256
        !== window.configuration_sha256
      || schedule?.state !== 'complete'
      || !['isolated_terminal', 'sustained_window'].includes(schedule.protocol)
      || schedule.submitted_logical_count !== expectedLogicalFrames
      || schedule.issued_count !== expectedLogicalFrames
      || schedule.terminal_count !== expectedLogicalFrames
      || schedule.draw_count_at_final_drain_start !== schedule.draw_count_at_completion) {
    throw new Error('current-stats control artifact is not an admitted control-only window');
  }
  if (q1ArtifactRole === null
      && truck1080pQualification && truckQualificationStage === 'throughput') {
    if (truckControlCompletion === null
        || truckControlCompletion.control_manifest_sha256 !== manifestSha256
        || truckControlCompletion.control_run_id !== manifest.run_id
        || truckControlCompletion.configuration_sha256 !== window.configuration_sha256) {
      throw new Error('Truck throughput control artifact drifted from its completed control stage');
    }
  }
  return {
    runId: manifest.run_id,
    configurationSha256: window.configuration_sha256,
    manifestPath,
    manifestSha256,
  };
}

async function loadQ1ControlBindings() {
  if (q1ArtifactRole !== 'throughput') return null;
  const bindings = [];
  for (const traceFrameIndex of [0, 1]) {
    const value = optionalEnvironmentValue(
      process.env[`GSPLAT_Q1_CONTROL_ARTIFACT_${traceFrameIndex}`],
    );
    if (value === null) throw new Error(`missing Q1 control artifact ${traceFrameIndex}`);
    const manifestPath = resolve(repoRoot, value, 'manifest.json');
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
    if (manifest.q1_comparison?.artifact_role !== 'control'
        || manifest.q1_comparison?.capture_trace_frame_index !== traceFrameIndex) {
      throw new Error(`Q1 control artifact ${traceFrameIndex} has the wrong role or trace`);
    }
    bindings.push({
      trace_frame_index: traceFrameIndex,
      run_id: manifest.run_id,
      manifest_sha256: await sha256File(manifestPath),
      configuration_sha256: manifest.q1_comparison.configuration_sha256,
    });
  }
  return bindings;
}
if (gpuOrderProducer !== null && (
  geometryPath !== 'packed'
  || orderBackend !== 'gpu'
  || projectedPolicy !== 'compact'
  || sortInterval !== 1
  || orderCompletionProtocol !== 'isolated_terminal'
  || benchmarkSync
)) {
  throw new Error(
    'GPU producer qualification requires Packed geometry, forced GPU ordering, ' +
    'forced Compact projected drawing, sort interval 1, asynchronous progression, ' +
    'and isolated terminals',
  );
}

async function sha256File(path) {
  const digest = createHash('sha256');
  for await (const chunk of createReadStream(path)) digest.update(chunk);
  return digest.digest('hex');
}

function parseMacPowerReceipt(output) {
  const match = /Now drawing from '([^']+)'/.exec(output);
  if (!match) throw new Error('pmset did not report the active power source');
  return match[1].trim().toLowerCase().replaceAll(' ', '_');
}

function parseMacThermalReceipt(output) {
  if (/No thermal warning level has been recorded/.test(output)) return 'nominal';
  throw new Error('macOS thermal state is not admissible or cannot be interpreted');
}

async function observeQ1HostState(phase) {
  if (q1ArtifactRole === null) return null;
  if (process.platform !== 'darwin') {
    throw new Error('Q1 gsplat-rs producer currently requires the admitted macOS endpoint');
  }
  const [power, thermal, osBuild] = await Promise.all([
    execFile('pmset', ['-g', 'batt']),
    execFile('pmset', ['-g', 'therm']),
    execFile('sw_vers', ['-buildVersion']),
  ]);
  return {
    phase,
    power_source: parseMacPowerReceipt(power.stdout),
    thermal: parseMacThermalReceipt(thermal.stdout),
    os_build: osBuild.stdout.trim(),
  };
}

async function q1PackageSnapshot() {
  if (q1ArtifactRole === null) return null;
  const directory = resolve(repoRoot, q1WasmPackageDirectory);
  const files = {
    runtime_js: resolve(directory, 'gsplat_web.js'),
    runtime_wasm: resolve(directory, 'gsplat_web_bg.wasm'),
    package_manifest: resolve(directory, 'gsplat_web_build_receipt.json'),
  };
  return {
    directory,
    files,
    hashes: Object.fromEntries(await Promise.all(Object.entries(files).map(
      async ([name, path]) => [name, await sha256File(path)],
    ))),
  };
}

async function q1RepositorySnapshot(phase) {
  const [head, status] = await Promise.all([
    execFile('git', ['rev-parse', 'HEAD'], { cwd: repoRoot }),
    execFile('git', ['status', '--porcelain'], { cwd: repoRoot }),
  ]);
  return {
    schema: 'gsplat-q1-repository-snapshot/v1',
    phase,
    head: head.stdout.trim(),
    porcelain: status.stdout,
    clean: status.stdout.length === 0,
  };
}

async function q1ServedSourceSnapshot(phase) {
  if (q1ArtifactRole === null) return null;
  const entrypoints = [
    resolve(repoRoot, 'examples/web/index.html'),
    resolve(repoRoot, 'examples/web/styles.css'),
    resolve(repoRoot, 'examples/web/src/main.js'),
  ];
  const pending = [...entrypoints];
  const visited = new Set();
  const importPattern = /(?:import|export)\s+(?:[^"'()]*?\s+from\s+)?["']([^"']+)["']/g;
  while (pending.length > 0) {
    const path = pending.pop();
    if (visited.has(path)) continue;
    const relativePath = relative(repoRoot, path);
    if (relativePath === '' || relativePath === '..' || relativePath.startsWith(`..${sep}`)) {
      throw new Error('Q1 served first-party module escaped the repository');
    }
    visited.add(path);
    if (!/\.(?:js|mjs)$/.test(path)) continue;
    const source = await readFile(path, 'utf8');
    for (const match of source.matchAll(importPattern)) {
      const specifier = match[1].split('?')[0];
      if (!specifier.startsWith('.')) continue;
      pending.push(resolve(dirname(path), specifier));
    }
  }
  const files = Object.fromEntries(await Promise.all([...visited].sort().map(async (path) => [
    relative(repoRoot, path).split(sep).join('/'),
    await sha256File(path),
  ])));
  return {
    schema: 'gsplat-q1-served-first-party-sources/v1',
    phase,
    entrypoints: entrypoints.map((path) => relative(repoRoot, path).split(sep).join('/')),
    files,
    aggregate_sha256: createHash('sha256').update(JSON.stringify(files)).digest('hex'),
  };
}

function q1BuildArtifactReceipts(snapshot) {
  if (snapshot === null) return null;
  const artifacts = {};
  for (const [name, source] of Object.entries(snapshot.files)) {
    const destination = resolve(outDir, 'build', source.split(sep).at(-1));
    const digest = snapshot.hashes[name];
    const path = relative(q1SeriesRoot, destination);
    if (path === '' || path === '..' || path.startsWith(`..${sep}`)) {
      throw new Error(`Q1 ${name} artifact escaped the series root`);
    }
    artifacts[name] = { path: path.split(sep).join('/'), sha256: digest };
  }
  return artifacts;
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

async function runPythonValidator(script, args, label) {
  await new Promise((resolvePromise, reject) => {
    const child = spawn(
      process.env.PYTHON ?? 'python3',
      [resolve(repoRoot, script), ...args],
      { stdio: 'inherit' },
    );
    child.once('error', reject);
    child.once('exit', (code) => {
      if (code === 0) resolvePromise();
      else reject(new Error(`${label} exited ${code}`));
    });
  });
}

async function admitTruck1080pQualification() {
  const expected = validateTruck1080pCollectorConfig({
    qualificationName,
    dataset,
    geometryPath,
    orderBackend,
    projectedPolicy,
    gpuOrderProducer,
    sortInterval,
    benchmarkSync,
    m4Smoke,
    orderCompletionProtocol,
    benchmarkWindowMode,
    qualificationStage: truckQualificationStage,
    warmup,
    frames: requestedFrameCount,
    cameraTraceUrl: process.env.GSPLAT_CAMERA_TRACE_URL ?? null,
    cameraTraceSequence: process.env.GSPLAT_CAMERA_TRACE_SEQUENCE === '1',
    cameraTraceLoops: Number(process.env.GSPLAT_CAMERA_TRACE_LOOPS ?? 0),
    cameraFrame: optionalEnvironmentValue(process.env.GSPLAT_CAMERA_FRAME),
    cameraFrameIndices: (process.env.GSPLAT_CAMERA_FRAME_INDICES ?? '')
      .split(',')
      .filter((value) => value.length > 0)
      .map(Number),
  }, truckQualification);
  const outputRoot = dirname(outDir);
  const stage = truckQualificationStage === expected.control.stage
    ? expected.control
    : expected.throughput;
  const porcelain = (
    await execFile('git', ['status', '--porcelain'], { cwd: repoRoot })
  ).stdout;
  validateTruck1080pCleanWorkingTree(porcelain);
  if (q1ArtifactRole === null) {
    if (outDir !== resolve(outputRoot, stage.artifact_name)) {
      throw new Error(`Truck 1080p ${stage.stage} artifact must be named ${stage.artifact_name}`);
    }
    if (fullQualitySuitePath !== resolve(outputRoot, expected.suite_name)) {
      throw new Error(`Truck 1080p suite must be ${resolve(outputRoot, expected.suite_name)}`);
    }
    if (stage === expected.control) {
      if (currentStatsControlArtifact !== null) {
        throw new Error('Truck control stage forbids a current-stats control artifact input');
      }
      await claimTruck1080pOutputRoot(outputRoot);
      claimedTruckOutputRoot = outputRoot;
    } else {
      const expectedControlManifest = resolve(
        outputRoot,
        expected.control.artifact_name,
        'manifest.json',
      );
      const requestedControlManifest = resolve(repoRoot, currentStatsControlArtifact);
      if (requestedControlManifest !== expectedControlManifest) {
        throw new Error(`Truck throughput control artifact must be ${expectedControlManifest}`);
      }
      claimedTruckOutputRoot = outputRoot;
      truckControlCompletion = await claimTruck1080pThroughputStage({
        outputRoot,
        controlManifestPath: requestedControlManifest,
        expected,
      });
    }
  } else {
    if ((q1ArtifactRole === 'control') !== (stage === expected.control)) {
      throw new Error('Q1 artifact role and Truck qualification stage disagree');
    }
    if (await pathExists(outDir)) throw new Error(`Q1 output already exists: ${outDir}`);
  }

  const datasetPath = resolve(repoRoot, expected.dataset.local_path);
  const datasetStat = await stat(datasetPath);
  if (datasetStat.size !== expected.dataset.bytes) {
    throw new Error(
      `Truck source bytes mismatch: expected ${expected.dataset.bytes}, observed ${datasetStat.size}`,
    );
  }
  const datasetSha256 = await sha256File(datasetPath);
  if (datasetSha256 !== expected.dataset.sha256) {
    throw new Error(
      `Truck source SHA-256 mismatch: expected ${expected.dataset.sha256}, observed ${datasetSha256}`,
    );
  }

  const tracePath = resolve(repoRoot, expected.trace.local_path);
  const traceFileSha256 = await sha256File(tracePath);
  if (traceFileSha256 !== expected.trace.file_sha256) {
    throw new Error(
      `Truck trace file SHA-256 mismatch: expected ${expected.trace.file_sha256}, observed ${traceFileSha256}`,
    );
  }
  const trace = JSON.parse(await readFile(tracePath, 'utf8'));
  const derivation = trace.derivation ?? {};
  if (trace.trace_id !== expected.trace.id
      || trace.content_sha256 !== expected.trace.content_sha256
      || trace.display?.width !== expected.trace.width
      || trace.display?.height !== expected.trace.height
      || trace.frames?.length !== expected.trace.frame_count
      || derivation.source_path !== expected.dataset.local_path
      || derivation.source_sha256 !== expected.dataset.sha256
      || derivation.source_bytes !== expected.dataset.bytes
      || derivation.source_splat_count !== expected.dataset.splat_count
      || derivation.source_sh_degree !== expected.dataset.sh_degree) {
    throw new Error('Truck trace identity, display, source, count, or SH3 receipt is not canonical');
  }
  await runPythonValidator(
    'tests/perf/trace/validate_trace_v1.py',
    [tracePath],
    'camera trace validator',
  );
}

async function q1WasmPackageUrl(repositoryCommit) {
  if (q1ArtifactRole === null) return null;
  if (q1WasmPackageDirectory === null) {
    throw new Error('Q1 producer requires GSPLAT_Q1_WASM_PACKAGE_DIR');
  }
  const directory = resolve(repoRoot, q1WasmPackageDirectory);
  const relativeDirectory = relative(repoRoot, directory);
  if (relativeDirectory === '' || relativeDirectory === '..'
      || relativeDirectory.startsWith(`..${sep}`)) {
    throw new Error('Q1 WASM package must be a fresh directory inside the repository');
  }
  const receipt = JSON.parse(await readFile(
    resolve(directory, 'gsplat_web_build_receipt.json'),
    'utf8',
  ));
  if (receipt.schema !== 'gsplat-web-diagnostic-build/v1'
      || receipt.profile !== 'quality-exact'
      || receipt.repository_commit !== repositoryCommit
      || receipt.dirty !== false
      || receipt.js_sha256 !== await sha256File(resolve(directory, 'gsplat_web.js'))
      || receipt.wasm_sha256 !== await sha256File(resolve(directory, 'gsplat_web_bg.wasm'))) {
    throw new Error('Q1 WASM package is not a clean same-commit quality-exact build');
  }
  return `/${relativeDirectory.split(sep).join('/')}`;
}

async function findChrome() {
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {
      // try next
    }
  }
  return null;
}

function startHttpServer() {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(
      process.env.PYTHON ?? 'python3',
      ['-m', 'http.server', String(port), '--bind', '127.0.0.1', '--directory', repoRoot],
      { cwd: repoRoot, stdio: ['ignore', 'pipe', 'pipe'] }
    );
    let settled = false;
    const fail = (error) => {
      if (!settled) {
        settled = true;
        void (async () => {
          try {
            if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
            await waitForChildExit(child, 2_000, 'failed HTTP server');
            reject(error);
          } catch (cleanupError) {
            reject(new AggregateError(
              [error, cleanupError],
              'HTTP server startup and cleanup failed',
            ));
          }
        })();
      }
    };
    child.once('error', fail);
    child.stderr.on('data', (chunk) => {
      const text = chunk.toString();
      if (text.includes('Address already in use')) {
        fail(new Error(text.trim()));
      }
    });
    setTimeout(() => {
      if (!settled) {
        settled = true;
        resolvePromise(child);
      }
    }, 400);
  });
}

function validateCollectorDatasetIdentity({ manifestDataset, loadReceipt = null }) {
  if (formalDatasetLogicalId !== null) {
    return validateFormalDatasetEvidenceIdentity({
      requestedLogicalId: dataset,
      expectedLogicalId: formalDatasetLogicalId,
      manifestDataset,
      loadReceipt,
    });
  }
  return validateDatasetEvidenceIdentity({
    requestedDataset: dataset,
    manifestDataset,
    loadReceipt,
  });
}

function parseArtifacts(consoleLines) {
  const manifests = [];
  const frameRecords = [];
  const summaries = [];
  const orderMeasurements = [];
  const cpuOrderMeasurements = [];
  const orderMeasurementSubmissions = [];
  const orderMeasurementFailures = [];
  const currentStatsSubmissions = [];
  const currentStatsTerminals = [];
  const currentStatsPendingFrames = [];
  const adaptiveGpuFailures = [];
  const loadReceipts = [];
  const gpuOrderPreparations = [];
  const monotonicOrderingWindows = [];
  const projectedMeasurements = [];
  const projectedMeasurementSubmissions = [];
  const projectedMeasurementFailures = [];
  const gpuProducerMeasurements = [];
  const gpuProducerMeasurementSubmissions = [];
  const gpuProducerMeasurementFailures = [];
  for (const line of consoleLines) {
    const text = line.includes(': ') ? line.slice(line.indexOf(': ') + 2) : line;
    if (text.startsWith('BENCHMARK_MANIFEST_JSON ')) {
      manifests.push(text.slice('BENCHMARK_MANIFEST_JSON '.length));
    } else if (text.startsWith('BENCHMARK_FRAME_JSON ')) {
      frameRecords.push(text.slice('BENCHMARK_FRAME_JSON '.length));
    } else if (text.startsWith('BENCHMARK_SUMMARY_JSON ')) {
      summaries.push(text.slice('BENCHMARK_SUMMARY_JSON '.length));
    } else if (text.startsWith('ORDER_MEASUREMENT_JSON ')) {
      orderMeasurements.push(text.slice('ORDER_MEASUREMENT_JSON '.length));
    } else if (text.startsWith('CPU_ORDER_MEASUREMENT_JSON ')) {
      cpuOrderMeasurements.push(text.slice('CPU_ORDER_MEASUREMENT_JSON '.length));
    } else if (text.startsWith('ORDER_MEASUREMENT_SUBMISSION_JSON ')) {
      orderMeasurementSubmissions.push(text.slice('ORDER_MEASUREMENT_SUBMISSION_JSON '.length));
    } else if (text.startsWith('ORDER_MEASUREMENT_FAILURE_JSON ')) {
      orderMeasurementFailures.push(text.slice('ORDER_MEASUREMENT_FAILURE_JSON '.length));
    } else if (text.startsWith('CURRENT_STATS_SUBMISSION_JSON ')) {
      currentStatsSubmissions.push(text.slice('CURRENT_STATS_SUBMISSION_JSON '.length));
    } else if (text.startsWith('CURRENT_STATS_TERMINAL_JSON ')) {
      currentStatsTerminals.push(text.slice('CURRENT_STATS_TERMINAL_JSON '.length));
    } else if (text.startsWith('CURRENT_STATS_PENDING_FRAME_JSON ')) {
      currentStatsPendingFrames.push(text.slice('CURRENT_STATS_PENDING_FRAME_JSON '.length));
    } else if (text.startsWith('ADAPTIVE_GPU_FAILURE_JSON ')) {
      adaptiveGpuFailures.push(text.slice('ADAPTIVE_GPU_FAILURE_JSON '.length));
    } else if (text.startsWith('SCENE_LOAD_RECEIPT_JSON ')) {
      loadReceipts.push(text.slice('SCENE_LOAD_RECEIPT_JSON '.length));
    } else if (text.startsWith('GPU_ORDER_PREPARATION_JSON ')) {
      gpuOrderPreparations.push(text.slice('GPU_ORDER_PREPARATION_JSON '.length));
    } else if (text.startsWith('ORDERING_WINDOW_MONOTONIC_JSON ')) {
      monotonicOrderingWindows.push(text.slice('ORDERING_WINDOW_MONOTONIC_JSON '.length));
    } else if (text.startsWith('PROJECTED_MEASUREMENT_JSON ')) {
      projectedMeasurements.push(text.slice('PROJECTED_MEASUREMENT_JSON '.length));
    } else if (text.startsWith('PROJECTED_MEASUREMENT_SUBMISSION_JSON ')) {
      projectedMeasurementSubmissions.push(
        text.slice('PROJECTED_MEASUREMENT_SUBMISSION_JSON '.length),
      );
    } else if (text.startsWith('PROJECTED_MEASUREMENT_FAILURE_JSON ')) {
      projectedMeasurementFailures.push(
        text.slice('PROJECTED_MEASUREMENT_FAILURE_JSON '.length),
      );
    } else if (text.startsWith('GPU_PRODUCER_MEASUREMENT_JSON ')) {
      gpuProducerMeasurements.push(
        text.slice('GPU_PRODUCER_MEASUREMENT_JSON '.length),
      );
    } else if (text.startsWith('GPU_PRODUCER_MEASUREMENT_SUBMISSION_JSON ')) {
      gpuProducerMeasurementSubmissions.push(
        text.slice('GPU_PRODUCER_MEASUREMENT_SUBMISSION_JSON '.length),
      );
    } else if (text.startsWith('GPU_PRODUCER_MEASUREMENT_FAILURE_JSON ')) {
      gpuProducerMeasurementFailures.push(
        text.slice('GPU_PRODUCER_MEASUREMENT_FAILURE_JSON '.length),
      );
    } else if (text.startsWith('TERMINAL_QUEUE_FENCE_')) {
      throw new Error('retired terminal queue fence evidence is not admissible');
    }
  }
  if (manifests.length !== 1 || summaries.length !== 1 || frameRecords.length === 0) {
    throw new Error(
      `expected one manifest/summary and frames; got manifest=${manifests.length} frame=${frameRecords.length} summary=${summaries.length}`
    );
  }
  for (const payload of [
    ...manifests,
    ...frameRecords,
    ...summaries,
    ...orderMeasurements,
    ...cpuOrderMeasurements,
    ...orderMeasurementSubmissions,
    ...orderMeasurementFailures,
    ...currentStatsSubmissions,
    ...currentStatsTerminals,
    ...currentStatsPendingFrames,
    ...adaptiveGpuFailures,
    ...loadReceipts,
    ...gpuOrderPreparations,
    ...monotonicOrderingWindows,
    ...projectedMeasurements,
    ...projectedMeasurementSubmissions,
    ...projectedMeasurementFailures,
    ...gpuProducerMeasurements,
    ...gpuProducerMeasurementSubmissions,
    ...gpuProducerMeasurementFailures,
  ]) {
    JSON.parse(payload);
  }
  const manifest = JSON.parse(manifests[0]);
  const rawFrames = frameRecords.map((payload) => JSON.parse(payload));
  const measurements = orderMeasurements.map((payload) => JSON.parse(payload));
  const cpuMeasurements = cpuOrderMeasurements.map((payload) => JSON.parse(payload));
  const submissions = orderMeasurementSubmissions.map((payload) => JSON.parse(payload));
  const failures = orderMeasurementFailures.map((payload) => JSON.parse(payload));
  const statsSubmissions = currentStatsSubmissions.map((payload) => JSON.parse(payload));
  const statsTerminals = currentStatsTerminals.map((payload) => JSON.parse(payload));
  const statsPendingFrames = currentStatsPendingFrames.map((payload) => JSON.parse(payload));
  const preparations = gpuOrderPreparations.map((payload) => JSON.parse(payload));
  const projectedSuccesses = projectedMeasurements
    .map((payload) => JSON.parse(payload))
    .filter((record) => record.run_id === manifest.run_id);
  const projectedSubmissions = projectedMeasurementSubmissions
    .map((payload) => JSON.parse(payload))
    .filter((record) => record.run_id === manifest.run_id);
  const projectedFailures = projectedMeasurementFailures
    .map((payload) => JSON.parse(payload))
    .filter((record) => record.run_id === manifest.run_id);
  const producerMeasurements = gpuProducerMeasurements
    .map((payload) => JSON.parse(payload))
    .filter((record) => record.run_id === manifest.run_id);
  const producerSubmissions = gpuProducerMeasurementSubmissions
    .map((payload) => JSON.parse(payload))
    .filter((record) => record.run_id === manifest.run_id);
  const producerFailures = gpuProducerMeasurementFailures
    .map((payload) => JSON.parse(payload))
    .filter((record) => record.run_id === manifest.run_id);
  if (monotonicOrderingWindows.length !== 1) {
    throw new Error(
      `expected one final monotonic ordering window; observed ${monotonicOrderingWindows.length}`,
    );
  }
  const rendererOwnedExact = rawFrames.every(
    (frame) => frame.raster_execution_plan === 'projected_quads_exact',
  );
  const terminalQueueThroughput = manifest.benchmark_window?.mode
    === 'terminal_queue_throughput_window';
  if (rendererOwnedExact) {
    const expectedLogicalFrameCount = manifest.ordering_window?.expected_logical_frame_count;
    const logicalCurrentStatsSubmissionCount = terminalQueueThroughput
      ? statsSubmissions.length
      : statsSubmissions.filter(
          (submission) => submission.phase === 'warmup' || submission.phase === 'measured',
        ).length;
    const benchmarkWindowConfiguration = manifest.benchmark_window?.configuration;
    if (!benchmarkWindowConfiguration || typeof benchmarkWindowConfiguration !== 'object') {
      throw new Error('renderer-owned Exact artifact lacks benchmark window configuration');
    }
    const configurationSha256 = createHash('sha256')
      .update(JSON.stringify(benchmarkWindowConfiguration))
      .digest('hex');
    validateBenchmarkWindowManifest({
      window: manifest.benchmark_window,
      currentStatsSubmissionCount: logicalCurrentStatsSubmissionCount,
      expectedLogicalFrameCount: terminalQueueThroughput
        ? rawFrames.length
        : expectedLogicalFrameCount,
      expectedWarmupFrameCount: terminalQueueThroughput
        ? benchmarkWindowConfiguration.warmup_frames
        : 0,
      expectedConfigurationSha256: configurationSha256,
    });
    validateAuxiliaryCurrentStatsFormalLedger({
      runId: manifest.run_id,
      expectedLogicalFrameCount: manifest.ordering_window?.expected_logical_frame_count,
      deferredPresentationCount:
        manifest.ordering_window?.current_stats_schedule?.deferred_presentation_count ?? 0,
      deferredPresentations:
        manifest.ordering_window?.current_stats_schedule?.deferred_presentations ?? [],
      submissions: manifest.ordering_window?.auxiliary_formal_submissions,
      terminals: manifest.ordering_window?.auxiliary_formal_terminals,
      terminalQueueThroughput,
    });
    if (terminalQueueThroughput) {
      if (manifest.benchmark_window?.control_artifact_identity?.run_id
            !== currentStatsControlIdentity?.runId
          || manifest.benchmark_window?.control_artifact_identity?.configuration_sha256
            !== currentStatsControlIdentity?.configurationSha256) {
        throw new Error('throughput artifact drifted from its admitted current-stats control');
      }
      const expectedBoundaryReceiptCount = warmup > 0 ? 2 : 1;
      if (statsSubmissions.length !== expectedBoundaryReceiptCount
          || statsTerminals.length !== expectedBoundaryReceiptCount
          || statsPendingFrames.length > expectedBoundaryReceiptCount
          || manifest.ordering_window?.current_stats_schedule !== null
          || manifest.ordering_window?.terminal_current_stats_receipts
            !== expectedBoundaryReceiptCount
          || manifest.timing?.performance_evidence !== true
          || manifest.qualification_scope
            !== 'qualification_q1_terminal_queue_throughput_candidate') {
        throw new Error(
          'terminal-queue throughput requires one untimed warmup boundary and one final receipt',
        );
      }
      const warmupSubmission = warmup > 0
        ? statsSubmissions.find((record) => record.phase === 'warmup_boundary')
        : null;
      const warmupTerminal = warmup > 0
        ? statsTerminals.find((record) => record.phase === 'warmup_boundary')
        : null;
      const finalSubmission = statsSubmissions.find(
        (record) => record.phase === 'final_measured',
      );
      const finalTerminal = statsTerminals.find(
        (record) => record.phase === 'final_measured',
      );
      const finalFrame = rawFrames.at(-1);
      const nonFinalFrames = rawFrames.slice(0, -1);
      const expectedPresented = warmup + requestedFrameCount;
      const currentStatsIdentityFields = [
        'current_stats_ticket',
        'current_stats_plan',
        'current_stats_scene_generation',
        'current_stats_camera_revision',
        'current_stats_viewport_generation',
        'current_stats_contract_generation',
        'current_stats_plan_set_generation',
        'current_stats_order_generation',
        'current_stats_raster_generation',
        'current_stats_encode_attempt',
        'current_stats_presentation_sequence',
      ];
      if (nonFinalFrames.some((frame) => frame.current_stats_submission !== 'not_requested'
          || currentStatsIdentityFields.some((field) => frame[field] !== null))) {
        throw new Error('throughput requested current stats before its final measured frame');
      }
      if (!finalSubmission || !finalTerminal
          || finalFrame.current_stats_submission !== 'issued'
          || finalFrame.current_stats_ticket !== finalSubmission.ticket
          || finalTerminal.ticket !== finalSubmission.ticket
          || finalTerminal.status !== 'ready'
          || manifest.benchmark_window?.final_measured_current_stats_ticket
            !== finalSubmission.ticket
          || manifest.benchmark_window?.terminal_receipt?.ticket !== finalSubmission.ticket
          || manifest.benchmark_window?.terminal_receipt?.status !== 'ready'
          || manifest.benchmark_window?.terminal_receipt?.plan !== finalTerminal.plan
          || manifest.benchmark_window?.terminal_receipt?.submitted_at_monotonic_ms
            !== finalSubmission.submitted_at_monotonic_ms
          || manifest.benchmark_window?.terminal_receipt?.terminal_at_monotonic_ms
            !== finalTerminal.terminal_at_monotonic_ms
          || manifest.benchmark_window?.terminal_receipt?.requested_at_monotonic_ms
            > finalSubmission.submitted_at_monotonic_ms
          || finalTerminal.terminal_at_monotonic_ms
            < finalSubmission.submitted_at_monotonic_ms
          || manifest.benchmark_window?.last_measured_submit_monotonic_ms
            !== finalSubmission.submitted_at_monotonic_ms
          || manifest.benchmark_window?.last_measured_terminal_monotonic_ms
            !== finalTerminal.terminal_at_monotonic_ms) {
        throw new Error('final measured current-stats receipt lacks an exact issued-terminal join');
      }
      if (warmup > 0 && (
        !warmupSubmission || !warmupTerminal
        || warmupTerminal.ticket !== warmupSubmission.ticket
        || warmupTerminal.status !== 'ready'
        || manifest.benchmark_window?.warmup_boundary_current_stats_ticket
          !== warmupSubmission.ticket
        || manifest.benchmark_window?.warmup_terminal_receipt?.ticket
          !== warmupSubmission.ticket
        || manifest.benchmark_window?.warmup_terminal_receipt?.status !== 'ready'
        || manifest.benchmark_window?.warmup_terminal_receipt?.plan !== warmupTerminal.plan
        || manifest.benchmark_window?.warmup_terminal_receipt?.submitted_at_monotonic_ms
          !== warmupSubmission.submitted_at_monotonic_ms
        || manifest.benchmark_window?.warmup_terminal_receipt?.terminal_at_monotonic_ms
          !== warmupTerminal.terminal_at_monotonic_ms
        || manifest.benchmark_window?.warmup_terminal_receipt?.requested_at_monotonic_ms
          > warmupSubmission.submitted_at_monotonic_ms
        || warmupTerminal.terminal_at_monotonic_ms
          < warmupSubmission.submitted_at_monotonic_ms
        || warmupTerminal.terminal_at_monotonic_ms
          > manifest.benchmark_window?.first_measured_input_monotonic_ms
        || warmupSubmission.ticket === finalSubmission.ticket
      )) {
        throw new Error('warmup boundary lacks an untimed issued-terminal join before measured input');
      }
      if (manifest.benchmark_window?.warmup_submit_count !== warmup
          || manifest.benchmark_window?.measured_submit_count !== requestedFrameCount
          || (warmup > 0
            && manifest.benchmark_window?.draw_count_at_warmup_drain_start !== warmup)
          || manifest.benchmark_window?.draw_count_at_warmup_drain_start
            !== manifest.benchmark_window?.draw_count_at_warmup_drain_completion
          || manifest.benchmark_window?.draw_count_at_final_drain_start !== expectedPresented
          || manifest.benchmark_window?.draw_count_at_completion !== expectedPresented
          || manifest.ordering_window?.presented_submit_count !== expectedPresented) {
        throw new Error('final current-stats drain submitted a new draw or lost a logical frame');
      }
      if (rawFrames.some((frame) => (!terminalFixedGpuCompactCell
            && frame.adaptive_state === 'disabled')
          || frame.submitted_measurement_ticket !== null
          || frame.submitted_measurement_backend !== null
          || frame.projected_measurement_submission !== 'not_requested'
          || frame.projected_measurement_ticket !== null
          || frame.gpu_producer_measurement_submission !== 'not_requested'
          || frame.gpu_producer_measurement_ticket !== null
          || !['cpu', 'gpu'].includes(frame.order_backend)
          || !['candidate', 'compact'].includes(frame.projected_execution))) {
        throw new Error('throughput frame lacks a natural Exact WholePlanController identity');
      }
      if (terminalFixedGpuCompactCell && rawFrames.some((frame) =>
        frame.adaptive_state !== 'disabled'
          || frame.projected_adaptive_state !== 'disabled'
          || frame.order_backend !== 'gpu'
          || frame.projected_execution !== 'compact'
          || frame.gpu_order_producer !== 'preproject')) {
        throw new Error('fixed GPU preproject Compact throughput execution drifted');
      }
      const adaptiveRecords = manifest.benchmark_window?.exact_adaptive_measured;
      if (!Array.isArray(adaptiveRecords) || adaptiveRecords.length !== rawFrames.length) {
        throw new Error('throughput window lacks its complete Exact adaptive state/plan ledger');
      }
      for (const [index, frame] of rawFrames.entries()) {
        const actualPlan = frame.order_backend === 'cpu'
          ? 'cpu_post_sort'
          : frame.projected_execution === 'compact'
            ? 'gpu_preproject'
            : 'gpu_post_sort';
        const adaptive = adaptiveRecords[index];
        if (adaptive.state !== frame.adaptive_state || adaptive.plan !== actualPlan
            || adaptive.projected_state !== frame.projected_adaptive_state
            || adaptive.projected_execution !== frame.projected_execution) {
          throw new Error(`throughput frame ${index} Exact adaptive plan identity drift`);
        }
      }
    } else {
      const currentStatsScheduleEvidence = validateCurrentStatsScheduleEvidence({
        evidence: manifest.ordering_window?.current_stats_schedule,
        protocol: manifest.ordering_window?.completion_protocol,
        frameWallSource: manifest.timing?.frame_wall_source,
        expectedLogicalFrameCount,
        expectedWarmupFrameCount: benchmarkWindowConfiguration.warmup_frames,
      });
      if (currentStatsScheduleEvidence.issued_count !== statsSubmissions.length
          || currentStatsScheduleEvidence.terminal_count !== statsTerminals.length
          || currentStatsScheduleEvidence.presented_attempt_count
            !== manifest.ordering_window?.presented_submit_count) {
        throw new Error(
          'current-stats schedule counts do not match the retained submission/terminal ledger',
        );
      }
      validateCurrentStatsAttemptSubmissionJoin({
        issuedPresentations: currentStatsScheduleEvidence.issued_presentations,
        submissions: statsSubmissions,
      });
      if (manifest.timing?.performance_evidence !== false
          || manifest.qualification_scope !== 'qualification_q1_current_stats_control_only') {
        throw new Error(
          'renderer current-stats evidence window overclaims cross-implementation performance',
        );
      }
    }
  }
  const terminalSubmissions = rendererOwnedExact
    ? statsSubmissions
    : submissions;
  const terminalReceipts = rendererOwnedExact
    ? statsTerminals
    : [...measurements, ...cpuMeasurements, ...failures];
  const pageMonotonicWindow = JSON.parse(monotonicOrderingWindows[0]);
  const monotonicWindow = terminalQueueThroughput
      ? Object.fromEntries([
        'monotonic_clock',
        'first_measured_input_monotonic_ms',
        'first_measured_submit_monotonic_ms',
        'last_measured_submit_monotonic_ms',
        'last_measured_terminal_monotonic_ms',
        'input_to_first_submit_ms',
        'submit_span_ms',
        'terminal_tail_ms',
        'terminal_window_ms',
      ].map((field) => [
        field,
        field === 'monotonic_clock'
          ? 'performance.now'
          : manifest.benchmark_window[field],
      ]))
    : monotonicOrderingWindow({
        submissions: terminalSubmissions,
        terminals: terminalReceipts,
      });
  const measuredSubmissionCount = terminalQueueThroughput
    ? rawFrames.length
    : terminalSubmissions.filter((submission) => submission.phase === 'measured').length;
  if (pageMonotonicWindow.measured_submit_count !== measuredSubmissionCount
      || pageMonotonicWindow.measured_terminal_count
        !== (terminalQueueThroughput ? 1 : measuredSubmissionCount)
      || (terminalQueueThroughput
        && pageMonotonicWindow.terminal_model
          !== 'untimed_warmup_boundary_plus_final_measured_current_stats_receipt')) {
    throw new Error('page monotonic ordering window has an incorrect measured ledger count');
  }
  for (const [field, value] of Object.entries(monotonicWindow)) {
    if (pageMonotonicWindow[field] !== value) {
      throw new Error(
        `page/collector monotonic ordering window mismatch for ${field}: ` +
        `${pageMonotonicWindow[field]} vs ${value}`,
      );
    }
  }
  for (const preparation of preparations) {
    if (preparation.frame_presented !== false
        || preparation.gpu_order_preparation_pending !== true
        || preparation.submitted_measurement_ticket !== null
        || preparation.submitted_measurement_backend !== null
        || preparation.projected_measurement_submission === 'issued'
        || preparation.projected_measurement_ticket !== null
        || preparation.gpu_producer_measurement_submission === 'issued'
        || preparation.gpu_producer_measurement_ticket !== null) {
      throw new Error(
        'GPU order preparation must be hidden, pending, and expose no order/projected identity',
      );
    }
  }
  if (orderBackend !== 'cpu' && adaptiveGpuFailures.length > 0) {
    const failure = JSON.parse(adaptiveGpuFailures[0]);
    throw new Error(`strict benchmark observed adaptive GPU failure ${failure.reason ?? 'unknown'}`);
  }
  const expectedIdentity = validateCollectorDatasetIdentity({
    manifestDataset: manifest.dataset,
  });
  const expected = expectedIdentity.id;
  if (manifest.renderer?.order_backend_requested !== orderBackend) {
    throw new Error(
      `benchmark backend mismatch: requested ${orderBackend}, observed ` +
      `${manifest.renderer?.order_backend_requested ?? 'missing'}`
    );
  }
  if (manifest.renderer?.projected_policy_requested !== projectedPolicy) {
    throw new Error(
      `benchmark projected policy mismatch: requested ${projectedPolicy}, observed ` +
      `${manifest.renderer?.projected_policy_requested ?? 'missing'}`,
    );
  }
  if (manifest.renderer?.gpu_order_producer_requested !== gpuOrderProducer) {
    throw new Error(
      `benchmark GPU producer mismatch: requested ${gpuOrderProducer ?? 'default'}; observed ` +
      `${manifest.renderer?.gpu_order_producer_requested ?? 'default'}`,
    );
  }
  const hasGpuFrame = rawFrames.some((frame) => frame.order_backend === 'gpu');
  const fixedGpuPreprojectCompactCell = truckQualification?.order_backend === 'gpu'
    && truckQualification?.projected_policy === 'compact';
  const expectedActualProducer = fixedGpuPreprojectCompactCell
    ? 'preproject'
    : gpuOrderProducer ?? (hasGpuFrame ? 'post-sort' : null);
  const frameActualProducers = [
    ...new Set(rawFrames.map((frame) => frame.gpu_order_producer).filter(Boolean)),
  ].sort();
  const manifestActualProducers = [
    ...(manifest.gpu_producer_evidence?.actual_producers ?? []),
  ].sort();
  if (terminalQueueThroughput) {
    if (JSON.stringify(frameActualProducers) !== JSON.stringify(manifestActualProducers)
        || (fixedGpuPreprojectCompactCell
          && JSON.stringify(frameActualProducers) !== JSON.stringify(['preproject']))) {
      throw new Error('throughput Exact actual-plan producer ledger drifted from its frames');
    }
  } else if (manifest.renderer?.gpu_order_producer_actual !== expectedActualProducer) {
    throw new Error(
      `benchmark actual GPU producer mismatch: expected ${expectedActualProducer ?? 'none'}; observed ` +
      `${manifest.renderer?.gpu_order_producer_actual ?? 'missing'}`,
    );
  }
  if (geometryPath === 'packed') {
    if (loadReceipts.length !== 1) {
      throw new Error(`exact Packed benchmark requires one load receipt; observed ${loadReceipts.length}`);
    }
    const receipt = JSON.parse(loadReceipts[0]);
    validateCollectorDatasetIdentity({
      manifestDataset: manifest.dataset,
      loadReceipt: receipt,
    });
    const count = manifest.dataset?.splat_count;
    if (!receipt.streamed || receipt.source_count !== count || receipt.decoded_count !== count
        || receipt.encoded_count !== count || receipt.resident_count !== count
        || receipt.addressable_count !== count
        || receipt.source_sh_degree !== manifest.dataset?.sh_degree
        || receipt.resident_sh_degree !== manifest.dataset?.sh_degree
        || receipt.full_quality !== true || receipt.source_membership !== 'all'
        || receipt.sampling_enabled !== false || receipt.lod_enabled !== false
        || receipt.partial_scene_published !== false) {
      throw new Error(
        'load receipt does not prove exact source=decoded=encoded=resident=addressable count and source SH',
      );
    }
    const exactness = manifest.exactness;
    if (!exactness
        || exactness.source_splat_count !== count
        || exactness.decoded_splat_count !== count
        || exactness.encoded_splat_count !== count
        || exactness.resident_splat_count !== count
        || exactness.addressable_splat_count !== count
        || exactness.source_sh_degree !== manifest.dataset?.sh_degree
        || exactness.resident_sh_degree !== manifest.dataset?.sh_degree
        || exactness.source_membership !== 'all'
        || exactness.sampling !== 'disabled'
        || exactness.lod !== 'disabled'
        || exactness.sh_degree_policy !== 'source'
        || exactness.partial_scene_published !== false
        || exactness.full_quality !== true) {
      throw new Error('manifest exactness receipt does not prove complete point/SH residency');
    }
  }
  if (rendererOwnedExact) {
    validateCurrentStatsTerminalLedger({
      submissions: statsSubmissions,
      terminals: statsTerminals,
      sourceCount: manifest.dataset?.splat_count,
    });
    if (terminalQueueThroughput) {
      // Reuse the canonical join on the one observed final frame so every
      // renderer-owned ticket/plan/generation identity is checked across the
      // frame, submission, and Result-bearing terminal receipt.
      joinCurrentStatsEvidence({
        frames: [rawFrames.at(-1)],
        submissions: statsSubmissions,
        terminals: statsTerminals,
        sourceCount: manifest.dataset?.splat_count,
      });
    }
    if (submissions.length > 0 || measurements.length > 0
        || cpuMeasurements.length > 0 || failures.length > 0) {
      throw new Error('renderer-owned Exact benchmark emitted a retired order-measurement ledger');
    }
    for (const pending of statsPendingFrames) {
      if (pending.visible !== null || pending.drawn !== null
          || pending.visible_count_pending !== true) {
        throw new Error(`pending Exact current-stats ticket ${pending.ticket} exposed numeric V/D`);
      }
      const terminal = statsTerminals.find((candidate) => candidate.ticket === pending.ticket);
      if (!terminal || terminal.camera_revision !== pending.camera_revision) {
        throw new Error(`pending Exact current-stats ticket ${pending.ticket} lacks its terminal join`);
      }
    }
    for (const terminal of statsTerminals) {
      if (terminal.count_semantics.startsWith('indirect_')
          && !statsPendingFrames.some((pending) => pending.ticket === terminal.ticket)) {
        throw new Error(`indirect Exact ticket ${terminal.ticket} did not preserve pending V/D`);
      }
    }
  } else {
    validateTerminalTicketLedger({ submissions, measurements, cpuMeasurements, failures });
  }
  validateProjectedFrameEvidence({ requestedPolicy: projectedPolicy, frames: rawFrames });
  if (!rendererOwnedExact) {
    validateGpuProducerFrameEvidence({ requestedProducer: gpuOrderProducer, frames: rawFrames });
  }
  if (!rendererOwnedExact && gpuOrderProducer !== null) {
    validateGpuProducerMeasuredSubmissions({
      frames: rawFrames,
      submissions: producerSubmissions,
    });
  }
  const projectedLedger = validateProjectedTerminalLedger({
    submissions: projectedSubmissions,
    measurements: projectedSuccesses,
    failures: projectedFailures,
  });
  if (projectedFailures.length > 0) {
    throw new Error(
      `strict benchmark observed projected failure ${projectedFailures[0].reason ?? 'unknown'}`,
    );
  }
  let producerLedger = {
    issued_count: 0,
    success_count: 0,
    failure_count: 0,
  };
  if (rendererOwnedExact) {
    if (producerSubmissions.length > 0
        || producerMeasurements.length > 0
        || producerFailures.length > 0) {
      throw new Error('renderer-owned Exact benchmark emitted a retired producer ticket ledger');
    }
  } else if (gpuOrderProducer === null) {
    if (producerSubmissions.length > 0
        || producerMeasurements.length > 0
        || producerFailures.length > 0) {
      throw new Error('unrequested GPU producer emitted a diagnostic terminal ledger');
    }
  } else {
    producerLedger = validateGpuProducerTerminalLedger({
      requestedProducer: gpuOrderProducer,
      sourceCount: manifest.dataset?.splat_count,
      submissions: producerSubmissions,
      measurements: producerMeasurements,
      failures: producerFailures,
    });
    if (producerFailures.length > 0) {
      throw new Error(
        `strict benchmark observed GPU producer failure ` +
        `${producerFailures[0].reason ?? 'unknown'}`,
      );
    }
  }
  const gpuFailures = failures.filter((failure) => failure.actual_backend === 'gpu');
  const admittedFrames = rendererOwnedExact
    ? terminalQueueThroughput
      ? rawFrames.map((frame) => ({
          ...frame,
          count_statistics_eligible: false,
        }))
      : joinCurrentStatsEvidence({
        frames: rawFrames,
        submissions: statsSubmissions,
        terminals: statsTerminals,
        sourceCount: manifest.dataset?.splat_count,
      })
    : joinOrderingEvidence({
        requestedBackend: orderBackend,
        frames: rawFrames,
        measurements,
        cpuMeasurements,
        failures: gpuFailures,
      });
  if (rendererOwnedExact && !terminalQueueThroughput) {
    validateCurrentStatsEvidence({ frames: admittedFrames });
    const fixedGpuPreprojectCompactControl = truckQualification?.order_backend === 'gpu'
      && truckQualification?.projected_policy === 'compact';
    if (fixedGpuPreprojectCompactControl) {
      validateTruck1080pFixedCompactControlEvidence({
        terminals: statsTerminals,
        frames: admittedFrames,
      });
    }
    if (gpuOrderProducer !== null || fixedGpuPreprojectCompactControl) {
      const requiredPlan = fixedGpuPreprojectCompactControl || gpuOrderProducer === 'preproject'
        ? 'gpu_preproject'
        : 'gpu_post_sort';
      if (statsTerminals.some((terminal) => terminal.plan !== requiredPlan)
          || (gpuOrderProducer !== null && admittedFrames.some(
            (frame) => frame.gpu_order_producer !== gpuOrderProducer,
          ))) {
        throw new Error(
          `renderer current-stats terminals do not prove required plan ${requiredPlan}`,
        );
      }
    }
  } else if (!rendererOwnedExact) {
    validateOrderingEvidence({
      requestedBackend: orderBackend,
      frames: admittedFrames,
      measurements,
      failures: gpuFailures,
      fixedCameraReuse: manifest.trace?.frame_index != null,
    });
  }
  const summary = benchmarkSummaryFromFrameRecords(
    admittedFrames,
    JSON.parse(summaries[0]),
  );
  summary.count_evidence = terminalQueueThroughput
    ? {
        source: 'bound_current_stats_control_artifact',
        control_artifact_identity: manifest.benchmark_window.control_artifact_identity,
      }
    : benchmarkCountEvidence(admittedFrames);
  summary.benchmark_window = manifest.benchmark_window ?? null;
  const measuredSubmissions = terminalQueueThroughput
    ? rawFrames.map((_, index) => ({ ticket: index + 1, phase: 'measured' }))
    : terminalSubmissions.filter((submission) => submission.phase === 'measured');
  const traceSequence = Array.isArray(manifest.trace?.frame_indices);
  if (traceSequence) {
    if (rawFrames.some((frame) => frame.sort_refreshed !== true)) {
      throw new Error('trace-sequence evidence contains a measured frame without a sort refresh');
    }
    if (measuredSubmissions.length !== rawFrames.length) {
      throw new Error(
        `trace-sequence submission off-by-one: measured submissions=${measuredSubmissions.length} ` +
        `frames=${rawFrames.length}`,
      );
    }
  }
  const measuredTickets = new Set(
    measuredSubmissions.map((submission) => submission.ticket),
  );
  const successfulTerminalTickets = new Set(
    terminalReceipts
      .filter((terminal) => !rendererOwnedExact || terminal.status === 'ready')
      .map((terminal) => terminal.ticket),
  );
  const measuredTerminalCount = terminalQueueThroughput
    ? statsTerminals.filter((terminal) => terminal.phase === 'final_measured').length
    : measuredSubmissions.filter(
        (submission) => successfulTerminalTickets.has(submission.ticket),
      ).length;
  if (terminalQueueThroughput ? measuredTerminalCount !== 1
    : measuredTerminalCount !== measuredSubmissions.length) {
    throw new Error(
      `measured queue-terminal off-by-one: submissions=${measuredSubmissions.length} ` +
      `terminals=${measuredTerminalCount}`,
    );
  }
  const orderingWindow = {
    completion_protocol: orderCompletionProtocol,
    performance_timing_source: 'page_performance.now_monotonic',
    utc_timestamps_are_identity_only: true,
    window_start_utc: manifest.identity?.measurement_started_at_utc ?? null,
    last_sample_recorded_utc: manifest.identity?.measurement_ended_at_utc ?? null,
    terminal_collected_at_utc: new Date().toISOString(),
    sample_count: rawFrames.length,
    measured_submit_count: measuredSubmissions.length,
    terminal_queue_done_count: measuredTerminalCount,
    warmup_queue_done_count: terminalQueueThroughput && warmup > 0 ? 1 : 0,
    first_measured_ticket: terminalQueueThroughput
      ? null
      : measuredSubmissions[0]?.ticket ?? null,
    last_measured_ticket: terminalQueueThroughput
      ? statsSubmissions.find((submission) => submission.phase === 'final_measured')?.ticket ?? null
      : measuredSubmissions.at(-1)?.ticket ?? null,
    queue_done_proven: terminalQueueThroughput
      ? measuredTerminalCount === 1
        && (warmup === 0
          || (statsTerminals.filter(
            (terminal) => terminal.phase === 'warmup_boundary' && terminal.status === 'ready',
          ).length === 1
            && manifest.benchmark_window?.warmup_terminal_receipt?.terminal_at_monotonic_ms
              <= manifest.benchmark_window?.first_measured_input_monotonic_ms))
        && manifest.benchmark_window?.draw_count_at_final_drain_start
          === manifest.benchmark_window?.draw_count_at_completion
      : measuredTerminalCount === measuredSubmissions.length,
    off_by_one_check: terminalQueueThroughput
      ? 'warmup_drained_before_input_and_all_measured_submits_covered_by_final_same_submission_terminal'
      : traceSequence
        ? 'one_sort_submission_and_terminal_per_measured_frame'
        : 'fixed_order_reuse',
    ...monotonicWindow,
  };
  manifest.ordering_window = {
    ...manifest.ordering_window,
    ...orderingWindow,
  };
  summary.ordering_window = orderingWindow;
  summary.order_completion = {
    cpu_frame_complete_ms: terminalQueueThroughput ? null : distribution(
      cpuMeasurements
        .filter((measurement) => measuredTickets.has(measurement.ticket))
        .map((measurement) => measurement.frame_complete_ms),
    ),
    gpu_frame_complete_ms: terminalQueueThroughput ? null : distribution(
      measurements
        .filter((measurement) => measuredTickets.has(measurement.ticket))
        .map((measurement) => measurement.gpu_complete_ms),
    ),
  };
  manifest.ordering_evidence = {
    terminal_model: rendererOwnedExact
      ? terminalQueueThroughput
        ? 'untimed_warmup_boundary_plus_final_measured_current_stats_receipt'
        : 'renderer_current_stats'
      : 'legacy_order_measurement',
    submission_predicate: rendererOwnedExact
      ? terminalQueueThroughput
        ? 'warmup_ready_before_input_first_n_minus_1_no_stats_final_receipt_issued'
        : 'current_stats_submission=issued'
      : 'sort_refreshed=true',
    receipt_join_keys: rendererOwnedExact
      ? terminalQueueThroughput
        ? [
          'current_stats_ticket',
          'current_stats_plan',
          'current_stats_camera_revision',
          'current_stats_presentation_sequence',
        ] : [
          'current_stats_ticket',
          'current_stats_plan',
          'current_stats_camera_revision',
          'current_stats_presentation_sequence',
        ]
      : ['submitted_measurement_ticket', 'camera_revision'],
    terminal_receipt_policy: 'exactly_one_of_success_or_structured_failure',
    structured_failure_policy: 'reject_strict_benchmark',
    cpu_completion: terminalQueueThroughput
      ? 'first_measured_input_to_final_current_stats_map_result'
      : 'frame_start_to_queue_done',
    gpu_completion: terminalQueueThroughput
      ? 'first_measured_input_to_final_current_stats_map_result'
      : 'frame_start_to_queue_done',
    completion_protocol: orderCompletionProtocol,
    frame_counts: rendererOwnedExact
      ? terminalQueueThroughput
        ? 'unavailable_in_timed_window_bound_to_current_stats_control'
        : 'post_join_renderer_current_stats_terminal'
      : orderBackend === 'cpu'
        ? 'synchronous_cpu_order'
        : 'post_join_terminal_gpu_receipt',
    summary: 'recomputed_from_post_join_frames',
  };
  manifest.gpu_order_preparation_evidence = {
    record_count: preparations.length,
    invariant:
      'frame_presented=false,pending=true,order_ticket=null,projected_ticket=null',
    presented_or_measured_as_frame: false,
  };
  manifest.projected_draw_evidence = {
    requested_policy: projectedPolicy,
    actual_executions: [
      ...new Set(admittedFrames.map((frame) => frame.projected_execution)),
    ],
    adaptive_state_field: 'projected_adaptive_state',
    submission_field: 'projected_measurement_submission',
    terminal_receipt_policy: 'exactly_one_of_success_or_structured_failure_per_issued_ticket',
    ticket_namespace: 'javascript_safe_high_half_from_2_pow_52',
    ...projectedLedger,
  };
  manifest.gpu_producer_evidence = {
    requested_producer: gpuOrderProducer,
    default_when_unset: fixedGpuPreprojectCompactCell
      ? 'derived_from_exact_whole_plan'
      : 'post-sort',
    actual_producers: [
      ...new Set(
        admittedFrames
          .map((frame) => frame.gpu_order_producer)
          .filter((producer) => producer !== null),
      ),
    ],
    completion_protocol: orderCompletionProtocol,
    ticket_namespace: 'javascript_safe_middle_quarter_from_2_pow_51',
    terminal_receipt_policy: 'exactly_one_success_or_structured_failure_per_issued_ticket',
    exact_scope: fixedGpuPreprojectCompactCell
      ? 'derived_whole_plan_actual'
      : gpuOrderProducer === null ? 'telemetry_disabled' : 'exact_current_contributors',
    ...producerLedger,
  };
  if (orderBackend !== 'cpu'
      && admittedFrames.every((frame) => frame.gpu_complete_ms != null)
      && Array.isArray(manifest.unavailable_fields)) {
    manifest.unavailable_fields = manifest.unavailable_fields.filter(
      (field) => field !== 'frames[*].gpu_complete_ms'
    );
  }
  return {
    manifests: [JSON.stringify(manifest)],
    frameRecords: admittedFrames.map((frame) => JSON.stringify(frame)),
    summaries: [JSON.stringify(summary)],
    orderMeasurements,
    cpuOrderMeasurements,
    orderMeasurementSubmissions,
    orderMeasurementFailures,
    currentStatsSubmissions,
    currentStatsTerminals,
    currentStatsPendingFrames,
    adaptiveGpuFailures,
    loadReceipts,
    gpuOrderPreparations,
    monotonicOrderingWindows,
    projectedMeasurements: projectedSuccesses.map((record) => JSON.stringify(record)),
    projectedMeasurementSubmissions:
      projectedSubmissions.map((record) => JSON.stringify(record)),
    projectedMeasurementFailures: projectedFailures.map((record) => JSON.stringify(record)),
    gpuProducerMeasurements: producerMeasurements.map((record) => JSON.stringify(record)),
    gpuProducerMeasurementSubmissions:
      producerSubmissions.map((record) => JSON.stringify(record)),
    gpuProducerMeasurementFailures:
      producerFailures.map((record) => JSON.stringify(record)),
  };
}

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function writeArtifact({
  manifests,
  frameRecords,
  summaries,
  orderMeasurements,
  cpuOrderMeasurements,
  orderMeasurementSubmissions,
  orderMeasurementFailures,
  currentStatsSubmissions,
  currentStatsTerminals,
  currentStatsPendingFrames,
  adaptiveGpuFailures,
  loadReceipts,
  gpuOrderPreparations,
  monotonicOrderingWindows,
  projectedMeasurements,
  projectedMeasurementSubmissions,
  projectedMeasurementFailures,
  gpuProducerMeasurements,
  gpuProducerMeasurementSubmissions,
  gpuProducerMeasurementFailures,
}) {
  let sibling;
  if (q1ArtifactTransaction !== null) {
    sibling = q1ArtifactTransaction.stagingDirectory;
  } else {
    if (await pathExists(outDir)) {
      throw new Error(`destination already exists: ${outDir}`);
    }
    await mkdir(dirname(outDir), { recursive: true });
    sibling = resolve(dirname(outDir), `.${outDir.split('/').pop()}.staging`);
    await rm(sibling, { recursive: true, force: true });
    await mkdir(sibling, { recursive: true });
  }
  if (q1BuildSnapshotForWrite !== null) {
    const buildRoot = resolve(sibling, 'build');
    await mkdir(buildRoot);
    for (const [name, source] of Object.entries(q1BuildSnapshotForWrite.files)) {
      const destination = resolve(buildRoot, source.split(sep).at(-1));
      await copyFile(source, destination);
      if (await sha256File(destination) !== q1BuildSnapshotForWrite.hashes[name]) {
        throw new Error(`Q1 ${name} changed while materializing the artifact`);
      }
    }
  }
  await writeFile(resolve(sibling, 'manifest.json'), `${manifests[0]}\n`);
  await writeFile(resolve(sibling, 'frames.jsonl'), `${frameRecords.join('\n')}\n`);
  await writeFile(resolve(sibling, 'summary.json'), `${summaries[0]}\n`);
  await writeFile(
    resolve(sibling, 'order-measurements.jsonl'),
    orderMeasurements.length > 0 ? `${orderMeasurements.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'cpu-order-measurements.jsonl'),
    cpuOrderMeasurements.length > 0 ? `${cpuOrderMeasurements.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'order-measurement-submissions.jsonl'),
    orderMeasurementSubmissions.length > 0 ? `${orderMeasurementSubmissions.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'order-measurement-failures.jsonl'),
    orderMeasurementFailures.length > 0 ? `${orderMeasurementFailures.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'current-stats-submissions.jsonl'),
    currentStatsSubmissions.length > 0 ? `${currentStatsSubmissions.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'current-stats-terminals.jsonl'),
    currentStatsTerminals.length > 0 ? `${currentStatsTerminals.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'current-stats-pending-frames.jsonl'),
    currentStatsPendingFrames.length > 0 ? `${currentStatsPendingFrames.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'adaptive-gpu-failures.jsonl'),
    adaptiveGpuFailures.length > 0 ? `${adaptiveGpuFailures.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'load-receipts.jsonl'),
    loadReceipts.length > 0 ? `${loadReceipts.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'gpu-order-preparations.jsonl'),
    gpuOrderPreparations.length > 0 ? `${gpuOrderPreparations.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'ordering-window-monotonic.json'),
    `${monotonicOrderingWindows[0]}\n`
  );
  await writeFile(
    resolve(sibling, 'projected-measurements.jsonl'),
    projectedMeasurements.length > 0 ? `${projectedMeasurements.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'projected-measurement-submissions.jsonl'),
    projectedMeasurementSubmissions.length > 0
      ? `${projectedMeasurementSubmissions.join('\n')}\n`
      : ''
  );
  await writeFile(
    resolve(sibling, 'projected-measurement-failures.jsonl'),
    projectedMeasurementFailures.length > 0
      ? `${projectedMeasurementFailures.join('\n')}\n`
      : ''
  );
  await writeFile(
    resolve(sibling, 'gpu-producer-measurements.jsonl'),
    gpuProducerMeasurements.length > 0 ? `${gpuProducerMeasurements.join('\n')}\n` : ''
  );
  await writeFile(
    resolve(sibling, 'gpu-producer-measurement-submissions.jsonl'),
    gpuProducerMeasurementSubmissions.length > 0
      ? `${gpuProducerMeasurementSubmissions.join('\n')}\n`
      : ''
  );
  await writeFile(
    resolve(sibling, 'gpu-producer-measurement-failures.jsonl'),
    gpuProducerMeasurementFailures.length > 0
      ? `${gpuProducerMeasurementFailures.join('\n')}\n`
      : ''
  );
  if (q1ArtifactTransaction === null) {
    await runPythonValidator(
      'tests/perf/validate-benchmark-artifacts.py',
      [sibling],
      'benchmark artifact validator',
    );
    await rename(sibling, outDir);
    return outDir;
  }
  return sibling;
}

async function publishTruck1080pSuite({ manifest, frames, imagePath }) {
  const imageSha256 = await sha256File(imagePath);
  return publishValidatedTruck1080pSuite({
    suitePath: fullQualitySuitePath,
    manifest,
    frames,
    imagePath,
    imageSha256,
    expected: truckQualification,
    validate: (staging) => runPythonValidator(
      'tests/perf/validate-full-quality-experiment.py',
      [staging, '--verify-inputs'],
      'full-quality suite validator',
    ),
  });
}

async function loadPuppeteer() {
  const candidates = [
    resolve(playcanvasRoot, 'node_modules/puppeteer-core/lib/esm/puppeteer/puppeteer-core.js'),
    resolve(playcanvasRoot, 'node_modules/puppeteer-core/lib/cjs/puppeteer/puppeteer-core.js'),
    resolve(playcanvasRoot, 'node_modules/puppeteer-core/index.js')
  ];
  for (const candidate of candidates) {
    if (await pathExists(candidate)) {
      const mod = await import(pathToFileURL(candidate).href);
      return mod.default ?? mod;
    }
  }
  throw new Error('puppeteer-core not found under tests/competitive/playcanvas; run npm ci there first');
}

if (truck1080pQualification) {
  try {
    await admitTruck1080pQualification();
  } catch (error) {
    let logPath = null;
    if (claimedTruckOutputRoot !== null) {
      logPath = resolve(claimedTruckOutputRoot, 'collector-admission-failure.log');
      await writeFile(logPath, `${error.stack ?? error}\n`, { flag: 'wx' });
    }
    console.error(JSON.stringify({
      status: 'blocked',
      scope: TRUCK_1080P_QUALIFICATION.name,
      reason: error.message,
      ...(logPath === null ? {} : { log: logPath }),
    }));
    process.exit(2);
  }
}

const currentStatsControlIdentity = await loadCurrentStatsControlIdentity();
const q1ControlBindings = await loadQ1ControlBindings();
const q1RepositoryPre = await q1RepositorySnapshot('pre_browser_session');
if (q1ArtifactRole !== null && !q1RepositoryPre.clean) {
  throw new Error('Q1 repository is not clean before the browser session');
}
const repositoryCommit = q1RepositoryPre.head;
const dirty = !q1RepositoryPre.clean;
const q1ServedSourcesPre = await q1ServedSourceSnapshot('pre_browser_session');
const q1PackageUrl = await q1WasmPackageUrl(repositoryCommit);
const q1PackagePre = await q1PackageSnapshot();
const q1HostPre = await observeQ1HostState('pre_browser_session');
const chrome = await findChrome();
if (!chrome) {
  console.error(JSON.stringify({ status: 'blocked', reason: 'no Chrome/Chromium found', chromeCandidates }));
  process.exit(2);
}
const q1BrowserExecutableShaPre = q1ArtifactRole === null
  ? null
  : await sha256File(chrome);

const puppeteerApi = await loadPuppeteer();
if (q1ArtifactRole !== null) {
  q1ArtifactTransaction = await Q1ArtifactTransaction.claim({
    seriesRoot: q1SeriesRoot,
    finalDirectory: outDir,
    collectionSessionId: q1CollectionSessionId,
  });
}
const browserOwnership = browserOwnershipConfig(
  process.env,
  Boolean(process.env.GSPLAT_Q1_SERIES_ROOT)
);
let server;
let browser;
let q1BrowserArgsReceipt = null;
let q1BrowserVersionPre = null;
let q1AdapterPre = null;
let q1BuildSnapshotForWrite = null;
let q1PendingResult = null;
let q1CollectionFailure = null;
const consoleLines = [];
try {
  server = await startHttpServer();
  const browserLaunchOptions = {
    executablePath: chrome,
    headless: q1ArtifactRole === null ? process.env.HEADLESS !== '0' : false,
    defaultViewport: q1ArtifactRole === null
      ? { width: 1280, height: 720, deviceScaleFactor: 1 }
      : { width: 1920, height: 1080, deviceScaleFactor: 1 },
    args: [...Q1_BROWSER_ARGS]
  };
  if (browserOwnership !== null) {
    browserLaunchOptions.userDataDir = browserOwnership.userDataDir;
  }
  browser = await puppeteerApi.launch(browserLaunchOptions);
  await publishBrowserOwnershipHandshake(browser, browserOwnership);
  const page = await browser.newPage();
  if (q1ArtifactRole !== null) {
    await page.bringToFront();
    const browserProcess = browser.process();
    q1BrowserArgsReceipt = browserProcessArgsReceipt({
      spawnfile: browserProcess?.spawnfile,
      spawnargs: browserProcess?.spawnargs,
      expectedExecutable: chrome,
      requiredArgs: Q1_BROWSER_ARGS,
    });
    q1BrowserVersionPre = await browser.version();
  }
  await page.evaluateOnNewDocument((commit, isDirty, device) => {
    globalThis.GSPLAT_BUILD_COMMIT = commit;
    globalThis.GSPLAT_BUILD_DIRTY = isDirty;
    globalThis.GSPLAT_BENCHMARK_DEVICE = device;
  }, repositoryCommit, dirty, os.hostname());
  await page.evaluateOnNewDocument((pairId, pairOrder, pairPosition) => {
    globalThis.GSPLAT_PHASE_E_PAIR_ID = pairId;
    globalThis.GSPLAT_PHASE_E_PAIR_ORDER = pairOrder;
    globalThis.GSPLAT_PHASE_E_PAIR_POSITION = pairPosition;
  }, process.env.PHASE_E_PAIR_ID ?? null, process.env.PHASE_E_PAIR_ORDER ?? null,
  Number(process.env.PHASE_E_PAIR_POSITION ?? 0) || null);
  page.on('console', (message) => {
    consoleLines.push(`${message.type()}: ${message.text()}`);
  });
  page.on('pageerror', (error) => {
    consoleLines.push(`pageerror: ${error.stack ?? error.message}`);
  });
  const params = new URLSearchParams({
    gsplat_benchmark: m4Smoke ? 'false' : 'true',
    gsplat_benchmark_sync: benchmarkSync ? 'true' : 'false',
    gsplat_benchmark_frames: String(requestedFrameCount),
    gsplat_benchmark_warmup_frames: String(warmup),
    gsplat_surface_sort_interval: String(sortInterval),
    gsplat_surface_order_backend: orderBackend,
    gsplat_surface_projected_policy: projectedPolicy,
    benchmark_yaw_step: qualification ? '0' : '0.001'
  });
  params.set('gsplat_order_completion_protocol', orderCompletionProtocol);
  params.set('gsplat_benchmark_window_mode', benchmarkWindowMode);
  if (currentStatsControlIdentity !== null) {
    params.set('gsplat_current_stats_control_run_id', currentStatsControlIdentity.runId);
    params.set(
      'gsplat_current_stats_control_configuration_sha256',
      currentStatsControlIdentity.configurationSha256,
    );
  }
  if (m4Smoke) params.set('gsplat_current_stats_smoke', 'true');
  if (gpuOrderProducer !== null) {
    params.set('gsplat_surface_gpu_order_producer', gpuOrderProducer);
  }
  if (q1ArtifactRole === 'control') {
    params.set('gsplat_q1_capture_trace_frame', String(q1CaptureTraceFrame));
  } else if (q1ArtifactRole === 'throughput') {
    params.set('gsplat_q1_queue_terminal', '1');
  }
  if (q1PackageUrl !== null) params.set('gsplat_wasm_package_url', q1PackageUrl);
  if (dataset) params.set('dataset', dataset);
  params.set('gsplat_geometry_path', geometryPath);
  if (process.env.GSPLAT_CAMERA_TRACE_URL) {
    params.set('gsplat_camera_trace_url', process.env.GSPLAT_CAMERA_TRACE_URL);
  }
  if (process.env.GSPLAT_CAMERA_TRACE_SEQUENCE === '1') {
    params.set('gsplat_camera_trace_sequence', 'true');
    if (process.env.GSPLAT_CAMERA_FRAME_INDICES) {
      params.set('gsplat_camera_frame_indices', process.env.GSPLAT_CAMERA_FRAME_INDICES);
    }
    if (process.env.GSPLAT_CAMERA_TRACE_LOOPS) {
      params.set('gsplat_camera_trace_loops', process.env.GSPLAT_CAMERA_TRACE_LOOPS);
    }
  } else if (process.env.GSPLAT_CAMERA_FRAME) {
    params.set('gsplat_camera_frame', process.env.GSPLAT_CAMERA_FRAME);
  }
  if (qualification) params.set('gsplat_camera_trace', `phase-e-${qualificationName}`);
  const url = `http://127.0.0.1:${port}/examples/web/?${params.toString()}`;
  await page.goto(url, { waitUntil: 'networkidle0', timeout: navigationTimeoutMs });
  if (q1ArtifactRole !== null) {
    await page.waitForFunction(
      () => globalThis.GSPLAT_Q1_SURFACE_DEVICE_PRE != null,
      { timeout: benchmarkTimeoutMs },
    );
    q1AdapterPre = await page.evaluate(() => globalThis.GSPLAT_Q1_SURFACE_DEVICE_PRE);
  }
  if (m4Smoke) {
    await page.waitForFunction(
      () => ['ready', 'failed'].includes(globalThis.GSPLAT_M4_SMOKE_RESULT?.status),
      { timeout: benchmarkTimeoutMs },
    );
    // Let the next animation turn report an asynchronous render failure before
    // retaining the smoke receipt. This is a stability fence, not a retry.
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(
      () => requestAnimationFrame(resolve),
    )));
    const smoke = await page.evaluate(() => {
      const canvas = document.getElementById('viewport');
      return {
        result: globalThis.GSPLAT_M4_SMOKE_RESULT,
        browser: {
          navigator_gpu: navigator.gpu != null,
          webgpu_context: canvas?.getContext('webgpu') != null,
          canvas_width: canvas?.width ?? null,
          canvas_height: canvas?.height ?? null,
          gpu_status: document.getElementById('gpuStatus')?.textContent ?? null,
          status_line: document.getElementById('statusLine')?.textContent ?? null,
          user_agent: navigator.userAgent,
        },
      };
    });
    const { result, browser: browserFacts } = smoke;
    const load = result?.load_receipt;
    const scene = result?.scene;
    const frame = result?.frame;
    const stats = result?.current_stats;
    const dimensions = [
      frame?.surface_width,
      frame?.surface_height,
      frame?.internal_render_width,
      frame?.internal_render_height,
      frame?.presented_width,
      frame?.presented_height,
    ];
    if (result?.status !== 'ready'
        || result.backend !== 'wasm'
        || result.sampled_webgl_enabled !== false
        || !browserFacts.navigator_gpu
        || !browserFacts.webgpu_context
        || browserFacts.gpu_status !== 'wgpu'
        || consoleLines.some((line) => line.includes('fallback=webgl'))
        || load?.fullQuality !== true
        || load?.sourceMembership !== 'all'
        || load?.samplingEnabled !== false
        || load?.lodEnabled !== false
        || load?.partialScenePublished !== false
        || ![load?.decodedCount, load?.encodedCount, load?.residentCount, load?.addressableCount]
          .every((count) => count === load?.sourceCount)
        || scene?.gaussians !== load?.sourceCount
        || scene?.shDegree !== load?.sourceShDegree
        || load?.sourceShDegree !== load?.residentShDegree
        || frame?.frame_presented !== true
        || !dimensions.every((value) => Number.isSafeInteger(value) && value > 0)
        || frame.surface_width !== frame.internal_render_width
        || frame.surface_height !== frame.internal_render_height
        || frame.surface_width !== frame.presented_width
        || frame.surface_height !== frame.presented_height
        || frame.surface_width !== browserFacts.canvas_width
        || frame.surface_height !== browserFacts.canvas_height
        || frame.current_stats_submission !== 'issued'
        || frame.current_stats_ticket !== stats?.ticket
        || frame.current_stats_plan !== stats?.plan
        || frame.current_stats_camera_revision !== frame.camera_revision
        || stats?.cameraRevision !== frame.camera_revision
        || frame.current_stats_presentation_sequence !== stats?.presentationSequence
        || !Number.isSafeInteger(stats?.presentationSequence)
        || stats.presentationSequence <= 0
        || stats.sourceCount !== load.sourceCount
        || stats.contributorCount > stats.visibleCount
        || stats.visibleCount > stats.sourceCount) {
      throw new Error(`M4 browser WebGPU smoke invariant failed: ${JSON.stringify(smoke)}`);
    }
    await mkdir(outDir, { recursive: true });
    await writeFile(resolve(outDir, 'm4-browser-smoke.json'), `${JSON.stringify({
      schema: 'gsplat-m4-browser-smoke/v1',
      repository_commit: repositoryCommit,
      dirty,
      url,
      ...smoke,
    }, null, 2)}\n`);
    const dataUrl = await page.$eval('#viewport', (canvas) => canvas.toDataURL('image/png'));
    await writeFile(resolve(outDir, 'final-frame.png'), Buffer.from(dataUrl.split(',')[1], 'base64'));
    await writeFile(resolve(outDir, 'browser-console.log'), `${consoleLines.join('\n')}\n`);
    console.log(JSON.stringify({ status: 'ok', scope: 'm4_functional_smoke', artifact_dir: outDir }));
  } else {
  await page.waitForFunction(
    () => {
      const el = document.getElementById('benchmarkStatus');
      return el && (el.textContent === 'complete' || el.textContent === 'failed');
    },
    { timeout: benchmarkTimeoutMs }
  );
  const benchmarkState = await page.$eval('#benchmarkStatus', (element) => element.textContent);
  if (benchmarkState !== 'complete') {
    const detail = await page.evaluate(() => ({
      result: document.getElementById('benchmarkResult')?.textContent,
      status: document.getElementById('statusLine')?.textContent,
      loading: document.getElementById('loadingMeta')?.textContent,
    }));
    throw new Error(`browser benchmark failed: ${JSON.stringify(detail)}`);
  }
  if (geometryPath !== 'paged') {
    await page.waitForFunction(
      () => globalThis.GSPLAT_ORDER_LEDGER_COMPLETE === true,
      { timeout: benchmarkTimeoutMs },
    );
  }
  await page.waitForFunction(
    () => globalThis.GSPLAT_PROJECTED_LEDGER_COMPLETE === true,
    { timeout: benchmarkTimeoutMs },
  );
  if (gpuOrderProducer !== null) {
    await page.waitForFunction(
      () => globalThis.GSPLAT_GPU_PRODUCER_LEDGER_COMPLETE === true,
      { timeout: benchmarkTimeoutMs },
    );
  }
  await new Promise((r) => setTimeout(r, benchmarkSettleMs));
  const settledBenchmarkState = await page.$eval('#benchmarkStatus', (element) => element.textContent);
  if (settledBenchmarkState !== 'complete') {
    throw new Error(`browser benchmark became ${settledBenchmarkState} while draining GPU receipts`);
  }
  let q1ObservedContext = q1RunContext;
  if (q1ArtifactRole !== null) {
    const [browserRuntime, rendererDevice, q1HostPost, q1PackagePost] = await Promise.all([
      page.evaluate(() => ({
        pre: globalThis.GSPLAT_Q1_BROWSER_RUNTIME_PRE ?? null,
        post: globalThis.GSPLAT_Q1_BROWSER_RUNTIME_POST ?? null,
        user_agent: navigator.userAgent,
        platform: navigator.platform,
      })),
      page.evaluate(() => ({
        pre: globalThis.GSPLAT_Q1_SURFACE_DEVICE_PRE ?? null,
        post: globalThis.GSPLAT_Q1_SURFACE_DEVICE_POST ?? null,
      })),
      observeQ1HostState('post_measurement_terminal'),
      q1PackageSnapshot(),
    ]);
    const browserVersionPost = await browser.version();
    assertStableBrowserRuntime(browserRuntime.pre, browserRuntime.post);
    const q1AdapterPost = assertStableRendererSurfaceDevice(
      rendererDevice.pre,
      rendererDevice.post,
    );
    if (q1BrowserVersionPre !== browserVersionPost
        || !sameJson(q1AdapterPre, q1AdapterPost)
        || !sameJson(q1PackagePre.hashes, q1PackagePost.hashes)
        || q1HostPre.power_source !== q1HostPost.power_source
        || q1HostPre.os_build !== q1HostPost.os_build) {
      throw new Error('Q1 browser, adapter, host, or runtime package identity drifted');
    }
    q1BuildSnapshotForWrite = q1PackagePost;
    const buildArtifacts = q1BuildArtifactReceipts(q1PackagePost);
    const browserExecutableSha256 = await sha256File(chrome);
    if (browserExecutableSha256 !== q1BrowserExecutableShaPre) {
      throw new Error('Q1 browser executable changed during collection');
    }
    const webGpuEnvironment = gsplatQ1WebGpuEnvironmentFields(q1AdapterPost);
    q1ObservedContext = observedRunContext({
      declared: q1RunContext,
      buildArtifacts,
      environment: {
        platform: browserRuntime.platform,
        os: `${os.type()} ${os.release()}`,
        device: os.hostname(),
        browser: `${browserVersionPost} ${browserRuntime.user_agent}`,
        browser_executable_sha256: browserExecutableSha256,
        browser_launch_args_sha256: q1BrowserArgsReceipt.normalized_sha256,
        browser_launch_args_receipt: q1BrowserArgsReceipt,
        adapter: webGpuEnvironment.adapter,
        adapter_identity_status: webGpuEnvironment.adapter_identity_status,
        driver: `apple_metal_os_build:${q1HostPost.os_build}`,
        driver_source: 'macos_sw_vers_buildVersion',
        canonical_adapter_supported_limits_sha256:
          webGpuEnvironment.canonical_adapter_supported_limits_sha256,
        adapter_supported_limits_sha256:
          webGpuEnvironment.adapter_supported_limits_sha256,
        device_effective_limits_sha256:
          webGpuEnvironment.device_effective_limits_sha256,
        webgpu_device_environment_receipt:
          webGpuEnvironment.webgpu_device_environment_receipt,
        power_source: q1HostPost.power_source,
        collection_session_id: q1CollectionSessionId,
        thermal: {
          source: 'macos_pmset_thermal_warning_level',
          pre: q1HostPre.thermal,
          post: q1HostPost.thermal,
          admitted: true,
        },
        browser_runtime: browserRuntime,
        renderer_surface_device_receipt: q1AdapterPost,
        repository_snapshot: q1RepositoryPre,
        served_first_party_sources: q1ServedSourcesPre,
        runtime_stability_receipt: {
          path: relative(
            q1SeriesRoot,
            resolve(outDir, 'q1-runtime-stability.json'),
          ).split(sep).join('/'),
          publication_policy: 'written_after_cleanup_before_atomic_publish',
        },
      },
    });
  }
  let parsed = parseArtifacts(consoleLines);
  let q1RendererCapture = null;
  if (q1ArtifactRole === 'control') {
    const payload = await page.evaluate(() => {
      const capture = globalThis.GSPLAT_Q1_RENDERER_CAPTURE;
      if (!capture?.rgba8 || !capture?.receipt) return null;
      let binary = '';
      for (let offset = 0; offset < capture.rgba8.length; offset += 0x8000) {
        binary += String.fromCharCode(...capture.rgba8.subarray(offset, offset + 0x8000));
      }
      return {
        trace_frame_index: capture.trace_frame_index,
        measured_frame_index: capture.measured_frame_index,
        receipt: capture.receipt,
        rgba8_base64: btoa(binary),
      };
    });
    if (payload === null || payload.trace_frame_index !== q1CaptureTraceFrame) {
      throw new Error('Q1 renderer-owned capture is unavailable or has the wrong trace');
    }
    q1RendererCapture = {
      receipt: payload.receipt,
      rgba8: Buffer.from(payload.rgba8_base64, 'base64'),
    };
    const trace = JSON.parse(await readFile(
      resolve(repoRoot, truckQualification.trace.local_path),
      'utf8',
    ));
    const decorated = decorateQ1GsplatControl({
      parsed,
      capture: q1RendererCapture,
      trace,
      traceFrameIndex: q1CaptureTraceFrame,
      protocolSha256: q1ProtocolSha256,
      runContext: q1ObservedContext,
    });
    parsed = decorated.parsed;
  } else if (q1ArtifactRole === 'throughput') {
    parsed = decorateQ1GsplatThroughput({
      parsed,
      protocolSha256: q1ProtocolSha256,
      controlBindings: q1ControlBindings,
      runContext: q1ObservedContext,
    });
  }
  const truckManifest = truck1080pQualification ? JSON.parse(parsed.manifests[0]) : null;
  const truckFrames = truck1080pQualification
    ? parsed.frameRecords.map((frame) => JSON.parse(frame))
    : null;
  if (truck1080pQualification && truckQualificationStage === 'control') {
    validateTruck1080pExactRasterEvidence({
      manifest: truckManifest,
      frames: truckFrames,
      expected: truckQualification,
    });
  }
  const artifactDir = await writeArtifact(parsed);
  const imagePath = resolve(artifactDir, 'final-frame.png');
  if (q1RendererCapture !== null) {
    await writeFile(imagePath, rgba8Png(1920, 1080, q1RendererCapture.rgba8));
  } else {
    const dataUrl = await page.$eval('#viewport', (canvas) => canvas.toDataURL('image/png'));
    await writeFile(imagePath, Buffer.from(dataUrl.split(',')[1], 'base64'));
  }
  await writeFile(resolve(artifactDir, 'browser-console.log'), `${consoleLines.join('\n')}\n`);
  const suitePath = q1ArtifactRole === null && truck1080pQualification && !(
    benchmarkWindowMode === 'terminal_queue_throughput_window'
  )
    ? await publishTruck1080pSuite({
        manifest: truckManifest,
        frames: truckFrames,
        imagePath,
      })
    : null;
  if (q1ArtifactRole === null
      && truck1080pQualification && truckQualificationStage === 'control') {
    const controlManifestPath = resolve(artifactDir, 'manifest.json');
    await publishTruck1080pControlCompletion({
      outputRoot: dirname(outDir),
      controlManifestPath,
      controlRunId: truckManifest.run_id,
      configurationSha256: truckManifest.benchmark_window.configuration_sha256,
      suitePath,
      expected: truckQualification,
    });
  }
  const resultLine = consoleLines.find((line) => line.includes('BENCHMARK_RESULT '));
  const result = {
    status: 'ok',
    artifact_dir: q1ArtifactRole === null ? artifactDir : outDir,
    result: resultLine ?? null,
  };
  if (suitePath !== null) result.full_quality_suite = suitePath;
  if (q1ArtifactRole === null) {
    console.log(JSON.stringify(result));
  } else {
    q1PendingResult = { result, stagingDirectory: artifactDir };
  }
  }
} catch (error) {
  if (q1ArtifactRole !== null) {
    q1CollectionFailure = error;
  } else {
    const logPath = truck1080pQualification
      ? resolve(dirname(outDir), 'collector-failure.log')
      : resolve(repoRoot, 'target/benchmarks/phase-a/web-collector-failure.log');
    await mkdir(dirname(logPath), { recursive: true });
    await writeFile(
      logPath,
      `${consoleLines.join('\n')}\n\n${error.stack ?? error}\n`,
      truck1080pQualification ? { flag: 'wx' } : undefined,
    );
    console.error(JSON.stringify({ status: 'failed', reason: error.message, log: logPath }));
    process.exitCode = 1;
  }
} finally {
  try {
    await cleanupBrowserAndServer(browser, server);
  } catch (error) {
    if (q1ArtifactTransaction !== null) {
      await q1ArtifactTransaction.recordCleanupFailure(error);
    } else {
      console.error(error);
      process.exitCode = 1;
    }
    q1CollectionFailure ??= error;
  }
}

if (q1ArtifactTransaction !== null) {
  if (q1CollectionFailure !== null) {
    if (!(await pathExists(q1ArtifactTransaction.cleanupBlockerPath))) {
      await q1ArtifactTransaction.recordFailure(q1CollectionFailure, 'collection');
    }
    console.error(JSON.stringify({
      status: 'failed',
      reason: q1CollectionFailure.message,
      blocker: q1ArtifactTransaction.blockerPath,
      ...(await pathExists(q1ArtifactTransaction.cleanupBlockerPath)
        ? { cleanup_blocker: q1ArtifactTransaction.cleanupBlockerPath }
        : {}),
    }));
    process.exitCode = 1;
  } else {
    try {
      if (q1PendingResult === null) throw new Error('Q1 collection produced no staged artifact');
      const [repositoryPost, servedSourcesPost, packageAfterCleanup] = await Promise.all([
        q1RepositorySnapshot('post_browser_cleanup'),
        q1ServedSourceSnapshot('post_browser_cleanup'),
        q1PackageSnapshot(),
      ]);
      if (!repositoryPost.clean || repositoryPost.head !== q1RepositoryPre.head
          || repositoryPost.porcelain !== q1RepositoryPre.porcelain
          || servedSourcesPost.aggregate_sha256 !== q1ServedSourcesPre.aggregate_sha256
          || !sameJson(servedSourcesPost.files, q1ServedSourcesPre.files)
          || !sameJson(packageAfterCleanup.hashes, q1PackagePre.hashes)) {
        throw new Error('Q1 repository, served first-party sources, or runtime package drifted');
      }
      await writeFile(
        resolve(q1PendingResult.stagingDirectory, 'q1-runtime-stability.json'),
        `${JSON.stringify({
          schema: 'gsplat-q1-runtime-stability/v1',
          repository: { pre: q1RepositoryPre, post: repositoryPost },
          served_first_party_sources: {
            pre: q1ServedSourcesPre,
            post: servedSourcesPost,
          },
          runtime_package_hashes: {
            pre: q1PackagePre.hashes,
            post: packageAfterCleanup.hashes,
          },
          browser_server_cleanup_complete: true,
          automatic_retry: false,
        }, null, 2)}\n`,
      );
      await writeFile(
        resolve(q1PendingResult.stagingDirectory, 'browser-console.log'),
        `${consoleLines.join('\n')}\n`,
      );
      await runPythonValidator(
        'tests/perf/validate-benchmark-artifacts.py',
        [q1PendingResult.stagingDirectory],
        'benchmark artifact validator',
      );
      q1ArtifactTransaction.markCleanupComplete();
      await q1ArtifactTransaction.publish();
      console.log(JSON.stringify(q1PendingResult.result));
    } catch (error) {
      await q1ArtifactTransaction.recordFailure(error, 'pre_publish');
      console.error(JSON.stringify({
        status: 'failed',
        reason: error.message,
        blocker: q1ArtifactTransaction.blockerPath,
      }));
      process.exitCode = 1;
    }
  }
}
process.exit(process.exitCode ?? 0);
