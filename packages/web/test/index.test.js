import assert from "node:assert/strict";
import test from "node:test";

import {
  GsplatWebRenderer,
  GsplatWebError,
  createGsplatRenderer,
  createGsplatRendererFromStream,
  createGsplatRendererFromUrl,
  getGsplatApiVersion,
  initGsplatWeb,
} from "../src/index.js";

function makeNativeRenderer(overrides = {}) {
  const calls = [];
  let surfaceWidth = 640;
  let surfaceHeight = 480;
  return {
    calls,
    resize(width, height) {
      calls.push(["resize", width, height]);
    },
    async resizeAsync(width, height) {
      calls.push(["resizeAsync", width, height]);
      surfaceWidth = width;
      surfaceHeight = height;
    },
    resetCamera() {
      calls.push(["resetCamera"]);
    },
    setCamera(values) {
      calls.push(["setCamera", ...values]);
    },
    cameraReceipt() {
      return new Float32Array([1, 2, 3, 0, 0, 0, 1, Math.PI / 3, 0.1, 100]);
    },
    orbit(deltaYawRadians, deltaPitchRadians) {
      calls.push(["orbit", deltaYawRadians, deltaPitchRadians]);
    },
    zoom(distanceScale) {
      calls.push(["zoom", distanceScale]);
    },
    pan(normalizedDeltaX, normalizedDeltaY) {
      calls.push(["pan", normalizedDeltaX, normalizedDeltaY]);
    },
    setSortInterval(interval) {
      calls.push(["setSortInterval", interval]);
    },
    setGeometryPath(path) {
      calls.push(["setGeometryPath", path]);
    },
    async setGeometryPathAsync(path) {
      calls.push(["setGeometryPathAsync", path]);
    },
    async prepareGpuOrder() {},
    renderFrame() {
      calls.push(["renderFrame"]);
      return {
        frameMs: "1.25",
        preprocessMs: null,
        sortMs: 0.5,
        rasterMs: Number.NaN,
        cpuGeometryMs: "0.25",
        renderSubmitMs: 0.75,
        frameWallMs: 1.5,
        visibleCount: "2",
        drawnCount: 1,
        refreshSort: 1,
        surfaceWidth: "640",
        surfaceHeight: 480,
        internalRenderWidth: "640",
        internalRenderHeight: 480,
        presentedWidth: "640",
        presentedHeight: 480,
      };
    },
    drainOrderMeasurementReceipts() {
      calls.push(["drainOrderMeasurementReceipts"]);
      return {
        completedCpuOrderMeasurements: [],
        completedOrderMeasurements: [],
        failedOrderMeasurements: [],
      };
    },
    sceneSummary() {
      return {
        gaussians: "3",
        shDegree: "1",
        hasShRest: 1,
      };
    },
    loadReceipt() {
      return {
        transportBytes: "1024",
        peakDecoderBufferBytes: "64",
        streamed: 1,
        inputSha256: "abc123",
        sourceCount: "3",
        decodedCount: 3,
        encodedCount: "3",
        residentCount: "3",
        addressableCount: 3,
        sourceShDegree: "1",
        residentShDegree: 1,
        shDegree: "1",
        fullQuality: 1,
        sourceMembership: "all",
        samplingEnabled: false,
        lodEnabled: false,
        partialScenePublished: false,
      };
    },
    surfaceSize() {
      return {
        width: String(surfaceWidth),
        height: surfaceHeight,
      };
    },
    free() {
      calls.push(["free"]);
    },
    ...overrides,
  };
}

function projectedIssuedFrame({ ticket, cameraRevision, execution, orderBackend }) {
  return {
    projectedPolicy: "adaptive",
    projectedExecution: execution,
    projectedAdaptiveState: execution === "compact" ? "compact_probe" : "candidate_learning",
    projectedMeasurementSubmission: "issued",
    projectedMeasurementTicket: ticket,
    projectedMeasurementExecution: execution,
    projectedMeasurementUnsampledReason: null,
    cameraRevision,
    orderBackend,
  };
}

function projectedSuccess({
  ticket,
  cameraRevision,
  execution,
  orderBackend,
  projectionGeneration = 1,
  probeGeneration = 1,
  visibleCount = 100,
  contributorCount = 81,
}) {
  const compact = execution === "compact";
  return {
    ticket,
    cameraRevision,
    execution,
    orderBackend,
    projectionGeneration,
    probeGeneration,
    projectionRebuilt: true,
    orderRefreshed: false,
    frameCompleteMs: 1,
    countSemantics: "candidate_visible_contributor_issued_v1",
    visibleCount,
    contributorCount,
    drawnCount: compact ? contributorCount : visibleCount,
    exactContributorCompaction: compact,
  };
}

test("GsplatWebRenderer validates native renderer handle", () => {
  assert.throws(() => new GsplatWebRenderer(null), TypeError);
});

test("GsplatWebRenderer preserves every completed GPU order receipt", () => {
  const native = makeNativeRenderer({
    renderFrame() {
      return {
        completedOrderMeasurements: [{
          ticket: "7",
          cameraRevision: "11",
          timingSource: "timestamp_query",
          gpuPreprocessMs: "0.25",
          gpuRadixMs: 1.5,
          gpuOrderMs: "1.75",
          gpuCompleteMs: "3.5",
          timestampPeriodNs: 1,
          belowTimestampResolution: 0,
          countSemantics: "candidate_visible_contributor_issued_v1",
          visibleCount: "123",
          contributorCount: "117",
          drawnCount: 117,
          exactContributorCompaction: true,
        }],
      };
    },
  });

  assert.deepEqual(new GsplatWebRenderer(native).renderFrame().completedOrderMeasurements, [{
    ticket: 7,
    cameraRevision: 11,
    actualBackend: "gpu",
    timingSource: "timestamp_query",
    gpuPreprocessMs: 0.25,
    gpuRadixMs: 1.5,
    gpuOrderMs: 1.75,
    gpuCompleteMs: 3.5,
    timestampPeriodNs: 1,
    belowTimestampResolution: false,
    countSemantics: "candidate_visible_contributor_issued_v1",
    visibleCount: 123,
    contributorCount: 117,
    drawnCount: 117,
    exactContributorCompaction: true,
  }]);
});

test("GsplatWebRenderer preserves terminal success and structured failure receipts", () => {
  const native = makeNativeRenderer({
    drainOrderMeasurementReceipts() {
      return {
        completedCpuOrderMeasurements: [{
          ticket: "8",
          cameraRevision: "12",
          actualBackend: "cpu",
          preprocessMs: "0.5",
          sortMs: "1.25",
          frameCompleteMs: "5.0",
          countSemantics: "candidate_visible_contributor_issued_v1",
          visibleCount: "48",
          contributorCount: "42",
          drawnCount: "42",
          exactContributorCompaction: true,
        }],
        completedOrderMeasurements: [{
          ticket: "9",
          cameraRevision: "13",
          timingSource: "completion_only",
          gpuCompleteMs: "4.25",
          countSemantics: "candidate_visible_contributor_issued_v1",
          visibleCount: "42",
          contributorCount: "40",
          drawnCount: 42,
          exactContributorCompaction: false,
        }],
        failedOrderMeasurements: [{
          ticket: "10",
          cameraRevision: "14",
          actualBackend: "cpu",
          reason: "readback_map",
        }],
      };
    },
  });

  assert.deepEqual(new GsplatWebRenderer(native).drainOrderMeasurementReceipts(), {
    completedCpuOrderMeasurements: [{
      ticket: 8,
      cameraRevision: 12,
      actualBackend: "cpu",
      preprocessMs: 0.5,
      sortMs: 1.25,
      frameCompleteMs: 5,
      countSemantics: "candidate_visible_contributor_issued_v1",
      visibleCount: 48,
      contributorCount: 42,
      drawnCount: 42,
      exactContributorCompaction: true,
    }],
    completedOrderMeasurements: [{
      ticket: 9,
      cameraRevision: 13,
      actualBackend: "gpu",
      timingSource: "completion_only",
      gpuPreprocessMs: null,
      gpuRadixMs: null,
      gpuOrderMs: null,
      gpuCompleteMs: 4.25,
      timestampPeriodNs: null,
      belowTimestampResolution: false,
      countSemantics: "candidate_visible_contributor_issued_v1",
      visibleCount: 42,
      contributorCount: 40,
      drawnCount: 42,
      exactContributorCompaction: false,
    }],
    failedOrderMeasurements: [{
      ticket: 10,
      cameraRevision: 14,
      actualBackend: "cpu",
      reason: "readback_map",
    }],
    completedProjectedMeasurements: [],
    failedProjectedMeasurements: [],
    completedGpuProducerMeasurements: [],
    failedGpuProducerMeasurements: [],
  });
});

