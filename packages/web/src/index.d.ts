export interface GsplatWebModule {
  default(moduleOrPath?: unknown): Promise<void>;
  api_version_major(): number;
  api_version_minor(): number;
  createRenderer(
    canvas: HTMLCanvasElement,
    plyBytes: Uint8Array,
    width: number,
    height: number,
  ): Promise<unknown>;
  createRendererWithGeometryPath?(
    canvas: HTMLCanvasElement,
    plyBytes: Uint8Array,
    width: number,
    height: number,
    geometryPath: 0 | 1 | 2,
  ): Promise<unknown>;
  createPackedPlyStream?(
    canvas: HTMLCanvasElement,
    width: number,
    height: number,
  ): GsplatPackedPlyStreamNative;
}

export interface GsplatPackedPlyStreamNative {
  pushChunk(bytes: Uint8Array): void;
  inputBytes(): number;
  peakDecoderBufferBytes(): number;
  finish(): Promise<unknown>;
  free?(): void;
}

export interface InitGsplatWebOptions {
  module?: GsplatWebModule;
  moduleUrl?: string | URL;
  wasmUrl?: string | URL | Response | ArrayBuffer | WebAssembly.Module;
}

export interface GsplatApiVersion {
  major: number;
  minor: number;
}

export type GsplatProjectedPolicy = "candidate" | "compact" | "adaptive";
export type GsplatProjectedExecution = "candidate" | "compact";
export type GsplatGpuOrderProducer = "post-sort" | "preproject";
export type GsplatExactPlan = "cpu_post_sort" | "gpu_post_sort" | "gpu_preproject";
export type GsplatCurrentStatsUnsampledReason =
  | "busy"
  | "gpu_unavailable"
  | "resource_unavailable"
  | "ticket_exhausted";
export type GsplatProjectedAdaptiveState =
  | "disabled"
  | "candidate_learning"
  | "candidate_stable"
  | "compact_probe"
  | "compact_stable"
  | "candidate_probe"
  | "candidate_only"
  | "cooldown";

export interface CreateRendererOptions {
  canvas: HTMLCanvasElement;
  plyBytes: Uint8Array | ArrayBuffer | ArrayBufferView;
  width?: number;
  height?: number;
  sortInterval?: number;
  /** Full-scene ordering policy. Defaults to runtime-adaptive CPU/GPU selection. */
  orderBackend?: "cpu" | "gpu" | "adaptive";
  /** Exact projected draw policy, independent of CPU/GPU ordering. Defaults to adaptive. */
  projectedPolicy?: GsplatProjectedPolicy;
  /**
   * Strict diagnostic selector inside the GPU lane. When set, construction
   * requires Packed geometry plus forced Compact projected drawing and enables
   * ticketed producer evidence. The product default remains PostSort.
   */
  gpuOrderProducer?: GsplatGpuOrderProducer | null;
  /** Resident geometry layout. Defaults to the exact compact `packed` path. */
  geometryPath?: "direct" | "packed" | "paged";
  module?: GsplatWebModule;
}

export interface CreateRendererFromUrlOptions
  extends Omit<CreateRendererOptions, "plyBytes"> {
  url: string | URL;
  fetchOptions?: RequestInit;
  onProgress?: (progress: {
    receivedBytes: number;
    totalBytes: number | null;
  }) => void;
}

export interface CreateRendererFromStreamOptions
  extends Omit<CreateRendererOptions, "plyBytes"> {
  stream: ReadableStream<Uint8Array>;
  totalBytes?: number | null;
  onProgress?: (progress: {
    receivedBytes: number;
    totalBytes: number | null;
  }) => void;
}

export interface GsplatSceneSummary {
  gaussians: number;
  shDegree: number;
  hasShRest: boolean;
}

export interface GsplatLoadReceipt {
  transportBytes: number;
  peakDecoderBufferBytes: number;
  streamed: boolean;
  inputSha256: string;
  sourceCount: number;
  decodedCount: number;
  encodedCount: number;
  residentCount: number;
  addressableCount: number;
  sourceShDegree: number;
  residentShDegree: number;
  /** Compatibility alias for residentShDegree. */
  shDegree: number;
  fullQuality: boolean;
  sourceMembership: "all" | string;
  samplingEnabled: boolean;
  lodEnabled: boolean;
  partialScenePublished: boolean;
}

export interface GsplatSurfaceSize {
  width: number;
  height: number;
}

