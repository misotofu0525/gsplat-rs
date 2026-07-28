export const GSPLAT_WEB_SDK_VERSION = "0.1.3";

const GEOMETRY_PATH_IDS = Object.freeze({
  direct: 0,
  packed: 1,
  paged: 2,
});

const ORDER_BACKEND_IDS = Object.freeze({
  cpu: 0,
  gpu: 1,
  adaptive: 2,
});

const PROJECTED_POLICY_IDS = Object.freeze({
  candidate: 0,
  compact: 1,
  adaptive: 2,
});
const GPU_ORDER_PRODUCER_IDS = Object.freeze({
  "post-sort": 0,
  preproject: 1,
});
const PROJECTED_EXECUTIONS = Object.freeze(["candidate", "compact"]);
const PROJECTED_ADAPTIVE_STATES = Object.freeze([
  "disabled",
  "candidate_learning",
  "candidate_stable",
  "compact_probe",
  "compact_stable",
  "candidate_probe",
  "candidate_only",
  "cooldown",
]);
const MIN_PROJECTED_TICKET = 2 ** 52;
const MIN_GPU_PRODUCER_TICKET = 2 ** 51;
const MAX_GPU_PRODUCER_TICKET = (2 ** 52) - 1;
const MAX_ORDER_TICKET = (2 ** 51) - 1;
const MIN_FOCAL_LENGTH_X_OVER_Y = 1 / 65_536;
const MAX_FOCAL_LENGTH_X_OVER_Y = 65_536;
const REQUIRED_SURFACE_DEVICE_LIMITS = Object.freeze([
  "maxBindGroups",
  "maxBindingsPerBindGroup",
  "maxBufferSize",
  "maxComputeInvocationsPerWorkgroup",
  "maxComputeWorkgroupStorageSize",
  "maxComputeWorkgroupsPerDimension",
  "maxStorageBufferBindingSize",
  "maxStorageBuffersPerShaderStage",
  "maxTextureDimension2D",
]);

const FAILURE_STAGES = Object.freeze({
  rendererCreate: "renderer_create",
  rendererConfigure: "renderer_configure",
  gpuOrderPrepare: "gpu_order_prepare",
  gpuOrderProducer: "gpu_order_producer",
  streamCreate: "stream_create",
  streamDecode: "stream_decode",
  streamFinish: "stream_finish",
  geometryPath: "geometry_path",
  projectedPolicy: "projected_policy",
  resize: "resize",
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

function normalizeDiagnosticSurfaceDeviceReceipt(raw) {
  if (raw?.schema !== "gsplat-renderer-surface-device/v1"
      || raw.provenance !== "renderer_owned_surface_session"
      || raw.adapterSelectionClass !== "high_performance"
      || raw.geometryPath !== "packed_atlas"
      || !Number.isSafeInteger(raw.addressableSplatCount)
      || raw.addressableSplatCount <= 0) {
    throw new TypeError("diagnostic Surface device receipt has invalid session provenance");
  }
  const adapter = raw.adapter;
  if (!adapter || typeof adapter !== "object"
      || typeof adapter.name !== "string"
      || !["available", "unavailable_wgpu28_web_backend"].includes(adapter.identityStatus)
      || (adapter.identityStatus === "available" && adapter.name.trim() === "")
      || (adapter.identityStatus === "unavailable_wgpu28_web_backend"
        && (adapter.backend !== "browser_webgpu" || adapter.name !== ""))
      || typeof adapter.backend !== "string" || adapter.backend.trim() === ""
      || typeof adapter.deviceType !== "string" || adapter.deviceType.trim() === ""
      || !Number.isSafeInteger(adapter.vendorId) || adapter.vendorId < 0
      || !Number.isSafeInteger(adapter.deviceId) || adapter.deviceId < 0
      || !Number.isSafeInteger(adapter.subgroupMinSize) || adapter.subgroupMinSize < 0
      || !Number.isSafeInteger(adapter.subgroupMaxSize) || adapter.subgroupMaxSize < 0
      || typeof adapter.driver !== "string"
      || typeof adapter.driverInfo !== "string"
      || typeof adapter.devicePciBusId !== "string"
      || typeof adapter.transientSavesMemory !== "boolean") {
    throw new TypeError("diagnostic Surface device receipt has invalid adapter identity");
  }
  const normalizeLimits = (value, label) => {
    const entries = Object.entries(value ?? {}).sort(([left], [right]) =>
      left.localeCompare(right));
    if (entries.length === 0 || entries.some(([name, limit]) =>
      !/^[a-z][A-Za-z0-9]*$/.test(name)
        || !Number.isSafeInteger(limit) || limit < 0)) {
      throw new TypeError(`diagnostic Surface device receipt has invalid ${label}`);
    }
    const names = new Set(entries.map(([name]) => name));
    if (REQUIRED_SURFACE_DEVICE_LIMITS.some((name) => !names.has(name))) {
      throw new TypeError(`diagnostic Surface device receipt lacks required ${label}`);
    }
    return Object.freeze(Object.fromEntries(entries));
  };
  const supportedAdapterLimits = normalizeLimits(
    raw.supportedAdapterLimits,
    "supported adapter limits",
  );
  const effectiveDeviceLimits = normalizeLimits(
    raw.effectiveDeviceLimits,
    "effective device limits",
  );
  return Object.freeze({
    schema: raw.schema,
    provenance: raw.provenance,
    adapterSelectionClass: raw.adapterSelectionClass,
    geometryPath: raw.geometryPath,
    addressableSplatCount: raw.addressableSplatCount,
    adapter: Object.freeze({ ...adapter }),
    supportedAdapterLimits,
    effectiveDeviceLimits,
  });
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
    sortInterval = 1,
    orderBackend = "adaptive",
    projectedPolicy = "adaptive",
    gpuOrderProducer = null,
    geometryPath = "packed",
    module,
  } = options ?? {};

  assertCanvas(canvas);
  assertPositiveInteger(width, "width");
  assertPositiveInteger(height, "height");
  assertPositiveInteger(sortInterval, "sortInterval");
  resolveOrderBackendId(orderBackend);
  resolveProjectedPolicyId(projectedPolicy);
  validateGpuProducerExperiment(gpuOrderProducer, geometryPath, projectedPolicy);
  const bytes = normalizeBytes(plyBytes);
  const geometryPathId = resolveGeometryPathId(geometryPath);
  const resolvedModule = module ?? loadedModule ?? (await initGsplatWeb());
  let nativeRenderer;
  try {
    nativeRenderer = await createRendererWithGeometryPath(
      resolvedModule,
      canvas,
      bytes,
      width,
      height,
      geometryPathId,
    );
  } catch (error) {
    throw structuredFailure(error, FAILURE_STAGES.rendererCreate);
  }
  return configureRenderer(
    nativeRenderer,
    sortInterval,
    orderBackend,
    projectedPolicy,
    gpuOrderProducer,
  );
}

async function configureRenderer(
  nativeRenderer,
  sortInterval,
  orderBackend,
  projectedPolicy,
  gpuOrderProducer,
) {
  const renderer = new GsplatWebRenderer(nativeRenderer);
  try {
    renderer.setSortInterval(sortInterval);
    if (orderBackend !== "cpu") {
      try {
        if (typeof nativeRenderer.prepareGpuOrder !== "function") {
          throw new Error(
            "the loaded gsplat-web module does not support transactional GPU order preparation",
          );
        }
        // Await transactional preparation before selecting GPU/Adaptive so a
        // validation or OOM scope failure cannot publish a half-ready scene.
        await nativeRenderer.prepareGpuOrder();
      } catch (error) {
        throw structuredFailure(error, FAILURE_STAGES.gpuOrderPrepare);
      }
    }
    renderer.setOrderBackend(orderBackend);
    applyNativeProjectedPolicy(nativeRenderer, projectedPolicy);
    if (gpuOrderProducer !== null) {
      try {
        if (typeof nativeRenderer.setGpuOrderProducerAsync !== "function") {
          throw new Error(
            "the loaded gsplat-web module does not support transactional GPU producer selection",
          );
        }
        await nativeRenderer.setGpuOrderProducerAsync(
          resolveGpuOrderProducerId(gpuOrderProducer),
        );
      } catch (error) {
        throw structuredFailure(error, FAILURE_STAGES.gpuOrderProducer);
      }
    }
    return renderer;
  } catch (error) {
    freeNative(renderer);
    throw structuredFailure(error, FAILURE_STAGES.rendererConfigure);
  }
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
  const { url, fetchOptions, onProgress, ...rendererOptions } = options ?? {};
  if (!url) {
    throw new TypeError("createGsplatRendererFromUrl requires url");
  }

  const response = await fetch(url, fetchOptions);
  if (!response.ok) {
    throw new Error(`failed to fetch PLY: ${response.status} ${response.statusText}`);
  }

  const {
    canvas,
    width = canvas?.width,
    height = canvas?.height,
    sortInterval = 1,
    orderBackend = "adaptive",
    projectedPolicy = "adaptive",
    gpuOrderProducer = null,
    geometryPath = "packed",
    module,
  } = rendererOptions;
  const geometryPathId = resolveGeometryPathId(geometryPath);
  if (geometryPathId === GEOMETRY_PATH_IDS.packed) {
    if (!response.body || typeof response.body.getReader !== "function") {
      throw new Error("this browser response does not expose a readable PLY byte stream");
    }
    const totalBytesHeader = Number(response.headers?.get?.("content-length") ?? 0);
    const totalBytes = Number.isFinite(totalBytesHeader) && totalBytesHeader > 0
      ? totalBytesHeader
      : null;
    return createGsplatRendererFromStream({
      canvas,
      stream: response.body,
      totalBytes,
      onProgress,
      width,
      height,
      sortInterval,
      orderBackend,
      projectedPolicy,
      gpuOrderProducer,
      geometryPath,
      module,
    });
  }

  const bytes = await response.arrayBuffer();
  onProgress?.({ receivedBytes: bytes.byteLength, totalBytes: bytes.byteLength });
  return createGsplatRenderer({
    ...rendererOptions,
    geometryPath,
    module,
    plyBytes: bytes,
  });
}

export async function createGsplatRendererFromStream(options) {
  const {
    stream: readable,
    totalBytes = null,
    onProgress,
    canvas,
    width = canvas?.width,
    height = canvas?.height,
    sortInterval = 1,
    orderBackend = "adaptive",
    projectedPolicy = "adaptive",
    gpuOrderProducer = null,
    geometryPath = "packed",
    module,
  } = options ?? {};
  if (resolveGeometryPathId(geometryPath) !== GEOMETRY_PATH_IDS.packed) {
    throw new TypeError("streamed scene construction supports only exact Packed geometry");
  }
  if (!readable || typeof readable.getReader !== "function") {
    throw new TypeError("createGsplatRendererFromStream requires a ReadableStream");
  }
  assertCanvas(canvas);
  assertPositiveInteger(width, "width");
  assertPositiveInteger(height, "height");
  assertPositiveInteger(sortInterval, "sortInterval");
  resolveOrderBackendId(orderBackend);
  resolveProjectedPolicyId(projectedPolicy);
  validateGpuProducerExperiment(gpuOrderProducer, geometryPath, projectedPolicy);
  const resolvedModule = module ?? loadedModule ?? (await initGsplatWeb());
  if (typeof resolvedModule.createPackedPlyStream !== "function") {
    throw structuredFailure(
      new Error(
        "the loaded gsplat-web module does not support allocation-bounded Packed PLY streaming",
      ),
      FAILURE_STAGES.streamCreate,
    );
  }

  const reader = readable.getReader();
  let nativeStream = null;
  let nativeRenderer = null;
  let receivedBytes = 0;
  let scenePublished = false;
  let stage = FAILURE_STAGES.streamCreate;
  try {
    nativeStream = resolvedModule.createPackedPlyStream(canvas, width, height);
    stage = FAILURE_STAGES.streamDecode;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      const bytes = normalizeBytes(value);
      nativeStream.pushChunk(bytes);
      receivedBytes += bytes.byteLength;
      onProgress?.({ receivedBytes, totalBytes });
    }
    stage = FAILURE_STAGES.streamFinish;
    nativeRenderer = await nativeStream.finish();
    const rendererHandle = nativeRenderer;
    nativeRenderer = null;
    const renderer = await configureRenderer(
      rendererHandle,
      sortInterval,
      orderBackend,
      projectedPolicy,
      gpuOrderProducer,
    );
    scenePublished = true;
    return renderer;
  } catch (error) {
    // configureRenderer owns cleanup after it wraps the native handle. This
    // fallback covers a native finish implementation that returned a handle
    // but failed before the wrapper could take ownership.
    freeNative(nativeRenderer);
    throw structuredFailure(error, stage);
  } finally {
    if (!scenePublished) {
      // Releasing a reader lock does not cancel the underlying fetch. Without
      // an explicit cancel, a failed large-scene admission can keep pulling a
      // multi-gigabyte response in the background while the UI reports the
      // failure (or starts another load).
      try {
        await reader.cancel?.("gsplat exact Packed scene construction failed");
      } catch {
        // Preserve the original decode/capacity error. Transport cancellation
        // is best-effort cleanup and must not replace the actionable cause.
      }
      freeNative(nativeStream);
    }
    try {
      reader.releaseLock?.();
    } catch {
      // The scene result or actionable construction failure takes precedence
      // over best-effort transport lock cleanup.
    }
  }
}

