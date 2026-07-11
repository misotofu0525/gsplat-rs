#!/usr/bin/env node
/**
 * Headless Chrome collector for gsplat-rs Web Phase A baseline artifacts.
 * Emits a validated gsplat-benchmark/v1 directory from console JSON lines.
 */
import { access, mkdir, writeFile, rename, rm } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';
import process from 'node:process';

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

const frames = Number(process.env.GSPLAT_BENCHMARK_FRAMES ?? 30);
const warmup = Number(process.env.GSPLAT_BENCHMARK_WARMUP_FRAMES ?? 5);
const dataset = process.env.GSPLAT_DATASET ?? '';
const outDir = resolve(
  process.env.GSPLAT_ARTIFACT_DIR ??
    resolve(repoRoot, 'target/benchmarks/phase-a/web-minimal-v1')
);
const port = Number(process.env.GSPLAT_HTTP_PORT ?? 4173);

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

function parseArtifacts(consoleLines) {
  const manifests = [];
  const frameRecords = [];
  const summaries = [];
  for (const line of consoleLines) {
    const text = line.includes(': ') ? line.slice(line.indexOf(': ') + 2) : line;
    if (text.startsWith('BENCHMARK_MANIFEST_JSON ')) {
      manifests.push(text.slice('BENCHMARK_MANIFEST_JSON '.length));
    } else if (text.startsWith('BENCHMARK_FRAME_JSON ')) {
      frameRecords.push(text.slice('BENCHMARK_FRAME_JSON '.length));
    } else if (text.startsWith('BENCHMARK_SUMMARY_JSON ')) {
      summaries.push(text.slice('BENCHMARK_SUMMARY_JSON '.length));
    }
  }
  if (manifests.length !== 1 || summaries.length !== 1 || frameRecords.length === 0) {
    throw new Error(
      `expected one manifest/summary and frames; got manifest=${manifests.length} frame=${frameRecords.length} summary=${summaries.length}`
    );
  }
  for (const payload of [...manifests, ...frameRecords, ...summaries]) {
    JSON.parse(payload);
  }
  return { manifests, frameRecords, summaries };
}

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function writeArtifact({ manifests, frameRecords, summaries }) {
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
  await new Promise((resolvePromise, reject) => {
    const child = spawn(
      process.env.PYTHON ?? 'python3',
      [resolve(repoRoot, 'tests/perf/validate-benchmark-artifacts.py'), sibling],
      { stdio: 'inherit' }
    );
    child.once('error', reject);
    child.once('exit', (code) => {
      if (code === 0) resolvePromise();
      else reject(new Error(`validator exited ${code}`));
    });
  });
  await rename(sibling, outDir);
  return outDir;
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
  page.on('console', (message) => {
    consoleLines.push(`${message.type()}: ${message.text()}`);
  });
  page.on('pageerror', (error) => {
    consoleLines.push(`pageerror: ${error.stack ?? error.message}`);
  });
  const params = new URLSearchParams({
    gsplat_benchmark: 'true',
    gsplat_benchmark_sync: 'true',
    gsplat_benchmark_frames: String(frames),
    gsplat_benchmark_warmup_frames: String(warmup),
    gsplat_surface_sort_interval: '2'
  });
  if (dataset) params.set('dataset', dataset);
  const url = `http://127.0.0.1:${port}/examples/web/?${params.toString()}`;
  await page.goto(url, { waitUntil: 'networkidle0', timeout: 120_000 });
  await page.waitForFunction(
    () => {
      const el = document.getElementById('benchmarkStatus');
      return el && el.textContent === 'complete';
    },
    { timeout: 180_000 }
  );
  await new Promise((r) => setTimeout(r, 1500));
  const parsed = parseArtifacts(consoleLines);
  const artifactDir = await writeArtifact(parsed);
  const resultLine = consoleLines.find((line) => line.includes('BENCHMARK_RESULT '));
  console.log(JSON.stringify({ status: 'ok', artifact_dir: artifactDir, result: resultLine ?? null }));
} catch (error) {
  const logPath = resolve(repoRoot, 'target/benchmarks/phase-a/web-collector-failure.log');
  await mkdir(dirname(logPath), { recursive: true });
  await writeFile(logPath, `${consoleLines.join('\n')}\n\n${error.stack ?? error}\n`);
  console.error(JSON.stringify({ status: 'failed', reason: error.message, log: logPath }));
  process.exitCode = 1;
} finally {
  await browser?.close();
  if (server) {
    server.kill('SIGTERM');
  }
}
