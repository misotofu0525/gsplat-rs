export const GSPLAT_WEB_SDK_VERSION = "0.1.3";

const GEOMETRY_PATH_IDS = Object.freeze({
  direct: 0,
  packed: 1,
  paged: 2,
});

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
    geometryPath = "direct",
    module,
  } = options ?? {};

  assertCanvas(canvas);
  assertPositiveInteger(width, "width");
  assertPositiveInteger(height, "height");
  const bytes = normalizeBytes(plyBytes);
  const geometryPathId = resolveGeometryPathId(geometryPath);
  const resolvedModule = module ?? loadedModule ?? (await initGsplatWeb());
  const nativeRenderer = geometryPathId === GEOMETRY_PATH_IDS.direct
    ? await resolvedModule.createRenderer(canvas, bytes, width, height)
    : await createRendererWithGeometryPath(
        resolvedModule,
        canvas,
        bytes,
        width,
        height,
        geometryPathId,
      );
  const renderer = new GsplatWebRenderer(nativeRenderer);
  renderer.setSortInterval(sortInterval);
  return renderer;
}

async function createRendererWithGeometryPath(
  module,
  canvas,
  bytes,
  width,
  height,
  geometryPathId,
) {
  if (typeof module.createRendererWithGeometryPath !== "function") {
    throw new Error(
      "the loaded gsplat-web module does not support constructor-time geometry selection",
    );
  }
  return module.createRendererWithGeometryPath(
    canvas,
    bytes,
    width,
    height,
    geometryPathId,
  );
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

  setCamera(camera) {
    const position = camera?.position;
    const rotation = camera?.rotationXyzw;
    const intrinsics = camera?.intrinsics;
    if (!Array.isArray(position) || position.length !== 3) {
      throw new TypeError("camera.position must contain three numbers");
    }
    if (!Array.isArray(rotation) || rotation.length !== 4) {
      throw new TypeError("camera.rotationXyzw must contain four numbers");
    }
    const values = [
      ...position,
      ...rotation,
      intrinsics?.verticalFovRadians,
      intrinsics?.nearPlane,
      intrinsics?.farPlane,
    ];
    values.forEach((value, index) => assertFinite(value, `camera value ${index}`));
    this.#requireNativeRenderer().setCamera(new Float32Array(values));
  }

  cameraReceipt() {
    const values = Array.from(this.#requireNativeRenderer().cameraReceipt());
    return {
      position: values.slice(0, 3),
      rotationXyzw: values.slice(3, 7),
      intrinsics: {
        verticalFovRadians: values[7],
        nearPlane: values[8],
        farPlane: values[9],
      },
    };
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

  setGeometryPath(path) {
    const id = resolveGeometryPathId(path);
    this.#requireNativeRenderer().setGeometryPath(id);
  }

  rasterPath() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.rasterPath !== "function") {
      return "sorted_index_direct";
    }
    return String(nativeRenderer.rasterPath());
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

function resolveGeometryPathId(path) {
  const id = GEOMETRY_PATH_IDS[path];
  if (id === undefined) {
    throw new TypeError("geometryPath must be direct, packed, or paged");
  }
  return id;
}

function normalizeFrameStats(raw) {
  return {
    frameMs: numberOr(raw.frameMs, 0),
    preprocessMs: numberOr(raw.preprocessMs, 0),
    sortMs: numberOr(raw.sortMs, 0),
    rasterMs: numberOr(raw.rasterMs, 0),
    cpuGeometryMs: numberOr(raw.cpuGeometryMs, raw.rasterMs),
    renderSubmitMs: numberOr(raw.renderSubmitMs, 0),
    frameWallMs: numberOr(raw.frameWallMs, raw.frameMs),
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