test("GsplatWebRenderer preserves JS-safe high projected tickets and complete terminals", () => {
  const highestSafeTicket = "9007199254740991";
  const failureTicket = "4503599627370497";
  const issued = [
    projectedIssuedFrame({
      ticket: highestSafeTicket,
      cameraRevision: 23,
      execution: "compact",
      orderBackend: "gpu",
    }),
    projectedIssuedFrame({
      ticket: failureTicket,
      cameraRevision: 24,
      execution: "candidate",
      orderBackend: "cpu",
    }),
  ];
  const native = makeNativeRenderer({
    renderFrame() {
      return issued.shift();
    },
    drainOrderMeasurementReceipts() {
      return {
        completedCpuOrderMeasurements: [],
        completedOrderMeasurements: [],
        failedOrderMeasurements: [],
        completedProjectedMeasurements: [{
          ticket: highestSafeTicket,
          cameraRevision: "23",
          execution: "compact",
          orderBackend: "gpu",
          projectionGeneration: "31",
          probeGeneration: "7",
          projectionRebuilt: 1,
          orderRefreshed: 0,
          frameCompleteMs: "4.75",
          countSemantics: "candidate_visible_contributor_issued_v1",
          visibleCount: "100",
          contributorCount: "81",
          drawnCount: "81",
          exactContributorCompaction: true,
        }],
        failedProjectedMeasurements: [{
          ticket: failureTicket,
          cameraRevision: "24",
          execution: "candidate",
          orderBackend: "cpu",
          projectionGeneration: "32",
          probeGeneration: "8",
          reason: "generation_invalidated",
        }],
      };
    },
  });

  const renderer = new GsplatWebRenderer(native);
  renderer.renderFrame();
  renderer.renderFrame();
  const receipts = renderer.drainOrderMeasurementReceipts();
  assert.deepEqual(receipts.completedProjectedMeasurements, [{
    ticket: Number(highestSafeTicket),
    cameraRevision: 23,
    execution: "compact",
    orderBackend: "gpu",
    projectionGeneration: 31,
    probeGeneration: 7,
    projectionRebuilt: true,
    orderRefreshed: false,
    frameCompleteMs: 4.75,
    countSemantics: "candidate_visible_contributor_issued_v1",
    visibleCount: 100,
    contributorCount: 81,
    drawnCount: 81,
    exactContributorCompaction: true,
  }]);
  assert.deepEqual(receipts.failedProjectedMeasurements, [{
    ticket: Number(failureTicket),
    cameraRevision: 24,
    execution: "candidate",
    orderBackend: "cpu",
    projectionGeneration: 32,
    probeGeneration: 8,
    reason: "generation_invalidated",
  }]);
  assert.equal(Number.isSafeInteger(receipts.completedProjectedMeasurements[0].ticket), true);
});

test("GsplatWebRenderer rejects unsafe or multiply-terminal projected tickets", () => {
  const unsafe = new GsplatWebRenderer(makeNativeRenderer({
    drainOrderMeasurementReceipts() {
      return {
        completedProjectedMeasurements: [{
          ticket: "9007199254740992",
          cameraRevision: 1,
          projectionGeneration: 1,
          probeGeneration: 1,
        }],
      };
    },
  }));
  assert.throws(
    () => unsafe.drainOrderMeasurementReceipts(),
    /projected measurement ticket must be a positive JS-safe integer/,
  );

  const duplicate = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      return projectedIssuedFrame({
        ticket: "4503599627370496",
        cameraRevision: 1,
        execution: "candidate",
        orderBackend: "cpu",
      });
    },
    drainOrderMeasurementReceipts() {
      const identity = {
        ticket: "4503599627370496",
        cameraRevision: 1,
        execution: "candidate",
        orderBackend: "cpu",
        projectionGeneration: 1,
        probeGeneration: 1,
      };
      return {
        completedProjectedMeasurements: [{
          ...projectedSuccess({
            ticket: identity.ticket,
            cameraRevision: identity.cameraRevision,
            execution: identity.execution,
            orderBackend: identity.orderBackend,
          }),
          projectionGeneration: identity.projectionGeneration,
          probeGeneration: identity.probeGeneration,
        }],
        failedProjectedMeasurements: [{ ...identity, reason: "readback_map" }],
      };
    },
  }));
  duplicate.renderFrame();
  assert.throws(
    () => duplicate.drainOrderMeasurementReceipts(),
    /projected measurement ticket 4503599627370496 has multiple terminals/,
  );

  const lowNamespace = new GsplatWebRenderer(makeNativeRenderer({
    drainOrderMeasurementReceipts() {
      return {
        completedProjectedMeasurements: [{
          ticket: "41",
          cameraRevision: 1,
          execution: "candidate",
          orderBackend: "cpu",
          projectionGeneration: 1,
          probeGeneration: 1,
          frameCompleteMs: 1,
        }],
      };
    },
  }));
  assert.throws(
    () => lowNamespace.drainOrderMeasurementReceipts(),
    /outside the projected ticket namespace/,
  );
});

test("GsplatWebRenderer permits exactly one terminal across render and standalone drains", () => {
  const terminal = projectedSuccess({
    ticket: "4503599627370496",
    cameraRevision: 1,
    execution: "candidate",
    orderBackend: "cpu",
    projectionGeneration: 1,
    probeGeneration: 1,
  });
  let rendered = 0;
  const renderer = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      rendered += 1;
      if (rendered === 1) {
        return projectedIssuedFrame({
          ticket: terminal.ticket,
          cameraRevision: terminal.cameraRevision,
          execution: terminal.execution,
          orderBackend: terminal.orderBackend,
        });
      }
      return {
        projectedPolicy: "adaptive",
        projectedExecution: "candidate",
        projectedAdaptiveState: "candidate_stable",
        projectedMeasurementSubmission: "not_requested",
        cameraRevision: 2,
        orderBackend: "cpu",
        completedProjectedMeasurements: [terminal],
      };
    },
    drainOrderMeasurementReceipts() {
      return { failedProjectedMeasurements: [{ ...terminal, reason: "readback_map" }] };
    },
  }));
  renderer.renderFrame();
  assert.equal(renderer.renderFrame().completedProjectedMeasurements.length, 1);
  assert.throws(
    () => renderer.drainOrderMeasurementReceipts(),
    /projected measurement ticket 4503599627370496 already terminated/,
  );
});

