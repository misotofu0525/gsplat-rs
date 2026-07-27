import {
  createGsplatRenderer,
  createGsplatRendererFromStream,
  createGsplatRendererFromUrl,
  GsplatWebError,
  initGsplatWeb,
} from "../../../packages/web/src/index.js?v=sorted-index-20260710";
import {
  BENCHMARK_SCHEMA,
  appendBenchmarkSample,
  benchmarkResolutionEvidence,
  benchmarkSummary,
  createBenchmarkCollector,
  frameRecords,
  legacyAverages,
} from "./benchmark-artifact.mjs";
import { createRasterDiagnosticPly } from "../../../tests/perf/raster-diagnostic.mjs";
import {
  cameraBasisFromTraceFrame,
  cameraTraceFrame,
  createCameraTraceSequence,
  validateCameraTraceV1,
} from "../../../tests/perf/trace/camera-trace-v1.mjs";
import { monotonicOrderingWindow } from "./benchmark-order-evidence.mjs";
import { createCurrentStatsSchedule } from "./benchmark-current-stats-schedule.mjs";
import {
  BENCHMARK_WINDOW_MODES,
  createTerminalQueueThroughputWindow,
  currentStatsEvidenceWindowIdentity,
  currentStatsEvidenceWindowManifest,
} from "./benchmark-window-mode.mjs";
import {
  rendererFailurePolicy,
  sampledWebglOptIn,
} from "./renderer-policy.mjs";
import { LatestAsyncRequestCoordinator } from "./latest-async-request.mjs";
import {
  canonicalDatasetIdentityFromObservation,
  WEB_DATASET_PATHS,
} from "./dataset-identity.mjs";
import {
  normalizeQ1SurfaceCapture,
  q1CaptureMeasuredFrame,
} from "./q1-gsplat-producer.mjs";

const API_VERSION = "0.1";
const ORBIT_RADIANS_PER_SCREEN = 3.2;
const DOUBLE_TAP_TIMEOUT_MS = 300;
const DOUBLE_TAP_SLOP_PX = 48;
const DEFAULT_BENCHMARK_FRAMES = 120;
const DEFAULT_BENCHMARK_WARMUP_FRAMES = 10;
const DEFAULT_BENCHMARK_YAW_STEP = 0.001;
const OPACITY_LOGIT_LIMIT = 16;
const DEFAULT_FRAME_BUDGET_MS = 1000 / 60;
const reportedStructuredFailures = new WeakSet();

const DATASETS = WEB_DATASET_PATHS;

const requestedWasmPackage = new URLSearchParams(window.location.search)
  .get("gsplat_wasm_package_url");
const wasmPackageBase = requestedWasmPackage === null
  ? new URL("../pkg/", import.meta.url)
  : new URL(`${requestedWasmPackage.replace(/\/+$/, "")}/`, window.location.href);
if (wasmPackageBase.origin !== window.location.origin) {
  throw new TypeError("gsplat_wasm_package_url must remain on the collector origin");
}
const WASM_ENTRY = new URL("gsplat_web.js?v=sorted-index-20260710", wasmPackageBase);
const WASM_BINARY = new URL("gsplat_web_bg.wasm?v=sorted-index-20260710", wasmPackageBase);

const REQUIRED_FIELDS = [
  "x",
  "y",
  "z",
  "opacity",
  "scale_0",
  "scale_1",
  "scale_2",
  "rot_0",
  "rot_1",
  "rot_2",
  "rot_3",
  "f_dc_0",
  "f_dc_1",
  "f_dc_2",
];

const TYPE_SIZES = new Map([
  ["char", 1],
  ["int8", 1],
  ["uchar", 1],
  ["uint8", 1],
  ["short", 2],
  ["int16", 2],
  ["ushort", 2],
  ["uint16", 2],
  ["int", 4],
  ["int32", 4],
  ["uint", 4],
  ["uint32", 4],
  ["float", 4],
  ["float32", 4],
  ["double", 8],
  ["float64", 8],
]);

const state = {
  gl: null,
  program: null,
  buffer: null,
  backend: "booting",
  wasmModule: null,
  wasmRenderer: null,
  wasmUnavailableReason: "",
  wasmFatalError: "",
  scene: null,
  scratchDepths: new Float32Array(0),
  drawBuffer: new Float32Array(0),
  lastSortedOrder: [],
  sortFrameCounter: 0,
  autoOrbit: true,
  camera: {
    target: [0, 0, 0],
    distance: 2.5,
    yaw: 0,
    pitch: 0,
    fovY: Math.PI / 3,
    near: 0.01,
    far: 1000,
  },
  pointers: new Map(),
  gesture: null,
  lastTapAt: 0,
  lastTapX: 0,
  lastTapY: 0,
  lastFrameTime: performance.now(),
  frameCounter: 0,
  fps: 0,
  cameraStatus: "camera=auto",
  rendererStatus: "state=booting",
  gpuOrderPreparationPending: false,
  surfaceSizeLabel: "pending",
  datasetPath: "pending",
  startDataset: "showcase",
  benchmark: null,
  autoStartBenchmark: false,
  strictBenchmarkMode: false,
  autoBenchmarkSync: false,
  qualificationTrace: null,
  qualificationTraceUrl: null,
  qualificationTraceFrameIndex: 0,
  qualificationTraceSequenceEnabled: false,
  qualificationTraceFrameIndices: null,
  qualificationTraceWarmupFrames: null,
  qualificationTraceMeasuredFrames: null,
  qualificationTraceLoops: 1,
  qualificationTraceSequence: null,
  qualificationCamera: null,
  cameraReceipt: null,
  geometryPath: "packed",
  requestedOrderBackend: "adaptive",
  requestedProjectedPolicy: "adaptive",
  requestedGpuOrderProducer: null,
  orderCompletionProtocol: "isolated_terminal",
  benchmarkWindowMode: BENCHMARK_WINDOW_MODES.currentStatsEvidence,
  currentStatsControlArtifactIdentity: null,
  q1CaptureTraceFrameIndex: null,
  q1QueueTerminalEnabled: false,
  sampledWebglEnabled: false,
  currentStatsSmokeEnabled: false,
  currentStatsSmokeRequested: false,
  currentStatsSmokeCompleted: false,
  currentStatsSmokeFrame: null,
  resizeMeasurePending: false,
  resizePending: false,
  lastLoadError: "",
};

const els = {
  canvas: document.getElementById("viewport"),
  loadShowcase: document.getElementById("loadShowcase"),
  loadMinimal: document.getElementById("loadMinimal"),
  loadFlowers: document.getElementById("loadFlowers"),
  fileInput: document.getElementById("fileInput"),
  resetCamera: document.getElementById("resetCamera"),
  toggleOrbit: document.getElementById("toggleOrbit"),
  drawBudget: document.getElementById("drawBudget"),
  drawBudgetValue: document.getElementById("drawBudgetValue"),
  sortInterval: document.getElementById("sortInterval"),
  sortIntervalValue: document.getElementById("sortIntervalValue"),
  pointScale: document.getElementById("pointScale"),
  pointScaleValue: document.getElementById("pointScaleValue"),
  benchmarkFrames: document.getElementById("benchmarkFrames"),
  benchmarkWarmup: document.getElementById("benchmarkWarmup"),
  benchmarkYaw: document.getElementById("benchmarkYaw"),
  benchmarkYawValue: document.getElementById("benchmarkYawValue"),
  runBenchmark: document.getElementById("runBenchmark"),
  benchmarkStatus: document.getElementById("benchmarkStatus"),
  benchmarkResult: document.getElementById("benchmarkResult"),
  formatBadge: document.getElementById("formatBadge"),
  backendBadge: document.getElementById("backendBadge"),
  gpuStatus: document.getElementById("gpuStatus"),
  renderMode: document.getElementById("renderMode"),
  gaussianCount: document.getElementById("gaussianCount"),
  shDegree: document.getElementById("shDegree"),
  visibleCount: document.getElementById("visibleCount"),
  drawnCount: document.getElementById("drawnCount"),
  surfaceSize: document.getElementById("surfaceSize"),
  frameCount: document.getElementById("frameCount"),
  fpsValue: document.getElementById("fpsValue"),
  preprocessMs: document.getElementById("preprocessMs"),
  sortMs: document.getElementById("sortMs"),
  geometrySubmitMs: document.getElementById("geometrySubmitMs"),
  callMs: document.getElementById("callMs"),
  frameMs: document.getElementById("frameMs"),
  sceneName: document.getElementById("sceneName"),
  sceneMeta: document.getElementById("sceneMeta"),
  statusLine: document.getElementById("statusLine"),
  themeToggle: document.getElementById("themeToggle"),
  loadingOverlay: document.getElementById("loadingOverlay"),
  loadingTitle: document.getElementById("loadingTitle"),
  loadingMeta: document.getElementById("loadingMeta"),
  loadingBar: document.getElementById("loadingBar"),
  sceneButtons: Array.from(document.querySelectorAll(".scene-switcher button")),
};

let resizeMeasureFrame = null;
let canvasResizeObserver = null;
const wasmResizeCoordinator = new LatestAsyncRequestCoordinator({
  async perform(request) {
    await request.renderer.resize(request.width, request.height);
    return request.renderer.surfaceSize();
  },
  publish(request, surface) {
    if (state.wasmRenderer !== request.renderer) return;
    if (surface.width !== request.width || surface.height !== request.height) {
      throw new GsplatWebError(
        new Error(
          `wrapper resize published ${surface.width}x${surface.height}; ` +
          `requested ${request.width}x${request.height}`,
        ),
        "resize",
        { scenePublished: true },
      );
    }
    if (els.canvas.width !== request.width || els.canvas.height !== request.height) {
      throw new GsplatWebError(
        new Error(
          `canvas backing remained ${els.canvas.width}x${els.canvas.height}; ` +
          `transaction published ${request.width}x${request.height}`,
        ),
        "resize",
        { scenePublished: true },
      );
    }
    request.renderer.setSortInterval(Number(els.sortInterval.value));
    state.surfaceSizeLabel = `${surface.width}x${surface.height}`;
    els.surfaceSize.textContent = state.surfaceSizeLabel;
  },
  onError(error) {
    failClosedWasmResize(error);
  },
  onPendingChange(pending) {
    state.resizePending = pending;
  },
});

void main().catch(reportStartupFailure);

function reportStartupFailure(error) {
  const reason = compactMessage(error);
  emitStructuredSceneFailure(state.startDataset, error);
  state.lastLoadError = reason;
  setStatus(`state=startup_failed dataset=${state.startDataset} error=${reason}`);
  setLoadingProgress("Scene could not load", reason, 0);
  els.benchmarkStatus.textContent = "failed";
  els.benchmarkResult.textContent = `STARTUP_FAILURE dataset=${state.startDataset} error=${reason}`;
  console.error(`STARTUP_FAILURE dataset=${state.startDataset} error=${reason}`);
}

async function main() {
  initTheme();
  applyUrlConfig();
  await loadQualificationTrace();
  updateControlLabels();
  await initWasmModule();
  if (state.backend !== "wasm") {
    if (webglFailurePolicy() === "sampled_webgl_diagnostic") {
      initRenderer();
    } else {
      throw new Error(
        `exact WebGPU renderer unavailable: ${state.wasmUnavailableReason || "unknown reason"}; ` +
        "sampled WebGL is diagnostic-only and requires gsplat_allow_sampled_webgl=true",
      );
    }
  }
  bindEvents();
  resizeCanvas();
  await loadStartupDataset();
  requestAnimationFrame(frame);
}

async function loadQualificationTrace() {
  if (!state.qualificationTraceUrl) return;
  const response = await fetch(state.qualificationTraceUrl, { cache: "no-store" });
  if (!response.ok) throw new Error(`qualification trace HTTP ${response.status}`);
  state.qualificationTrace = validateCameraTraceV1(await response.json());
  if (state.qualificationTraceSequenceEnabled) {
    state.qualificationTraceSequence = createCameraTraceSequence(state.qualificationTrace, {
      frameIndices: state.qualificationTraceFrameIndices ?? undefined,
      warmupFrames: state.qualificationTraceWarmupFrames ?? 0,
      measuredFrames: state.qualificationTraceMeasuredFrames ?? undefined,
      loops: state.qualificationTraceLoops,
    });
    const lastIndex = state.qualificationTraceSequence.frameIndices.at(-1);
    state.qualificationCamera = cameraTraceFrame(state.qualificationTrace, lastIndex);
  } else {
    state.qualificationCamera = cameraTraceFrame(
      state.qualificationTrace,
      state.qualificationTraceFrameIndex,
    );
  }
  const selected = state.qualificationTraceSequenceEnabled
    ? state.qualificationTraceSequence.frameIndices.join(",")
    : String(state.qualificationTraceFrameIndex);
  const selectedFrame = state.qualificationTraceSequenceEnabled
    ? state.qualificationTrace.frames[state.qualificationTraceSequence.frameIndices[0]]
    : state.qualificationTrace.frames[state.qualificationTraceFrameIndex];
  console.info(
    `CAMERA_TRACE trace_id=${state.qualificationTrace.trace_id} ` +
    `trace_sha256=${state.qualificationTrace.content_sha256} ` +
    `mode=${state.qualificationTraceSequenceEnabled ? "trace_sequence" : "fixed_frame"} ` +
    `frame_indices=${selected} frame_index=${selectedFrame.frame_index} ` +
    `timestamp_ns=${selectedFrame.timestamp_ns} requested_backend=${state.requestedOrderBackend}`,
  );
}

async function loadStartupDataset() {
  const strictRequestedDataset = state.autoStartBenchmark;
  const ladderCount = /^truck-(\d+)$/.exec(state.startDataset)?.[1] ?? null;
  const largeDataset = ["bonsai", "truck", "garden", "bicycle"].includes(state.startDataset)
    ? state.startDataset
    : null;
  const requested = ladderCount
    ? [{ path: DATASETS[state.startDataset], name: `point_cloud-n${ladderCount}.ply` }]
    : largeDataset
      ? [
          { path: DATASETS[largeDataset], name: `${largeDataset}.ply` },
          { path: DATASETS.minimal, name: "minimal_ascii.ply" },
        ]
    :
    state.startDataset === "diagnostic"
      ? [{ path: DATASETS.diagnostic, name: "raster_diagnostic_v1.ply" }]
      : state.startDataset === "minimal"
      ? [{ path: DATASETS.minimal, name: "minimal_ascii.ply" }]
      : state.startDataset === "flowers"
        ? [
            { path: DATASETS.flowers, name: "flowers_1.ply" },
            { path: DATASETS.minimal, name: "minimal_ascii.ply" },
          ]
        : [
            { path: DATASETS.showcase, name: "kitune1.ply" },
            { path: DATASETS.flowers, name: "flowers_1.ply" },
            { path: DATASETS.minimal, name: "minimal_ascii.ply" },
          ];

  for (let index = 0; index < requested.length; index += 1) {
    const dataset = requested[index];
    const allowFallback = !strictRequestedDataset && index < requested.length - 1;
    const loaded = dataset.path === DATASETS.diagnostic
      ? await loadGeneratedDiagnostic(dataset.name)
      : await loadDataset(dataset.path, dataset.name, {
        allowFallback,
      });
    if (loaded) {
      return;
    }
    if (!allowFallback) {
      break;
    }
  }
  throw new Error(
    `exact requested dataset ${state.startDataset} did not load: ${state.lastLoadError || "unknown load failure"}`,
  );
}

async function loadGeneratedDiagnostic(name) {
  const bytes = createRasterDiagnosticPly();
  const scene = parsePly(bytes, name, DATASETS.diagnostic);
  scene.sourceBytes = bytes.byteLength;
  scene.sourceSha256 = await sha256Hex(bytes);
  await applyScene(scene);
  return true;
}

async function initWasmModule() {
  try {
    const response = await fetch(WASM_ENTRY, { method: "HEAD" });
    if (!response.ok) {
      state.wasmUnavailableReason = `pkg_missing_http_${response.status}`;
      state.backend = webglFailurePolicy() === "sampled_webgl_diagnostic" ? "webgl" : "none";
      return;
    }

    const module = await import(WASM_ENTRY.href);
    state.wasmModule = await initGsplatWeb({ module, wasmUrl: WASM_BINARY });
    state.backend = "wasm";
    els.gpuStatus.textContent = "wasm ready";
    setRenderMode("Rust/WASM + wgpu Surface");
    updateBackendControls();
    setStatus("state=wasm_ready");
  } catch (error) {
    state.wasmUnavailableReason = compactMessage(error);
    state.backend = webglFailurePolicy() === "sampled_webgl_diagnostic" ? "webgl" : "none";
  }
}

function initRenderer() {
  if (state.gl && state.program) {
    return;
  }

  const gl = els.canvas.getContext("webgl2", {
    alpha: false,
    antialias: false,
    powerPreference: "high-performance",
  });
  state.gl = gl;
  if (!gl) {
    els.gpuStatus.textContent = "unavailable";
    setStatus("state=webgl_unavailable");
    return;
  }

  const vertex = `#version 300 es
    precision highp float;

    in vec3 aPosition;
    in vec3 aColor;
    in float aAlpha;
    in float aRadius;

    uniform vec3 uEye;
    uniform vec3 uRight;
    uniform vec3 uUp;
    uniform vec3 uForward;
    uniform float uF;
    uniform float uAspect;
    uniform float uFocalPixels;
    uniform float uPointScale;
    uniform float uNear;
    uniform float uFar;

    out vec4 vColor;

    void main() {
      vec3 rel = aPosition - uEye;
      float x = dot(rel, uRight);
      float y = dot(rel, uUp);
      float z = max(dot(rel, uForward), 0.0001);
      float ndcX = (x * uF / uAspect) / z;
      float ndcY = (y * uF) / z;
      float ndcZ = ((z - uNear) / max(uFar - uNear, 0.0001)) * 2.0 - 1.0;

      gl_Position = vec4(ndcX, ndcY, ndcZ, 1.0);
      gl_PointSize = clamp(aRadius * uFocalPixels * uPointScale / z, 1.0, 128.0);
      vColor = vec4(aColor * aAlpha, aAlpha);
    }
  `;
  const fragment = `#version 300 es
    precision highp float;

    in vec4 vColor;
    out vec4 outColor;

    void main() {
      vec2 p = gl_PointCoord * 2.0 - 1.0;
      float r2 = dot(p, p);
      if (r2 > 1.0) {
        discard;
      }
      float coverage = exp(-r2 * 3.5);
      outColor = vec4(vColor.rgb * coverage, vColor.a * coverage);
    }
  `;

  const program = createProgram(gl, vertex, fragment);
  const buffer = gl.createBuffer();
  if (!program || !buffer) {
    els.gpuStatus.textContent = "failed";
    setStatus("state=shader_setup_failed");
    return;
  }

  state.program = program;
  state.buffer = buffer;
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);

  const stride = 8 * 4;
  bindAttribute(gl, program, "aPosition", 3, stride, 0);
  bindAttribute(gl, program, "aColor", 3, stride, 3 * 4);
  bindAttribute(gl, program, "aAlpha", 1, stride, 6 * 4);
  bindAttribute(gl, program, "aRadius", 1, stride, 7 * 4);

  gl.disable(gl.DEPTH_TEST);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  gl.clearColor(0.047, 0.051, 0.059, 1.0);
  els.gpuStatus.textContent = "ready";
  setRenderMode("WebGL2 sampled diagnostic point splats");
  updateBackendControls();
  setStatus("state=waiting_for_scene");
}

function setRenderMode(label) {
  els.renderMode.textContent = label;
}

function usingWasm() {
  return state.backend === "wasm" && Boolean(state.wasmRenderer);
}

function ensureFallbackRenderer() {
  if (webglFailurePolicy() !== "sampled_webgl_diagnostic") {
    state.backend = "none";
    return false;
  }
  state.backend = "webgl";
  initRenderer();
  if (state.gl && state.program) {
    els.gpuStatus.textContent = "ready";
    setRenderMode("WebGL2 sampled diagnostic point splats");
    updateBackendControls();
  }
  return Boolean(state.gl && state.program);
}

function formalWebgpuEvidenceRequested() {
  return Boolean(state.strictBenchmarkMode || state.qualificationTraceUrl || state.qualificationTrace);
}

function webglFailurePolicy() {
  return rendererFailurePolicy({
    sampledWebglEnabled: state.sampledWebglEnabled,
    formalEvidenceRequested: formalWebgpuEvidenceRequested(),
  });
}

async function disposeWasmRenderer() {
  wasmResizeCoordinator.invalidate();
  const renderer = state.wasmRenderer;
  state.wasmRenderer = null;
  state.wasmFatalError = "";
  renderer?.free?.();
  await wasmResizeCoordinator.whenIdle();
}

async function createWasmRenderer(scene) {
  if (!state.wasmModule || !scene.rawBytes) {
    return false;
  }

  await disposeWasmRenderer();
  setStatus(`state=wasm_creating dataset=${scene.name}`);
  try {
    const renderer = await createGsplatRenderer({
      module: state.wasmModule,
      canvas: els.canvas,
      plyBytes: scene.rawBytes,
      width: els.canvas.width,
      height: els.canvas.height,
      sortInterval: Number(els.sortInterval.value),
      orderBackend: state.requestedOrderBackend,
      projectedPolicy: state.requestedProjectedPolicy,
      gpuOrderProducer: state.requestedGpuOrderProducer,
      geometryPath: state.geometryPath,
    });
    adoptWasmRenderer(renderer, scene);
    return true;
  } catch (error) {
    const reason = compactMessage(error);
    emitStructuredSceneFailure(scene.name, error);
    await disposeWasmRenderer();
    state.wasmUnavailableReason = reason;
    if (webglFailurePolicy() !== "sampled_webgl_diagnostic") {
      state.backend = "none";
      state.wasmFatalError = `wasm_create_failed:${reason}`;
      els.gpuStatus.textContent = "failed";
      els.benchmarkStatus.textContent = "failed";
      updateBackendControls();
      setStatus(`state=wasm_create_failed fail_closed=true error=${reason}`);
      throw error;
    }
    ensureFallbackRenderer();
    updateBackendControls();
    setStatus(`state=wasm_create_failed fallback=webgl error=${state.wasmUnavailableReason}`);
    return false;
  }
}

function adoptWasmRenderer(renderer, scene) {
  state.wasmRenderer = renderer;
  state.backend = "wasm";
  state.wasmUnavailableReason = "";
  state.wasmFatalError = "";
  if (state.qualificationCamera) {
    renderer.setCamera(state.qualificationCamera);
    state.cameraReceipt = renderer.cameraReceipt();
  }

  const summary = renderer.sceneSummary();
  const surface = renderer.surfaceSize();
  state.surfaceSizeLabel = `${surface.width}x${surface.height}`;
  els.gpuStatus.textContent = "wgpu";
  els.gaussianCount.textContent = formatNumber(summary.gaussians ?? scene.count);
  els.shDegree.textContent = String(summary.shDegree ?? scene.shDegree);
  setRenderMode(`Rust/WASM ${renderer.rasterPath()}`);
  updateBackendControls();
}

function requestWasmRendererResize(width, height) {
  const renderer = state.wasmRenderer;
  if (!renderer || state.wasmFatalError) return;
  wasmResizeCoordinator.request({ renderer, width, height });
}

function failClosedWasmResize(error) {
  const failure = error instanceof GsplatWebError
      && error.stage === "resize"
      && error.scene_published === true
    ? error
    : new GsplatWebError(error, "resize", { scenePublished: true });
  const reason = compactMessage(failure);
  state.wasmUnavailableReason = reason;
  state.wasmFatalError = `wasm_resize_failed:${reason}`;
  if (state.benchmark) {
    state.benchmark.enabled = false;
    state.benchmark.failure = state.wasmFatalError;
  }
  els.gpuStatus.textContent = "resize failed";
  els.benchmarkStatus.textContent = "failed";
  els.runBenchmark.disabled = true;
  els.benchmarkResult.textContent = `WASM_RESIZE_FAILURE error=${reason}`;
  setStatus(`state=wasm_resize_failed fatal=true fallback=disabled error=${reason}`);
  emitStructuredResizeFailure(failure);
}

function updateBackendControls() {
  const wasmActive = usingWasm();
  els.backendBadge.textContent = state.backend === "wasm" ? "WASM surface" : "WebGL diagnostic";
  els.drawBudget.disabled = wasmActive;
  els.pointScale.disabled = wasmActive;
}

function bindEvents() {
  window.addEventListener("resize", scheduleCanvasResize);
  if (typeof ResizeObserver === "function") {
    canvasResizeObserver = new ResizeObserver(scheduleCanvasResize);
    canvasResizeObserver.observe(els.canvas);
  }
  els.loadShowcase.addEventListener("click", () =>
    void loadDataset(DATASETS.showcase, "kitune1.ply"),
  );
  els.loadMinimal.addEventListener("click", () => void loadDataset(DATASETS.minimal, "minimal_ascii.ply"));
  els.loadFlowers.addEventListener("click", () => void loadDataset(DATASETS.flowers, "flowers_1.ply"));
  els.fileInput.addEventListener("change", async (event) => {
    const file = event.target.files?.[0];
    if (!file) {
      return;
    }
    await loadFile(file);
  });
  els.resetCamera.addEventListener("click", resetCamera);
  els.toggleOrbit.addEventListener("click", () => setAutoOrbit(!state.autoOrbit));
  els.themeToggle.addEventListener("click", () => {
    const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    setTheme(next, true);
  });
  els.runBenchmark.addEventListener("click", startBenchmark);

  for (const input of [els.drawBudget, els.sortInterval, els.pointScale, els.benchmarkYaw]) {
    input.addEventListener("input", updateControlLabels);
  }
  els.drawBudget.addEventListener("input", invalidateSortedOrder);
  els.sortInterval.addEventListener("input", () => {
    invalidateSortedOrder();
    if (wasmResizeCoordinator.pending) return;
    state.wasmRenderer?.setSortInterval(Number(els.sortInterval.value));
  });
  els.canvas.addEventListener("pointerdown", handlePointerDown);
  els.canvas.addEventListener("pointermove", handlePointerMove);
  els.canvas.addEventListener("pointerup", handlePointerEnd);
  els.canvas.addEventListener("pointercancel", handlePointerEnd);
  els.canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      stopInteractiveOrbit();
      zoomCamera(Math.exp(event.deltaY * 0.001));
      state.cameraStatus = "camera=zoom";
    },
    { passive: false },
  );
}

