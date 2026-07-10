import {
  createGsplatRenderer,
  initGsplatWeb,
} from "../../../packages/web/src/index.js?v=sorted-index-20260710";

const API_VERSION = "0.1";
const MAX_SURFACE_SIDE = 1600;
const ORBIT_RADIANS_PER_SCREEN = 3.2;
const DOUBLE_TAP_TIMEOUT_MS = 300;
const DOUBLE_TAP_SLOP_PX = 48;
const DEFAULT_BENCHMARK_FRAMES = 120;
const DEFAULT_BENCHMARK_WARMUP_FRAMES = 10;
const DEFAULT_BENCHMARK_YAW_STEP = 0.001;
const OPACITY_LOGIT_LIMIT = 16;

const DATASETS = {
  showcase: "/tests/datasets/external/wakufactory_kitune/kitune1.ply",
  minimal: "/tests/datasets/minimal_ascii.ply",
  flowers: "/tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply",
};

const WASM_ENTRY = new URL("../pkg/gsplat_web.js?v=sorted-index-20260710", import.meta.url);
const WASM_BINARY = new URL("../pkg/gsplat_web_bg.wasm?v=sorted-index-20260710", import.meta.url);

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
  surfaceSizeLabel: "pending",
  datasetPath: "pending",
  startDataset: "showcase",
  benchmark: null,
  autoStartBenchmark: false,
  autoBenchmarkSync: false,
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

void main();

async function main() {
  initTheme();
  applyUrlConfig();
  updateControlLabels();
  await initWasmModule();
  if (state.backend !== "wasm") {
    initRenderer();
  }
  bindEvents();
  resizeCanvas();
  await loadStartupDataset();
  requestAnimationFrame(frame);
}

async function loadStartupDataset() {
  const requested =
    state.startDataset === "minimal"
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
    const loaded = await loadDataset(dataset.path, dataset.name, {
      allowFallback: index < requested.length - 1,
    });
    if (loaded) {
      return;
    }
  }
}

