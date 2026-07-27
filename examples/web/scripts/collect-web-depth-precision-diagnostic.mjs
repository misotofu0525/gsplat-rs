#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFile as execFileCallback } from "node:child_process";
import { access, lstat, mkdir, readFile, writeFile } from "node:fs/promises";
import { constants } from "node:fs";
import { createServer } from "node:http";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";

import {
  DEPTH_PRECISION_CONSOLE_PREFIX,
  createDepthPrecisionDiagnosticArtifact,
  parsePresentedDepthPrecisionConsoleLine,
  validateDepthPrecisionDiagnostic,
} from "../src/depth-precision-diagnostic.mjs";

const execFile = promisify(execFileCallback);
const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "../../..");
const sdkPath = resolve(repoRoot, "packages/web/src/index.js");
const minimalPlyPath = resolve(repoRoot, "tests/datasets/minimal_binary.ply");
const successfulPresentCount = 3;

function usage() {
  return "usage: node examples/web/scripts/collect-web-depth-precision-diagnostic.mjs " +
    "--pkg <candidate20-pkg-dir> --chrome <executable> --output <fresh-dir>";
}

function parseArguments(argv) {
  const allowed = new Set(["--pkg", "--chrome", "--output"]);
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!allowed.has(key) || value === undefined || value.length === 0 || values.has(key)) {
      throw new Error(usage());
    }
    values.set(key, value);
  }
  if (values.size !== allowed.size) throw new Error(usage());
  return {
    pkgDir: resolve(values.get("--pkg")),
    chrome: resolve(values.get("--chrome")),
    outputDir: resolve(values.get("--output")),
  };
}

async function requireFile(path, mode, label) {
  try {
    await access(path, mode);
  } catch {
    throw new Error(`${label} is unavailable: ${path}`);
  }
}

async function requireFreshDirectory(path) {
  await mkdir(dirname(path), { recursive: true });
  try {
    await lstat(path);
  } catch (error) {
    if (error?.code === "ENOENT") {
      await mkdir(path);
      return;
    }
    throw error;
  }
  throw new Error(`output must be fresh and is preserved: ${path}`);
}

async function sha256(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
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
  const prefix = JSON.stringify(DEPTH_PRECISION_CONSOLE_PREFIX);
  return `<!doctype html>
<html><body><canvas id="canvas" width="64" height="64"></canvas>
<script type="module">
import { createGsplatRenderer, initGsplatWeb } from "/sdk.js";

const receiptPrefix = ${prefix};
const receiptLines = [];
const originalLog = console.log.bind(console);
console.log = (...values) => {
  const line = values.map(String).join(" ");
  if (line.startsWith(receiptPrefix)) receiptLines.push(line);
  originalLog(...values);
};
const nextFrame = () => new Promise((resolveFrame) => requestAnimationFrame(resolveFrame));
let renderer = null;
try {
  if (!navigator.gpu) throw new Error("navigator.gpu is unavailable");
  const module = await import("/candidate/gsplat_web.js");
  await initGsplatWeb({ module, wasmUrl: "/candidate/gsplat_web_bg.wasm" });
  const response = await fetch("/minimal.ply");
  if (!response.ok) throw new Error("minimal PLY fetch failed: " + response.status);
  const plyBytes = new Uint8Array(await response.arrayBuffer());
  renderer = await createGsplatRenderer({
    canvas: document.querySelector("#canvas"),
    plyBytes,
    width: 64,
    height: 64,
    sortInterval: 1,
    geometryPath: "packed",
    orderBackend: "gpu",
    projectedPolicy: "compact",
    gpuOrderProducer: "preproject",
    module,
  });
  const frames = [];
  for (let attempt = 0; attempt < 120 && frames.length < ${successfulPresentCount}; attempt += 1) {
    const frame = renderer.renderFrame();
    if (frame.framePresented) {
      frames.push({
        frame_presented: true,
        camera_revision: frame.cameraRevision,
        applied_order_revision: frame.appliedOrderRevision,
        presented_order_revision_lag: frame.presentedOrderRevisionLag,
        order_backend: frame.orderBackend,
        projected_execution: frame.projectedExecution,
        gpu_order_producer: frame.gpuOrderProducer,
        raster_execution_plan: frame.rasterExecutionPlan,
      });
    }
    if (frames.length < ${successfulPresentCount}) await nextFrame();
  }
  if (frames.length !== ${successfulPresentCount}) {
    throw new Error("expected ${successfulPresentCount} successful presents, got " + frames.length);
  }
  window.__GSPLAT_DEPTH_DIAGNOSTIC_RESULT__ = {
    status: "complete",
    runtime: { raster_path: renderer.rasterPath() },
    frames,
    receiptLines,
  };
} catch (error) {
  window.__GSPLAT_DEPTH_DIAGNOSTIC_RESULT__ = {
    status: "failed",
    message: error?.message ?? String(error),
    stack: error?.stack ?? null,
    receiptLines,
  };
} finally {
  renderer?.free();
}
</script></body></html>`;
}