export class GsplatWebError extends Error {
  constructor(error, stage, options = {}) {
    const { scenePublished = false } = options;
    const errorMessage = failureMessage(error);
    super(errorMessage);
    this.name = "GsplatWebError";
    this.stage = stage;
    this.error_code = failureCode(error, errorMessage, stage);
    this.error_message = errorMessage;
    this.scene_published = Boolean(scenePublished);
    const resource = failureResource(error, errorMessage);
    if (resource) {
      this.resource = resource;
    }
    if (error !== this && error != null) {
      this.cause = error;
    }
  }
}

export class GsplatWebRenderer {
  #nativeRenderer;
  #nativePendingDispose = null;
  #disposed = false;
  #mutationQueue = Promise.resolve();
  #pendingMutations = new Set();

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
    return this.#enqueueMutation(FAILURE_STAGES.resize, async () => {
      let current;
      try {
        current = nativeRenderer.surfaceSize();
      } catch (error) {
        throw structuredRuntimeMutationFailure(error, FAILURE_STAGES.resize);
      }
      if (Number(current.width) === width && Number(current.height) === height) {
        return;
      }
      if (typeof nativeRenderer.resizeAsync !== "function") {
        throw structuredRuntimeMutationFailure(
          new Error(
            "the loaded gsplat-web module does not support transactional asynchronous resize",
          ),
          FAILURE_STAGES.resize,
        );
      }
      try {
        await nativeRenderer.resizeAsync(width, height);
        const published = nativeRenderer.surfaceSize();
        if (Number(published.width) !== width || Number(published.height) !== height) {
          throw new Error(
            `native resize published ${published.width}x${published.height}; ` +
            `requested ${width}x${height}`,
          );
        }
      } catch (error) {
        throw structuredRuntimeMutationFailure(error, FAILURE_STAGES.resize);
      }
    });
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
    if (Object.hasOwn(intrinsics ?? {}, "focalLengthXOverY")) {
      const ratio = intrinsics.focalLengthXOverY;
      assertFinite(ratio, "camera.intrinsics.focalLengthXOverY");
      if (ratio < MIN_FOCAL_LENGTH_X_OVER_Y || ratio > MAX_FOCAL_LENGTH_X_OVER_Y) {
        throw new RangeError(
          "camera.intrinsics.focalLengthXOverY must be within 2^-16..=2^16",
        );
      }
      values.push(ratio);
    }
    this.#requireNativeRenderer().setCamera(new Float32Array(values));
  }

  cameraReceipt() {
    const values = Array.from(this.#requireNativeRenderer().cameraReceipt());
    if (!matchesCameraReceiptLength(values.length)
        || values.some((value) => !Number.isFinite(value))) {
      throw new TypeError("native camera receipt must contain 10 or 11 finite values");
    }
    const focalLengthXOverY = values.length === 11 ? values[10] : 1;
    if (focalLengthXOverY < MIN_FOCAL_LENGTH_X_OVER_Y
        || focalLengthXOverY > MAX_FOCAL_LENGTH_X_OVER_Y) {
      throw new TypeError("native camera receipt has an invalid focal-length ratio");
    }
    return {
      position: values.slice(0, 3),
      rotationXyzw: values.slice(3, 7),
      intrinsics: {
        verticalFovRadians: values[7],
        nearPlane: values[8],
        farPlane: values[9],
        focalLengthXOverY,
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
    const nativeRenderer = this.#requireNativeRenderer();
    try {
      nativeRenderer.setGeometryPath(id);
    } catch (error) {
      throw structuredRuntimeMutationFailure(error, FAILURE_STAGES.geometryPath);
    }
  }

  setGeometryPathAsync(path) {
    const id = resolveGeometryPathId(path);
    const nativeRenderer = this.#requireNativeRenderer();
    return this.#enqueueMutation(FAILURE_STAGES.geometryPath, async () => {
      if (typeof nativeRenderer.setGeometryPathAsync !== "function") {
        throw new Error(
          "the loaded gsplat-web module does not support transactional asynchronous geometry-path switching",
        );
      }
      await nativeRenderer.setGeometryPathAsync(id);
    });
  }

  setOrderBackend(backend) {
    const id = resolveOrderBackendId(backend);
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.setOrderBackend !== "function") {
      if (backend === "cpu") return;
      throw new Error("the loaded gsplat-web module does not support GPU/adaptive ordering");
    }
    nativeRenderer.setOrderBackend(id);
  }

  /**
   * Changes only the exact projected draw strategy. Native validation is
   * transactional: a failed Compact request leaves the old policy active.
   */
  setProjectedPolicy(policy) {
    resolveProjectedPolicyId(policy);
    const pendingMutation = this.#pendingMutations.values().next().value;
    if (pendingMutation) {
      throw structuredRuntimeMutationFailure(
        new Error("projected policy cannot change while another renderer transaction is pending"),
        FAILURE_STAGES.projectedPolicy,
      );
    }
    try {
      applyNativeProjectedPolicy(this.#requireNativeRenderer(), policy);
    } catch (error) {
      throw structuredRuntimeMutationFailure(error, FAILURE_STAGES.projectedPolicy);
    }
  }

  /**
   * Transactionally switches only the implementation inside the GPU order
   * lane and enables its independent diagnostic receipt stream. Native code
   * rejects any context other than Packed + ProjectedQuadsExact + forced
   * Compact without replacing the live producer.
   */
  setGpuOrderProducerAsync(producer) {
    const id = resolveGpuOrderProducerId(producer);
    const nativeRenderer = this.#requireNativeRenderer();
    return this.#enqueueMutation(FAILURE_STAGES.gpuOrderProducer, async () => {
      if (typeof nativeRenderer.setGpuOrderProducerAsync !== "function") {
        throw new Error(
          "the loaded gsplat-web module does not support transactional GPU producer selection",
        );
      }
      await nativeRenderer.setGpuOrderProducerAsync(id);
      if (typeof nativeRenderer.gpuOrderProducer === "function") {
        const published = String(nativeRenderer.gpuOrderProducer());
        if (published !== producer) {
          throw new Error(
            `native GPU producer published ${published}; requested ${producer}`,
          );
        }
      }
    });
  }

  rasterPath() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.rasterPath !== "function") {
      return "sorted_index_direct";
    }
    return String(nativeRenderer.rasterPath());
  }

  renderFrame() {
    const pendingMutation = this.#pendingMutations.values().next().value;
    if (pendingMutation) {
      const operation = pendingMutation.stage === FAILURE_STAGES.resize
        ? "resize"
        : pendingMutation.stage === FAILURE_STAGES.geometryPath
          ? "geometry-path switch"
          : "GPU-producer switch";
      throw structuredRuntimeMutationFailure(
        new Error(`render submission is blocked while transactional ${operation} is pending`),
        pendingMutation.stage,
      );
    }
    const frame = normalizeFrameStats(this.#requireNativeRenderer().renderFrame());
    return frame;
  }

  drainOrderMeasurementReceipts() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.drainOrderMeasurementReceipts !== "function") {
      throw new Error(
        "the loaded gsplat-web module does not support standalone terminal order receipts",
      );
    }
    return normalizeMeasurementReceipts(
      nativeRenderer.drainOrderMeasurementReceipts(),
    );
  }

  requestCurrentStats() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.requestCurrentStats !== "function") {
      throw new Error("the loaded gsplat-web module does not support Exact current stats");
    }
    return normalizeCurrentStatsRequest(nativeRenderer.requestCurrentStats());
  }

  pollCurrentStats() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.pollCurrentStats !== "function") {
      throw new Error("the loaded gsplat-web module does not support Exact current stats");
    }
    return normalizeCurrentStatsPoll(nativeRenderer.pollCurrentStats());
  }

  async requestDiagnosticSurfaceCapture() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.requestDiagnosticSurfaceCapture !== "function") {
      throw new Error(
        "the loaded gsplat-web module does not include diagnostic Surface capture",
      );
    }
    await nativeRenderer.requestDiagnosticSurfaceCapture();
  }

  async takeDiagnosticSurfaceCapture() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.takeDiagnosticSurfaceCapture !== "function") {
      throw new Error(
        "the loaded gsplat-web module does not include diagnostic Surface capture",
      );
    }
    return nativeRenderer.takeDiagnosticSurfaceCapture();
  }

  requestDiagnosticQueueTerminal() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.requestDiagnosticQueueTerminal !== "function") {
      throw new Error(
        "the loaded gsplat-web module does not include diagnostic queue completion",
      );
    }
    nativeRenderer.requestDiagnosticQueueTerminal();
  }

  pollDiagnosticQueueTerminal() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.pollDiagnosticQueueTerminal !== "function") {
      throw new Error(
        "the loaded gsplat-web module does not include diagnostic queue completion",
      );
    }
    const terminal = nativeRenderer.pollDiagnosticQueueTerminal();
    if (terminal?.status === "pending" && terminal.completedAtMonotonicMs === null) {
      return Object.freeze({ status: "pending", completedAtMonotonicMs: null });
    }
    if (terminal?.status === "ready"
        && Number.isFinite(terminal.completedAtMonotonicMs)
        && terminal.completedAtMonotonicMs >= 0) {
      return Object.freeze({
        status: "ready",
        completedAtMonotonicMs: terminal.completedAtMonotonicMs,
      });
    }
    throw new TypeError("diagnostic queue completion returned an invalid terminal");
  }

  diagnosticSurfaceDeviceReceipt() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.diagnosticSurfaceDeviceReceipt !== "function") {
      throw new Error(
        "the loaded gsplat-web module does not include diagnostic Surface device evidence",
      );
    }
    return normalizeDiagnosticSurfaceDeviceReceipt(
      nativeRenderer.diagnosticSurfaceDeviceReceipt(),
    );
  }

  sceneSummary() {
    const summary = this.#requireNativeRenderer().sceneSummary();
    return {
      gaussians: numberOr(summary.gaussians, 0),
      shDegree: numberOr(summary.shDegree, 0),
      hasShRest: Boolean(summary.hasShRest),
    };
  }

  loadReceipt() {
    const nativeRenderer = this.#requireNativeRenderer();
    if (typeof nativeRenderer.loadReceipt !== "function") {
      return null;
    }
    const receipt = nativeRenderer.loadReceipt();
    return {
      transportBytes: numberOr(receipt.transportBytes, 0),
      peakDecoderBufferBytes: numberOr(receipt.peakDecoderBufferBytes, 0),
      streamed: Boolean(receipt.streamed),
      inputSha256: String(receipt.inputSha256 ?? ""),
      sourceCount: numberOr(receipt.sourceCount, 0),
      decodedCount: numberOr(receipt.decodedCount, 0),
      encodedCount: numberOr(receipt.encodedCount, 0),
      residentCount: numberOr(receipt.residentCount, 0),
      addressableCount: numberOr(receipt.addressableCount, 0),
      sourceShDegree: numberOr(receipt.sourceShDegree, receipt.shDegree),
      residentShDegree: numberOr(receipt.residentShDegree, receipt.shDegree),
      shDegree: numberOr(receipt.shDegree, 0),
      fullQuality: Boolean(receipt.fullQuality),
      sourceMembership: String(receipt.sourceMembership ?? "unknown"),
      samplingEnabled: Boolean(receipt.samplingEnabled),
      lodEnabled: Boolean(receipt.lodEnabled),
      partialScenePublished: Boolean(receipt.partialScenePublished),
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
    const nativeRenderer = this.#nativeRenderer;
    this.#nativeRenderer = null;
    this.#disposed = true;
    if (this.#pendingMutations.size === 0) {
      freeNative(nativeRenderer);
    } else {
      this.#nativePendingDispose = nativeRenderer;
    }
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

  #enqueueMutation(stage, mutate) {
    const token = { stage };
    this.#pendingMutations.add(token);
    const operation = this.#mutationQueue
      .then(async () => {
        if (this.#disposed) {
          throw new Error("GsplatWebRenderer is disposed");
        }
        await mutate();
      })
      .catch((error) => {
        throw structuredRuntimeMutationFailure(error, stage);
      });
    this.#mutationQueue = operation.catch(() => {});
    operation.then(
      () => this.#finishMutationOperation(token),
      () => this.#finishMutationOperation(token),
    );
    return operation;
  }

  #finishMutationOperation(token) {
    this.#pendingMutations.delete(token);
    if (this.#pendingMutations.size === 0 && this.#nativePendingDispose) {
      const nativeRenderer = this.#nativePendingDispose;
      this.#nativePendingDispose = null;
      freeNative(nativeRenderer);
    }
  }
}