async function initWasmModule() {
  try {
    const response = await fetch(WASM_ENTRY, { method: "HEAD" });
    if (!response.ok) {
      state.backend = "webgl";
      state.wasmUnavailableReason = `pkg_missing_http_${response.status}`;
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
    state.backend = "webgl";
    state.wasmUnavailableReason = compactMessage(error);
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
  setRenderMode("WebGL2 fallback point splats");
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
  state.backend = "webgl";
  initRenderer();
  if (state.gl && state.program) {
    els.gpuStatus.textContent = "ready";
    setRenderMode("WebGL2 fallback point splats");
    updateBackendControls();
  }
}

function disposeWasmRenderer() {
  const renderer = state.wasmRenderer;
  state.wasmRenderer = null;
  renderer?.free?.();
}

async function createWasmRenderer(scene) {
  if (!state.wasmModule || !scene.rawBytes) {
    return false;
  }

  disposeWasmRenderer();
  setStatus(`state=wasm_creating dataset=${scene.name}`);
  try {
    const renderer = await createGsplatRenderer({
      module: state.wasmModule,
      canvas: els.canvas,
      plyBytes: scene.rawBytes,
      width: els.canvas.width,
      height: els.canvas.height,
      sortInterval: Number(els.sortInterval.value),
    });
    state.wasmRenderer = renderer;
    state.backend = "wasm";
    state.wasmUnavailableReason = "";

    const summary = renderer.sceneSummary();
    const surface = renderer.surfaceSize();
    state.surfaceSizeLabel = `${surface.width}x${surface.height}`;
    els.gpuStatus.textContent = "wgpu";
    els.gaussianCount.textContent = formatNumber(summary.gaussians ?? scene.count);
    els.shDegree.textContent = String(summary.shDegree ?? scene.shDegree);
    setRenderMode("Rust/WASM sorted-index direct");
    updateBackendControls();
    return true;
  } catch (error) {
    state.wasmUnavailableReason = compactMessage(error);
    disposeWasmRenderer();
    ensureFallbackRenderer();
    updateBackendControls();
    setStatus(`state=wasm_create_failed fallback=webgl error=${state.wasmUnavailableReason}`);
    return false;
  }
}

function resizeWasmRenderer() {
  if (!state.wasmRenderer) {
    return;
  }
  try {
    state.wasmRenderer.resize(els.canvas.width, els.canvas.height);
    const surface = state.wasmRenderer.surfaceSize();
    state.surfaceSizeLabel = `${surface.width}x${surface.height}`;
  } catch (error) {
    state.wasmUnavailableReason = compactMessage(error);
    setStatus(`state=wasm_resize_failed error=${state.wasmUnavailableReason}`);
  }
}

function updateBackendControls() {
  const wasmActive = usingWasm();
  els.backendBadge.textContent = state.backend === "wasm" ? "WASM surface" : "WebGL fallback";
  els.drawBudget.disabled = wasmActive;
  els.pointScale.disabled = wasmActive;
}

function bindEvents() {
  window.addEventListener("resize", resizeCanvas);
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
  state.wasmRenderer?.orbit(deltaYaw, deltaPitch);
  state.camera.yaw += deltaYaw;
  state.camera.pitch = clamp(state.camera.pitch + deltaPitch, -1.35, 1.35);
  invalidateSortedOrder();
}

function zoomCamera(distanceScale) {
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
  setLoadingProgress(`Loading ${sceneTitle(name)}`, "Fetching scene data.", 0.04);
  setStatus(`state=loading dataset=${name}`);
  try {
    const response = await fetch(path);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    const bytes = await readResponseBytes(response, name);
    setLoadingProgress("Reading captured light", `${formatBytes(bytes.byteLength)} received.`, 0.76);
    const scene = parsePly(bytes, name, path);
    setLoadingProgress(
      "Building the scene",
      `${formatNumber(scene.count)} Gaussians ready for the GPU.`,
      0.86,
    );
    await applyScene(scene);
    return true;
  } catch (error) {
    setStatus(`state=load_failed dataset=${name} error=${compactMessage(error)}`);
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

async function loadFile(file) {
  setLoadingProgress(`Opening ${file.name}`, "Reading the local PLY in your browser.", 0.12);
  setStatus(`state=loading dataset=${file.name}`);
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    setLoadingProgress("Reading captured light", `${formatBytes(bytes.byteLength)} received.`, 0.76);
    const scene = parsePly(bytes, file.name, `browser:${file.name}`);
    await applyScene(scene);
  } catch (error) {
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
    await createWasmRenderer(scene);
  } else {
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
  if (!state.scene) {
    return;
  }
  fitCameraToScene(state.scene);
  state.wasmRenderer?.resetCamera();
  state.cameraStatus = "camera=reset";
}

function frame(now) {
  const dt = Math.min((now - state.lastFrameTime) / 1000, 0.05);
  state.lastFrameTime = now;
  state.fps = dt > 0 ? 1 / dt : state.fps;

  if (state.benchmark?.enabled) {
    orbitCamera(state.benchmark.yawStep, 0);
    state.cameraStatus = "camera=benchmark_orbit";
  } else if (state.autoOrbit && state.scene) {
    orbitCamera(dt * 0.22, 0);
  }

  const stats = render();
  if (stats && state.benchmark?.enabled) {
    recordBenchmark(stats);
  }
  requestAnimationFrame(frame);
}

function render() {
  if (usingWasm()) {
    return renderWasm();
  }
  return renderWebgl();
}

function renderWasm() {
  if (!state.scene || !state.wasmRenderer) {
    return null;
  }

  state.frameCounter += 1;
  const callStart = performance.now();
  try {
    const raw = state.wasmRenderer.renderFrame();
    const callMs = performance.now() - callStart;
    const surfaceWidth = raw.surfaceWidth ?? els.canvas.width;
    const surfaceHeight = raw.surfaceHeight ?? els.canvas.height;
    state.surfaceSizeLabel = `${surfaceWidth}x${surfaceHeight}`;

    const stats = {
      visible: raw.visibleCount ?? 0,
      drawn: raw.drawnCount ?? 0,
      preprocessMs: raw.preprocessMs ?? 0,
      sortMs: raw.sortMs ?? 0,
      pipelineMs: (raw.cpuGeometryMs ?? raw.rasterMs ?? 0) + (raw.renderSubmitMs ?? 0),
      frameMs: raw.frameMs ?? callMs,
      callMs,
    };
    updateFrameStats(stats);
    updateStatusOverlay(stats);
    return stats;
  } catch (error) {
    state.wasmUnavailableReason = compactMessage(error);
    state.wasmRenderer = null;
    ensureFallbackRenderer();
    updateBackendControls();
    setStatus(`state=wasm_render_failed fallback=webgl error=${state.wasmUnavailableReason}`);
    return renderWebgl();
  }
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

function resizeCanvas() {
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  let width = Math.max(1, Math.floor(els.canvas.clientWidth * ratio));
  let height = Math.max(1, Math.floor(els.canvas.clientHeight * ratio));
  const maxSide = Math.max(width, height);
  if (maxSide > MAX_SURFACE_SIDE) {
    const scale = MAX_SURFACE_SIDE / maxSide;
    width = Math.max(1, Math.floor(width * scale));
    height = Math.max(1, Math.floor(height * scale));
  }

  if (els.canvas.width !== width || els.canvas.height !== height) {
    els.canvas.width = width;
    els.canvas.height = height;
    state.surfaceSizeLabel = `${width}x${height}`;
    if (usingWasm()) {
      resizeWasmRenderer();
    } else {
      invalidateSortedOrder();
    }
  }
  els.surfaceSize.textContent = state.surfaceSizeLabel;
  if (state.scene && state.frameCounter === 0) {
    fitCameraToScene(state.scene);
  }
}

function startBenchmark() {
  if (!state.scene) {
    setStatus("state=benchmark_waiting_for_scene");
    return;
  }

  const benchmark = createBenchmarkState(true);
  state.benchmark = benchmark;
  setAutoOrbit(false);
  els.benchmarkStatus.textContent = "running";
  els.runBenchmark.disabled = true;
  els.benchmarkResult.textContent = "Running benchmark orbit...";
  setStatus(`state=benchmark_running frames=${benchmark.frames} warmup=${benchmark.warmupFrames}`);
}

function runBenchmarkSync() {
  if (!state.scene) {
    setStatus("state=benchmark_waiting_for_scene");
    return;
  }

  const benchmark = createBenchmarkState(false);
  setAutoOrbit(false);
  els.benchmarkStatus.textContent = "running";
  els.runBenchmark.disabled = true;
  els.benchmarkResult.textContent = "Running sync benchmark orbit...";

  const totalFrames = benchmark.frames + benchmark.warmupFrames;
  for (let i = 0; i < totalFrames; i += 1) {
    orbitCamera(benchmark.yawStep, 0);
    state.cameraStatus = "camera=benchmark_orbit";
    const stats = render();
    benchmark.observedFrames += 1;
    if (stats && i >= benchmark.warmupFrames) {
      accumulateBenchmark(benchmark, stats);
    }
  }

  finishBenchmark(benchmark);
  state.benchmark = null;
}

function createBenchmarkState(enabled) {
  const frames = clampInt(Number(els.benchmarkFrames.value), 1, 2000, DEFAULT_BENCHMARK_FRAMES);
  const warmupFrames = clampInt(Number(els.benchmarkWarmup.value), 0, 500, DEFAULT_BENCHMARK_WARMUP_FRAMES);
  const yawStep = finiteOrDefault(Number(els.benchmarkYaw.value), DEFAULT_BENCHMARK_YAW_STEP);
  return {
    enabled,
    frames,
    warmupFrames,
    yawStep,
    observedFrames: 0,
    samples: 0,
    totalCallMs: 0,
    totalFrameMs: 0,
    totalPreprocessMs: 0,
    totalSortMs: 0,
    totalPipelineMs: 0,
    totalVisible: 0,
    totalDrawn: 0,
  };
}

function applyUrlConfig() {
  const params = new URLSearchParams(window.location.search);
  setNumberInputFromParam(els.benchmarkFrames, params.get("gsplat_benchmark_frames") ?? params.get("benchmark_frames"));
  setNumberInputFromParam(
    els.benchmarkWarmup,
    params.get("gsplat_benchmark_warmup_frames") ?? params.get("benchmark_warmup"),
  );
  setNumberInputFromParam(els.benchmarkYaw, params.get("gsplat_benchmark_yaw_step") ?? params.get("benchmark_yaw_step"));
  setNumberInputFromParam(els.sortInterval, params.get("gsplat_surface_sort_interval") ?? params.get("sort_interval"));
  setNumberInputFromParam(els.drawBudget, params.get("draw_budget"));
  const dataset = (params.get("dataset") ?? params.get("scene") ?? "").toLowerCase();
  if (dataset === "flowers" || dataset === "flower") {
    state.startDataset = "flowers";
  } else if (dataset === "minimal" || dataset === "smoke") {
    state.startDataset = "minimal";
  } else if (["showcase", "kitsune", "kitune", "fox"].includes(dataset)) {
    state.startDataset = "showcase";
  }
  state.autoStartBenchmark = ["1", "true", "yes"].includes(
    (params.get("gsplat_benchmark") ?? params.get("benchmark") ?? "").toLowerCase(),
  );
  state.autoBenchmarkSync = ["1", "true", "yes"].includes(
    (params.get("gsplat_benchmark_sync") ?? params.get("benchmark_sync") ?? "").toLowerCase(),
  );
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
  benchmark.observedFrames += 1;
  if (benchmark.observedFrames <= benchmark.warmupFrames) {
    return;
  }

  accumulateBenchmark(benchmark, stats);

  if (benchmark.samples < benchmark.frames) {
    return;
  }

  finishBenchmark(benchmark);
}

function accumulateBenchmark(benchmark, stats) {
  benchmark.samples += 1;
  benchmark.totalCallMs += stats.callMs;
  benchmark.totalFrameMs += stats.frameMs;
  benchmark.totalPreprocessMs += stats.preprocessMs;
  benchmark.totalSortMs += stats.sortMs;
  benchmark.totalPipelineMs += stats.pipelineMs;
  benchmark.totalVisible += stats.visible;
  benchmark.totalDrawn += stats.drawn;
}

function finishBenchmark(benchmark) {
  benchmark.enabled = false;
  const result = benchmarkResultLine(benchmark);
  console.info(result);
  els.benchmarkStatus.textContent = "complete";
  els.runBenchmark.disabled = false;
  els.benchmarkResult.textContent = result;
  setStatus(`state=benchmark_complete ${result}`);
}

function wasmRendererLabel() {
  if (!usingWasm()) {
    return "webgl2_point_splats";
  }
  return "wasm_sorted_index_direct";
}

function benchmarkResultLine(benchmark) {
  const samples = Math.max(benchmark.samples, 1);
  const avg = (value) => (value / samples).toFixed(3);
  return (
    `BENCHMARK_RESULT dataset=${state.scene.name} ` +
    `samples=${benchmark.samples} warmup=${benchmark.warmupFrames} ` +
    `sort_interval=${Number(els.sortInterval.value)} renderer=${wasmRendererLabel()} ` +
    `draw_budget=${usingWasm() ? "full" : Number(els.drawBudget.value)} ` +
    `avg_call_ms=${avg(benchmark.totalCallMs)} ` +
    `avg_frame_ms=${avg(benchmark.totalFrameMs)} ` +
    `avg_preprocess_ms=${avg(benchmark.totalPreprocessMs)} ` +
    `avg_sort_ms=${avg(benchmark.totalSortMs)} ` +
    `avg_geometry_submit_cpu_wall_ms=${avg(benchmark.totalPipelineMs)} ` +
    `avg_visible=${Math.round(benchmark.totalVisible / samples)} ` +
    `avg_drawn=${Math.round(benchmark.totalDrawn / samples)}`
  );
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
  if (window.innerWidth < 620) {
    state.rendererStatus = [
      `state=rendering frames=${state.frameCounter}`,
      `visible=${stats.visible} drawn=${stats.drawn}/${stats.visible}`,
      `frame=${stats.frameMs.toFixed(2)}ms preprocess=${stats.preprocessMs.toFixed(2)}ms`,
      `sort=${stats.sortMs.toFixed(2)}ms geometry_submit=${stats.pipelineMs.toFixed(2)}ms call=${stats.callMs.toFixed(2)}ms`,
    ].join("\n");
  } else {
    state.rendererStatus =
      `state=rendering frames=${state.frameCounter} ` +
      `visible=${stats.visible} drawn=${stats.drawn}/${stats.visible} ` +
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
