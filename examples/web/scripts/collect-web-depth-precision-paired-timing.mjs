#!/usr/bin/env node

import { execFile as execFileCallback } from "node:child_process";
import { createHash } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import { access, lstat, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";

import {
  WEB_DEPTH_PAIRED_TIMING,
  classifyPairs,
  schedulePairs,
  validateCollectedRuns,
  validateK1dAdmission,
} from "../src/depth-precision-paired-timing.mjs";

const execFile = promisify(execFileCallback);
const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "../../..");
const sdkModulePath = resolve(repoRoot, "packages/web/src/index.js");
const timingModulePath = resolve(repoRoot, "examples/web/src/depth-precision-paired-timing.mjs");
const matrixPath = resolve(repoRoot, "tests/perf/full-quality-matrix-plan-v1.json");
const qualityValidatorPath = resolve(repoRoot, "tests/perf/validate-balanced-image-gate.py");

function usage() {
  return "usage: node examples/web/scripts/collect-web-depth-precision-paired-timing.mjs "
    + "--exact-pkg <qualified-exact-pkg> --candidate-pkg <qualified-candidate20-pkg> "
    + "--quality-suite <retained-k1d-suite.json> --expected-commit <full-sha> "
    + "--chrome <executable> --output <fresh-dir> [--pairs <n>=3] [--seed <u32>]";
}

function positiveInteger(value, label, { zero = false } = {}) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < (zero ? 0 : 1)) {
    throw new Error(`${label} must be a ${zero ? "non-negative" : "positive"} safe integer`);
  }
  return parsed;
}

export function parseArguments(argv) {
  const required = ["--exact-pkg", "--candidate-pkg", "--quality-suite", "--expected-commit",
    "--chrome", "--output"];
  const allowed = new Set([...required, "--pairs", "--seed"]);
  const values = new Map();
  if (argv.length % 2 !== 0) throw new Error(usage());
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!allowed.has(key) || !value || values.has(key)) throw new Error(usage());
    values.set(key, value);
  }
  if (required.some((key) => !values.has(key))) throw new Error(usage());
  const commit = values.get("--expected-commit");
  if (!/^[0-9a-f]{40}$/.test(commit)) throw new Error("--expected-commit must be a full SHA");
  const pairs = positiveInteger(
    values.get("--pairs") ?? WEB_DEPTH_PAIRED_TIMING.defaultPairs,
    "--pairs",
  );
  if (pairs < WEB_DEPTH_PAIRED_TIMING.minimumPairs) {
    throw new Error(`--pairs must be at least ${WEB_DEPTH_PAIRED_TIMING.minimumPairs}`);
  }
  const seed = positiveInteger(
    values.get("--seed") ?? WEB_DEPTH_PAIRED_TIMING.defaultSeed,
    "--seed",
    { zero: true },
  );
  if (seed > 0xffffffff) throw new Error("--seed must fit in u32");
  return Object.freeze({
    exactPkg: resolve(values.get("--exact-pkg")),
    candidatePkg: resolve(values.get("--candidate-pkg")),
    qualitySuite: resolve(values.get("--quality-suite")),
    expectedCommit: commit,
    chrome: resolve(values.get("--chrome")),
    output: resolve(values.get("--output")),
    pairs,
    seed,
  });
}

async function requireFile(path, mode, label) {
  try {
    await access(path, mode);
    if (!(await stat(path)).isFile()) throw new Error(`${label} is not a file: ${path}`);
  } catch (error) {
    if (error.message?.startsWith(`${label} is not a file`)) throw error;
    throw new Error(`${label} is unavailable: ${path}`);
  }
}

async function requireFreshDestination(path) {
  try {
    await lstat(path);
  } catch (error) {
    if (error?.code === "ENOENT") return;
    throw error;
  }
  throw new Error(`output must be fresh and is preserved: ${path}`);
}