function handlePointerDown(event) {
  event.preventDefault();
  els.canvas.setPointerCapture(event.pointerId);
  if (state.pointers.size === 0) {
    maybeResetCameraFromDoubleTap(event.clientX, event.clientY);
  }
  stopInteractiveOrbit();
  state.pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
  beginGesture();
}

function handlePointerMove(event) {
  if (!state.pointers.has(event.pointerId)) {
    return;
  }
  event.preventDefault();
  state.pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });

  if (state.pointers.size >= 2) {
    handleTransformGesture();
  } else {
    handleOrbitGesture();
  }
}

function handlePointerEnd(event) {
  state.pointers.delete(event.pointerId);
  if (state.pointers.size === 0) {
    state.gesture = null;
    return;
  }
  beginGesture();
}

function beginGesture() {
  const points = [...state.pointers.values()];
  if (points.length >= 2) {
    state.gesture = {
      mode: "transform",
      lastSpan: pointerSpan(points),
      lastFocus: pointerFocus(points),
    };
    return;
  }

  const point = points[0];
  state.gesture = {
    mode: "orbit",
    lastX: point.x,
    lastY: point.y,
  };
}

function handleOrbitGesture() {
  if (!state.gesture || state.gesture.mode !== "orbit") {
    beginGesture();
    return;
  }
  const point = [...state.pointers.values()][0];
  const size = Math.max(1, Math.min(els.canvas.clientWidth, els.canvas.clientHeight));
  const dx = (point.x - state.gesture.lastX) / size;
  const dy = (point.y - state.gesture.lastY) / size;
  state.gesture.lastX = point.x;
  state.gesture.lastY = point.y;

  if (Math.abs(dx) < 0.0001 && Math.abs(dy) < 0.0001) {
    return;
  }
  orbitCamera(-dx * ORBIT_RADIANS_PER_SCREEN, -dy * ORBIT_RADIANS_PER_SCREEN);
  state.cameraStatus = "camera=orbit";
}

function handleTransformGesture() {
  if (!state.gesture || state.gesture.mode !== "transform") {
    beginGesture();
    return;
  }

  const points = [...state.pointers.values()];
  const span = pointerSpan(points);
  const focus = pointerFocus(points);
  if (state.gesture.lastSpan > 24 && span > 24) {
    const zoomScale = clamp(state.gesture.lastSpan / span, 0.5, 2.0);
    if (Math.abs(zoomScale - 1) > 0.003) {
      zoomCamera(zoomScale);
      state.cameraStatus = "camera=zoom";
    }
  }

  const dx = (focus.x - state.gesture.lastFocus.x) / Math.max(1, els.canvas.clientWidth);
  const dy = (focus.y - state.gesture.lastFocus.y) / Math.max(1, els.canvas.clientHeight);
  if (Math.abs(dx) > 0.0001 || Math.abs(dy) > 0.0001) {
    panCamera(dx, dy);
    if (state.cameraStatus !== "camera=zoom") {
      state.cameraStatus = "camera=pan";
    }
  }

  state.gesture.lastSpan = span;
  state.gesture.lastFocus = focus;
}

function maybeResetCameraFromDoubleTap(x, y) {
  const now = performance.now();
  const isDoubleTap =
    now - state.lastTapAt <= DOUBLE_TAP_TIMEOUT_MS &&
    Math.hypot(x - state.lastTapX, y - state.lastTapY) <= DOUBLE_TAP_SLOP_PX;

  if (isDoubleTap) {
    resetCamera();
    state.lastTapAt = 0;
    return;
  }

  state.lastTapAt = now;
  state.lastTapX = x;
  state.lastTapY = y;
}

function pointerSpan(points) {
  return Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y);
}

function pointerFocus(points) {
  let x = 0;
  let y = 0;
  for (const point of points) {
    x += point.x;
    y += point.y;
  }
  return { x: x / points.length, y: y / points.length };
}

function panCamera(normalizedDeltaX, normalizedDeltaY) {
  if (state.qualificationCamera || wasmResizeCoordinator.pending) return;
  state.wasmRenderer?.pan(normalizedDeltaX, normalizedDeltaY);

  const basis = buildCameraBasis();
  const aspect = Math.max(els.canvas.clientWidth / Math.max(els.canvas.clientHeight, 1), 0.1);
  const viewHeight = 2 * state.camera.distance * Math.tan(state.camera.fovY * 0.5);
  const viewWidth = viewHeight * aspect;
  const target = state.camera.target;
  for (let axis = 0; axis < 3; axis += 1) {
    target[axis] += -basis.right[axis] * normalizedDeltaX * viewWidth;
    target[axis] += basis.up[axis] * normalizedDeltaY * viewHeight;
  }
}

function orbitCamera(deltaYaw, deltaPitch) {
  if (state.qualificationCamera || wasmResizeCoordinator.pending) return;
  state.wasmRenderer?.orbit(deltaYaw, deltaPitch);
  state.camera.yaw += deltaYaw;
  state.camera.pitch = clamp(state.camera.pitch + deltaPitch, -1.35, 1.35);
  invalidateSortedOrder();
}

function zoomCamera(distanceScale) {
  if (state.qualificationCamera || wasmResizeCoordinator.pending) return;
  state.wasmRenderer?.zoom(distanceScale);
  state.camera.distance = clamp(state.camera.distance * distanceScale, 0.02, 10000);
  invalidateSortedOrder();
}

function stopInteractiveOrbit() {
  if (!state.autoOrbit) {
    return;
  }
  setAutoOrbit(false);
}

function setAutoOrbit(enabled) {
  state.autoOrbit = enabled;
  els.toggleOrbit.setAttribute("aria-pressed", String(enabled));
  els.toggleOrbit.textContent = enabled ? "Pause orbit" : "Play orbit";
  if (enabled) {
    state.cameraStatus = "camera=auto";
  }
}

async function loadDataset(path, name, options = {}) {
  const { allowFallback = false } = options;
  if (state.wasmModule && state.geometryPath === "packed") {
    return loadPackedDatasetStream(path, name, allowFallback);
  }
  setLoadingProgress(`Loading ${sceneTitle(name)}`, "Fetching scene data.", 0.04);
  setStatus(`state=loading dataset=${name}`);
  try {
    const response = await fetch(path);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    const bytes = await readResponseBytes(response, name);
    const datasetSha256 = await sha256Hex(bytes);
    setLoadingProgress("Reading captured light", `${formatBytes(bytes.byteLength)} received.`, 0.76);
    const scene = parsePly(bytes, name, path);
    scene.sourceBytes = bytes.byteLength;
    scene.sourceSha256 = datasetSha256;
    setLoadingProgress(
      "Building the scene",
      `${formatNumber(scene.count)} Gaussians ready for the GPU.`,
      0.86,
    );
    await applyScene(scene);
    return true;
  } catch (error) {
    state.lastLoadError = compactMessage(error);
    emitStructuredSceneFailure(name, error);
    console.error(`SCENE_LOAD_FAILURE dataset=${name} error=${state.lastLoadError}`);
    setStatus(`state=load_failed dataset=${name} error=${state.lastLoadError}`);
    if (allowFallback) {
      setLoadingProgress(
        `${sceneTitle(name)} is not installed`,
        "Trying the next local scene.",
        0.06,
      );
    } else {
      setLoadingProgress("Scene could not load", compactMessage(error), 0);
      window.setTimeout(hideLoading, 1400);
    }
    return false;
  }
}

async function loadPackedDatasetStream(path, name, allowFallback) {
  setLoadingProgress(`Loading ${sceneTitle(name)}`, "Streaming the complete PLY into WASM.", 0.04);
  setStatus(`state=loading_stream dataset=${name}`);
  await disposeWasmRenderer();
  try {
    const renderer = await createGsplatRendererFromUrl({
      module: state.wasmModule,
      canvas: els.canvas,
      url: path,
      width: els.canvas.width,
      height: els.canvas.height,
      sortInterval: Number(els.sortInterval.value),
      orderBackend: state.requestedOrderBackend,
      projectedPolicy: state.requestedProjectedPolicy,
      gpuOrderProducer: state.requestedGpuOrderProducer,
      geometryPath: "packed",
      onProgress: ({ receivedBytes, totalBytes }) => {
        const detail = totalBytes == null
          ? `${formatBytes(receivedBytes)} received`
          : `${formatBytes(receivedBytes)} of ${formatBytes(totalBytes)}`;
        const fraction = totalBytes == null ? 0.35 : receivedBytes / totalBytes;
        setLoadingProgress(`Loading ${sceneTitle(name)}`, detail, 0.04 + fraction * 0.72);
      },
    });
    installStreamedWasmScene(renderer, name, path);
    return true;
  } catch (error) {
    state.wasmUnavailableReason = compactMessage(error);
    state.lastLoadError = state.wasmUnavailableReason;
    emitStructuredSceneFailure(name, error);
    console.error(`SCENE_LOAD_FAILURE dataset=${name} error=${state.wasmUnavailableReason}`);
    await disposeWasmRenderer();
    setStatus(`state=load_failed dataset=${name} error=${state.wasmUnavailableReason}`);
    if (webglFailurePolicy() === "sampled_webgl_diagnostic") {
      // The exact streaming attempt consumed its response. Explicit
      // diagnostic opt-in permits one fresh fetch through the sampled parser;
      // default/product mode never takes this branch.
      state.wasmModule = null;
      ensureFallbackRenderer();
      return loadDataset(path, name, { allowFallback });
    }
    if (allowFallback) {
      setLoadingProgress(
        `${sceneTitle(name)} is not installed`,
        "Trying the next complete local scene.",
        0.06,
      );
    } else {
      setLoadingProgress("Scene could not load", state.wasmUnavailableReason, 0);
      window.setTimeout(hideLoading, 1400);
    }
    return false;
  }
}

function installStreamedWasmScene(renderer, name, sourcePath) {
  const summary = renderer.sceneSummary();
  const receipt = renderer.loadReceipt();
  if (!receipt?.streamed
      || receipt.sourceCount !== summary.gaussians
      || receipt.decodedCount !== summary.gaussians
      || receipt.encodedCount !== summary.gaussians
      || receipt.residentCount !== summary.gaussians
      || receipt.addressableCount !== summary.gaussians
      || receipt.sourceShDegree !== summary.shDegree
      || receipt.residentShDegree !== summary.shDegree
      || receipt.shDegree !== summary.shDegree
      || receipt.fullQuality !== true
      || receipt.sourceMembership !== "all"
      || receipt.samplingEnabled
      || receipt.lodEnabled
      || receipt.partialScenePublished) {
    renderer.free();
    throw new Error("streamed scene count/SH receipt is incomplete or inconsistent");
  }
  const scene = {
    name,
    sourcePath,
    format: "streamed_ply",
    shDegree: summary.shDegree,
    count: summary.gaussians,
    sourceBytes: receipt.transportBytes,
    sourceSha256: receipt.inputSha256,
    exactnessReceipt: receipt,
    rawBytes: null,
  };
  state.scene = scene;
  state.datasetPath = sourcePath;
  state.frameCounter = 0;
  state.benchmark = null;
  state.scratchDepths = new Float32Array(0);
  invalidateSortedOrder();
  els.benchmarkStatus.textContent = "idle";
  els.runBenchmark.disabled = false;
  els.formatBadge.textContent = "streamed ply";
  els.gaussianCount.textContent = formatNumber(scene.count);
  els.shDegree.textContent = String(scene.shDegree);
  els.sceneName.textContent = sceneTitle(scene.name);
  els.sceneMeta.textContent = `${formatNumber(scene.count)} gaussians · full streamed PLY`;
  for (const button of els.sceneButtons) {
    button.setAttribute("aria-pressed", String(button.dataset.scene === scene.name));
  }
  adoptWasmRenderer(renderer, scene);
  console.info(`SCENE_LOAD_RECEIPT_JSON ${JSON.stringify({
    dataset: name,
    source_path: sourcePath,
    source_bytes: receipt.transportBytes,
    input_sha256: receipt.inputSha256,
    peak_decoder_buffer_bytes: receipt.peakDecoderBufferBytes,
    streamed: receipt.streamed,
    source_count: receipt.sourceCount,
    decoded_count: receipt.decodedCount,
    encoded_count: receipt.encodedCount,
    resident_count: receipt.residentCount,
    addressable_count: receipt.addressableCount,
    source_sh_degree: receipt.sourceShDegree,
    resident_sh_degree: receipt.residentShDegree,
    sh_degree: receipt.shDegree,
    full_quality: receipt.fullQuality,
    source_membership: receipt.sourceMembership,
    sampling_enabled: receipt.samplingEnabled,
    lod_enabled: receipt.lodEnabled,
    partial_scene_published: receipt.partialScenePublished,
  })}`);
  setStatus(
    `state=scene_ready backend=wasm source=${receipt.sourceCount} ` +
    `resident=${receipt.residentCount} peak_decoder_bytes=${receipt.peakDecoderBufferBytes}`,
  );
  setLoadingProgress("Scene ready", "Complete source count is resident on the GPU.", 1);
  hideLoading();
  if (state.autoStartBenchmark) {
    state.autoStartBenchmark = false;
    if (state.autoBenchmarkSync) runBenchmarkSync();
    else startBenchmark();
  }
}

async function loadFile(file) {
  setLoadingProgress(`Opening ${file.name}`, "Reading the local PLY in your browser.", 0.12);
  setStatus(`state=loading dataset=${file.name}`);
  try {
    if (state.wasmModule && state.geometryPath === "packed" && typeof file.stream === "function") {
      await disposeWasmRenderer();
      const renderer = await createGsplatRendererFromStream({
        module: state.wasmModule,
        canvas: els.canvas,
        stream: file.stream(),
        totalBytes: file.size,
        width: els.canvas.width,
        height: els.canvas.height,
        sortInterval: Number(els.sortInterval.value),
        orderBackend: state.requestedOrderBackend,
        projectedPolicy: state.requestedProjectedPolicy,
        gpuOrderProducer: state.requestedGpuOrderProducer,
        geometryPath: "packed",
        onProgress: ({ receivedBytes, totalBytes }) => {
          setLoadingProgress(
            `Opening ${file.name}`,
            `${formatBytes(receivedBytes)} of ${formatBytes(totalBytes ?? file.size)}`,
            0.12 + (receivedBytes / Math.max(totalBytes ?? file.size, 1)) * 0.7,
          );
        },
      });
      installStreamedWasmScene(renderer, file.name, `browser:${file.name}`);
      return;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    const datasetSha256 = await sha256Hex(bytes);
    setLoadingProgress("Reading captured light", `${formatBytes(bytes.byteLength)} received.`, 0.76);
    const scene = parsePly(bytes, file.name, `browser:${file.name}`);
    scene.sourceBytes = bytes.byteLength;
    scene.sourceSha256 = datasetSha256;
    await applyScene(scene);
  } catch (error) {
    emitStructuredSceneFailure(file.name, error);
    setStatus(`state=parse_failed dataset=${file.name} error=${compactMessage(error)}`);
    setLoadingProgress("PLY could not open", compactMessage(error), 0);
    window.setTimeout(hideLoading, 1400);
  }
}

async function applyScene(scene) {
  state.scene = scene;
  state.datasetPath = scene.sourcePath;
  state.frameCounter = 0;
  state.benchmark = null;
  els.benchmarkStatus.textContent = "idle";
  els.runBenchmark.disabled = false;
  if (state.scratchDepths.length < scene.count) {
    state.scratchDepths = new Float32Array(scene.count);
  }
  invalidateSortedOrder();
  fitCameraToScene(scene);
  els.formatBadge.textContent = scene.format.replace("_", " ");
  els.gaussianCount.textContent = formatNumber(scene.count);
  els.shDegree.textContent = String(scene.shDegree);
  els.sceneName.textContent = sceneTitle(scene.name);
  els.sceneMeta.textContent = `${formatNumber(scene.count)} gaussians · ${formatLabel(scene.format)}`;
  for (const button of els.sceneButtons) {
    button.setAttribute("aria-pressed", String(button.dataset.scene === scene.name));
  }

  if (state.wasmModule) {
    setLoadingProgress("Uploading to the GPU", "Preparing the realtime surface.", 0.92);
    const wasmReady = await createWasmRenderer(scene);
    if (!wasmReady && (state.strictBenchmarkMode || state.qualificationTraceUrl || state.qualificationTrace)) {
      state.autoStartBenchmark = false;
      els.runBenchmark.disabled = true;
      setLoadingProgress("Exact renderer failed", state.wasmUnavailableReason, 0);
      return;
    }
  } else {
    if (webglFailurePolicy() !== "sampled_webgl_diagnostic") {
      state.autoStartBenchmark = false;
      state.wasmFatalError = "wasm_module_unavailable";
      els.gpuStatus.textContent = "failed";
      els.benchmarkStatus.textContent = "failed";
      els.runBenchmark.disabled = true;
      setStatus("state=wasm_unavailable qualification=failed");
      setLoadingProgress("Exact renderer unavailable", "WASM/WebGPU is required for qualification.", 0);
      throw new Error(
        `exact WebGPU renderer unavailable: ${state.wasmUnavailableReason || "WASM module unavailable"}`,
      );
    }
    ensureFallbackRenderer();
    updateBackendControls();
  }

  setStatus(`state=scene_ready backend=${usingWasm() ? "wasm" : "webgl"}`);
  setLoadingProgress("Scene ready", "Drag anywhere to explore.", 1);
  hideLoading();
  if (state.autoStartBenchmark) {
    state.autoStartBenchmark = false;
    if (state.autoBenchmarkSync) {
      runBenchmarkSync();
    } else {
      startBenchmark();
    }
  }
}

function parsePly(bytes, name, sourcePath) {
  const { headerText, bodyOffset } = splitHeader(bytes);
  const header = parseHeader(headerText);
  for (const field of REQUIRED_FIELDS) {
    if (!header.propertyMap.has(field)) {
      throw new Error(`missing field ${field}`);
    }
  }

  const scene = allocateScene(header.vertexCount, name, sourcePath, header);
  if (header.format === "ascii") {
    parseAsciiBody(bytes, bodyOffset, header, scene);
  } else if (header.format === "binary_little_endian") {
    parseBinaryBody(bytes, bodyOffset, header, scene, true);
  } else if (header.format === "binary_big_endian") {
    parseBinaryBody(bytes, bodyOffset, header, scene, false);
  } else {
    throw new Error(`unsupported format ${header.format}`);
  }

  computeBounds(scene);
  scene.rawBytes = bytes;
  return scene;
}

function splitHeader(bytes) {
  const preview = new TextDecoder("ascii").decode(bytes.slice(0, Math.min(bytes.length, 131072)));
  let marker = "end_header\n";
  let idx = preview.indexOf(marker);
  if (idx < 0) {
    marker = "end_header\r\n";
    idx = preview.indexOf(marker);
  }
  if (idx < 0) {
    throw new Error("malformed header");
  }
  const bodyOffset = idx + marker.length;
  return {
    headerText: preview.slice(0, bodyOffset),
    bodyOffset,
  };
}

function parseHeader(headerText) {
  const lines = headerText.split(/\r?\n/);
  let format = "";
  let vertexCount = 0;
  let inVertex = false;
  const properties = [];
  let stride = 0;

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed === "ply" || trimmed.startsWith("comment")) {
      continue;
    }
    const parts = trimmed.split(/\s+/);
    if (parts[0] === "format") {
      format = parts[1]?.replace("1.0", "") === "ascii" ? "ascii" : parts[1];
      continue;
    }
    if (parts[0] === "element") {
      inVertex = parts[1] === "vertex";
      if (inVertex) {
        vertexCount = Number(parts[2]);
      }
      continue;
    }
    if (inVertex && parts[0] === "property") {
      if (parts[1] === "list") {
        throw new Error("vertex list properties are not supported");
      }
      const type = parts[1];
      const name = parts[2];
      const size = TYPE_SIZES.get(type);
      if (!size) {
        throw new Error(`unsupported property type ${type}`);
      }
      properties.push({ type, name, offset: stride, size });
      stride += size;
    }
  }

  if (!format || !Number.isFinite(vertexCount) || vertexCount <= 0) {
    throw new Error("missing vertex element");
  }

  const propertyMap = new Map(properties.map((property, index) => [property.name, { ...property, index }]));
  const restCount = properties.filter((property) => property.name.startsWith("f_rest_")).length;
  const coeffTotal = restCount > 0 ? restCount / 3 + 1 : 1;
  const shDegree = Math.max(0, Math.round(Math.sqrt(coeffTotal) - 1));

  return {
    format,
    vertexCount,
    properties,
    propertyMap,
    stride,
    shDegree,
  };
}

function allocateScene(count, name, sourcePath, header) {
  return {
    name,
    sourcePath,
    format: header.format,
    shDegree: header.shDegree,
    rawBytes: null,
    count,
    positions: new Float32Array(count * 3),
    colors: new Float32Array(count * 3),
    alphas: new Float32Array(count),
    radii: new Float32Array(count),
    boundsMin: [0, 0, 0],
    boundsMax: [0, 0, 0],
  };
}

function parseAsciiBody(bytes, bodyOffset, header, scene) {
  const text = new TextDecoder("utf-8").decode(bytes.slice(bodyOffset));
  const lines = text.trim().split(/\r?\n/);
  if (lines.length < header.vertexCount) {
    throw new Error("vertex count mismatch");
  }

  const read = (values, name) => parseNumericToken(values[header.propertyMap.get(name).index]);
  const get = (values, name) => finiteNumber(read(values, name), name);
  for (let i = 0; i < header.vertexCount; i += 1) {
    const values = lines[i].trim().split(/\s+/);
    if (values.length < header.properties.length) {
      throw new Error(`short vertex row ${i}`);
    }
    writeSceneVertex(scene, i, {
      x: get(values, "x"),
      y: get(values, "y"),
      z: get(values, "z"),
      opacity: normalizeOpacityLogit(read(values, "opacity")),
      scale0: get(values, "scale_0"),
      scale1: get(values, "scale_1"),
      scale2: get(values, "scale_2"),
      dc0: get(values, "f_dc_0"),
      dc1: get(values, "f_dc_1"),
      dc2: get(values, "f_dc_2"),
    });
  }
}

function parseBinaryBody(bytes, bodyOffset, header, scene, littleEndian) {
  const expected = bodyOffset + header.vertexCount * header.stride;
  if (expected > bytes.byteLength) {
    throw new Error("binary body is shorter than declared vertex count");
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const readRaw = (base, name) =>
    readProperty(
      view,
      base + header.propertyMap.get(name).offset,
      header.propertyMap.get(name).type,
      littleEndian,
    );
  const read = (base, name) => finiteNumber(readRaw(base, name), name);

  for (let i = 0; i < header.vertexCount; i += 1) {
    const base = bodyOffset + i * header.stride;
    writeSceneVertex(scene, i, {
      x: read(base, "x"),
      y: read(base, "y"),
      z: read(base, "z"),
      opacity: normalizeOpacityLogit(readRaw(base, "opacity")),
      scale0: read(base, "scale_0"),
      scale1: read(base, "scale_1"),
      scale2: read(base, "scale_2"),
      dc0: read(base, "f_dc_0"),
      dc1: read(base, "f_dc_1"),
      dc2: read(base, "f_dc_2"),
    });
  }
}

function readProperty(view, offset, type, littleEndian) {
  switch (type) {
    case "char":
    case "int8":
      return view.getInt8(offset);
    case "uchar":
    case "uint8":
      return view.getUint8(offset);
    case "short":
    case "int16":
      return view.getInt16(offset, littleEndian);
    case "ushort":
    case "uint16":
      return view.getUint16(offset, littleEndian);
    case "int":
    case "int32":
      return view.getInt32(offset, littleEndian);
    case "uint":
    case "uint32":
      return view.getUint32(offset, littleEndian);
    case "double":
    case "float64":
      return view.getFloat64(offset, littleEndian);
    default:
      return view.getFloat32(offset, littleEndian);
  }
}

function writeSceneVertex(scene, i, vertex) {
  const p = i * 3;
  scene.positions[p] = vertex.x;
  scene.positions[p + 1] = -vertex.y;
  scene.positions[p + 2] = vertex.z;

  const c0 = 0.28209479177387814;
  scene.colors[p] = clamp(c0 * vertex.dc0 + 0.5, 0, 1);
  scene.colors[p + 1] = clamp(c0 * vertex.dc1 + 0.5, 0, 1);
  scene.colors[p + 2] = clamp(c0 * vertex.dc2 + 0.5, 0, 1);
  scene.alphas[i] = clamp(sigmoid(vertex.opacity), 0, 1);
  scene.radii[i] = Math.exp(Math.max(vertex.scale0, vertex.scale1, vertex.scale2));
}

function computeBounds(scene) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < scene.count; i += 1) {
    const p = i * 3;
    for (let axis = 0; axis < 3; axis += 1) {
      const value = scene.positions[p + axis];
      min[axis] = Math.min(min[axis], value);
      max[axis] = Math.max(max[axis], value);
    }
  }
  scene.boundsMin = min;
  scene.boundsMax = max;
}