test("GsplatWebRenderer rejects unissued or identity-changing projected terminals", () => {
  const success = projectedSuccess({
    ticket: "4503599627370496",
    cameraRevision: 7,
    execution: "candidate",
    orderBackend: "cpu",
  });
  const unissued = new GsplatWebRenderer(makeNativeRenderer({
    drainOrderMeasurementReceipts() {
      return { completedProjectedMeasurements: [success] };
    },
  }));
  assert.throws(
    () => unissued.drainOrderMeasurementReceipts(),
    /projected measurement ticket 4503599627370496 was never issued/,
  );

  const changed = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      return projectedIssuedFrame({
        ticket: success.ticket,
        cameraRevision: success.cameraRevision,
        execution: success.execution,
        orderBackend: success.orderBackend,
      });
    },
    drainOrderMeasurementReceipts() {
      return { completedProjectedMeasurements: [{ ...success, orderBackend: "gpu" }] };
    },
  }));
  changed.renderFrame();
  assert.throws(
    () => changed.drainOrderMeasurementReceipts(),
    /projected measurement ticket 4503599627370496 changed terminal identity/,
  );
});

test("GsplatWebRenderer rejects invalid projected timing and V-C-D evidence", () => {
  const invalidCounts = projectedSuccess({
    ticket: "4503599627370496",
    cameraRevision: 7,
    execution: "compact",
    orderBackend: "gpu",
  });
  invalidCounts.drawnCount += 1;
  const countsRenderer = new GsplatWebRenderer(makeNativeRenderer({
    drainOrderMeasurementReceipts() {
      return { completedProjectedMeasurements: [invalidCounts] };
    },
  }));
  assert.throws(
    () => countsRenderer.drainOrderMeasurementReceipts(),
    /has invalid V\/C\/D evidence/,
  );

  const invalidTiming = projectedSuccess({
    ticket: "4503599627370497",
    cameraRevision: 8,
    execution: "candidate",
    orderBackend: "cpu",
  });
  invalidTiming.frameCompleteMs = Number.NaN;
  const timingRenderer = new GsplatWebRenderer(makeNativeRenderer({
    drainOrderMeasurementReceipts() {
      return { completedProjectedMeasurements: [invalidTiming] };
    },
  }));
  assert.throws(
    () => timingRenderer.drainOrderMeasurementReceipts(),
    /frameCompleteMs must be a non-negative finite number/,
  );
});

test("forced projected frames expose execution without manufacturing a ticket", () => {
  const forced = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      return {
        projectedPolicy: "compact",
        projectedExecution: "compact",
        projectedAdaptiveState: "disabled",
        projectedMeasurementSubmission: "not_requested",
        projectedMeasurementTicket: null,
      };
    },
  })).renderFrame();
  assert.equal(forced.projectedPolicy, "compact");
  assert.equal(forced.projectedExecution, "compact");
  assert.equal(forced.projectedMeasurementSubmission, "not_requested");
  assert.equal(forced.projectedMeasurementTicket, null);

  const fake = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      return {
        projectedPolicy: "candidate",
        projectedExecution: "candidate",
        projectedMeasurementSubmission: "issued",
        projectedMeasurementTicket: "4503599627370496",
        projectedMeasurementExecution: "candidate",
      };
    },
  }));
  assert.throws(
    () => fake.renderFrame(),
    /forced projected policy must match execution, disable adaptation, and expose no sample/,
  );

  const mismatched = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      return {
        projectedPolicy: "compact",
        projectedExecution: "candidate",
        projectedAdaptiveState: "disabled",
        projectedMeasurementSubmission: "not_requested",
      };
    },
  }));
  assert.throws(
    () => mismatched.renderFrame(),
    /forced projected policy must match execution/,
  );
});

test("adaptive projected frame exposes submission and scalar terminal compatibility", () => {
  let rendered = 0;
  const renderer = new GsplatWebRenderer(makeNativeRenderer({
    renderFrame() {
      rendered += 1;
      if (rendered === 1) {
        return projectedIssuedFrame({
          ticket: "4503599627370497",
          cameraRevision: 41,
          execution: "candidate",
          orderBackend: "cpu",
        });
      }
      if (rendered === 2) {
        return projectedIssuedFrame({
          ticket: "4503599627370498",
          cameraRevision: 42,
          execution: "compact",
          orderBackend: "gpu",
        });
      }
      return {
        projectedPolicy: "adaptive",
        projectedExecution: "compact",
        projectedAdaptiveState: "compact_probe",
        projectedMeasurementSubmission: "issued",
        projectedMeasurementTicket: "4503599627370499",
        projectedMeasurementExecution: "compact",
        cameraRevision: "43",
        orderBackend: "gpu",
        completedProjectedMeasurementAvailable: true,
        completedProjectedMeasurementTicket: "4503599627370497",
        completedProjectedMeasurementRevision: "41",
        completedProjectedMeasurementExecution: "candidate",
        completedProjectedMeasurementOrderBackend: "cpu",
        completedProjectedProjectionGeneration: "101",
        completedProjectedProbeGeneration: "9",
        completedProjectedProjectionRebuilt: true,
        completedProjectedOrderRefreshed: false,
        completedProjectedFrameCompleteMs: "6.25",
        completedProjectedVisibleCount: 90,
        completedProjectedContributorCount: 77,
        completedProjectedDrawnCount: 90,
        completedProjectedExactContributorCompaction: false,
        failedProjectedMeasurementAvailable: true,
        failedProjectedMeasurementTicket: "4503599627370498",
        failedProjectedMeasurementRevision: "42",
        failedProjectedMeasurementExecution: "compact",
        failedProjectedMeasurementOrderBackend: "gpu",
        failedProjectedProjectionGeneration: "102",
        failedProjectedProbeGeneration: "10",
        failedProjectedMeasurementReason: "invariant_violation",
      };
    },
  }));
  renderer.renderFrame();
  renderer.renderFrame();
  const frame = renderer.renderFrame();

  assert.equal(frame.projectedMeasurementSubmission, "issued");
  assert.equal(frame.projectedMeasurementTicket, 4503599627370499);
  assert.equal(frame.projectedAdaptiveState, "compact_probe");
  assert.deepEqual(frame.completedProjectedMeasurements, [{
    ticket: 4503599627370497,
    cameraRevision: 41,
    execution: "candidate",
    orderBackend: "cpu",
    projectionGeneration: 101,
    probeGeneration: 9,
    projectionRebuilt: true,
    orderRefreshed: false,
    frameCompleteMs: 6.25,
    countSemantics: "candidate_visible_contributor_issued_v1",
    visibleCount: 90,
    contributorCount: 77,
    drawnCount: 90,
    exactContributorCompaction: false,
  }]);
  assert.equal(frame.failedProjectedMeasurements[0].reason, "invariant_violation");
});

test("GsplatWebRenderer normalizes frame-level adaptive and terminal failure fields", () => {
  const native = makeNativeRenderer({
    renderFrame() {
      return {
        adaptiveGpuFailure: "validation",
        failedMeasurementAvailable: 1,
        failedMeasurementTicket: "21",
        failedMeasurementRevision: "34",
        failedMeasurementReason: "generation_invalidated",
      };
    },
  });

  const frame = new GsplatWebRenderer(native).renderFrame();
  assert.equal(frame.adaptiveGpuFailure, "validation");
  assert.equal(frame.failedMeasurementAvailable, true);
  assert.equal(frame.failedMeasurementTicket, 21);
  assert.equal(frame.failedMeasurementRevision, 34);
  assert.equal(frame.failedMeasurementReason, "generation_invalidated");
  assert.deepEqual(frame.failedOrderMeasurements, [{
    ticket: 21,
    cameraRevision: 34,
    actualBackend: "gpu",
    reason: "generation_invalidated",
  }]);
});