function resolveGeometryPathId(path) {
  const id = GEOMETRY_PATH_IDS[path];
  if (id === undefined) {
    throw new TypeError("geometryPath must be direct, packed, or paged");
  }
  return id;
}

function resolveOrderBackendId(backend) {
  const id = ORDER_BACKEND_IDS[backend];
  if (id === undefined) {
    throw new TypeError("orderBackend must be cpu, gpu, or adaptive");
  }
  return id;
}

function resolveProjectedPolicyId(policy) {
  const id = PROJECTED_POLICY_IDS[policy];
  if (id === undefined) {
    throw new TypeError("projectedPolicy must be candidate, compact, or adaptive");
  }
  return id;
}

function resolveGpuOrderProducerId(producer) {
  const id = GPU_ORDER_PRODUCER_IDS[producer];
  if (id === undefined) {
    throw new TypeError("gpuOrderProducer must be post-sort or preproject");
  }
  return id;
}

function validateGpuProducerExperiment(producer, geometryPath, projectedPolicy) {
  if (producer === null || producer === undefined) return;
  resolveGpuOrderProducerId(producer);
  if (geometryPath !== "packed" || projectedPolicy !== "compact") {
    throw new TypeError(
      "gpuOrderProducer diagnostics require geometryPath=packed and projectedPolicy=compact",
    );
  }
}

function applyNativeProjectedPolicy(nativeRenderer, policy) {
  const id = resolveProjectedPolicyId(policy);
  if (typeof nativeRenderer.setProjectedPolicy !== "function") {
    if (policy === "adaptive") return;
    throw new Error("the loaded gsplat-web module does not support projected draw policies");
  }
  nativeRenderer.setProjectedPolicy(id);
}

function normalizeCurrentStatsRequest(raw) {
  const status = String(raw?.status ?? "unknown");
  if (status === "requested") {
    return { status, reason: null };
  }
  if (status === "unsampled") {
    return { status, reason: normalizeCurrentStatsUnsampledReason(raw?.reason) };
  }
  throw new TypeError(`unknown current-stats request status ${status}`);
}

function normalizeCurrentStatsPoll(raw) {
  const status = String(raw?.status ?? "unknown");
  if (status === "empty") return { status };
  if (status === "unsampled") {
    return { status, reason: normalizeCurrentStatsUnsampledReason(raw?.reason) };
  }
  const terminalStatuses = new Set([
    "ready",
    "map_failure",
    "generation_invalidated",
    "expired",
    "dropped",
  ]);
  if (!terminalStatuses.has(status)) {
    throw new TypeError(`unknown current-stats poll status ${status}`);
  }
  const terminal = {
    status,
    ticket: telemetrySafeInteger(raw?.ticket, "current-stats ticket"),
    plan: normalizeCurrentStatsPlan(raw?.plan),
    sceneGeneration: telemetrySafeInteger(
      raw?.sceneGeneration,
      "current-stats sceneGeneration",
    ),
    cameraRevision: telemetrySafeInteger(
      raw?.cameraRevision,
      "current-stats cameraRevision",
    ),
    viewportGeneration: telemetrySafeInteger(
      raw?.viewportGeneration,
      "current-stats viewportGeneration",
    ),
    contractGeneration: telemetrySafeInteger(
      raw?.contractGeneration,
      "current-stats contractGeneration",
    ),
    planSetGeneration: telemetrySafeInteger(
      raw?.planSetGeneration,
      "current-stats planSetGeneration",
    ),
    orderGeneration: telemetrySafeInteger(
      raw?.orderGeneration,
      "current-stats orderGeneration",
    ),
    rasterGeneration: telemetrySafeInteger(
      raw?.rasterGeneration,
      "current-stats rasterGeneration",
    ),
    encodeAttempt: telemetrySafeInteger(raw?.encodeAttempt, "current-stats encodeAttempt"),
    presentationSequence: telemetrySafeInteger(
      raw?.presentationSequence,
      "current-stats presentationSequence",
    ),
  };
  if (status !== "ready") return terminal;
  const sourceCount = telemetrySafeInteger(raw?.sourceCount, "current-stats sourceCount");
  const visibleCount = telemetrySafeInteger(raw?.visibleCount, "current-stats visibleCount");
  const contributorCount = telemetrySafeInteger(
    raw?.contributorCount,
    "current-stats contributorCount",
  );
  const drawnCount = telemetrySafeInteger(raw?.drawnCount, "current-stats drawnCount");
  if (contributorCount > visibleCount || visibleCount > sourceCount) {
    throw new TypeError("current-stats receipt violates C <= V <= S");
  }
  const countSemantics = String(raw?.countSemantics ?? "unknown");
  if (!["draw_equals_visible", "indirect_draw_equals_visible", "indirect_draw_equals_contributor"]
    .includes(countSemantics)) {
    throw new TypeError(`unknown current-stats count semantics ${countSemantics}`);
  }
  const expectedDrawn = countSemantics === "indirect_draw_equals_contributor"
    ? contributorCount
    : visibleCount;
  if (drawnCount !== expectedDrawn) {
    throw new TypeError("current-stats drawn count disagrees with its exact semantics");
  }
  return {
    ...terminal,
    countSemantics,
    sourceCount,
    visibleCount,
    contributorCount,
    drawnCount,
  };
}

