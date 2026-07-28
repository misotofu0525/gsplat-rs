import {
  Application,
  Asset,
  AssetListLoader,
  Camera,
  Color,
  DEVICETYPE_WEBGPU,
  Entity,
  FILLMODE_NONE,
  GSPLAT_RENDERER_RASTER_GPU_SORT,
  Mat4,
  RESOLUTION_FIXED,
  createGraphicsDevice,
  revision,
  version
} from 'playcanvas';
import { createRasterDiagnosticPly, RASTER_DIAGNOSTIC_ID } from '/perf/raster-diagnostic.mjs';
import {
  drainQueueWhileFrameLoopStopped,
  measuredFrameWallMs,
  requireQueueTerminalApi,
  resumeApplicationFrameLoop,
  stopApplicationFrameLoop,
  stopApplicationFrameLoopIfScheduled,
  summarizeSustainedMeasurement
} from '/harness/queue-terminal.js';
import {
  createPlayCanvasCameraReceipt,
  PLAYCANVAS_MIN_PRESENTATION_STABLE_FRAMES,
  PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA,
  traceFrameIndexForPhase,
  traceFrameToPlayCanvasPose,
  validatePlayCanvasCameraReceipt
} from '/harness/trace-camera.js';
import {
  captureBrowserPresentationState,
  shouldObserveMeasuredPresentation
} from '/harness/presentation-receipt.js';
import {
  beginPlayCanvasWebgpuRendererCapture,
  finalizePlayCanvasWebgpuRendererCapture,
  rgba8ToBase64
} from '/harness/webgpu-renderer-capture.js';
import { captureWebGpuEnvironmentReceipt } from '/harness/webgpu-environment-receipt.js';
import {
  captureQ1HostStartReceipt,
  createQ1HostStartGate
} from '/perf/q1-host-start-gate.mjs';

const EXPECTED_VERSION = '2.21.0-beta.14';
const EXPECTED_RUNTIME_REVISION = 'd5fe888';
const params = new URLSearchParams(window.location.search);
const benchmarkMode = params.get('benchmark') === '1';
const qualificationName = params.get('qualification');
const QUALIFICATIONS = Object.freeze({
  'minimal-static-v1': {
    datasetUrl: '/datasets/minimal_binary.ply',
    manifestUrl: '/perf/datasets/minimal_binary.json',
    traceUrl: '/traces/phase-e-minimal-static-640x480-v1.json',
    evidenceClass: 'diagnostic'
  },
  'minimal-presentation-2412x1080-v1': {
    datasetUrl: '/datasets/minimal_binary.ply',
    manifestUrl: '/perf/datasets/minimal_binary.json',
    traceUrl: '/traces/quality/candidate-truck-quality-2412x1080-v1.json',
    evidenceClass: 'presentation_diagnostic'
  },
  'raster-diagnostic-v1': {
    generated: true,
    traceUrl: '/traces/phase-e-minimal-static-640x480-v1.json',
    evidenceClass: 'diagnostic'
  },
  'kitsune-static-v1': {
    datasetUrl: '/datasets/external/wakufactory_kitune/kitune1.ply',
    manifestUrl: '/perf/datasets/kitsune.json',
    traceUrl: '/traces/phase-e-kitsune-static-640x480-v1.json',
    evidenceClass: 'diagnostic'
  },
  'flowers-quality-1080p-v1': {
    datasetUrl: '/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply',
    manifestUrl: '/perf/datasets/flowers.json',
    traceUrl: '/traces/quality/candidate-flowers-quality-1920x1080-v1.json',
    evidenceClass: 'competitor_qualification'
  },
  'bonsai-quality-1080p-v1': {
    datasetUrl: '/datasets/external/inria_3dgs/bonsai/point_cloud.ply',
    manifestUrl: '/datasets/external/inria_3dgs/bonsai/source.json',
    traceUrl: '/traces/quality/candidate-bonsai-quality-1920x1080-v1.json',
    evidenceClass: 'competitor_qualification'
  },
  'truck-quality-1080p-v1': {
    datasetUrl: '/datasets/external/inria_3dgs/truck/point_cloud.ply',
    manifestUrl: '/datasets/external/inria_3dgs/truck/source.json',
    traceUrl: '/traces/quality/candidate-truck-quality-1920x1080-v1.json',
    evidenceClass: 'competitor_qualification'
  },
  'truck-quality-2412x1080-v1': {
    datasetUrl: '/datasets/external/inria_3dgs/truck/point_cloud.ply',
    manifestUrl: '/datasets/external/inria_3dgs/truck/source.json',
    traceUrl: '/traces/quality/candidate-truck-quality-2412x1080-v1.json',
    evidenceClass: 'competitor_qualification'
  },
  'garden-quality-1080p-v1': {
    datasetUrl: '/datasets/external/inria_3dgs/garden/point_cloud.ply',
    manifestUrl: '/datasets/external/inria_3dgs/garden/source.json',
    traceUrl: '/traces/quality/candidate-garden-quality-1920x1080-v1.json',
    evidenceClass: 'competitor_qualification'
  },
  'bicycle-quality-1080p-v1': {
    datasetUrl: '/datasets/external/inria_3dgs/bicycle/point_cloud.ply',
    manifestUrl: '/datasets/external/inria_3dgs/bicycle/source.json',
    traceUrl: '/traces/quality/candidate-bicycle-quality-1920x1080-v1.json',
    evidenceClass: 'competitor_qualification'
  }
});
const qualification = qualificationName ? QUALIFICATIONS[qualificationName] : null;
const qualificationMode = Boolean(qualification);
const DATASET_URL = qualification?.datasetUrl ?? '/datasets/minimal_binary.ply';
const TRACE_URL = qualification?.traceUrl ?? null;
const requestedTraceFrameIndex = Number(params.get('trace_frame') ?? 0);
const requestedWarmupFrames = Number(params.get('warmup_frames') ?? (qualificationMode ? 120 : 30));
const requestedMeasuredFrames = Number(params.get('measured_frames') ?? (qualificationMode ? 3600 : 60));
const requestedCameraMode = params.get('camera_mode') ?? 'static';
const requestedCaptureTraceFrameParameter = params.get('capture_trace_frame');
const requestedCaptureTraceFrameIndex = requestedCaptureTraceFrameParameter === null
  ? null
  : Number(requestedCaptureTraceFrameParameter);
