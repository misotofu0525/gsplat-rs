export const QUEUE_TERMINAL_SEMANTICS_SOURCE =
  'WebGPU GPUQueue.onSubmittedWorkDone resolves after all work submitted before the call has completed';

export const QUEUE_TERMINAL_SPECIFICATION_URL =
  'https://www.w3.org/TR/webgpu/#dom-gpuqueue-onsubmittedworkdone';

function requireFiniteNonNegative(value, label) {
  if (!Number.isFinite(value) || value < 0) {
    throw new Error(`${label} must be a non-negative finite number`);
  }
  return value;
}

function requireSafeNonNegativeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} must be a non-negative safe integer`);
  }
  return value;
}

export function requireQueueTerminalApi(graphicsDevice) {
  const device = graphicsDevice?.wgpu;
  const queue = device?.queue;
  if (!device || !queue || typeof queue.onSubmittedWorkDone !== 'function') {
    throw new Error(
      'PlayCanvas WebGPU device does not expose wgpu.queue.onSubmittedWorkDone()'
    );
  }
  requireSafeNonNegativeInteger(graphicsDevice.submitVersion, 'graphicsDevice.submitVersion');
  return { device, queue };
}

export function measuredFrameWallMs({
  sampleIndex,
  frameUpdateAtMs,
  measurementStartedAtMs,
  playCanvasFrameWallMs
}) {
  requireSafeNonNegativeInteger(sampleIndex, 'sampleIndex');
  requireFiniteNonNegative(frameUpdateAtMs, 'frameUpdateAtMs');
  requireFiniteNonNegative(measurementStartedAtMs, 'measurementStartedAtMs');
  requireFiniteNonNegative(playCanvasFrameWallMs, 'playCanvasFrameWallMs');
  if (frameUpdateAtMs < measurementStartedAtMs) {
    throw new Error('frame update precedes the post-drain measurement boundary');
  }
  return sampleIndex === 0
    ? frameUpdateAtMs - measurementStartedAtMs
    : playCanvasFrameWallMs;
}

export function stopApplicationFrameLoop(app) {
  if (!app || typeof app.constructor?.cancelTick !== 'function') {
    throw new Error('PlayCanvas Application.cancelTick is unavailable');
  }
  if (app.frameRequestId === null || app.frameRequestId === undefined) {
    throw new Error('PlayCanvas frame loop has no scheduled frame to cancel');
  }
  const cancelledFrameRequestId = app.frameRequestId;
  app.constructor.cancelTick(app);
  if (app.frameRequestId !== null && app.frameRequestId !== undefined) {
    throw new Error('PlayCanvas frame loop remained scheduled after cancelTick');
  }
  return cancelledFrameRequestId;
}

export function stopApplicationFrameLoopIfScheduled(app) {
  if (!app) return false;
  if (app.frameRequestId === null || app.frameRequestId === undefined) return false;
  stopApplicationFrameLoop(app);
  return true;
}

export function resumeApplicationFrameLoop(app) {
  if (!app || typeof app.requestAnimationFrame !== 'function') {
    throw new Error('PlayCanvas Application.requestAnimationFrame is unavailable');
  }
  if (app.frameRequestId !== null && app.frameRequestId !== undefined) {
    throw new Error('refusing to start a second PlayCanvas frame loop');
  }
  app.requestAnimationFrame();
  if (app.frameRequestId === null || app.frameRequestId === undefined) {
    throw new Error('PlayCanvas failed to schedule the resumed frame loop');
  }
  return app.frameRequestId;
}

export async function drainQueueWhileFrameLoopStopped(
  app,
  phase,
  { now = () => performance.now(), utcNow = () => new Date().toISOString() } = {}
) {
  if (app.frameRequestId !== null && app.frameRequestId !== undefined) {
    throw new Error(`${phase}: frame loop must be stopped before draining the GPU queue`);
  }
  const { queue } = requireQueueTerminalApi(app.graphicsDevice);
  const submitVersionBefore = requireSafeNonNegativeInteger(
    app.graphicsDevice.submitVersion,
    `${phase}.submitVersionBefore`
  );
  const startedAtMs = requireFiniteNonNegative(now(), `${phase}.startedAtMs`);
  const startedAtUtc = utcNow();
  await queue.onSubmittedWorkDone();
  const endedAtMs = requireFiniteNonNegative(now(), `${phase}.endedAtMs`);
  const submitVersionAfter = requireSafeNonNegativeInteger(
    app.graphicsDevice.submitVersion,
    `${phase}.submitVersionAfter`
  );
  if (endedAtMs < startedAtMs) {
    throw new Error(`${phase}: monotonic clock moved backwards while draining`);
  }
  if (submitVersionAfter !== submitVersionBefore) {
    throw new Error(
      `${phase}: GPU submissions continued during terminal drain ` +
      `(${submitVersionBefore} -> ${submitVersionAfter})`
    );
  }
  return {
    phase,
    api: 'app.graphicsDevice.wgpu.queue.onSubmittedWorkDone()',
    semanticsSource: QUEUE_TERMINAL_SEMANTICS_SOURCE,
    specificationUrl: QUEUE_TERMINAL_SPECIFICATION_URL,
    frameLoopStopped: true,
    submitVersionBefore,
    submitVersionAfter,
    submitVersionStable: true,
    startedAtUtc,
    endedAtUtc: utcNow(),
    drainMs: endedAtMs - startedAtMs,
    endedAtMs
  };
}

export function summarizeSustainedMeasurement({
  measuredFrameCount,
  submitVersionStart,
  submitVersionEnd,
  measurementStartedAtMs,
  measurementSubmitEndedAtMs,
  measurementQueueDrainedAtMs
}) {
  if (!Number.isSafeInteger(measuredFrameCount) || measuredFrameCount <= 0) {
    throw new Error('measuredFrameCount must be a positive safe integer');
  }
  requireSafeNonNegativeInteger(submitVersionStart, 'submitVersionStart');
  requireSafeNonNegativeInteger(submitVersionEnd, 'submitVersionEnd');
  if (submitVersionEnd <= submitVersionStart) {
    throw new Error('measurement must issue at least one WebGPU queue submission');
  }
  requireFiniteNonNegative(measurementStartedAtMs, 'measurementStartedAtMs');
  requireFiniteNonNegative(measurementSubmitEndedAtMs, 'measurementSubmitEndedAtMs');
  requireFiniteNonNegative(measurementQueueDrainedAtMs, 'measurementQueueDrainedAtMs');
  if (measurementSubmitEndedAtMs < measurementStartedAtMs) {
    throw new Error('measurement submit end precedes measurement start');
  }
  if (measurementQueueDrainedAtMs < measurementSubmitEndedAtMs) {
    throw new Error('terminal queue drain precedes the last measured submission');
  }

  const measurementSubmitSpanMs = measurementSubmitEndedAtMs - measurementStartedAtMs;
  const queueDrainAfterLastSubmitMs = measurementQueueDrainedAtMs - measurementSubmitEndedAtMs;
  const queueTerminalSpanMs = measurementQueueDrainedAtMs - measurementStartedAtMs;
  if (queueTerminalSpanMs <= 0) {
    throw new Error('queue terminal span must be positive');
  }

  return {
    method: 'stopped_raf_gpu_queue_terminal_drain',
    measuredFrameCount,
    submitVersionStart,
    submitVersionEnd,
    queueSubmitCallCount: submitVersionEnd - submitVersionStart,
    measurementSubmitSpanMs,
    queueDrainAfterLastSubmitMs,
    queueTerminalSpanMs,
    sustainedMeanFrameMs: queueTerminalSpanMs / measuredFrameCount,
    sustainedFps: (measuredFrameCount * 1000) / queueTerminalSpanMs,
    submitSpanSource:
      'post-warmup queue drain immediately before scheduling the first measured frame through final measured frameend after PlayCanvas frameEnd submission',
    queueDrainSource:
      'frame loop cancelled at final frameend, then GPUQueue.onSubmittedWorkDone awaited with stable PlayCanvas submitVersion',
    sustainedSource:
      'measured frame count divided by pre-first-frame-schedule-to-terminal-queue-drain wall time; no GPU phase timing inferred'
  };
}

export function validateQueueTerminalCapture(capture, expectedSampleCount) {
  if (!capture || !Array.isArray(capture.samples) ||
      !Number.isSafeInteger(expectedSampleCount) || expectedSampleCount <= 0 ||
      capture.samples.length !== expectedSampleCount) {
    throw new Error('queue-terminal capture sample count is invalid');
  }
  const warmupDrain = capture.warmupDrain;
  const measurementDrain = capture.measurementDrain;
  const sustained = capture.sustained;
  let submittedCallCount = 0;
  let previousSubmitVersionAfter = null;
  for (const [sampleIndex, sample] of capture.samples.entries()) {
    if (!Number.isSafeInteger(sample.submitVersionBefore) || sample.submitVersionBefore < 0 ||
        !Number.isSafeInteger(sample.submitVersionAfter) ||
        sample.submitVersionAfter <= sample.submitVersionBefore ||
        !Number.isSafeInteger(sample.queueSubmitCallCount) ||
        sample.queueSubmitCallCount !== sample.submitVersionAfter - sample.submitVersionBefore ||
        (previousSubmitVersionAfter !== null &&
          sample.submitVersionBefore !== previousSubmitVersionAfter)) {
      throw new Error(`measurement sample ${sampleIndex} lacks a contiguous WebGPU submission receipt`);
    }
    submittedCallCount += sample.queueSubmitCallCount;
    previousSubmitVersionAfter = sample.submitVersionAfter;
  }
  for (const [label, expectedPhase, receipt] of [
    ['warmupDrain', 'pre_measurement_warmup', warmupDrain],
    ['measurementDrain', 'post_measurement_terminal', measurementDrain]
  ]) {
    const startedAtEpochMs = Date.parse(receipt?.startedAtUtc);
    const endedAtEpochMs = Date.parse(receipt?.endedAtUtc);
    if (!receipt || receipt.phase !== expectedPhase ||
        receipt.frameLoopStopped !== true || receipt.submitVersionStable !== true ||
        receipt.submitVersionBefore !== receipt.submitVersionAfter ||
        receipt.api !== 'app.graphicsDevice.wgpu.queue.onSubmittedWorkDone()' ||
        receipt.semanticsSource !== QUEUE_TERMINAL_SEMANTICS_SOURCE ||
        receipt.specificationUrl !== QUEUE_TERMINAL_SPECIFICATION_URL ||
        typeof receipt.startedAtUtc !== 'string' || receipt.startedAtUtc.length === 0 ||
        typeof receipt.endedAtUtc !== 'string' || receipt.endedAtUtc.length === 0 ||
        !Number.isFinite(startedAtEpochMs) || !Number.isFinite(endedAtEpochMs) ||
        endedAtEpochMs < startedAtEpochMs ||
        !Number.isFinite(receipt.drainMs) || receipt.drainMs < 0) {
      throw new Error(`invalid fail-closed GPU queue receipt: ${label}`);
    }
  }
  if (!sustained || sustained.method !== 'stopped_raf_gpu_queue_terminal_drain' ||
      sustained.measuredFrameCount !== expectedSampleCount ||
      sustained.submitVersionStart !== warmupDrain.submitVersionAfter ||
      sustained.submitVersionEnd !== measurementDrain.submitVersionBefore ||
      capture.samples[0].submitVersionBefore !== sustained.submitVersionStart ||
      capture.samples.at(-1).submitVersionAfter !== sustained.submitVersionEnd ||
      sustained.queueSubmitCallCount !== sustained.submitVersionEnd - sustained.submitVersionStart ||
      sustained.queueSubmitCallCount !== submittedCallCount ||
      sustained.queueSubmitCallCount <= 0 ||
      !Number.isFinite(sustained.measurementSubmitSpanMs) || sustained.measurementSubmitSpanMs <= 0 ||
      !Number.isFinite(sustained.queueDrainAfterLastSubmitMs) || sustained.queueDrainAfterLastSubmitMs < 0 ||
      !Number.isFinite(sustained.queueTerminalSpanMs) || sustained.queueTerminalSpanMs <= 0 ||
      !Number.isFinite(sustained.sustainedMeanFrameMs) || sustained.sustainedMeanFrameMs <= 0 ||
      !Number.isFinite(sustained.sustainedFps) || sustained.sustainedFps <= 0) {
    throw new Error('invalid fail-closed sustained-throughput receipt');
  }
  const expectedMean = sustained.queueTerminalSpanMs / expectedSampleCount;
  const expectedFps = (expectedSampleCount * 1000) / sustained.queueTerminalSpanMs;
  const expectedTerminalSpan =
    sustained.measurementSubmitSpanMs + sustained.queueDrainAfterLastSubmitMs;
  const tailTolerance = Number.EPSILON * Math.max(
    1,
    sustained.queueDrainAfterLastSubmitMs,
    measurementDrain.drainMs
  ) * 8;
  if (Math.abs(sustained.sustainedMeanFrameMs - expectedMean) > Number.EPSILON * expectedMean * 4 ||
      Math.abs(sustained.sustainedFps - expectedFps) > Number.EPSILON * expectedFps * 4 ||
      Math.abs(sustained.queueTerminalSpanMs - expectedTerminalSpan) >
        Number.EPSILON * expectedTerminalSpan * 4 ||
      sustained.queueDrainAfterLastSubmitMs + tailTolerance < measurementDrain.drainMs) {
    throw new Error('sustained-throughput receipt is not recomputable');
  }
  const boundaryFlags = capture.samples.filter((sample) => sample.firstFrameAfterWarmupDrain === true);
  if (boundaryFlags.length !== 1 || capture.samples[0]?.firstFrameAfterWarmupDrain !== true) {
    throw new Error('measurement must identify exactly one first frame after the warmup drain');
  }
  return { warmupDrain, measurementDrain, sustained };
}
