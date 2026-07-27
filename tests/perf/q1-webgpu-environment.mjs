import { createHash } from "node:crypto";

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

/**
 * Canonical cross-endpoint identity for one actually selected WebGPU adapter.
 * GPUAdapterInfo is intentionally excluded because wgpu 28 cannot expose it
 * on the browser backend. Device limits are endpoint configuration and remain
 * in the endpoint receipt; only adapter-supported limits identify hardware.
 */
export function canonicalWebGpuAdapterEnvironment({
  backend,
  selectionClass,
  supportedLimits,
}) {
  if (backend !== "browser_webgpu" || selectionClass !== "high_performance"
      || !supportedLimits || typeof supportedLimits !== "object"
      || Array.isArray(supportedLimits)) {
    throw new TypeError("Q1 canonical adapter input is not an actual high-performance WebGPU adapter");
  }
  const subset = Object.fromEntries(Q1_CANONICAL_WEBGPU_SUPPORTED_LIMITS.map((name) => {
    const value = supportedLimits[name];
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new TypeError(`Q1 selected adapter lacks canonical supported limit ${name}`);
    }
    return [name, value];
  }));
  const limitsSha256 = sha256Json(subset);
  return Object.freeze({
    adapter: `browser_webgpu/high_performance/supported_limits:${limitsSha256}`,
    adapter_limits_sha256: limitsSha256,
    supported_limits: Object.freeze(subset),
  });
}