test("GsplatWebRenderer forwards commands and normalizes return values", async () => {
  const native = makeNativeRenderer();
  native.setProjectedPolicy = (policy) => native.calls.push(["setProjectedPolicy", policy]);
  const renderer = new GsplatWebRenderer(native);

  await renderer.resize(800, 600);
  renderer.resetCamera();
  renderer.setCamera({
    position: [1, 2, 3],
    rotationXyzw: [0, 0, 0, 1],
    intrinsics: { verticalFovRadians: 1, nearPlane: 0.1, farPlane: 100 },
  });
  renderer.orbit(0.1, -0.2);
  renderer.zoom(1.1);
  renderer.pan(0.05, -0.05);
  renderer.setSortInterval(2);
  renderer.setGeometryPath("paged");
  renderer.setProjectedPolicy("compact");

  assert.deepEqual(renderer.renderFrame(), {
    frameMs: 1.25,
    preprocessMs: 0,
    sortMs: 0.5,
    rasterMs: 0,
    cpuGeometryMs: 0.25,
    renderSubmitMs: 0.75,
    frameWallMs: 1.5,
    framePresented: true,
    gpuOrderPreparationPending: false,
    tiledPreparationPending: false,
    rasterExecutionPlan: "global_quads",
    visibleCount: 2,
    drawnCount: 1,
    refreshSort: true,
    orderBackend: "cpu",
    adaptiveGpuFailure: null,
    gpuSortFallback: false,
    adaptiveState: "disabled",
    projectedPolicy: "adaptive",
    projectedExecution: "candidate",
    projectedAdaptiveState: "disabled",
    projectedMeasurementSubmission: "not_requested",
    projectedMeasurementTicket: null,
    projectedMeasurementExecution: null,
    projectedMeasurementUnsampledReason: null,
    gpuOrderProducer: null,
    gpuProducerMeasurementSubmission: "not_requested",
    gpuProducerMeasurementTicket: null,
    gpuProducerMeasurementProducer: null,
    gpuProducerMeasurementUnsampledReason: null,
    cameraRevision: 0,
    appliedOrderRevision: 0,
    presentedOrderRevisionLag: 0,
    submittedMeasurementTicket: null,
    submittedMeasurementBackend: null,
    measurementUnsampledReason: null,
    visibleCountRevision: null,
    visibleCountPending: false,
    gpuTimestampQueriesEnabled: false,
    completedMeasurementAvailable: false,
    completedMeasurementTicket: null,
    completedMeasurementRevision: null,
    completedMeasurementTimingSource: null,
    gpuPreprocessMs: null,
    gpuRadixMs: null,
    gpuOrderMs: null,
    gpuCompleteMs: null,
    gpuTimestampPeriodNs: null,
    gpuBelowTimestampResolution: null,
    completedVisibleCount: null,
    completedContributorCount: null,
    completedDrawnCount: null,
    completedExactContributorCompaction: null,
    failedMeasurementAvailable: false,
    failedMeasurementTicket: null,
    failedMeasurementRevision: null,
    failedMeasurementReason: null,
    completedProjectedMeasurementAvailable: false,
    completedProjectedMeasurementTicket: null,
    completedProjectedMeasurementRevision: null,
    completedProjectedMeasurementExecution: null,
    completedProjectedMeasurementOrderBackend: null,
    completedProjectedProjectionGeneration: null,
    completedProjectedProbeGeneration: null,
    completedProjectedProjectionRebuilt: null,
    completedProjectedOrderRefreshed: null,
    completedProjectedFrameCompleteMs: null,
    completedProjectedVisibleCount: null,
    completedProjectedContributorCount: null,
    completedProjectedDrawnCount: null,
    completedProjectedExactContributorCompaction: null,
    failedProjectedMeasurementAvailable: false,
    failedProjectedMeasurementTicket: null,
    failedProjectedMeasurementRevision: null,
    failedProjectedMeasurementExecution: null,
    failedProjectedMeasurementOrderBackend: null,
    failedProjectedProjectionGeneration: null,
    failedProjectedProbeGeneration: null,
    failedProjectedMeasurementReason: null,
    completedGpuProducerMeasurementAvailable: false,
    completedGpuProducerMeasurementTicket: null,
    completedGpuProducerMeasurementRevision: null,
    completedGpuProducerMeasurementProducer: null,
    completedGpuProducerOrderGeneration: null,
    completedGpuProducerProjectionGeneration: null,
    completedGpuProducerSourceCount: null,
    completedGpuProducerContributorCount: null,
    completedGpuProducerDrawnCount: null,
    completedGpuProducerOrderRefreshed: null,
    completedGpuProducerDrawScope: null,
    completedGpuProducerExactCurrentContributorDraw: null,
    completedGpuProducerStaleOrder: null,
    completedGpuProducerQueueCompleteMs: null,
    failedGpuProducerMeasurementAvailable: false,
    failedGpuProducerMeasurementTicket: null,
    failedGpuProducerMeasurementRevision: null,
    failedGpuProducerMeasurementProducer: null,
    failedGpuProducerOrderGeneration: null,
    failedGpuProducerProjectionGeneration: null,
    failedGpuProducerMeasurementReason: null,
    completedCpuOrderMeasurements: [],
    completedOrderMeasurements: [],
    failedOrderMeasurements: [],
    completedProjectedMeasurements: [],
    failedProjectedMeasurements: [],
    completedGpuProducerMeasurements: [],
    failedGpuProducerMeasurements: [],
    surfaceWidth: 640,
    surfaceHeight: 480,
    internalRenderWidth: 640,
    internalRenderHeight: 480,
    presentedWidth: 640,
    presentedHeight: 480,
  });
  assert.deepEqual(renderer.sceneSummary(), {
    gaussians: 3,
    shDegree: 1,
    hasShRest: true,
  });
  assert.deepEqual(renderer.loadReceipt(), {
    transportBytes: 1024,
    peakDecoderBufferBytes: 64,
    streamed: true,
    inputSha256: "abc123",
    sourceCount: 3,
    decodedCount: 3,
    encodedCount: 3,
    residentCount: 3,
    addressableCount: 3,
    sourceShDegree: 1,
    residentShDegree: 1,
    shDegree: 1,
    fullQuality: true,
    sourceMembership: "all",
    samplingEnabled: false,
    lodEnabled: false,
    partialScenePublished: false,
  });
  assert.deepEqual(renderer.surfaceSize(), {
    width: 800,
    height: 600,
  });
  const cameraReceipt = renderer.cameraReceipt();
  assert.deepEqual(cameraReceipt.position, [1, 2, 3]);
  assert.deepEqual(cameraReceipt.rotationXyzw, [0, 0, 0, 1]);
  assert.ok(Math.abs(cameraReceipt.intrinsics.verticalFovRadians - Math.PI / 3) < 1e-6);
  renderer.free();

  assert.deepEqual(native.calls.slice(0, 11), [
    ["resizeAsync", 800, 600],
    ["resetCamera"],
    ["setCamera", 1, 2, 3, 0, 0, 0, 1, 1, Math.fround(0.1), 100],
    ["orbit", 0.1, -0.2],
    ["zoom", 1.1],
    ["pan", 0.05, -0.05],
    ["setSortInterval", 2],
    ["setGeometryPath", 2],
    ["setProjectedPolicy", 1],
    ["renderFrame"],
    ["free"],
  ]);
  assert.equal(renderer.isDisposed, true);
  assert.throws(() => renderer.renderFrame(), /disposed/);
});

test("GsplatWebRenderer same-size resize is a no-op on a legacy module", async () => {
  const native = makeNativeRenderer();
  delete native.resizeAsync;
  const renderer = new GsplatWebRenderer(native);

  await renderer.resize(640, 480);

  assert.deepEqual(native.calls, []);
});

