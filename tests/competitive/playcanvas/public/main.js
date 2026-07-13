import {
  Application,
  Asset,
  AssetListLoader,
  Color,
  DEVICETYPE_WEBGPU,
  Entity,
  FILLMODE_NONE,
  GSPLAT_RENDERER_RASTER_GPU_SORT,
  RESOLUTION_FIXED,
  createGraphicsDevice,
  revision,
  version
} from 'playcanvas';
import { createRasterDiagnosticPly, RASTER_DIAGNOSTIC_ID } from '/perf/raster-diagnostic.mjs';

const EXPECTED_VERSION = '2.21.0-beta.14';
const EXPECTED_RUNTIME_REVISION = 'd5fe888';
const params = new URLSearchParams(window.location.search);
const benchmarkMode = params.get('benchmark') === '1';
const qualificationName = params.get('qualification');
const qualificationMode = ['kitsune-static-v1', 'minimal-static-v1', 'raster-diagnostic-v1'].includes(qualificationName);
const DATASET_URL = qualificationName === 'kitsune-static-v1'
  ? '/datasets/external/wakufactory_kitune/kitune1.ply'
  : '/datasets/minimal_binary.ply';
const TRACE_URL = qualificationName === 'minimal-static-v1' || qualificationName === 'raster-diagnostic-v1'
  ? '/traces/phase-e-minimal-static-640x480-v1.json'
  : '/traces/phase-e-kitsune-static-640x480-v1.json';
const status = document.querySelector('#status');
const runtimeSignals = {
  engine: 'playcanvas',
  engineVersion: version,
  engineRuntimeRevision: revision,
  backendRequested: 'webgpu',
  backendSelected: null,
  deviceType: null,
  isWebGPU: null,
  isWebGL2: null,
  rendererRequested: 'raster_gpu_sort',
  rendererResolved: null,
  rendererActive: null,
  rendererPath: null,
  usesGpuSort: null,
  sourceFormat: 'ply',
  datasetUrl: qualificationName === 'raster-diagnostic-v1' ? 'generated:raster_diagnostic_v1' : DATASET_URL
};

function fail(message, details = {}) {
  const error = { status: 'blocked', message, signals: { ...runtimeSignals }, details };
  window.__PLAYCANVAS_HARNESS_ERROR__ = error;
  status.textContent = JSON.stringify(error, null, 2);
  console.error('PLAYCANVAS_HARNESS_BLOCKED', error);
  throw new Error(message);
}

function rendererLabel(value) {
  return value === GSPLAT_RENDERER_RASTER_GPU_SORT ? 'raster_gpu_sort' : `unknown:${value}`;
}

function centerBounds(centers) {
  if (!centers || centers.length < 3) return null;
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let offset = 0; offset + 2 < centers.length; offset += 3) {
    for (let axis = 0; axis < 3; axis += 1) {
      min[axis] = Math.min(min[axis], centers[offset + axis]);
      max[axis] = Math.max(max[axis], centers[offset + axis]);
    }
  }
  return { min, max };
}

async function loadAsset(app, asset) {
  const loader = new AssetListLoader([asset], app.assets);
  await new Promise((resolve, reject) => {
    loader.on('error', (error) => reject(error ?? new Error('asset load failed')));
    loader.load(resolve);
  });
}

async function nextFrames(count) {
  for (let i = 0; i < count; i += 1) {
    await new Promise(requestAnimationFrame);
  }
}

function activeSplatCount(manager, fallback) {
  const state = manager.world?.getState(manager.world.currentVersion);
  return state?.totalActiveSplats ?? fallback;
}

async function collectFrameSamples(app, manager, splatCount, warmupFrames, sampleFrames) {
  await nextFrames(warmupFrames);
  const measurementStartedAtUtc = new Date().toISOString();
  const measurementStart = performance.now();
  const samples = [];
  let frameStart = null;
  let frameWallMs = null;

  await new Promise((resolve) => {
    const frameUpdateEvent = app.on('frameupdate', (wallMs) => {
      if (frameStart === null && samples.length < sampleFrames) {
        frameStart = performance.now();
        frameWallMs = wallMs;
      }
    });
    const frameEndEvent = app.on('frameend', () => {
      if (frameStart === null) return;
      const end = performance.now();
      samples.push({
        elapsedNs: Math.round((end - measurementStart) * 1_000_000),
        callMs: end - frameStart,
        frameWallMs,
        activeSplats: activeSplatCount(manager, splatCount)
      });
      frameStart = null;
      frameWallMs = null;
      if (samples.length === sampleFrames) {
        frameUpdateEvent.off();
        frameEndEvent.off();
        resolve();
      }
    });
  });

  return {
    warmupCount: warmupFrames,
    samples,
    measurementStartedAtUtc,
    measurementEndedAtUtc: new Date().toISOString(),
    timing: {
      callSource: 'playcanvas frameupdate to frameend CPU boundary',
      frameWallSource: 'PlayCanvas frameupdate ms from requestAnimationFrame timestamps',
      countsSource: 'GSplatWorld current state totalActiveSplats'
    }
  };
}

