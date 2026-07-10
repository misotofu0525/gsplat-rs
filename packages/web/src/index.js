export const GSPLAT_WEB_SDK_VERSION = "0.1.1";

let loadedModule = null;
let initPromise = null;

export async function initGsplatWeb(options = {}) {
  const { module, moduleUrl = "./wasm/gsplat_web.js", wasmUrl } = options;
  if (module) {
    await module.default(wasmInitInput(wasmUrl));
    loadedModule = module;
    initPromise = Promise.resolve(module);
    return module;
  }

  if (!initPromise) {
    initPromise = import(moduleUrl).then(async (importedModule) => {
      await importedModule.default(wasmInitInput(wasmUrl));
      loadedModule = importedModule;
      return importedModule;
    });
  }
  return initPromise;
}

function wasmInitInput(wasmUrl) {
  return wasmUrl === undefined ? undefined : { module_or_path: wasmUrl };
}

export function getGsplatApiVersion(module = loadedModule) {
  const resolvedModule = requireModule(module);
  return {
    major: Number(resolvedModule.api_version_major()),
    minor: Number(resolvedModule.api_version_minor()),
  };
}

export async function createGsplatRenderer(options) {
  const {
    canvas,
    plyBytes,
    width = canvas?.width,
    height = canvas?.height,
    sortInterval = 2,
    module,
  } = options ?? {};

  assertCanvas(canvas);
  assertPositiveInteger(width, "width");
  assertPositiveInteger(height, "height");
  const bytes = normalizeBytes(plyBytes);
  const resolvedModule = module ?? loadedModule ?? (await initGsplatWeb());
  const nativeRenderer = await resolvedModule.createRenderer(canvas, bytes, width, height);
  const renderer = new GsplatWebRenderer(nativeRenderer);
  renderer.setSortInterval(sortInterval);
  return renderer;
}

export async function createGsplatRendererFromUrl(options) {
  const { url, fetchOptions, ...rendererOptions } = options ?? {};
  if (!url) {
    throw new TypeError("createGsplatRendererFromUrl requires url");
  }

  const response = await fetch(url, fetchOptions);
  if (!response.ok) {
    throw new Error(`failed to fetch PLY: ${response.status} ${response.statusText}`);
  }

  return createGsplatRenderer({
    ...rendererOptions,
    plyBytes: await response.arrayBuffer(),
  });
}

export class GsplatWebRenderer {
  #nativeRenderer;
  #disposed = false;

  constructor(nativeRenderer) {
    if (!nativeRenderer) {
      throw new TypeError("GsplatWebRenderer requires a native renderer");
    }
    this.#nativeRenderer = nativeRenderer;
  }

  get isDisposed() {
    return this.#disposed;
  }

  resize(width, height) {
    const nativeRenderer = this.#requireNativeRenderer();
    assertPositiveInteger(width, "width");
    assertPositiveInteger(height, "height");
    nativeRenderer.resize(width, height);
  }

  resetCamera() {
    this.#requireNativeRenderer().resetCamera();
  }

  orbit(deltaYawRadians, deltaPitchRadians) {
    const nativeRenderer = this.#requireNativeRenderer();
    assertFinite(deltaYawRadians, "deltaYawRadians");
    assertFinite(deltaPitchRadians, "deltaPitchRadians");
    nativeRenderer.orbit(deltaYawRadians, deltaPitchRadians);
  }

  zoom(distanceScale) {
    const nativeRenderer = this.#requireNativeRenderer();
    assertFinite(distanceScale, "distanceScale");
    if (distanceScale <= 0) {
      throw new RangeError("distanceScale must be greater than zero");
    }
    nativeRenderer.zoom(distanceScale);
  }

  pan(normalizedDeltaX, normalizedDeltaY) {
    const nativeRenderer = this.#requireNativeRenderer();
    assertFinite(normalizedDeltaX, "normalizedDeltaX");
    assertFinite(normalizedDeltaY, "normalizedDeltaY");
    nativeRenderer.pan(normalizedDeltaX, normalizedDeltaY);
  }

  setSortInterval(interval) {
    const nativeRenderer = this.#requireNativeRenderer();
    assertPositiveInteger(interval, "interval");
    nativeRenderer.setSortInterval(interval);
  }

  renderFrame() {
    return normalizeFrameStats(this.#requireNativeRenderer().renderFrame());
  }

  sceneSummary() {
    const summary = this.#requireNativeRenderer().sceneSummary();
    return {
      gaussians: numberOr(summary.gaussians, 0),
      shDegree: numberOr(summary.shDegree, 0),
      hasShRest: Boolean(summary.hasShRest),
    };
  }

  surfaceSize() {
    const surface = this.#requireNativeRenderer().surfaceSize();
    return {
      width: numberOr(surface.width, 0),
      height: numberOr(surface.height, 0),
    };
  }

  free() {
    if (this.#disposed) {
      return;
    }
    this.#nativeRenderer?.free?.();
    this.#nativeRenderer = null;
    this.#disposed = true;
  }

  dispose() {
    this.free();
  }

  #requireNativeRenderer() {
    if (this.#disposed || !this.#nativeRenderer) {
      throw new Error("GsplatWebRenderer is disposed");
    }
    return this.#nativeRenderer;
  }
}

function normalizeFrameStats(raw) {
  return {
    frameMs: numberOr(raw.frameMs, 0),
    preprocessMs: numberOr(raw.preprocessMs, 0),
    sortMs: numberOr(raw.sortMs, 0),
    rasterMs: numberOr(raw.rasterMs, 0),
    visibleCount: numberOr(raw.visibleCount, 0),
    drawnCount: numberOr(raw.drawnCount, 0),
    refreshSort: Boolean(raw.refreshSort),
    surfaceWidth: numberOr(raw.surfaceWidth, 0),
    surfaceHeight: numberOr(raw.surfaceHeight, 0),
  };
}

function normalizeBytes(value) {
  if (value instanceof Uint8Array) {
    return value;
  }
  if (value instanceof ArrayBuffer) {
    return new Uint8Array(value);
  }
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  throw new TypeError("plyBytes must be a Uint8Array, ArrayBuffer, or ArrayBufferView");
}

function requireModule(module) {
  if (!module) {
    throw new Error("gsplat-web wasm module is not initialized");
  }
  return module;
}

function assertCanvas(canvas) {
  if (!canvas || typeof canvas.width !== "number" || typeof canvas.height !== "number") {
    throw new TypeError("canvas must be an HTMLCanvasElement-like object");
  }
}

function assertFinite(value, name) {
  if (!Number.isFinite(value)) {
    throw new TypeError(`${name} must be finite`);
  }
}

function assertPositiveInteger(value, name) {
  if (!Number.isInteger(value) || value <= 0) {
    throw new RangeError(`${name} must be a positive integer`);
  }
}

function numberOr(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}