function fitCameraToScene(scene) {
  if (state.qualificationCamera) {
    const camera = state.qualificationCamera;
    state.camera.fovY = camera.intrinsics.verticalFovRadians;
    state.camera.near = camera.intrinsics.nearPlane;
    state.camera.far = camera.intrinsics.farPlane;
    state.wasmRenderer?.setCamera(camera);
    setAutoOrbit(false);
    state.cameraStatus = "camera=qualification_trace";
    invalidateSortedOrder();
    return;
  }
  const min = scene.boundsMin;
  const max = scene.boundsMax;
  const center = [
    (min[0] + max[0]) * 0.5,
    (min[1] + max[1]) * 0.5,
    (min[2] + max[2]) * 0.5,
  ];
  const extent = [
    Math.max(max[0] - min[0], 1e-3),
    Math.max(max[1] - min[1], 1e-3),
    Math.max(max[2] - min[2], 1e-3),
  ];
  const radius = Math.max(extent[0], extent[1], extent[2]) * 0.5;
  const aspect = Math.max(els.canvas.clientWidth / Math.max(els.canvas.clientHeight, 1), 0.1);
  const vfov = state.camera.fovY;
  const hfov = 2 * Math.atan(Math.tan(vfov * 0.5) * aspect);
  const distY = (extent[1] * 0.5) / Math.tan(vfov * 0.5);
  const distX = (extent[0] * 0.5) / Math.tan(hfov * 0.5);
  const distance = (Math.max(distX, distY) + extent[2] * 0.5) * 1.35;

  state.camera.target = center;
  state.camera.distance = Math.max(distance, 0.2);
  state.camera.yaw = 0;
  state.camera.pitch = 0;
  state.camera.near = Math.max(0.01, state.camera.distance - radius * 2);
  state.camera.far = Math.max(100, state.camera.distance + radius * 8);
  state.cameraStatus = "camera=auto";
  invalidateSortedOrder();
}

function resetCamera() {
  if (!state.scene || wasmResizeCoordinator.pending) {
    return;
  }
  fitCameraToScene(state.scene);
  if (state.qualificationCamera) {
    state.wasmRenderer?.setCamera(state.qualificationCamera);
  } else {
    state.wasmRenderer?.resetCamera();
  }
  state.cameraStatus = "camera=reset";
}

function frame(now) {
  const dt = Math.min((now - state.lastFrameTime) / 1000, 0.05);
  state.lastFrameTime = now;
  state.fps = dt > 0 ? 1 / dt : state.fps;

  // The bounded M4 smoke is complete after its one presentation and terminal
  // current-stats receipt. Stop submitting new frames so its retained result
  // describes one stable endpoint observation rather than a stale snapshot of
  // a renderer that continues to mutate in the background.
  if (state.currentStatsSmokeEnabled && state.currentStatsSmokeCompleted) {
    return;
  }
  if (state.benchmark?.terminalQueueThroughput
      && state.benchmark.terminalQueueThroughput.state === "complete") {
    return;
  }

  // resizeAsync holds the wasm renderer mutably until WebGPU error scopes
  // complete. Never submit or poll a frame against an in-flight/unknown size.
  if (state.resizeMeasurePending || wasmResizeCoordinator.pending) {
    requestAnimationFrame(frame);
    return;
  }

  const benchmark = state.benchmark;
  if (benchmark?.enabled) benchmark.animationFrameTimestampMs = now;
  if (benchmark?.q1Capture?.armPending || benchmark?.q1Capture?.takePending) {
    requestAnimationFrame(frame);
    return;
  }
  if (usingWasm() && benchmark?.enabled && benchmark.terminalQueueThroughput
      && progressBenchmarkTerminalReceipt(benchmark)) {
    requestAnimationFrame(frame);
    return;
  }
  const currentStatsSchedule = benchmark?.currentStatsSchedule ?? null;
  if (usingWasm() && benchmark?.enabled
      && currentStatsSchedule?.pendingCount > 0) {
    pollBenchmarkCurrentStatsReceipts(benchmark);
    if (!benchmark.enabled) {
      requestAnimationFrame(frame);
      return;
    }
  }
  if (usingWasm() && benchmark?.enabled && currentStatsSchedule) {
    if (currentStatsSchedule.action === "fail_capacity") {
      try {
        currentStatsSchedule.noteDraw(now);
      } catch (error) {
        failStrictBenchmarkForOrderEvidence(compactMessage(error));
      }
      requestAnimationFrame(frame);
      return;
    }
    if (currentStatsSchedule.action !== "draw") {
      requestAnimationFrame(frame);
      return;
    }
  }

  const benchmarkWaitingForTerminal = benchmark?.enabled
    && (benchmark.pendingOrderSample != null
      || state.benchmark.pendingProjectedSample != null
      || state.benchmark.pendingGpuProducerSample != null);
  if (usingWasm() && benchmarkWaitingForTerminal
      && state.benchmark.orderCompletionProtocol === "isolated_terminal") {
    pollPendingBenchmarkReceipts(benchmark);
    requestAnimationFrame(frame);
    return;
  }
  if (usingWasm() && benchmark?.enabled && maybeArmQ1ControlCapture(benchmark)) {
    requestAnimationFrame(frame);
    return;
  }
  if (!state.gpuOrderPreparationPending && state.benchmark?.enabled
      && !benchmarkWaitingForTerminal
      && !currentStatsSchedule?.requestOutstanding) {
    if (state.benchmark.traceSequence) {
      applyBenchmarkTraceStep(state.benchmark);
    } else if (state.benchmark.fixedCameraPrimePending && state.qualificationCamera) {
      const alternateIndex = state.qualificationTraceFrameIndex === 0 ? 1 : 0;
      state.wasmRenderer?.setCamera(
        cameraTraceFrame(state.qualificationTrace, alternateIndex),
      );
      state.benchmark.fixedCameraPrimePending = false;
      state.benchmark.primingOrder = true;
      state.cameraStatus = "camera=qualification_preflight";
    } else if (state.benchmark.fixedCameraApplyPending && state.qualificationCamera) {
      // Apply the fixed trace on the same RAF turn as its first formal draw.
      // This mirrors sequence playback and prevents a constructor/startup
      // boundary from standing in for the benchmark's order invalidation.
      state.wasmRenderer?.setCamera(state.qualificationCamera);
      state.benchmark.fixedCameraApplyPending = false;
      state.cameraStatus = "camera=qualification_trace";
    } else if (state.benchmark.yawStep !== 0) {
      orbitCamera(state.benchmark.yawStep, 0);
      state.cameraStatus = "camera=benchmark_orbit";
    }
  } else if (!state.gpuOrderPreparationPending && state.autoOrbit && state.scene) {
    orbitCamera(dt * 0.22, 0);
  }

  const stats = render();
  if (stats && state.benchmark?.enabled) {
    recordBenchmark(stats);
    maybeTakeQ1ControlCapture(state.benchmark, stats);
  }
  requestAnimationFrame(frame);
}

function q1CaptureLogicalSubmissionIndex(benchmark) {
  return benchmark.warmupFrames + benchmark.q1Capture.measuredFrameIndex;
}

function maybeArmQ1ControlCapture(benchmark) {
  const capture = benchmark.q1Capture;
  const schedule = benchmark.currentStatsSchedule;
  if (!capture || capture.armed || capture.ready || capture.armPending || capture.takePending
      || !schedule || schedule.requestOutstanding
      || schedule.nextSubmissionIndex !== q1CaptureLogicalSubmissionIndex(benchmark)) {
    return false;
  }
  capture.armPending = true;
  void state.wasmRenderer.requestDiagnosticSurfaceCapture().then(() => {
    capture.armPending = false;
    capture.armed = true;
  }).catch((error) => {
    capture.armPending = false;
    failStrictBenchmarkForOrderEvidence(
      `Q1 renderer capture arm failed: ${compactMessage(error)}`,
    );
  });
  return true;
}

function maybeTakeQ1ControlCapture(benchmark, stats) {
  const capture = benchmark.q1Capture;
  const step = benchmark.currentTraceStep;
  if (!capture || !capture.armed || capture.takePending || capture.ready
      || !stats.framePresented || stats.currentStatsSubmission !== "issued"
      || step?.phase !== "measured"
      || step.phaseFrameIndex !== capture.measuredFrameIndex
      || step.traceFrameIndex !== capture.traceFrameIndex) {
    return;
  }
  capture.armed = false;
  capture.takePending = true;
  void state.wasmRenderer.takeDiagnosticSurfaceCapture().then((raw) => {
    const normalized = normalizeQ1SurfaceCapture(raw);
    capture.takePending = false;
    capture.ready = normalized;
    globalThis.GSPLAT_Q1_RENDERER_CAPTURE = {
      trace_frame_index: capture.traceFrameIndex,
      measured_frame_index: capture.measuredFrameIndex,
      receipt: normalized.receipt,
      rgba8: normalized.rgba8,
    };
  }).catch((error) => {
    capture.takePending = false;
    failStrictBenchmarkForOrderEvidence(
      `Q1 renderer capture take failed: ${compactMessage(error)}`,
    );
  });
}

function progressBenchmarkTerminalReceipt(benchmark) {
  const window = benchmark.terminalQueueThroughput;
  if (!window || window.action === "draw") return false;
  try {
    if (window.action === "accept_measured_input") {
      // Freeze the common Q0 start before the first measured camera mutation,
      // order work, encoding, submission, and presentation.
      window.beginMeasuredInput({ acceptedAtMonotonicMs: performance.now() });
      if (window.action !== "request_final_receipt") return false;
      // N=1 accepts its first input and arms the final-frame receipt in this
      // same RAF turn, still before setCamera/render below.
    }
    if (window.action === "request_warmup_receipt"
        || window.action === "request_final_receipt") {
      const phase = window.action === "request_warmup_receipt"
        ? "warmup_boundary"
        : "final_measured";
      const request = state.wasmRenderer.requestCurrentStats();
      if (request.status !== "requested") {
        throw new Error(
          `${phase} current-stats request was ${request.status ?? "missing"}: `
          + `${request.reason ?? "unknown"}`,
        );
      }
      const requestedAtMonotonicMs = performance.now();
      if (phase === "warmup_boundary") {
        window.beginWarmupReceipt({ requestedAtMonotonicMs });
      } else {
        window.beginFinalReceipt({ requestedAtMonotonicMs });
      }
      benchmark.terminalCurrentStatsRequestOutstanding = true;
      // Continue in this same RAF turn. The renderer encodes the receipt in
      // the boundary draw's command buffer, without another queue submission.
      return false;
    }
    if (window.action === "poll_warmup_receipt"
        || window.action === "poll_final_receipt") {
      const phase = window.action === "poll_warmup_receipt"
        ? "warmup_boundary"
        : "final_measured";
      let terminal = benchmark.terminalCurrentStatsObserved;
      if (terminal === null) {
        terminal = state.wasmRenderer.pollCurrentStats();
        benchmark.terminalPollCount += 1;
        if (terminal.status === "empty") return true;
        if (terminal.status === "unsampled") {
          throw new Error(
            `${phase} current-stats receipt was unsampled: ${terminal.reason ?? "unknown"}`,
          );
        }
        benchmark.terminalCurrentStatsObserved = terminal;
      }
      const pending = benchmark.terminalCurrentStatsPending;
      if (!pending || pending.phase !== phase) {
        throw new Error(
          `${phase} current-stats terminal ${terminal.status ?? "missing"} `
          + "had no matching issued ticket",
        );
      }
      if (terminal.status !== "ready") {
        joinBenchmarkCurrentStatsTerminal(
          benchmark,
          pending,
          terminal,
          performance.now(),
        );
        return true;
      }
      let terminalAtMonotonicMs = performance.now();
      if (state.q1QueueTerminalEnabled) {
        const queueTerminal = state.wasmRenderer.pollDiagnosticQueueTerminal();
        if (queueTerminal.status === "pending") return true;
        if (queueTerminal.status !== "ready"
            || !Number.isFinite(queueTerminal.completedAtMonotonicMs)) {
          throw new Error(`${phase} queue terminal callback returned invalid evidence`);
        }
        terminalAtMonotonicMs = queueTerminal.completedAtMonotonicMs;
      }
      const joined = joinBenchmarkCurrentStatsTerminal(
        benchmark,
        pending,
        terminal,
        terminalAtMonotonicMs,
      );
      if (!joined || !benchmark.enabled) return true;
      const receipt = {
        ticket: terminal.ticket,
        status: terminal.status,
        plan: terminal.plan,
        terminalAtMonotonicMs,
      };
      if (phase === "warmup_boundary") {
        window.recordWarmupReceipt(receipt);
      } else {
        window.recordTerminalReceipt(receipt);
      }
      benchmark.terminalCurrentStatsPending = null;
      benchmark.terminalCurrentStatsObserved = null;
      benchmark.terminalCurrentStatsRequestOutstanding = false;
      if (phase === "warmup_boundary") {
        benchmark.lastObservedFrameMs = null;
      } else {
        finishBenchmark(benchmark);
      }
      return true;
    }
    return true;
  } catch (error) {
    failStrictBenchmarkForOrderEvidence(
      `terminal-boundary current-stats receipt failed closed: ${compactMessage(error)}`,
    );
    return true;
  }
}

function render() {
  if (state.resizeMeasurePending || wasmResizeCoordinator.pending) {
    return null;
  }
  if (usingWasm()) {
    if (state.wasmFatalError) return null;
    return renderWasm();
  }
  return renderWebgl();
}

