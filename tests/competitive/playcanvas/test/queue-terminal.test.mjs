import assert from 'node:assert/strict';
import test from 'node:test';
import {
  drainQueueWhileFrameLoopStopped,
  measuredFrameWallMs,
  requireQueueTerminalApi,
  resumeApplicationFrameLoop,
  stopApplicationFrameLoop,
  stopApplicationFrameLoopIfScheduled,
  summarizeSustainedMeasurement,
  validateQueueTerminalCapture
} from '../public/queue-terminal.js';

function fakeApp({ submitVersion = 7, frameRequestId = 42, onDrain } = {}) {
  class FakeApplication {
    static cancelTick(app) {
      app.frameRequestId = undefined;
    }
  }
  const app = new FakeApplication();
  app.frameRequestId = frameRequestId;
  app.graphicsDevice = {
    submitVersion,
    wgpu: {
      queue: {
        async onSubmittedWorkDone() {
          await onDrain?.(app);
        }
      }
    }
  };
  app.requestAnimationFrame = () => {
    app.frameRequestId = 99;
  };
  return app;
}

test('frame loop stop/drain/resume preserves a stable submission boundary', async () => {
  const app = fakeApp();
  assert.equal(stopApplicationFrameLoop(app), 42);
  const clock = [10, 13];
  const receipt = await drainQueueWhileFrameLoopStopped(app, 'warmup', {
    now: () => clock.shift(),
    utcNow: () => '2026-07-23T00:00:00.000Z'
  });
  assert.equal(receipt.drainMs, 3);
  assert.equal(receipt.submitVersionBefore, 7);
  assert.equal(receipt.submitVersionAfter, 7);
  assert.equal(receipt.submitVersionStable, true);
  assert.equal(resumeApplicationFrameLoop(app), 99);
});

test('top-level failure cleanup is idempotent after cancelling the scheduled frame', () => {
  const app = fakeApp();
  assert.equal(stopApplicationFrameLoopIfScheduled(app), true);
  assert.equal(app.frameRequestId, undefined);
  assert.equal(stopApplicationFrameLoopIfScheduled(app), false);
});

test('terminal drain fails closed when submissions continue after the frame loop stops', async () => {
  const app = fakeApp({
    onDrain(current) {
      current.graphicsDevice.submitVersion += 1;
    }
  });
  stopApplicationFrameLoop(app);
  await assert.rejects(
    drainQueueWhileFrameLoopStopped(app, 'measurement'),
    /GPU submissions continued during terminal drain/
  );
});

test('terminal API check rejects missing WebGPU queue completion support', () => {
  assert.throws(
    () => requireQueueTerminalApi({ submitVersion: 0, wgpu: { queue: {} } }),
    /onSubmittedWorkDone/
  );
});

test('first measured frame wall excludes the deliberate warmup queue drain', () => {
  assert.equal(measuredFrameWallMs({
    sampleIndex: 0,
    frameUpdateAtMs: 116.7,
    measurementStartedAtMs: 100,
    playCanvasFrameWallMs: 105.5
  }), 16.700000000000003);
  assert.equal(measuredFrameWallMs({
    sampleIndex: 1,
    frameUpdateAtMs: 133.4,
    measurementStartedAtMs: 100,
    playCanvasFrameWallMs: 16.7
  }), 16.7);
});

test('sustained throughput includes terminal queue drain and never invents GPU phase time', () => {
  const receipt = summarizeSustainedMeasurement({
    measuredFrameCount: 600,
    submitVersionStart: 10,
    submitVersionEnd: 1210,
    measurementStartedAtMs: 100,
    measurementSubmitEndedAtMs: 10_000,
    measurementQueueDrainedAtMs: 10_300
  });
  assert.equal(receipt.queueSubmitCallCount, 1200);
  assert.equal(receipt.measurementSubmitSpanMs, 9900);
  assert.equal(receipt.queueDrainAfterLastSubmitMs, 300);
  assert.equal(receipt.queueTerminalSpanMs, 10_200);
  assert.equal(receipt.sustainedMeanFrameMs, 17);
  assert.equal(receipt.sustainedFps, 1000 / 17);
  assert.match(receipt.sustainedSource, /no GPU phase timing inferred/);
});

