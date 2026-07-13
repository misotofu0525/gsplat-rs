import assert from "node:assert/strict";
import test from "node:test";

import {
  GsplatWebRenderer,
  createGsplatRenderer,
  getGsplatApiVersion,
  initGsplatWeb,
} from "../src/index.js";

function makeNativeRenderer(overrides = {}) {
  const calls = [];
  return {
    calls,
    resize(width, height) {
      calls.push(["resize", width, height]);
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
      };
    },
    sceneSummary() {
      return {
        gaussians: "3",
        shDegree: "1",
        hasShRest: 1,
      };
    },
    surfaceSize() {
      return {
        width: "640",
        height: 480,
      };
    },
    free() {
      calls.push(["free"]);
    },
    ...overrides,
  };
}

test("GsplatWebRenderer validates native renderer handle", () => {
  assert.throws(() => new GsplatWebRenderer(null), TypeError);
});

test("GsplatWebRenderer forwards commands and normalizes return values", () => {
  const native = makeNativeRenderer();
  const renderer = new GsplatWebRenderer(native);

  renderer.resize(640, 480);
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

  assert.deepEqual(renderer.renderFrame(), {
    frameMs: 1.25,
    preprocessMs: 0,
    sortMs: 0.5,
    rasterMs: 0,
    cpuGeometryMs: 0.25,
    renderSubmitMs: 0.75,
    frameWallMs: 1.5,
    visibleCount: 2,
    drawnCount: 1,
    refreshSort: true,
    surfaceWidth: 640,
    surfaceHeight: 480,
  });
  assert.deepEqual(renderer.sceneSummary(), {
    gaussians: 3,
    shDegree: 1,
    hasShRest: true,
  });
  assert.deepEqual(renderer.surfaceSize(), {
    width: 640,
    height: 480,
  });
  const cameraReceipt = renderer.cameraReceipt();
  assert.deepEqual(cameraReceipt.position, [1, 2, 3]);
  assert.deepEqual(cameraReceipt.rotationXyzw, [0, 0, 0, 1]);
  assert.ok(Math.abs(cameraReceipt.intrinsics.verticalFovRadians - Math.PI / 3) < 1e-6);
  renderer.free();

  assert.deepEqual(native.calls.slice(0, 10), [
    ["resize", 640, 480],
    ["resetCamera"],
    ["setCamera", 1, 2, 3, 0, 0, 0, 1, 1, Math.fround(0.1), 100],
    ["orbit", 0.1, -0.2],
    ["zoom", 1.1],
    ["pan", 0.05, -0.05],
    ["setSortInterval", 2],
    ["setGeometryPath", 2],
    ["renderFrame"],
    ["free"],
  ]);
  assert.equal(renderer.isDisposed, true);
  assert.throws(() => renderer.renderFrame(), /disposed/);
});

test("GsplatWebRenderer rejects invalid command arguments", () => {
  const renderer = new GsplatWebRenderer(makeNativeRenderer());

  assert.throws(() => renderer.resize(0, 480), RangeError);
  assert.throws(() => renderer.orbit(Number.NaN, 0), TypeError);
  assert.throws(() => renderer.zoom(0), RangeError);
  assert.throws(() => renderer.pan(0, Number.POSITIVE_INFINITY), TypeError);
  assert.throws(() => renderer.setSortInterval(1.5), RangeError);
  assert.throws(() => renderer.setGeometryPath("unknown"), TypeError);
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
});

test("createGsplatRenderer normalizes bytes and applies render options", async () => {
  const native = makeNativeRenderer();
  let captured;
  const module = {
    async createRenderer(canvas, plyBytes, width, height) {
      captured = { canvas, plyBytes, width, height };
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
    module,
  });

  assert.ok(renderer instanceof GsplatWebRenderer);
  assert.equal(captured.canvas, canvas);
  assert.deepEqual(Array.from(captured.plyBytes), [1, 2, 3]);
  assert.equal(captured.width, 320);
  assert.equal(captured.height, 240);
  assert.deepEqual(native.calls[0], ["setSortInterval", 3]);
  assert.deepEqual(native.calls[1], ["setGeometryPath", 1]);
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
