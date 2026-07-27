#!/usr/bin/env node
/**
 * Headless Chrome collector for gsplat-rs Web Phase A baseline artifacts.
 * Emits a validated gsplat-benchmark/v1 directory from console JSON lines.
 */
import { access, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { constants, createReadStream } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
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
  validateCurrentStatsEvidence,
  validateCurrentStatsTerminalLedger,
} from '../src/benchmark-current-stats-evidence.mjs';
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
  publishValidatedTruck1080pSuite,
  validateTruck1080pCleanWorkingTree,
  validateTruck1080pCollectorConfig,
} from '../src/truck-1080p-qualification.mjs';

const execFile = promisify(execFileCallback);

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, '../../..');
const playcanvasRoot = resolve(repoRoot, 'tests/competitive/playcanvas');
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
    : qualificationName === TRUCK_1080P_QUALIFICATION.name ? 'truck' : null;
const truck1080pQualification = qualificationName === TRUCK_1080P_QUALIFICATION.name;
const m4Smoke = process.env.GSPLAT_M4_SMOKE === '1';
const frames = Number(process.env.GSPLAT_BENCHMARK_FRAMES ?? (qualification ? 3600 : 30));
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
          ? 'target/qualification/q1-webgpu-truck-1080p/run-adaptive'
        : qualification
          ? 'target/benchmarks/phase-e/gsplat-web-kitsune-static-v1'
          : 'target/benchmarks/phase-a/web-minimal-v1'
    )
);
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
    warmup,
    frames,
    cameraTraceUrl: process.env.GSPLAT_CAMERA_TRACE_URL ?? null,
    cameraTraceSequence: process.env.GSPLAT_CAMERA_TRACE_SEQUENCE === '1',
    cameraTraceLoops: Number(process.env.GSPLAT_CAMERA_TRACE_LOOPS ?? 0),
    cameraFrame: process.env.GSPLAT_CAMERA_FRAME ?? null,
    cameraFrameIndices: (process.env.GSPLAT_CAMERA_FRAME_INDICES ?? '')
      .split(',')
      .filter((value) => value.length > 0)
      .map(Number),
  });
  const porcelain = (
    await execFile('git', ['status', '--porcelain'], { cwd: repoRoot })
  ).stdout;
  validateTruck1080pCleanWorkingTree(porcelain);
  const outputRoot = dirname(outDir);
  if (outDir !== resolve(outputRoot, expected.artifact_name)) {
    throw new Error(`Truck 1080p artifact must be named ${expected.artifact_name}`);
  }
  if (fullQualitySuitePath !== resolve(outputRoot, expected.suite_name)) {
    throw new Error(`Truck 1080p suite must be ${resolve(outputRoot, expected.suite_name)}`);
  }
  if (await pathExists(outputRoot)) {
    throw new Error(`Truck 1080p output root already exists: ${outputRoot}`);
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
        reject(error);
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
  const terminalSubmissions = rendererOwnedExact ? statsSubmissions : submissions;
  const terminalReceipts = rendererOwnedExact
    ? statsTerminals
    : [...measurements, ...cpuMeasurements, ...failures];
  const pageMonotonicWindow = JSON.parse(monotonicOrderingWindows[0]);
  const monotonicWindow = monotonicOrderingWindow({
    submissions: terminalSubmissions,
    terminals: terminalReceipts,
  });
  const measuredSubmissionCount = terminalSubmissions.filter(
    (submission) => submission.phase === 'measured',
  ).length;
  if (pageMonotonicWindow.measured_submit_count !== measuredSubmissionCount
      || pageMonotonicWindow.measured_terminal_count !== measuredSubmissionCount) {
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
  const expectedActualProducer = gpuOrderProducer ?? (hasGpuFrame ? 'post-sort' : null);
  if (manifest.renderer?.gpu_order_producer_actual !== expectedActualProducer) {
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
  const frames = rendererOwnedExact
    ? joinCurrentStatsEvidence({
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
  if (rendererOwnedExact) {
    validateCurrentStatsEvidence({ frames });
    if (gpuOrderProducer !== null) {
      const requiredPlan = gpuOrderProducer === 'preproject'
        ? 'gpu_preproject'
        : 'gpu_post_sort';
      if (statsTerminals.some((terminal) => terminal.plan !== requiredPlan)
          || frames.some((frame) => frame.gpu_order_producer !== gpuOrderProducer)) {
        throw new Error(
          `renderer current-stats terminals do not prove requested producer ${gpuOrderProducer}`,
        );
      }
    }
  } else {
    validateOrderingEvidence({
      requestedBackend: orderBackend,
      frames,
      measurements,
      failures: gpuFailures,
      fixedCameraReuse: manifest.trace?.frame_index != null,
    });
  }
  const summary = benchmarkSummaryFromFrameRecords(frames, JSON.parse(summaries[0]));
  summary.count_evidence = benchmarkCountEvidence(frames);
  const measuredSubmissions = terminalSubmissions.filter(
    (submission) => submission.phase === 'measured',
  );
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
  const measuredTerminalCount = measuredSubmissions.filter(
    (submission) => successfulTerminalTickets.has(submission.ticket),
  ).length;
  if (measuredTerminalCount !== measuredSubmissions.length) {
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
    first_measured_ticket: measuredSubmissions[0]?.ticket ?? null,
    last_measured_ticket: measuredSubmissions.at(-1)?.ticket ?? null,
    queue_done_proven: measuredTerminalCount === measuredSubmissions.length,
    off_by_one_check: traceSequence ? 'one_sort_submission_and_terminal_per_measured_frame' : 'fixed_order_reuse',
    ...monotonicWindow,
  };
  manifest.ordering_window = {
    ...manifest.ordering_window,
    ...orderingWindow,
  };
  summary.ordering_window = orderingWindow;
  summary.order_completion = {
    cpu_frame_complete_ms: distribution(
      cpuMeasurements
        .filter((measurement) => measuredTickets.has(measurement.ticket))
        .map((measurement) => measurement.frame_complete_ms),
    ),
    gpu_frame_complete_ms: distribution(
      measurements
        .filter((measurement) => measuredTickets.has(measurement.ticket))
        .map((measurement) => measurement.gpu_complete_ms),
    ),
  };
  manifest.ordering_evidence = {
    terminal_model: rendererOwnedExact
      ? 'renderer_current_stats'
      : 'legacy_order_measurement',
    submission_predicate: rendererOwnedExact
      ? 'current_stats_submission=issued'
      : 'sort_refreshed=true',
    receipt_join_keys: rendererOwnedExact
      ? [
          'current_stats_ticket',
          'current_stats_plan',
          'current_stats_camera_revision',
          'current_stats_presentation_sequence',
        ]
      : ['submitted_measurement_ticket', 'camera_revision'],
    terminal_receipt_policy: 'exactly_one_of_success_or_structured_failure',
    structured_failure_policy: 'reject_strict_benchmark',
    cpu_completion: 'frame_start_to_queue_done',
    gpu_completion: 'frame_start_to_queue_done',
    completion_protocol: orderCompletionProtocol,
    frame_counts: rendererOwnedExact
      ? 'post_join_renderer_current_stats_terminal'
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
    actual_executions: [...new Set(frames.map((frame) => frame.projected_execution))],
    adaptive_state_field: 'projected_adaptive_state',
    submission_field: 'projected_measurement_submission',
    terminal_receipt_policy: 'exactly_one_of_success_or_structured_failure_per_issued_ticket',
    ticket_namespace: 'javascript_safe_high_half_from_2_pow_52',
    ...projectedLedger,
  };
  manifest.gpu_producer_evidence = {
    requested_producer: gpuOrderProducer,
    default_when_unset: 'post-sort',
    actual_producers: [
      ...new Set(
        frames
          .map((frame) => frame.gpu_order_producer)
          .filter((producer) => producer !== null),
      ),
    ],
    completion_protocol: orderCompletionProtocol,
    ticket_namespace: 'javascript_safe_middle_quarter_from_2_pow_51',
    terminal_receipt_policy: 'exactly_one_success_or_structured_failure_per_issued_ticket',
    exact_scope: gpuOrderProducer === null ? 'telemetry_disabled' : 'exact_current_contributors',
    ...producerLedger,
  };
  if (orderBackend !== 'cpu'
      && frames.every((frame) => frame.gpu_complete_ms != null)
      && Array.isArray(manifest.unavailable_fields)) {
    manifest.unavailable_fields = manifest.unavailable_fields.filter(
      (field) => field !== 'frames[*].gpu_complete_ms'
    );
  }
  return {
    manifests: [JSON.stringify(manifest)],
    frameRecords: frames.map((frame) => JSON.stringify(frame)),
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
  if (await pathExists(outDir)) {
    throw new Error(`destination already exists: ${outDir}`);
  }
  await mkdir(dirname(outDir), { recursive: true });
  const sibling = resolve(dirname(outDir), `.${outDir.split('/').pop()}.staging`);
  await rm(sibling, { recursive: true, force: true });
  await mkdir(sibling, { recursive: true });
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
  await runPythonValidator(
    'tests/perf/validate-benchmark-artifacts.py',
    [sibling],
    'benchmark artifact validator',
  );
  await rename(sibling, outDir);
  return outDir;
}

async function publishTruck1080pSuite({ manifest, imagePath }) {
  const imageSha256 = await sha256File(imagePath);
  return publishValidatedTruck1080pSuite({
    suitePath: fullQualitySuitePath,
    manifest,
    imagePath,
    imageSha256,
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
    const outputRoot = dirname(outDir);
    let logPath = null;
    if (!await pathExists(outputRoot)) {
      logPath = resolve(outputRoot, 'collector-admission-failure.log');
      await mkdir(outputRoot, { recursive: true });
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

const chrome = await findChrome();
if (!chrome) {
  console.error(JSON.stringify({ status: 'blocked', reason: 'no Chrome/Chromium found', chromeCandidates }));
  process.exit(2);
}

const puppeteerApi = await loadPuppeteer();
let server;
let browser;
const consoleLines = [];
try {
  server = await startHttpServer();
  browser = await puppeteerApi.launch({
    executablePath: chrome,
    headless: process.env.HEADLESS !== '0',
    defaultViewport: { width: 1280, height: 720, deviceScaleFactor: 1 },
    args: ['--enable-unsafe-webgpu', '--enable-gpu', '--ignore-gpu-blocklist']
  });
  const page = await browser.newPage();
  const repositoryCommit = (await execFile('git', ['rev-parse', 'HEAD'], { cwd: repoRoot })).stdout.trim();
  const dirty = (await execFile('git', ['status', '--porcelain'], { cwd: repoRoot })).stdout.trim().length > 0;
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
    gsplat_benchmark_frames: String(frames),
    gsplat_benchmark_warmup_frames: String(warmup),
    gsplat_surface_sort_interval: String(sortInterval),
    gsplat_surface_order_backend: orderBackend,
    gsplat_surface_projected_policy: projectedPolicy,
    benchmark_yaw_step: qualification ? '0' : '0.001'
  });
  params.set('gsplat_order_completion_protocol', orderCompletionProtocol);
  if (m4Smoke) params.set('gsplat_current_stats_smoke', 'true');
  if (gpuOrderProducer !== null) {
    params.set('gsplat_surface_gpu_order_producer', gpuOrderProducer);
  }
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
  const parsed = parseArtifacts(consoleLines);
  const artifactDir = await writeArtifact(parsed);
  const dataUrl = await page.$eval('#viewport', (canvas) => canvas.toDataURL('image/png'));
  const imagePath = resolve(artifactDir, 'final-frame.png');
  await writeFile(imagePath, Buffer.from(dataUrl.split(',')[1], 'base64'));
  await writeFile(resolve(artifactDir, 'browser-console.log'), `${consoleLines.join('\n')}\n`);
  const suitePath = truck1080pQualification
    ? await publishTruck1080pSuite({
        manifest: JSON.parse(parsed.manifests[0]),
        imagePath,
      })
    : null;
  const resultLine = consoleLines.find((line) => line.includes('BENCHMARK_RESULT '));
  const result = {
    status: 'ok',
    artifact_dir: artifactDir,
    result: resultLine ?? null,
  };
  if (suitePath !== null) result.full_quality_suite = suitePath;
  console.log(JSON.stringify(result));
  }
} catch (error) {
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
} finally {
  if (browser) {
    const browserProcess = browser.process?.();
    let cleanupTimer;
    const closed = await Promise.race([
      browser.close().then(() => true, () => true),
      new Promise((resolvePromise) => {
        cleanupTimer = setTimeout(() => resolvePromise(false), 2_000);
      })
    ]);
    if (cleanupTimer !== undefined) clearTimeout(cleanupTimer);
    if (!closed) browserProcess?.kill('SIGKILL');
  }
  if (server) {
    server.kill('SIGTERM');
    server.unref();
  }
}
process.exit(process.exitCode ?? 0);
