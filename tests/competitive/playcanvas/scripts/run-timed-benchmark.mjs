import { execFile as execFileCallback } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import os from 'node:os';
import { dirname, resolve } from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import process from 'node:process';
import puppeteer from 'puppeteer-core';
import { startServer } from './server.mjs';

const execFile = promisify(execFileCallback);
const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(harnessRoot, '..', '..', '..');
const qualification = process.env.PHASE_E_QUALIFICATION === 'kitsune-static-v1';
const outputRoot = resolve(
  process.env.PLAYCANVAS_ARTIFACT_DIR ??
    resolve(
      repoRoot,
      qualification
        ? 'target/benchmarks/phase-e/playcanvas-kitsune-static-v1'
        : 'target/benchmarks/playcanvas-collector-smoke'
    )
);
const chromeCandidates = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium'
].filter(Boolean);

async function findChrome() {
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {}
  }
  return null;
}

function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

function distribution(frames, metric) {
  const values = frames.map((frame) => frame[metric]).filter((value) => value !== null).sort((a, b) => a - b);
  if (values.length === 0) return null;
  const rank = (fraction) => values[Math.max(Math.ceil(fraction * values.length) - 1, 0)];
  return {
    count: values.length,
    mean: values.reduce((sum, value) => sum + value, 0) / values.length,
    p50: rank(0.5),
    p90: rank(0.9),
    p95: rank(0.95),
    p99: rank(0.99),
    max: values.at(-1)
  };
}

await mkdir(outputRoot, { recursive: true });
const chrome = await findChrome();
if (!chrome) {
  const blocker = { status: 'blocked', reason: 'no supported Chrome/Chromium executable found', candidates: chromeCandidates };
  await writeFile(resolve(outputRoot, 'blocker.json'), `${JSON.stringify(blocker, null, 2)}\n`);
  console.error(JSON.stringify(blocker));
  process.exit(2);
}