function startServer(paths) {
  const routes = new Map([
    ["/", { type: "text/html; charset=utf-8", body: browserDocument() }],
    ["/sdk.js", { type: "text/javascript; charset=utf-8", path: sdkPath }],
    ["/candidate/gsplat_web.js", { type: "text/javascript; charset=utf-8", path: paths.js }],
    ["/candidate/gsplat_web_bg.wasm", { type: "application/wasm", path: paths.wasm }],
    ["/minimal.ply", { type: "application/octet-stream", path: minimalPlyPath }],
  ]);
  const server = createServer(async (request, response) => {
    try {
      const route = routes.get(new URL(request.url, "http://127.0.0.1").pathname);
      if (!route) {
        response.writeHead(404).end("not found");
        return;
      }
      response.writeHead(200, { "content-type": route.type, "cache-control": "no-store" });
      response.end(route.body ?? await readFile(route.path));
    } catch (error) {
      response.writeHead(500).end(error.message);
    }
  });
  return new Promise((resolveServer, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      resolveServer({ server, port: address.port });
    });
  });
}

async function closeServer(server) {
  if (!server) return;
  await new Promise((resolveClose) => server.close(resolveClose));
}

async function main() {
  const inputs = parseArguments(process.argv.slice(2));
  const candidateJs = resolve(inputs.pkgDir, "gsplat_web.js");
  const candidateWasm = resolve(inputs.pkgDir, "gsplat_web_bg.wasm");
  await requireFile(inputs.chrome, constants.X_OK, "Chrome executable");
  await requireFile(candidateJs, constants.R_OK, "Candidate20 JS package");
  await requireFile(candidateWasm, constants.R_OK, "Candidate20 WASM package");
  await requireFile(minimalPlyPath, constants.R_OK, "minimal PLY");
  await requireFreshDirectory(inputs.outputDir);

  const browserConsole = [];
  let server = null;
  let browser = null;
  try {
    const puppeteer = await loadPuppeteer();
    const running = await startServer({ js: candidateJs, wasm: candidateWasm });
    server = running.server;
    browser = await puppeteer.launch({
      executablePath: inputs.chrome,
      headless: true,
      defaultViewport: { width: 64, height: 64, deviceScaleFactor: 1 },
      args: ["--enable-unsafe-webgpu", "--enable-gpu", "--ignore-gpu-blocklist"],
    });
    const page = await browser.newPage();
    page.on("console", (message) => browserConsole.push(`${message.type()}: ${message.text()}`));
    page.on("pageerror", (error) => browserConsole.push(`pageerror: ${error.stack ?? error.message}`));
    await page.goto(`http://127.0.0.1:${running.port}/`, {
      waitUntil: "networkidle0",
      timeout: 120_000,
    });
    await page.waitForFunction(() => window.__GSPLAT_DEPTH_DIAGNOSTIC_RESULT__ !== undefined, {
      timeout: 120_000,
    });
    const outcome = await page.evaluate(() => window.__GSPLAT_DEPTH_DIAGNOSTIC_RESULT__);
    await page.evaluate(() => new Promise((resolveFrame) => requestAnimationFrame(resolveFrame)));
    if (outcome.status !== "complete") throw new Error(outcome.message ?? "browser diagnostic failed");

    const pageReceipts = outcome.receiptLines.map(parsePresentedDepthPrecisionConsoleLine);
    const hostReceipts = browserConsole
      .map(parsePresentedDepthPrecisionConsoleLine)
      .filter((value) => value !== null);
    validateDepthPrecisionDiagnostic({
      frames: outcome.frames,
      receipts: pageReceipts,
      runtime: outcome.runtime,
    });
    if (JSON.stringify(hostReceipts) !== JSON.stringify(pageReceipts)) {
      throw new Error("host console receipts do not exactly match the in-page console ledger");
    }

    const [{ stdout: commit }, { stdout: status }] = await Promise.all([
      execFile("git", ["rev-parse", "HEAD"], { cwd: repoRoot }),
      execFile("git", ["status", "--porcelain"], { cwd: repoRoot }),
    ]);
    const artifact = createDepthPrecisionDiagnosticArtifact({
      identity: {
        commit: commit.trim(),
        dirty: status.trim().length > 0,
        created_at_utc: new Date().toISOString(),
      },
      inputs: {
        dataset_id: "minimal_binary",
        dataset_sha256: await sha256(minimalPlyPath),
        candidate_js_sha256: await sha256(candidateJs),
        candidate_wasm_sha256: await sha256(candidateWasm),
        chrome_executable: inputs.chrome,
      },
      runtime: outcome.runtime,
      frames: outcome.frames,
      receipts: pageReceipts,
    });
    await writeFile(
      resolve(inputs.outputDir, "browser-console.log"),
      `${browserConsole.join("\n")}\n`,
    );
    await writeFile(
      resolve(inputs.outputDir, "artifact.json"),
      `${JSON.stringify(artifact, null, 2)}\n`,
    );
    console.log(JSON.stringify({ status: "accepted_diagnostic", output: inputs.outputDir }));
  } catch (error) {
    await writeFile(
      resolve(inputs.outputDir, "failure.json"),
      `${JSON.stringify({ status: "rejected", message: error.message, stack: error.stack }, null, 2)}\n`,
    );
    throw error;
  } finally {
    await browser?.close();
    await closeServer(server);
  }
}

main().catch((error) => {
  console.error(error.stack ?? error.message);
  process.exitCode = 1;
});
