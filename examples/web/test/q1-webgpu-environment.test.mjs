import assert from "node:assert/strict";
import test from "node:test";

import {
  Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS,
  canonicalWebGpuAdapterEnvironment,
} from "../../../tests/perf/q1-webgpu-environment.mjs";

const supportedLimits = Object.fromEntries(
  Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS.map((name, index) => [name, index + 1]),
);

test("Q1 endpoints normalize the same selected adapter supported limits identically", () => {
  const gsplat = canonicalWebGpuAdapterEnvironment({
    backend: "browser_webgpu",
    selectionClass: "high_performance",
    supportedLimits,
  });
  const playcanvas = canonicalWebGpuAdapterEnvironment({
    backend: "browser_webgpu",
    selectionClass: "high_performance",
    supportedLimits: { ...supportedLimits, endpointOnlyLimit: 999 },
  });
  assert.deepEqual(gsplat, playcanvas);
});

test("Q1 canonical adapter identity fails closed on selected-adapter drift", () => {
  const drifted = { ...supportedLimits, maxBufferSize: supportedLimits.maxBufferSize + 1 };
  assert.notEqual(
    canonicalWebGpuAdapterEnvironment({
      backend: "browser_webgpu",
      selectionClass: "high_performance",
      supportedLimits,
    }).adapter_limits_sha256,
    canonicalWebGpuAdapterEnvironment({
      backend: "browser_webgpu",
      selectionClass: "high_performance",
      supportedLimits: drifted,
    }).adapter_limits_sha256,
  );
  assert.throws(() => canonicalWebGpuAdapterEnvironment({
    backend: "browser_webgpu",
    selectionClass: "high_performance",
    supportedLimits: { ...supportedLimits, maxBufferSize: undefined },
  }), /maxBufferSize/);
});