function emitOrderMeasurements(measurements, adaptiveState) {
  for (const measurement of measurements) {
    const terminalAtMonotonicMs = performance.now();
    if (recordAuxiliaryCurrentStatsFormalTerminal({
      actualBackend: measurement.actualBackend ?? "gpu",
      ticket: measurement.ticket,
      cameraRevision: measurement.cameraRevision,
      outcome: "success",
      terminalAtMonotonicMs,
    })) continue;
    console.info(`ORDER_MEASUREMENT_JSON ${JSON.stringify({
      requested_backend: state.requestedOrderBackend,
      // SurfaceOrderMeasurement is emitted only by the GPU telemetry ring.
      // The frame that harvests it may already be a CPU frame during an
      // Adaptive ABBA probe, so the harvesting frame backend is not the
      // backend that produced this receipt.
      actual_backend: measurement.actualBackend ?? "gpu",
      adaptive_state: adaptiveState,
      ticket: measurement.ticket,
      camera_revision: measurement.cameraRevision,
      timing_source: measurement.timingSource,
      gpu_preprocess_ms: measurement.gpuPreprocessMs,
      gpu_radix_ms: measurement.gpuRadixMs,
      gpu_order_ms: measurement.gpuOrderMs,
      gpu_complete_ms: measurement.gpuCompleteMs,
      timestamp_period_ns: measurement.timestampPeriodNs,
      below_timestamp_resolution: measurement.belowTimestampResolution,
      count_semantics: measurement.countSemantics ?? "candidate_visible_contributor_issued_v1",
      visible: measurement.visibleCount,
      contributor: measurement.contributorCount,
      drawn: measurement.drawnCount,
      exact_contributor_compaction: Boolean(measurement.exactContributorCompaction),
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordOrderTerminalReceipt(
      measurement.actualBackend ?? "gpu",
      measurement.ticket,
      measurement.cameraRevision,
      "success",
      terminalAtMonotonicMs,
    );
  }
}

function emitCpuOrderMeasurements(measurements, adaptiveState) {
  for (const measurement of measurements) {
    const terminalAtMonotonicMs = performance.now();
    if (recordAuxiliaryCurrentStatsFormalTerminal({
      actualBackend: "cpu",
      ticket: measurement.ticket,
      cameraRevision: measurement.cameraRevision,
      outcome: "success",
      terminalAtMonotonicMs,
    })) continue;
    console.info(`CPU_ORDER_MEASUREMENT_JSON ${JSON.stringify({
      requested_backend: state.requestedOrderBackend,
      actual_backend: "cpu",
      adaptive_state: adaptiveState,
      ticket: measurement.ticket,
      camera_revision: measurement.cameraRevision,
      preprocess_ms: measurement.preprocessMs,
      sort_ms: measurement.sortMs,
      frame_complete_ms: measurement.frameCompleteMs,
      count_semantics: measurement.countSemantics ?? "candidate_visible_contributor_issued_v1",
      visible: measurement.visibleCount,
      contributor: measurement.contributorCount,
      drawn: measurement.drawnCount,
      exact_contributor_compaction: Boolean(measurement.exactContributorCompaction),
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordOrderTerminalReceipt(
      "cpu",
      measurement.ticket,
      measurement.cameraRevision,
      "success",
      terminalAtMonotonicMs,
    );
  }
}

function emitOrderMeasurementFailures(failures, adaptiveState) {
  for (const failure of failures) {
    const terminalAtMonotonicMs = performance.now();
    if (recordAuxiliaryCurrentStatsFormalTerminal({
      actualBackend: failure.actualBackend ?? (failure.ticket % 2 === 0 ? "cpu" : "gpu"),
      ticket: failure.ticket,
      cameraRevision: failure.cameraRevision,
      outcome: "failure",
      reason: failure.reason,
      terminalAtMonotonicMs,
    })) {
      failStrictBenchmarkForOrderEvidence(
        `auxiliary formal order ticket=${failure.ticket} failed: ${failure.reason}`,
      );
      continue;
    }
    console.error(`ORDER_MEASUREMENT_FAILURE_JSON ${JSON.stringify({
      requested_backend: state.requestedOrderBackend,
      actual_backend: failure.actualBackend ?? (failure.ticket % 2 === 0 ? "cpu" : "gpu"),
      adaptive_state: adaptiveState,
      ticket: failure.ticket,
      camera_revision: failure.cameraRevision,
      reason: failure.reason,
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordOrderTerminalReceipt(
      failure.actualBackend ?? (failure.ticket % 2 === 0 ? "cpu" : "gpu"),
      failure.ticket,
      failure.cameraRevision,
      "failure",
      terminalAtMonotonicMs,
    );
  }
}

function emitProjectedMeasurements(measurements, adaptiveState) {
  for (const measurement of measurements) {
    const terminalAtMonotonicMs = performance.now();
    console.info(`PROJECTED_MEASUREMENT_JSON ${JSON.stringify({
      run_id: state.benchmark?.collector.runId ?? null,
      requested_policy: state.requestedProjectedPolicy,
      execution: measurement.execution,
      order_backend: measurement.orderBackend,
      adaptive_state: adaptiveState,
      ticket: measurement.ticket,
      camera_revision: measurement.cameraRevision,
      projection_generation: measurement.projectionGeneration,
      probe_generation: measurement.probeGeneration,
      projection_rebuilt: measurement.projectionRebuilt,
      order_refreshed: measurement.orderRefreshed,
      frame_complete_ms: measurement.frameCompleteMs,
      count_semantics:
        measurement.countSemantics ?? "candidate_visible_contributor_issued_v1",
      visible: measurement.visibleCount,
      contributor: measurement.contributorCount,
      drawn: measurement.drawnCount,
      exact_contributor_compaction: Boolean(measurement.exactContributorCompaction),
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordProjectedTerminalReceipt(
      measurement.execution,
      measurement.orderBackend,
      measurement.ticket,
      measurement.cameraRevision,
      "success",
      terminalAtMonotonicMs,
    );
  }
}

function emitProjectedMeasurementFailures(failures, adaptiveState) {
  for (const failure of failures) {
    const terminalAtMonotonicMs = performance.now();
    console.error(`PROJECTED_MEASUREMENT_FAILURE_JSON ${JSON.stringify({
      run_id: state.benchmark?.collector.runId ?? null,
      requested_policy: state.requestedProjectedPolicy,
      execution: failure.execution,
      order_backend: failure.orderBackend,
      adaptive_state: adaptiveState,
      ticket: failure.ticket,
      camera_revision: failure.cameraRevision,
      projection_generation: failure.projectionGeneration,
      probe_generation: failure.probeGeneration,
      reason: failure.reason,
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordProjectedTerminalReceipt(
      failure.execution,
      failure.orderBackend,
      failure.ticket,
      failure.cameraRevision,
      "failure",
      terminalAtMonotonicMs,
    );
  }
}

function emitGpuProducerMeasurements(measurements) {
  for (const measurement of measurements) {
    const terminalAtMonotonicMs = performance.now();
    const runId = state.benchmark?.issuedGpuProducerTickets.has(measurement.ticket)
      ? state.benchmark.collector.runId
      : null;
    console.info(`GPU_PRODUCER_MEASUREMENT_JSON ${JSON.stringify({
      run_id: runId,
      producer: measurement.producer,
      ticket: measurement.ticket,
      camera_revision: measurement.cameraRevision,
      order_generation: measurement.orderGeneration,
      projection_generation: measurement.projectionGeneration,
      queue_complete_ms: measurement.queueCompleteMs,
      count_semantics: measurement.countSemantics ?? "source_contributor_issued_v1",
      source: measurement.sourceCount,
      contributor: measurement.contributorCount,
      drawn: measurement.drawnCount,
      order_refreshed: measurement.orderRefreshed,
      draw_scope: measurement.drawScope,
      exact_current_contributor_draw: measurement.exactCurrentContributorDraw,
      stale_order: measurement.staleOrder,
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordGpuProducerTerminalReceipt(
      measurement.producer,
      measurement.ticket,
      measurement.cameraRevision,
      "success",
      terminalAtMonotonicMs,
    );
  }
}

function emitGpuProducerMeasurementFailures(failures) {
  for (const failure of failures) {
    const terminalAtMonotonicMs = performance.now();
    const runId = state.benchmark?.issuedGpuProducerTickets.has(failure.ticket)
      ? state.benchmark.collector.runId
      : null;
    console.error(`GPU_PRODUCER_MEASUREMENT_FAILURE_JSON ${JSON.stringify({
      run_id: runId,
      producer: failure.producer,
      ticket: failure.ticket,
      camera_revision: failure.cameraRevision,
      order_generation: failure.orderGeneration,
      projection_generation: failure.projectionGeneration,
      reason: failure.reason,
      terminal_at_monotonic_ms: terminalAtMonotonicMs,
    })}`);
    recordGpuProducerTerminalReceipt(
      failure.producer,
      failure.ticket,
      failure.cameraRevision,
      "failure",
      terminalAtMonotonicMs,
    );
  }
}

function recordGpuProducerTerminalReceipt(
  producer,
  ticket,
  revision,
  outcome,
  terminalAtMonotonicMs,
) {
  const benchmark = state.benchmark;
  if (!benchmark) return;
  const submission = benchmark.issuedGpuProducerTickets.get(ticket);
  if (!submission) return;
  if (submission.producer !== producer || submission.revision !== revision) {
    failStrictBenchmarkForOrderEvidence(
      `GPU producer terminal ticket=${ticket} identity mismatch issued=` +
      `${submission.producer}/${submission.revision} terminal=${producer}/${revision}`,
    );
    return;
  }
  if (benchmark.terminalGpuProducerTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(
      `GPU producer ticket=${ticket} produced more than one terminal receipt`,
    );
    return;
  }
  benchmark.terminalGpuProducerTickets.set(ticket, outcome);
  benchmark.gpuProducerTerminalRecords.push({
    producer,
    ticket,
    camera_revision: revision,
    outcome,
    terminal_at_monotonic_ms: terminalAtMonotonicMs,
  });
  globalThis.GSPLAT_GPU_PRODUCER_LEDGER_COMPLETE =
    benchmark.issuedGpuProducerTickets.size > 0
    && benchmark.terminalGpuProducerTickets.size === benchmark.issuedGpuProducerTickets.size;
}

function recordProjectedTerminalReceipt(
  execution,
  orderBackend,
  ticket,
  revision,
  outcome,
  terminalAtMonotonicMs,
) {
  const benchmark = state.benchmark;
  if (!benchmark) return;
  const submission = benchmark.issuedProjectedTickets.get(ticket);
  if (!submission) return;
  if (submission.execution !== execution
      || submission.orderBackend !== orderBackend
      || submission.revision !== revision) {
    failStrictBenchmarkForOrderEvidence(
      `projected terminal ticket=${ticket} identity mismatch issued=` +
      `${submission.execution}/${submission.orderBackend}/${submission.revision} terminal=` +
      `${execution}/${orderBackend}/${revision}`,
    );
    return;
  }
  if (benchmark.terminalProjectedTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(
      `projected ticket=${ticket} produced more than one terminal receipt`,
    );
    return;
  }
  benchmark.terminalProjectedTickets.set(ticket, outcome);
  benchmark.projectedTerminalRecords.push({
    execution,
    order_backend: orderBackend,
    ticket,
    camera_revision: revision,
    outcome,
    terminal_at_monotonic_ms: terminalAtMonotonicMs,
  });
  globalThis.GSPLAT_PROJECTED_LEDGER_COMPLETE =
    benchmark.issuedProjectedTickets.size === benchmark.terminalProjectedTickets.size;
}

function recordOrderTerminalReceipt(backend, ticket, revision, outcome, terminalAtMonotonicMs) {
  const benchmark = state.benchmark;
  if (!benchmark) return;
  const submission = benchmark.issuedOrderTickets.get(ticket);
  if (!submission) return;
  if (submission.backend !== backend || submission.revision !== revision) {
    failStrictBenchmarkForOrderEvidence(
      `terminal ticket=${ticket} backend/revision mismatch issued=${submission.backend}/` +
      `${submission.revision} terminal=${backend}/${revision}`,
    );
    return;
  }
  if (benchmark.terminalOrderTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(`ticket=${ticket} produced more than one terminal receipt`);
    return;
  }
  benchmark.terminalOrderTickets.set(ticket, outcome);
  benchmark.orderTerminalRecords.push({
    actual_backend: backend,
    ticket,
    camera_revision: revision,
    outcome,
    terminal_at_monotonic_ms: terminalAtMonotonicMs,
  });
  globalThis.GSPLAT_ORDER_LEDGER_COMPLETE = benchmark.issuedOrderTickets.size > 0
    && benchmark.terminalOrderTickets.size === benchmark.issuedOrderTickets.size;
  maybeEmitMonotonicOrderingWindow(benchmark);
}

function maybeEmitMonotonicOrderingWindow(benchmark) {
  if (benchmark.enabled || benchmark.monotonicOrderingWindowEmitted) return;
  if (benchmark.terminalQueueThroughput) {
    const evidence = benchmark.terminalQueueThroughput.evidence();
    benchmark.monotonicOrderingWindowEmitted = true;
    console.info(`ORDERING_WINDOW_MONOTONIC_JSON ${JSON.stringify({
      schema: BENCHMARK_SCHEMA,
      record_type: "ordering_window_monotonic",
      terminal_model: "untimed_warmup_boundary_plus_final_measured_current_stats_receipt",
      measured_submit_count: evidence.measured_submit_count,
      measured_terminal_count: evidence.terminal_current_stats_terminal_count,
      monotonic_clock: "performance.now",
      first_measured_input_monotonic_ms:
        evidence.first_measured_input_monotonic_ms,
      first_measured_submit_monotonic_ms:
        evidence.first_measured_submit_monotonic_ms,
      last_measured_submit_monotonic_ms:
        evidence.last_measured_submit_monotonic_ms,
      last_measured_terminal_monotonic_ms:
        evidence.last_measured_terminal_monotonic_ms,
      input_to_first_submit_ms: evidence.input_to_first_submit_ms,
      submit_span_ms: evidence.submit_span_ms,
      terminal_tail_ms: evidence.terminal_tail_ms,
      terminal_window_ms: evidence.terminal_window_ms,
    })}`);
    return;
  }
  const rendererOwned = benchmark.currentStatsSubmissionRecords.length > 0;
  const submissions = rendererOwned
    ? benchmark.currentStatsSubmissionRecords
    : benchmark.orderSubmissionRecords;
  const terminals = rendererOwned
    ? benchmark.currentStatsTerminalRecords
    : benchmark.orderTerminalRecords;
  const measuredSubmissions = submissions.filter(
    (submission) => submission.phase === "measured",
  );
  const terminalTickets = new Set(
    terminals.map((terminal) => terminal.ticket),
  );
  if (measuredSubmissions.some((submission) => !terminalTickets.has(submission.ticket))) return;
  const window = monotonicOrderingWindow({
    submissions,
    terminals,
  });
  benchmark.monotonicOrderingWindowEmitted = true;
  console.info(`ORDERING_WINDOW_MONOTONIC_JSON ${JSON.stringify({
    schema: BENCHMARK_SCHEMA,
    record_type: "ordering_window_monotonic",
    terminal_model: rendererOwned ? "renderer_current_stats" : "legacy_order_measurement",
    measured_submit_count: measuredSubmissions.length,
    measured_terminal_count: measuredSubmissions.length,
    ...window,
  })}`);
}

function failStrictBenchmarkForOrderEvidence(message, failures = null) {
  const benchmark = state.benchmark;
  if (!benchmark) return;
  if (!benchmark.enabled && failures) {
    const matchesIssuedTicket = failures.some(
      (failure) => benchmark.issuedOrderTickets.get(failure.ticket)?.revision === failure.cameraRevision,
    );
    if (!matchesIssuedTicket) return;
  }
  benchmark.enabled = false;
  benchmark.failure = message;
  els.benchmarkStatus.textContent = "failed";
  els.runBenchmark.disabled = false;
  els.benchmarkResult.textContent = `STRICT_ORDER_EVIDENCE_FAILURE ${message}`;
  setStatus(`state=benchmark_failed ${message}`);
  console.error(`STRICT_ORDER_EVIDENCE_FAILURE ${message}`);
}

function failClosedWasmRender(error) {
  const reason = compactMessage(error);
  let drainError = "";
  try {
    const receipts = state.wasmRenderer?.drainOrderMeasurementReceipts();
    emitCpuOrderMeasurements(receipts?.completedCpuOrderMeasurements ?? [], "render_failed");
    emitOrderMeasurements(receipts?.completedOrderMeasurements ?? [], "render_failed");
    emitOrderMeasurementFailures(receipts?.failedOrderMeasurements ?? [], "render_failed");
    emitProjectedMeasurements(receipts?.completedProjectedMeasurements ?? [], "render_failed");
    emitProjectedMeasurementFailures(
      receipts?.failedProjectedMeasurements ?? [],
      "render_failed",
    );
    emitGpuProducerMeasurements(receipts?.completedGpuProducerMeasurements ?? []);
    emitGpuProducerMeasurementFailures(receipts?.failedGpuProducerMeasurements ?? []);
  } catch (drainFailure) {
    drainError = compactMessage(drainFailure);
  }
  state.wasmUnavailableReason = reason;
  state.wasmFatalError = reason;
  // A smoke receipt is only valid while the renderer remains healthy. A later
  // render failure must replace an earlier ready value rather than letting a
  // collector observe a stale success during the next animation turn.
  if (state.currentStatsSmokeEnabled) {
    globalThis.GSPLAT_M4_SMOKE_RESULT = {
      status: "failed",
      stage: "render_frame",
      reason,
    };
    state.currentStatsSmokeCompleted = true;
  }
  if (state.benchmark) state.benchmark.enabled = false;
  els.benchmarkStatus.textContent = "failed";
  els.runBenchmark.disabled = false;
  els.benchmarkResult.textContent = `WASM_RENDER_FAILURE error=${reason}`;
  const drainDetail = drainError ? ` receipt_drain_error=${drainError}` : "";
  setStatus(`state=wasm_render_failed fatal=true fallback=disabled error=${reason}${drainDetail}`);
  console.error(`WASM_RENDER_FAILURE fallback=disabled error=${reason}${drainDetail}`);
}

function renderWasm() {
  if (!state.scene || !state.wasmRenderer) {
    return null;
  }

  state.frameCounter += 1;
  const callStart = performance.now();
  try {
    const benchmark = state.benchmark;
    const exactBenchmark = benchmark?.enabled
      && state.wasmRenderer.rasterPath() === "packed_atlas";
    if (exactBenchmark && !benchmark.terminalQueueThroughput) {
      const schedule = ensureBenchmarkCurrentStatsSchedule(benchmark);
      try {
        schedule.noteDraw(benchmark.animationFrameTimestampMs ?? performance.now());
        if (!schedule.requestOutstanding) {
          schedule.beginRequest({
            nowMs: performance.now(),
            priming: benchmark.primingOrder,
            traceStep: benchmark.currentTraceStep,
          });
          const request = state.wasmRenderer.requestCurrentStats();
          if (request.status !== "requested") {
            throw new Error(
              `renderer current-stats request was ${request.status}: ` +
              `${request.reason ?? "unknown"}`,
            );
          }
        }
      } catch (error) {
        failStrictBenchmarkForOrderEvidence(
          compactMessage(error),
        );
        return null;
      }
    }
    if (state.currentStatsSmokeEnabled && !state.currentStatsSmokeRequested) {
      const request = state.wasmRenderer.requestCurrentStats();
      if (request.status !== "requested") {
        globalThis.GSPLAT_M4_SMOKE_RESULT = {
          status: "failed",
          stage: "current_stats_request",
          reason: request.reason ?? "unknown",
        };
        state.currentStatsSmokeCompleted = true;
      } else {
        state.currentStatsSmokeRequested = true;
      }
    }
    const raw = state.wasmRenderer.renderFrame();
    const callMs = performance.now() - callStart;
    const surfaceWidth = raw.surfaceWidth ?? els.canvas.width;
    const surfaceHeight = raw.surfaceHeight ?? els.canvas.height;
    state.surfaceSizeLabel = `${surfaceWidth}x${surfaceHeight}`;

    const gpuOrderPreparationPending = Boolean(raw.gpuOrderPreparationPending);
    const stats = {
      visible: raw.visibleCount ?? null,
      contributor: null,
      drawn: raw.drawnCount ?? null,
      exactContributorCompaction: raw.exactContributorCompaction ?? null,
      currentStatsSubmission: raw.currentStatsSubmission ?? "not_requested",
      currentStatsTicket: raw.currentStatsTicket ?? null,
      currentStatsPlan: raw.currentStatsPlan ?? null,
      currentStatsSceneGeneration: raw.currentStatsSceneGeneration ?? null,
      currentStatsCameraRevision: raw.currentStatsCameraRevision ?? null,
      currentStatsViewportGeneration: raw.currentStatsViewportGeneration ?? null,
      currentStatsContractGeneration: raw.currentStatsContractGeneration ?? null,
      currentStatsPlanSetGeneration: raw.currentStatsPlanSetGeneration ?? null,
      currentStatsOrderGeneration: raw.currentStatsOrderGeneration ?? null,
      currentStatsRasterGeneration: raw.currentStatsRasterGeneration ?? null,
      currentStatsEncodeAttempt: raw.currentStatsEncodeAttempt ?? null,
      currentStatsPresentationSequence: raw.currentStatsPresentationSequence ?? null,
      preprocessMs: raw.preprocessMs ?? 0,
      sortMs: raw.sortMs ?? 0,
      pipelineMs: (raw.cpuGeometryMs ?? raw.rasterMs ?? 0) + (raw.renderSubmitMs ?? 0),
      frameMs: raw.frameMs ?? callMs,
      callMs,
      framePresented: raw.framePresented !== false,
      gpuOrderPreparationPending,
      rasterExecutionPlan: raw.rasterExecutionPlan ?? "global_quads",
      surfaceWidth,
      surfaceHeight,
      internalRenderWidth: raw.internalRenderWidth ?? null,
      internalRenderHeight: raw.internalRenderHeight ?? null,
      presentedWidth: raw.presentedWidth ?? null,
      presentedHeight: raw.presentedHeight ?? null,
      orderBackend: raw.orderBackend ?? "cpu",
      adaptiveGpuFailure: raw.adaptiveGpuFailure ?? null,
      gpuSortFallback: Boolean(raw.gpuSortFallback),
      refreshSort: Boolean(raw.refreshSort),
      adaptiveState: raw.adaptiveState ?? "disabled",
      projectedPolicy: raw.projectedPolicy ?? "adaptive",
      projectedExecution: raw.projectedExecution ?? "candidate",
      projectedAdaptiveState: raw.projectedAdaptiveState ?? "disabled",
      projectedMeasurementSubmission:
        raw.projectedMeasurementSubmission ?? "not_requested",
      projectedMeasurementTicket: raw.projectedMeasurementTicket ?? null,
      projectedMeasurementExecution: raw.projectedMeasurementExecution ?? null,
      projectedMeasurementUnsampledReason:
        raw.projectedMeasurementUnsampledReason ?? null,
      gpuOrderProducer: raw.gpuOrderProducer ?? null,
      gpuProducerMeasurementSubmission:
        raw.gpuProducerMeasurementSubmission ?? "not_requested",
      gpuProducerMeasurementTicket: raw.gpuProducerMeasurementTicket ?? null,
      gpuProducerMeasurementProducer: raw.gpuProducerMeasurementProducer ?? null,
      gpuProducerMeasurementUnsampledReason:
        raw.gpuProducerMeasurementUnsampledReason ?? null,
      cameraRevision: raw.cameraRevision ?? 0,
      appliedOrderRevision: raw.appliedOrderRevision ?? 0,
      presentedOrderRevisionLag: raw.presentedOrderRevisionLag ?? 0,
      submittedMeasurementTicket: raw.submittedMeasurementTicket ?? null,
      submittedMeasurementBackend: raw.submittedMeasurementBackend ?? null,
      measurementUnsampledReason: raw.measurementUnsampledReason ?? null,
      visibleCountRevision: raw.visibleCountRevision ?? null,
      visibleCountPending: Boolean(raw.visibleCountPending),
      gpuTimestampQueriesEnabled: Boolean(raw.gpuTimestampQueriesEnabled),
      completedMeasurementAvailable: Boolean(raw.completedMeasurementAvailable),
      completedMeasurementTicket: raw.completedMeasurementTicket ?? null,
      completedMeasurementRevision: raw.completedMeasurementRevision ?? null,
      completedMeasurementTimingSource: raw.completedMeasurementTimingSource ?? null,
      gpuPreprocessMs: raw.gpuPreprocessMs ?? null,
      gpuRadixMs: raw.gpuRadixMs ?? null,
      gpuOrderMs: raw.gpuOrderMs ?? null,
      gpuCompleteMs: raw.gpuCompleteMs ?? null,
      gpuTimestampPeriodNs: raw.gpuTimestampPeriodNs ?? null,
      gpuBelowTimestampResolution: raw.gpuBelowTimestampResolution ?? null,
      completedVisibleCount: raw.completedVisibleCount ?? null,
      completedContributorCount: raw.completedContributorCount ?? null,
      completedDrawnCount: raw.completedDrawnCount ?? null,
      completedExactContributorCompaction:
        raw.completedExactContributorCompaction ?? null,
      failedMeasurementAvailable: Boolean(raw.failedMeasurementAvailable),
      failedMeasurementTicket: raw.failedMeasurementTicket ?? null,
      failedMeasurementRevision: raw.failedMeasurementRevision ?? null,
      failedMeasurementReason: raw.failedMeasurementReason ?? null,
      completedProjectedMeasurementAvailable:
        Boolean(raw.completedProjectedMeasurementAvailable),
      completedProjectedMeasurementTicket:
        raw.completedProjectedMeasurementTicket ?? null,
      completedProjectedMeasurementRevision:
        raw.completedProjectedMeasurementRevision ?? null,
      completedProjectedMeasurementExecution:
        raw.completedProjectedMeasurementExecution ?? null,
      completedProjectedMeasurementOrderBackend:
        raw.completedProjectedMeasurementOrderBackend ?? null,
      completedProjectedProjectionGeneration:
        raw.completedProjectedProjectionGeneration ?? null,
      completedProjectedProbeGeneration:
        raw.completedProjectedProbeGeneration ?? null,
      completedProjectedProjectionRebuilt:
        raw.completedProjectedProjectionRebuilt ?? null,
      completedProjectedOrderRefreshed:
        raw.completedProjectedOrderRefreshed ?? null,
      completedProjectedFrameCompleteMs:
        raw.completedProjectedFrameCompleteMs ?? null,
      completedProjectedVisibleCount: raw.completedProjectedVisibleCount ?? null,
      completedProjectedContributorCount:
        raw.completedProjectedContributorCount ?? null,
      completedProjectedDrawnCount: raw.completedProjectedDrawnCount ?? null,
      completedProjectedExactContributorCompaction:
        raw.completedProjectedExactContributorCompaction ?? null,
      failedProjectedMeasurementAvailable:
        Boolean(raw.failedProjectedMeasurementAvailable),
      failedProjectedMeasurementTicket: raw.failedProjectedMeasurementTicket ?? null,
      failedProjectedMeasurementRevision:
        raw.failedProjectedMeasurementRevision ?? null,
      failedProjectedMeasurementExecution:
        raw.failedProjectedMeasurementExecution ?? null,
      failedProjectedMeasurementOrderBackend:
        raw.failedProjectedMeasurementOrderBackend ?? null,
      failedProjectedProjectionGeneration:
        raw.failedProjectedProjectionGeneration ?? null,
      failedProjectedProbeGeneration: raw.failedProjectedProbeGeneration ?? null,
      failedProjectedMeasurementReason: raw.failedProjectedMeasurementReason ?? null,
    };
    state.gpuOrderPreparationPending = stats.gpuOrderPreparationPending;
    const completedMeasurements = Array.isArray(raw.completedOrderMeasurements)
      ? raw.completedOrderMeasurements
      : stats.completedMeasurementAvailable
        ? [{
            ticket: stats.completedMeasurementTicket,
            cameraRevision: stats.completedMeasurementRevision,
            timingSource: stats.completedMeasurementTimingSource,
            gpuPreprocessMs: stats.gpuPreprocessMs,
            gpuRadixMs: stats.gpuRadixMs,
            gpuOrderMs: stats.gpuOrderMs,
            gpuCompleteMs: stats.gpuCompleteMs,
            timestampPeriodNs: stats.gpuTimestampPeriodNs,
            belowTimestampResolution: stats.gpuBelowTimestampResolution,
            visibleCount: stats.completedVisibleCount,
            contributorCount: stats.completedContributorCount,
            drawnCount: stats.completedDrawnCount,
            exactContributorCompaction: stats.completedExactContributorCompaction,
          }]
        : [];
    const completedCpuMeasurements = Array.isArray(raw.completedCpuOrderMeasurements)
      ? raw.completedCpuOrderMeasurements
      : [];
    const failedMeasurements = Array.isArray(raw.failedOrderMeasurements)
      ? raw.failedOrderMeasurements
      : stats.failedMeasurementAvailable
        ? [{
            ticket: stats.failedMeasurementTicket,
            cameraRevision: stats.failedMeasurementRevision,
            reason: stats.failedMeasurementReason,
          }]
        : [];
    const completedProjectedMeasurements = Array.isArray(raw.completedProjectedMeasurements)
      ? raw.completedProjectedMeasurements
      : stats.completedProjectedMeasurementAvailable
        ? [{
            ticket: stats.completedProjectedMeasurementTicket,
            cameraRevision: stats.completedProjectedMeasurementRevision,
            execution: stats.completedProjectedMeasurementExecution,
            orderBackend: stats.completedProjectedMeasurementOrderBackend,
            projectionGeneration: stats.completedProjectedProjectionGeneration,
            probeGeneration: stats.completedProjectedProbeGeneration,
            projectionRebuilt: stats.completedProjectedProjectionRebuilt,
            orderRefreshed: stats.completedProjectedOrderRefreshed,
            frameCompleteMs: stats.completedProjectedFrameCompleteMs,
            visibleCount: stats.completedProjectedVisibleCount,
            contributorCount: stats.completedProjectedContributorCount,
            drawnCount: stats.completedProjectedDrawnCount,
            exactContributorCompaction:
              stats.completedProjectedExactContributorCompaction,
          }]
        : [];
    const failedProjectedMeasurements = Array.isArray(raw.failedProjectedMeasurements)
      ? raw.failedProjectedMeasurements
      : stats.failedProjectedMeasurementAvailable
        ? [{
            ticket: stats.failedProjectedMeasurementTicket,
            cameraRevision: stats.failedProjectedMeasurementRevision,
            execution: stats.failedProjectedMeasurementExecution,
            orderBackend: stats.failedProjectedMeasurementOrderBackend,
            projectionGeneration: stats.failedProjectedProjectionGeneration,
            probeGeneration: stats.failedProjectedProbeGeneration,
            reason: stats.failedProjectedMeasurementReason,
          }]
        : [];
    const completedGpuProducerMeasurements =
      Array.isArray(raw.completedGpuProducerMeasurements)
        ? raw.completedGpuProducerMeasurements
        : [];
    const failedGpuProducerMeasurements =
      Array.isArray(raw.failedGpuProducerMeasurements)
        ? raw.failedGpuProducerMeasurements
        : [];
    emitCpuOrderMeasurements(completedCpuMeasurements, stats.adaptiveState);
    emitOrderMeasurements(completedMeasurements, stats.adaptiveState);
    emitOrderMeasurementFailures(failedMeasurements, stats.adaptiveState);
    emitProjectedMeasurements(completedProjectedMeasurements, stats.projectedAdaptiveState);
    emitProjectedMeasurementFailures(
      failedProjectedMeasurements,
      stats.projectedAdaptiveState,
    );
    emitGpuProducerMeasurements(completedGpuProducerMeasurements);
    emitGpuProducerMeasurementFailures(failedGpuProducerMeasurements);
    if (stats.adaptiveGpuFailure) {
      console.error(`ADAPTIVE_GPU_FAILURE_JSON ${JSON.stringify({
        requested_backend: state.requestedOrderBackend,
        camera_revision: stats.cameraRevision,
        reason: stats.adaptiveGpuFailure,
      })}`);
    }
    if (failedProjectedMeasurements.length > 0) {
      const failure = failedProjectedMeasurements[0];
      failStrictBenchmarkForOrderEvidence(
        `projected ticket=${failure.ticket} revision=${failure.cameraRevision} ` +
        `reason=${failure.reason}`,
      );
    }
    if (failedGpuProducerMeasurements.length > 0) {
      const failure = failedGpuProducerMeasurements[0];
      failStrictBenchmarkForOrderEvidence(
        `GPU producer ticket=${failure.ticket} revision=${failure.cameraRevision} ` +
        `reason=${failure.reason}`,
      );
    }
    updateFrameStats(stats);
    updateStatusOverlay(stats);
    publishCurrentStatsSmoke(raw);
    if (failedMeasurements.length > 0) {
      const failure = failedMeasurements[0];
      failStrictBenchmarkForOrderEvidence(
        `ticket=${failure.ticket} revision=${failure.cameraRevision} reason=${failure.reason}`,
        failedMeasurements,
      );
    }
    if (stats.adaptiveGpuFailure) {
      failStrictBenchmarkForOrderEvidence(
        `adaptive_gpu_failure=${stats.adaptiveGpuFailure} revision=${stats.cameraRevision}`,
      );
    }
    if (state.benchmark?.failure) {
      setStatus(`state=benchmark_failed ${state.benchmark.failure}`);
    }
    return stats;
  } catch (error) {
    // Once an exact-count WASM scene exists, a render failure is terminal for
    // that scene. Keep its handle alive so terminal telemetry can still drain;
    // silently switching to the sampled WebGL preview would falsify quality.
    failClosedWasmRender(error);
    return null;
  }
}

function publishCurrentStatsSmoke(frame) {
  if (!state.currentStatsSmokeEnabled || state.currentStatsSmokeCompleted) return;
  if (frame.currentStatsSubmission === "issued") {
    state.currentStatsSmokeFrame = {
      frame,
      cameraReceipt: state.wasmRenderer.cameraReceipt(),
    };
  }
  const terminal = state.wasmRenderer.pollCurrentStats();
  if (terminal.status === "empty") return;
  if (terminal.status !== "ready") {
    globalThis.GSPLAT_M4_SMOKE_RESULT = {
      status: "failed",
      stage: "current_stats_poll",
      reason: terminal.reason ?? terminal.status,
      terminal,
    };
    state.currentStatsSmokeCompleted = true;
    return;
  }
  const issued = state.currentStatsSmokeFrame;
  if (!issued) {
    globalThis.GSPLAT_M4_SMOKE_RESULT = {
      status: "failed",
      stage: "current_stats_identity",
      reason: "ready terminal has no issued frame",
      terminal,
    };
    state.currentStatsSmokeCompleted = true;
    return;
  }
  const issuedFrame = issued.frame;
  const result = {
    status: "ready",
    backend: state.backend,
    sampled_webgl_enabled: state.sampledWebglEnabled,
    scene: state.wasmRenderer.sceneSummary(),
    load_receipt: state.wasmRenderer.loadReceipt(),
    camera_receipt: issued.cameraReceipt,
    surface: state.wasmRenderer.surfaceSize(),
    frame: {
      frame_presented: issuedFrame.framePresented,
      surface_width: issuedFrame.surfaceWidth,
      surface_height: issuedFrame.surfaceHeight,
      internal_render_width: issuedFrame.internalRenderWidth,
      internal_render_height: issuedFrame.internalRenderHeight,
      presented_width: issuedFrame.presentedWidth,
      presented_height: issuedFrame.presentedHeight,
      camera_revision: issuedFrame.cameraRevision,
      current_stats_submission: issuedFrame.currentStatsSubmission,
      current_stats_ticket: issuedFrame.currentStatsTicket,
      current_stats_plan: issuedFrame.currentStatsPlan,
      current_stats_camera_revision: issuedFrame.currentStatsCameraRevision,
      current_stats_presentation_sequence: issuedFrame.currentStatsPresentationSequence,
    },
    current_stats: terminal,
  };
  globalThis.GSPLAT_M4_SMOKE_RESULT = result;
  state.currentStatsSmokeCompleted = true;
  console.info(`M4_WEBGPU_SMOKE_JSON ${JSON.stringify(result)}`);
}

function renderWebgl() {
  const gl = state.gl;
  if (!gl || !state.program) {
    return null;
  }
  gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
  gl.clear(gl.COLOR_BUFFER_BIT);

  if (!state.scene) {
    return null;
  }

  state.frameCounter += 1;
  const frameStart = performance.now();
  const camera = buildCameraBasis();
  const orderInfo = prepareDrawOrder(camera);

  const uploadStart = performance.now();
  const buffer = fillDrawBuffer(orderInfo.order);
  gl.bindBuffer(gl.ARRAY_BUFFER, state.buffer);
  gl.bufferData(gl.ARRAY_BUFFER, buffer, gl.DYNAMIC_DRAW);
  const pipelineMs = performance.now() - uploadStart;

  gl.useProgram(state.program);
  setUniforms(gl, state.program, camera);
  gl.drawArrays(gl.POINTS, 0, orderInfo.order.length);

  const frameMs = performance.now() - frameStart;
  const stats = {
    visible: orderInfo.visible,
    drawn: orderInfo.order.length,
    preprocessMs: orderInfo.preprocessMs,
    sortMs: orderInfo.sortMs,
    pipelineMs,
    frameMs,
    callMs: frameMs,
    orderBackend: "cpu",
    gpuSortFallback: false,
    framePresented: true,
    gpuOrderPreparationPending: false,
    rasterExecutionPlan: "global_quads",
    projectedPolicy: state.requestedProjectedPolicy,
    projectedExecution: "candidate",
    projectedAdaptiveState: "disabled",
    projectedMeasurementSubmission: "not_requested",
    projectedMeasurementTicket: null,
    projectedMeasurementExecution: null,
    projectedMeasurementUnsampledReason: null,
    surfaceWidth: gl.drawingBufferWidth,
    surfaceHeight: gl.drawingBufferHeight,
    internalRenderWidth: gl.drawingBufferWidth,
    internalRenderHeight: gl.drawingBufferHeight,
    presentedWidth: gl.drawingBufferWidth,
    presentedHeight: gl.drawingBufferHeight,
  };
  updateFrameStats(stats);
  updateStatusOverlay(stats);
  return stats;
}

function prepareDrawOrder(camera) {
  const interval = Number(els.sortInterval.value);
  const refreshSort = state.lastSortedOrder.length === 0 || state.sortFrameCounter % interval === 0;
  state.sortFrameCounter += 1;

  if (!refreshSort) {
    return {
      order: state.lastSortedOrder,
      visible: state.lastSortedOrder.length,
      preprocessMs: 0,
      sortMs: 0,
    };
  }

  const prepStart = performance.now();
  const visible = collectVisible(camera);
  const preprocessMs = performance.now() - prepStart;

  const sortStart = performance.now();
  visible.sort((a, b) => state.scratchDepths[b] - state.scratchDepths[a]);
  const sortMs = performance.now() - sortStart;
  state.lastSortedOrder = visible;

  return {
    order: visible,
    visible: visible.length,
    preprocessMs,
    sortMs,
  };
}

function collectVisible(camera) {
  const scene = state.scene;
  const budget = Number(els.drawBudget.value);
  const step = Math.max(1, Math.ceil(scene.count / budget));
  const visible = [];
  const positions = scene.positions;
  const depths = state.scratchDepths;
  const maxSelected = Math.ceil(scene.count / step);

  for (let i = 0; i < scene.count; i += step) {
    const p = i * 3;
    const rx = positions[p] - camera.eye[0];
    const ry = positions[p + 1] - camera.eye[1];
    const rz = positions[p + 2] - camera.eye[2];
    const depth = rx * camera.forward[0] + ry * camera.forward[1] + rz * camera.forward[2];
    if (depth >= state.camera.near && depth <= state.camera.far) {
      depths[i] = depth;
      visible.push(i);
    }
  }

  if (visible.length > maxSelected) {
    visible.length = maxSelected;
  }
  return visible;
}

function fillDrawBuffer(order) {
  const needed = order.length * 8;
  if (state.drawBuffer.length < needed) {
    state.drawBuffer = new Float32Array(needed);
  }
  const out = state.drawBuffer.subarray(0, needed);
  const scene = state.scene;

  for (let outIndex = 0; outIndex < order.length; outIndex += 1) {
    const sceneIndex = order[outIndex];
    const src = sceneIndex * 3;
    const dst = outIndex * 8;
    out[dst] = scene.positions[src];
    out[dst + 1] = scene.positions[src + 1];
    out[dst + 2] = scene.positions[src + 2];
    out[dst + 3] = scene.colors[src];
    out[dst + 4] = scene.colors[src + 1];
    out[dst + 5] = scene.colors[src + 2];
    out[dst + 6] = scene.alphas[sceneIndex];
    out[dst + 7] = scene.radii[sceneIndex];
  }

  return out;
}

function buildCameraBasis() {
  if (state.qualificationCamera) {
    return cameraBasisFromTraceFrame(state.qualificationCamera);
  }
  const camera = state.camera;
  const cosPitch = Math.cos(camera.pitch);
  const eye = [
    camera.target[0] + Math.sin(camera.yaw) * cosPitch * camera.distance,
    camera.target[1] + Math.sin(camera.pitch) * camera.distance,
    camera.target[2] - Math.cos(camera.yaw) * cosPitch * camera.distance,
  ];
  const forward = normalize([
    camera.target[0] - eye[0],
    camera.target[1] - eye[1],
    camera.target[2] - eye[2],
  ]);
  let right = normalize(cross([0, 1, 0], forward));
  if (!Number.isFinite(right[0])) {
    right = [1, 0, 0];
  }
  const up = normalize(cross(forward, right));
  return { eye, forward, right, up };
}

function setUniforms(gl, program, camera) {
  const aspect = gl.drawingBufferWidth / Math.max(gl.drawingBufferHeight, 1);
  const f = 1 / Math.tan(state.camera.fovY * 0.5);
  gl.uniform3fv(gl.getUniformLocation(program, "uEye"), camera.eye);
  gl.uniform3fv(gl.getUniformLocation(program, "uRight"), camera.right);
  gl.uniform3fv(gl.getUniformLocation(program, "uUp"), camera.up);
  gl.uniform3fv(gl.getUniformLocation(program, "uForward"), camera.forward);
  gl.uniform1f(gl.getUniformLocation(program, "uF"), f);
  gl.uniform1f(gl.getUniformLocation(program, "uAspect"), aspect);
  gl.uniform1f(gl.getUniformLocation(program, "uFocalPixels"), gl.drawingBufferHeight * 0.5 * f);
  gl.uniform1f(gl.getUniformLocation(program, "uPointScale"), Number(els.pointScale.value));
  gl.uniform1f(gl.getUniformLocation(program, "uNear"), state.camera.near);
  gl.uniform1f(gl.getUniformLocation(program, "uFar"), state.camera.far);
}

function scheduleCanvasResize() {
  state.resizeMeasurePending = true;
  if (resizeMeasureFrame !== null) return;
  resizeMeasureFrame = requestAnimationFrame(() => {
    resizeMeasureFrame = null;
    try {
      resizeCanvas();
    } finally {
      state.resizeMeasurePending = false;
    }
  });
}

function resizeCanvas() {
  const fixedDisplay = state.qualificationTrace?.display;
  const ratio = fixedDisplay ? 1 : (window.devicePixelRatio || 1);
  const width = fixedDisplay?.width ?? Math.max(1, Math.floor(els.canvas.clientWidth * ratio));
  const height = fixedDisplay?.height ?? Math.max(1, Math.floor(els.canvas.clientHeight * ratio));

  if (els.canvas.width !== width || els.canvas.height !== height) {
    if (usingWasm()) {
      requestWasmRendererResize(width, height);
    } else {
      // Before a WASM renderer exists this establishes its constructor size;
      // WebGL likewise owns its backing store directly. An active WASM Surface
      // is changed only by the awaited transaction above.
      els.canvas.width = width;
      els.canvas.height = height;
      state.surfaceSizeLabel = `${width}x${height}`;
      invalidateSortedOrder();
    }
  }
  els.surfaceSize.textContent = state.surfaceSizeLabel;
  if (state.scene && state.frameCounter === 0 && !wasmResizeCoordinator.pending) {
    fitCameraToScene(state.scene);
  }
}

function startBenchmark() {
  if (state.resizeMeasurePending || wasmResizeCoordinator.pending) {
    setStatus("state=benchmark_waiting_for_resize");
    els.benchmarkResult.textContent = "Waiting for the pending surface resize...";
    return;
  }
  if (!state.scene) {
    setStatus("state=benchmark_waiting_for_scene");
    return;
  }

  if (!usingWasm() && (state.strictBenchmarkMode || state.qualificationTrace)) {
    setStatus("state=benchmark_error exact_wasm_required=true unavailable_on=webgl2");
    els.benchmarkStatus.textContent = "failed";
    return;
  }
  if (!usingWasm() && state.requestedOrderBackend !== "cpu") {
    setStatus(`state=benchmark_error requested_backend=${state.requestedOrderBackend} unavailable_on=webgl2`);
    return;
  }
  if (state.wasmFatalError) {
    setStatus(`state=benchmark_error renderer_failed=${state.wasmFatalError}`);
    els.benchmarkStatus.textContent = "failed";
    return;
  }
  const benchmark = createBenchmarkState(true);
  if (state.qualificationCamera && !benchmark.traceSequence) {
    // The qualification camera is installed while the streamed scene is
    // adopted. Re-enter it from the renderer's scene camera so a fixed-frame
    // run starts from an explicit post-publication camera revision instead of
    // depending on constructor dirtiness or a previously cached order.
    state.wasmRenderer?.resetCamera();
    fitCameraToScene(state.scene);
  }
  state.benchmark = benchmark;
  setAutoOrbit(false);
  els.benchmarkStatus.textContent = "running";
  els.runBenchmark.disabled = true;
  els.benchmarkResult.textContent = benchmark.traceSequence
    ? "Running camera-trace sequence benchmark..."
    : state.qualificationCamera
    ? "Running fixed-camera benchmark..."
    : "Running benchmark orbit...";
  setStatus(`state=benchmark_running frames=${benchmark.frames} warmup=${benchmark.warmupFrames}`);
}

function runBenchmarkSync() {
  if (state.resizeMeasurePending || wasmResizeCoordinator.pending) {
    setStatus("state=benchmark_waiting_for_resize");
    els.benchmarkResult.textContent = "Waiting for the pending surface resize...";
    return;
  }
  if (!state.scene) {
    setStatus("state=benchmark_waiting_for_scene");
    return;
  }

  if (!usingWasm() && state.requestedOrderBackend !== "cpu") {
    setStatus(`state=benchmark_error requested_backend=${state.requestedOrderBackend} unavailable_on=webgl2`);
    return;
  }
  if (state.wasmFatalError) {
    setStatus(`state=benchmark_error renderer_failed=${state.wasmFatalError}`);
    els.benchmarkStatus.textContent = "failed";
    return;
  }
  // WebGPU completion, map, and timestamp-query callbacks are delivered only
  // after JavaScript yields back to the browser. A tight synchronous loop can
  // submit every measured frame before a single exact visible-count/timing
  // receipt is observable, producing a misleading all-zero artifact. Exact
  // All WASM benchmark modes use the requestAnimationFrame event loop.
  if (usingWasm()) {
    startBenchmark();
    return;
  }
  const benchmark = createBenchmarkState(false);
  if (state.qualificationCamera && !benchmark.traceSequence) fitCameraToScene(state.scene);
  setAutoOrbit(false);
  els.benchmarkStatus.textContent = "running";
  els.runBenchmark.disabled = true;
  els.benchmarkResult.textContent = benchmark.traceSequence
    ? "Running camera-trace sequence sync benchmark..."
    : state.qualificationCamera
    ? "Running fixed-camera sync benchmark..."
    : "Running sync benchmark orbit...";

  const totalFrames = benchmark.frames + benchmark.warmupFrames;
  for (let i = 0; i < totalFrames; i += 1) {
    if (benchmark.traceSequence) {
      applyBenchmarkTraceStep(benchmark);
    } else if (benchmark.yawStep !== 0) {
      orbitCamera(benchmark.yawStep, 0);
      state.cameraStatus = "camera=benchmark_orbit";
    }
    const stats = render();
    if (state.wasmFatalError) return;
    benchmark.observedFrames += 1;
    if (stats && i >= benchmark.warmupFrames) {
      accumulateBenchmark(benchmark, stats, undefined, undefined, benchmark.currentTraceStep);
    }
  }

  finishBenchmark(benchmark);
  state.benchmark = null;
}

function createBenchmarkState(enabled) {
  const traceSequence = state.qualificationTraceSequenceEnabled
    ? state.qualificationTraceSequence
    : null;
  const frames = traceSequence
    ? traceSequence.measuredSampleCount
    : clampInt(Number(els.benchmarkFrames.value), 1, 5000, DEFAULT_BENCHMARK_FRAMES);
  const warmupFrames = traceSequence
    ? traceSequence.warmupFrames
    : clampInt(Number(els.benchmarkWarmup.value), 0, 500, DEFAULT_BENCHMARK_WARMUP_FRAMES);
  const yawStep = state.qualificationTrace
    ? 0
    : finiteOrDefault(Number(els.benchmarkYaw.value), DEFAULT_BENCHMARK_YAW_STEP);
  const startedAt = new Date().toISOString();
  const runId = globalThis.crypto?.randomUUID?.() ?? `web-${Date.now()}-${Math.trunc(performance.now() * 1000)}`;
  globalThis.GSPLAT_ORDER_LEDGER_COMPLETE = false;
  globalThis.GSPLAT_PROJECTED_LEDGER_COMPLETE = true;
  globalThis.GSPLAT_GPU_PRODUCER_LEDGER_COMPLETE =
    state.requestedGpuOrderProducer === null;
  const terminalQueueThroughput = state.benchmarkWindowMode
    === BENCHMARK_WINDOW_MODES.terminalQueueThroughput
    ? createTerminalQueueThroughputWindow({
        warmupFrames,
        measuredFrames: frames,
        configurationSha256:
          state.currentStatsControlArtifactIdentity.configuration_sha256,
        controlArtifactIdentity: state.currentStatsControlArtifactIdentity,
        executionCell: state.requestedOrderBackend === "gpu"
          && state.requestedProjectedPolicy === "compact"
          && state.requestedGpuOrderProducer === null
          ? "fixed_gpu_preproject_compact"
          : "adaptive",
        directQueueCompletion: state.q1QueueTerminalEnabled,
      })
    : null;
  const q1Capture = state.q1CaptureTraceFrameIndex === null ? null : {
    traceFrameIndex: state.q1CaptureTraceFrameIndex,
    measuredFrameIndex: q1CaptureMeasuredFrame(state.q1CaptureTraceFrameIndex),
    armPending: false,
    armed: false,
    takePending: false,
    ready: null,
  };
  return {
    enabled,
    frameWallSource: terminalQueueThroughput
      ? "request_animation_frame_interval_and_first_input_to_final_terminal_window"
      : enabled
      ? state.orderCompletionProtocol === "isolated_terminal"
        ? "isolated_terminal_progression"
        : "request_animation_frame_interval"
      : "synchronous_renderer_reported_frame_wall",
    frames,
    warmupFrames,
    yawStep,
    traceSequence,
    orderCompletionProtocol: state.orderCompletionProtocol,
    fixedCameraPrimePending: state.qualificationTrace != null
      && traceSequence == null
      && state.requestedOrderBackend !== "cpu"
      && state.qualificationTrace.frames.length > 1,
    fixedCameraApplyPending: state.qualificationTrace != null && traceSequence == null,
    primingOrder: false,
    currentTraceStep: null,
    frameReceipts: [],
    requestedWidth: els.canvas.width,
    requestedHeight: els.canvas.height,
    issuedOrderTickets: new Map(),
    terminalOrderTickets: new Map(),
    issuedAuxiliaryFormalTickets: new Map(),
    terminalAuxiliaryFormalTickets: new Map(),
    auxiliaryFormalSubmissionRecords: [],
    auxiliaryFormalTerminalRecords: [],
    orderSubmissionRecords: [],
    orderTerminalRecords: [],
    issuedCurrentStatsTickets: new Map(),
    terminalCurrentStatsTickets: new Map(),
    currentStatsSubmissionRecords: [],
    currentStatsTerminalRecords: [],
    issuedProjectedTickets: new Map(),
    terminalProjectedTickets: new Map(),
    projectedSubmissionRecords: [],
    projectedTerminalRecords: [],
    issuedGpuProducerTickets: new Map(),
    terminalGpuProducerTickets: new Map(),
    gpuProducerSubmissionRecords: [],
    gpuProducerTerminalRecords: [],
    monotonicOrderingWindowEmitted: false,
    pendingOrderSample: null,
    currentStatsSchedule: null,
    terminalQueueThroughput,
    q1Capture,
    terminalCurrentStatsRequestOutstanding: false,
    terminalCurrentStatsPending: null,
    terminalCurrentStatsObserved: null,
    pendingProjectedSample: null,
    pendingGpuProducerSample: null,
    presentedSubmissionCount: 0,
    terminalPollCount: 0,
    failure: null,
    observedFrames: 0,
    presentationPending: false,
    animationFrameTimestampMs: null,
    measuredStartMs: null,
    lastObservedFrameMs: null,
    startedAt,
    measurementStartedAt: null,
    measurementEndedAt: null,
    collector: createBenchmarkCollector({ runId, warmupCount: warmupFrames, frameBudgetMs: DEFAULT_FRAME_BUDGET_MS }),
  };
}

function ensureBenchmarkCurrentStatsSchedule(benchmark) {
  if (benchmark.currentStatsSchedule === null) {
    benchmark.currentStatsSchedule = createCurrentStatsSchedule({
      protocol: benchmark.orderCompletionProtocol,
      warmupFrames: benchmark.warmupFrames,
      measuredFrames: benchmark.frames,
    });
    benchmark.frameWallSource = benchmark.currentStatsSchedule.frameWallSource;
  }
  return benchmark.currentStatsSchedule;
}

function applyBenchmarkTraceStep(benchmark) {
  const submissionIndex = benchmark.currentStatsSchedule?.nextSubmissionIndex
    ?? benchmark.observedFrames;
  const step = benchmark.traceSequence.step(submissionIndex);
  benchmark.currentTraceStep = step;
  state.qualificationCamera = step.camera;
  state.camera.fovY = step.camera.intrinsics.verticalFovRadians;
  state.camera.near = step.camera.intrinsics.nearPlane;
  state.camera.far = step.camera.intrinsics.farPlane;
  state.wasmRenderer?.setCamera(step.camera);
  invalidateSortedOrder();
  state.cameraStatus = `camera=trace_sequence frame=${step.traceFrameIndex}`;
  console.info(
    `CAMERA_TRACE_FRAME trace_id=${state.qualificationTrace.trace_id} ` +
    `trace_sha256=${state.qualificationTrace.content_sha256} phase=${step.phase} ` +
    `loop=${step.loopIndex} phase_frame=${step.phaseFrameIndex} ` +
    `frame_index=${step.traceFrameIndex} timestamp_ns=${step.timestampNs} ` +
    `requested_backend=${state.requestedOrderBackend}`,
  );
}

function applyUrlConfig() {
  const params = new URLSearchParams(window.location.search);
  state.sampledWebglEnabled = sampledWebglOptIn(params);
  state.currentStatsSmokeEnabled = ["1", "true", "yes"].includes(
    (params.get("gsplat_current_stats_smoke") ?? "").toLowerCase(),
  );
  if (state.currentStatsSmokeEnabled) state.autoOrbit = false;
  const benchmarkFramesParam = params.get("gsplat_benchmark_frames") ?? params.get("benchmark_frames");
  const benchmarkWarmupParam = params.get("gsplat_benchmark_warmup_frames") ?? params.get("benchmark_warmup");
  const sortIntervalParam = params.get("gsplat_surface_sort_interval") ?? params.get("sort_interval");
  setNumberInputFromParam(els.benchmarkFrames, benchmarkFramesParam);
  setNumberInputFromParam(
    els.benchmarkWarmup,
    benchmarkWarmupParam,
  );
  setNumberInputFromParam(els.benchmarkYaw, params.get("gsplat_benchmark_yaw_step") ?? params.get("benchmark_yaw_step"));
  setNumberInputFromParam(els.sortInterval, sortIntervalParam);
  setNumberInputFromParam(els.drawBudget, params.get("draw_budget"));
  const dataset = (params.get("dataset") ?? params.get("scene") ?? "").toLowerCase();
  const geometryPath = (params.get("gsplat_geometry_path") ?? "packed").toLowerCase();
  if (["direct", "packed", "paged"].includes(geometryPath)) {
    state.geometryPath = geometryPath;
  }
  if (dataset === "flowers" || dataset === "flower") {
    state.startDataset = "flowers";
  } else if (["bonsai", "truck", "garden", "bicycle"].includes(dataset)) {
    state.startDataset = dataset;
  } else if (/^truck-(?:50000|100000|200000|300000|500000|1000000|1500000|2000000)$/.test(dataset)) {
    state.startDataset = dataset;
  } else if (dataset === "minimal" || dataset === "smoke") {
    state.startDataset = "minimal";
  } else if (dataset === "diagnostic" || dataset === "raster_diagnostic_v1") {
    state.startDataset = "diagnostic";
  } else if (["showcase", "kitsune", "kitune", "fox"].includes(dataset)) {
    state.startDataset = "showcase";
  }
  state.autoStartBenchmark = ["1", "true", "yes"].includes(
    (params.get("gsplat_benchmark") ?? params.get("benchmark") ?? "").toLowerCase(),
  );
  state.strictBenchmarkMode = state.autoStartBenchmark;
  state.autoBenchmarkSync = ["1", "true", "yes"].includes(
    (params.get("gsplat_benchmark_sync") ?? params.get("benchmark_sync") ?? "").toLowerCase(),
  );
  const trace = params.get("gsplat_camera_trace");
  const traceUrl = params.get("gsplat_camera_trace_url");
  state.qualificationTraceSequenceEnabled = ["1", "true", "yes"].includes(
    (params.get("gsplat_camera_trace_sequence") ?? "").toLowerCase(),
  );
  const frameIndicesText = params.get("gsplat_camera_frame_indices");
  state.qualificationTraceFrameIndices = frameIndicesText == null
    ? null
    : parseTraceFrameIndices(frameIndicesText);
  const traceWarmup = params.get("gsplat_camera_trace_warmup_frames") ?? benchmarkWarmupParam;
  const traceMeasured = params.get("gsplat_camera_trace_measured_frames") ?? benchmarkFramesParam;
  state.qualificationTraceWarmupFrames = traceWarmup == null
    ? null
    : clampInt(Number(traceWarmup), 0, 5000, 0);
  state.qualificationTraceMeasuredFrames = traceMeasured == null
    ? null
    : clampInt(Number(traceMeasured), 1, 5000, 1);
  state.qualificationTraceLoops = clampInt(
    Number(params.get("gsplat_camera_trace_loops") ?? 1),
    1,
    1000,
    1,
  );
  const requestedBackend = (params.get("gsplat_surface_order_backend") ?? "adaptive").toLowerCase();
  if (!["cpu", "gpu", "adaptive"].includes(requestedBackend)) {
    throw new TypeError("gsplat_surface_order_backend must be cpu, gpu, or adaptive");
  }
  state.requestedOrderBackend = requestedBackend;
  const requestedProjectedPolicy = (
    params.get("gsplat_surface_projected_policy") ?? "adaptive"
  ).toLowerCase();
  if (!["candidate", "compact", "adaptive"].includes(requestedProjectedPolicy)) {
    throw new TypeError(
      "gsplat_surface_projected_policy must be candidate, compact, or adaptive",
    );
  }
  state.requestedProjectedPolicy = requestedProjectedPolicy;
  const requestedGpuOrderProducerText = params.get("gsplat_surface_gpu_order_producer");
  if (requestedGpuOrderProducerText !== null) {
    const requestedGpuOrderProducer = requestedGpuOrderProducerText.toLowerCase();
    if (!["post-sort", "preproject"].includes(requestedGpuOrderProducer)) {
      throw new TypeError(
        "gsplat_surface_gpu_order_producer must be post-sort or preproject",
      );
    }
    state.requestedGpuOrderProducer = requestedGpuOrderProducer;
  }
  const orderCompletionProtocol = (
    params.get("gsplat_order_completion_protocol") ?? "isolated_terminal"
  ).toLowerCase();
  if (!["isolated_terminal", "sustained_window"].includes(orderCompletionProtocol)) {
    throw new TypeError(
      "gsplat_order_completion_protocol must be isolated_terminal or sustained_window",
    );
  }
  state.orderCompletionProtocol = orderCompletionProtocol;
  const benchmarkWindowMode = (
    params.get("gsplat_benchmark_window_mode")
      ?? BENCHMARK_WINDOW_MODES.currentStatsEvidence
  ).toLowerCase();
  if (!Object.values(BENCHMARK_WINDOW_MODES).includes(benchmarkWindowMode)) {
    throw new TypeError(`unknown gsplat_benchmark_window_mode ${benchmarkWindowMode}`);
  }
  state.benchmarkWindowMode = benchmarkWindowMode;
  if (benchmarkWindowMode === BENCHMARK_WINDOW_MODES.terminalQueueThroughput) {
    state.currentStatsControlArtifactIdentity = currentStatsEvidenceWindowIdentity({
      runId: params.get("gsplat_current_stats_control_run_id"),
      configurationSha256: params.get("gsplat_current_stats_control_configuration_sha256"),
    });
    const adaptiveCell = state.requestedOrderBackend === "adaptive"
      && state.requestedProjectedPolicy === "adaptive"
      && state.requestedGpuOrderProducer === null;
    const fixedGpuCompactCell = state.requestedOrderBackend === "gpu"
      && state.requestedProjectedPolicy === "compact"
      && state.requestedGpuOrderProducer === null;
    if (!state.strictBenchmarkMode
        || state.autoBenchmarkSync
        || state.geometryPath !== "packed"
        || (!adaptiveCell && !fixedGpuCompactCell)
        || state.orderCompletionProtocol !== "sustained_window") {
      throw new TypeError(
        "terminal-queue throughput requires an admitted strict async Packed Exact sustained_window cell",
      );
    }
  }
  if (state.requestedGpuOrderProducer !== null) {
    if (!state.strictBenchmarkMode
        || state.autoBenchmarkSync
        || state.geometryPath !== "packed"
        || state.requestedOrderBackend !== "gpu"
        || state.requestedProjectedPolicy !== "compact"
        || state.orderCompletionProtocol !== "isolated_terminal"
        || Number(els.sortInterval.value) !== 1) {
      throw new TypeError(
        "GPU producer qualification requires strict benchmark mode, Packed geometry, " +
        "forced GPU ordering, forced Compact projected drawing, asynchronous isolated " +
        "terminals, and sort interval 1",
      );
    }
  }
  const q1CaptureText = params.get("gsplat_q1_capture_trace_frame");
  if (q1CaptureText !== null) {
    const traceFrameIndex = Number(q1CaptureText);
    q1CaptureMeasuredFrame(traceFrameIndex);
    if (!state.strictBenchmarkMode || state.autoBenchmarkSync
        || benchmarkWindowMode !== BENCHMARK_WINDOW_MODES.currentStatsEvidence
        || state.geometryPath !== "packed"
        || state.requestedOrderBackend !== "gpu"
        || state.requestedProjectedPolicy !== "compact"
        || state.requestedGpuOrderProducer !== null
        || state.orderCompletionProtocol !== "isolated_terminal") {
      throw new TypeError(
        "Q1 control capture requires strict async Packed GPU Preproject Compact current-stats control",
      );
    }
    state.q1CaptureTraceFrameIndex = traceFrameIndex;
  }
  const q1QueueTerminalText = params.get("gsplat_q1_queue_terminal");
  if (q1QueueTerminalText !== null) {
    const fixedGpuCompactCell = state.requestedOrderBackend === "gpu"
      && state.requestedProjectedPolicy === "compact"
      && state.requestedGpuOrderProducer === null;
    if (q1QueueTerminalText !== "1"
        || !state.strictBenchmarkMode
        || state.autoBenchmarkSync
        || benchmarkWindowMode !== BENCHMARK_WINDOW_MODES.terminalQueueThroughput
        || state.geometryPath !== "packed"
        || !fixedGpuCompactCell
        || state.orderCompletionProtocol !== "sustained_window") {
      throw new TypeError(
        "Q1 queue terminal requires strict async Packed GPU Preproject Compact throughput",
      );
    }
    state.q1QueueTerminalEnabled = true;
  }
  if (state.qualificationTraceSequenceEnabled) {
    if (params.has("gsplat_camera_frame")) {
      throw new TypeError("gsplat_camera_frame cannot be combined with gsplat_camera_trace_sequence");
    }
    if (sortIntervalParam != null && Number(sortIntervalParam) !== 1) {
      throw new TypeError("camera trace sequence requires gsplat_surface_sort_interval=1");
    }
    els.sortInterval.value = "1";
  } else if (frameIndicesText != null || params.has("gsplat_camera_trace_loops")
      || params.has("gsplat_camera_trace_warmup_frames")
      || params.has("gsplat_camera_trace_measured_frames")) {
    throw new TypeError("camera trace sequence parameters require gsplat_camera_trace_sequence=true");
  }
  state.qualificationTraceFrameIndex = clampInt(
    Number(params.get("gsplat_camera_frame") ?? 0),
    0,
    Number.MAX_SAFE_INTEGER,
    0,
  );
  if (trace === "phase-e-kitsune-static-v1") {
    state.qualificationTraceUrl = "/tests/perf/trace/fixtures/phase-e-kitsune-static-640x480-v1.json";
  } else if (trace === "phase-e-minimal-static-v1") {
    state.qualificationTraceUrl = "/tests/perf/trace/fixtures/phase-e-minimal-static-640x480-v1.json";
  } else if (trace === "phase-e-raster-diagnostic-v1") {
    state.qualificationTraceUrl = "/tests/perf/trace/fixtures/phase-e-minimal-static-640x480-v1.json";
  } else if (trace && /^(?:https?:\/\/|\/|\.\/|\.\.\/)/.test(trace)) {
    state.qualificationTraceUrl = trace;
  }
  if (traceUrl) state.qualificationTraceUrl = traceUrl;
}

function parseTraceFrameIndices(value) {
  if (value.trim() === "") throw new TypeError("gsplat_camera_frame_indices must not be empty");
  return value.split(",").map((part) => {
    const index = Number(part.trim());
    if (!Number.isSafeInteger(index) || index < 0) {
      throw new TypeError("gsplat_camera_frame_indices must contain non-negative integers");
    }
    return index;
  });
}

function setNumberInputFromParam(input, value) {
  if (value === null || value === undefined || value === "") {
    return;
  }
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return;
  }
  input.value = String(number);
}

function recordBenchmark(stats) {
  const benchmark = state.benchmark;
  const now = performance.now();
  // GPU-order preparation has no drawable and no ticket identity yet. Record
  // that invariant explicitly, yield to WebGPU, then retry the same trace step
  // on the next RAF. A visible/drawn value of zero here is hidden state, not a
  // presented frame or benchmark sample.
  if (!stats.framePresented || stats.gpuOrderPreparationPending) {
    console.info(`GPU_ORDER_PREPARATION_JSON ${JSON.stringify({
      schema: "gsplat-benchmark/v1",
      record_type: "gpu_order_preparation",
      camera_revision: stats.cameraRevision,
      frame_presented: stats.framePresented,
      gpu_order_preparation_pending: stats.gpuOrderPreparationPending,
      submitted_measurement_ticket: stats.submittedMeasurementTicket,
      submitted_measurement_backend: stats.submittedMeasurementBackend,
      projected_policy: stats.projectedPolicy,
      projected_execution: stats.projectedExecution,
      projected_measurement_submission: stats.projectedMeasurementSubmission,
      projected_measurement_ticket: stats.projectedMeasurementTicket,
      gpu_order_producer: stats.gpuOrderProducer,
      gpu_producer_measurement_submission: stats.gpuProducerMeasurementSubmission,
      gpu_producer_measurement_ticket: stats.gpuProducerMeasurementTicket,
      visible: stats.visible,
      drawn: stats.drawn,
      surface_width: stats.surfaceWidth,
      surface_height: stats.surfaceHeight,
      raster_execution_plan: stats.rasterExecutionPlan,
    })}`);
    benchmark.presentationPending = true;
    return;
  }
  benchmark.presentationPending = false;
  if (benchmark.currentStatsSchedule
      && stats.rasterExecutionPlan !== "projected_quads_exact") {
    failStrictBenchmarkForOrderEvidence(
      `Packed Exact current-stats schedule observed raster plan ` +
      `${stats.rasterExecutionPlan ?? "missing"}`,
    );
    return;
  }
  if (stats.rasterExecutionPlan === "projected_quads_exact") {
    if (benchmark.terminalQueueThroughput) {
      const terminalWarmup = benchmark.terminalQueueThroughput.state
        === "warmup_terminal_submitting";
      const terminalMeasured = benchmark.terminalQueueThroughput.state
        === "terminal_measured_submitting";
      const terminalBoundary = terminalWarmup || terminalMeasured;
      const actualPlan = exactPlanForFrame(stats);
      const currentStatsShapeValid = terminalBoundary
        ? stats.currentStatsSubmission === "issued"
          && Number.isSafeInteger(stats.currentStatsTicket)
          && stats.currentStatsTicket > 0
        : stats.currentStatsSubmission === "not_requested"
          && stats.currentStatsTicket === null
          && stats.currentStatsPlan === null
          && stats.currentStatsSceneGeneration === null
          && stats.currentStatsCameraRevision === null
          && stats.currentStatsViewportGeneration === null
          && stats.currentStatsContractGeneration === null
          && stats.currentStatsPlanSetGeneration === null
          && stats.currentStatsOrderGeneration === null
          && stats.currentStatsRasterGeneration === null
          && stats.currentStatsEncodeAttempt === null
          && stats.currentStatsPresentationSequence === null;
      const fixedGpuCompactCell = state.requestedOrderBackend === "gpu"
        && state.requestedProjectedPolicy === "compact"
        && state.requestedGpuOrderProducer === null;
      if (!currentStatsShapeValid
          || (!fixedGpuCompactCell && stats.adaptiveState === "disabled")
          || stats.submittedMeasurementTicket !== null
          || stats.submittedMeasurementBackend !== null
          || stats.projectedMeasurementSubmission !== "not_requested"
          || stats.projectedMeasurementTicket !== null
          || stats.gpuProducerMeasurementSubmission !== "not_requested"
          || stats.gpuProducerMeasurementTicket !== null
          || actualPlan === null) {
        failStrictBenchmarkForOrderEvidence(
          "terminal-queue throughput observed non-terminal current-stats work, "
          + "a missing boundary receipt, or an invalid Exact adaptive plan",
        );
        return;
      }
      let currentStatsSubmission = null;
      if (terminalBoundary) {
        const phase = terminalWarmup ? "warmup_boundary" : "final_measured";
        currentStatsSubmission = trackBenchmarkCurrentStatsSubmission(
          benchmark,
          {
            requestOutstanding: benchmark.terminalCurrentStatsRequestOutstanding,
            requestPhase: phase,
          },
          stats,
        );
        if (!currentStatsSubmission || !benchmark.enabled) return;
        benchmark.terminalCurrentStatsPending = {
          ticket: stats.currentStatsTicket,
          stats,
          traceStep: benchmark.currentTraceStep,
          phase,
        };
      }
      let phase;
      try {
        phase = benchmark.terminalQueueThroughput.noteDraw({
          currentStatsSubmission: stats.currentStatsSubmission,
          currentStatsTicket: stats.currentStatsTicket,
          submittedAtMonotonicMs:
            currentStatsSubmission?.submitted_at_monotonic_ms ?? now,
          projectedAdaptiveState: stats.projectedAdaptiveState,
          projectedExecution: stats.projectedExecution,
          exactAdaptiveState: stats.adaptiveState,
          actualPlan,
        });
      } catch (error) {
        failStrictBenchmarkForOrderEvidence(compactMessage(error));
        return;
      }
      benchmark.presentedSubmissionCount += 1;
      benchmark.observedFrames += 1;
      benchmark.primingOrder = false;
      if (terminalBoundary && state.q1QueueTerminalEnabled) {
        try {
          state.wasmRenderer.requestDiagnosticQueueTerminal();
        } catch (error) {
          failStrictBenchmarkForOrderEvidence(
            `queue terminal callback arm failed: ${compactMessage(error)}`,
          );
          return;
        }
      }
      if (phase === "warmup") {
        benchmark.lastObservedFrameMs = now;
        return;
      }
      const frameWallMs = benchmark.lastObservedFrameMs === null
        ? stats.frameMs
        : now - benchmark.lastObservedFrameMs;
      benchmark.lastObservedFrameMs = now;
      accumulateBenchmark(
        benchmark,
        { ...stats, visible: null, contributor: null, drawn: null },
        frameWallMs,
        now,
        benchmark.currentTraceStep,
      );
      return;
    }
    const currentStatsSchedule = benchmark.currentStatsSchedule;
    if (!currentStatsSchedule) {
      failStrictBenchmarkForOrderEvidence(
        "Exact benchmark frame lacks its current-stats schedule",
      );
      return;
    }
    if (stats.currentStatsSubmission === "not_requested") {
      try {
        // An accepted observer request may be deferred across queue-boundary
        // and formal Adaptive sample presentations. Keep the same logical
        // trace step until the renderer publishes the requested ticket; these
        // boundary frames belong to the untimed control ledger, not its 20+80
        // logical samples.
        const deferredPresentation = currentStatsSchedule.recordDeferredPresentation({
          submission: stats.currentStatsSubmission,
          ticket: stats.currentStatsTicket,
          cameraRevision: stats.cameraRevision,
          traceStep: benchmark.currentTraceStep,
          observedAtMonotonicMs: now,
        });
        if (stats.measurementUnsampledReason !== null
            || (stats.submittedMeasurementTicket === null)
              !== (stats.submittedMeasurementBackend === null)
            || (stats.submittedMeasurementTicket !== null && stats.refreshSort !== true)) {
          throw new Error(
            "deferred current-stats presentation exposed an incomplete order terminal identity",
          );
        }
        if (stats.submittedMeasurementTicket !== null) {
          if (!trackAuxiliaryCurrentStatsFormalSubmission(
            benchmark,
            currentStatsSchedule,
            stats,
            deferredPresentation,
          )) return;
          benchmark.pendingOrderSample = {
            ticket: stats.submittedMeasurementTicket,
            stats,
            traceStep: benchmark.currentTraceStep,
            priming: false,
            auxiliaryCurrentStats: true,
            gpuProducerTicket: stats.gpuProducerMeasurementSubmission === "issued"
              ? stats.gpuProducerMeasurementTicket
              : null,
          };
        }
        if (stats.projectedMeasurementSubmission !== "not_requested"
            || stats.projectedMeasurementTicket !== null
            || stats.gpuProducerMeasurementSubmission !== "not_requested"
            || stats.gpuProducerMeasurementTicket !== null) {
          throw new Error(
            "deferred current-stats presentation exposed an unrelated formal ticket",
          );
        }
      } catch (error) {
        failStrictBenchmarkForOrderEvidence(compactMessage(error));
        return;
      }
      benchmark.presentedSubmissionCount += 1;
      return;
    }
    if (stats.currentStatsSubmission !== "issued") {
      failStrictBenchmarkForOrderEvidence(
        `presented Exact frame did not issue its requested current-stats ticket: ` +
        `${stats.currentStatsSubmission ?? "missing"}`,
      );
      return;
    }
    benchmark.presentedSubmissionCount += 1;
    const submission = trackBenchmarkCurrentStatsSubmission(
      benchmark,
      currentStatsSchedule,
      stats,
    );
    if (!submission) return;
    try {
      currentStatsSchedule.recordIssued({
        ticket: stats.currentStatsTicket,
        stats,
        submittedAtMonotonicMs: submission.submitted_at_monotonic_ms,
      });
    } catch (error) {
      failStrictBenchmarkForOrderEvidence(compactMessage(error));
      return;
    }
    benchmark.primingOrder = false;
    return;
  }
  benchmark.presentedSubmissionCount += 1;
  if (benchmark.pendingOrderSample) {
    const pending = benchmark.pendingOrderSample;
    if (!benchmark.terminalOrderTickets.has(pending.ticket)
        || (pending.gpuProducerTicket !== null
          && !benchmark.terminalGpuProducerTickets.has(pending.gpuProducerTicket))) return;
    benchmark.pendingOrderSample = null;
    if (pending.priming) return;
    acceptBenchmarkFrame(
      benchmark,
      pending.stats,
      now,
      pending.traceStep,
    );
    return;
  }
  if (benchmark.pendingProjectedSample) {
    const pending = benchmark.pendingProjectedSample;
    if (!benchmark.terminalProjectedTickets.has(pending.ticket)) return;
    benchmark.pendingProjectedSample = null;
    acceptBenchmarkFrame(benchmark, pending.stats, now, pending.traceStep);
    return;
  }
  if (benchmark.pendingGpuProducerSample) {
    const pending = benchmark.pendingGpuProducerSample;
    if (!benchmark.terminalGpuProducerTickets.has(pending.ticket)) return;
    benchmark.pendingGpuProducerSample = null;
    acceptBenchmarkFrame(benchmark, pending.stats, now, pending.traceStep);
    return;
  }

  if (benchmark.primingOrder && stats.refreshSort !== true) {
    failStrictBenchmarkForOrderEvidence("fixed-camera GPU preflight did not refresh order");
    return;
  }
  if (!trackBenchmarkOrderSubmission(benchmark, stats)) return;
  if (!trackBenchmarkProjectedSubmission(benchmark, stats)) return;
  if (!trackBenchmarkGpuProducerSubmission(benchmark, stats)) return;
  if (stats.refreshSort === true
      && (benchmark.primingOrder
        || benchmark.orderCompletionProtocol === "isolated_terminal")) {
    benchmark.pendingOrderSample = {
      ticket: stats.submittedMeasurementTicket,
      stats,
      traceStep: benchmark.currentTraceStep,
      priming: benchmark.primingOrder,
      gpuProducerTicket: stats.gpuProducerMeasurementSubmission === "issued"
        ? stats.gpuProducerMeasurementTicket
        : null,
    };
    benchmark.primingOrder = false;
    return;
  }
  if (stats.projectedMeasurementSubmission === "issued"
      && benchmark.orderCompletionProtocol === "isolated_terminal") {
    benchmark.pendingProjectedSample = {
      ticket: stats.projectedMeasurementTicket,
      stats,
      traceStep: benchmark.currentTraceStep,
    };
    return;
  }
  if (stats.gpuProducerMeasurementSubmission === "issued"
      && benchmark.orderCompletionProtocol === "isolated_terminal") {
    benchmark.pendingGpuProducerSample = {
      ticket: stats.gpuProducerMeasurementTicket,
      stats,
      traceStep: benchmark.currentTraceStep,
    };
    return;
  }
  acceptBenchmarkFrame(benchmark, stats, now, benchmark.currentTraceStep);
}

function currentStatsIdentityFromFrame(stats) {
  return {
    ticket: stats.currentStatsTicket,
    plan: stats.currentStatsPlan,
    scene_generation: stats.currentStatsSceneGeneration,
    camera_revision: stats.currentStatsCameraRevision,
    viewport_generation: stats.currentStatsViewportGeneration,
    contract_generation: stats.currentStatsContractGeneration,
    plan_set_generation: stats.currentStatsPlanSetGeneration,
    order_generation: stats.currentStatsOrderGeneration,
    raster_generation: stats.currentStatsRasterGeneration,
    encode_attempt: stats.currentStatsEncodeAttempt,
    presentation_sequence: stats.currentStatsPresentationSequence,
  };
}

function trackBenchmarkCurrentStatsSubmission(benchmark, schedule, stats) {
  const identity = currentStatsIdentityFromFrame(stats);
  const countsUnavailable = stats.visible === null && stats.drawn === null;
  const countsAvailable = Number.isSafeInteger(stats.visible) && stats.visible >= 0
    && Number.isSafeInteger(stats.drawn) && stats.drawn >= 0;
  const invalidCountAvailability = stats.visibleCountPending
    ? !countsUnavailable
    : !countsAvailable;
  if (!schedule.requestOutstanding
      || stats.currentStatsSubmission !== "issued"
      || !Number.isSafeInteger(identity.ticket) || identity.ticket <= 0
      || identity.camera_revision !== stats.cameraRevision
      || invalidCountAvailability
      || stats.submittedMeasurementTicket !== null
      || stats.submittedMeasurementBackend !== null
      || stats.measurementUnsampledReason !== null) {
    failStrictBenchmarkForOrderEvidence(
      `Exact frame lacks an issued renderer current-stats identity or exposed provisional counts ` +
      `ticket=${identity.ticket} revision=${identity.camera_revision}/${stats.cameraRevision} ` +
      `visible=${stats.visible} drawn=${stats.drawn} pending=${stats.visibleCountPending}`,
    );
    return null;
  }
  if (benchmark.issuedCurrentStatsTickets.has(identity.ticket)) {
    failStrictBenchmarkForOrderEvidence(
      `duplicate renderer current-stats ticket=${identity.ticket}`,
    );
    return null;
  }
  const phase = schedule.requestPhase;
  if (!phase) {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats ticket=${identity.ticket} lacks its request phase`,
    );
    return null;
  }
  const record = {
    run_id: benchmark.collector.runId,
    ...identity,
    phase,
    submitted_at_monotonic_ms: performance.now(),
  };
  benchmark.issuedCurrentStatsTickets.set(identity.ticket, record);
  benchmark.currentStatsSubmissionRecords.push(record);
  globalThis.GSPLAT_ORDER_LEDGER_COMPLETE = false;
  console.info(`CURRENT_STATS_SUBMISSION_JSON ${JSON.stringify(record)}`);
  if (stats.visibleCountPending) {
    console.info(`CURRENT_STATS_PENDING_FRAME_JSON ${JSON.stringify({
      run_id: benchmark.collector.runId,
      ticket: identity.ticket,
      camera_revision: identity.camera_revision,
      visible: stats.visible,
      drawn: stats.drawn,
      visible_count_pending: true,
    })}`);
  }
  return record;
}

function exactPlanForFrame(stats) {
  if (stats.orderBackend === "cpu") return "cpu_post_sort";
  if (stats.orderBackend === "gpu" && stats.projectedExecution === "compact") {
    return "gpu_preproject";
  }
  if (stats.orderBackend === "gpu") return "gpu_post_sort";
  return null;
}

function joinBenchmarkCurrentStatsTerminal(
  benchmark,
  pending,
  terminal,
  terminalAtMonotonicMs = performance.now(),
) {
  const issued = benchmark.issuedCurrentStatsTickets.get(pending.ticket);
  const terminalRecord = {
    run_id: benchmark.collector.runId,
    phase: issued?.phase ?? null,
    status: terminal.status,
    ticket: terminal.ticket,
    plan: terminal.plan,
    scene_generation: terminal.sceneGeneration,
    camera_revision: terminal.cameraRevision,
    viewport_generation: terminal.viewportGeneration,
    contract_generation: terminal.contractGeneration,
    plan_set_generation: terminal.planSetGeneration,
    order_generation: terminal.orderGeneration,
    raster_generation: terminal.rasterGeneration,
    encode_attempt: terminal.encodeAttempt,
    presentation_sequence: terminal.presentationSequence,
    count_semantics: terminal.countSemantics ?? null,
    source_count: terminal.sourceCount ?? null,
    visible: terminal.visibleCount ?? null,
    contributor: terminal.contributorCount ?? null,
    drawn: terminal.drawnCount ?? null,
    terminal_at_monotonic_ms: terminalAtMonotonicMs,
  };
  console.info(`CURRENT_STATS_TERMINAL_JSON ${JSON.stringify(terminalRecord)}`);
  if (!issued || terminal.status !== "ready") {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats ticket=${pending.ticket} terminated with ${terminal.status}`,
    );
    return null;
  }
  const frameIdentity = currentStatsIdentityFromFrame(pending.stats);
  for (const key of [
    "ticket", "plan", "scene_generation", "camera_revision", "viewport_generation",
    "contract_generation", "plan_set_generation", "order_generation", "raster_generation",
    "encode_attempt", "presentation_sequence",
  ]) {
    if (issued[key] !== terminalRecord[key] || frameIdentity[key] !== terminalRecord[key]) {
      failStrictBenchmarkForOrderEvidence(
        `renderer current-stats ticket=${pending.ticket} identity mismatch for ${key}`,
      );
      return null;
    }
  }
  const exactPlan = exactPlanForFrame(pending.stats);
  const exactContributorCompaction = terminal.countSemantics === "indirect_draw_equals_contributor";
  const expectedDrawn = exactContributorCompaction
    ? terminal.contributorCount
    : terminal.visibleCount;
  if (terminal.plan !== exactPlan
      || terminal.sourceCount !== state.scene?.count
      || !Number.isSafeInteger(terminal.visibleCount)
      || !Number.isSafeInteger(terminal.contributorCount)
      || !Number.isSafeInteger(terminal.drawnCount)
      || terminal.contributorCount > terminal.visibleCount
      || terminal.visibleCount > terminal.sourceCount
      || terminal.drawnCount !== expectedDrawn) {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats ticket=${pending.ticket} has mismatched plan or invalid S/V/C/D`,
    );
    return null;
  }
  if (state.requestedGpuOrderProducer !== null) {
    const requestedPlan = state.requestedGpuOrderProducer === "preproject"
      ? "gpu_preproject"
      : "gpu_post_sort";
    if (terminal.plan !== requestedPlan) {
      failStrictBenchmarkForOrderEvidence(
        `renderer current-stats plan ${terminal.plan} does not prove requested producer ` +
        `${state.requestedGpuOrderProducer}`,
      );
      return null;
    }
    globalThis.GSPLAT_GPU_PRODUCER_LEDGER_COMPLETE = true;
  }
  benchmark.terminalCurrentStatsTickets.set(pending.ticket, terminalRecord);
  benchmark.currentStatsTerminalRecords.push(terminalRecord);
  globalThis.GSPLAT_ORDER_LEDGER_COMPLETE =
    benchmark.issuedCurrentStatsTickets.size > 0
    && benchmark.terminalCurrentStatsTickets.size === benchmark.issuedCurrentStatsTickets.size;
  maybeEmitMonotonicOrderingWindow(benchmark);
  return {
    ...pending.stats,
    visible: terminal.visibleCount,
    contributor: terminal.contributorCount,
    drawn: terminal.drawnCount,
    exactContributorCompaction,
    visibleCountRevision: terminal.cameraRevision,
    visibleCountPending: false,
    currentStatsTerminalStatus: terminal.status,
  };
}

function pollBenchmarkCurrentStatsReceipts(benchmark) {
  const schedule = benchmark.currentStatsSchedule;
  if (!schedule || schedule.pendingCount === 0) return;
  let terminal;
  try {
    benchmark.terminalPollCount += 1;
    terminal = state.wasmRenderer.pollCurrentStats();
  } catch (error) {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats poll failed: ${compactMessage(error)}`,
    );
    return;
  }
  if (terminal.status === "empty") {
    try {
      schedule.noteEmptyPoll(performance.now());
    } catch (error) {
      failStrictBenchmarkForOrderEvidence(compactMessage(error));
    }
    return;
  }
  if (terminal.status === "unsampled") {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats poll was unsampled: ${terminal.reason ?? "unknown"}`,
    );
    return;
  }
  if (terminal.status !== "ready") {
    // Preserve the renderer-owned failed terminal in the raw console ledger;
    // join rejects it before dereferencing frame evidence or publishing.
    joinBenchmarkCurrentStatsTerminal(
      benchmark,
      { ticket: terminal.ticket, stats: null },
      terminal,
    );
    return;
  }
  let pending;
  try {
    pending = schedule.pendingForTerminal({
      ticket: terminal.ticket,
      status: terminal.status,
    });
  } catch (error) {
    failStrictBenchmarkForOrderEvidence(compactMessage(error));
    return;
  }
  const joined = joinBenchmarkCurrentStatsTerminal(benchmark, pending, terminal);
  if (!joined || !benchmark.enabled) return;
  try {
    schedule.recordTerminal({
      pending,
      joined,
      terminalAtMonotonicMs: performance.now(),
    });
    const ready = schedule.takeReadyInSubmissionOrder();
    for (const completed of ready) {
      updateFrameStats(completed.joined);
      updateStatusOverlay(completed.joined);
      if (completed.priming) continue;
      const frameProgressAt = schedule.protocol === "sustained_window"
        ? completed.frameProgressAtMonotonicMs
        : completed.terminalAtMonotonicMs;
      acceptBenchmarkFrame(
        benchmark,
        completed.joined,
        frameProgressAt,
        completed.traceStep,
      );
    }
  } catch (error) {
    failStrictBenchmarkForOrderEvidence(compactMessage(error));
    return;
  }
  if (schedule.complete && benchmark.enabled
      && benchmark.collector.samples.length !== benchmark.frames) {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats drain completed with ${benchmark.collector.samples.length}/` +
      `${benchmark.frames} measured samples`,
    );
  }
}

function pollPendingBenchmarkReceipts(benchmark) {
  const pendingOrder = benchmark.pendingOrderSample;
  const pendingProjected = benchmark.pendingProjectedSample;
  const pendingGpuProducer = benchmark.pendingGpuProducerSample;
  if ((pendingOrder && pendingProjected)
      || (pendingProjected && pendingGpuProducer)) {
    failStrictBenchmarkForOrderEvidence(
      "one frame exposed simultaneous order and projected formal tickets",
    );
    return;
  }
  const pending = pendingOrder ?? pendingProjected ?? pendingGpuProducer;
  if (!pending) return;
  try {
    if (pending.auxiliaryCurrentStats) {
      benchmark.currentStatsSchedule.requireRequestWithinDeadline(performance.now());
    }
    benchmark.terminalPollCount += 1;
    const receipts = state.wasmRenderer.drainOrderMeasurementReceipts();
    const adaptiveState = pending.stats.adaptiveState ?? "disabled";
    emitCpuOrderMeasurements(receipts?.completedCpuOrderMeasurements ?? [], adaptiveState);
    emitOrderMeasurements(receipts?.completedOrderMeasurements ?? [], adaptiveState);
    emitOrderMeasurementFailures(receipts?.failedOrderMeasurements ?? [], adaptiveState);
    const projectedAdaptiveState = pending.stats.projectedAdaptiveState ?? "disabled";
    emitProjectedMeasurements(
      receipts?.completedProjectedMeasurements ?? [],
      projectedAdaptiveState,
    );
    emitProjectedMeasurementFailures(
      receipts?.failedProjectedMeasurements ?? [],
      projectedAdaptiveState,
    );
    emitGpuProducerMeasurements(receipts?.completedGpuProducerMeasurements ?? []);
    emitGpuProducerMeasurementFailures(receipts?.failedGpuProducerMeasurements ?? []);
  } catch (error) {
    failStrictBenchmarkForOrderEvidence(
      `standalone order receipt poll failed: ${compactMessage(error)}`,
    );
    return;
  }
  const terminalObserved = pendingOrder
    ? (pending.auxiliaryCurrentStats
      ? benchmark.terminalAuxiliaryFormalTickets.has(pending.ticket)
      : benchmark.terminalOrderTickets.has(pending.ticket))
      && (pending.gpuProducerTicket === null
        || benchmark.terminalGpuProducerTickets.has(pending.gpuProducerTicket))
    : pendingProjected
      ? benchmark.terminalProjectedTickets.has(pending.ticket)
      : benchmark.terminalGpuProducerTickets.has(pending.ticket);
  if (!benchmark.enabled || !terminalObserved) return;
  if (pendingOrder) benchmark.pendingOrderSample = null;
  else if (pendingProjected) benchmark.pendingProjectedSample = null;
  else benchmark.pendingGpuProducerSample = null;
  if (pending.priming || pending.auxiliaryCurrentStats) return;
  acceptBenchmarkFrame(
    benchmark,
    pending.stats,
    performance.now(),
    pending.traceStep,
  );
}

function trackBenchmarkProjectedSubmission(benchmark, stats) {
  if (!usingWasm()) return true;
  const submission = stats.projectedMeasurementSubmission;
  const ticket = stats.projectedMeasurementTicket;
  const execution = stats.projectedMeasurementExecution;
  const revision = stats.cameraRevision;
  const orderBackend = stats.orderBackend;
  if (stats.projectedPolicy !== state.requestedProjectedPolicy) {
    failStrictBenchmarkForOrderEvidence(
      `projected policy mismatch requested=${state.requestedProjectedPolicy} ` +
      `reported=${stats.projectedPolicy ?? "missing"}`,
    );
    return false;
  }
  if (state.requestedProjectedPolicy !== "adaptive") {
    if (stats.projectedExecution !== state.requestedProjectedPolicy
        || stats.projectedAdaptiveState !== "disabled"
        || submission !== "not_requested" || ticket !== null
        || execution !== null || stats.projectedMeasurementUnsampledReason !== null) {
      failStrictBenchmarkForOrderEvidence(
        `forced projected policy ${state.requestedProjectedPolicy} reported inconsistent ` +
        `execution/state or exposed measurement identity`,
      );
      return false;
    }
    return true;
  }
  if (submission === "not_requested") {
    if (ticket !== null || execution !== null
        || stats.projectedMeasurementUnsampledReason !== null) {
      failStrictBenchmarkForOrderEvidence(
        "not-requested projected sample exposed ticket, execution, or unsampled reason",
      );
      return false;
    }
    return true;
  }
  if (submission === "unsampled") {
    if (ticket !== null
        || !["candidate", "compact"].includes(execution)
        || execution !== stats.projectedExecution
        || !["ring_busy", "surface_unavailable"].includes(
          stats.projectedMeasurementUnsampledReason,
        )) {
      failStrictBenchmarkForOrderEvidence(
        "unsampled projected request exposed invalid ticket, execution, or reason",
      );
      return false;
    }
    return true;
  }
  if (submission !== "issued"
      || !Number.isSafeInteger(ticket) || ticket < 2 ** 52
      || !Number.isSafeInteger(revision) || revision < 0
      || !["candidate", "compact"].includes(execution)
      || !["cpu", "gpu"].includes(orderBackend)
      || execution !== stats.projectedExecution
      || stats.projectedMeasurementUnsampledReason !== null) {
    failStrictBenchmarkForOrderEvidence(
      `projected sample lacks a valid matching ticket/execution/backend/revision ` +
      `ticket=${ticket} execution=${execution} frame_execution=${stats.projectedExecution} ` +
      `backend=${orderBackend} revision=${revision}`,
    );
      return false;
  }
  if (stats.submittedMeasurementTicket !== null) {
    failStrictBenchmarkForOrderEvidence(
      "one frame exposed simultaneous order and projected formal tickets",
    );
    return false;
  }
  if (benchmark.issuedProjectedTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(`duplicate issued projected ticket=${ticket}`);
    return false;
  }
  const phase = benchmark.observedFrames < benchmark.warmupFrames ? "warmup" : "measured";
  const submittedAtMonotonicMs = performance.now();
  benchmark.issuedProjectedTickets.set(ticket, { revision, execution, orderBackend });
  globalThis.GSPLAT_PROJECTED_LEDGER_COMPLETE = false;
  const record = {
    run_id: benchmark.collector.runId,
    requested_policy: state.requestedProjectedPolicy,
    execution,
    order_backend: orderBackend,
    ticket,
    camera_revision: revision,
    phase,
    submitted_at_monotonic_ms: submittedAtMonotonicMs,
  };
  benchmark.projectedSubmissionRecords.push(record);
  console.info(`PROJECTED_MEASUREMENT_SUBMISSION_JSON ${JSON.stringify(record)}`);
  return true;
}

function trackBenchmarkGpuProducerSubmission(benchmark, stats) {
  if (!usingWasm()) return true;
  const requested = state.requestedGpuOrderProducer;
  const actual = stats.gpuOrderProducer;
  const submission = stats.gpuProducerMeasurementSubmission;
  const ticket = stats.gpuProducerMeasurementTicket;
  const producer = stats.gpuProducerMeasurementProducer;
  const unsampled = stats.gpuProducerMeasurementUnsampledReason;

  if (requested === null) {
    if ((stats.orderBackend === "gpu" && actual !== "post-sort")
        || (stats.orderBackend === "cpu" && actual !== null)
        || submission !== "not_requested"
        || ticket !== null || producer !== null || unsampled !== null) {
      failStrictBenchmarkForOrderEvidence(
        "unrequested GPU producer evidence did not preserve the disabled PostSort default",
      );
      return false;
    }
    return true;
  }

  if (actual !== requested
      || stats.orderBackend !== "gpu"
      || stats.rasterExecutionPlan !== "projected_quads_exact"
      || stats.projectedPolicy !== "compact"
      || stats.projectedExecution !== "compact"
      || stats.projectedAdaptiveState !== "disabled"
      || stats.refreshSort !== true) {
    failStrictBenchmarkForOrderEvidence(
      `GPU producer frame is outside the strict context requested=${requested} ` +
      `actual=${actual} backend=${stats.orderBackend} raster=${stats.rasterExecutionPlan} ` +
      `projected=${stats.projectedPolicy}/${stats.projectedExecution} ` +
      `sort_refreshed=${stats.refreshSort}`,
    );
    return false;
  }
  if (submission !== "issued"
      || !Number.isSafeInteger(ticket) || ticket < 2 ** 51 || ticket >= 2 ** 52
      || producer !== requested || unsampled !== null) {
    failStrictBenchmarkForOrderEvidence(
      `GPU producer frame lacks an issued matching ticket requested=${requested} ` +
      `submission=${submission} ticket=${ticket} producer=${producer} unsampled=${unsampled}`,
    );
    return false;
  }
  if (benchmark.issuedGpuProducerTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(`duplicate issued GPU producer ticket=${ticket}`);
    return false;
  }
  const phase = benchmark.primingOrder
    ? "preflight"
    : benchmark.observedFrames < benchmark.warmupFrames ? "warmup" : "measured";
  const submittedAtMonotonicMs = performance.now();
  benchmark.issuedGpuProducerTickets.set(ticket, { revision: stats.cameraRevision, producer });
  globalThis.GSPLAT_GPU_PRODUCER_LEDGER_COMPLETE = false;
  const record = {
    run_id: benchmark.collector.runId,
    producer,
    ticket,
    camera_revision: stats.cameraRevision,
    phase,
    submitted_at_monotonic_ms: submittedAtMonotonicMs,
  };
  benchmark.gpuProducerSubmissionRecords.push(record);
  console.info(`GPU_PRODUCER_MEASUREMENT_SUBMISSION_JSON ${JSON.stringify(record)}`);
  return true;
}

function acceptBenchmarkFrame(benchmark, stats, now, traceStep) {
  benchmark.observedFrames += 1;
  if (benchmark.observedFrames <= benchmark.warmupFrames) {
    benchmark.lastObservedFrameMs = now;
    return;
  }

  const frameWallMs = benchmark.lastObservedFrameMs === null ? stats.frameMs : now - benchmark.lastObservedFrameMs;
  benchmark.lastObservedFrameMs = now;
  accumulateBenchmark(benchmark, stats, frameWallMs, now, traceStep);

  if (benchmark.collector.samples.length < benchmark.frames) {
    return;
  }

  finishBenchmark(benchmark);
}

function trackBenchmarkOrderSubmission(benchmark, stats) {
  if (stats.refreshSort !== true) return true;
  const ticket = stats.submittedMeasurementTicket;
  const revision = stats.cameraRevision;
  const backend = stats.submittedMeasurementBackend;
  if (stats.measurementUnsampledReason) {
    failStrictBenchmarkForOrderEvidence(
      `${stats.orderBackend} sort refresh was unsampled: ${stats.measurementUnsampledReason}`,
    );
    return false;
  }
  if (!Number.isSafeInteger(ticket) || ticket <= 0
      || !Number.isSafeInteger(revision) || revision < 0
      || (backend !== "cpu" && backend !== "gpu")
      || backend !== stats.orderBackend) {
    failStrictBenchmarkForOrderEvidence(
      `sort refresh lacks a valid matching ticket/backend/revision ` +
      `ticket=${ticket} backend=${backend} frame_backend=${stats.orderBackend} revision=${revision}`,
    );
    return false;
  }
  if (benchmark.issuedOrderTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(`duplicate issued order measurement ticket=${ticket}`);
    return false;
  }
  const phase = benchmark.primingOrder
    ? "preflight"
    : benchmark.observedFrames < benchmark.warmupFrames ? "warmup" : "measured";
  const submittedAtMonotonicMs = performance.now();
  benchmark.issuedOrderTickets.set(ticket, { revision, backend });
  globalThis.GSPLAT_ORDER_LEDGER_COMPLETE = false;
  const submissionRecord = {
    requested_backend: state.requestedOrderBackend,
    actual_backend: backend,
    ticket,
    camera_revision: revision,
    phase,
    submitted_at_monotonic_ms: submittedAtMonotonicMs,
  };
  benchmark.orderSubmissionRecords.push(submissionRecord);
  console.info(`ORDER_MEASUREMENT_SUBMISSION_JSON ${JSON.stringify(submissionRecord)}`);
  return true;
}

function trackAuxiliaryCurrentStatsFormalSubmission(
  benchmark,
  schedule,
  stats,
  deferredPresentation,
) {
  const ticket = stats.submittedMeasurementTicket;
  const revision = stats.cameraRevision;
  const backend = stats.submittedMeasurementBackend;
  if (schedule.protocol !== "isolated_terminal"
      || !schedule.requestOutstanding
      || stats.refreshSort !== true
      || !Number.isSafeInteger(ticket) || ticket <= 0
      || !Number.isSafeInteger(revision) || revision < 0
      || !["cpu", "gpu"].includes(backend)
      || backend !== stats.orderBackend
      || stats.measurementUnsampledReason !== null
      || benchmark.issuedOrderTickets.has(ticket)
      || benchmark.terminalOrderTickets.has(ticket)
      || benchmark.issuedAuxiliaryFormalTickets.has(ticket)
      || benchmark.terminalAuxiliaryFormalTickets.has(ticket)
      || deferredPresentation?.phase !== schedule.requestPhase
      || deferredPresentation?.logical_submission_index !== schedule.nextSubmissionIndex
      || deferredPresentation?.camera_revision !== revision) {
    failStrictBenchmarkForOrderEvidence(
      `deferred current-stats presentation has invalid auxiliary formal identity ` +
      `ticket=${ticket} backend=${backend} revision=${revision}`,
    );
    return false;
  }
  const record = {
    run_id: benchmark.collector.runId,
    phase: schedule.requestPhase,
    logical_submission_index: schedule.nextSubmissionIndex,
    attempt_index: deferredPresentation.attempt_index,
    trace_frame_index: deferredPresentation.trace_frame_index,
    deferred_at_monotonic_ms: deferredPresentation.observed_at_monotonic_ms,
    ticket,
    camera_revision: revision,
    actual_backend: backend,
    submitted_at_monotonic_ms: performance.now(),
  };
  try {
    schedule.recordAuxiliaryFormal({
      deferredPresentation,
      ticket,
      backend,
      submittedAtMonotonicMs: record.submitted_at_monotonic_ms,
    });
  } catch (error) {
    failStrictBenchmarkForOrderEvidence(compactMessage(error));
    return false;
  }
  benchmark.issuedAuxiliaryFormalTickets.set(ticket, record);
  benchmark.auxiliaryFormalSubmissionRecords.push(record);
  console.info(`CURRENT_STATS_AUXILIARY_FORMAL_SUBMISSION_JSON ${JSON.stringify(record)}`);
  return true;
}

function recordAuxiliaryCurrentStatsFormalTerminal({
  actualBackend,
  ticket,
  cameraRevision,
  outcome,
  reason = null,
  terminalAtMonotonicMs,
}) {
  const benchmark = state.benchmark;
  const submission = benchmark?.issuedAuxiliaryFormalTickets.get(ticket);
  if (!submission) return false;
  if (submission.actual_backend !== actualBackend
      || submission.camera_revision !== cameraRevision
      || benchmark.terminalOrderTickets.has(ticket)
      || benchmark.terminalAuxiliaryFormalTickets.has(ticket)) {
    failStrictBenchmarkForOrderEvidence(
      `auxiliary formal ticket=${ticket} terminal identity or uniqueness mismatch`,
    );
    return true;
  }
  const terminal = {
    ...submission,
    outcome,
    reason,
    terminal_at_monotonic_ms: terminalAtMonotonicMs,
  };
  benchmark.terminalAuxiliaryFormalTickets.set(ticket, terminal);
  benchmark.auxiliaryFormalTerminalRecords.push(terminal);
  const method = outcome === "success" ? "info" : "error";
  console[method](
    `CURRENT_STATS_AUXILIARY_FORMAL_TERMINAL_JSON ${JSON.stringify(terminal)}`,
  );
  return true;
}

function accumulateBenchmark(
  benchmark,
  stats,
  frameWallMs = stats.frameMs,
  now = performance.now(),
  traceStep = null,
) {
  if (benchmark.measuredStartMs === null) {
    benchmark.measuredStartMs = now;
    benchmark.measurementStartedAt = new Date().toISOString();
  }
  appendBenchmarkSample(benchmark.collector, {
    elapsed_ns: Math.round((now - benchmark.measuredStartMs) * 1_000_000),
    call_ms: stats.callMs,
    frame_wall_ms: frameWallMs,
    renderer_frame_ms: stats.frameMs,
    preprocess_ms: stats.preprocessMs,
    sort_ms: stats.sortMs,
    geometry_submit_ms: stats.pipelineMs,
    gpu_wait_ms: null,
    gpu_complete_ms: stats.gpuCompleteMs ?? null,
    visible: stats.visible == null ? null : Math.max(0, Math.trunc(stats.visible)),
    contributor: stats.contributor == null
      ? null
      : Math.max(0, Math.trunc(stats.contributor)),
    drawn: stats.drawn == null ? null : Math.max(0, Math.trunc(stats.drawn)),
    exact_contributor_compaction: stats.exactContributorCompaction,
    sort_refreshed: stats.refreshSort ?? null,
  });
  const frameReceipt = {
    traceStep,
    rasterExecutionPlan: stats.rasterExecutionPlan ?? "global_quads",
    orderBackend: stats.orderBackend ?? "cpu",
    adaptiveGpuFailure: stats.adaptiveGpuFailure ?? null,
    gpuSortFallback: Boolean(stats.gpuSortFallback),
    adaptiveState: stats.adaptiveState ?? "disabled",
    projectedPolicy: stats.projectedPolicy ?? "adaptive",
    projectedExecution: stats.projectedExecution ?? "candidate",
    projectedAdaptiveState: stats.projectedAdaptiveState ?? "disabled",
    projectedMeasurementSubmission:
      stats.projectedMeasurementSubmission ?? "not_requested",
    projectedMeasurementTicket: stats.projectedMeasurementTicket ?? null,
    projectedMeasurementExecution: stats.projectedMeasurementExecution ?? null,
    projectedMeasurementUnsampledReason:
      stats.projectedMeasurementUnsampledReason ?? null,
    gpuOrderProducer: stats.gpuOrderProducer ?? null,
    gpuProducerMeasurementSubmission:
      stats.gpuProducerMeasurementSubmission ?? "not_requested",
    gpuProducerMeasurementTicket: stats.gpuProducerMeasurementTicket ?? null,
    gpuProducerMeasurementProducer: stats.gpuProducerMeasurementProducer ?? null,
    gpuProducerMeasurementUnsampledReason:
      stats.gpuProducerMeasurementUnsampledReason ?? null,
    cameraRevision: stats.cameraRevision ?? null,
    appliedOrderRevision: stats.appliedOrderRevision ?? null,
    presentedOrderRevisionLag: stats.presentedOrderRevisionLag ?? null,
    submittedMeasurementTicket: stats.submittedMeasurementTicket ?? null,
    submittedMeasurementBackend: stats.submittedMeasurementBackend ?? null,
    measurementUnsampledReason: stats.measurementUnsampledReason ?? null,
    completedMeasurementTicket: stats.completedMeasurementTicket ?? null,
    completedMeasurementRevision: stats.completedMeasurementRevision ?? null,
    completedMeasurementTimingSource: stats.completedMeasurementTimingSource ?? null,
    gpuPreprocessMs: stats.gpuPreprocessMs ?? null,
    gpuRadixMs: stats.gpuRadixMs ?? null,
    gpuOrderMs: stats.gpuOrderMs ?? null,
    gpuCompleteMs: stats.gpuCompleteMs ?? null,
    completedVisibleCount: stats.completedVisibleCount ?? null,
    completedContributorCount: stats.completedContributorCount ?? null,
    completedDrawnCount: stats.completedDrawnCount ?? null,
    completedExactContributorCompaction:
      stats.completedExactContributorCompaction ?? null,
    visibleCountRevision: stats.visibleCountRevision ?? null,
    visibleCountPending: Boolean(stats.visibleCountPending),
    currentStatsSubmission: stats.currentStatsSubmission ?? "not_requested",
    currentStatsTicket: stats.currentStatsTicket ?? null,
    currentStatsPlan: stats.currentStatsPlan ?? null,
    currentStatsSceneGeneration: stats.currentStatsSceneGeneration ?? null,
    currentStatsCameraRevision: stats.currentStatsCameraRevision ?? null,
    currentStatsViewportGeneration: stats.currentStatsViewportGeneration ?? null,
    currentStatsContractGeneration: stats.currentStatsContractGeneration ?? null,
    currentStatsPlanSetGeneration: stats.currentStatsPlanSetGeneration ?? null,
    currentStatsOrderGeneration: stats.currentStatsOrderGeneration ?? null,
    currentStatsRasterGeneration: stats.currentStatsRasterGeneration ?? null,
    currentStatsEncodeAttempt: stats.currentStatsEncodeAttempt ?? null,
    currentStatsPresentationSequence: stats.currentStatsPresentationSequence ?? null,
    currentStatsTerminalStatus: stats.currentStatsTerminalStatus ?? null,
    surfaceWidth: stats.surfaceWidth ?? null,
    surfaceHeight: stats.surfaceHeight ?? null,
    internalRenderWidth: stats.internalRenderWidth ?? null,
    internalRenderHeight: stats.internalRenderHeight ?? null,
    presentedWidth: stats.presentedWidth ?? null,
    presentedHeight: stats.presentedHeight ?? null,
  };
  if (benchmark.q1Capture
      && traceStep?.phase === "measured"
      && traceStep.phaseFrameIndex === benchmark.q1Capture.measuredFrameIndex) {
    if (traceStep.traceFrameIndex !== benchmark.q1Capture.traceFrameIndex
        || benchmark.q1Capture.ready === null) {
      throw new Error("Q1 terminal frame lacks its renderer-owned same-present capture");
    }
    frameReceipt.captureDepthPrecision = benchmark.q1Capture.ready.receipt;
  }
  benchmark.frameReceipts.push(frameReceipt);
  benchmark.measurementEndedAt = new Date().toISOString();
}

function finishBenchmark(benchmark) {
  if (benchmark.currentStatsSchedule && !benchmark.currentStatsSchedule.complete) {
    failStrictBenchmarkForOrderEvidence(
      `renderer current-stats schedule attempted publication while ` +
      `${benchmark.currentStatsSchedule.state}`,
    );
    return;
  }
  if (benchmark.q1Capture && benchmark.q1Capture.ready === null) {
    failStrictBenchmarkForOrderEvidence("Q1 control completed without renderer-owned capture");
    return;
  }
  benchmark.enabled = false;
  maybeEmitMonotonicOrderingWindow(benchmark);
  const result = benchmarkResultLine(benchmark);
  console.info(result);
  els.benchmarkStatus.textContent = "validating";
  els.benchmarkResult.textContent = "Validating exact frame-count and full-resolution evidence...";
  void emitBenchmarkArtifacts(benchmark).then(() => {
    els.benchmarkStatus.textContent = "complete";
    els.runBenchmark.disabled = false;
    els.benchmarkResult.textContent = result;
    setStatus(`state=benchmark_complete ${result}`);
  }).catch((error) => {
    const reason = compactMessage(error);
    benchmark.failure = `artifact validation failed: ${reason}`;
    els.benchmarkStatus.textContent = "failed";
    els.runBenchmark.disabled = false;
    els.benchmarkResult.textContent = `BENCHMARK_ARTIFACT_ERROR ${reason}`;
    setStatus(`state=benchmark_failed ${benchmark.failure}`);
    console.error(`BENCHMARK_ARTIFACT_ERROR ${reason}`);
  });
}

function wasmRendererLabel() {
  if (!usingWasm()) {
    return "webgl2_point_splats";
  }
  return `wasm_${state.wasmRenderer.rasterPath()}`;
}

function benchmarkResultLine(benchmark) {
  const averages = legacyAverages(benchmark.collector);
  const evidenceClassification = benchmark.currentStatsSchedule
    ? " evidence_role=renderer_exact_current_stats_control performance_evidence=false"
    : benchmark.terminalQueueThroughput
      ? " evidence_role=cross_implementation_terminal_queue_throughput performance_evidence=true"
    : "";
  const fixedGpuCompactCell = state.requestedOrderBackend === "gpu"
    && state.requestedProjectedPolicy === "compact"
    && state.requestedGpuOrderProducer === null;
  const producerResult = fixedGpuCompactCell
    ? "gpu_order_producer_override=unset gpu_order_producer_actual=preproject"
    : `gpu_order_producer=${state.requestedGpuOrderProducer ?? "post-sort"}`;
  return (
    `BENCHMARK_RESULT dataset=${state.scene.name} ` +
    `samples=${averages.count} warmup=${benchmark.warmupFrames} ` +
    `sort_interval=${Number(els.sortInterval.value)} renderer=${wasmRendererLabel()} ` +
    `requested_backend=${state.requestedOrderBackend} ` +
    `projected_policy=${state.requestedProjectedPolicy} ` +
    `${producerResult} ` +
    `draw_budget=${usingWasm() ? "full" : Number(els.drawBudget.value)} ` +
    `avg_call_ms=${(averages.callMs ?? 0).toFixed(3)} ` +
    `avg_frame_ms=${(averages.frameMs ?? 0).toFixed(3)} ` +
    `avg_preprocess_ms=${(averages.preprocessMs ?? 0).toFixed(3)} ` +
    `avg_sort_ms=${(averages.sortMs ?? 0).toFixed(3)} ` +
    `avg_geometry_submit_cpu_wall_ms=${(averages.geometrySubmitMs ?? 0).toFixed(3)} ` +
    `avg_visible=${Math.round(averages.visible ?? 0)} ` +
    `avg_drawn=${Math.round(averages.drawn ?? 0)}` +
    evidenceClassification
  );
}

async function emitBenchmarkArtifacts(benchmark) {
  const scene = state.scene;
  const datasetIdentity = canonicalDatasetIdentityFromObservation({
    id: scene.name,
    source_path: scene.sourcePath,
    sha256: scene.sourceSha256,
  });
  if (usingWasm()) {
    state.cameraReceipt = state.wasmRenderer.cameraReceipt();
  }
  const rendererPath = wasmRendererLabel();
  const backend = usingWasm() ? "webgpu" : "webgl2";
  const sortInterval = Number(els.sortInterval.value);
  const resolution = benchmarkResolutionEvidence(
    benchmark.frameReceipts,
    benchmark.requestedWidth,
    benchmark.requestedHeight,
  );
  const surfaceWidth = resolution.surface_width;
  const surfaceHeight = resolution.surface_height;
  const traceText = `benchmark_orbit_v1 yaw_step=${benchmark.yawStep} frames=${benchmark.frames}`;
  const traceSha256 = state.qualificationTrace?.content_sha256 ??
    await sha256Hex(new TextEncoder().encode(traceText));
  const benchmarkWindowConfiguration = (
    benchmark.currentStatsSchedule || benchmark.terminalQueueThroughput
  ) ? {
    schema: "gsplat-benchmark-window-configuration/v1",
    build_commit: globalThis.GSPLAT_BUILD_COMMIT ?? null,
    dataset_sha256: datasetIdentity.sha256,
    dataset_bytes: scene.sourceBytes,
    dataset_splat_count: scene.count,
    dataset_sh_degree: scene.shDegree,
    trace_sha256: traceSha256,
    trace_frame_indices: benchmark.traceSequence
      ? [...benchmark.traceSequence.frameIndices]
      : state.qualificationTrace ? [state.qualificationTraceFrameIndex] : null,
    requested_width: benchmark.requestedWidth,
    requested_height: benchmark.requestedHeight,
    warmup_frames: benchmark.warmupFrames,
    measured_frames: benchmark.frames,
    geometry_path: state.geometryPath,
    order_backend: state.requestedOrderBackend,
    projected_policy: state.requestedProjectedPolicy,
    gpu_order_producer: state.requestedGpuOrderProducer,
    sort_interval: sortInterval,
    cadence: "request_animation_frame",
  } : null;
  const benchmarkWindowConfigurationSha256 = benchmarkWindowConfiguration
    ? await sha256Hex(new TextEncoder().encode(JSON.stringify(benchmarkWindowConfiguration)))
    : null;
  let benchmarkWindow = null;
  if (benchmark.currentStatsSchedule) {
    benchmarkWindow = {
        ...currentStatsEvidenceWindowManifest({
          runId: benchmark.collector.runId,
          configurationSha256: benchmarkWindowConfigurationSha256,
        }),
        configuration: benchmarkWindowConfiguration,
      };
  } else if (benchmark.terminalQueueThroughput) {
    benchmarkWindow = {
      ...benchmark.terminalQueueThroughput.evidence(),
      configuration: benchmarkWindowConfiguration,
    };
    if (benchmarkWindow.configuration_sha256 !== benchmarkWindowConfigurationSha256) {
      throw new Error(
        "terminal-queue throughput configuration does not match its current-stats control",
      );
    }
  }
  const actualGpuOrderProducers = [
    ...new Set(
      benchmark.frameReceipts
        .map((receipt) => receipt.gpuOrderProducer)
        .filter((producer) => producer !== null),
    ),
  ];
  const unavailableFields = [
    "environment.adapter",
    "environment.driver",
    "frames[*].gpu_wait_ms",
    "frames[*].gpu_complete_ms",
  ];
  if (!usingWasm()) {
    unavailableFields.push("frames[*].sort_refreshed");
  }
  if (benchmark.terminalQueueThroughput) {
    unavailableFields.push(
      "frames[*].visible",
      "frames[*].contributor",
      "frames[*].drawn",
      "summary.count_evidence",
    );
  }
  if (globalThis.GSPLAT_BUILD_COMMIT == null) unavailableFields.push("build.repository_commit");
  if (globalThis.GSPLAT_BUILD_DIRTY == null) unavailableFields.push("build.dirty");
  if (globalThis.GSPLAT_BENCHMARK_DEVICE == null) unavailableFields.push("environment.device");
  const manifest = {
    schema: BENCHMARK_SCHEMA,
    record_type: "manifest",
    run_id: benchmark.collector.runId,
    identity: {
      series_id: state.qualificationTraceSequenceEnabled
        ? "web-camera-trace-sequence-v1"
        : state.qualificationTrace ? "web-fixed-camera-v1" : "web-local",
      started_at_utc: benchmark.startedAt,
      ended_at_utc: new Date().toISOString(),
      measurement_started_at_utc: benchmark.measurementStartedAt,
      measurement_ended_at_utc: benchmark.measurementEndedAt,
    },
    build: {
      repository_commit: globalThis.GSPLAT_BUILD_COMMIT ?? null,
      dirty: globalThis.GSPLAT_BUILD_DIRTY ?? null,
      profile: "browser",
      package_version: API_VERSION,
    },
    dataset: {
      ...datasetIdentity,
      bytes: scene.sourceBytes,
      splat_count: scene.count,
      sh_degree: scene.shDegree,
    },
    exactness: {
      source_splat_count: scene.exactnessReceipt?.sourceCount ?? null,
      decoded_splat_count: scene.exactnessReceipt?.decodedCount ?? null,
      encoded_splat_count: scene.exactnessReceipt?.encodedCount ?? null,
      resident_splat_count: scene.exactnessReceipt?.residentCount ?? null,
      addressable_splat_count: scene.exactnessReceipt?.addressableCount ?? null,
      source_sh_degree: scene.exactnessReceipt?.sourceShDegree ?? null,
      resident_sh_degree: scene.exactnessReceipt?.residentShDegree ?? null,
      source_membership: scene.exactnessReceipt?.sourceMembership ?? "unknown",
      sampling: scene.exactnessReceipt?.samplingEnabled === false ? "disabled" : "enabled",
      lod: scene.exactnessReceipt?.lodEnabled === false ? "disabled" : "enabled",
      sh_degree_policy: "source",
      partial_scene_published: scene.exactnessReceipt?.partialScenePublished ?? null,
      full_quality: scene.exactnessReceipt?.fullQuality ?? false,
    },
    trace: state.qualificationTraceSequenceEnabled ? {
      id: state.qualificationTrace.trace_id,
      sha256: traceSha256,
      reference_width: state.qualificationTrace.display.width,
      reference_height: state.qualificationTrace.display.height,
      require_display_match: true,
      display_policy: "trace_display_exact",
      quality_comparable: true,
      frame_indices: [...benchmark.traceSequence.frameIndices],
    } : {
      id: state.qualificationTrace?.trace_id ?? "benchmark-orbit-v1",
      sha256: traceSha256,
      reference_width: state.qualificationTrace?.display.width ?? null,
      reference_height: state.qualificationTrace?.display.height ?? null,
      require_display_match: state.qualificationTrace != null,
      display_policy: state.qualificationTrace ? "trace_display_exact" : "endpoint_orbit",
      quality_comparable: state.qualificationTrace != null,
      frame_index: state.qualificationTrace ? state.qualificationTraceFrameIndex : null,
    },
    renderer: {
      implementation: "gsplat-rs",
      path: rendererPath,
      backend,
      order_backend_requested: state.requestedOrderBackend,
      projected_policy_requested: state.requestedProjectedPolicy,
      gpu_order_producer_requested: state.requestedGpuOrderProducer,
      gpu_order_producer_actual: actualGpuOrderProducers[0] ?? null,
      sort_interval: sortInterval,
      sort_policy: `interval_${sortInterval}`,
      raster_execution_plan: benchmark.frameReceipts[0]?.rasterExecutionPlan ?? null,
      count_semantics: usingWasm()
        ? benchmark.terminalQueueThroughput
          ? "bound_current_stats_control_artifact"
          : "candidate_visible_contributor_issued_v1"
        : undefined,
    },
    timing: {
      frame_wall_source: benchmark.frameWallSource,
      renderer_frame_ms_included: true,
      performance_evidence: benchmarkWindow?.performance_evidence ?? null,
    },
    ordering_window: {
      completion_protocol: benchmark.orderCompletionProtocol,
      presented_submit_count: benchmark.presentedSubmissionCount,
      terminal_poll_count: benchmark.terminalPollCount,
      expected_logical_frame_count: benchmark.warmupFrames + benchmark.frames,
      current_stats_schedule: benchmark.currentStatsSchedule?.evidence() ?? null,
      terminal_current_stats_receipts: benchmark.terminalQueueThroughput
        ? benchmark.currentStatsTerminalRecords.length
        : null,
      auxiliary_formal_submissions: benchmark.auxiliaryFormalSubmissionRecords,
      auxiliary_formal_terminals: benchmark.auxiliaryFormalTerminalRecords,
    },
    benchmark_window: benchmarkWindow,
    gpu_producer_evidence: {
      requested_producer: state.requestedGpuOrderProducer,
      default_when_unset: state.requestedOrderBackend === "gpu"
          && state.requestedProjectedPolicy === "compact"
          && state.requestedGpuOrderProducer === null
        ? "derived_from_exact_whole_plan"
        : "post-sort",
      actual_producers: actualGpuOrderProducers,
      issued_count: benchmark.issuedGpuProducerTickets.size,
      terminal_count: benchmark.terminalGpuProducerTickets.size,
      completion_protocol: benchmark.orderCompletionProtocol,
      terminal_receipt_policy:
        "exactly_one_success_or_structured_failure_per_issued_ticket",
    },
    pairing: state.qualificationTrace ? {
      pair_id: globalThis.GSPLAT_PHASE_E_PAIR_ID ?? null,
      run_order: globalThis.GSPLAT_PHASE_E_PAIR_ORDER ?? null,
      position: globalThis.GSPLAT_PHASE_E_PAIR_POSITION ?? null,
    } : undefined,
    display: {
      width: surfaceWidth,
      height: surfaceHeight,
      dpr: window.devicePixelRatio || 1,
      refresh_hz: 60,
      frame_budget_ms: benchmark.collector.frameBudgetMs,
      refresh_hz_source: "configured",
      frame_budget_source: "configured",
    },
    resolution,
    environment: {
      platform: "web",
      os: navigator.platform || "browser",
      device: globalThis.GSPLAT_BENCHMARK_DEVICE ?? null,
      browser: navigator.userAgent,
      adapter: null,
      driver: null,
    },
    unavailable_fields: unavailableFields,
    qualification_scope: benchmark.currentStatsSchedule
      ? "qualification_q1_current_stats_control_only"
      : benchmark.terminalQueueThroughput
        ? "qualification_q1_terminal_queue_throughput_candidate"
      : state.qualificationTrace ? "phase_e_paired_candidate" : "collector_smoke_only",
    policies: state.qualificationTrace ? {
      dynamicResolution: `disabled_fixed_${surfaceWidth}x${surfaceHeight}`,
      lod: "disabled_full_ply",
      renderer: state.geometryPath,
      membership: "source_equals_decoded_equals_resident_no_sampling",
    } : undefined,
    camera_receipt: state.cameraReceipt,
  };
  console.info(`BENCHMARK_MANIFEST_JSON ${JSON.stringify(manifest)}`);
  for (const [index, frame] of frameRecords(benchmark.collector).entries()) {
    const receipt = benchmark.frameReceipts[index];
    console.info(`BENCHMARK_FRAME_JSON ${JSON.stringify({
      ...frame,
      trace_frame_index: receipt?.traceStep?.traceFrameIndex ?? null,
      trace_timestamp_ns: receipt?.traceStep?.timestampNs ?? null,
      order_backend_requested: state.requestedOrderBackend,
      order_backend: receipt?.orderBackend ?? "cpu",
      raster_execution_plan: receipt?.rasterExecutionPlan ?? null,
      surface_width: receipt?.surfaceWidth ?? null,
      surface_height: receipt?.surfaceHeight ?? null,
      internal_render_width: receipt?.internalRenderWidth ?? null,
      internal_render_height: receipt?.internalRenderHeight ?? null,
      presented_width: receipt?.presentedWidth ?? null,
      presented_height: receipt?.presentedHeight ?? null,
      adaptive_gpu_failure: receipt?.adaptiveGpuFailure ?? null,
      gpu_sort_fallback: receipt?.gpuSortFallback ?? false,
      adaptive_state: receipt?.adaptiveState ?? "disabled",
      projected_policy: receipt?.projectedPolicy ?? "adaptive",
      projected_execution: receipt?.projectedExecution ?? "candidate",
      projected_adaptive_state: receipt?.projectedAdaptiveState ?? "disabled",
      projected_measurement_submission:
        receipt?.projectedMeasurementSubmission ?? "not_requested",
      projected_measurement_ticket: receipt?.projectedMeasurementTicket ?? null,
      projected_measurement_execution:
        receipt?.projectedMeasurementExecution ?? null,
      projected_measurement_unsampled_reason:
        receipt?.projectedMeasurementUnsampledReason ?? null,
      gpu_order_producer: receipt?.gpuOrderProducer ?? null,
      gpu_producer_measurement_submission:
        receipt?.gpuProducerMeasurementSubmission ?? "not_requested",
      gpu_producer_measurement_ticket:
        receipt?.gpuProducerMeasurementTicket ?? null,
      gpu_producer_measurement_producer:
        receipt?.gpuProducerMeasurementProducer ?? null,
      gpu_producer_measurement_unsampled_reason:
        receipt?.gpuProducerMeasurementUnsampledReason ?? null,
      camera_revision: receipt?.cameraRevision ?? null,
      applied_order_revision: receipt?.appliedOrderRevision ?? null,
      presented_order_revision_lag: receipt?.presentedOrderRevisionLag ?? null,
      submitted_measurement_ticket: receipt?.submittedMeasurementTicket ?? null,
      submitted_measurement_backend: receipt?.submittedMeasurementBackend ?? null,
      measurement_unsampled_reason: receipt?.measurementUnsampledReason ?? null,
      visible_count_revision: receipt?.visibleCountRevision ?? null,
      visible_count_pending: receipt?.visibleCountPending ?? false,
      current_stats_submission: receipt?.currentStatsSubmission ?? "not_requested",
      current_stats_ticket: receipt?.currentStatsTicket ?? null,
      current_stats_plan: receipt?.currentStatsPlan ?? null,
      current_stats_scene_generation: receipt?.currentStatsSceneGeneration ?? null,
      current_stats_camera_revision: receipt?.currentStatsCameraRevision ?? null,
      current_stats_viewport_generation: receipt?.currentStatsViewportGeneration ?? null,
      current_stats_contract_generation: receipt?.currentStatsContractGeneration ?? null,
      current_stats_plan_set_generation: receipt?.currentStatsPlanSetGeneration ?? null,
      current_stats_order_generation: receipt?.currentStatsOrderGeneration ?? null,
      current_stats_raster_generation: receipt?.currentStatsRasterGeneration ?? null,
      current_stats_encode_attempt: receipt?.currentStatsEncodeAttempt ?? null,
      current_stats_presentation_sequence:
        receipt?.currentStatsPresentationSequence ?? null,
      presentation_sequence: receipt?.currentStatsPresentationSequence ?? null,
      current_stats_terminal_status: receipt?.currentStatsTerminalStatus ?? null,
      completed_measurement_ticket: receipt?.completedMeasurementTicket ?? null,
      completed_measurement_revision: receipt?.completedMeasurementRevision ?? null,
      completed_measurement_timing_source: receipt?.completedMeasurementTimingSource ?? null,
      gpu_preprocess_ms: receipt?.gpuPreprocessMs ?? null,
      gpu_radix_ms: receipt?.gpuRadixMs ?? null,
      gpu_order_ms: receipt?.gpuOrderMs ?? null,
      completed_visible: receipt?.completedVisibleCount ?? null,
      completed_contributor: receipt?.completedContributorCount ?? null,
      completed_drawn: receipt?.completedDrawnCount ?? null,
      completed_exact_contributor_compaction:
        receipt?.completedExactContributorCompaction ?? null,
      ...(receipt?.captureDepthPrecision
        ? { capture_depth_precision: receipt.captureDepthPrecision }
        : {}),
    })}`);
  }
  console.info(`BENCHMARK_SUMMARY_JSON ${JSON.stringify(benchmarkSummary(benchmark.collector))}`);
}

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, "0")).join("");
}

function updateControlLabels() {
  els.drawBudgetValue.textContent = formatCompact(Number(els.drawBudget.value));
  els.sortIntervalValue.textContent = String(Number(els.sortInterval.value));
  els.pointScaleValue.textContent = Number(els.pointScale.value).toFixed(2);
  els.benchmarkYawValue.textContent = Number(els.benchmarkYaw.value).toFixed(4);
}

function updateFrameStats(stats) {
  els.visibleCount.textContent = formatNumber(stats.visible);
  els.drawnCount.textContent = formatNumber(stats.drawn);
  els.surfaceSize.textContent = state.surfaceSizeLabel;
  els.frameCount.textContent = formatNumber(state.frameCounter);
  els.fpsValue.textContent = state.fps.toFixed(1);
  els.preprocessMs.textContent = `${stats.preprocessMs.toFixed(2)} ms`;
  els.sortMs.textContent = `${stats.sortMs.toFixed(2)} ms`;
  els.geometrySubmitMs.textContent = `${stats.pipelineMs.toFixed(2)} ms`;
  els.callMs.textContent = `${stats.callMs.toFixed(2)} ms`;
  els.frameMs.textContent = `${stats.frameMs.toFixed(2)} ms`;
}

function updateStatusOverlay(stats) {
  const visible = stats.visible == null ? "unavailable" : stats.visible;
  const drawn = stats.drawn == null ? "unavailable" : stats.drawn;
  if (window.innerWidth < 620) {
    state.rendererStatus = [
      `state=rendering frames=${state.frameCounter}`,
      `visible=${visible} drawn=${drawn}/${visible}`,
      `projected=${stats.projectedPolicy}/${stats.projectedExecution} state=${stats.projectedAdaptiveState}`,
      `frame=${stats.frameMs.toFixed(2)}ms preprocess=${stats.preprocessMs.toFixed(2)}ms`,
      `sort=${stats.sortMs.toFixed(2)}ms geometry_submit=${stats.pipelineMs.toFixed(2)}ms call=${stats.callMs.toFixed(2)}ms`,
    ].join("\n");
  } else {
    state.rendererStatus =
      `state=rendering frames=${state.frameCounter} ` +
      `visible=${visible} drawn=${drawn}/${visible} ` +
      `projected=${stats.projectedPolicy}/${stats.projectedExecution} ` +
      `projected_state=${stats.projectedAdaptiveState} ` +
      `frame=${stats.frameMs.toFixed(2)}ms preprocess=${stats.preprocessMs.toFixed(2)}ms ` +
      `sort=${stats.sortMs.toFixed(2)}ms geometry_submit=${stats.pipelineMs.toFixed(2)}ms call=${stats.callMs.toFixed(2)}ms`;
  }
  els.statusLine.textContent = buildStatusText();
}

function buildStatusText() {
  const backend = usingWasm() ? "wasm-wgpu" : "webgl2";
  const lines = [
    "gsplat web example",
    `api=${API_VERSION}`,
    `surface=${backend} realtime ${state.surfaceSizeLabel}`,
    state.rendererStatus,
    state.cameraStatus,
  ];
  if (!usingWasm() && state.wasmUnavailableReason) {
    lines.push(`wasm=${state.wasmUnavailableReason}`);
  }
  if (state.benchmark?.enabled) {
    lines.push(
      `benchmark=orbit frames=${state.benchmark.frames} warmup=${state.benchmark.warmupFrames}`,
    );
  }
  lines.push(`dataset=${state.scene?.name ?? "pending"}`);
  lines.push(`path=${state.datasetPath}`);
  return lines.join("\n");
}

function setStatus(message) {
  state.rendererStatus = message;
  els.statusLine.textContent = buildStatusText();
}

function invalidateSortedOrder() {
  state.lastSortedOrder = [];
  state.sortFrameCounter = 0;
}

function createProgram(gl, vertexSource, fragmentSource) {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, vertexSource);
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, fragmentSource);
  if (!vertex || !fragment) {
    return null;
  }
  const program = gl.createProgram();
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    setStatus(`state=program_link_failed error=${compactMessage(gl.getProgramInfoLog(program) || "link failed")}`);
    return null;
  }
  return program;
}

function compileShader(gl, type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    setStatus(`state=shader_compile_failed error=${compactMessage(gl.getShaderInfoLog(shader) || "compile failed")}`);
    return null;
  }
  return shader;
}

function bindAttribute(gl, program, name, size, stride, offset) {
  const location = gl.getAttribLocation(program, name);
  gl.enableVertexAttribArray(location);
  gl.vertexAttribPointer(location, size, gl.FLOAT, false, stride, offset);
}

function finiteNumber(value, field) {
  if (!Number.isFinite(value)) {
    throw new Error(`non-finite field ${field}`);
  }
  return value;
}

function parseNumericToken(value) {
  if (/^\+?inf(?:inity)?$/i.test(value)) {
    return Number.POSITIVE_INFINITY;
  }
  if (/^-inf(?:inity)?$/i.test(value)) {
    return Number.NEGATIVE_INFINITY;
  }
  return Number(value);
}

function normalizeOpacityLogit(value) {
  if (Number.isFinite(value)) {
    return value;
  }
  if (value === Number.POSITIVE_INFINITY) {
    return OPACITY_LOGIT_LIMIT;
  }
  if (value === Number.NEGATIVE_INFINITY) {
    return -OPACITY_LOGIT_LIMIT;
  }
  throw new Error("non-finite field opacity");
}

function finiteOrDefault(value, fallback) {
  return Number.isFinite(value) ? value : fallback;
}

function clampInt(value, min, max, fallback) {
  if (!Number.isFinite(value)) {
    return fallback;
  }
  return Math.trunc(clamp(value, min, max));
}

function sigmoid(value) {
  return 1 / (1 + Math.exp(-value));
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function normalize(v) {
  const len = Math.hypot(v[0], v[1], v[2]);
  if (len <= 0) {
    return [NaN, NaN, NaN];
  }
  return [v[0] / len, v[1] / len, v[2] / len];
}

function cross(a, b) {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

function formatCompact(value) {
  if (value >= 1000) {
    return `${Math.round(value / 1000)}k`;
  }
  return String(value);
}

function formatNumber(value) {
  if (value == null) return "Unavailable";
  return new Intl.NumberFormat("en-US").format(value);
}

function formatLabel(value) {
  return String(value).replaceAll("_", " ");
}

function formatBytes(value) {
  if (value >= 1024 * 1024) {
    return `${(value / (1024 * 1024)).toFixed(value >= 10 * 1024 * 1024 ? 0 : 1)} MB`;
  }
  if (value >= 1024) {
    return `${Math.round(value / 1024)} KB`;
  }
  return `${value} B`;
}

function sceneTitle(name) {
  if (name === "kitune1.ply") {
    return "Kitsune shrine";
  }
  if (name === "flowers_1.ply") {
    return "NVIDIA flowers";
  }
  if (name === "minimal_ascii.ply") {
    return "Minimal smoke scene";
  }
  if (["bonsai.ply", "truck.ply", "garden.ply", "bicycle.ply"].includes(name)) {
    return `INRIA ${name.slice(0, -4)}`;
  }
  return name;
}

async function readResponseBytes(response, name) {
  const total = Number(response.headers.get("content-length") ?? 0);
  if (!response.body || !Number.isFinite(total) || total <= 0) {
    return new Uint8Array(await response.arrayBuffer());
  }

  const bytes = new Uint8Array(total);
  const reader = response.body.getReader();
  let received = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    if (received + value.byteLength > bytes.byteLength) {
      throw new Error("scene response exceeded its declared size");
    }
    bytes.set(value, received);
    received += value.byteLength;
    setLoadingProgress(
      `Loading ${sceneTitle(name)}`,
      `${formatBytes(received)} of ${formatBytes(total)}`,
      0.04 + (received / total) * 0.68,
    );
  }
  return received === bytes.byteLength ? bytes : bytes.slice(0, received);
}

function setLoadingProgress(title, meta, progress) {
  els.loadingTitle.textContent = title;
  els.loadingMeta.textContent = meta;
  els.loadingBar.style.width = `${clamp(progress, 0, 1) * 100}%`;
  els.loadingOverlay.classList.remove("is-hidden");
}

function hideLoading() {
  els.loadingOverlay.classList.add("is-hidden");
}

function initTheme() {
  let stored = "";
  try {
    stored = window.localStorage.getItem("gsplat-showcase-theme") ?? "";
  } catch {
    stored = "";
  }
  setTheme(stored === "light" ? "light" : "dark", false);
}

function setTheme(theme, persist) {
  document.documentElement.dataset.theme = theme;
  const dark = theme === "dark";
  els.themeToggle.textContent = dark ? "Light mode" : "Dark mode";
  els.themeToggle.setAttribute("aria-label", dark ? "Switch to light mode" : "Switch to dark mode");
  if (persist) {
    try {
      window.localStorage.setItem("gsplat-showcase-theme", theme);
    } catch {
      // The theme still applies when storage is unavailable.
    }
  }
}

function compactMessage(error) {
  return String(error?.message ?? error ?? "unknown")
    .replaceAll("\n", " ")
    .slice(0, 160);
}

function emitStructuredSceneFailure(dataset, error) {
  if (!error || typeof error !== "object" || reportedStructuredFailures.has(error)) {
    return;
  }
  if (typeof error.stage !== "string"
      || typeof error.error_code !== "string"
      || typeof error.error_message !== "string"
      || error.scene_published !== false) {
    return;
  }
  reportedStructuredFailures.add(error);
  const resource = error.resource && typeof error.resource === "object"
    ? {
        kind: String(error.resource.kind),
        required_bytes: Number(error.resource.required_bytes),
        limit_bytes: Number(error.resource.limit_bytes),
      }
    : null;
  console.error(`SCENE_LOAD_FAILURE_JSON ${JSON.stringify({
    record_type: "scene_construction_failure",
    dataset,
    stage: error.stage,
    error_code: error.error_code,
    error_message: error.error_message,
    scene_published: false,
    ...(resource ? { resource } : {}),
  })}`);
}

function emitStructuredResizeFailure(error) {
  if (!error || typeof error !== "object" || reportedStructuredFailures.has(error)) {
    return;
  }
  if (error.stage !== "resize"
      || typeof error.error_code !== "string"
      || typeof error.error_message !== "string"
      || error.scene_published !== true) {
    return;
  }
  reportedStructuredFailures.add(error);
  console.error(`RUNTIME_RESIZE_FAILURE_JSON ${JSON.stringify({
    record_type: "runtime_resize_failure",
    dataset: state.scene?.name ?? null,
    stage: "resize",
    error_code: error.error_code,
    error_message: error.error_message,
    scene_published: true,
    previous_surface_size: state.surfaceSizeLabel,
    ...(error.resource ? { resource: error.resource } : {}),
  })}`);
}