const startedAtUtc = new Date().toISOString();
const { server, port } = await startServer(0);
let browser;
try {
  browser = await puppeteer.launch({
    executablePath: chrome,
    headless: process.env.HEADLESS !== '0',
    defaultViewport: { width: 640, height: 480, deviceScaleFactor: 1 },
    args: ['--enable-unsafe-webgpu', '--enable-gpu', '--ignore-gpu-blocklist']
  });
  const page = await browser.newPage();
  const browserLog = [];
  page.on('console', (message) => browserLog.push(`${message.type()}: ${message.text()}`));
  page.on('pageerror', (error) => browserLog.push(`pageerror: ${error.stack ?? error.message}`));
  const qualificationQuery = qualification ? '&qualification=kitsune-static-v1' : '';
  await page.goto(`http://127.0.0.1:${port}/?benchmark=1${qualificationQuery}`, {
    waitUntil: 'networkidle0',
    timeout: qualification ? 120_000 : 30_000
  });
  await page.waitForFunction(
    () => window.__PLAYCANVAS_HARNESS_RESULT__ || window.__PLAYCANVAS_HARNESS_ERROR__,
    { timeout: qualification ? 180_000 : 60_000 }
  );
  const outcome = await page.evaluate(() => ({
    result: window.__PLAYCANVAS_HARNESS_RESULT__ ?? window.__PLAYCANVAS_HARNESS_ERROR__,
    userAgent: navigator.userAgent,
    platform: navigator.platform
  }));
  await writeFile(resolve(outputRoot, 'runtime.log'), `${browserLog.join('\n')}\n`);
  if (outcome.result.status !== 'raw_frame_capture_complete') throw new Error(JSON.stringify(outcome.result));

  const datasetName = qualification ? 'kitsune.json' : 'minimal_binary.json';
  const dataset = JSON.parse(await readFile(resolve(repoRoot, 'tests/perf/datasets', datasetName), 'utf8'));
  const expectedEngine = JSON.parse(await readFile(resolve(harnessRoot, 'expected-engine.json'), 'utf8'));
  const repositoryCommit = (await execFile('git', ['rev-parse', 'HEAD'], { cwd: repoRoot })).stdout.trim();
  const dirty = (await execFile('git', ['status', '--porcelain'], { cwd: repoRoot })).stdout.trim().length > 0;
  const scope = qualification ? 'phase-e-kitsune-static-v1' : 'playcanvas-collector-smoke';
  const runId = `${scope}-${randomUUID()}`;
  const frameBudgetMs = 1000 / 60;
  const nullMetrics = ['preprocess_ms', 'sort_ms', 'geometry_submit_ms', 'gpu_wait_ms', 'gpu_complete_ms'];
  const frames = outcome.result.capture.samples.map((sample, frameIndex) => ({
    schema: 'gsplat-benchmark/v1',
    record_type: 'frame',
    run_id: runId,
    frame_index: frameIndex,
    elapsed_ns: sample.elapsedNs,
    call_ms: sample.callMs,
    frame_wall_ms: sample.frameWallMs,
    preprocess_ms: null,
    sort_ms: null,
    geometry_submit_ms: null,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: sample.activeSplats,
    drawn: sample.activeSplats,
    sort_refreshed: null
  }));
  const unavailableFields = [
    'environment.adapter',
    'environment.driver',
    ...nullMetrics.map((metric) => `frames[*].${metric}`),
    'frames[*].sort_refreshed'
  ];
  const manifest = {
    schema: 'gsplat-benchmark/v1',
    record_type: 'manifest',
    run_id: runId,
    identity: {
      series_id: qualification ? 'phase-e-paired-kitsune-static-v1' : 'playcanvas-collector-smoke-v1',
      started_at_utc: startedAtUtc,
      ended_at_utc: new Date().toISOString(),
      measurement_started_at_utc: outcome.result.capture.measurementStartedAtUtc,
      measurement_ended_at_utc: outcome.result.capture.measurementEndedAtUtc
    },
    build: {
      repository_commit: repositoryCommit,
      dirty,
      profile: 'playcanvas-production-esm',
      package_version: expectedEngine.version
    },
    dataset: {
      id: dataset.id,
      sha256: dataset.sha256,
      bytes: dataset.bytes,
      splat_count: dataset.splat_count,
      sh_degree: dataset.sh_degree
    },
    trace: {
      id: outcome.result.traceDescriptor.trace_id ?? outcome.result.traceDescriptor.id,
      sha256: outcome.result.traceDescriptor.content_sha256 ?? sha256(JSON.stringify(outcome.result.traceDescriptor))
    },
    renderer: {
      implementation: `playcanvas-${outcome.result.engineRuntimeRevision}`,
      path: outcome.result.rendererPath,
      backend: outcome.result.backendSelected,
      sort_policy: outcome.result.rendererActive
    },
    display: {
      width: outcome.result.canvasBackingWidth,
      height: outcome.result.canvasBackingHeight,
      dpr: outcome.result.devicePixelRatio,
      refresh_hz: 60,
      frame_budget_ms: frameBudgetMs,
      refresh_hz_source: 'configured',
      frame_budget_source: 'configured'
    },
    environment: {
      platform: outcome.platform,
      os: `${os.type()} ${os.release()}`,
      device: os.hostname(),
      browser: `${await browser.version()} ${outcome.userAgent}`,
      adapter: null,
      driver: null
    },
    timing: outcome.result.capture.timing,
    policies: outcome.result.policies,
    pairing: qualification ? {
      pair_id: process.env.PHASE_E_PAIR_ID ?? null,
      run_order: process.env.PHASE_E_PAIR_ORDER ?? null,
      position: Number(process.env.PHASE_E_PAIR_POSITION ?? 0) || null
    } : undefined,
    qualification_scope: qualification ? 'phase_e_paired_candidate' : 'collector_smoke_only',
    unavailable_fields: unavailableFields
  };
  const distributions = Object.fromEntries(
    ['call_ms', 'frame_wall_ms', ...nullMetrics].map((metric) => [metric, distribution(frames, metric)])
  );
  const summary = {
    schema: 'gsplat-benchmark/v1',
    record_type: 'summary',
    run_id: runId,
    sample_count: frames.length,
    warmup_count: outcome.result.capture.warmupCount,
    frame_budget_ms: frameBudgetMs,
    missed_frame_count: frames.filter((frame) => frame.frame_wall_ms > frameBudgetMs).length,
    distributions
  };
  await writeFile(resolve(outputRoot, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(resolve(outputRoot, 'frames.jsonl'), `${frames.map((frame) => JSON.stringify(frame)).join('\n')}\n`);
  await writeFile(resolve(outputRoot, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
  if (qualification) {
    const dataUrl = await page.$eval('#canvas', (canvas) => canvas.toDataURL('image/png'));
    await writeFile(resolve(outputRoot, 'final-frame.png'), Buffer.from(dataUrl.split(',')[1], 'base64'));
  }
  await execFile('python3', [resolve(repoRoot, 'tests/perf/validate-benchmark-artifacts.py'), outputRoot]);
  console.log(JSON.stringify({
    status: qualification ? 'valid_paired_candidate' : 'valid_collector_smoke',
    outputRoot,
    runId,
    sampleCount: frames.length
  }));
} catch (error) {
  const blocker = { status: 'blocked', reason: error.message, stack: error.stack, chrome };
  await writeFile(resolve(outputRoot, 'blocker.json'), `${JSON.stringify(blocker, null, 2)}\n`);
  console.error(JSON.stringify(blocker));
  process.exitCode = 2;
} finally {
  await browser?.close();
  await new Promise((resolvePromise) => server.close(resolvePromise));
}
