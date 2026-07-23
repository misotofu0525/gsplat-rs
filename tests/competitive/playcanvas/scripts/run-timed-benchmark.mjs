import { execFile as execFileCallback } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import os from 'node:os';
import { dirname, resolve } from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import process from 'node:process';
import puppeteer from 'puppeteer-core';
import { assertBrowserPresentationState } from '../public/presentation-receipt.js';
import { validateQueueTerminalCapture } from '../public/queue-terminal.js';
import {
  PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA,
  validatePlayCanvasCameraReceipt,
  validatePlayCanvasCaptureEvidence,
  validatePlayCanvasScreenshotBinding
} from '../public/trace-camera.js';
import {
  captureAndroidScreenReceipt,
  collectAndroidDeviceReceipt,
  loadExpectedAndroidDeviceReceipt,
  verifyAndroidBrowserIdentity,
  verifyExpectedAndroidDeviceReceipt,
  verifyStableAndroidDeviceIdentity,
  verifyAndroidWindowPresentation
} from './android-device-receipt.mjs';
import {
  browserSessionConfig,
  classifyBrowserFailure,
  isValidDevicePixelRatio,
  openBrowserSession
} from './browser-session.mjs';
import { startServer } from './server.mjs';

const execFile = promisify(execFileCallback);
const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(harnessRoot, '..', '..', '..');
const qualificationName = process.env.PHASE_E_QUALIFICATION?.trim() || null;
const qualification = qualificationName !== null;
const warmupFrames = Number(process.env.PLAYCANVAS_WARMUP_FRAMES ?? (qualification ? 120 : 30));
const measuredFrames = Number(process.env.PLAYCANVAS_MEASURED_FRAMES ?? (qualification ? 3600 : 60));
const traceFrame = Number(process.env.PLAYCANVAS_TRACE_FRAME ?? 0);
const captureTraceFrameEnvironment = process.env.PLAYCANVAS_CAPTURE_TRACE_FRAME;
const captureTraceFrame = captureTraceFrameEnvironment === undefined
  ? null
  : Number(captureTraceFrameEnvironment);
const cameraMode = process.env.PLAYCANVAS_CAMERA_MODE?.trim() || 'static';
if (!['static', 'sequence'].includes(cameraMode)) {
  throw new Error('PLAYCANVAS_CAMERA_MODE must be static or sequence');
}
const defaultWidth = qualificationName?.includes('2412x1080')
  ? 2412
  : qualificationName?.includes('1080p') ? 1920 : 640;
const defaultHeight = qualificationName?.includes('2412x1080') || qualificationName?.includes('1080p')
  ? 1080
  : 480;
const viewportWidth = Number(process.env.PLAYCANVAS_VIEWPORT_WIDTH ?? defaultWidth);
const viewportHeight = Number(process.env.PLAYCANVAS_VIEWPORT_HEIGHT ?? defaultHeight);
const screenContentSsimThreshold = Number(
  process.env.PLAYCANVAS_SCREEN_SSIM_THRESHOLD ?? 0.99
);
for (const [label, value, allowZero] of [
  ['PLAYCANVAS_WARMUP_FRAMES', warmupFrames, true],
  ['PLAYCANVAS_MEASURED_FRAMES', measuredFrames, false],
  ['PLAYCANVAS_TRACE_FRAME', traceFrame, true],
  ...(captureTraceFrame === null
    ? []
    : [['PLAYCANVAS_CAPTURE_TRACE_FRAME', captureTraceFrame, true]]),
  ['PLAYCANVAS_VIEWPORT_WIDTH', viewportWidth, false],
  ['PLAYCANVAS_VIEWPORT_HEIGHT', viewportHeight, false]
]) {
  if (!Number.isSafeInteger(value) || value < (allowZero ? 0 : 1)) {
    throw new Error(`${label} must be a ${allowZero ? 'non-negative' : 'positive'} safe integer`);
  }
}
if (!Number.isFinite(screenContentSsimThreshold) ||
    screenContentSsimThreshold < -1 || screenContentSsimThreshold > 1) {
  throw new Error('PLAYCANVAS_SCREEN_SSIM_THRESHOLD must be between -1 and 1');
}
const outputRoot = resolve(
  process.env.PLAYCANVAS_ARTIFACT_DIR ??
    resolve(
      repoRoot,
      qualification
        ? `target/benchmarks/competitive/playcanvas-${qualificationName}`
        : 'target/benchmarks/playcanvas-collector-smoke'
    )
);
const sessionConfig = browserSessionConfig(process.env);
const chromeCandidates = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium'
].filter(Boolean);