async function sha256File(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

async function cleanExactCommit(expectedCommit) {
  const [{ stdout: revision }, { stdout: status }] = await Promise.all([
    execFile("git", ["rev-parse", "HEAD"], { cwd: repoRoot }),
    execFile("git", ["status", "--porcelain"], { cwd: repoRoot }),
  ]);
  if (status.trim()) throw new Error("paired timing requires a clean committed worktree");
  if (revision.trim() !== expectedCommit) {
    throw new Error(`worktree HEAD differs from --expected-commit: ${revision.trim()}`);
  }
  return revision.trim();
}

function matrixEntry(values, id, label) {
  const entry = values.find((value) => value && typeof value === "object" && value.id === id);
  if (!entry) throw new Error(`${label} ${id} is absent from the committed matrix`);
  return entry;
}

async function loadTruckWorkload() {
  const matrix = JSON.parse(await readFile(matrixPath, "utf8"));
  const dataset = matrixEntry(matrix.datasets ?? [], "truck-full", "dataset");
  const traceEntry = matrixEntry(
    matrix.traces ?? [],
    "candidate-truck-quality-2view-1920x1080-v1",
    "trace",
  );
  if (dataset.role !== "full_scene" || traceEntry.dataset_id !== dataset.id
      || traceEntry.width !== WEB_DEPTH_PAIRED_TIMING.width
      || traceEntry.height !== WEB_DEPTH_PAIRED_TIMING.height
      || dataset.splat_count !== WEB_DEPTH_PAIRED_TIMING.sourceCount
      || dataset.sh_degree !== WEB_DEPTH_PAIRED_TIMING.shDegree) {
    throw new Error("committed matrix does not describe complete Truck moving 1080p");
  }
  const datasetPath = resolve(repoRoot, dataset.local_path);
  const tracePath = resolve(repoRoot, traceEntry.local_path);
  await Promise.all([
    requireFile(datasetPath, constants.R_OK, "complete Truck PLY"),
    requireFile(tracePath, constants.R_OK, "Truck moving trace"),
  ]);
  const datasetStatus = await stat(datasetPath);
  if (datasetStatus.size !== dataset.bytes || await sha256File(datasetPath) !== dataset.sha256) {
    throw new Error("complete Truck PLY bytes differ from the committed matrix");
  }
  const trace = JSON.parse(await readFile(tracePath, "utf8"));
  if (trace.trace_id !== traceEntry.id || trace.content_sha256 !== traceEntry.sha256
      || trace.display?.width !== WEB_DEPTH_PAIRED_TIMING.width
      || trace.display?.height !== WEB_DEPTH_PAIRED_TIMING.height
      || !Array.isArray(trace.frames) || trace.frames.length < 2
      || trace.derivation?.source_sha256 !== dataset.sha256
      || trace.derivation?.source_splat_count !== dataset.splat_count
      || trace.derivation?.source_sh_degree !== dataset.sh_degree) {
    throw new Error("Truck trace identity differs from the committed matrix");
  }
  return Object.freeze({
    dataset: Object.freeze({ ...dataset }),
    datasetPath,
    trace,
    tracePath,
    matrixSha256: await sha256File(matrixPath),
    traceFileSha256: await sha256File(tracePath),
  });
}

async function validateQualitySuite(path) {
  const result = await execFile(
    "python3",
    [qualityValidatorPath, path],
    { cwd: repoRoot },
  ).catch((error) => ({ stdout: error.stdout ?? "", stderr: error.stderr ?? "", error }));
  if (result.error) {
    const detail = result.stderr.trim() || result.stdout.trim() || result.error.message;
    throw new Error(`retained K1d quality suite is invalid: ${detail}`);
  }
}

async function loadPuppeteer() {
  const candidates = [
    "tests/competitive/playcanvas/node_modules/puppeteer-core/lib/esm/puppeteer/puppeteer-core.js",
    "tests/competitive/playcanvas/node_modules/puppeteer-core/lib/cjs/puppeteer/puppeteer-core.js",
    "tests/competitive/playcanvas/node_modules/puppeteer-core/index.js",
  ].map((path) => resolve(repoRoot, path));
  for (const candidate of candidates) {
    try {
      await access(candidate, constants.R_OK);
      const module = await import(pathToFileURL(candidate).href);
      return module.default ?? module;
    } catch {}
  }
  throw new Error("pinned puppeteer-core is unavailable under tests/competitive/playcanvas");
}

function browserDocument() {
  return `<!doctype html><html><body><script type="module">
import { createGsplatRendererFromUrl } from "/sdk.mjs";
import { collectLaneTiming } from "/timing.mjs";
const outcome = { status: "failed", runs: [] };
try {
  if (!navigator.gpu) throw new Error("navigator.gpu is unavailable");
  const config = await fetch("/config.json").then((response) => response.json());
  const dataset = await fetch("/dataset.json").then((response) => response.json());
  const trace = await fetch("/trace.json").then((response) => response.json());
  const modules = {};
  for (const lane of ["exact", "candidate"]) {
    const module = await import("/pkg/" + lane + "/gsplat_web.js");
    await module.default({ module_or_path: "/pkg/" + lane + "/gsplat_web_bg.wasm" });
    modules[lane] = module;
  }
  for (const pair of config.schedule) {
    for (let position = 0; position < pair.order.length; position += 1) {
      const lane = pair.order[position];
      const canvas = document.createElement("canvas");
      canvas.width = config.width;
      canvas.height = config.height;
      canvas.style.width = config.width + "px";
      canvas.style.height = config.height + "px";
      document.body.append(canvas);
      const renderer = await createGsplatRendererFromUrl({
        canvas,
        url: "/truck.ply",
        width: config.width,
        height: config.height,
        sortInterval: 1,
        orderBackend: "gpu",
        projectedPolicy: "compact",
        gpuOrderProducer: "preproject",
        geometryPath: "packed",
        module: modules[lane],
      });
      try {
        outcome.runs.push(await collectLaneTiming({
          renderer,
          lane,
          pairIndex: pair.pair_index,
          position: position + 1,
          trace,
          dataset,
          warmupFrames: config.warmup_frames,
          measuredFrames: config.measured_frames,
        }));
      } finally {
        renderer.dispose();
        canvas.remove();
      }
      await new Promise((resolveFrame) => requestAnimationFrame(resolveFrame));
    }
  }
  outcome.status = "complete";
} catch (error) {
  outcome.message = error?.message ?? String(error);
  outcome.stack = error?.stack ?? null;
}
globalThis.__GSPLAT_WEB_DEPTH_PAIRED_TIMING__ = outcome;
</script></body></html>`;
}

function contentType(path) {
  return ({ ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm",
    ".json": "application/json", ".ply": "application/octet-stream" })[extname(path)]
    ?? "application/octet-stream";
}

function startServer({ paths, workload, config }) {
  const fixedRoutes = new Map([
    ["/sdk.mjs", sdkModulePath],
    ["/timing.mjs", timingModulePath],
    ["/trace.json", workload.tracePath],
    ["/pkg/exact/gsplat_web.js", paths.exactJs],
    ["/pkg/exact/gsplat_web_bg.wasm", paths.exactWasm],
    ["/pkg/candidate/gsplat_web.js", paths.candidateJs],
    ["/pkg/candidate/gsplat_web_bg.wasm", paths.candidateWasm],
  ]);
  const server = createServer((request, response) => {
    const pathname = new URL(request.url, "http://127.0.0.1").pathname;
    if (pathname === "/") {
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      response.end(browserDocument());
      return;
    }
    if (pathname === "/config.json" || pathname === "/dataset.json") {
      const value = pathname === "/config.json" ? config : workload.dataset;
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify(value));
      return;
    }
    const file = pathname === "/truck.ply" ? workload.datasetPath : fixedRoutes.get(pathname);
    if (!file) {
      response.writeHead(404);
      response.end("not found");
      return;
    }
    stat(file).then((status) => {
      response.writeHead(200, {
        "content-type": contentType(file),
        "content-length": status.size,
        "cache-control": "no-store",
      });
      createReadStream(file).on("error", (error) => response.destroy(error)).pipe(response);
    }).catch((error) => {
      response.writeHead(500);
      response.end(error.message);
    });
  });
  return new Promise((resolveServer, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolveServer({ server, port: server.address().port }));
  });
}