const rendererCaptureEnabled = params.get('renderer_capture') !== '0';
const hostStartGateParameter = params.get('host_start_gate');
if (hostStartGateParameter !== null && hostStartGateParameter !== '1') {
  throw new Error('host_start_gate must equal 1 when present');
}
const hostStartGateEnabled = hostStartGateParameter === '1';
const status = document.querySelector('#status');
// The status panel is useful for an interactive smoke, but it would obscure
// the top-left of a formal device presentation and contaminate an external
// screen receipt. Failures remain available through the harness result and
// host-side artifact log in benchmark mode.
status.hidden = benchmarkMode;
let activeApplication = null;
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
  datasetUrl: qualification?.generated ? 'generated:raster_diagnostic_v1' : DATASET_URL
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

async function sha256Hex(bytes) {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
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

function applyTraceFrameToCamera(camera, frame) {
  const pose = traceFrameToPlayCanvasPose(frame);
  camera.setPosition(...pose.position);
  camera.lookAt(...pose.target, ...pose.up);
  camera.camera.fov = (frame.intrinsics.vertical_fov_radians * 180) / Math.PI;
  camera.camera.nearClip = frame.intrinsics.near_plane;
  camera.camera.farClip = frame.intrinsics.far_plane;
  return pose;
}

const rawViewProjectionScratch = new Mat4();
const shaderProjectionScratch = new Mat4();
const shaderViewProjectionScratch = new Mat4();

function captureRuntimeCameraObservation(camera, graphicsDevice) {
  const runtimeCamera = camera.camera.camera;
  const view = runtimeCamera.viewMatrix;
  const projection = runtimeCamera.projectionMatrix;
  rawViewProjectionScratch.mul2(projection, view);
  const renderTargetFlipY = Boolean(runtimeCamera.renderTarget?.flipY);
  Camera.applyShaderProjectionTransform(
    projection,
    shaderProjectionScratch,
    renderTargetFlipY,
    graphicsDevice.isWebGPU
  );
  shaderViewProjectionScratch.mul2(shaderProjectionScratch, view);
  return {
    position: camera.getPosition().toArray(),
    forward: camera.forward.toArray(),
    up: camera.up.toArray(),
    verticalFovRadians: (runtimeCamera.fov * Math.PI) / 180,
    nearPlane: runtimeCamera.nearClip,
    farPlane: runtimeCamera.farClip,
    aspect: runtimeCamera.aspectRatio,
    horizontalFov: runtimeCamera.horizontalFov,
    renderTargetFlipY,
    webGpuDepthRangeApplied: graphicsDevice.isWebGPU,
    viewMatrixColumnMajor: Array.from(view.data),
    projectionMatrixOpenGlColumnMajor: Array.from(projection.data),
    viewProjectionMatrixOpenGlColumnMajor: Array.from(rawViewProjectionScratch.data),
    shaderProjectionMatrixWebGpuColumnMajor: Array.from(shaderProjectionScratch.data),
    shaderViewProjectionMatrixWebGpuColumnMajor:
      Array.from(shaderViewProjectionScratch.data)
  };
}

function applyAndCaptureTraceCamera({ app, camera, trace, traceFrameIndex, phase }) {
  const frame = trace?.frames?.[traceFrameIndex];
  if (!frame) throw new Error(`trace frame ${traceFrameIndex} is unavailable during ${phase}`);
  applyTraceFrameToCamera(camera, frame);
  return createPlayCanvasCameraReceipt({
    trace,
    traceFrameIndex,
    phase,
    observation: captureRuntimeCameraObservation(camera, app.graphicsDevice)
  });
}

async function collectFrameSamples(
  app,
  manager,
  splatCount,
  warmupFrames,
  sampleFrames,
  camera,
  trace,
  cameraMode,
  requestedCaptureFrameIndex,
  capturePresentation,
  presentationProbe,
  rendererCaptureIdentity
) {
  requireQueueTerminalApi(app.graphicsDevice);
  const traceFrames = trace?.frames ?? [];
  const samples = [];
  let frameStart = null;
  let frameWallMs = null;
  let frameSubmitVersionStart = null;
  let activeTraceFrameIndex = null;
  let activeCameraReceipt = null;
  let completedWarmupFrames = 0;
  let measurementStartedAtUtc = null;
  let measurementEndedAtUtc = null;
  let measurementStart = null;
  let measurementSubmitEnd = null;
  let measurementSubmitVersionStart = null;
  let measurementSubmitVersionEnd = null;
  let warmupDrain = null;
  let measurementDrain = null;
  let sustained = null;
  let preMeasurementPresentation = null;
  let postMeasurementPresentation = null;
  let postPresentationTerminalPresentation = null;
  let measurementPresentationCheckCount = 0;
  let presentationTraceFrameIndex = null;
  let presentationTraceFrameSource = null;
  let presentationDrain = null;
  let presentationCapture = null;
  let pendingRendererCapture = null;
  let rendererCapture = null;
  const presentationFrames = [];
  let state = warmupFrames > 0 ? 'warming_up' : 'draining_warmup';

  await new Promise((resolve, reject) => {
    let frameUpdateEvent;
    let frameEndEvent;

    const cleanup = () => {
      frameUpdateEvent?.off();
      frameEndEvent?.off();
    };

    const rejectCapture = (error) => {
      cleanup();
      if (app.frameRequestId !== null && app.frameRequestId !== undefined &&
          typeof app.constructor?.cancelTick === 'function') {
        app.constructor.cancelTick(app);
      }
      reject(error);
    };

    const applyCameraForWarmupOrMeasurement = () => {
      if (traceFrames.length === 0) return null;
      const phaseFrameIndex = state === 'warming_up' ? completedWarmupFrames : samples.length;
      activeTraceFrameIndex = traceFrameIndexForPhase(
        cameraMode,
        requestedTraceFrameIndex,
        traceFrames.length,
        phaseFrameIndex
      );
      const frame = traceFrames[activeTraceFrameIndex];
      if (!frame) throw new Error(`trace frame ${activeTraceFrameIndex} is unavailable`);
      if (state === 'warming_up') {
        applyTraceFrameToCamera(camera, frame);
        return null;
      }
      applyTraceFrameToCamera(camera, frame);
      return null;
    };

    const beginMeasurementAfterWarmupDrain = async () => {
      try {
        warmupDrain = await drainQueueWhileFrameLoopStopped(app, 'pre_measurement_warmup');
        preMeasurementPresentation = presentationProbe('pre_measurement_after_warmup_drain');
        measurementSubmitVersionStart = app.graphicsDevice.submitVersion;
        measurementStart = performance.now();
        measurementStartedAtUtc = new Date().toISOString();
        state = 'waiting_for_first_measurement_frame';
        resumeApplicationFrameLoop(app);
      } catch (error) {
        rejectCapture(error);
      }
    };

    const finishPresentationAfterTerminalDrain = async () => {
      try {
        if (!pendingRendererCapture) {
          throw new Error('presentation terminal omitted its WebGPU renderer capture');
        }
        let completedRendererCapture;
        [presentationDrain, completedRendererCapture] = await Promise.all([
          drainQueueWhileFrameLoopStopped(app, 'post_capture_presentation'),
          pendingRendererCapture.completion
        ]);
        rendererCapture = finalizePlayCanvasWebgpuRendererCapture(
          completedRendererCapture,
          presentationDrain
        );
        window.__PLAYCANVAS_RENDERER_CAPTURE_RGBA8_BASE64__ =
          rgba8ToBase64(rendererCapture.rgba8);
        postPresentationTerminalPresentation = presentationProbe(
          'post_capture_presentation_terminal_drain'
        );
        const terminalCameraReceipt = createPlayCanvasCameraReceipt({
          trace,
          traceFrameIndex: presentationTraceFrameIndex,
          phase: 'external_capture_terminal',
          observation: captureRuntimeCameraObservation(camera, app.graphicsDevice)
        });
        validatePlayCanvasCameraReceipt(
          terminalCameraReceipt,
          trace,
          presentationTraceFrameIndex
        );
        presentationCapture = {
          schema: PLAYCANVAS_PRESENTATION_CAPTURE_SCHEMA,
          ready_for_external_capture: true,
          excluded_from_performance: true,
          capture_trace_frame_index: presentationTraceFrameIndex,
          capture_trace_frame_source: presentationTraceFrameSource,
          stable_frame_count: presentationFrames.length,
          minimum_stable_frame_count: PLAYCANVAS_MIN_PRESENTATION_STABLE_FRAMES,
          measurement_terminal_submit_version: measurementDrain.submitVersionAfter,
          frames: presentationFrames,
          queue_drain: presentationDrain,
          renderer_capture: rendererCapture.receipt,
          terminal_camera_receipt: terminalCameraReceipt,
          browser_presentation: postPresentationTerminalPresentation
        };
        cleanup();
        resolve();
      } catch (error) {
        rejectCapture(error);
      }
    };

    const finishMeasurementAfterTerminalDrain = async () => {
      try {
        measurementDrain = await drainQueueWhileFrameLoopStopped(app, 'post_measurement_terminal');
        postMeasurementPresentation = presentationProbe('post_measurement_terminal_drain');
        sustained = summarizeSustainedMeasurement({
          measuredFrameCount: samples.length,
          submitVersionStart: measurementSubmitVersionStart,
          submitVersionEnd: measurementSubmitVersionEnd,
          measurementStartedAtMs: measurementStart,
          measurementSubmitEndedAtMs: measurementSubmitEnd,
          measurementQueueDrainedAtMs: measurementDrain.endedAtMs
        });
        measurementEndedAtUtc = measurementDrain.endedAtUtc;
        if (traceFrames.length === 0) {
          cleanup();
          resolve();
          return;
        }
        // Oracle work is deliberately outside the measured queue-terminal
        // interval so strict receipts do not distort the competitor timing.
        samples.forEach((sample) => validatePlayCanvasCameraReceipt(
          sample.cameraReceipt,
          trace,
          sample.traceFrameIndex
        ));
        if (!capturePresentation) {
          cleanup();
          resolve();
          return;
        }
        presentationTraceFrameIndex = requestedCaptureFrameIndex ??
          samples.at(-1).traceFrameIndex;
        presentationTraceFrameSource = requestedCaptureFrameIndex === null
          ? 'last_measured_trace_frame'
          : 'explicit_capture_trace_frame';
        if (!Number.isSafeInteger(presentationTraceFrameIndex) ||
            presentationTraceFrameIndex < 0 ||
            presentationTraceFrameIndex >= traceFrames.length) {
          throw new Error(`capture trace frame ${presentationTraceFrameIndex} is unavailable`);
        }
        state = 'presenting_capture';
        resumeApplicationFrameLoop(app);
      } catch (error) {
        rejectCapture(error);
      }
    };

    frameUpdateEvent = app.on('frameupdate', (wallMs) => {
      try {
        if (![
          'warming_up',
          'waiting_for_first_measurement_frame',
          'measuring',
          'presenting_capture'
        ].includes(state)) {
          return;
        }
        if (state === 'presenting_capture') {
          if (frameSubmitVersionStart !== null) {
            throw new Error('presentation frame boundaries overlapped');
          }
          presentationProbe(`presentation_frame_${presentationFrames.length}_start`);
          frameSubmitVersionStart = app.graphicsDevice.submitVersion;
          activeTraceFrameIndex = presentationTraceFrameIndex;
          applyTraceFrameToCamera(camera, traceFrames[presentationTraceFrameIndex]);
          return;
        }
        if (state === 'waiting_for_first_measurement_frame') {
          state = 'measuring';
        }
        if (state === 'measuring') {
          if (frameStart !== null || samples.length >= sampleFrames) {
            throw new Error('measurement frame boundaries overlapped');
          }
          frameStart = performance.now();
          if (shouldObserveMeasuredPresentation(capturePresentation)) {
            presentationProbe(`measurement_frame_${samples.length}_start`);
            measurementPresentationCheckCount += 1;
          }
          frameSubmitVersionStart = app.graphicsDevice.submitVersion;
          frameWallMs = measuredFrameWallMs({
            sampleIndex: samples.length,
            frameUpdateAtMs: frameStart,
            measurementStartedAtMs: measurementStart,
            playCanvasFrameWallMs: wallMs
          });
        }
        applyCameraForWarmupOrMeasurement();
      } catch (error) {
        rejectCapture(error);
      }
    });

    frameEndEvent = app.on('frameend', () => {
      try {
        const end = performance.now();
        if (state === 'warming_up') {
          completedWarmupFrames += 1;
          if (completedWarmupFrames === warmupFrames) {
            state = 'draining_warmup';
            stopApplicationFrameLoop(app);
            void beginMeasurementAfterWarmupDrain();
          }
          return;
        }
        if (state === 'measuring' && frameStart !== null) {
          const frameSubmitVersionEnd = app.graphicsDevice.submitVersion;
          if (!Number.isSafeInteger(frameSubmitVersionStart) ||
              frameSubmitVersionEnd <= frameSubmitVersionStart) {
            throw new Error('measured frame did not issue a WebGPU queue submission');
          }
          activeCameraReceipt = traceFrames.length === 0
            ? null
            : createPlayCanvasCameraReceipt({
                trace,
                traceFrameIndex: activeTraceFrameIndex,
                phase: `measurement_frame_${samples.length}`,
                observation: captureRuntimeCameraObservation(camera, app.graphicsDevice)
              });
          samples.push({
            elapsedNs: Math.round((end - measurementStart) * 1_000_000),
            callMs: end - frameStart,
            frameWallMs,
            activeSplats: activeSplatCount(manager, splatCount),
            traceFrameIndex: activeTraceFrameIndex,
            cameraReceipt: activeCameraReceipt,
            firstFrameAfterWarmupDrain: samples.length === 0,
            submitVersionBefore: frameSubmitVersionStart,
            submitVersionAfter: frameSubmitVersionEnd,
            queueSubmitCallCount: frameSubmitVersionEnd - frameSubmitVersionStart
          });
          frameStart = null;
          frameWallMs = null;
          frameSubmitVersionStart = null;
          activeCameraReceipt = null;
        }
        if (state === 'measuring' && samples.length === sampleFrames) {
          state = 'draining_measurement';
          measurementSubmitEnd = end;
          measurementSubmitVersionEnd = app.graphicsDevice.submitVersion;
          stopApplicationFrameLoop(app);
          void finishMeasurementAfterTerminalDrain();
          return;
        }
        if (state === 'presenting_capture') {
          const frameSubmitVersionEnd = app.graphicsDevice.submitVersion;
          if (!Number.isSafeInteger(frameSubmitVersionStart) ||
              frameSubmitVersionEnd <= frameSubmitVersionStart) {
            throw new Error('presentation frame did not issue a verified WebGPU submission');
          }
          activeCameraReceipt = createPlayCanvasCameraReceipt({
            trace,
            traceFrameIndex: presentationTraceFrameIndex,
            phase: `presentation_frame_${presentationFrames.length}`,
            observation: captureRuntimeCameraObservation(camera, app.graphicsDevice)
          });
          validatePlayCanvasCameraReceipt(
            activeCameraReceipt,
            trace,
            presentationTraceFrameIndex
          );
          presentationFrames.push({
            trace_frame_index: presentationTraceFrameIndex,
            camera_receipt: activeCameraReceipt,
            submit_version_before: frameSubmitVersionStart,
            submit_version_after: frameSubmitVersionEnd,
            queue_submit_call_count: frameSubmitVersionEnd - frameSubmitVersionStart
          });
          if (presentationFrames.length === PLAYCANVAS_MIN_PRESENTATION_STABLE_FRAMES) {
            pendingRendererCapture = beginPlayCanvasWebgpuRendererCapture({
              graphicsDevice: app.graphicsDevice,
              rendererFrameSequence: app.frame,
              rendererSubmitVersion: frameSubmitVersionEnd,
              identity: {
                ...rendererCaptureIdentity,
                camera_receipt: activeCameraReceipt
              }
            });
            presentationFrames.at(-1).renderer_capture_copy = {
              submit_version_after: pendingRendererCapture.copySubmitVersionAfter
            };
          }
          frameSubmitVersionStart = null;
          activeCameraReceipt = null;
          if (presentationFrames.length === PLAYCANVAS_MIN_PRESENTATION_STABLE_FRAMES) {
            state = 'draining_presentation';
            stopApplicationFrameLoop(app);
            void finishPresentationAfterTerminalDrain();
          }
        }
      } catch (error) {
        rejectCapture(error);
      }
    });

    if (warmupFrames === 0) {
      try {
        stopApplicationFrameLoop(app);
        void beginMeasurementAfterWarmupDrain();
      } catch (error) {
        rejectCapture(error);
      }
    }
  });

  return {
    warmupCount: warmupFrames,
    samples,
    measurementStartedAtUtc,
    measurementEndedAtUtc,
    warmupDrain,
    measurementDrain,
    sustained,
    presentationCapture,
    presentation: {
      preMeasurement: preMeasurementPresentation,
      postMeasurement: postMeasurementPresentation,
      postPresentationTerminal: postPresentationTerminalPresentation,
      measuredFrameCheckCount: measurementPresentationCheckCount,
      everyMeasuredFrameChecked: measurementPresentationCheckCount === sampleFrames
    },
    timing: {
      call_source:
        'harness frameupdate handler entry before trace-camera mutation to frameend handler entry after PlayCanvas render submission',
      frame_wall_source:
        'first sample uses post-drain scheduling-to-frameupdate wall time; remaining samples use PlayCanvas frameupdate ms from requestAnimationFrame timestamps',
      frame_wall_boundary_note:
        'the first measured frame follows a deliberate stopped-loop warmup drain, is tagged firstFrameAfterWarmupDrain, and excludes the drain itself',
      counts_source: 'GSplatWorld current state totalActiveSplats',
      camera_source: cameraMode === 'sequence'
        ? 'next trace pose/intrinsics applied in every captured frameupdate before update/render'
        : 'fixed trace pose/intrinsics reapplied in every captured frameupdate before update/render',
      gpu_queue_terminal_source:
        'PlayCanvas frame loop cancelled before each app.graphicsDevice.wgpu.queue.onSubmittedWorkDone call; submitVersion remained stable',
      frame_loop_control_source:
        'pinned PlayCanvas Application.cancelTick to stop and requestAnimationFrame to resume without re-running app.start',
      queue_submission_boundary_source:
        'pinned PlayCanvas WebgpuGraphicsDevice.submitVersion around each stopped-loop drain',
      per_frame_submission_source:
        'submitVersion sampled at measured frameupdate before trace-camera mutation and again at frameend after graphicsDevice.frameEnd submission',
      final_submission_order_source:
        'pinned AppBase tick fires frameend after render; render calls graphicsDevice.frameEnd; WebgpuGraphicsDevice.frameEnd submits queued command buffers',
      presentation_capture_source:
        capturePresentation
          ? `after measurement terminal drain, ${PLAYCANVAS_MIN_PRESENTATION_STABLE_FRAMES} fixed-trace frames are submitted outside timing, then the stopped loop is drained again`
          : 'disabled_for_independent_throughput_artifact',
      gpu_phase_timing: 'not_available_not_inferred'
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
  if (qualificationName && !qualification) {
    fail('unknown qualification preset', { qualificationName, supported: Object.keys(QUALIFICATIONS) });
  }
  if (!Number.isSafeInteger(requestedTraceFrameIndex) || requestedTraceFrameIndex < 0) {
    fail('trace_frame must be a non-negative safe integer', { requestedTraceFrameIndex });
  }
  if (requestedCaptureTraceFrameIndex !== null &&
      (!Number.isSafeInteger(requestedCaptureTraceFrameIndex) ||
       requestedCaptureTraceFrameIndex < 0)) {
    fail('capture_trace_frame must be a non-negative safe integer when provided', {
      requestedCaptureTraceFrameIndex
    });
  }
  if (!Number.isSafeInteger(requestedWarmupFrames) || requestedWarmupFrames < 0 ||
      !Number.isSafeInteger(requestedMeasuredFrames) || requestedMeasuredFrames <= 0) {
    fail('warmup/measured frame counts are invalid', {
      requestedWarmupFrames,
      requestedMeasuredFrames
    });
  }
  if (!['static', 'sequence'].includes(requestedCameraMode)) {
    fail('camera_mode must be static or sequence', { requestedCameraMode });
  }
  if (hostStartGateEnabled && (!benchmarkMode || !qualificationMode)) {
    fail('host start gate requires a benchmark qualification');
  }

  const fetchJson = async (url, label) => {
    const response = await fetch(url);
    if (!response.ok) fail(`${label} fetch failed`, { url, status: response.status });
    return response.json();
  };
  const trace = qualificationMode ? await fetchJson(TRACE_URL, 'camera trace') : null;
  const traceFrame = trace?.frames?.[requestedTraceFrameIndex] ?? null;
  if (qualificationMode && !traceFrame) {
    fail('requested trace frame is unavailable', {
      requestedTraceFrameIndex,
      frameCount: trace?.frames?.length ?? null
    });
  }
  if (requestedCaptureTraceFrameIndex !== null &&
      !trace?.frames?.[requestedCaptureTraceFrameIndex]) {
    fail('requested capture trace frame is unavailable', {
      requestedCaptureTraceFrameIndex,
      frameCount: trace?.frames?.length ?? null
    });
  }
  if (requestedCameraMode === 'sequence' && (trace?.frames?.length ?? 0) < 2) {
    fail('sequence camera mode requires at least two trace frames', {
      frameCount: trace?.frames?.length ?? null
    });
  }
  const requestedWidth = trace?.display?.width ?? 640;
  const requestedHeight = trace?.display?.height ?? 480;
  if (!Number.isSafeInteger(requestedWidth) || requestedWidth <= 0 ||
      !Number.isSafeInteger(requestedHeight) || requestedHeight <= 0) {
    fail('trace display dimensions are invalid', { requestedWidth, requestedHeight });
  }
  const datasetManifest = qualification?.manifestUrl
    ? await fetchJson(qualification.manifestUrl, 'dataset manifest')
    : null;

  const canvas = document.querySelector('#canvas');
  // The render target stays at the trace's exact physical pixel dimensions,
  // while CSS fills the browser's real viewport. On Android this preserves
  // the device's native DPR instead of emulating a 2412x1080 CSS viewport and
  // showing only its upper-left physical-screen crop.
  canvas.style.width = '100vw';
  canvas.style.height = '100vh';
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
  activeApplication = app;
  app.setCanvasFillMode(FILLMODE_NONE);
  app.setCanvasResolution(RESOLUTION_FIXED, requestedWidth, requestedHeight);
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

  const diagnostic = qualification?.generated === true;
  const diagnosticPly = diagnostic ? createRasterDiagnosticPly() : null;
  const diagnosticUrl = diagnostic
    ? URL.createObjectURL(new Blob([diagnosticPly], { type: 'application/octet-stream' }))
    : null;
  const asset = new Asset(diagnostic ? RASTER_DIAGNOSTIC_ID : qualificationName ?? 'minimal_binary', 'gsplat', {
    url: diagnosticUrl ?? DATASET_URL,
    ...(diagnostic ? { filename: `${RASTER_DIAGNOSTIC_ID}.ply` } : {})
  });
  // GSplatComponent resolves asset ids through the application's registry. An
  // AssetListLoader can load a standalone asset, but that alone does not make
  // it discoverable by the component's AssetReference.
  app.assets.add(asset);
  await loadAsset(app, asset);
  if (!asset.resource) fail('PLY loaded without a gsplat resource');

  const camera = new Entity('Camera');
  camera.addComponent('camera', {
    clearColor: qualificationMode ? new Color(0, 0, 0) : new Color(0.02, 0.02, 0.03),
    fov: traceFrame ? (traceFrame.intrinsics.vertical_fov_radians * 180) / Math.PI : 45,
    nearClip: traceFrame?.intrinsics.near_plane ?? 0.01,
    farClip: traceFrame?.intrinsics.far_plane ?? 100
  });
  const cameraPose = traceFrame
    ? traceFrameToPlayCanvasPose(traceFrame)
    : { position: [0, 0, 3], target: [0, 0, 0], forward: [0, 0, -1], up: [0, 1, 0] };
  camera.setPosition(...cameraPose.position);
  camera.lookAt(...cameraPose.target, ...cameraPose.up);
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
  const expectedSplatCount = datasetManifest?.splat_count ?? splatCount;
  const expectedShDegree = datasetManifest?.sh_degree ?? asset.resource.shBands ?? 0;
  const residentState = manager?.world?.getState(manager.world.currentVersion);
  const residentSplatCount = residentState?.totalActiveSplats ?? null;
  if (!Number.isSafeInteger(expectedSplatCount) || expectedSplatCount <= 0 ||
      splatCount !== expectedSplatCount || residentSplatCount !== expectedSplatCount) {
    fail('complete source membership was not preserved', {
      expectedSplatCount,
      decodedSplatCount: splatCount,
      residentSplatCount
    });
  }
  if (asset.resource.shBands !== expectedShDegree) {
    fail('source SH degree was not preserved', {
      expectedShDegree,
      residentShDegree: asset.resource.shBands
    });
  }
  const internalWidth = app.graphicsDevice.width;
  const internalHeight = app.graphicsDevice.height;
  const resolution = {
    requested_width: requestedWidth,
    requested_height: requestedHeight,
    surface_width: canvas.width,
    surface_height: canvas.height,
    internal_render_width: internalWidth,
    internal_render_height: internalHeight,
    presented_width: null,
    presented_height: null,
    presented_source: 'requires_external_device_screen_receipt',
    dynamic_resolution: 'disabled',
    upscaling: 'disabled',
    internal_full_resolution: true,
    full_resolution: false
  };
  const exactness = {
    source_splat_count: expectedSplatCount,
    decoded_splat_count: splatCount,
    encoded_splat_count: residentSplatCount,
    resident_splat_count: residentSplatCount,
    addressable_splat_count: residentSplatCount,
    source_sh_degree: expectedShDegree,
    resident_sh_degree: asset.resource.shBands,
    source_membership: 'all',
    sampling: 'disabled',
    lod: 'disabled',
    partial_scene_published: false,
    full_quality: true
  };
  const datasetSha256 = datasetManifest?.sha256 ??
    (diagnosticPly ? await sha256Hex(diagnosticPly) : null);
  const resolutionPairs = [
    ['surface', resolution.surface_width, resolution.surface_height],
    ['internal_render', resolution.internal_render_width, resolution.internal_render_height]
  ];
  for (const [stage, width, height] of resolutionPairs) {
    if (width !== requestedWidth || height !== requestedHeight) {
      fail('qualification resolution mismatch', {
        stage,
        requested: [requestedWidth, requestedHeight],
        actual: [width, height]
      });
    }
  }
  const presentationProbe = (phase) => captureBrowserPresentationState({
    canvas,
    expectedWidth: requestedWidth,
    expectedHeight: requestedHeight,
    phase
  });
  let hostStartReceipt = null;
  if (hostStartGateEnabled) {
    let releaseHostStart;
    const hostStart = new Promise((resolve) => { releaseHostStart = resolve; });
    const gate = createQ1HostStartGate({
      capture: () => captureQ1HostStartReceipt({
        canvas,
        expectedWidth: requestedWidth,
        expectedHeight: requestedHeight
      }),
      start: releaseHostStart
    });
    window.__PLAYCANVAS_Q1_HOST_START_READY__ = gate.arm();
    window.__PLAYCANVAS_Q1_HOST_START__ = () => {
      const receipt = gate.start();
      window.__PLAYCANVAS_Q1_HOST_START_RECEIPT__ = receipt;
      return receipt;
    };
    status.textContent = 'host_start_armed';
    await hostStart;
    hostStartReceipt = window.__PLAYCANVAS_Q1_HOST_START_RECEIPT__;
  }
  const preCapturePresentation = presentationProbe('pre_capture');
  const initialCameraReceipt = trace
    ? applyAndCaptureTraceCamera({
      app,
      camera,
      trace,
      traceFrameIndex: requestedTraceFrameIndex,
      phase: 'pre_capture'
    })
    : null;
  if (initialCameraReceipt) {
    validatePlayCanvasCameraReceipt(
      initialCameraReceipt,
      trace,
      requestedTraceFrameIndex
    );
  }
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
    ? await collectFrameSamples(
      app,
      manager,
      splatCount,
      requestedWarmupFrames,
      requestedMeasuredFrames,
      camera,
      trace,
      requestedCameraMode,
      requestedCaptureTraceFrameIndex,
      rendererCaptureEnabled,
      presentationProbe,
      {
        resolution,
        source: {
          ...exactness,
          dataset_id: datasetManifest?.id ?? asset.name,
          dataset_sha256: datasetSha256
        }
      }
    )
    : null;
  const postCapturePresentation = presentationProbe('post_capture');
  const terminalCameraReceipt = capture?.presentationCapture?.terminal_camera_receipt ??
    initialCameraReceipt;
  if (benchmarkMode && qualificationMode && rendererCaptureEnabled &&
      (capture?.presentationCapture?.ready_for_external_capture !== true ||
       !terminalCameraReceipt)) {
    fail('benchmark ended without a terminal presentation camera receipt');
  }
  const result = {
    status: benchmarkMode
      ? rendererCaptureEnabled ? 'raw_frame_capture_complete' : 'raw_frame_measurement_complete'
      : 'ready_for_pre_timing_capture',
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
    datasetReceipt: datasetManifest,
    splatCount,
    sourceCenterBounds: centerBounds(asset.resource.centers),
    canvasBackingWidth: canvas.width,
    canvasBackingHeight: canvas.height,
    devicePixelRatio: window.devicePixelRatio,
    webGpuEnvironmentReceipt: captureWebGpuEnvironmentReceipt(app.graphicsDevice),
    cameraReceipt: terminalCameraReceipt,
    exactness,
    resolution,
    browserPresentationReceipt: {
      source: 'browser_page_visibility_focus_and_geometry',
      physicalPresentationClaim: false,
      hostStart: hostStartReceipt,
      preCapture: preCapturePresentation,
      preMeasurement: capture?.presentation?.preMeasurement ?? null,
      postMeasurement: capture?.presentation?.postMeasurement ?? null,
      postPresentationTerminal:
        capture?.presentation?.postPresentationTerminal ?? null,
      postCapture: postCapturePresentation,
      measuredFrameCheckCount: capture?.presentation?.measuredFrameCheckCount ?? 0,
      everyMeasuredFrameChecked: capture?.presentation?.everyMeasuredFrameChecked ?? false
    },
    traceDescriptor,
    policies: {
      dynamicResolution: 'disabled_fixed_backing',
      upscaling: 'disabled',
      evidenceClass: qualification?.evidenceClass ?? 'smoke',
      lod: 'disabled_full_ply',
      renderer: 'raster_gpu_sort',
      sourceCoordinateConversion: qualificationMode ? 'rdf_to_playcanvas_rub_entity_yz_reflection' : 'none',
      cameraCoordinateConversion: qualificationMode ? 'ruf_plus_z_to_playcanvas_minus_z' : 'none',
      cameraProjectionConversion: qualificationMode
        ? 'canonical_row_major_plus_z_ndc_0_1_to_playcanvas_column_major_minus_z_opengl_then_webgpu_0_1'
        : 'none',
      externalCaptureState: capture?.presentationCapture?.ready_for_external_capture === true
        ? 'ready_after_fixed_trace_frames_and_terminal_queue_drain'
        : 'not_applicable',
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
  let frameLoopStopError = null;
  try {
    stopApplicationFrameLoopIfScheduled(activeApplication);
  } catch (stopError) {
    frameLoopStopError = stopError?.message ?? String(stopError);
  }
  const failureCleanup = {
    frameLoopStopped: activeApplication
      ? activeApplication.frameRequestId === null || activeApplication.frameRequestId === undefined
      : null,
    frameLoopStopError
  };
  if (!window.__PLAYCANVAS_HARNESS_ERROR__) {
    const details = {
      name: error?.name,
      message: error?.message,
      stack: error?.stack,
      failureCleanup
    };
    window.__PLAYCANVAS_HARNESS_ERROR__ = { status: 'blocked', message: 'unexpected harness failure', details };
    status.textContent = JSON.stringify(window.__PLAYCANVAS_HARNESS_ERROR__, null, 2);
    console.error('PLAYCANVAS_HARNESS_BLOCKED', window.__PLAYCANVAS_HARNESS_ERROR__);
  } else {
    window.__PLAYCANVAS_HARNESS_ERROR__.details = {
      ...window.__PLAYCANVAS_HARNESS_ERROR__.details,
      failureCleanup
    };
    status.textContent = JSON.stringify(window.__PLAYCANVAS_HARNESS_ERROR__, null, 2);
  }
});
