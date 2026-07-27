import assert from "node:assert/strict";
import test from "node:test";

import {
  Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS,
  gsplatQ1WebGpuEnvironmentFields,
} from "../../../tests/perf/q1-webgpu-environment.mjs";

const supportedLimits = Object.fromEntries(
  Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS.map((name, index) => [name, index + 1]),
);

function surfaceReceipt(adapterLimits = supportedLimits, deviceLimits = supportedLimits) {
  return {
    schema: "gsplat-renderer-surface-device/v1",
    provenance: "renderer_owned_surface_session",
    adapterSelectionClass: "high_performance",
    adapter: {
      identityStatus: "unavailable_wgpu28_web_backend",
      backend: "browser_webgpu",
      name: "",
      vendorId: 0,
      deviceId: 0,
      deviceType: "other",
      driver: "",
      driverInfo: "",
    },
    supportedAdapterLimits: adapterLimits,
    effectiveDeviceLimits: deviceLimits,
  };
}

test("Q1 gsplat receipt uses the accepted selected adapter/device schema", () => {
  const fields = gsplatQ1WebGpuEnvironmentFields(surfaceReceipt(
    { ...supportedLimits, endpointOnlyLimit: 999 },
    { ...supportedLimits, endpointDeviceLimit: 123 },
  ));
  assert.equal(fields.adapter, "browser_webgpu_renderer_actual_selected_adapter");
  assert.equal(
    fields.adapter_identity_status,
    "hardware_name_unavailable_cross_endpoint_wgpu28_browser_backend",
  );
  assert.deepEqual(fields.webgpu_device_environment_receipt.selected_adapter.info, {
    name: "",
    vendor_id: 0,
    device_id: 0,
    device_type: "Other",
    driver: "",
    driver_info: "",
    backend: "BrowserWebGpu",
  });
  assert.equal(
    fields.webgpu_device_environment_receipt.selected_adapter
      .supported_limits.endpointOnlyLimit,
    999,
  );
  assert.equal(
    fields.webgpu_device_environment_receipt.selected_device
      .effective_limits.endpointDeviceLimit,
    123,
  );
  assert.deepEqual(
    Object.keys(fields.webgpu_device_environment_receipt.canonical_adapter.supported_limits),
    Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS,
  );
});

test("Q1 canonical adapter identity fails closed on selected-adapter drift", () => {
  const drifted = { ...supportedLimits, maxBufferSize: supportedLimits.maxBufferSize + 1 };
  assert.notEqual(
    gsplatQ1WebGpuEnvironmentFields(surfaceReceipt(supportedLimits))
      .canonical_adapter_supported_limits_sha256,
    gsplatQ1WebGpuEnvironmentFields(surfaceReceipt(drifted))
      .canonical_adapter_supported_limits_sha256,
  );
  assert.throws(() => gsplatQ1WebGpuEnvironmentFields(surfaceReceipt({
    ...supportedLimits,
    maxBufferSize: undefined,
  })), /invalid limit/);
});

test("Q1 gsplat receipt rejects a separately selected or named adapter", () => {
  const receipt = surfaceReceipt();
  receipt.provenance = "navigator.gpu.requestAdapter";
  assert.throws(
    () => gsplatQ1WebGpuEnvironmentFields(receipt),
    /actual opaque BrowserWebGpu Surface adapter/,
  );
  const named = surfaceReceipt();
  named.adapter.name = "invented hardware";
  assert.throws(
    () => gsplatQ1WebGpuEnvironmentFields(named),
    /actual opaque BrowserWebGpu Surface adapter/,
  );
});