test("GsplatWebRenderer changed-size legacy resize fails closed without using sync resize", async () => {
  const native = makeNativeRenderer();
  delete native.resizeAsync;
  const renderer = new GsplatWebRenderer(native);

  await assert.rejects(
    renderer.resize(800, 600),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "resize");
      assert.equal(error.error_code, "unsupported");
      assert.equal(error.scene_published, true);
      return true;
    },
  );
  assert.deepEqual(native.calls, []);
  assert.deepEqual(renderer.surfaceSize(), { width: 640, height: 480 });
});

test("GsplatWebRenderer resize preserves published-scene semantics on capacity failure", async () => {
  const native = makeNativeRenderer({
    async resizeAsync() {
      throw new Error(
        "resident resource projected axes requires 160000000 bytes " +
        "but the effective binding limit is 134217728 bytes",
      );
    },
  });
  const renderer = new GsplatWebRenderer(native);

  await assert.rejects(
    renderer.resize(1920, 1080),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "resize");
      assert.equal(error.error_code, "capacity_exceeded");
      assert.equal(error.scene_published, true);
      assert.deepEqual(error.resource, {
        kind: "projected axes",
        required_bytes: 160000000,
        limit_bytes: 134217728,
      });
      return true;
    },
  );
  assert.deepEqual(renderer.surfaceSize(), { width: 640, height: 480 });
});

test("GsplatWebRenderer restages nested runtime failures as resize failures", async () => {
  const nested = new GsplatWebError(
    new Error("unexpected runtime failure"),
    "renderer_create",
    { scenePublished: true },
  );
  const native = makeNativeRenderer({
    async resizeAsync() {
      throw nested;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  await assert.rejects(
    renderer.resize(800, 600),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "resize");
      assert.equal(error.error_code, "internal");
      assert.equal(error.scene_published, true);
      assert.equal(error.cause, nested);
      return true;
    },
  );
});

test("GsplatWebRenderer exposes transactional geometry-path switching", async () => {
  const native = makeNativeRenderer();
  const renderer = new GsplatWebRenderer(native);

  await renderer.setGeometryPathAsync("direct");

  assert.deepEqual(native.calls, [["setGeometryPathAsync", 0]]);
});

test("GsplatWebRenderer publishes a GPU producer transactionally and blocks frames", async () => {
  let releaseProducer;
  const producerGate = new Promise((resolve) => {
    releaseProducer = resolve;
  });
  let published = "post-sort";
  const native = makeNativeRenderer({
    async setGpuOrderProducerAsync(producer) {
      native.calls.push(["setGpuOrderProducerAsync:start", producer]);
      await producerGate;
      published = producer === 1 ? "preproject" : "post-sort";
      native.calls.push(["setGpuOrderProducerAsync:complete", producer]);
    },
    gpuOrderProducer() {
      return published;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const switching = renderer.setGpuOrderProducerAsync("preproject");
  await new Promise((resolve) => setImmediate(resolve));
  assert.throws(
    () => renderer.renderFrame(),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "gpu_order_producer");
      assert.equal(error.scene_published, true);
      return true;
    },
  );
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), false);

  releaseProducer();
  await switching;
  renderer.renderFrame();
  assert.deepEqual(native.calls.slice(0, 2), [
    ["setGpuOrderProducerAsync:start", 1],
    ["setGpuOrderProducerAsync:complete", 1],
  ]);
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), true);
});

test("GsplatWebRenderer async geometry switch fails closed on a legacy module", async () => {
  const native = makeNativeRenderer();
  delete native.setGeometryPathAsync;
  const renderer = new GsplatWebRenderer(native);

  await assert.rejects(
    renderer.setGeometryPathAsync("direct"),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "geometry_path");
      assert.equal(error.error_code, "unsupported");
      assert.equal(error.scene_published, true);
      return true;
    },
  );
  assert.equal(native.calls.some((call) => call[0] === "setGeometryPath"), false);
});

test("GsplatWebRenderer geometry failure preserves the published renderer", async () => {
  const native = makeNativeRenderer({
    async setGeometryPathAsync() {
      throw new Error(
        "resident resource packed storage requires 160000000 bytes " +
        "but the effective binding limit is 134217728 bytes",
      );
    },
  });
  const renderer = new GsplatWebRenderer(native);

  await assert.rejects(
    renderer.setGeometryPathAsync("direct"),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "geometry_path");
      assert.equal(error.error_code, "capacity_exceeded");
      assert.equal(error.scene_published, true);
      assert.deepEqual(error.resource, {
        kind: "packed storage",
        required_bytes: 160000000,
        limit_bytes: 134217728,
      });
      return true;
    },
  );

  renderer.renderFrame();
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), true);
});

