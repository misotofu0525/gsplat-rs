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

export interface CreateRendererOptions {
  canvas: HTMLCanvasElement;
  plyBytes: Uint8Array | ArrayBuffer | ArrayBufferView;
  width?: number;
  height?: number;
  sortInterval?: number;
  module?: GsplatWebModule;
}

export interface CreateRendererFromUrlOptions
  extends Omit<CreateRendererOptions, "plyBytes"> {
  url: string | URL;
  fetchOptions?: RequestInit;
}

export interface GsplatSceneSummary {
  gaussians: number;
  shDegree: number;
  hasShRest: boolean;
}

export interface GsplatSurfaceSize {
  width: number;
  height: number;
}

export interface GsplatFrameStats {
  frameMs: number;
  preprocessMs: number;
  sortMs: number;
  rasterMs: number;
  visibleCount: number;
  drawnCount: number;
  refreshSort: boolean;
  surfaceWidth: number;
  surfaceHeight: number;
}

export class GsplatWebRenderer {
  readonly isDisposed: boolean;
  resize(width: number, height: number): void;
  resetCamera(): void;
  orbit(deltaYawRadians: number, deltaPitchRadians: number): void;
  zoom(distanceScale: number): void;
  pan(normalizedDeltaX: number, normalizedDeltaY: number): void;
  setSortInterval(interval: number): void;
  renderFrame(): GsplatFrameStats;
  sceneSummary(): GsplatSceneSummary;
  surfaceSize(): GsplatSurfaceSize;
  free(): void;
  dispose(): void;
}

export const GSPLAT_WEB_SDK_VERSION: "0.1.0";

export function initGsplatWeb(options?: InitGsplatWebOptions): Promise<GsplatWebModule>;
export function getGsplatApiVersion(module?: GsplatWebModule): GsplatApiVersion;
export function createGsplatRenderer(options: CreateRendererOptions): Promise<GsplatWebRenderer>;
export function createGsplatRendererFromUrl(
  options: CreateRendererFromUrlOptions,
): Promise<GsplatWebRenderer>;
