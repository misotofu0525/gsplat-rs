#!/usr/bin/env node

import { execFile as execFileCallback } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import { access, lstat, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { deflateSync } from "node:zlib";

import {
  WEB_DEPTH_QUALITY,
  computeFrameMetrics,
  computeTemporalMetric,
  timingFromFrame,
  validateMatchedPair,
} from "../src/depth-precision-quality.mjs";

const execFile = promisify(execFileCallback);
const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "../../..");
const sourceModule = resolve(repoRoot, "examples/web/src/depth-precision-quality.mjs");
const datasetManifestPath = resolve(repoRoot, "tests/perf/datasets/kitsune.json");
const tracePath = resolve(
  repoRoot,
  "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json",
);
const plyPath = resolve(repoRoot, "tests/datasets/external/wakufactory_kitune/kitune1.ply");
const benchmarkValidator = resolve(repoRoot, "tests/perf/validate-benchmark-artifacts.py");
const gateValidator = resolve(repoRoot, "tests/perf/validate-balanced-image-gate.py");

function usage() {
  return "usage: node examples/web/scripts/collect-web-depth-precision-quality.mjs " +
    "--exact-pkg <fresh-quality-exact-pkg> --candidate-pkg <fresh-quality-candidate20-pkg> " +
    "--chrome <executable> --output <fresh-dir>";
}