function normalizeCurrentStatsUnsampledReason(value) {
  const reason = String(value ?? "unknown");
  if (!["busy", "gpu_unavailable", "resource_unavailable", "ticket_exhausted"].includes(reason)) {
    throw new TypeError(`unknown current-stats unsampled reason ${reason}`);
  }
  return reason;
}

function normalizeCurrentStatsPlan(value) {
  const plan = String(value ?? "unknown");
  if (!["cpu_post_sort", "gpu_post_sort", "gpu_preproject"].includes(plan)) {
    throw new TypeError(`unknown Exact current-stats plan ${plan}`);
  }
  return plan;
}

function normalizeFrameStats(raw) {
  const receipts = normalizeMeasurementReceipts(raw);
  const gpuOrderPreparationPending = Boolean(raw.gpuOrderPreparationPending);
  if (receipts.completedOrderMeasurements.length === 0 && raw.completedMeasurementAvailable) {
    receipts.completedOrderMeasurements.push(normalizeOrderMeasurement({
      ticket: raw.completedMeasurementTicket,
      cameraRevision: raw.completedMeasurementRevision,
      timingSource: raw.completedMeasurementTimingSource,
      gpuPreprocessMs: raw.gpuPreprocessMs,
      gpuRadixMs: raw.gpuRadixMs,
      gpuOrderMs: raw.gpuOrderMs,
      gpuCompleteMs: raw.gpuCompleteMs,
      timestampPeriodNs: raw.gpuTimestampPeriodNs,
      belowTimestampResolution: raw.gpuBelowTimestampResolution,
      countSemantics: raw.completedContributorCount == null ||
        raw.completedExactContributorCompaction == null
        ? null
        : "candidate_visible_contributor_issued_v1",
      visibleCount: raw.completedVisibleCount,
      contributorCount: raw.completedContributorCount,
      drawnCount: raw.completedDrawnCount,
      exactContributorCompaction: raw.completedExactContributorCompaction,
    }));
  }
  if (receipts.failedOrderMeasurements.length === 0 && raw.failedMeasurementAvailable) {
    receipts.failedOrderMeasurements.push(normalizeOrderMeasurementFailure({
      ticket: raw.failedMeasurementTicket,
      cameraRevision: raw.failedMeasurementRevision,
      reason: raw.failedMeasurementReason,
    }));
  }
  if (receipts.completedProjectedMeasurements.length === 0
      && raw.completedProjectedMeasurementAvailable) {
    receipts.completedProjectedMeasurements.push(normalizeProjectedMeasurement({
      ticket: raw.completedProjectedMeasurementTicket,
      cameraRevision: raw.completedProjectedMeasurementRevision,
      execution: raw.completedProjectedMeasurementExecution,
      orderBackend: raw.completedProjectedMeasurementOrderBackend,
      projectionGeneration: raw.completedProjectedProjectionGeneration,
      probeGeneration: raw.completedProjectedProbeGeneration,
      projectionRebuilt: raw.completedProjectedProjectionRebuilt,
      orderRefreshed: raw.completedProjectedOrderRefreshed,
      frameCompleteMs: raw.completedProjectedFrameCompleteMs,
      countSemantics: "candidate_visible_contributor_issued_v1",
      visibleCount: raw.completedProjectedVisibleCount,
      contributorCount: raw.completedProjectedContributorCount,
      drawnCount: raw.completedProjectedDrawnCount,
      exactContributorCompaction: raw.completedProjectedExactContributorCompaction,
    }));
  }
  if (receipts.failedProjectedMeasurements.length === 0
      && raw.failedProjectedMeasurementAvailable) {
    receipts.failedProjectedMeasurements.push(normalizeProjectedMeasurementFailure({
      ticket: raw.failedProjectedMeasurementTicket,
      cameraRevision: raw.failedProjectedMeasurementRevision,
      execution: raw.failedProjectedMeasurementExecution,
      orderBackend: raw.failedProjectedMeasurementOrderBackend,
      projectionGeneration: raw.failedProjectedProjectionGeneration,
      probeGeneration: raw.failedProjectedProbeGeneration,
      reason: raw.failedProjectedMeasurementReason,
    }));
  }
  if (receipts.completedGpuProducerMeasurements.length === 0
      && raw.completedGpuProducerMeasurementAvailable) {
    receipts.completedGpuProducerMeasurements.push(normalizeGpuProducerMeasurement({
      ticket: raw.completedGpuProducerMeasurementTicket,
      cameraRevision: raw.completedGpuProducerMeasurementRevision,
      producer: raw.completedGpuProducerMeasurementProducer,
      orderGeneration: raw.completedGpuProducerOrderGeneration,
      projectionGeneration: raw.completedGpuProducerProjectionGeneration,
      countSemantics: "source_contributor_issued_v1",
      sourceCount: raw.completedGpuProducerSourceCount,
      contributorCount: raw.completedGpuProducerContributorCount,
      drawnCount: raw.completedGpuProducerDrawnCount,
      orderRefreshed: raw.completedGpuProducerOrderRefreshed,
      drawScope: raw.completedGpuProducerDrawScope,
      exactCurrentContributorDraw: raw.completedGpuProducerExactCurrentContributorDraw,
      staleOrder: raw.completedGpuProducerStaleOrder,
      queueCompleteMs: raw.completedGpuProducerQueueCompleteMs,
    }));
  }
  if (receipts.failedGpuProducerMeasurements.length === 0
      && raw.failedGpuProducerMeasurementAvailable) {
    receipts.failedGpuProducerMeasurements.push(normalizeGpuProducerMeasurementFailure({
      ticket: raw.failedGpuProducerMeasurementTicket,
      cameraRevision: raw.failedGpuProducerMeasurementRevision,
      producer: raw.failedGpuProducerMeasurementProducer,
      orderGeneration: raw.failedGpuProducerOrderGeneration,
      projectionGeneration: raw.failedGpuProducerProjectionGeneration,
      reason: raw.failedGpuProducerMeasurementReason,
    }));
  }
  assertUniqueProjectedTerminals(receipts);
  assertUniqueGpuProducerTerminals(receipts);
  const visibleCount = nullableSafeInteger(raw.visibleCount, "visibleCount");
  const drawnCount = nullableSafeInteger(raw.drawnCount, "drawnCount");
  const frame = {
    currentStatsSubmission: String(raw.currentStatsSubmission ?? "not_requested"),
    currentStatsTicket: nullableSafeInteger(raw.currentStatsTicket, "currentStatsTicket"),
    currentStatsPlan: raw.currentStatsPlan == null ? null : normalizeCurrentStatsPlan(raw.currentStatsPlan),
    currentStatsSceneGeneration: nullableSafeInteger(
      raw.currentStatsSceneGeneration,
      "currentStatsSceneGeneration",
    ),
    currentStatsCameraRevision: nullableSafeInteger(
      raw.currentStatsCameraRevision,
      "currentStatsCameraRevision",
    ),
    currentStatsViewportGeneration: nullableSafeInteger(
      raw.currentStatsViewportGeneration,
      "currentStatsViewportGeneration",
    ),
    currentStatsContractGeneration: nullableSafeInteger(
      raw.currentStatsContractGeneration,
      "currentStatsContractGeneration",
    ),
    currentStatsPlanSetGeneration: nullableSafeInteger(
      raw.currentStatsPlanSetGeneration,
      "currentStatsPlanSetGeneration",
    ),
    currentStatsOrderGeneration: nullableSafeInteger(
      raw.currentStatsOrderGeneration,
      "currentStatsOrderGeneration",
    ),
    currentStatsRasterGeneration: nullableSafeInteger(
      raw.currentStatsRasterGeneration,
      "currentStatsRasterGeneration",
    ),
    currentStatsEncodeAttempt: nullableSafeInteger(
      raw.currentStatsEncodeAttempt,
      "currentStatsEncodeAttempt",
    ),
    currentStatsPresentationSequence: nullableSafeInteger(
      raw.currentStatsPresentationSequence,
      "currentStatsPresentationSequence",
    ),
    frameMs: numberOr(raw.frameMs, 0),
    preprocessMs: numberOr(raw.preprocessMs, 0),
    sortMs: numberOr(raw.sortMs, 0),
    rasterMs: numberOr(raw.rasterMs, 0),
    cpuGeometryMs: numberOr(raw.cpuGeometryMs, raw.rasterMs),
    renderSubmitMs: numberOr(raw.renderSubmitMs, 0),
    frameWallMs: numberOr(raw.frameWallMs, raw.frameMs),
    framePresented: raw.framePresented !== false,
    gpuOrderPreparationPending,
    rasterExecutionPlan: String(raw.rasterExecutionPlan ?? "global_quads"),
    visibleCount,
    drawnCount,
    refreshSort: Boolean(raw.refreshSort),
    orderBackend: String(raw.orderBackend ?? "cpu"),
    adaptiveGpuFailure: raw.adaptiveGpuFailure == null
      ? null
      : String(raw.adaptiveGpuFailure),
    gpuSortFallback: Boolean(raw.gpuSortFallback),
    adaptiveState: String(raw.adaptiveState ?? "disabled"),
    projectedPolicy: String(raw.projectedPolicy ?? "adaptive"),
    projectedExecution: String(raw.projectedExecution ?? "candidate"),
    projectedAdaptiveState: String(raw.projectedAdaptiveState ?? "disabled"),
    projectedMeasurementSubmission: String(
      raw.projectedMeasurementSubmission ?? "not_requested",
    ),
    projectedMeasurementTicket: nullableSafeInteger(
      raw.projectedMeasurementTicket,
      "projectedMeasurementTicket",
    ),
    projectedMeasurementExecution: raw.projectedMeasurementExecution == null
      ? null
      : String(raw.projectedMeasurementExecution),
    projectedMeasurementUnsampledReason: raw.projectedMeasurementUnsampledReason == null
      ? null
      : String(raw.projectedMeasurementUnsampledReason),
    gpuOrderProducer: raw.gpuOrderProducer == null
      ? null
      : String(raw.gpuOrderProducer),
    gpuProducerMeasurementSubmission: String(
      raw.gpuProducerMeasurementSubmission ?? "not_requested",
    ),
    gpuProducerMeasurementTicket: nullableSafeInteger(
      raw.gpuProducerMeasurementTicket,
      "gpuProducerMeasurementTicket",
    ),
    gpuProducerMeasurementProducer: raw.gpuProducerMeasurementProducer == null
      ? null
      : String(raw.gpuProducerMeasurementProducer),
    gpuProducerMeasurementUnsampledReason:
      raw.gpuProducerMeasurementUnsampledReason == null
        ? null
        : String(raw.gpuProducerMeasurementUnsampledReason),
    cameraRevision: numberOr(raw.cameraRevision, 0),
    appliedOrderRevision: numberOr(raw.appliedOrderRevision, 0),
    presentedOrderRevisionLag: numberOr(raw.presentedOrderRevisionLag, 0),
    submittedMeasurementTicket: raw.submittedMeasurementTicket == null
      ? null
      : orderTicketSafeInteger(raw.submittedMeasurementTicket, "submittedMeasurementTicket"),
    submittedMeasurementBackend: raw.submittedMeasurementBackend == null
      ? null
      : String(raw.submittedMeasurementBackend),
    measurementUnsampledReason: raw.measurementUnsampledReason == null
      ? null
      : String(raw.measurementUnsampledReason),
    visibleCountRevision: nullableNumber(raw.visibleCountRevision),
    visibleCountPending: Boolean(raw.visibleCountPending),
    gpuTimestampQueriesEnabled: Boolean(raw.gpuTimestampQueriesEnabled),
    completedMeasurementAvailable: Boolean(raw.completedMeasurementAvailable),
    completedMeasurementTicket: raw.completedMeasurementTicket == null
      ? null
      : orderTicketSafeInteger(raw.completedMeasurementTicket, "completedMeasurementTicket"),
    completedMeasurementRevision: nullableNumber(raw.completedMeasurementRevision),
    completedMeasurementTimingSource: raw.completedMeasurementTimingSource == null
      ? null
      : String(raw.completedMeasurementTimingSource),
    gpuPreprocessMs: nullableNumber(raw.gpuPreprocessMs),
    gpuRadixMs: nullableNumber(raw.gpuRadixMs),
    gpuOrderMs: nullableNumber(raw.gpuOrderMs),
    gpuCompleteMs: nullableNumber(raw.gpuCompleteMs),
    gpuTimestampPeriodNs: nullableNumber(raw.gpuTimestampPeriodNs),
    gpuBelowTimestampResolution: raw.gpuBelowTimestampResolution == null
      ? null
      : Boolean(raw.gpuBelowTimestampResolution),
    completedVisibleCount: nullableNumber(raw.completedVisibleCount),
    completedContributorCount: nullableNumber(raw.completedContributorCount),
    completedDrawnCount: nullableNumber(raw.completedDrawnCount),
    completedExactContributorCompaction:
      raw.completedExactContributorCompaction == null
        ? null
        : Boolean(raw.completedExactContributorCompaction),
    failedMeasurementAvailable: Boolean(raw.failedMeasurementAvailable),
    failedMeasurementTicket: raw.failedMeasurementTicket == null
      ? null
      : orderTicketSafeInteger(raw.failedMeasurementTicket, "failedMeasurementTicket"),
    failedMeasurementRevision: nullableNumber(raw.failedMeasurementRevision),
    failedMeasurementReason: raw.failedMeasurementReason == null
      ? null
      : String(raw.failedMeasurementReason),
    completedProjectedMeasurementAvailable:
      Boolean(raw.completedProjectedMeasurementAvailable),
    completedProjectedMeasurementTicket: nullableSafeInteger(
      raw.completedProjectedMeasurementTicket,
      "completedProjectedMeasurementTicket",
    ),
    completedProjectedMeasurementRevision: nullableSafeInteger(
      raw.completedProjectedMeasurementRevision,
      "completedProjectedMeasurementRevision",
    ),
    completedProjectedMeasurementExecution:
      raw.completedProjectedMeasurementExecution == null
        ? null
        : String(raw.completedProjectedMeasurementExecution),
    completedProjectedMeasurementOrderBackend:
      raw.completedProjectedMeasurementOrderBackend == null
        ? null
        : String(raw.completedProjectedMeasurementOrderBackend),
    completedProjectedProjectionGeneration: nullableSafeInteger(
      raw.completedProjectedProjectionGeneration,
      "completedProjectedProjectionGeneration",
    ),
    completedProjectedProbeGeneration: nullableSafeInteger(
      raw.completedProjectedProbeGeneration,
      "completedProjectedProbeGeneration",
    ),
    completedProjectedProjectionRebuilt:
      nullableBoolean(raw.completedProjectedProjectionRebuilt),
    completedProjectedOrderRefreshed:
      nullableBoolean(raw.completedProjectedOrderRefreshed),
    completedProjectedFrameCompleteMs:
      nullableNumber(raw.completedProjectedFrameCompleteMs),
    completedProjectedVisibleCount: nullableNumber(raw.completedProjectedVisibleCount),
    completedProjectedContributorCount: nullableNumber(raw.completedProjectedContributorCount),
    completedProjectedDrawnCount: nullableNumber(raw.completedProjectedDrawnCount),
    completedProjectedExactContributorCompaction:
      nullableBoolean(raw.completedProjectedExactContributorCompaction),
    failedProjectedMeasurementAvailable: Boolean(raw.failedProjectedMeasurementAvailable),
    failedProjectedMeasurementTicket: nullableSafeInteger(
      raw.failedProjectedMeasurementTicket,
      "failedProjectedMeasurementTicket",
    ),
    failedProjectedMeasurementRevision: nullableSafeInteger(
      raw.failedProjectedMeasurementRevision,
      "failedProjectedMeasurementRevision",
    ),
    failedProjectedMeasurementExecution: raw.failedProjectedMeasurementExecution == null
      ? null
      : String(raw.failedProjectedMeasurementExecution),
    failedProjectedMeasurementOrderBackend:
      raw.failedProjectedMeasurementOrderBackend == null
        ? null
        : String(raw.failedProjectedMeasurementOrderBackend),
    failedProjectedProjectionGeneration: nullableSafeInteger(
      raw.failedProjectedProjectionGeneration,
      "failedProjectedProjectionGeneration",
    ),
    failedProjectedProbeGeneration: nullableSafeInteger(
      raw.failedProjectedProbeGeneration,
      "failedProjectedProbeGeneration",
    ),
    failedProjectedMeasurementReason: raw.failedProjectedMeasurementReason == null
      ? null
      : String(raw.failedProjectedMeasurementReason),
    completedGpuProducerMeasurementAvailable:
      Boolean(raw.completedGpuProducerMeasurementAvailable),
    completedGpuProducerMeasurementTicket: nullableSafeInteger(
      raw.completedGpuProducerMeasurementTicket,
      "completedGpuProducerMeasurementTicket",
    ),
    completedGpuProducerMeasurementRevision: nullableSafeInteger(
      raw.completedGpuProducerMeasurementRevision,
      "completedGpuProducerMeasurementRevision",
    ),
    completedGpuProducerMeasurementProducer:
      raw.completedGpuProducerMeasurementProducer == null
        ? null
        : String(raw.completedGpuProducerMeasurementProducer),
    completedGpuProducerOrderGeneration: nullableSafeInteger(
      raw.completedGpuProducerOrderGeneration,
      "completedGpuProducerOrderGeneration",
    ),
    completedGpuProducerProjectionGeneration: nullableSafeInteger(
      raw.completedGpuProducerProjectionGeneration,
      "completedGpuProducerProjectionGeneration",
    ),
    completedGpuProducerSourceCount: nullableSafeInteger(
      raw.completedGpuProducerSourceCount,
      "completedGpuProducerSourceCount",
    ),
    completedGpuProducerContributorCount: nullableSafeInteger(
      raw.completedGpuProducerContributorCount,
      "completedGpuProducerContributorCount",
    ),
    completedGpuProducerDrawnCount: nullableSafeInteger(
      raw.completedGpuProducerDrawnCount,
      "completedGpuProducerDrawnCount",
    ),
    completedGpuProducerOrderRefreshed:
      nullableBoolean(raw.completedGpuProducerOrderRefreshed),
    completedGpuProducerDrawScope: raw.completedGpuProducerDrawScope == null
      ? null
      : String(raw.completedGpuProducerDrawScope),
    completedGpuProducerExactCurrentContributorDraw:
      nullableBoolean(raw.completedGpuProducerExactCurrentContributorDraw),
    completedGpuProducerStaleOrder:
      nullableBoolean(raw.completedGpuProducerStaleOrder),
    completedGpuProducerQueueCompleteMs:
      nullableNumber(raw.completedGpuProducerQueueCompleteMs),
    failedGpuProducerMeasurementAvailable:
      Boolean(raw.failedGpuProducerMeasurementAvailable),
    failedGpuProducerMeasurementTicket: nullableSafeInteger(
      raw.failedGpuProducerMeasurementTicket,
      "failedGpuProducerMeasurementTicket",
    ),
    failedGpuProducerMeasurementRevision: nullableSafeInteger(
      raw.failedGpuProducerMeasurementRevision,
      "failedGpuProducerMeasurementRevision",
    ),
    failedGpuProducerMeasurementProducer:
      raw.failedGpuProducerMeasurementProducer == null
        ? null
        : String(raw.failedGpuProducerMeasurementProducer),
    failedGpuProducerOrderGeneration: nullableSafeInteger(
      raw.failedGpuProducerOrderGeneration,
      "failedGpuProducerOrderGeneration",
    ),
    failedGpuProducerProjectionGeneration: nullableSafeInteger(
      raw.failedGpuProducerProjectionGeneration,
      "failedGpuProducerProjectionGeneration",
    ),
    failedGpuProducerMeasurementReason:
      raw.failedGpuProducerMeasurementReason == null
        ? null
        : String(raw.failedGpuProducerMeasurementReason),
    ...receipts,
    surfaceWidth: numberOr(raw.surfaceWidth, 0),
    surfaceHeight: numberOr(raw.surfaceHeight, 0),
    internalRenderWidth: numberOr(raw.internalRenderWidth, 0),
    internalRenderHeight: numberOr(raw.internalRenderHeight, 0),
    presentedWidth: nullableNumber(raw.presentedWidth),
    presentedHeight: nullableNumber(raw.presentedHeight),
  };
  if (frame.rasterExecutionPlan === "projected_quads_exact" && frame.framePresented) {
    if (frame.visibleCountPending) {
      if (frame.visibleCount !== null || frame.drawnCount !== null) {
        throw new TypeError("pending Exact V/D counts must remain unavailable");
      }
    } else if (frame.visibleCount === null || frame.drawnCount === null) {
      throw new TypeError("current Exact V/D counts must both be available");
    }
  }
  validateCurrentStatsFrameSubmission(frame);
  validateProjectedFrameSubmission(frame);
  validateGpuProducerFrameSubmission(frame);
  return frame;
}