async function closeServer(server) {
  if (!server) return;
  await new Promise((resolveClose) => server.close(resolveClose));
}

async function runBrowser({ puppeteer, args, paths, workload, config, browserLog }) {
  const running = await startServer({ paths, workload, config });
  let browser = null;
  try {
    browser = await puppeteer.launch({
      executablePath: args.chrome,
      headless: true,
      defaultViewport: {
        width: WEB_DEPTH_PAIRED_TIMING.width,
        height: WEB_DEPTH_PAIRED_TIMING.height,
        deviceScaleFactor: 1,
      },
      args: ["--enable-unsafe-webgpu", "--enable-gpu", "--ignore-gpu-blocklist"],
    });
    const page = await browser.newPage();
    page.on("console", (message) => browserLog.push(`${message.type()}: ${message.text()}`));
    page.on("pageerror", (error) => browserLog.push(`pageerror: ${error.stack ?? error.message}`));
    await page.goto(`http://127.0.0.1:${running.port}/`, {
      waitUntil: "networkidle0",
      timeout: 600_000,
    });
    await page.waitForFunction(
      () => globalThis.__GSPLAT_WEB_DEPTH_PAIRED_TIMING__ !== undefined,
      { timeout: 7_200_000 },
    );
    const outcome = await page.evaluate(() => globalThis.__GSPLAT_WEB_DEPTH_PAIRED_TIMING__);
    if (outcome.status !== "complete") throw new Error(outcome.message ?? "browser timing failed");
    return {
      outcome,
      environment: await page.evaluate(() => ({
        user_agent: navigator.userAgent,
        platform: navigator.platform,
        webgpu: Boolean(navigator.gpu),
      })),
    };
  } finally {
    await browser?.close().catch(() => {});
    await closeServer(running.server);
  }
}