async function findChrome() {
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {}
  }
  return null;
}

function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

function distribution(frames, metric) {
  const values = frames.map((frame) => frame[metric]).filter((value) => value !== null).sort((a, b) => a - b);
  if (values.length === 0) return null;
  const rank = (fraction) => values[Math.max(Math.ceil(fraction * values.length) - 1, 0)];
  return {
    count: values.length,
    mean: values.reduce((sum, value) => sum + value, 0) / values.length,
    p50: rank(0.5),
    p90: rank(0.9),
    p95: rank(0.95),
    p99: rank(0.99),
    max: values.at(-1)
  };
}

function queueDrainManifestReceipt(receipt) {
  return {
    phase: receipt.phase,
    api: receipt.api,
    semantics_source: receipt.semanticsSource,
    specification_url: receipt.specificationUrl,
    frame_loop_stopped: receipt.frameLoopStopped,
    submit_version_before: receipt.submitVersionBefore,
    submit_version_after: receipt.submitVersionAfter,
    submit_version_stable: receipt.submitVersionStable,
    started_at_utc: receipt.startedAtUtc,
    ended_at_utc: receipt.endedAtUtc,
    drain_ms: receipt.drainMs
  };
}

function sustainedManifestReceipt(receipt) {
  return {
    method: receipt.method,
    measured_frame_count: receipt.measuredFrameCount,
    submit_version_start: receipt.submitVersionStart,
    submit_version_end: receipt.submitVersionEnd,
    queue_submit_call_count: receipt.queueSubmitCallCount,
    measurement_submit_span_ms: receipt.measurementSubmitSpanMs,
    queue_drain_after_last_submit_ms: receipt.queueDrainAfterLastSubmitMs,
    queue_terminal_span_ms: receipt.queueTerminalSpanMs,
    mean_frame_ms: receipt.sustainedMeanFrameMs,
    mean_fps: receipt.sustainedFps,
    submit_span_source: receipt.submitSpanSource,
    queue_drain_source: receipt.queueDrainSource,
    sustained_source: receipt.sustainedSource
  };
}

await mkdir(outputRoot, { recursive: true });
const chrome = sessionConfig.mode === 'local-launch' ? await findChrome() : null;
if (sessionConfig.mode === 'local-launch' && !chrome) {
  const blocker = { status: 'blocked', reason: 'no supported Chrome/Chromium executable found', candidates: chromeCandidates };
  await writeFile(resolve(outputRoot, 'blocker.json'), `${JSON.stringify(blocker, null, 2)}\n`);
  console.error(JSON.stringify(blocker));
  process.exit(2);
}

