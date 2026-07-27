import assert from 'node:assert/strict';
import test from 'node:test';
import { captureWebGpuEnvironmentReceipt } from '../public/webgpu-environment-receipt.js';

test('environment receipt snapshots the actual PlayCanvas adapter and inherited device limits', () => {
  const limitPrototype = Object.create(null, {
    maxBufferSize: { get: () => 4294967292 }
  });
  const limits = Object.create(limitPrototype);
  Object.defineProperty(limits, 'maxTextureDimension2D', { value: 16384 });
  const receipt = captureWebGpuEnvironmentReceipt({
    isWebGPU: true,
    gpuAdapter: {
      info: {
        vendor: 'Apple',
        architecture: 'apple8',
        device: 'M4',
        description: 'Metal',
        subgroupMinSize: 4,
        subgroupMaxSize: 32
      }
    },
    wgpu: { limits }
  });
  assert.deepEqual(receipt, {
    schema: 'gsplat-playcanvas-webgpu-environment/v1',
    adapter: 'Apple / apple8 / M4',
    driver: null,
    driver_status: 'not_exposed_by_webgpu',
    adapter_info: {
      vendor: 'Apple',
      architecture: 'apple8',
      device: 'M4',
      description: 'Metal',
      subgroupMinSize: 4,
      subgroupMaxSize: 32
    },
    limits: { maxBufferSize: 4294967292, maxTextureDimension2D: 16384 }
  });
});

test('environment receipt fails closed without the renderer-selected device or limits', () => {
  assert.throws(() => captureWebGpuEnvironmentReceipt({}), /adapter\/device is unavailable/);
  assert.throws(() => captureWebGpuEnvironmentReceipt({
    isWebGPU: true,
    gpuAdapter: { info: {} },
    wgpu: { limits: {} }
  }), /device limits are unavailable/);
});