function validateCurrentStatsFrameSubmission(frame) {
  if (frame.currentStatsSubmission === "not_requested") {
    for (const value of [
      frame.currentStatsTicket,
      frame.currentStatsPlan,
      frame.currentStatsSceneGeneration,
      frame.currentStatsCameraRevision,
      frame.currentStatsViewportGeneration,
      frame.currentStatsContractGeneration,
      frame.currentStatsPlanSetGeneration,
      frame.currentStatsOrderGeneration,
      frame.currentStatsRasterGeneration,
      frame.currentStatsEncodeAttempt,
      frame.currentStatsPresentationSequence,
    ]) {
      if (value !== null) {
        throw new TypeError("not-requested current stats exposed receipt identity");
      }
    }
    return;
  }
  if (frame.currentStatsSubmission !== "issued"
      || frame.currentStatsTicket === null
      || frame.currentStatsPlan === null
      || frame.currentStatsCameraRevision !== frame.cameraRevision) {
    throw new TypeError("issued current stats lacks a matching Exact frame identity");
  }
}

function normalizeMeasurementReceipts(raw) {
  const receipts = {
    completedCpuOrderMeasurements: Array.isArray(raw?.completedCpuOrderMeasurements)
      ? raw.completedCpuOrderMeasurements.map(normalizeCpuOrderMeasurement)
      : [],
    completedOrderMeasurements: Array.isArray(raw?.completedOrderMeasurements)
      ? raw.completedOrderMeasurements.map(normalizeOrderMeasurement)
      : [],
    failedOrderMeasurements: Array.isArray(raw?.failedOrderMeasurements)
      ? raw.failedOrderMeasurements.map(normalizeOrderMeasurementFailure)
      : [],
    completedProjectedMeasurements: Array.isArray(raw?.completedProjectedMeasurements)
      ? raw.completedProjectedMeasurements.map(normalizeProjectedMeasurement)
      : [],
    failedProjectedMeasurements: Array.isArray(raw?.failedProjectedMeasurements)
      ? raw.failedProjectedMeasurements.map(normalizeProjectedMeasurementFailure)
      : [],
    completedGpuProducerMeasurements: Array.isArray(raw?.completedGpuProducerMeasurements)
      ? raw.completedGpuProducerMeasurements.map(normalizeGpuProducerMeasurement)
      : [],
    failedGpuProducerMeasurements: Array.isArray(raw?.failedGpuProducerMeasurements)
      ? raw.failedGpuProducerMeasurements.map(normalizeGpuProducerMeasurementFailure)
      : [],
  };
  assertUniqueProjectedTerminals(receipts);
  assertUniqueGpuProducerTerminals(receipts);
  return receipts;
}

