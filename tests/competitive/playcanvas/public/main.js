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

const EXPECTED_VERSION = '2.21.0-beta.14';
const EXPECTED_RUNTIME_REVISION = 'd5fe888';
const DATASET_URL = '/datasets/minimal_binary.ply';
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
  datasetUrl: DATASET_URL
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
  runtimeSignals.rendererResolved = rendererLabel(app.scene.gsplat.currentRenderer);

  const asset = new Asset('minimal_binary', 'gsplat', { url: DATASET_URL });
  // GSplatComponent resolves asset ids through the application's registry. An
  // AssetListLoader can load a standalone asset, but that alone does not make
  // it discoverable by the component's AssetReference.
  app.assets.add(asset);
  await loadAsset(app, asset);
  if (!asset.resource) fail('minimal PLY loaded without a gsplat resource');

  const camera = new Entity('Camera');
  camera.addComponent('camera', {
    clearColor: new Color(0.02, 0.02, 0.03),
    nearClip: 0.01,
    farClip: 100
  });
  camera.setPosition(0, 0, 3);
  camera.lookAt(0, 0, 0);
  app.root.addChild(camera);

  const splat = new Entity('Splat');
  splat.addComponent('gsplat', { asset });
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
  const result = {
    status: 'ready_for_pre_timing_capture',
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
    datasetUrl: DATASET_URL,
    splatCount,
    canvasBackingWidth: canvas.width,
    canvasBackingHeight: canvas.height,
    devicePixelRatio: window.devicePixelRatio
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