test("GsplatWebRenderer keeps legacy synchronous geometry switching fail closed", () => {
  let activePath = 1;
  const native = makeNativeRenderer({
    setGeometryPath(path) {
      native.calls.push(["setGeometryPath", path]);
      if (path !== activePath) {
        throw new Error("changed-path synchronous geometry switching is unsupported");
      }
      activePath = path;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  renderer.setGeometryPath("packed");
  renderer.setGeometryPath("packed");
  assert.throws(
    () => renderer.setGeometryPath("direct"),
    /synchronous geometry switching is unsupported/,
  );
  assert.equal(activePath, 1);
  assert.deepEqual(native.calls, [
    ["setGeometryPath", 1],
    ["setGeometryPath", 1],
    ["setGeometryPath", 0],
  ]);
});

test("GsplatWebRenderer serializes concurrent async resize calls", async () => {
  let surface = { width: 640, height: 480 };
  let releaseFirst;
  const firstGate = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  let active = 0;
  let maxActive = 0;
  const native = makeNativeRenderer({
    surfaceSize() {
      return surface;
    },
    async resizeAsync(width, height) {
      active += 1;
      maxActive = Math.max(maxActive, active);
      native.calls.push(["resizeAsync:start", width, height]);
      if (width === 800) await firstGate;
      surface = { width, height };
      native.calls.push(["resizeAsync:complete", width, height]);
      active -= 1;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const first = renderer.resize(800, 600);
  const second = renderer.resize(1024, 768);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(native.calls, [["resizeAsync:start", 800, 600]]);

  releaseFirst();
  await Promise.all([first, second]);
  assert.equal(maxActive, 1);
  assert.deepEqual(native.calls, [
    ["resizeAsync:start", 800, 600],
    ["resizeAsync:complete", 800, 600],
    ["resizeAsync:start", 1024, 768],
    ["resizeAsync:complete", 1024, 768],
  ]);
  assert.deepEqual(renderer.surfaceSize(), { width: 1024, height: 768 });
});

test("GsplatWebRenderer serializes resize and geometry on one mutation queue", async () => {
  let releaseResize;
  const resizeGate = new Promise((resolve) => {
    releaseResize = resolve;
  });
  let surface = { width: 640, height: 480 };
  let active = 0;
  let maxActive = 0;
  const native = makeNativeRenderer({
    surfaceSize() {
      return surface;
    },
    async resizeAsync(width, height) {
      active += 1;
      maxActive = Math.max(maxActive, active);
      native.calls.push(["resizeAsync:start", width, height]);
      await resizeGate;
      surface = { width, height };
      native.calls.push(["resizeAsync:complete", width, height]);
      active -= 1;
    },
    async setGeometryPathAsync(path) {
      active += 1;
      maxActive = Math.max(maxActive, active);
      native.calls.push(["setGeometryPathAsync", path]);
      active -= 1;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const resizing = renderer.resize(800, 600);
  const switching = renderer.setGeometryPathAsync("direct");
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(native.calls, [["resizeAsync:start", 800, 600]]);

  releaseResize();
  await Promise.all([resizing, switching]);
  assert.equal(maxActive, 1);
  assert.deepEqual(native.calls, [
    ["resizeAsync:start", 800, 600],
    ["resizeAsync:complete", 800, 600],
    ["setGeometryPathAsync", 0],
  ]);
});

test("GsplatWebRenderer mutation queue continues after geometry failure", async () => {
  let surface = { width: 640, height: 480 };
  const native = makeNativeRenderer({
    surfaceSize() {
      return surface;
    },
    async setGeometryPathAsync() {
      throw new Error("geometry candidate validation failed");
    },
    async resizeAsync(width, height) {
      surface = { width, height };
      native.calls.push(["resizeAsync", width, height]);
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const switching = renderer.setGeometryPathAsync("direct");
  const resizing = renderer.resize(800, 600);
  await assert.rejects(switching, (error) => error.stage === "geometry_path");
  await resizing;

  assert.deepEqual(renderer.surfaceSize(), { width: 800, height: 600 });
  assert.deepEqual(native.calls, [["resizeAsync", 800, 600]]);
});

test("GsplatWebRenderer blocks frame submission while resize is pending", async () => {
  let releaseResize;
  const resizeGate = new Promise((resolve) => {
    releaseResize = resolve;
  });
  let surface = { width: 640, height: 480 };
  const native = makeNativeRenderer({
    surfaceSize() {
      return surface;
    },
    async resizeAsync(width, height) {
      await resizeGate;
      surface = { width, height };
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const resizing = renderer.resize(800, 600);
  await new Promise((resolve) => setImmediate(resolve));
  assert.throws(
    () => renderer.renderFrame(),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "resize");
      assert.equal(error.scene_published, true);
      return true;
    },
  );
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), false);

  releaseResize();
  await resizing;
  renderer.renderFrame();
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), true);
});

test("GsplatWebRenderer blocks frame submission while geometry mutation is pending", async () => {
  let releaseGeometry;
  const geometryGate = new Promise((resolve) => {
    releaseGeometry = resolve;
  });
  const native = makeNativeRenderer({
    async setGeometryPathAsync(path) {
      native.calls.push(["setGeometryPathAsync", path]);
      await geometryGate;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const switching = renderer.setGeometryPathAsync("direct");
  await new Promise((resolve) => setImmediate(resolve));
  assert.throws(
    () => renderer.renderFrame(),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "geometry_path");
      assert.equal(error.error_code, "geometry_path_failed");
      assert.equal(error.scene_published, true);
      return true;
    },
  );
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), false);

  releaseGeometry();
  await switching;
  renderer.renderFrame();
  assert.equal(native.calls.some((call) => call[0] === "renderFrame"), true);
});

test("GsplatWebRenderer defers native free until an in-flight resize settles", async () => {
  let releaseResize;
  const resizeGate = new Promise((resolve) => {
    releaseResize = resolve;
  });
  let surface = { width: 640, height: 480 };
  const native = makeNativeRenderer({
    surfaceSize() {
      return surface;
    },
    async resizeAsync(width, height) {
      await resizeGate;
      surface = { width, height };
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const resizing = renderer.resize(800, 600);
  await new Promise((resolve) => setImmediate(resolve));
  renderer.free();
  assert.equal(renderer.isDisposed, true);
  assert.equal(native.calls.some((call) => call[0] === "free"), false);

  releaseResize();
  await resizing;
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(native.calls.filter((call) => call[0] === "free").length, 1);
});

test("GsplatWebRenderer defers native free until geometry mutation settles", async () => {
  let releaseGeometry;
  const geometryGate = new Promise((resolve) => {
    releaseGeometry = resolve;
  });
  const native = makeNativeRenderer({
    async setGeometryPathAsync() {
      await geometryGate;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const switching = renderer.setGeometryPathAsync("direct");
  await new Promise((resolve) => setImmediate(resolve));
  renderer.dispose();
  assert.equal(renderer.isDisposed, true);
  assert.equal(native.calls.some((call) => call[0] === "free"), false);

  releaseGeometry();
  await switching;
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(native.calls.filter((call) => call[0] === "free").length, 1);
});

test("GsplatWebRenderer defers native free until the complete mutation queue settles", async () => {
  let releaseGeometry;
  const geometryGate = new Promise((resolve) => {
    releaseGeometry = resolve;
  });
  const native = makeNativeRenderer({
    async setGeometryPathAsync() {
      await geometryGate;
    },
  });
  const renderer = new GsplatWebRenderer(native);

  const switching = renderer.setGeometryPathAsync("direct");
  const queuedResize = renderer.resize(800, 600);
  await new Promise((resolve) => setImmediate(resolve));
  renderer.dispose();
  assert.equal(native.calls.some((call) => call[0] === "free"), false);

  releaseGeometry();
  const [switchResult, resizeResult] = await Promise.allSettled([switching, queuedResize]);
  assert.equal(switchResult.status, "fulfilled");
  assert.equal(resizeResult.status, "rejected");
  assert.equal(resizeResult.reason.stage, "resize");
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(native.calls.some((call) => call[0] === "resizeAsync"), false);
  assert.equal(native.calls.filter((call) => call[0] === "free").length, 1);
});

test("GsplatWebRenderer projected policy setter is fail closed", () => {
  let publishedPolicy = "adaptive";
  const native = makeNativeRenderer();
  native.setProjectedPolicy = (policy) => {
    native.calls.push(["setProjectedPolicy", policy]);
    if (policy === 1) {
      throw new Error("projected contributor compaction is unsupported");
    }
    publishedPolicy = policy === 0 ? "candidate" : "adaptive";
  };
  const renderer = new GsplatWebRenderer(native);

  renderer.setProjectedPolicy("candidate");
  assert.equal(publishedPolicy, "candidate");
  assert.throws(
    () => renderer.setProjectedPolicy("compact"),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "projected_policy");
      assert.equal(error.error_code, "unsupported");
      assert.equal(error.scene_published, true);
      return true;
    },
  );
  assert.equal(publishedPolicy, "candidate");
  assert.deepEqual(native.calls, [
    ["setProjectedPolicy", 0],
    ["setProjectedPolicy", 1],
  ]);
});

test("GsplatWebRenderer rejects invalid command arguments", () => {
  const renderer = new GsplatWebRenderer(makeNativeRenderer());

  assert.throws(() => renderer.resize(0, 480), RangeError);
  assert.throws(() => renderer.orbit(Number.NaN, 0), TypeError);
  assert.throws(() => renderer.zoom(0), RangeError);
  assert.throws(() => renderer.pan(0, Number.POSITIVE_INFINITY), TypeError);
  assert.throws(() => renderer.setSortInterval(1.5), RangeError);
  assert.throws(() => renderer.setGeometryPath("unknown"), TypeError);
  assert.throws(() => renderer.setGeometryPathAsync("unknown"), TypeError);
  assert.throws(() => renderer.setOrderBackend("unknown"), TypeError);
  assert.throws(() => renderer.setProjectedPolicy("unknown"), TypeError);
});

test("createGsplatRenderer validates inputs before creating native renderer", async () => {
  const module = {
    async createRenderer() {
      throw new Error("should not create native renderer");
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: null,
      plyBytes: new Uint8Array(),
      module,
    }),
    TypeError,
  );
  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 640, height: 480 },
      plyBytes: new Uint8Array(),
      width: 0,
      height: 480,
      module,
    }),
    RangeError,
  );
  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 640, height: 480 },
      plyBytes: [1, 2, 3],
      module,
    }),
    TypeError,
  );
  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 640, height: 480 },
      plyBytes: new Uint8Array(),
      sortInterval: 0,
      module,
    }),
    RangeError,
  );
  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 640, height: 480 },
      plyBytes: new Uint8Array(),
      orderBackend: "unknown",
      module,
    }),
    TypeError,
  );
  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 640, height: 480 },
      plyBytes: new Uint8Array(),
      projectedPolicy: "unknown",
      module,
    }),
    TypeError,
  );
});