const startedAtUtc = new Date().toISOString();
const { server, port } = await startServer(sessionConfig.serverPort);
let browserSession;
let browser;
let page;
const browserLog = [];
let expectedAndroidReceipt = null;
let preAndroidReceipt = null;
let postAndroidReceipt = null;
let androidBrowserIdentity = null;
let androidIdentityStability = null;
let preAndroidWindowPresentation = null;
let postAndroidWindowPresentation = null;
let androidScreenReceipt = null;
let verifiedExpectedAndroidReceipt = null;
let finalResolution = null;
let canvasScreenReceipt = null;
let screenshotBinding = null;
let screenContentComparison = null;
try {
  if (sessionConfig.mode === 'remote-cdp') {
    expectedAndroidReceipt = await loadExpectedAndroidDeviceReceipt(
      sessionConfig.expectedReceiptPath
    );
    preAndroidReceipt = await collectAndroidDeviceReceipt({
      execFile,
      adbPath: sessionConfig.adbPath,
      serial: sessionConfig.adbSerial,
      cdpPort: sessionConfig.cdpPort,
      androidRuntimeKind: sessionConfig.androidRuntimeKind,
      hostPackage: sessionConfig.hostPackage,
      runtimePackage: sessionConfig.runtimePackage,
      cdpSocket: sessionConfig.cdpSocket,
      phase: 'pre_browser_session'
    });
    preAndroidWindowPresentation = verifyAndroidWindowPresentation(
      preAndroidReceipt,
      viewportWidth,
      viewportHeight
    );
    verifiedExpectedAndroidReceipt = verifyExpectedAndroidDeviceReceipt(
      expectedAndroidReceipt,
      preAndroidReceipt
    );
  }
  browserSession = await openBrowserSession({
    puppeteer,
    config: sessionConfig,
    executablePath: chrome,
    headless: process.env.HEADLESS !== '0',
    viewport: { width: viewportWidth, height: viewportHeight, deviceScaleFactor: 1 }
  });
  ({ browser, page } = browserSession);
  page.on('console', (message) => browserLog.push(`${message.type()}: ${message.text()}`));
  page.on('pageerror', (error) => browserLog.push(`pageerror: ${error.stack ?? error.message}`));
  const qualificationQuery = qualification ? `&qualification=${encodeURIComponent(qualificationName)}` : '';
  const captureTraceFrameQuery = captureTraceFrame === null
    ? ''
    : `&capture_trace_frame=${captureTraceFrame}`;
  await page.goto(
    `http://127.0.0.1:${port}/?benchmark=1${qualificationQuery}` +
      `&trace_frame=${traceFrame}&warmup_frames=${warmupFrames}` +
      `&measured_frames=${measuredFrames}&camera_mode=${cameraMode}` +
      captureTraceFrameQuery,
    {
    waitUntil: 'networkidle0',
    timeout: qualification ? 600_000 : 30_000
    }
  );
  await page.waitForFunction(
    () => window.__PLAYCANVAS_HARNESS_RESULT__ || window.__PLAYCANVAS_HARNESS_ERROR__,
    { timeout: qualification ? 1_800_000 : 60_000 }
  );
  const outcome = await page.evaluate(() => ({
    result: window.__PLAYCANVAS_HARNESS_RESULT__ ?? window.__PLAYCANVAS_HARNESS_ERROR__,
    userAgent: navigator.userAgent,
    platform: navigator.platform
  }));
  const browserVersion = await browser.version();
  await writeFile(resolve(outputRoot, 'runtime.log'), `${browserLog.join('\n')}\n`);
  if (outcome.result.status !== 'raw_frame_capture_complete') throw new Error(JSON.stringify(outcome.result));
  const browserPresentation = outcome.result.browserPresentationReceipt;
  const presentationReceipts = [
    ['preCapture', browserPresentation?.preCapture],
    ['preMeasurement', browserPresentation?.preMeasurement],
    ['postMeasurement', browserPresentation?.postMeasurement],
    ['postCapture', browserPresentation?.postCapture]
  ];
  if (qualification) {
    presentationReceipts.push([
      'postPresentationTerminal',
      browserPresentation?.postPresentationTerminal
    ]);
  }
  for (const [label, receipt] of presentationReceipts) {
    if (!receipt) throw new Error(`browser presentation receipt omitted ${label}`);
    assertBrowserPresentationState(receipt);
  }
  if (browserPresentation?.physicalPresentationClaim !== false ||
      browserPresentation?.everyMeasuredFrameChecked !== true ||
      browserPresentation?.measuredFrameCheckCount !== measuredFrames) {
    throw new Error('browser presentation lifecycle receipt is incomplete');
  }

  let cameraEvidence = null;
  if (qualification) {
    cameraEvidence = validatePlayCanvasCaptureEvidence({
      trace: outcome.result.traceDescriptor,
      capture: outcome.result.capture,
      expectedMeasuredFrames: measuredFrames,
      requestedCaptureTraceFrameIndex: captureTraceFrame
    });
    validatePlayCanvasCameraReceipt(
      outcome.result.cameraReceipt,
      outcome.result.traceDescriptor,
      cameraEvidence.expectedCaptureIndex
    );
    if (JSON.stringify(outcome.result.cameraReceipt) !==
        JSON.stringify(cameraEvidence.presentation.terminal_camera_receipt)) {
      throw new Error('top-level camera receipt is not the terminal presentation receipt');
    }
    const dataUrl = await page.$eval('#canvas', (canvas) => canvas.toDataURL('image/png'));
    const canvasPng = Buffer.from(dataUrl.split(',')[1], 'base64');
    canvasScreenReceipt = {
      file: 'final-frame.png',
      source: 'canvas_toDataURL_after_presentation_terminal',
      captured_at_utc: new Date().toISOString(),
      captured_after_presentation_terminal: true,
      capture_trace_frame_index: cameraEvidence.expectedCaptureIndex,
      width: outcome.result.canvasBackingWidth,
      height: outcome.result.canvasBackingHeight,
      byte_count: canvasPng.length,
      sha256: sha256(canvasPng)
    };
    await writeFile(resolve(outputRoot, canvasScreenReceipt.file), canvasPng);
  }

  finalResolution = outcome.result.resolution;
  if (sessionConfig.mode === 'remote-cdp') {
    postAndroidReceipt = await collectAndroidDeviceReceipt({
      execFile,
      adbPath: sessionConfig.adbPath,
      serial: sessionConfig.adbSerial,
      cdpPort: sessionConfig.cdpPort,
      androidRuntimeKind: sessionConfig.androidRuntimeKind,
      hostPackage: sessionConfig.hostPackage,
      runtimePackage: sessionConfig.runtimePackage,
      cdpSocket: sessionConfig.cdpSocket,
      phase: 'post_measurement_terminal'
    });
    postAndroidWindowPresentation = verifyAndroidWindowPresentation(
      postAndroidReceipt,
      viewportWidth,
      viewportHeight
    );
    androidIdentityStability = verifyStableAndroidDeviceIdentity(
      preAndroidReceipt,
      postAndroidReceipt
    );
    verifyExpectedAndroidDeviceReceipt(expectedAndroidReceipt, postAndroidReceipt);
    androidBrowserIdentity = verifyAndroidBrowserIdentity(postAndroidReceipt, {
      browserVersion,
      userAgent: outcome.userAgent,
      platform: outcome.platform
    });
    const screenCapture = await captureAndroidScreenReceipt({
      execFile,
      adbPath: sessionConfig.adbPath,
      serial: sessionConfig.adbSerial,
      expectedWidth: viewportWidth,
      expectedHeight: viewportHeight
    });
    androidScreenReceipt = screenCapture.receipt;
    await writeFile(resolve(outputRoot, 'device-screen.png'), screenCapture.png);
    const comparisonPath = resolve(outputRoot, 'canvas-device-ssim.json');
    const comparisonRun = await execFile(process.execPath, [
      resolve(repoRoot, 'tests/perf/compare-image-ssim.mjs'),
      resolve(outputRoot, canvasScreenReceipt.file),
      resolve(outputRoot, 'device-screen.png'),
      '--threshold',
      String(screenContentSsimThreshold),
      '--output',
      comparisonPath
    ], { cwd: repoRoot, maxBuffer: 4 * 1024 * 1024 });
    const comparison = JSON.parse(comparisonRun.stdout.trim().split('\n').at(-1));
    if (comparison.pass !== true) {
      throw new Error(`canvas/ADB screen content mismatch: ${comparisonRun.stdout.trim()}`);
    }
    screenContentComparison = {
      file: 'canvas-device-ssim.json',
      schema: comparison.schema,
      metric: comparison.metric,
      width: comparison.width,
      height: comparison.height,
      score: comparison.score,
      threshold: comparison.threshold,
      pass: comparison.pass,
      canvas_sha256: canvasScreenReceipt.sha256,
      adb_sha256: androidScreenReceipt.sha256
    };
    finalResolution = {
      ...outcome.result.resolution,
      presented_width: androidScreenReceipt.width,
      presented_height: androidScreenReceipt.height,
      presented_source: androidScreenReceipt.source,
      external_device_screen_receipt: 'device-screen.png',
      internal_full_resolution: true,
      full_resolution: true
    };
    const expected = [viewportWidth, viewportHeight];
    const resolution = finalResolution;
    const stages = [
      ['canvas_backing', outcome.result.canvasBackingWidth, outcome.result.canvasBackingHeight],
      ['surface', resolution?.surface_width, resolution?.surface_height],
      ['internal_render', resolution?.internal_render_width, resolution?.internal_render_height],
      ['presented', resolution?.presented_width, resolution?.presented_height]
    ];
    const mismatch = stages.find(([, width, height]) => width !== expected[0] || height !== expected[1]);
    if (mismatch || !isValidDevicePixelRatio(outcome.result.devicePixelRatio) ||
        resolution?.full_resolution !== true ||
        resolution?.dynamic_resolution !== 'disabled' || resolution?.upscaling !== 'disabled') {
      throw new Error(JSON.stringify({
        status: 'blocked',
        reason: 'remote full-resolution receipt mismatch',
        expected,
        stages,
        devicePixelRatio: outcome.result.devicePixelRatio,
        resolution
      }));
    }
    await writeFile(
      resolve(outputRoot, 'android-device-receipt.json'),
      `${JSON.stringify({
        schema: 'gsplat-android-benchmark-device-evidence/v1',
        pre: preAndroidReceipt,
        post: postAndroidReceipt,
        stable_identity: androidIdentityStability,
        browser_identity: androidBrowserIdentity,
        window_presentation: {
          pre: preAndroidWindowPresentation,
          post: postAndroidWindowPresentation
        },
        expected_receipt: verifiedExpectedAndroidReceipt,
        screen: androidScreenReceipt
      }, null, 2)}\n`
    );
  }
  if (qualification) {
    const adbBinding = androidScreenReceipt
      ? {
          file: 'device-screen.png',
          source: androidScreenReceipt.source,
          captured_at_utc: androidScreenReceipt.captured_at_utc,
          captured_after_presentation_terminal: true,
          capture_trace_frame_index: cameraEvidence.expectedCaptureIndex,
          width: androidScreenReceipt.width,
          height: androidScreenReceipt.height,
          byte_count: androidScreenReceipt.byte_count,
          sha256: androidScreenReceipt.sha256
        }
      : null;
    screenshotBinding = {
      schema: PLAYCANVAS_SCREENSHOT_BINDING_SCHEMA,
      presentation_state: 'ready_for_external_capture',
      capture_trace_frame_index: cameraEvidence.expectedCaptureIndex,
      camera_receipt_schema: outcome.result.cameraReceipt.schema,
      camera_receipt_sha256: sha256(JSON.stringify(outcome.result.cameraReceipt)),
      canvas: canvasScreenReceipt,
      adb: adbBinding,
      content_comparison: screenContentComparison
    };
    validatePlayCanvasScreenshotBinding({
      binding: screenshotBinding,
      cameraReceipt: outcome.result.cameraReceipt,
      expectedWidth: viewportWidth,
      expectedHeight: viewportHeight,
      requireAdb: sessionConfig.mode === 'remote-cdp'
    });
    await writeFile(
      resolve(outputRoot, 'screenshot-binding.json'),
      `${JSON.stringify(screenshotBinding, null, 2)}\n`
    );
  }
  const queueTerminal = validateQueueTerminalCapture(outcome.result.capture, measuredFrames);

  const dataset = outcome.result.datasetReceipt ??
    JSON.parse(await readFile(resolve(repoRoot, 'tests/perf/datasets/minimal_binary.json'), 'utf8'));
  const expectedEngine = JSON.parse(await readFile(resolve(harnessRoot, 'expected-engine.json'), 'utf8'));
  const repositoryCommit = (await execFile('git', ['rev-parse', 'HEAD'], { cwd: repoRoot })).stdout.trim();
  const dirty = (await execFile('git', ['status', '--porcelain'], { cwd: repoRoot })).stdout.trim().length > 0;
  const scope = qualification ? `competitive-${qualificationName}` : 'playcanvas-collector-smoke';
  const runId = `${scope}-${randomUUID()}`;
  const frameBudgetMs = 1000 / sessionConfig.refreshHz;
  const nullMetrics = ['preprocess_ms', 'sort_ms', 'geometry_submit_ms', 'gpu_wait_ms', 'gpu_complete_ms'];
  const frames = outcome.result.capture.samples.map((sample, frameIndex) => ({
    schema: 'gsplat-benchmark/v1',
    record_type: 'frame',
    run_id: runId,
    frame_index: frameIndex,
    elapsed_ns: sample.elapsedNs,
    call_ms: sample.callMs,
    frame_wall_ms: sample.frameWallMs,
    preprocess_ms: null,
    sort_ms: null,
    geometry_submit_ms: null,
    gpu_wait_ms: null,
    gpu_complete_ms: null,
    visible: null,
    drawn: null,
    active_splats: sample.activeSplats,
    // The pinned GSplatHybridRenderer calls sortAndProjectForCamera for every
    // forward frame, including a fixed camera. This is an observed code-path
    // property of the locked revision, not an inference from camera motion.
    sort_refreshed: qualification ? true : null,
    trace_frame_index: sample.traceFrameIndex,
    camera_receipt: sample.cameraReceipt,
    first_frame_after_warmup_drain: sample.firstFrameAfterWarmupDrain,
    submit_version_before: sample.submitVersionBefore,
    submit_version_after: sample.submitVersionAfter,
    queue_submit_call_count: sample.queueSubmitCallCount
  }));
  const unavailableFields = [
    'environment.adapter',
    'environment.driver',
    'frames[*].visible',
    'frames[*].drawn',
    ...nullMetrics.map((metric) => `frames[*].${metric}`),
    ...(qualification ? [] : ['frames[*].sort_refreshed'])
  ];
  const manifest = {
    schema: 'gsplat-benchmark/v1',
    record_type: 'manifest',
    run_id: runId,
    identity: {
      series_id: qualification ? `competitive-paired-${qualificationName}` : 'playcanvas-collector-smoke-v1',
      started_at_utc: startedAtUtc,
      ended_at_utc: new Date().toISOString(),
      measurement_started_at_utc: outcome.result.capture.measurementStartedAtUtc,
      measurement_ended_at_utc: outcome.result.capture.measurementEndedAtUtc
    },
    build: {
      repository_commit: repositoryCommit,
      dirty,
      profile: sessionConfig.mode === 'remote-cdp'
        ? 'playcanvas-production-esm-remote-cdp'
        : 'playcanvas-production-esm',
      package_version: expectedEngine.version
    },
    dataset: {
      id: dataset.id,
      sha256: dataset.sha256,
      bytes: dataset.bytes,
      splat_count: dataset.splat_count,
      sh_degree: dataset.sh_degree
    },
    trace: {
      id: outcome.result.traceDescriptor.trace_id ?? outcome.result.traceDescriptor.id,
      sha256: outcome.result.traceDescriptor.content_sha256 ?? sha256(JSON.stringify(outcome.result.traceDescriptor)),
      camera_mode: cameraMode,
      capture_frame_index: qualification
        ? outcome.result.cameraReceipt.trace_frame_index
        : undefined,
      capture_frame_source: qualification
        ? outcome.result.capture.presentationCapture.capture_trace_frame_source
        : undefined,
      frame_index: cameraMode === 'static' ? traceFrame : undefined,
      frame_indices: cameraMode === 'sequence'
        ? outcome.result.traceDescriptor.frames.map((frame) => frame.frame_index)
        : undefined
    },
    renderer: {
      implementation: `playcanvas-${outcome.result.engineRuntimeRevision}`,
      path: outcome.result.rendererPath,
      backend: outcome.result.backendSelected,
      sort_policy: outcome.result.rendererActive
    },
    display: {
      width: outcome.result.canvasBackingWidth,
      height: outcome.result.canvasBackingHeight,
      dpr: outcome.result.devicePixelRatio,
      refresh_hz: sessionConfig.refreshHz,
      frame_budget_ms: frameBudgetMs,
      refresh_hz_source: sessionConfig.refreshHzSource,
      frame_budget_source: sessionConfig.refreshHzSource
    },
    environment: {
      platform: outcome.platform,
      os: sessionConfig.mode === 'remote-cdp'
        ? `Android ${postAndroidReceipt.device.android_release} ` +
          `(SDK ${postAndroidReceipt.device.sdk}; ${postAndroidReceipt.device.build_fingerprint})`
        : `${os.type()} ${os.release()}`,
      device: sessionConfig.mode === 'remote-cdp'
        ? `${postAndroidReceipt.device.manufacturer} ${postAndroidReceipt.device.model}`.trim()
        : os.hostname(),
      browser: `${browserVersion} ${outcome.userAgent}`,
      adapter: null,
      driver: null,
      browser_transport: sessionConfig.mode,
      host_os: `${os.type()} ${os.release()}`,
      host_device: os.hostname(),
      android_adb_serial: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.adb.serial
        : null,
      android_build_fingerprint: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.device.build_fingerprint
        : null,
      android_soc: sessionConfig.mode === 'remote-cdp'
        ? {
            manufacturer: postAndroidReceipt.device.soc_manufacturer,
            model: postAndroidReceipt.device.soc_model,
            hardware: postAndroidReceipt.device.hardware
          }
        : null,
      android_runtime_kind: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.browser_runtime.kind
        : null,
      android_runtime_host_package: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.browser_runtime.host.package
        : null,
      android_runtime_host_version: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.browser_runtime.host.version_name
        : null,
      android_runtime_engine_package: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.browser_runtime.engine.package
        : null,
      android_runtime_engine_version: sessionConfig.mode === 'remote-cdp'
        ? postAndroidReceipt.browser_runtime.engine.version_name
        : null,
      android_device_receipt: sessionConfig.mode === 'remote-cdp'
        ? 'android-device-receipt.json'
        : null
    },
    timing: {
      ...outcome.result.capture.timing,
      queue_terminal_receipt: {
        warmup_drain: queueDrainManifestReceipt(queueTerminal.warmupDrain),
        measurement_drain: queueDrainManifestReceipt(queueTerminal.measurementDrain),
        sustained: sustainedManifestReceipt(queueTerminal.sustained)
      }
    },
    policies: {
      ...outcome.result.policies,
      sortRefresh: qualification ? 'every_frame' : 'unobserved',
      sortRefreshSource: qualification
        ? 'pinned GSplatHybridRenderer.prepareRenderView -> sortAndProjectForCamera'
        : null
    },
    exactness: outcome.result.exactness,
    resolution: finalResolution,
    camera_receipt: qualification ? outcome.result.cameraReceipt : null,
    presentation_capture: qualification
      ? outcome.result.capture.presentationCapture
      : null,
    screenshot_binding: qualification ? screenshotBinding : null,
    browser_presentation: outcome.result.browserPresentationReceipt,
    device_evidence: sessionConfig.mode === 'remote-cdp'
      ? {
          direct_adb_verified: true,
          serial: postAndroidReceipt.adb.serial,
          adb_state: postAndroidReceipt.adb.state,
          forward: postAndroidReceipt.adb.forward,
          identity_stable: androidIdentityStability.stable,
          browser_identity_match: androidBrowserIdentity.identity_match,
          keyguard_locked: postAndroidReceipt.state.keyguard_locked,
          wakefulness: postAndroidReceipt.state.wakefulness,
          display_on: postAndroidReceipt.state.display_on,
          runtime_host_top_resumed: postAndroidReceipt.state.runtime_host_top_resumed,
          chrome_top_resumed: postAndroidReceipt.state.chrome_top_resumed,
          top_resumed_component: postAndroidReceipt.state.top_resumed_component,
          window_presentation: {
            pre: preAndroidWindowPresentation,
            post: postAndroidWindowPresentation,
            receipt: postAndroidReceipt.window_presentation
          },
          expected_receipt: verifiedExpectedAndroidReceipt,
          screen_receipt: androidScreenReceipt,
          canvas_screen_content_comparison: screenContentComparison
        }
      : null,
    pairing: qualification ? {
      pair_id: process.env.PHASE_E_PAIR_ID ?? null,
      run_order: process.env.PHASE_E_PAIR_ORDER ?? null,
      position: Number(process.env.PHASE_E_PAIR_POSITION ?? 0) || null
    } : undefined,
    qualification_scope: qualification ? outcome.result.policies.evidenceClass : 'collector_smoke_only',
    unavailable_fields: unavailableFields
  };
  const distributions = Object.fromEntries(
    ['call_ms', 'frame_wall_ms', ...nullMetrics].map((metric) => [metric, distribution(frames, metric)])
  );
  const summary = {
    schema: 'gsplat-benchmark/v1',
    record_type: 'summary',
    run_id: runId,
    sample_count: frames.length,
    warmup_count: outcome.result.capture.warmupCount,
    frame_budget_ms: frameBudgetMs,
    missed_frame_count: frames.filter((frame) => frame.frame_wall_ms > frameBudgetMs).length,
    distributions,
    sustained_throughput: sustainedManifestReceipt(queueTerminal.sustained)
  };
  await writeFile(resolve(outputRoot, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(resolve(outputRoot, 'frames.jsonl'), `${frames.map((frame) => JSON.stringify(frame)).join('\n')}\n`);
  await writeFile(resolve(outputRoot, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
  await execFile('python3', [resolve(repoRoot, 'tests/perf/validate-benchmark-artifacts.py'), outputRoot]);
  const hasPairing = qualification &&
    Boolean(process.env.PHASE_E_PAIR_ID) && Boolean(process.env.PHASE_E_PAIR_ORDER) &&
    Number(process.env.PHASE_E_PAIR_POSITION ?? 0) > 0;
  console.log(JSON.stringify({
    status: hasPairing
      ? 'valid_paired_candidate'
      : qualification ? 'valid_qualification_run' : 'valid_collector_smoke',
    outputRoot,
    runId,
    sampleCount: frames.length,
    captureTraceFrameIndex: qualification
      ? outcome.result.cameraReceipt.trace_frame_index
      : null,
    frameWallMeanMs: distributions.frame_wall_ms.mean,
    queueDrainAfterLastSubmitMs: queueTerminal.sustained.queueDrainAfterLastSubmitMs,
    sustainedMeanFrameMs: queueTerminal.sustained.sustainedMeanFrameMs,
    sustainedFps: queueTerminal.sustained.sustainedFps
  }));
} catch (error) {
  await writeFile(resolve(outputRoot, 'runtime.log'), `${browserLog.join('\n')}\n`);
  let blockerScreenReceipt = null;
  let blockerScreenError = null;
  let blockerDeviceReceipt = null;
  let blockerDeviceReceiptError = null;
  if (sessionConfig.mode === 'remote-cdp' && browserSession) {
    try {
      const screenCapture = await captureAndroidScreenReceipt({
        execFile,
        adbPath: sessionConfig.adbPath,
        serial: sessionConfig.adbSerial,
        expectedWidth: viewportWidth,
        expectedHeight: viewportHeight
      });
      blockerScreenReceipt = screenCapture.receipt;
      await writeFile(resolve(outputRoot, 'device-screen-blocker.png'), screenCapture.png);
    } catch (screenError) {
      blockerScreenError = screenError?.message ?? String(screenError);
    }
    try {
      blockerDeviceReceipt = await collectAndroidDeviceReceipt({
        execFile,
        adbPath: sessionConfig.adbPath,
        serial: sessionConfig.adbSerial,
        cdpPort: sessionConfig.cdpPort,
        androidRuntimeKind: sessionConfig.androidRuntimeKind,
        hostPackage: sessionConfig.hostPackage,
        runtimePackage: sessionConfig.runtimePackage,
        cdpSocket: sessionConfig.cdpSocket,
        phase: 'blocker_before_browser_cleanup'
      });
      await writeFile(
        resolve(outputRoot, 'android-device-blocker-receipt.json'),
        `${JSON.stringify(blockerDeviceReceipt, null, 2)}\n`
      );
    } catch (receiptError) {
      blockerDeviceReceiptError = receiptError?.message ?? String(receiptError);
    }
  }
  const blocker = {
    status: 'blocked',
    failure_class: classifyBrowserFailure(error),
    reason: error?.message ?? String(error),
    stack: error.stack,
    browser_transport: sessionConfig.mode,
    cdp_endpoint: sessionConfig.endpoint,
    chrome,
    blocker_screen_receipt: blockerScreenReceipt,
    blocker_screen_error: blockerScreenError,
    blocker_device_receipt: blockerDeviceReceipt,
    blocker_device_receipt_error: blockerDeviceReceiptError
  };
  await writeFile(resolve(outputRoot, 'blocker.json'), `${JSON.stringify(blocker, null, 2)}\n`);
  console.error(JSON.stringify(blocker));
  process.exitCode = 2;
} finally {
  const cleanupErrors = [];
  try {
    await browserSession?.close();
  } catch (error) {
    cleanupErrors.push({ stage: 'browser_session_close', message: error.message, stack: error.stack });
  }
  try {
    await new Promise((resolvePromise, rejectPromise) => server.close((error) => {
      if (error) rejectPromise(error);
      else resolvePromise();
    }));
  } catch (error) {
    cleanupErrors.push({ stage: 'harness_server_close', message: error.message, stack: error.stack });
  }
  if (cleanupErrors.length > 0) {
    const cleanupBlocker = {
      status: 'blocked',
      reason: 'benchmark cleanup failed',
      browser_transport: sessionConfig.mode,
      cdp_endpoint: sessionConfig.endpoint,
      cleanup_errors: cleanupErrors
    };
    await writeFile(
      resolve(outputRoot, 'cleanup-blocker.json'),
      `${JSON.stringify(cleanupBlocker, null, 2)}\n`
    );
    console.error(JSON.stringify(cleanupBlocker));
    process.exitCode = 2;
  }
}