function artifactName(run) {
  return `pair-${String(run.pair_index).padStart(2, "0")}-position-${run.pair_position}-${run.lane}.json`;
}

async function publishSuccess({ args, config, inputs, browserResult, browserLog }) {
  validateCollectedRuns(browserResult.outcome.runs, config.schedule, {
    warmupFrames: config.warmup_frames,
    measuredFrames: config.measured_frames,
  });
  const runsDirectory = resolve(args.output, "runs");
  await mkdir(runsDirectory);
  const runReceipts = [];
  for (const run of browserResult.outcome.runs) {
    const name = artifactName(run);
    const path = resolve(runsDirectory, name);
    await writeFile(path, `${JSON.stringify(run, null, 2)}\n`, { flag: "wx" });
    runReceipts.push({
      pair_index: run.pair_index,
      pair_position: run.pair_position,
      lane: run.lane,
      path: `runs/${name}`,
      sha256: await sha256File(path),
      terminal_ms_per_frame: run.terminal_ms_per_frame,
      frame_wall_mean_ms: run.frame_wall_mean_ms,
    });
  }
  const result = classifyPairs(browserResult.outcome.runs);
  const suite = {
    schema: WEB_DEPTH_PAIRED_TIMING.schema,
    status: "complete",
    scope: "gsplat-rs-only-web-candidate20-paired-timing",
    q1_acceptance: false,
    competitor_comparison: "not_applicable",
    renderer_commit: args.expectedCommit,
    collector_commit: args.expectedCommit,
    inputs,
    environment: browserResult.environment,
    workload: {
      dataset_id: "truck-full",
      source_splat_count: WEB_DEPTH_PAIRED_TIMING.sourceCount,
      source_sh_degree: WEB_DEPTH_PAIRED_TIMING.shDegree,
      trace_id: "candidate-truck-quality-2view-1920x1080-v1",
      camera_mode: "moving_sequence",
      width: WEB_DEPTH_PAIRED_TIMING.width,
      height: WEB_DEPTH_PAIRED_TIMING.height,
    },
    protocol: {
      pairs: args.pairs,
      seed: args.seed,
      schedule: config.schedule,
      warmup_frames: WEB_DEPTH_PAIRED_TIMING.warmupFrames,
      measured_frames: WEB_DEPTH_PAIRED_TIMING.measuredFrames,
      capture_during_timing: false,
      current_stats_terminals_per_run: 2,
      automatic_retry: false,
      executions_per_scheduled_run: 1,
    },
    runs: runReceipts,
    result,
  };
  await Promise.all([
    writeFile(resolve(args.output, "suite.json"), `${JSON.stringify(suite, null, 2)}\n`, {
      flag: "wx",
    }),
    writeFile(resolve(args.output, "browser-console.log"), `${browserLog.join("\n")}\n`, {
      flag: "wx",
    }),
  ]);
  return suite;
}

