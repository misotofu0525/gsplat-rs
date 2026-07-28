import assert from "node:assert/strict";
import test from "node:test";

import {
  captureQ1HostStartReceipt,
  createQ1HostStartGate,
  validateQ1HostStartReceipt,
} from "../../../tests/perf/q1-host-start-gate.mjs";

function fixture(overrides = {}) {
  const documentObject = {
    visibilityState: "visible",
    hasFocus: () => true,
    ...overrides.documentObject,
  };
  const windowObject = {
    innerWidth: 1920,
    innerHeight: 1080,
    devicePixelRatio: 1,
    visualViewport: { width: 1920, height: 1080 },
    ...overrides.windowObject,
  };
  const canvas = {
    width: 1920,
    height: 1080,
    getBoundingClientRect: () => ({ width: 1920, height: 1080 }),
    ...overrides.canvas,
  };
  return captureQ1HostStartReceipt({
    documentObject,
    windowObject,
    canvas,
    expectedWidth: 1920,
    expectedHeight: 1080,
  });
}

test("Q1 host gate starts exactly once from an armed focused surface", () => {
  let starts = 0;
  const gate = createQ1HostStartGate({ capture: fixture, start: () => { starts += 1; } });
  assert.equal(gate.arm().state, "armed");
  assert.equal(starts, 0);
  assert.equal(gate.start().document_has_focus, true);
  assert.equal(starts, 1);
  assert.equal(gate.status().state, "started");
  assert.throws(() => gate.start(), /cannot begin from started/);
  assert.equal(starts, 1);
});

test("Q1 host gate fails before warmup when focus or geometry is invalid", () => {
  for (const capture of [
    () => fixture({ documentObject: { hasFocus: () => false } }),
    () => fixture({ canvas: { width: 1280 } }),
  ]) {
    let starts = 0;
    const gate = createQ1HostStartGate({ capture, start: () => { starts += 1; } });
    gate.arm();
    assert.throws(() => gate.start(), /visible focused full-resolution/);
    assert.equal(starts, 0);
    assert.equal(gate.status().state, "failed");
  }
});

test("Q1 host receipt validator rejects a caller relabeling a failed receipt", () => {
  const receipt = fixture();
  assert.equal(validateQ1HostStartReceipt(receipt), receipt);
  assert.throws(
    () => validateQ1HostStartReceipt({ ...receipt, phase: "pre_measurement" }),
    /visible focused full-resolution/,
  );
});