export function parseArguments(argv) {
  const allowed = new Set(["--exact-pkg", "--candidate-pkg", "--chrome", "--output"]);
  const values = new Map();
  if (argv.length !== allowed.size * 2) throw new Error(usage());
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!allowed.has(key) || !value || values.has(key)) throw new Error(usage());
    values.set(key, value);
  }
  return {
    exactPkg: resolve(values.get("--exact-pkg")),
    candidatePkg: resolve(values.get("--candidate-pkg")),
    chrome: resolve(values.get("--chrome")),
    output: resolve(values.get("--output")),
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

function sha256Bytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function sha256File(path) {
  return sha256Bytes(await readFile(path));
}

async function cleanBuildIdentity() {
  const [{ stdout: commit }, { stdout: status }] = await Promise.all([
    execFile("git", ["rev-parse", "HEAD"], { cwd: repoRoot }),
    execFile("git", ["status", "--porcelain"], { cwd: repoRoot }),
  ]);
  if (status.trim()) throw new Error("quality collection requires a clean committed worktree");
  const revision = commit.trim();
  if (!/^[0-9a-f]{40}$/.test(revision)) throw new Error("Git commit is not a full SHA");
  return revision;
}

async function validateBuildReceipt(path, expectedProfile, commit, jsSha256, wasmSha256) {
  const raw = JSON.parse(await readFile(path, "utf8"));
  const expected = {
    schema: "gsplat-web-diagnostic-build/v1",
    repository_commit: commit,
    dirty: false,
    profile: expectedProfile,
    js_sha256: jsSha256,
    wasm_sha256: wasmSha256,
  };
  for (const [key, value] of Object.entries(expected)) {
    if (raw[key] !== value) throw new Error(`${expectedProfile} build receipt ${key} mismatch`);
  }
  return sha256File(path);
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
import { captureQualityLane } from "/quality.mjs";
const result = { status: "failed", lanes: {} };
try {
  const [dataset, trace, plyResponse] = await Promise.all([
    fetch("/dataset.json").then((response) => response.json()),
    fetch("/trace.json").then((response) => response.json()),
    fetch("/kitsune.ply"),
  ]);
  if (!plyResponse.ok) throw new Error("Kitsune fetch failed: " + plyResponse.status);
  const plyBytes = new Uint8Array(await plyResponse.arrayBuffer());
  for (const lane of ["exact", "candidate"]) {
    const canvas = document.createElement("canvas");
    canvas.width = 1920;
    canvas.height = 1080;
    canvas.style.width = "1920px";
    canvas.style.height = "1080px";
    document.body.append(canvas);
    const module = await import("/pkg/" + lane + "/gsplat_web.js");
    const captured = await captureQualityLane({
      module,
      wasmUrl: "/pkg/" + lane + "/gsplat_web_bg.wasm",
      canvas,
      plyBytes,
      trace,
      dataset,
      lane,
    });
    for (let index = 0; index < captured.captures.length; index += 1) {
      const entry = captured.captures[index];
      const response = await fetch("/__capture", {
        method: "POST",
        headers: { "x-gsplat-lane": lane, "x-gsplat-capture-index": String(index),
          "x-gsplat-rgba-sha256": entry.identity.rgba8_sha256 },
        body: entry.rgba8,
      });
      if (!response.ok) throw new Error("capture upload failed: " + await response.text());
      delete entry.rgba8;
    }
    result.lanes[lane] = captured;
    canvas.remove();
    await new Promise((resolveFrame) => requestAnimationFrame(resolveFrame));
  }
  result.status = "complete";
} catch (error) {
  result.message = error?.message ?? String(error);
  result.stack = error?.stack ?? null;
}
window.__GSPLAT_WEB_DEPTH_QUALITY__ = result;
</script></body></html>`;
}

function contentType(path) {
  return ({ ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm",
    ".json": "application/json", ".ply": "application/octet-stream" })[extname(path)] ??
    "application/octet-stream";
}

function startServer(paths, uploaded) {
  const routes = new Map([
    ["/quality.mjs", sourceModule],
    ["/dataset.json", datasetManifestPath],
    ["/trace.json", tracePath],
    ["/kitsune.ply", plyPath],
    ["/pkg/exact/gsplat_web.js", paths.exactJs],
    ["/pkg/exact/gsplat_web_bg.wasm", paths.exactWasm],
    ["/pkg/candidate/gsplat_web.js", paths.candidateJs],
    ["/pkg/candidate/gsplat_web_bg.wasm", paths.candidateWasm],
  ]);
  const expectedBytes = WEB_DEPTH_QUALITY.width * WEB_DEPTH_QUALITY.height * 4;
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, "http://127.0.0.1");
      if (request.method === "POST" && url.pathname === "/__capture") {
        const lane = request.headers["x-gsplat-lane"];
        const index = Number(request.headers["x-gsplat-capture-index"]);
        const digest = request.headers["x-gsplat-rgba-sha256"];
        if (!["exact", "candidate"].includes(lane) || ![0, 1, 2].includes(index) ||
            !/^[0-9a-f]{64}$/.test(digest ?? "")) throw new Error("invalid capture upload identity");
        const key = `${lane}:${index}`;
        if (uploaded.has(key)) throw new Error(`duplicate capture upload ${key}`);
        const chunks = [];
        let length = 0;
        for await (const chunk of request) {
          length += chunk.length;
          if (length > expectedBytes) throw new Error(`capture ${key} exceeds RGBA8 size`);
          chunks.push(chunk);
        }
        const bytes = Buffer.concat(chunks);
        if (bytes.length !== expectedBytes || sha256Bytes(bytes) !== digest) {
          throw new Error(`capture ${key} body/hash mismatch`);
        }
        uploaded.set(key, bytes);
        response.writeHead(204).end();
        return;
      }
      if (request.method !== "GET") {
        response.writeHead(405).end("method not allowed");
        return;
      }
      if (url.pathname === "/") {
        response.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
        response.end(browserDocument());
        return;
      }
      const path = routes.get(url.pathname);
      if (!path) {
        response.writeHead(404).end("not found");
        return;
      }
      response.writeHead(200, { "content-type": contentType(path), "cache-control": "no-store" });
      createReadStream(path).pipe(response);
    } catch (error) {
      response.writeHead(400).end(error.message);
    }
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

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(kind, payload) {
  const type = Buffer.from(kind, "ascii");
  const result = Buffer.alloc(payload.length + 12);
  result.writeUInt32BE(payload.length, 0);
  type.copy(result, 4);
  payload.copy(result, 8);
  result.writeUInt32BE(crc32(Buffer.concat([type, payload])), payload.length + 8);
  return result;
}

export function rgba8Png(width, height, rgba) {
  if (rgba.length !== width * height * 4) throw new Error("PNG RGBA8 length mismatch");
  const rowBytes = width * 4;
  const filtered = Buffer.alloc(height * (rowBytes + 1));
  for (let row = 0; row < height; row += 1) {
    const target = row * (rowBytes + 1);
    filtered[target] = 0;
    rgba.copy(filtered, target + 1, row * rowBytes, (row + 1) * rowBytes);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from("89504e470d0a1a0a", "hex"),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", deflateSync(filtered, { level: 9 })),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

async function authoritativePoseHashes() {
  const program = [
    "import hashlib,json,pathlib,sys",
    "trace=json.loads(pathlib.Path(sys.argv[1]).read_text())",
    "values=[]",
    "for frame in trace['frames']:",
    " value={'pose':frame['pose'],'intrinsics':frame['intrinsics']}",
    " encoded=json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()",
    " values.append(hashlib.sha256(encoded).hexdigest())",
    "print(json.dumps(values))",
  ].join("\n");
  const { stdout } = await execFile("python3", ["-c", program, tracePath], { cwd: repoRoot });
  return JSON.parse(stdout);
}

function distribution(value) {
  return { count: 1, mean: value, p50: value, p90: value, p95: value, p99: value, max: value };
}

async function artifactDirectorySha256(directory) {
  const files = [];
  async function walk(path) {
    for (const entry of await readdir(path, { withFileTypes: true })) {
      const child = resolve(path, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`artifact contains symlink: ${child}`);
      if (entry.isDirectory()) await walk(child);
      else if (entry.isFile()) files.push(child);
      else throw new Error(`artifact contains non-file: ${child}`);
    }
  }
  await walk(directory);
  files.sort((left, right) => relative(directory, left).split(sep).join("/").localeCompare(
    relative(directory, right).split(sep).join("/"),
  ));
  const hash = createHash("sha256");
  for (const path of files) {
    const name = Buffer.from(relative(directory, path).split(sep).join("/"));
    const data = await readFile(path);
    const length = Buffer.alloc(8);
    length.writeBigUInt64BE(BigInt(name.length));
    hash.update(length).update(name);
    length.writeBigUInt64BE(BigInt(data.length));
    hash.update(length).update(data);
  }
  return hash.digest("hex");
}

function precisionImageReceipt(capture, imagePath, imageSha) {
  return {
    path: imagePath,
    sha256: imageSha,
    width: WEB_DEPTH_QUALITY.width,
    height: WEB_DEPTH_QUALITY.height,
    depth_precision: capture.depth_precision,
    projected_cache_precision: capture.projected_cache_precision,
    resident_sh: capture.resident_sh,
  };
}

function presentationReceipt(capture) {
  const identity = capture.identity;
  return {
    ticket: capture.counts.ticket,
    outcome: "presented",
    scene_generation: identity.scene_generation,
    camera_generation: identity.camera_revision,
    viewport_generation: identity.viewport_generation,
    contract_generation: identity.contract_generation,
    plan_generation: identity.plan_set_generation,
    presentation_generation: identity.presentation_sequence,
  };
}

export function qualityRendererImplementation(lane) {
  if (lane === "exact") return "gsplat-rs-webgpu-exact-quality";
  if (lane === "candidate") return "gsplat-rs-webgpu-candidate20-quality";
  throw new Error(`unknown quality lane: ${lane}`);
}

async function writeBenchmarkArtifact({
  output,
  lane,
  captureIndex,
  traceFrameIndex,
  pairId,
  commit,
  dataset,
  trace,
  image,
  capture,
  browserReceipt,
  poseIntrinsicsSha256,
}) {
  const runId = `web-candidate20-${lane}-${captureIndex}-${randomUUID()}`;
  const directory = resolve(output, "benchmark", runId);
  await mkdir(directory, { recursive: true });
  const imageBytes = await readFile(resolve(output, image.path));
  await writeFile(resolve(directory, "final-frame.png"), imageBytes);
  const timing = timingFromFrame(capture.frame);
  const started = new Date().toISOString();
  const exactness = lane === "exact"
    ? { full_quality: true, full_membership_full_resolution: true, quality_candidate: false }
    : { full_quality: false, full_membership_full_resolution: true, quality_candidate: true };
  const manifest = {
    schema: "gsplat-benchmark/v1",
    record_type: "manifest",
    run_id: runId,
    identity: {
      series_id: `web-depth-candidate20-${captureIndex}`,
      started_at_utc: started,
      ended_at_utc: started,
      measurement_started_at_utc: started,
      measurement_ended_at_utc: started,
    },
    build: { repository_commit: commit, dirty: false, profile: "release", package_version: "0.1.3" },
    dataset: {
      id: dataset.id,
      sha256: dataset.sha256,
      bytes: dataset.bytes,
      splat_count: dataset.splat_count,
      sh_degree: dataset.sh_degree,
    },
    trace: { id: trace.trace_id, sha256: trace.content_sha256 },
    renderer: {
      implementation: qualityRendererImplementation(lane),
      path: "packed_atlas",
      backend: "webgpu",
      sort_policy: "gpu_preproject_every_capture",
      count_semantics: "candidate_visible_contributor_issued_v1",
      blend_mode: "sorted_alpha",
      depth_precision_profile: capture.depth_precision.profile,
    },
    display: {
      width: WEB_DEPTH_QUALITY.width,
      height: WEB_DEPTH_QUALITY.height,
      dpr: 1,
      refresh_hz: 60,
      frame_budget_ms: 16.6666666667,
      refresh_hz_source: "diagnostic contract only",
      frame_budget_source: "diagnostic contract only",
    },
    environment: {
      platform: browserReceipt.platform,
      os: browserReceipt.userAgent,
      device: null,
      browser: browserReceipt.userAgent,
      adapter: null,
      driver: null,
    },
    exactness,
    image: { path: "final-frame.png", sha256: image.sha256, width: image.width, height: image.height },
    unavailable_fields: ["frames[*].preprocess_ms", "frames[*].sort_ms",
      "frames[*].geometry_submit_ms", "frames[*].gpu_wait_ms", "frames[*].gpu_complete_ms"],
  };
  const camera = {
    trace_id: trace.trace_id,
    trace_content_sha256: trace.content_sha256,
    pose_intrinsics_sha256: poseIntrinsicsSha256,
  };
  const presentation = presentationReceipt(capture);
  const frame = {
    schema: "gsplat-benchmark/v1",
    record_type: "frame",
    run_id: runId,
    frame_index: 0,
    elapsed_ns: 0,
    call_ms: timing.call_ms,
    frame_wall_ms: timing.frame_wall_ms,
    preprocess_ms: null,
    sort_ms: null,
    geometry_submit_ms: null,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: capture.counts.visibleCount,
    contributor: capture.counts.contributorCount,
    drawn: capture.counts.drawnCount,
    active_splats: dataset.splat_count,
    exact_contributor_compaction: true,
    sort_refreshed: true,
    pair_id: pairId,
    capture_index: captureIndex,
    trace_frame_index: traceFrameIndex,
    camera,
    terminal_outcome: "presented",
    presentation,
    capture_depth_precision: image.depth_precision,
    capture_projected_cache_precision: image.projected_cache_precision,
    capture_resident_sh: image.resident_sh,
  };
  const summary = {
    schema: "gsplat-benchmark/v1",
    record_type: "summary",
    run_id: runId,
    sample_count: 1,
    warmup_count: 0,
    frame_budget_ms: manifest.display.frame_budget_ms,
    missed_frame_count: timing.frame_wall_ms > manifest.display.frame_budget_ms ? 1 : 0,
    distributions: {
      call_ms: distribution(timing.call_ms),
      frame_wall_ms: distribution(timing.frame_wall_ms),
      preprocess_ms: null,
      sort_ms: null,
      geometry_submit_ms: null,
      gpu_wait_ms: null,
      gpu_complete_ms: null,
    },
  };
  await Promise.all([
    writeFile(resolve(directory, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`),
    writeFile(resolve(directory, "frames.jsonl"), `${JSON.stringify(frame)}\n`),
    writeFile(resolve(directory, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`),
  ]);
  await execFile("python3", [benchmarkValidator, directory], { cwd: repoRoot });
  return {
    path: relative(output, directory).split(sep).join("/"),
    sha256: await artifactDirectorySha256(directory),
    run_id: runId,
    frame_index: 0,
    depth_precision: image.depth_precision,
    projected_cache_precision: image.projected_cache_precision,
    resident_sh: image.resident_sh,
  };
}

export async function writeSuite({
  output,
  outcome,
  uploaded,
  commit,
  dataset,
  trace,
  inputs,
  browserReceipt,
}) {
  const frames = [];
  const poseHashes = await authoritativePoseHashes();
  for (let captureIndex = 0; captureIndex < 3; captureIndex += 1) {
    const traceFrameIndex = WEB_DEPTH_QUALITY.traceFrameIndices[captureIndex];
    const exact = outcome.lanes.exact.captures[captureIndex];
    const candidate = outcome.lanes.candidate.captures[captureIndex];
    validateMatchedPair(exact, candidate);
    const laneImages = {};
    for (const [lane, capture] of [["exact", exact], ["candidate", candidate]]) {
      const rgba = uploaded.get(`${lane}:${captureIndex}`);
      if (!rgba) throw new Error(`missing renderer-owned ${lane}:${captureIndex} upload`);
      const png = rgba8Png(WEB_DEPTH_QUALITY.width, WEB_DEPTH_QUALITY.height, rgba);
      const imagePath = `images/${lane}-${captureIndex}.png`;
      await mkdir(resolve(output, "images"), { recursive: true });
      await writeFile(resolve(output, imagePath), png);
      laneImages[lane] = precisionImageReceipt(capture, imagePath, sha256Bytes(png));
    }
    const pairId = `web-candidate20-pair-${captureIndex}`;
    const camera = {
      trace_id: trace.trace_id,
      trace_content_sha256: trace.content_sha256,
      pose_intrinsics_sha256: poseHashes[traceFrameIndex],
    };
    const benchmark = {};
    for (const [lane, capture] of [["exact", exact], ["candidate", candidate]]) {
      benchmark[lane] = await writeBenchmarkArtifact({
        output, lane, captureIndex, traceFrameIndex, pairId, commit, dataset, trace,
        image: laneImages[lane], capture, browserReceipt,
        poseIntrinsicsSha256: poseHashes[traceFrameIndex],
      });
    }
    frames.push({
      capture_index: captureIndex,
      trace_frame_index: traceFrameIndex,
      presented: true,
      camera,
      presentation: { exact: presentationReceipt(exact), candidate: presentationReceipt(candidate) },
      exact: laneImages.exact,
      candidate: laneImages.candidate,
      metrics: computeFrameMetrics(
        uploaded.get(`exact:${captureIndex}`),
        uploaded.get(`candidate:${captureIndex}`),
        WEB_DEPTH_QUALITY.width,
        WEB_DEPTH_QUALITY.height,
      ),
      benchmark_artifacts: { pair_id: pairId, ...benchmark },
    });
  }
  const transitions = [0, 1].map((index) => ({
    from_capture_index: index,
    to_capture_index: index + 1,
    from_trace_frame_index: WEB_DEPTH_QUALITY.traceFrameIndices[index],
    to_trace_frame_index: WEB_DEPTH_QUALITY.traceFrameIndices[index + 1],
    metrics: {
      temporal_rgb_residual_mae_normalized: computeTemporalMetric(
        uploaded.get(`exact:${index}`), uploaded.get(`exact:${index + 1}`),
        uploaded.get(`candidate:${index}`), uploaded.get(`candidate:${index + 1}`),
      ),
    },
  }));
  const suite = {
    schema: "gsplat-balanced-image-gate/v1",
    evidence_class: "balanced_quality_candidate",
    experiment: { name: "b1-depth-key-candidate20", changed_receipt: "depth_precision" },
    authority: {
      dataset_manifest: {
        path: "tests/perf/datasets/kitsune.json",
        sha256: inputs.dataset_manifest_sha256,
        dataset_id: dataset.id,
        asset_sha256: dataset.sha256,
      },
      trace: {
        path: "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json",
        sha256: inputs.trace_file_sha256,
        trace_id: trace.trace_id,
        content_sha256: trace.content_sha256,
      },
    },
    exactness: {
      source_splat_count: dataset.splat_count,
      decoded_splat_count: dataset.splat_count,
      encoded_splat_count: dataset.splat_count,
      resident_splat_count: dataset.splat_count,
      addressable_splat_count: dataset.splat_count,
      source_sh_degree: dataset.sh_degree,
      resident_sh_degree: dataset.sh_degree,
      source_membership: "all",
      sampling: "disabled",
      lod: "disabled",
      sh_degree_policy: "source",
      render_mode: "sorted_alpha",
      partial_scene_published: false,
      full_quality: false,
      full_membership_full_resolution: true,
      quality_candidate: true,
    },
    resolution: {
      requested_width: WEB_DEPTH_QUALITY.width,
      requested_height: WEB_DEPTH_QUALITY.height,
      surface_width: WEB_DEPTH_QUALITY.width,
      surface_height: WEB_DEPTH_QUALITY.height,
      internal_render_width: WEB_DEPTH_QUALITY.width,
      internal_render_height: WEB_DEPTH_QUALITY.height,
      presented_width: WEB_DEPTH_QUALITY.width,
      presented_height: WEB_DEPTH_QUALITY.height,
      dynamic_resolution: "disabled",
      upscaling: "disabled",
      full_resolution: true,
    },
    camera: { mode: "moving_sequence", trace_frame_indices: [...WEB_DEPTH_QUALITY.traceFrameIndices] },
    inputs: { commit, ...inputs, browser: browserReceipt },
    frames,
    transitions,
  };
  const path = resolve(output, "suite.json");
  await writeFile(path, `${JSON.stringify(suite, null, 2)}\n`);
  await execFile("python3", [gateValidator, path], { cwd: repoRoot });
  return path;
}

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const paths = {
    exactJs: resolve(args.exactPkg, "gsplat_web.js"),
    exactWasm: resolve(args.exactPkg, "gsplat_web_bg.wasm"),
    candidateJs: resolve(args.candidatePkg, "gsplat_web.js"),
    candidateWasm: resolve(args.candidatePkg, "gsplat_web_bg.wasm"),
    exactReceipt: resolve(args.exactPkg, "gsplat_web_build_receipt.json"),
    candidateReceipt: resolve(args.candidatePkg, "gsplat_web_build_receipt.json"),
  };
  await Promise.all([
    requireFile(args.chrome, constants.X_OK, "Chrome executable"),
    ...Object.entries(paths).map(([label, path]) => requireFile(path, constants.R_OK, label)),
    requireFile(plyPath, constants.R_OK, "Kitsune PLY"),
    requireFile(datasetManifestPath, constants.R_OK, "Kitsune manifest"),
    requireFile(tracePath, constants.R_OK, "Kitsune quality trace"),
  ]);
  if (resolve(args.exactPkg) === resolve(args.candidatePkg)) {
    throw new Error("Exact and Candidate20 package directories must be independent");
  }
  const commit = await cleanBuildIdentity();
  await requireFreshDirectory(args.output);
  const dataset = JSON.parse(await readFile(datasetManifestPath, "utf8"));
  const trace = JSON.parse(await readFile(tracePath, "utf8"));
  const inputs = {
    dataset_manifest_sha256: await sha256File(datasetManifestPath),
    dataset_asset_sha256: await sha256File(plyPath),
    trace_file_sha256: await sha256File(tracePath),
    exact_js_sha256: await sha256File(paths.exactJs),
    exact_wasm_sha256: await sha256File(paths.exactWasm),
    candidate_js_sha256: await sha256File(paths.candidateJs),
    candidate_wasm_sha256: await sha256File(paths.candidateWasm),
  };
  inputs.exact_build_receipt_sha256 = await validateBuildReceipt(
    paths.exactReceipt,
    "quality-exact",
    commit,
    inputs.exact_js_sha256,
    inputs.exact_wasm_sha256,
  );
  inputs.candidate_build_receipt_sha256 = await validateBuildReceipt(
    paths.candidateReceipt,
    "quality-candidate20",
    commit,
    inputs.candidate_js_sha256,
    inputs.candidate_wasm_sha256,
  );
  if (inputs.dataset_asset_sha256 !== dataset.sha256) throw new Error("Kitsune asset hash mismatch");

  const uploaded = new Map();
  const browserLog = [];
  let server = null;
  let browser = null;
  try {
    const puppeteer = await loadPuppeteer();
    const running = await startServer(paths, uploaded);
    server = running.server;
    browser = await puppeteer.launch({
      executablePath: args.chrome,
      headless: true,
      defaultViewport: { width: WEB_DEPTH_QUALITY.width, height: WEB_DEPTH_QUALITY.height, deviceScaleFactor: 1 },
      args: ["--enable-unsafe-webgpu", "--enable-gpu", "--ignore-gpu-blocklist"],
    });
    const page = await browser.newPage();
    page.on("console", (message) => browserLog.push(`${message.type()}: ${message.text()}`));
    page.on("pageerror", (error) => browserLog.push(`pageerror: ${error.stack ?? error.message}`));
    await page.goto(`http://127.0.0.1:${running.port}/`, { waitUntil: "networkidle0", timeout: 600_000 });
    await page.waitForFunction(() => window.__GSPLAT_WEB_DEPTH_QUALITY__ !== undefined, { timeout: 1_800_000 });
    const outcome = await page.evaluate(() => window.__GSPLAT_WEB_DEPTH_QUALITY__);
    if (outcome.status !== "complete") throw new Error(outcome.message ?? "browser quality capture failed");
    if (uploaded.size !== 6) throw new Error(`expected six renderer-owned captures, got ${uploaded.size}`);
    const browserReceipt = await page.evaluate(() => ({
      userAgent: navigator.userAgent,
      platform: navigator.platform,
      webgpu: Boolean(navigator.gpu),
    }));
    const suitePath = await writeSuite({
      output: args.output, outcome, uploaded, commit, dataset, trace, inputs, browserReceipt,
    });
    await Promise.all([
      writeFile(resolve(args.output, "browser-console.log"), `${browserLog.join("\n")}\n`),
      writeFile(resolve(args.output, "inputs.json"), `${JSON.stringify({ commit, ...inputs, browserReceipt }, null, 2)}\n`),
    ]);
    console.log(JSON.stringify({ status: "valid_balanced_quality_candidate", suite: suitePath }));
  } catch (error) {
    await writeFile(resolve(args.output, "failure.json"), `${JSON.stringify({
      status: "failed",
      message: error.message,
      stack: error.stack ?? null,
      browser_log: browserLog,
    }, null, 2)}\n`);
    throw error;
  } finally {
    await browser?.close().catch(() => {});
    await closeServer(server);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.stack ?? error.message);
    process.exitCode = 1;
  });
}