export async function collect(args, { executeBrowser = runBrowser } = {}) {
  const paths = {
    exactJs: resolve(args.exactPkg, "gsplat_web.js"),
    exactWasm: resolve(args.exactPkg, "gsplat_web_bg.wasm"),
    exactReceipt: resolve(args.exactPkg, "gsplat_web_build_receipt.json"),
    candidateJs: resolve(args.candidatePkg, "gsplat_web.js"),
    candidateWasm: resolve(args.candidatePkg, "gsplat_web_bg.wasm"),
    candidateReceipt: resolve(args.candidatePkg, "gsplat_web_build_receipt.json"),
  };
  await requireFreshDestination(args.output);
  await Promise.all([
    requireFile(args.chrome, constants.X_OK, "Chrome executable"),
    requireFile(args.qualitySuite, constants.R_OK, "retained K1d suite"),
    requireFile(sdkModulePath, constants.R_OK, "Web SDK module"),
    requireFile(timingModulePath, constants.R_OK, "paired timing module"),
    ...Object.entries(paths).map(([label, path]) => requireFile(path, constants.R_OK, label)),
  ]);
  if (args.exactPkg === args.candidatePkg) {
    throw new Error("Exact and Candidate20 packages must be independent");
  }
  await cleanExactCommit(args.expectedCommit);
  await validateQualitySuite(args.qualitySuite);
  const [workload, suite, exactBuild, candidateBuild, puppeteer] = await Promise.all([
    loadTruckWorkload(),
    readFile(args.qualitySuite, "utf8").then((text) => JSON.parse(text)),
    readFile(paths.exactReceipt, "utf8").then((text) => JSON.parse(text)),
    readFile(paths.candidateReceipt, "utf8").then((text) => JSON.parse(text)),
    loadPuppeteer(),
  ]);
  const packageHashes = {
    exact_js_sha256: await sha256File(paths.exactJs),
    exact_wasm_sha256: await sha256File(paths.exactWasm),
    candidate_js_sha256: await sha256File(paths.candidateJs),
    candidate_wasm_sha256: await sha256File(paths.candidateWasm),
    exact_build_receipt_sha256: await sha256File(paths.exactReceipt),
    candidate_build_receipt_sha256: await sha256File(paths.candidateReceipt),
  };
  const admission = validateK1dAdmission({
    suite,
    expectedCommit: args.expectedCommit,
    exactBuild,
    candidateBuild,
    packageHashes,
  });
  const config = {
    width: WEB_DEPTH_PAIRED_TIMING.width,
    height: WEB_DEPTH_PAIRED_TIMING.height,
    warmup_frames: WEB_DEPTH_PAIRED_TIMING.warmupFrames,
    measured_frames: WEB_DEPTH_PAIRED_TIMING.measuredFrames,
    schedule: schedulePairs(args.pairs, args.seed),
  };
  const inputs = {
    quality_suite: {
      path: args.qualitySuite,
      sha256: await sha256File(args.qualitySuite),
      admission,
    },
    packages: packageHashes,
    matrix_sha256: workload.matrixSha256,
    dataset_sha256: workload.dataset.sha256,
    dataset_bytes: workload.dataset.bytes,
    trace_content_sha256: workload.trace.content_sha256,
    trace_file_sha256: workload.traceFileSha256,
  };

  await mkdir(dirname(args.output), { recursive: true });
  await mkdir(args.output);
  await writeFile(resolve(args.output, "attempt.json"), `${JSON.stringify({
    schema: WEB_DEPTH_PAIRED_TIMING.schema,
    status: "claimed",
    expected_commit: args.expectedCommit,
    one_execution_no_retry: true,
    config,
    inputs,
  }, null, 2)}\n`, { flag: "wx" });
  const browserLog = [];
  try {
    const browserResult = await executeBrowser({
      puppeteer,
      args,
      paths,
      workload,
      config,
      browserLog,
    });
    // The server reads the SDK and timing modules directly from this worktree.
    // Revalidate immediately before publication so a concurrent edit cannot
    // inherit the preflight commit receipt.
    await cleanExactCommit(args.expectedCommit);
    const published = await publishSuccess({
      args,
      config,
      inputs,
      browserResult,
      browserLog,
    });
    console.log(JSON.stringify({
      status: "complete",
      outcome: published.result.outcome,
      suite: resolve(args.output, "suite.json"),
    }));
    return published;
  } catch (error) {
    await writeFile(resolve(args.output, "failure.json"), `${JSON.stringify({
      schema: WEB_DEPTH_PAIRED_TIMING.schema,
      status: "failed",
      automatic_retry: false,
      message: error.message,
      stack: error.stack ?? null,
      browser_log: browserLog,
    }, null, 2)}\n`, { flag: "wx" });
    throw error;
  }
}

async function main() {
  await collect(parseArguments(process.argv.slice(2)));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.stack ?? error.message);
    process.exitCode = 1;
  });
}