export interface GsplatFailureResource {
  kind: string;
  required_bytes: number;
  limit_bytes: number;
}

export type GsplatFailureStage =
  | "renderer_create"
  | "renderer_configure"
  | "gpu_order_prepare"
  | "gpu_order_producer"
  | "stream_create"
  | "stream_decode"
  | "stream_finish"
  | "geometry_path"
  | "projected_policy"
  | "resize";

export interface GsplatWebErrorOptions {
  /** False during construction; true when a live scene survives a runtime failure. */
  scenePublished?: boolean;
}

/**
 * Fail-closed renderer error. Construction failures report
 * `scene_published=false`; runtime resize and geometry-path failures report
 * true because the previously published scene remains owned by the renderer.
 * Capacity details are included when the native failure identifies the
 * limiting buffer.
 */
export class GsplatWebError extends Error {
  constructor(error: unknown, stage: GsplatFailureStage, options?: GsplatWebErrorOptions);
  readonly stage: GsplatFailureStage;
  readonly error_code: string;
  readonly error_message: string;
  readonly scene_published: boolean;
  readonly resource?: GsplatFailureResource;
  readonly cause?: unknown;
}

export interface GsplatFrameStats {
  currentStatsSubmission: "not_requested" | "issued";
  currentStatsTicket: number | null;
  currentStatsPlan: GsplatExactPlan | null;
  currentStatsSceneGeneration: number | null;
  currentStatsCameraRevision: number | null;
  currentStatsViewportGeneration: number | null;
  currentStatsContractGeneration: number | null;
  currentStatsPlanSetGeneration: number | null;
  currentStatsOrderGeneration: number | null;
  currentStatsRasterGeneration: number | null;
  currentStatsEncodeAttempt: number | null;
  currentStatsPresentationSequence: number | null;
  frameMs: number;
  preprocessMs: number;
  sortMs: number;
  /** Compatibility field; always zero for the direct production pipeline. */
  rasterMs: number;
  cpuGeometryMs: number;
  renderSubmitMs: number;
  frameWallMs: number;
  framePresented: boolean;
  gpuOrderPreparationPending: boolean;
  /** Compatibility alias for gpuOrderPreparationPending. */
  tiledPreparationPending: boolean;
  rasterExecutionPlan: "global_quads" | "projected_quads_exact" | "tiled_exact";
  /** Null while renderer-owned Exact count evidence is pending or unavailable. */
  visibleCount: number | null;
  /** Null while renderer-owned Exact count evidence is pending or unavailable. */
  drawnCount: number | null;
  refreshSort: boolean;
  orderBackend: "cpu" | "gpu";
  adaptiveGpuFailure:
    | "unsupported"
    | "initialization"
    | "out_of_memory"
    | "validation"
    | null;
  gpuSortFallback: boolean;
  adaptiveState:
    | "disabled"
    | "cpu_learning"
    | "cpu_stable"
    | "gpu_probe"
    | "gpu_stable"
    | "cpu_probe"
    | "cooldown";
  /** Requested projected policy, independent of orderBackend. */
  projectedPolicy: GsplatProjectedPolicy;
  /** Candidate or Compact path that actually rendered this frame. */
  projectedExecution: GsplatProjectedExecution;
  projectedAdaptiveState: GsplatProjectedAdaptiveState;
  projectedMeasurementSubmission: "not_requested" | "issued" | "unsampled";
  /** JS-safe independent projected ticket, issued only by an Adaptive formal sample. */
  projectedMeasurementTicket: number | null;
  projectedMeasurementExecution: GsplatProjectedExecution | null;
  projectedMeasurementUnsampledReason:
    | "ring_busy"
    | "surface_unavailable"
    | null;
  /** Producer used by a GPU frame; null when the CPU supplied this frame. */
  gpuOrderProducer: GsplatGpuOrderProducer | null;
  gpuProducerMeasurementSubmission: "not_requested" | "issued" | "unsampled";
  gpuProducerMeasurementTicket: number | null;
  gpuProducerMeasurementProducer: GsplatGpuOrderProducer | null;
  gpuProducerMeasurementUnsampledReason:
    | "ring_busy"
    | "surface_unavailable"
    | null;
  cameraRevision: number;
  appliedOrderRevision: number;
  presentedOrderRevisionLag: number;
  submittedMeasurementTicket: number | null;
  submittedMeasurementBackend: "cpu" | "gpu" | null;
  measurementUnsampledReason: "ring_busy" | "surface_unavailable" | null;
  visibleCountRevision: number | null;
  visibleCountPending: boolean;
  gpuTimestampQueriesEnabled: boolean;
  completedMeasurementAvailable: boolean;
  completedMeasurementTicket: number | null;
  completedMeasurementRevision: number | null;
  completedMeasurementTimingSource: "timestamp_query" | "completion_only" | null;
  gpuPreprocessMs: number | null;
  gpuRadixMs: number | null;
  gpuOrderMs: number | null;
  gpuCompleteMs: number | null;
  gpuTimestampPeriodNs: number | null;
  gpuBelowTimestampResolution: boolean | null;
  completedVisibleCount: number | null;
  completedContributorCount: number | null;
  completedDrawnCount: number | null;
  completedExactContributorCompaction: boolean | null;
  failedMeasurementAvailable: boolean;
  failedMeasurementTicket: number | null;
  failedMeasurementRevision: number | null;
  failedMeasurementReason: GsplatOrderMeasurementFailureReason | null;
  completedProjectedMeasurementAvailable: boolean;
  completedProjectedMeasurementTicket: number | null;
  completedProjectedMeasurementRevision: number | null;
  completedProjectedMeasurementExecution: GsplatProjectedExecution | null;
  completedProjectedMeasurementOrderBackend: "cpu" | "gpu" | null;
  completedProjectedProjectionGeneration: number | null;
  completedProjectedProbeGeneration: number | null;
  completedProjectedProjectionRebuilt: boolean | null;
  completedProjectedOrderRefreshed: boolean | null;
  completedProjectedFrameCompleteMs: number | null;
  completedProjectedVisibleCount: number | null;
  completedProjectedContributorCount: number | null;
  completedProjectedDrawnCount: number | null;
  completedProjectedExactContributorCompaction: boolean | null;
  failedProjectedMeasurementAvailable: boolean;
  failedProjectedMeasurementTicket: number | null;
  failedProjectedMeasurementRevision: number | null;
  failedProjectedMeasurementExecution: GsplatProjectedExecution | null;
  failedProjectedMeasurementOrderBackend: "cpu" | "gpu" | null;
  failedProjectedProjectionGeneration: number | null;
  failedProjectedProbeGeneration: number | null;
  failedProjectedMeasurementReason: GsplatProjectedMeasurementFailureReason | null;
  completedGpuProducerMeasurementAvailable: boolean;
  completedGpuProducerMeasurementTicket: number | null;
  completedGpuProducerMeasurementRevision: number | null;
  completedGpuProducerMeasurementProducer: GsplatGpuOrderProducer | null;
  completedGpuProducerOrderGeneration: number | null;
  completedGpuProducerProjectionGeneration: number | null;
  completedGpuProducerSourceCount: number | null;
  completedGpuProducerContributorCount: number | null;
  completedGpuProducerDrawnCount: number | null;
  completedGpuProducerOrderRefreshed: boolean | null;
  completedGpuProducerDrawScope:
    | "exact_current_contributors"
    | "stale_order_candidates"
    | null;
  completedGpuProducerExactCurrentContributorDraw: boolean | null;
  completedGpuProducerStaleOrder: boolean | null;
  completedGpuProducerQueueCompleteMs: number | null;
  failedGpuProducerMeasurementAvailable: boolean;
  failedGpuProducerMeasurementTicket: number | null;
  failedGpuProducerMeasurementRevision: number | null;
  failedGpuProducerMeasurementProducer: GsplatGpuOrderProducer | null;
  failedGpuProducerOrderGeneration: number | null;
  failedGpuProducerProjectionGeneration: number | null;
  failedGpuProducerMeasurementReason: GsplatGpuProducerMeasurementFailureReason | null;
  /** All CPU-order queue-completion receipts since the preceding renderFrame call. */
  completedCpuOrderMeasurements: GsplatCpuOrderMeasurement[];
  /** All GPU-order receipts completed since the preceding renderFrame call. */
  completedOrderMeasurements: GsplatOrderMeasurement[];
  /** All terminal CPU/GPU order failures observed since the preceding renderFrame call. */
  failedOrderMeasurements: GsplatOrderMeasurementFailure[];
  /** All completed Adaptive projected samples since the preceding drain. */
  completedProjectedMeasurements: GsplatProjectedMeasurement[];
  /** All terminal failures for issued projected tickets since the preceding drain. */
  failedProjectedMeasurements: GsplatProjectedMeasurementFailure[];
  /** All completed strict GPU-producer samples since the preceding drain. */
  completedGpuProducerMeasurements: GsplatGpuProducerMeasurement[];
  /** All terminal failures for strict GPU-producer tickets since the preceding drain. */
  failedGpuProducerMeasurements: GsplatGpuProducerMeasurementFailure[];
  surfaceWidth: number;
  surfaceHeight: number;
  /** Native internal raster width. Exact benchmarks require this to equal surfaceWidth. */
  internalRenderWidth: number;
  /** Native internal raster height. Exact benchmarks require this to equal surfaceHeight. */
  internalRenderHeight: number;
  /** Width of the texture actually presented this frame, or null before presentation. */
  presentedWidth: number | null;
  /** Height of the texture actually presented this frame, or null before presentation. */
  presentedHeight: number | null;
}

