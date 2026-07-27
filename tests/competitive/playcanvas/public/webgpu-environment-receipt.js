function finiteLimitEntries(limits) {
  const names = new Set();
  let current = limits;
  while (current && current !== Object.prototype) {
    for (const name of Object.getOwnPropertyNames(current)) names.add(name);
    current = Object.getPrototypeOf(current);
  }
  return [...names]
    .filter((name) => name !== 'constructor' && Number.isFinite(limits?.[name]))
    .sort()
    .map((name) => [name, Number(limits[name])]);
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
  const limits = Object.fromEntries(finiteLimitEntries(graphicsDevice.wgpu.limits));
  if (Object.keys(limits).length === 0) {
    throw new Error('actual PlayCanvas WebGPU device limits are unavailable');
  }
  const adapter = [info.vendor, info.architecture, info.device]
    .filter((value) => typeof value === 'string' && value.trim().length > 0)
    .join(' / ');
  return {
    schema: 'gsplat-playcanvas-webgpu-environment/v1',
    adapter: adapter || 'webgpu_adapter_identity_redacted_by_browser',
    driver: null,
    driver_status: 'not_exposed_by_webgpu',
    adapter_info: info,
    limits
  };
}