function normalizeGpuProducerMeasurement(raw) {
  const ticket = gpuProducerTicketSafeInteger(raw?.ticket, "GPU producer measurement ticket");
  const cameraRevision = telemetrySafeInteger(
    raw?.cameraRevision,
    "GPU producer measurement cameraRevision",
  );
  const producer = String(raw?.producer ?? "unknown");
  if (!Object.hasOwn(GPU_ORDER_PRODUCER_IDS, producer)) {
    throw new TypeError(`unknown GPU order producer ${producer}`);
  }
  const orderGeneration = telemetrySafeInteger(
    raw?.orderGeneration,
    "GPU producer measurement orderGeneration",
  );
  const projectionGeneration = telemetrySafeInteger(
    raw?.projectionGeneration,
    "GPU producer measurement projectionGeneration",
  );
  const countSemantics = String(raw?.countSemantics ?? "");
  if (countSemantics !== "source_contributor_issued_v1") {
    throw new TypeError(`unknown GPU producer count semantics ${countSemantics}`);
  }
  const sourceCount = telemetrySafeInteger(
    raw?.sourceCount,
    "GPU producer measurement sourceCount",
  );
  const contributorCount = telemetrySafeInteger(
    raw?.contributorCount,
    "GPU producer measurement contributorCount",
  );
  const drawnCount = telemetrySafeInteger(
    raw?.drawnCount,
    "GPU producer measurement drawnCount",
  );
  if (contributorCount > sourceCount || drawnCount > sourceCount) {
    throw new TypeError(`GPU producer ticket ${ticket} has invalid S/C/D evidence`);
  }
  const orderRefreshed = strictBoolean(
    raw?.orderRefreshed,
    "GPU producer measurement orderRefreshed",
  );
  const drawScope = String(raw?.drawScope ?? "unknown");
  const exactCurrentContributorDraw = strictBoolean(
    raw?.exactCurrentContributorDraw,
    "GPU producer measurement exactCurrentContributorDraw",
  );
  const staleOrder = strictBoolean(
    raw?.staleOrder,
    "GPU producer measurement staleOrder",
  );
  if (drawScope === "exact_current_contributors") {
    if (!orderRefreshed || !exactCurrentContributorDraw || staleOrder
        || drawnCount !== contributorCount) {
      throw new TypeError(`GPU producer ticket ${ticket} has invalid exact-current scope`);
    }
  } else if (drawScope === "stale_order_candidates") {
    if (orderRefreshed || exactCurrentContributorDraw || !staleOrder) {
      throw new TypeError(`GPU producer ticket ${ticket} has invalid stale-order scope`);
    }
  } else {
    throw new TypeError(`unknown GPU producer draw scope ${drawScope}`);
  }
  const queueCompleteMs = nonNegativeFiniteNumber(
    raw?.queueCompleteMs,
    "GPU producer measurement queueCompleteMs",
  );
  return {
    ticket,
    cameraRevision,
    producer,
    orderGeneration,
    projectionGeneration,
    countSemantics,
    sourceCount,
    contributorCount,
    drawnCount,
    orderRefreshed,
    drawScope,
    exactCurrentContributorDraw,
    staleOrder,
    queueCompleteMs,
  };
}

function normalizeGpuProducerMeasurementFailure(raw) {
  const ticket = gpuProducerTicketSafeInteger(raw?.ticket, "GPU producer failure ticket");
  const cameraRevision = telemetrySafeInteger(
    raw?.cameraRevision,
    "GPU producer failure cameraRevision",
  );
  const producer = String(raw?.producer ?? "unknown");
  if (!Object.hasOwn(GPU_ORDER_PRODUCER_IDS, producer)) {
    throw new TypeError(`unknown GPU producer failure producer ${producer}`);
  }
  const orderGeneration = telemetrySafeInteger(
    raw?.orderGeneration,
    "GPU producer failure orderGeneration",
  );
  const projectionGeneration = telemetrySafeInteger(
    raw?.projectionGeneration,
    "GPU producer failure projectionGeneration",
  );
  const reason = String(raw?.reason ?? "unknown");
  if (!["readback_map", "generation_invalidated", "invariant_violation"].includes(reason)) {
    throw new TypeError(`unknown GPU producer failure reason ${reason}`);
  }
  return {
    ticket,
    cameraRevision,
    producer,
    orderGeneration,
    projectionGeneration,
    reason,
  };
}

function assertUniqueGpuProducerTerminals(receipts) {
  const seen = new Set();
  for (const terminal of [
    ...receipts.completedGpuProducerMeasurements,
    ...receipts.failedGpuProducerMeasurements,
  ]) {
    if (seen.has(terminal.ticket)) {
      throw new TypeError(`GPU producer ticket ${terminal.ticket} has multiple terminals`);
    }
    seen.add(terminal.ticket);
  }
}