export interface GsplatOrderMeasurement {
  ticket: number;
  cameraRevision: number;
  actualBackend: "gpu";
  timingSource: "timestamp_query" | "completion_only";
  gpuPreprocessMs: number | null;
  gpuRadixMs: number | null;
  gpuOrderMs: number | null;
  gpuCompleteMs: number;
  timestampPeriodNs: number | null;
  belowTimestampResolution: boolean;
  countSemantics: "candidate_visible_contributor_issued_v1" | null;
  visibleCount: number | null;
  contributorCount: number | null;
  drawnCount: number | null;
  exactContributorCompaction: boolean | null;
}

export interface GsplatCpuOrderMeasurement {
  ticket: number;
  cameraRevision: number;
  actualBackend: "cpu";
  preprocessMs: number;
  sortMs: number;
  frameCompleteMs: number;
  countSemantics: "candidate_visible_contributor_issued_v1" | null;
  visibleCount: number | null;
  contributorCount: number | null;
  drawnCount: number | null;
  exactContributorCompaction: boolean | null;
}

export type GsplatOrderMeasurementFailureReason =
  | "readback_map"
  | "generation_invalidated";

export interface GsplatOrderMeasurementFailure {
  ticket: number;
  cameraRevision: number;
  actualBackend: "cpu" | "gpu";
  reason: GsplatOrderMeasurementFailureReason;
}

