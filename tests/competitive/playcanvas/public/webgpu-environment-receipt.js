export const Q1_WEBGPU_ENVIRONMENT_SCHEMA = 'gsplat-q1-webgpu-selected-device-environment/v1';
export const Q1_CANONICAL_ADAPTER_SCHEMA = 'gsplat-q1-webgpu-canonical-adapter/v1';
export const Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA =
  'wgpu-28-browser-webgpu-direct-supported-limits/v1';
export const Q1_ADAPTER_SELECTION_CLASS = 'browser_webgpu_renderer_actual_selected_adapter';
export const Q1_ADAPTER_IDENTITY_STATUS =
  'hardware_name_unavailable_cross_endpoint_wgpu28_browser_backend';

// These are exactly the GPUSupportedLimits fields that wgpu 28's
// BrowserWebGpu backend reads directly from the selected GPUAdapter. Fields
// that wgpu fills from its own defaults are deliberately excluded from the
// cross-endpoint adapter fingerprint.
export const Q1_CANONICAL_SUPPORTED_LIMIT_NAMES = Object.freeze([
  'maxBindGroups',
  'maxBindingsPerBindGroup',
  'maxBufferSize',
  'maxColorAttachmentBytesPerSample',
  'maxColorAttachments',
  'maxComputeInvocationsPerWorkgroup',
  'maxComputeWorkgroupSizeX',
  'maxComputeWorkgroupSizeY',
  'maxComputeWorkgroupSizeZ',
  'maxComputeWorkgroupStorageSize',
  'maxComputeWorkgroupsPerDimension',
  'maxDynamicStorageBuffersPerPipelineLayout',
  'maxDynamicUniformBuffersPerPipelineLayout',
  'maxSampledTexturesPerShaderStage',
  'maxSamplersPerShaderStage',
  'maxStorageBufferBindingSize',
  'maxStorageBuffersPerShaderStage',
  'maxStorageTexturesPerShaderStage',
  'maxTextureArrayLayers',
  'maxTextureDimension1D',
  'maxTextureDimension2D',
  'maxTextureDimension3D',
  'maxUniformBufferBindingSize',
  'maxUniformBuffersPerShaderStage',
  'maxVertexAttributes',
  'maxVertexBufferArrayStride',
  'maxVertexBuffers',
  'minStorageBufferOffsetAlignment',
  'minUniformBufferOffsetAlignment'
]);

function finiteLimitEntries(limits) {
  const names = new Set();
  let current = limits;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  return [...names]
    .filter((name) => name !== 'constructor' && Number.isSafeInteger(limits?.[name]))
    .sort()
    .map((name) => [name, Number(limits[name])]);
}

function completeLimits(limits, label) {
  const result = Object.fromEntries(finiteLimitEntries(limits));
  if (Object.keys(result).length === 0) {
    throw new Error(`actual PlayCanvas WebGPU ${label} limits are unavailable`);
  }
  return result;
}

function canonicalSupportedLimits(supportedLimits) {
  const result = {};
  for (const name of Q1_CANONICAL_SUPPORTED_LIMIT_NAMES) {
    const value = supportedLimits[name];
    if (!Number.isSafeInteger(value)) {
      throw new Error(`actual selected adapter is missing canonical supported limit ${name}`);
    }
    result[name] = value;
  }
  return result;
}

function adapterInfoFields(info) {
  return Object.fromEntries([
    'vendor',
    'architecture',
    'device',
    'description',
    'subgroupMinSize',
    'subgroupMaxSize'
  ].map((name) => [name, info?.[name] ?? null]));
}

export function captureWebGpuEnvironmentReceipt(graphicsDevice) {
  if (!graphicsDevice?.isWebGPU || !graphicsDevice.gpuAdapter || !graphicsDevice.wgpu) {
    throw new Error('actual PlayCanvas WebGPU adapter/device is unavailable');
  }
  const info = adapterInfoFields(graphicsDevice.gpuAdapter.info);
  const adapterSupportedLimits = completeLimits(
    graphicsDevice.gpuAdapter.limits,
    'adapter supported'
  );
  const deviceEffectiveLimits = completeLimits(
    graphicsDevice.wgpu.limits,
    'device effective'
  );
  const exposedInfo = Object.values(info).some((value) =>
    (typeof value === 'string' && value.trim().length > 0) || Number.isSafeInteger(value)
  );
  return {
    schema: Q1_WEBGPU_ENVIRONMENT_SCHEMA,
    endpoint: 'playcanvas',
    selected_adapter: {
      provenance: 'playcanvas_graphicsDevice.gpuAdapter',
      info_status: exposedInfo ? 'browser_exposed' : 'browser_redacted',
      info,
      supported_limits: adapterSupportedLimits
    },
    selected_device: {
      provenance: 'playcanvas_graphicsDevice.wgpu',
      effective_limits: deviceEffectiveLimits
    },
    canonical_adapter: {
      schema: Q1_CANONICAL_ADAPTER_SCHEMA,
      selection_class: Q1_ADAPTER_SELECTION_CLASS,
      backend_class: 'browser_webgpu',
      hardware_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
      supported_limits_schema: Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
      supported_limits: canonicalSupportedLimits(adapterSupportedLimits)
    }
  };
}