async function main() {
  window.__PLAYCANVAS_HARNESS_STATE__ = 'initializing';
  if (version !== EXPECTED_VERSION || revision !== EXPECTED_RUNTIME_REVISION) {
    fail('runtime identity mismatch', { version, revision });
  }
  if (!navigator.gpu) {
    fail('navigator.gpu is unavailable; requested WebGPU cannot be verified');
  }

  const canvas = document.querySelector('#canvas');
  const device = await createGraphicsDevice(canvas, {
    deviceTypes: [DEVICETYPE_WEBGPU],
    antialias: false,
    powerPreference: 'high-performance'
  });
  if (!device?.isWebGPU || device.deviceType !== DEVICETYPE_WEBGPU) {
    fail('PlayCanvas did not create the requested WebGPU device', {
      deviceType: device?.deviceType ?? null,
      isWebGPU: device?.isWebGPU ?? false,
      isWebGL2: device?.isWebGL2 ?? false
    });
  }
  Object.assign(runtimeSignals, {
    backendSelected: device.deviceType,
    deviceType: device.deviceType,
    isWebGPU: device.isWebGPU,
    isWebGL2: device.isWebGL2
  });

  const app = new Application(canvas, { graphicsDevice: device });
  app.setCanvasFillMode(FILLMODE_NONE);
  app.setCanvasResolution(RESOLUTION_FIXED, 640, 480);
  app.graphicsDevice.maxPixelRatio = 1;
  app.scene.gsplat.renderer = GSPLAT_RENDERER_RASTER_GPU_SORT;
  if (qualificationMode) {
    app.scene.gsplat.alphaClipForward = 1 / 256;
    app.scene.gsplat.minPixelSize = 0;
    app.scene.gsplat.minContribution = 0;
    app.scene.gsplat.foveationStrength = 0;
    app.scene.gsplat.antiAlias = false;
  }
  runtimeSignals.rendererResolved = rendererLabel(app.scene.gsplat.currentRenderer);

  const diagnostic = qualificationName === 'raster-diagnostic-v1';
  const diagnosticUrl = diagnostic
    ? URL.createObjectURL(new Blob([createRasterDiagnosticPly()], { type: 'application/octet-stream' }))
    : null;
  const asset = new Asset(diagnostic ? RASTER_DIAGNOSTIC_ID : 'minimal_binary', 'gsplat', {
    url: diagnosticUrl ?? DATASET_URL,
    ...(diagnostic ? { filename: `${RASTER_DIAGNOSTIC_ID}.ply` } : {})
  });
  // GSplatComponent resolves asset ids through the application's registry. An
  // AssetListLoader can load a standalone asset, but that alone does not make
  // it discoverable by the component's AssetReference.
  app.assets.add(asset);
  await loadAsset(app, asset);
  if (!asset.resource) fail('minimal PLY loaded without a gsplat resource');

  const trace = qualificationMode ? await fetch(TRACE_URL).then((response) => response.json()) : null;
  const traceFrame = trace?.frames?.[0] ?? null;
  const camera = new Entity('Camera');
  camera.addComponent('camera', {
    clearColor: qualificationMode ? new Color(0, 0, 0) : new Color(0.02, 0.02, 0.03),
    fov: traceFrame ? (traceFrame.intrinsics.vertical_fov_radians * 180) / Math.PI : 45,
    nearClip: traceFrame?.intrinsics.near_plane ?? 0.01,
    farClip: traceFrame?.intrinsics.far_plane ?? 100
  });
  const tracePosition = traceFrame?.pose.position ?? [0, 0, 3];
  const cameraPosition = traceFrame
    ? [tracePosition[0], tracePosition[1], -tracePosition[2]]
    : tracePosition;
  const cameraTarget = traceFrame
    ? [cameraPosition[0], cameraPosition[1], cameraPosition[2] - 1]
    : [0, 0, 0];
  camera.setPosition(...cameraPosition);
  camera.lookAt(...cameraTarget);
  app.root.addChild(camera);

  const splat = new Entity('Splat');
  splat.addComponent('gsplat', { asset });
  if (qualificationMode) splat.setLocalScale(1, -1, -1);
  app.root.addChild(splat);
  app.start();
  await nextFrames(12);

  const director = app.renderer.gsplatDirector;
  const cameraData = director?.camerasMap?.get(camera.camera.camera);
  const managers = cameraData
    ? [...cameraData.layersMap.values()].map((entry) => entry.gsplatManager).filter(Boolean)
    : [];
  const manager = managers[0];
  const resolved = app.scene.gsplat.currentRenderer;
  const actualPath = manager?.renderer?.constructor?.name ?? null;
  const actualUsesGpuSort = manager?.renderer?.usesGpuSort ?? null;
  Object.assign(runtimeSignals, {
    rendererResolved: rendererLabel(resolved),
    rendererActive: manager ? rendererLabel(manager.activeRenderer) : null,
    rendererPath: actualPath,
    usesGpuSort: actualUsesGpuSort
  });

  if (resolved !== GSPLAT_RENDERER_RASTER_GPU_SORT) {
    fail('requested GPU-sort renderer did not resolve to GPU sort', { resolved });
  }
  if (!manager || manager.activeRenderer !== GSPLAT_RENDERER_RASTER_GPU_SORT || actualUsesGpuSort !== true) {
    const worldLayer = app.scene.layers.getLayerById(splat.gsplat.layers[0]);
    fail('actual gsplat manager path does not prove GPU sorting', {
      managerCount: managers.length,
      directorCameraCount: director?.camerasMap?.size ?? null,
      directorHasCamera: director?.camerasMap?.has(camera.camera.camera) ?? null,
      cameraLayerDataCount: cameraData?.layersMap?.size ?? null,
      cameraLayerManagerStates: cameraData
        ? [...cameraData.layersMap.entries()].map(([layer, entry]) => ({
            layerId: layer.id,
            placementCount: layer.gsplatPlacements?.length ?? null,
            hasManager: Boolean(entry.gsplatManager)
          }))
        : [],
      cameraLayers: camera.camera.layers,
      componentLayers: splat.gsplat.layers,
      componentUnified: splat.gsplat.unified,
      componentHasPlacement: Boolean(splat.gsplat._placement),
      worldPlacementCount: worldLayer?.gsplatPlacements?.length ?? null,
      activeRenderer: manager?.activeRenderer ?? null,
      actualPath,
      actualUsesGpuSort
    });
  }

  const splatCount = asset.resource.numSplats ?? asset.resource.gsplatData?.numSplats ?? null;
  const traceDescriptor = trace ?? {
    id: 'playcanvas-static-minimal-v1',
    camera: {
      position: [0, 0, 3],
      target: [0, 0, 0],
      verticalFovDegrees: camera.camera.fov,
      nearPlane: camera.camera.nearClip,
      farPlane: camera.camera.farClip
    },
    display: { width: canvas.width, height: canvas.height, dpr: window.devicePixelRatio }
  };
  const capture = benchmarkMode
    ? await collectFrameSamples(app, manager, splatCount, qualificationMode ? 120 : 30, qualificationMode ? 3600 : 60)
    : null;
  const result = {
    status: benchmarkMode ? 'raw_frame_capture_complete' : 'ready_for_pre_timing_capture',
    engine: 'playcanvas',
    engineVersion: version,
    engineRuntimeRevision: revision,
    backendRequested: 'webgpu',
    backendSelected: device.deviceType,
    isWebGPU: device.isWebGPU,
    isWebGL2: device.isWebGL2,
    rendererRequested: 'raster_gpu_sort',
    rendererResolved: rendererLabel(resolved),
    rendererActive: rendererLabel(manager.activeRenderer),
    rendererPath: actualPath,
    usesGpuSort: actualUsesGpuSort,
    sourceFormat: 'ply',
    datasetUrl: runtimeSignals.datasetUrl,
    splatCount,
    sourceCenterBounds: centerBounds(asset.resource.centers),
    canvasBackingWidth: canvas.width,
    canvasBackingHeight: canvas.height,
    devicePixelRatio: window.devicePixelRatio,
    cameraReceipt: {
      position: camera.getPosition().toArray(),
      forward: camera.forward.toArray(),
      fovDegrees: camera.camera.fov,
      horizontalFov: camera.camera.horizontalFov,
      aspectRatio: camera.camera.aspectRatio,
      nearClip: camera.camera.nearClip,
      farClip: camera.camera.farClip,
      projectionMatrixColumnMajor: Array.from(camera.camera.projectionMatrix.data)
    },
    traceDescriptor,
    policies: {
      dynamicResolution: 'disabled_fixed_640x480',
      lod: 'disabled_full_ply',
      renderer: 'raster_gpu_sort',
      sourceCoordinateConversion: qualificationMode ? 'rdf_to_playcanvas_rub_entity_yz_reflection' : 'none',
      cameraCoordinateConversion: qualificationMode ? 'ruf_plus_z_to_playcanvas_minus_z' : 'none',
      alphaClipForward: qualificationMode ? 1 / 256 : app.scene.gsplat.alphaClipForward,
      minPixelSize: app.scene.gsplat.minPixelSize,
      minContribution: app.scene.gsplat.minContribution,
      foveationStrength: app.scene.gsplat.foveationStrength,
      antiAlias: app.scene.gsplat.antiAlias
    },
    capture
  };
  window.__PLAYCANVAS_HARNESS_RESULT__ = result;
  window.__PLAYCANVAS_HARNESS_STATE__ = 'ready';
  status.textContent = JSON.stringify(result, null, 2);
  console.log('PLAYCANVAS_HARNESS_READY', result);
}

main().catch((error) => {
  if (!window.__PLAYCANVAS_HARNESS_ERROR__) {
    const details = { name: error?.name, message: error?.message, stack: error?.stack };
    window.__PLAYCANVAS_HARNESS_ERROR__ = { status: 'blocked', message: 'unexpected harness failure', details };
    status.textContent = JSON.stringify(window.__PLAYCANVAS_HARNESS_ERROR__, null, 2);
    console.error('PLAYCANVAS_HARNESS_BLOCKED', window.__PLAYCANVAS_HARNESS_ERROR__);
  }
});