export interface GsplatProjectedMeasurement {
  /** Projected tickets occupy the exact JS-safe high namespace beginning at 2^52. */
  ticket: number;
  cameraRevision: number;
  execution: GsplatProjectedExecution;
  orderBackend: "cpu" | "gpu";
  projectionGeneration: number;
  probeGeneration: number;
  projectionRebuilt: boolean;
  orderRefreshed: boolean;
  frameCompleteMs: number;
  countSemantics: "candidate_visible_contributor_issued_v1" | null;
  visibleCount: number | null;
  contributorCount: number | null;
  drawnCount: number | null;
  exactContributorCompaction: boolean | null;
}

export type GsplatProjectedMeasurementFailureReason =
  | "readback_map"
  | "generation_invalidated"
  | "invariant_violation";

export interface GsplatProjectedMeasurementFailure {
  ticket: number;
  cameraRevision: number;
  execution: GsplatProjectedExecution;
  orderBackend: "cpu" | "gpu";
  projectionGeneration: number;
  probeGeneration: number;
  reason: GsplatProjectedMeasurementFailureReason;
}

export interface GsplatGpuProducerMeasurement {
  /** Producer tickets occupy [2^51, 2^52) in the JS-safe namespace. */
  ticket: number;
  cameraRevision: number;
  producer: GsplatGpuOrderProducer;
  orderGeneration: number;
  projectionGeneration: number;
  countSemantics: "source_contributor_issued_v1";
  sourceCount: number;
  contributorCount: number;
  drawnCount: number;
  orderRefreshed: boolean;
  drawScope: "exact_current_contributors" | "stale_order_candidates";
  exactCurrentContributorDraw: boolean;
  staleOrder: boolean;
  queueCompleteMs: number;
}

export type GsplatGpuProducerMeasurementFailureReason =
  | "readback_map"
  | "generation_invalidated"
  | "invariant_violation";

export interface GsplatGpuProducerMeasurementFailure {
  ticket: number;
  cameraRevision: number;
  producer: GsplatGpuOrderProducer;
  orderGeneration: number;
  projectionGeneration: number;
  reason: GsplatGpuProducerMeasurementFailureReason;
}