test('collector-side receipt validation rejects an unproven boundary', () => {
  const sustained = summarizeSustainedMeasurement({
    measuredFrameCount: 1,
    submitVersionStart: 10,
    submitVersionEnd: 11,
    measurementStartedAtMs: 100,
    measurementSubmitEndedAtMs: 116,
    measurementQueueDrainedAtMs: 117
  });
  const drain = (phase, version) => ({
    phase,
    api: 'app.graphicsDevice.wgpu.queue.onSubmittedWorkDone()',
    semanticsSource:
      'WebGPU GPUQueue.onSubmittedWorkDone resolves after all work submitted before the call has completed',
    specificationUrl: 'https://www.w3.org/TR/webgpu/#dom-gpuqueue-onsubmittedworkdone',
    frameLoopStopped: true,
    submitVersionBefore: version,
    submitVersionAfter: version,
    submitVersionStable: true,
    startedAtUtc: '2026-07-23T00:00:00.000Z',
    endedAtUtc: '2026-07-23T00:00:00.001Z',
    drainMs: 1
  });
  const capture = {
    samples: [{
      firstFrameAfterWarmupDrain: true,
      submitVersionBefore: 10,
      submitVersionAfter: 11,
      queueSubmitCallCount: 1
    }],
    warmupDrain: drain('pre_measurement_warmup', 10),
    measurementDrain: drain('post_measurement_terminal', 11),
    sustained
  };
  assert.deepEqual(validateQueueTerminalCapture(capture, 1), {
    warmupDrain: capture.warmupDrain,
    measurementDrain: capture.measurementDrain,
    sustained
  });
  capture.measurementDrain.drainMs = 1.1;
  assert.throws(
    () => validateQueueTerminalCapture(capture, 1),
    /sustained-throughput receipt is not recomputable/
  );
  capture.measurementDrain.drainMs = 1;
  capture.warmupDrain.endedAtUtc = '2026-07-22T23:59:59.999Z';
  assert.throws(
    () => validateQueueTerminalCapture(capture, 1),
    /warmupDrain/
  );
  capture.warmupDrain.endedAtUtc = '2026-07-23T00:00:00.001Z';
  capture.samples[0].submitVersionAfter = 10;
  capture.samples[0].queueSubmitCallCount = 0;
  assert.throws(
    () => validateQueueTerminalCapture(capture, 1),
    /sample 0/
  );
  capture.samples[0].submitVersionAfter = 11;
  capture.samples[0].queueSubmitCallCount = 1;
  capture.measurementDrain.submitVersionAfter = 12;
  assert.throws(
    () => validateQueueTerminalCapture(capture, 1),
    /measurementDrain/
  );
});

test('collector-side receipt validation rejects an unobserved submission gap', () => {
  const sustained = summarizeSustainedMeasurement({
    measuredFrameCount: 2,
    submitVersionStart: 10,
    submitVersionEnd: 13,
    measurementStartedAtMs: 100,
    measurementSubmitEndedAtMs: 132,
    measurementQueueDrainedAtMs: 133
  });
  const drain = (phase, version) => ({
    phase,
    api: 'app.graphicsDevice.wgpu.queue.onSubmittedWorkDone()',
    semanticsSource:
      'WebGPU GPUQueue.onSubmittedWorkDone resolves after all work submitted before the call has completed',
    specificationUrl: 'https://www.w3.org/TR/webgpu/#dom-gpuqueue-onsubmittedworkdone',
    frameLoopStopped: true,
    submitVersionBefore: version,
    submitVersionAfter: version,
    submitVersionStable: true,
    startedAtUtc: '2026-07-23T00:00:00.000Z',
    endedAtUtc: '2026-07-23T00:00:00.001Z',
    drainMs: 1
  });
  const capture = {
    samples: [
      {
        firstFrameAfterWarmupDrain: true,
        submitVersionBefore: 10,
        submitVersionAfter: 11,
        queueSubmitCallCount: 1
      },
      {
        firstFrameAfterWarmupDrain: false,
        submitVersionBefore: 12,
        submitVersionAfter: 13,
        queueSubmitCallCount: 1
      }
    ],
    warmupDrain: drain('pre_measurement_warmup', 10),
    measurementDrain: drain('post_measurement_terminal', 13),
    sustained
  };
  assert.throws(
    () => validateQueueTerminalCapture(capture, 2),
    /sample 1/
  );
});