function validateGpuProducerFrameSubmission(frame) {
  const actual = frame.gpuOrderProducer;
  if (actual !== null && !Object.hasOwn(GPU_ORDER_PRODUCER_IDS, actual)) {
    throw new TypeError(`unknown actual GPU order producer ${actual}`);
  }
  const submission = frame.gpuProducerMeasurementSubmission;
  if (!["not_requested", "issued", "unsampled"].includes(submission)) {
    throw new TypeError(`unknown GPU producer measurement submission ${submission}`);
  }
  if (submission === "issued") {
    gpuProducerTicketSafeInteger(
      frame.gpuProducerMeasurementTicket,
      "gpuProducerMeasurementTicket",
    );
    if (!Object.hasOwn(GPU_ORDER_PRODUCER_IDS, frame.gpuProducerMeasurementProducer)
        || frame.gpuProducerMeasurementProducer !== actual
        || frame.gpuProducerMeasurementUnsampledReason !== null
        || frame.orderBackend !== "gpu"
        || frame.rasterExecutionPlan !== "projected_quads_exact"
        || frame.projectedPolicy !== "compact"
        || frame.projectedExecution !== "compact") {
      throw new TypeError("issued GPU producer measurement has invalid experiment identity");
    }
    return;
  }
  if (submission === "unsampled") {
    if (frame.gpuProducerMeasurementTicket !== null
        || !Object.hasOwn(GPU_ORDER_PRODUCER_IDS, frame.gpuProducerMeasurementProducer)
        || frame.gpuProducerMeasurementProducer !== actual
        || !["ring_busy", "surface_unavailable"].includes(
          frame.gpuProducerMeasurementUnsampledReason,
        )) {
      throw new TypeError("unsampled GPU producer measurement has invalid identity");
    }
    return;
  }
  if (frame.gpuProducerMeasurementTicket !== null
      || frame.gpuProducerMeasurementProducer !== null
      || frame.gpuProducerMeasurementUnsampledReason !== null) {
    throw new TypeError("not-requested GPU producer measurement must not expose identity");
  }
}

function normalizeProjectedMeasurement(raw) {
  const ticket = projectedTicketSafeInteger(raw?.ticket, "projected measurement ticket");
  const cameraRevision = telemetrySafeInteger(
    raw?.cameraRevision,
    "projected measurement cameraRevision",
  );
  const execution = String(raw?.execution ?? "unknown");
  const orderBackend = String(raw?.orderBackend ?? "unknown");
  const projectionGeneration = telemetrySafeInteger(
    raw?.projectionGeneration,
    "projected measurement projectionGeneration",
  );
  const probeGeneration = telemetrySafeInteger(
    raw?.probeGeneration,
    "projected measurement probeGeneration",
  );
  const projectionRebuilt = strictBoolean(
    raw?.projectionRebuilt,
    "projected measurement projectionRebuilt",
  );
  const orderRefreshed = strictBoolean(
    raw?.orderRefreshed,
    "projected measurement orderRefreshed",
  );
  const frameCompleteMs = nonNegativeFiniteNumber(
    raw?.frameCompleteMs,
    "projected measurement frameCompleteMs",
  );
  const countSemantics = String(raw?.countSemantics ?? "");
  const visibleCount = telemetrySafeInteger(
    raw?.visibleCount,
    "projected measurement visibleCount",
  );
  const contributorCount = telemetrySafeInteger(
    raw?.contributorCount,
    "projected measurement contributorCount",
  );
  const drawnCount = telemetrySafeInteger(
    raw?.drawnCount,
    "projected measurement drawnCount",
  );
  const exactContributorCompaction = strictBoolean(
    raw?.exactContributorCompaction,
    "projected measurement exactContributorCompaction",
  );
  if (!PROJECTED_EXECUTIONS.includes(execution)) {
    throw new TypeError(`unknown projected measurement execution ${execution}`);
  }
  if (!Object.hasOwn(ORDER_BACKEND_IDS, orderBackend) || orderBackend === "adaptive") {
    throw new TypeError(`unknown projected measurement order backend ${orderBackend}`);
  }
  if (!projectionRebuilt || orderRefreshed) {
    throw new TypeError("projected measurement was not isolated from order work");
  }
  if (countSemantics !== "candidate_visible_contributor_issued_v1"
      || contributorCount > visibleCount
      || (execution === "candidate"
        && (exactContributorCompaction || drawnCount !== visibleCount))
      || (execution === "compact"
        && (!exactContributorCompaction || drawnCount !== contributorCount))) {
    throw new TypeError(`projected measurement ticket ${ticket} has invalid V/C/D evidence`);
  }
  return {
    ticket,
    cameraRevision,
    execution,
    orderBackend,
    projectionGeneration,
    probeGeneration,
    projectionRebuilt,
    orderRefreshed,
    frameCompleteMs,
    countSemantics,
    visibleCount,
    contributorCount,
    drawnCount,
    exactContributorCompaction,
  };
}

function normalizeProjectedMeasurementFailure(raw) {
  const ticket = projectedTicketSafeInteger(raw?.ticket, "projected failure ticket");
  const cameraRevision = telemetrySafeInteger(
    raw?.cameraRevision,
    "projected failure cameraRevision",
  );
  const execution = String(raw?.execution ?? "unknown");
  const orderBackend = String(raw?.orderBackend ?? "unknown");
  const projectionGeneration = telemetrySafeInteger(
      raw?.projectionGeneration,
      "projected failure projectionGeneration",
  );
  const probeGeneration = telemetrySafeInteger(
      raw?.probeGeneration,
      "projected failure probeGeneration",
  );
  const reason = String(raw?.reason ?? "unknown");
  if (!PROJECTED_EXECUTIONS.includes(execution)) {
    throw new TypeError(`unknown projected failure execution ${execution}`);
  }
  if (!Object.hasOwn(ORDER_BACKEND_IDS, orderBackend) || orderBackend === "adaptive") {
    throw new TypeError(`unknown projected failure order backend ${orderBackend}`);
  }
  if (!["readback_map", "generation_invalidated", "invariant_violation"].includes(reason)) {
    throw new TypeError(`unknown projected failure reason ${reason}`);
  }
  return {
    ticket,
    cameraRevision,
    execution,
    orderBackend,
    projectionGeneration,
    probeGeneration,
    reason,
  };
}

function assertUniqueProjectedTerminals(receipts) {
  const seen = new Set();
  for (const terminal of [
    ...receipts.completedProjectedMeasurements,
    ...receipts.failedProjectedMeasurements,
  ]) {
    if (seen.has(terminal.ticket)) {
      throw new TypeError(`projected measurement ticket ${terminal.ticket} has multiple terminals`);
    }
    seen.add(terminal.ticket);
  }
}

function validateProjectedFrameSubmission(frame) {
  const submission = frame.projectedMeasurementSubmission;
  if (PROJECTED_POLICY_IDS[frame.projectedPolicy] === undefined) {
    throw new TypeError(`unknown projected policy ${frame.projectedPolicy}`);
  }
  if (!PROJECTED_EXECUTIONS.includes(frame.projectedExecution)) {
    throw new TypeError(`unknown projected execution ${frame.projectedExecution}`);
  }
  if (!PROJECTED_ADAPTIVE_STATES.includes(frame.projectedAdaptiveState)) {
    throw new TypeError(`unknown projected adaptive state ${frame.projectedAdaptiveState}`);
  }
  if (!["not_requested", "issued", "unsampled"].includes(submission)) {
    throw new TypeError(`unknown projected measurement submission ${submission}`);
  }
  if (frame.projectedPolicy !== "adaptive") {
    if (frame.projectedExecution !== frame.projectedPolicy
        || frame.projectedAdaptiveState !== "disabled"
        || submission !== "not_requested"
        || frame.projectedMeasurementTicket !== null
        || frame.projectedMeasurementExecution !== null
        || frame.projectedMeasurementUnsampledReason !== null) {
      throw new TypeError(
        "forced projected policy must match execution, disable adaptation, and expose no sample",
      );
    }
    return;
  }
  if (submission === "issued") {
    projectedTicketSafeInteger(
      frame.projectedMeasurementTicket,
      "projectedMeasurementTicket",
    );
    if (!PROJECTED_EXECUTIONS.includes(frame.projectedMeasurementExecution)
        || frame.projectedMeasurementExecution !== frame.projectedExecution
        || frame.projectedMeasurementUnsampledReason !== null) {
      throw new TypeError("issued projected measurement has invalid ticket or execution");
    }
    return;
  }
  if (submission === "unsampled") {
    if (frame.projectedMeasurementTicket !== null
        || !PROJECTED_EXECUTIONS.includes(frame.projectedMeasurementExecution)
        || frame.projectedMeasurementExecution !== frame.projectedExecution
        || !["ring_busy", "surface_unavailable"].includes(
          frame.projectedMeasurementUnsampledReason,
        )) {
      throw new TypeError("unsampled projected measurement has invalid identity");
    }
    return;
  }
  if (frame.projectedMeasurementTicket !== null
      || frame.projectedMeasurementExecution !== null
      || frame.projectedMeasurementUnsampledReason !== null) {
    throw new TypeError("not-requested projected measurement must not expose identity");
  }
}

function normalizeOrderMeasurementFailure(raw) {
  const ticket = orderTicketSafeInteger(raw?.ticket, "order failure ticket");
  const actualBackend = String(
    raw?.actualBackend ?? (ticket % 2 === 0 ? "cpu" : "gpu"),
  );
  if (!["cpu", "gpu"].includes(actualBackend)
      || (actualBackend === "cpu") !== (ticket % 2 === 0)) {
    throw new TypeError(`order failure ticket ${ticket} changed backend namespace`);
  }
  return {
    ticket,
    cameraRevision: telemetrySafeInteger(raw?.cameraRevision, "order failure cameraRevision"),
    actualBackend,
    reason: String(raw?.reason ?? "unknown"),
  };
}