test("createGsplatRenderer normalizes bytes and applies render options", async () => {
  const native = makeNativeRenderer();
  native.setOrderBackend = (backend) => native.calls.push(["setOrderBackend", backend]);
  native.setProjectedPolicy = (policy) => native.calls.push(["setProjectedPolicy", policy]);
  let captured;
  const module = {
    async createRendererWithGeometryPath(canvas, plyBytes, width, height, geometryPath) {
      captured = { canvas, plyBytes, width, height, geometryPath };
      return native;
    },
  };
  const canvas = { width: 320, height: 240 };
  const buffer = new Uint8Array([1, 2, 3]).buffer;

  const renderer = await createGsplatRenderer({
    canvas,
    plyBytes: buffer,
    sortInterval: 3,
    geometryPath: "packed",
    orderBackend: "gpu",
    projectedPolicy: "compact",
    module,
  });

  assert.ok(renderer instanceof GsplatWebRenderer);
  assert.equal(captured.canvas, canvas);
  assert.deepEqual(Array.from(captured.plyBytes), [1, 2, 3]);
  assert.equal(captured.width, 320);
  assert.equal(captured.height, 240);
  assert.equal(captured.geometryPath, 1);
  assert.deepEqual(native.calls[0], ["setSortInterval", 3]);
  assert.deepEqual(native.calls[1], ["setProjectedPolicy", 1]);
  assert.deepEqual(native.calls[2], ["setOrderBackend", 1]);
  assert.equal(native.calls.length, 3);
});

test("createGsplatRenderer publishes an explicit GPU producer only after its transaction", async () => {
  const native = makeNativeRenderer();
  let releaseProducer;
  const producerPrepared = new Promise((resolve) => {
    releaseProducer = resolve;
  });
  native.setProjectedPolicy = (policy) => native.calls.push(["setProjectedPolicy", policy]);
  native.setGpuOrderProducerAsync = async (producer) => {
    native.calls.push(["setGpuOrderProducerAsync:start", producer]);
    await producerPrepared;
    native.calls.push(["setGpuOrderProducerAsync:complete", producer]);
  };
  native.gpuOrderProducer = () => "preproject";
  native.prepareGpuOrder = async () => native.calls.push(["prepareGpuOrder"]);
  native.setOrderBackend = (backend) => native.calls.push(["setOrderBackend", backend]);
  const module = {
    async createRendererWithGeometryPath() {
      return native;
    },
  };

  let resolved = false;
  const creating = createGsplatRenderer({
    canvas: { width: 320, height: 240 },
    plyBytes: new Uint8Array([1, 2, 3]),
    geometryPath: "packed",
    orderBackend: "gpu",
    projectedPolicy: "compact",
    gpuOrderProducer: "preproject",
    module,
  }).then((renderer) => {
    resolved = true;
    return renderer;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(resolved, false);
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["setProjectedPolicy", 1],
    ["setGpuOrderProducerAsync:start", 1],
  ]);

  releaseProducer();
  const renderer = await creating;
  assert.ok(renderer instanceof GsplatWebRenderer);
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["setProjectedPolicy", 1],
    ["setGpuOrderProducerAsync:start", 1],
    ["setGpuOrderProducerAsync:complete", 1],
    ["prepareGpuOrder"],
    ["setOrderBackend", 1],
  ]);
});

test("createGsplatRenderer rejects GPU producer diagnostics outside the exact context", async () => {
  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      geometryPath: "packed",
      orderBackend: "gpu",
      projectedPolicy: "adaptive",
      gpuOrderProducer: "preproject",
      module: {},
    }),
    /require geometryPath=packed and projectedPolicy=compact/,
  );
});

test("createGsplatRenderer fails closed when an explicit projected policy is unavailable", async () => {
  const native = makeNativeRenderer();
  const module = {
    async createRendererWithGeometryPath() {
      return native;
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      orderBackend: "cpu",
      projectedPolicy: "compact",
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "renderer_configure");
      assert.equal(error.error_code, "unsupported");
      assert.equal(error.scene_published, false);
      return true;
    },
  );
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["free"],
  ]);
});

test("createGsplatRenderer awaits transactional GPU preparation before publishing", async () => {
  const native = makeNativeRenderer();
  let releasePreparation;
  const preparation = new Promise((resolve) => {
    releasePreparation = resolve;
  });
  native.prepareGpuOrder = async () => {
    native.calls.push(["prepareGpuOrder:start"]);
    await preparation;
    native.calls.push(["prepareGpuOrder:complete"]);
  };
  native.setOrderBackend = (backend) => native.calls.push(["setOrderBackend", backend]);
  const module = {
    async createRendererWithGeometryPath() {
      return native;
    },
  };

  let resolved = false;
  const creating = createGsplatRenderer({
    canvas: { width: 320, height: 240 },
    plyBytes: new Uint8Array([1, 2, 3]),
    orderBackend: "gpu",
    module,
  }).then((renderer) => {
    resolved = true;
    return renderer;
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(resolved, false);
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["prepareGpuOrder:start"],
  ]);

  releasePreparation();
  const renderer = await creating;
  assert.ok(renderer instanceof GsplatWebRenderer);
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["prepareGpuOrder:start"],
    ["prepareGpuOrder:complete"],
    ["setOrderBackend", 1],
  ]);
});

test("createGsplatRenderer frees native state and structures GPU preparation failure", async () => {
  const native = makeNativeRenderer({
    async prepareGpuOrder() {
      throw new Error(
        "resident resource projected axes requires 160000000 bytes " +
        "but the effective binding limit is 134217728 bytes",
      );
    },
  });
  const module = {
    async createRendererWithGeometryPath() {
      return native;
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      orderBackend: "adaptive",
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "gpu_order_prepare");
      assert.equal(error.error_code, "capacity_exceeded");
      assert.equal(error.scene_published, false);
      assert.deepEqual(error.resource, {
        kind: "projected axes",
        required_bytes: 160000000,
        limit_bytes: 134217728,
      });
      return true;
    },
  );
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["free"],
  ]);
});

test("createGsplatRenderer fails closed when transactional GPU preparation is unavailable", async () => {
  const native = makeNativeRenderer();
  delete native.prepareGpuOrder;
  native.setOrderBackend = (backend) => native.calls.push(["setOrderBackend", backend]);
  const module = {
    async createRendererWithGeometryPath() {
      return native;
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      orderBackend: "gpu",
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "gpu_order_prepare");
      assert.equal(error.error_code, "unsupported");
      assert.equal(error.scene_published, false);
      return true;
    },
  );
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["free"],
  ]);
});

test("createGsplatRenderer frees native state when synchronous configuration fails", async () => {
  const nativeFailure = new Error("surface configure failed: validation error");
  const native = makeNativeRenderer({
    setSortInterval() {
      throw nativeFailure;
    },
  });
  const module = {
    async createRendererWithGeometryPath() {
      return native;
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "renderer_configure");
      assert.equal(error.error_code, "renderer_configuration_failed");
      assert.equal(error.error_message, nativeFailure.message);
      assert.equal(error.scene_published, false);
      assert.equal(error.cause, nativeFailure);
      return true;
    },
  );
  assert.deepEqual(native.calls, [["free"]]);
});

