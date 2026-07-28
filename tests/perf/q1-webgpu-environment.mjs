import { createHash } from "node:crypto";

export const Q1_WEBGPU_ENVIRONMENT_SCHEMA =
  "gsplat-q1-webgpu-selected-device-environment/v1";
export const Q1_CANONICAL_ADAPTER_SCHEMA = "gsplat-q1-webgpu-canonical-adapter/v1";
export const Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA =
  "wgpu-28-browser-webgpu-direct-supported-limits/v1";
export const Q1_ADAPTER_SELECTION_CLASS =
  "browser_webgpu_renderer_actual_selected_adapter";
export const Q1_ADAPTER_IDENTITY_STATUS =
  "hardware_name_unavailable_cross_endpoint_wgpu28_browser_backend";

// Exact GPUSupportedLimits fields that wgpu 28 reads directly from the
// selected BrowserWebGpu adapter. This is the same frozen cross-endpoint set
// enforced by q1_pair_admission; endpoint-only limits remain in the full
// selected-adapter receipt and never become a second hardware identity.
export const Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS = Object.freeze([
  "maxBindGroups",
  "maxBindingsPerBindGroup",
  "maxBufferSize",
  "maxColorAttachmentBytesPerSample",
  "maxColorAttachments",
  "maxComputeInvocationsPerWorkgroup",
  "maxComputeWorkgroupSizeX",
  "maxComputeWorkgroupSizeY",
  "maxComputeWorkgroupSizeZ",
  "maxComputeWorkgroupStorageSize",
  "maxComputeWorkgroupsPerDimension",
  "maxDynamicStorageBuffersPerPipelineLayout",
  "maxDynamicUniformBuffersPerPipelineLayout",
  "maxSampledTexturesPerShaderStage",
  "maxSamplersPerShaderStage",
  "maxStorageBufferBindingSize",
  "maxStorageBuffersPerShaderStage",
  "maxStorageTexturesPerShaderStage",
  "maxTextureArrayLayers",
  "maxTextureDimension1D",
  "maxTextureDimension2D",
  "maxTextureDimension3D",
  "maxUniformBufferBindingSize",
  "maxUniformBuffersPerShaderStage",
  "maxVertexAttributes",
  "maxVertexBufferArrayStride",
  "maxVertexBuffers",
  "minStorageBufferOffsetAlignment",
  "minUniformBufferOffsetAlignment",
]);

function sha256Json(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

function normalizedLimits(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`Q1 ${label} must be an object`);
  }
  // Use the language-independent JSON member order also used by the Python
  // admission contract. localeCompare is locale-sensitive and orders prefix
  // pairs such as Workgroups/WorkgroupStorage differently from code-point
  // order, producing a hash that the consumer cannot reproduce.
  const entries = Object.entries(value).sort(([left], [right]) =>
    left < right ? -1 : left > right ? 1 : 0);
  if (entries.length === 0 || entries.some(([name, limit]) =>
    !/^[a-z][A-Za-z0-9]*$/.test(name)
      || !Number.isSafeInteger(limit) || limit < 0)) {
    throw new TypeError(`Q1 ${label} contains an invalid limit`);
  }
  return Object.fromEntries(entries);
}

function canonicalSupportedLimits(supportedLimits) {
  return Object.fromEntries(Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS.map((name) => {
    const value = supportedLimits[name];
    if (!Number.isSafeInteger(value) || value <= 0) {
      throw new TypeError(`Q1 selected adapter lacks canonical supported limit ${name}`);
    }
    return [name, value];
  }));
}

/**
 * Materialize the accepted Q1 endpoint receipt from the adapter and device
 * already owned by the gsplat Surface session. This function never requests a
 * second adapter/device and intentionally does not invent a hardware name that
 * wgpu 28's browser backend cannot expose.
 */
export function gsplatQ1WebGpuEnvironmentFields(surfaceReceipt) {
  const adapter = surfaceReceipt?.adapter;
  if (surfaceReceipt?.schema !== "gsplat-renderer-surface-device/v1"
      || surfaceReceipt.provenance !== "renderer_owned_surface_session"
      || surfaceReceipt.adapterSelectionClass !== "high_performance"
      || adapter?.identityStatus !== "unavailable_wgpu28_web_backend"
      || adapter.backend !== "browser_webgpu"
      || adapter.name !== ""
      || adapter.vendorId !== 0
      || adapter.deviceId !== 0
      || adapter.deviceType !== "other"
      || adapter.driver !== ""
      || adapter.driverInfo !== "") {
    throw new TypeError("Q1 gsplat receipt is not the actual opaque BrowserWebGpu Surface adapter");
  }
  const adapterSupportedLimits = normalizedLimits(
    surfaceReceipt.supportedAdapterLimits,
    "selected adapter supported limits",
  );
  const deviceEffectiveLimits = normalizedLimits(
    surfaceReceipt.effectiveDeviceLimits,
    "selected device effective limits",
  );
  const canonicalLimits = canonicalSupportedLimits(adapterSupportedLimits);
  const receipt = {
    schema: Q1_WEBGPU_ENVIRONMENT_SCHEMA,
    endpoint: "gsplat_rs",
    selected_adapter: {
      provenance: "gsplat_surface_session.wgpu_adapter",
      info_status: "unavailable_wgpu28_browser_backend",
      info: {
        name: "",
        vendor_id: 0,
        device_id: 0,
        device_type: "Other",
        driver: "",
        driver_info: "",
        backend: "BrowserWebGpu",
      },
      supported_limits: adapterSupportedLimits,
    },
    selected_device: {
      provenance: "gsplat_surface_session.wgpu_device",
      effective_limits: deviceEffectiveLimits,
    },
    canonical_adapter: {
      schema: Q1_CANONICAL_ADAPTER_SCHEMA,
      selection_class: Q1_ADAPTER_SELECTION_CLASS,
      backend_class: "browser_webgpu",
      hardware_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
      supported_limits_schema: Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
      supported_limits: canonicalLimits,
    },
  };
  return Object.freeze({
    adapter: Q1_ADAPTER_SELECTION_CLASS,
    adapter_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
    canonical_adapter_supported_limits_sha256: sha256Json(canonicalLimits),
    adapter_supported_limits_sha256: sha256Json(adapterSupportedLimits),
    device_effective_limits_sha256: sha256Json(deviceEffectiveLimits),
    webgpu_device_environment_receipt: receipt,
  });
}