function normalizeCpuOrderMeasurement(raw) {
  const ticket = orderTicketSafeInteger(raw?.ticket, "CPU order measurement ticket");
  if (ticket % 2 !== 0) {
    throw new TypeError(`CPU order ticket ${ticket} is outside the CPU namespace`);
  }
  return {
    ticket,
    cameraRevision: telemetrySafeInteger(
      raw?.cameraRevision,
      "CPU order measurement cameraRevision",
    ),
    actualBackend: String(raw?.actualBackend ?? "cpu"),
    preprocessMs: numberOr(raw?.preprocessMs, 0),
    sortMs: numberOr(raw?.sortMs, 0),
    frameCompleteMs: numberOr(raw?.frameCompleteMs, 0),
    countSemantics: raw?.countSemantics == null ? null : String(raw.countSemantics),
    visibleCount: nullableNumber(raw?.visibleCount),
    contributorCount: nullableNumber(raw?.contributorCount),
    drawnCount: nullableNumber(raw?.drawnCount),
    exactContributorCompaction: raw?.exactContributorCompaction == null
      ? null
      : Boolean(raw.exactContributorCompaction),
  };
}

function normalizeOrderMeasurement(raw) {
  const ticket = orderTicketSafeInteger(raw?.ticket, "GPU order measurement ticket");
  if (ticket % 2 !== 1) {
    throw new TypeError(`GPU order ticket ${ticket} is outside the GPU namespace`);
  }
  return {
    ticket,
    cameraRevision: telemetrySafeInteger(
      raw?.cameraRevision,
      "GPU order measurement cameraRevision",
    ),
    actualBackend: String(raw?.actualBackend ?? "gpu"),
    timingSource: String(raw?.timingSource ?? "completion_only"),
    gpuPreprocessMs: nullableNumber(raw?.gpuPreprocessMs),
    gpuRadixMs: nullableNumber(raw?.gpuRadixMs),
    gpuOrderMs: nullableNumber(raw?.gpuOrderMs),
    gpuCompleteMs: numberOr(raw?.gpuCompleteMs, 0),
    timestampPeriodNs: nullableNumber(raw?.timestampPeriodNs),
    belowTimestampResolution: Boolean(raw?.belowTimestampResolution),
    countSemantics: raw?.countSemantics == null ? null : String(raw.countSemantics),
    visibleCount: nullableNumber(raw?.visibleCount),
    contributorCount: nullableNumber(raw?.contributorCount),
    drawnCount: nullableNumber(raw?.drawnCount),
    exactContributorCompaction: raw?.exactContributorCompaction == null
      ? null
      : Boolean(raw.exactContributorCompaction),
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

function matchesCameraReceiptLength(length) {
  return length === 10 || length === 11;
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

function nullableNumber(value) {
  if (value == null) return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function telemetrySafeInteger(value, name, positive = false) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < (positive ? 1 : 0)) {
    throw new TypeError(
      `${name} must be a ${positive ? "positive" : "non-negative"} JS-safe integer`,
    );
  }
  return number;
}

function projectedTicketSafeInteger(value, name) {
  const ticket = telemetrySafeInteger(value, name, true);
  if (ticket < MIN_PROJECTED_TICKET) {
    throw new TypeError(`${name} is outside the projected ticket namespace`);
  }
  return ticket;
}

function gpuProducerTicketSafeInteger(value, name) {
  const ticket = telemetrySafeInteger(value, name, true);
  if (ticket < MIN_GPU_PRODUCER_TICKET || ticket > MAX_GPU_PRODUCER_TICKET) {
    throw new TypeError(`${name} is outside the GPU producer ticket namespace`);
  }
  return ticket;
}

function orderTicketSafeInteger(value, name) {
  const ticket = telemetrySafeInteger(value, name, true);
  if (ticket > MAX_ORDER_TICKET) {
    throw new TypeError(`${name} is outside the order ticket namespace`);
  }
  return ticket;
}

function nullableSafeInteger(value, name) {
  return value == null ? null : telemetrySafeInteger(value, name);
}

function nullableBoolean(value) {
  return value == null ? null : Boolean(value);
}

function strictBoolean(value, name) {
  if (value === true || value === 1) return true;
  if (value === false || value === 0) return false;
  throw new TypeError(`${name} must be a boolean`);
}

function nonNegativeFiniteNumber(value, name) {
  const number = Number(value);
  if (!Number.isFinite(number) || number < 0) {
    throw new TypeError(`${name} must be a non-negative finite number`);
  }
  return number;
}

function structuredFailure(error, stage, options = {}) {
  const scenePublished = Boolean(options.scenePublished);
  if (error instanceof GsplatWebError && error.scene_published === scenePublished) {
    return error;
  }
  return new GsplatWebError(error, stage, { scenePublished });
}

function structuredRuntimeMutationFailure(error, stage) {
  if (error instanceof GsplatWebError
      && error.stage === stage
      && error.scene_published === true) {
    return error;
  }
  return new GsplatWebError(error, stage, { scenePublished: true });
}

function failureMessage(error) {
  if (typeof error?.error_message === "string" && error.error_message.length > 0) {
    return error.error_message;
  }
  if (typeof error?.message === "string" && error.message.length > 0) {
    return error.message;
  }
  const message = String(error ?? "unknown gsplat-web failure");
  return message.length > 0 ? message : "unknown gsplat-web failure";
}

function failureCode(error, message, stage) {
  const supplied = [error?.error_code, error?.errorCode]
    .find((value) => typeof value === "string" && value.length > 0);
  if (supplied) {
    return supplied;
  }

  const normalized = message.toLowerCase();
  if (failureResource(error, message)
      || normalized.includes("capacity exceeded")
      || normalized.includes("resource limit")) {
    return "capacity_exceeded";
  }
  if (normalized.includes("out of memory")
      || normalized.includes("allocation failed")
      || normalized.includes("failed to reserve")) {
    return "out_of_memory";
  }
  if (normalized.includes("unsupported")
      || normalized.includes("does not support")
      || normalized.includes("exceed the device")
      || normalized.includes("exceeds effective device")) {
    return "unsupported";
  }
  if (normalized.includes("invalid argument") || normalized.startsWith("invalid ")) {
    return "invalid_argument";
  }
  if (normalized.includes("parse")
      || normalized.includes("malformed")
      || normalized.includes("missing required field")
      || normalized.includes("ply stream")) {
    return "parse_failed";
  }
  if (stage === FAILURE_STAGES.gpuOrderPrepare) {
    return "gpu_order_prepare_failed";
  }
  if (stage === FAILURE_STAGES.rendererConfigure) {
    return "renderer_configuration_failed";
  }
  if (stage === FAILURE_STAGES.geometryPath) {
    return "geometry_path_failed";
  }
  if (stage === FAILURE_STAGES.projectedPolicy) {
    return "projected_policy_failed";
  }
  if (stage === FAILURE_STAGES.resize) {
    return "resize_failed";
  }
  return "internal";
}

function failureResource(error, message) {
  const supplied = normalizeFailureResource(error?.resource)
    ?? normalizeFailureResource({
      kind: (typeof error?.resource === "string" ? error.resource : null)
        ?? error?.resource_kind
        ?? error?.resourceKind,
      required_bytes: error?.required_bytes ?? error?.requiredBytes,
      limit_bytes: error?.limit_bytes ?? error?.limitBytes,
    });
  if (supplied) {
    return supplied;
  }

  const directRequirements = Array.from(message.matchAll(
    /DirectSceneResourceRequirement \{ resource: ([A-Za-z0-9_]+), required_bytes: ([\d_]+), limit_bytes: ([\d_]+), fits: false \}/g,
  ));
  if (directRequirements.length > 0) {
    const limitingKind = /limiting_resource: ([A-Za-z0-9_]+)/.exec(message)?.[1];
    const selected = directRequirements.find((match) => match[1] === limitingKind)
      ?? directRequirements[0];
    return byteResource(selected[1], selected[2], selected[3]);
  }

  const packedBinding = /failure: Some\(StorageBindingSize \{ required_bytes: ([\d_]+), limit_bytes: ([\d_]+) \}\)/.exec(message);
  if (packedBinding) {
    return byteResource("packed_storage_binding", packedBinding[1], packedBinding[2]);
  }

  const patterns = [
    /resident resource (.+?) (?:requires|needs) ([\d_]+) bytes(?: but the effective binding limit is|; binding limit is) ([\d_]+) bytes/i,
    /PLY resource limit exceeded for (.+?): requested ([\d_]+), limit ([\d_]+)/i,
    /(?:direct GPU order )?(.+?) buffer needs ([\d_]+) bytes; device storage-binding limit is ([\d_]+) bytes/i,
  ];
  for (const pattern of patterns) {
    const match = pattern.exec(message);
    if (match) {
      return byteResource(match[1], match[2], match[3]);
    }
  }

  const paged = /paged compact atlas requires ([\d_]+) hot-record bytes and ([\d_]+) order bytes, exceeding the effective ([\d_]+)-byte binding limit/i.exec(message);
  if (paged) {
    const hotBytes = parseByteCount(paged[1]);
    const orderBytes = parseByteCount(paged[2]);
    return hotBytes >= orderBytes
      ? byteResource("paged_hot_records", hotBytes, paged[3])
      : byteResource("paged_order", orderBytes, paged[3]);
  }
  return null;
}

function normalizeFailureResource(resource) {
  if (!resource || typeof resource !== "object") {
    return null;
  }
  const kind = resource.kind ?? resource.resource_kind ?? resource.resourceKind;
  const requiredBytes = resource.required_bytes ?? resource.requiredBytes;
  const limitBytes = resource.limit_bytes ?? resource.limitBytes;
  return byteResource(kind, requiredBytes, limitBytes);
}

function byteResource(kind, requiredBytes, limitBytes) {
  const normalizedKind = typeof kind === "string" ? kind.trim() : "";
  const required = parseByteCount(requiredBytes);
  const limit = parseByteCount(limitBytes);
  if (!normalizedKind || required == null || limit == null) {
    return null;
  }
  return {
    kind: normalizedKind,
    required_bytes: required,
    limit_bytes: limit,
  };
}

function parseByteCount(value) {
  if (typeof value === "string") {
    value = value.replaceAll("_", "");
  }
  const number = Number(value);
  return Number.isSafeInteger(number) && number >= 0 ? number : null;
}

function freeNative(handle) {
  try {
    handle?.free?.();
  } catch {
    // Never replace the configuration/capacity failure that triggered cleanup.
  }
}