test("createGsplatRenderer structures constructor capacity failures", async () => {
  const module = {
    async createRendererWithGeometryPath() {
      throw new Error(
        "PLY resource limit exceeded for decoded scene bytes: " +
        "requested 2_200_000_000, limit 2_147_483_647",
      );
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "renderer_create");
      assert.equal(error.error_code, "capacity_exceeded");
      assert.equal(error.scene_published, false);
      assert.deepEqual(error.resource, {
        kind: "decoded scene bytes",
        required_bytes: 2200000000,
        limit_bytes: 2147483647,
      });
      return true;
    },
  );
});

test("createGsplatRenderer defaults to exact packed geometry and adaptive ordering", async () => {
  const native = makeNativeRenderer();
  native.setOrderBackend = (backend) => native.calls.push(["setOrderBackend", backend]);
  let captured;
  const module = {
    async createRendererWithGeometryPath(canvas, plyBytes, width, height, geometryPath) {
      captured = { canvas, plyBytes, width, height, geometryPath };
      return native;
    },
  };
  const canvas = { width: 320, height: 240 };

  await createGsplatRenderer({
    canvas,
    plyBytes: new Uint8Array([1, 2, 3]),
    module,
  });

  assert.equal(captured.canvas, canvas);
  assert.equal(captured.geometryPath, 1);
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["setOrderBackend", 2],
  ]);
});

test("createGsplatRenderer keeps direct available as an explicit f32 oracle", async () => {
  const native = makeNativeRenderer();
  let captured;
  const module = {
    async createRenderer() {
      throw new Error("the compatibility constructor must not select an explicit path");
    },
    async createRendererWithGeometryPath(canvas, plyBytes, width, height, geometryPath) {
      captured = { canvas, plyBytes, width, height, geometryPath };
      return native;
    },
  };
  const canvas = { width: 320, height: 240 };

  await createGsplatRenderer({
    canvas,
    plyBytes: new Uint8Array([1, 2, 3]),
    geometryPath: "direct",
    orderBackend: "cpu",
    module,
  });

  assert.equal(captured.canvas, canvas);
  assert.deepEqual(Array.from(captured.plyBytes), [1, 2, 3]);
  assert.equal(captured.width, 320);
  assert.equal(captured.height, 240);
  assert.equal(captured.geometryPath, 0);
  assert.deepEqual(native.calls, [["setSortInterval", 1]]);
});

test("createGsplatRenderer fails closed when explicit geometry construction is unavailable", async () => {
  let compatibilityConstructorCalled = false;
  const module = {
    async createRenderer() {
      compatibilityConstructorCalled = true;
      return makeNativeRenderer();
    },
  };

  await assert.rejects(
    createGsplatRenderer({
      canvas: { width: 320, height: 240 },
      plyBytes: new Uint8Array([1, 2, 3]),
      geometryPath: "direct",
      module,
    }),
    /does not support constructor-time geometry selection/,
  );
  assert.equal(compatibilityConstructorCalled, false);
});

test("createGsplatRendererFromUrl streams default Packed input without arrayBuffer", async () => {
  const native = makeNativeRenderer();
  native.setOrderBackend = (backend) => native.calls.push(["setOrderBackend", backend]);
  const pushed = [];
  let released = false;
  let arrayBufferCalled = false;
  const chunks = [new Uint8Array([1, 2]), new Uint8Array([3, 4, 5])];
  const module = {
    createPackedPlyStream(canvas, width, height) {
      assert.equal(canvas.width, 320);
      assert.equal(width, 320);
      assert.equal(height, 240);
      return {
        pushChunk(bytes) {
          pushed.push(Array.from(bytes));
        },
        async finish() {
          return native;
        },
        free() {
          throw new Error("successful stream must not be freed by failure cleanup");
        },
      };
    },
  };
  const previousFetch = globalThis.fetch;
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    statusText: "OK",
    body: {
      getReader() {
        let index = 0;
        return {
          async read() {
            return index < chunks.length
              ? { done: false, value: chunks[index++] }
              : { done: true, value: undefined };
          },
          releaseLock() {
            released = true;
          },
        };
      },
    },
    async arrayBuffer() {
      arrayBufferCalled = true;
      return new ArrayBuffer(0);
    },
  });

  try {
    const renderer = await createGsplatRendererFromUrl({
      canvas: { width: 320, height: 240 },
      url: "/large-scene.ply",
      module,
    });
    assert.ok(renderer instanceof GsplatWebRenderer);
    assert.deepEqual(pushed, [[1, 2], [3, 4, 5]]);
    assert.equal(arrayBufferCalled, false);
    assert.equal(released, true);
    assert.deepEqual(native.calls, [
      ["setSortInterval", 1],
      ["setOrderBackend", 2],
    ]);
  } finally {
    globalThis.fetch = previousFetch;
  }
});

test("failed Packed streaming cancels transport and frees native state", async () => {
  const failure = new Error("resident SH R plane allocation failed");
  let cancelledWith = null;
  let released = false;
  let freed = false;
  const module = {
    createPackedPlyStream() {
      return {
        pushChunk() {
          throw failure;
        },
        free() {
          freed = true;
        },
      };
    },
  };
  const reader = {
    async read() {
      return { done: false, value: new Uint8Array([1, 2, 3]) };
    },
    async cancel(reason) {
      cancelledWith = reason;
    },
    releaseLock() {
      released = true;
    },
  };

  await assert.rejects(
    createGsplatRendererFromStream({
      canvas: { width: 320, height: 240 },
      stream: { getReader: () => reader },
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "stream_decode");
      assert.equal(error.error_code, "out_of_memory");
      assert.equal(error.error_message, failure.message);
      assert.equal(error.scene_published, false);
      assert.equal(error.cause, failure);
      return true;
    },
  );

  assert.equal(cancelledWith, "gsplat exact Packed scene construction failed");
  assert.equal(freed, true);
  assert.equal(released, true);
});

test("Packed streaming configuration failure frees renderer and stream before publication", async () => {
  const native = makeNativeRenderer({
    setOrderBackend() {
      throw new Error("GPU ordering is unsupported on this adapter");
    },
  });
  let streamFreed = false;
  let cancelled = false;
  let released = false;
  const module = {
    createPackedPlyStream() {
      return {
        pushChunk() {},
        async finish() {
          return native;
        },
        free() {
          streamFreed = true;
        },
      };
    },
  };
  let readCount = 0;
  const reader = {
    async read() {
      readCount += 1;
      return readCount === 1
        ? { done: false, value: new Uint8Array([1, 2, 3]) }
        : { done: true, value: undefined };
    },
    async cancel() {
      cancelled = true;
    },
    releaseLock() {
      released = true;
    },
  };

  await assert.rejects(
    createGsplatRendererFromStream({
      canvas: { width: 320, height: 240 },
      stream: { getReader: () => reader },
      orderBackend: "gpu",
      module,
    }),
    (error) => {
      assert.ok(error instanceof GsplatWebError);
      assert.equal(error.stage, "renderer_configure");
      assert.equal(error.error_code, "unsupported");
      assert.equal(error.scene_published, false);
      return true;
    },
  );
  assert.deepEqual(native.calls, [
    ["setSortInterval", 1],
    ["free"],
  ]);
  assert.equal(streamFreed, true);
  assert.equal(cancelled, true);
  assert.equal(released, true);
});

test("initGsplatWeb stores provided module for version lookup", async () => {
  let initializedWith;
  const module = {
    async default(init) {
      initializedWith = init;
    },
    api_version_major() {
      return 0;
    },
    api_version_minor() {
      return 1;
    },
  };

  const resolved = await initGsplatWeb({ module, wasmUrl: "fixture.wasm" });

  assert.equal(resolved, module);
  assert.deepEqual(initializedWith, { module_or_path: "fixture.wasm" });
  assert.deepEqual(getGsplatApiVersion(), { major: 0, minor: 1 });
});