export interface GsplatOrderMeasurementReceipts {
  completedCpuOrderMeasurements: GsplatCpuOrderMeasurement[];
  completedOrderMeasurements: GsplatOrderMeasurement[];
  failedOrderMeasurements: GsplatOrderMeasurementFailure[];
  completedProjectedMeasurements: GsplatProjectedMeasurement[];
  failedProjectedMeasurements: GsplatProjectedMeasurementFailure[];
  completedGpuProducerMeasurements: GsplatGpuProducerMeasurement[];
  failedGpuProducerMeasurements: GsplatGpuProducerMeasurementFailure[];
}

export type GsplatCurrentStatsRequest =
  | { status: "requested"; reason: null }
  | { status: "unsampled"; reason: GsplatCurrentStatsUnsampledReason };

export interface GsplatCurrentStatsIdentity {
  ticket: number;
  plan: GsplatExactPlan;
  sceneGeneration: number;
  cameraRevision: number;
  viewportGeneration: number;
  contractGeneration: number;
  planSetGeneration: number;
  orderGeneration: number;
  rasterGeneration: number;
  encodeAttempt: number;
  presentationSequence: number;
}

export type GsplatCurrentStatsPoll =
  | { status: "empty" }
  | { status: "unsampled"; reason: GsplatCurrentStatsUnsampledReason }
  | (GsplatCurrentStatsIdentity & {
      status: "map_failure" | "generation_invalidated" | "expired" | "dropped";
    })
  | (GsplatCurrentStatsIdentity & {
      status: "ready";
      countSemantics:
        | "draw_equals_visible"
        | "indirect_draw_equals_visible"
        | "indirect_draw_equals_contributor";
      sourceCount: number;
      visibleCount: number;
      contributorCount: number;
      drawnCount: number;
    });

export class GsplatWebRenderer {
  readonly isDisposed: boolean;
  /** Transactionally resize and resolve only after native publication. */
  resize(width: number, height: number): Promise<void>;
  resetCamera(): void;
  orbit(deltaYawRadians: number, deltaPitchRadians: number): void;
  zoom(distanceScale: number): void;
  pan(normalizedDeltaX: number, normalizedDeltaY: number): void;
  setSortInterval(interval: number): void;
  /**
   * Legacy compatibility entrypoint. Native same-path calls are idempotent;
   * changed-path calls fail closed instead of performing a synchronous switch.
   */
  setGeometryPath(path: "direct" | "packed" | "paged"): void;
  /**
   * Request a transactional Direct/Packed switch after native publication.
   * Packed streams may reject a later Direct request when wide source planes
   * were intentionally not retained; the previously published path survives.
   */
  setGeometryPathAsync(path: "direct" | "packed" | "paged"): Promise<void>;
  setOrderBackend(backend: "cpu" | "gpu" | "adaptive"): void;
  /** Fail-closed transactional switch of the independent projected draw policy. */
  setProjectedPolicy(policy: GsplatProjectedPolicy): void;
  /** Transactionally select and enable strict diagnostics for one GPU producer. */
  setGpuOrderProducerAsync(producer: GsplatGpuOrderProducer): Promise<void>;
  rasterPath(): string;
  renderFrame(): GsplatFrameStats;
  /** Drain terminal receipts even when rendering itself failed. */
  drainOrderMeasurementReceipts(): GsplatOrderMeasurementReceipts;
  /** Request one renderer-owned S/V/C/D receipt from the next presented Exact frame. */
  requestCurrentStats(): GsplatCurrentStatsRequest;
  /** Poll at most one terminal without submitting or blocking on another frame. */
  pollCurrentStats(): GsplatCurrentStatsPoll;
  sceneSummary(): GsplatSceneSummary;
  loadReceipt(): GsplatLoadReceipt | null;
  surfaceSize(): GsplatSurfaceSize;
  free(): void;
  dispose(): void;
}

export const GSPLAT_WEB_SDK_VERSION: "0.1.3";

export function initGsplatWeb(options?: InitGsplatWebOptions): Promise<GsplatWebModule>;
export function getGsplatApiVersion(module?: GsplatWebModule): GsplatApiVersion;
export function createGsplatRenderer(options: CreateRendererOptions): Promise<GsplatWebRenderer>;
export function createGsplatRendererFromUrl(
  options: CreateRendererFromUrlOptions,
): Promise<GsplatWebRenderer>;
export function createGsplatRendererFromStream(
  options: CreateRendererFromStreamOptions,
): Promise<GsplatWebRenderer>;
