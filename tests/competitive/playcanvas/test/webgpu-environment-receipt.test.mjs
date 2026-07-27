import assert from 'node:assert/strict';
import test from 'node:test';
import {
  Q1_ADAPTER_IDENTITY_STATUS,
  Q1_ADAPTER_SELECTION_CLASS,
  Q1_CANONICAL_ADAPTER_SCHEMA,
  Q1_CANONICAL_SUPPORTED_LIMIT_NAMES,
  Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
  Q1_WEBGPU_ENVIRONMENT_SCHEMA,
  captureWebGpuEnvironmentReceipt
} from '../public/webgpu-environment-receipt.js';

function limits(offset = 0) {
  return Object.fromEntries(
    Q1_CANONICAL_SUPPORTED_LIMIT_NAMES.map((name, index) => [name, 1000 + index + offset])
  );
}

test('environment receipt separates the actual selected adapter and effective device limits', () => {
  const adapterLimits = limits();
  adapterLimits.futureBrowserLimit = 9876;
  const deviceLimits = limits(-100);
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
      },
      limits: adapterLimits
    },
    wgpu: { limits: deviceLimits }
  });
  assert.equal(receipt.schema, Q1_WEBGPU_ENVIRONMENT_SCHEMA);
  assert.equal(receipt.endpoint, 'playcanvas');
  assert.equal(receipt.selected_adapter.provenance, 'playcanvas_graphicsDevice.gpuAdapter');
  assert.equal(receipt.selected_adapter.info_status, 'browser_exposed');
  assert.equal(receipt.selected_adapter.supported_limits.futureBrowserLimit, 9876);
  assert.equal(receipt.selected_device.provenance, 'playcanvas_graphicsDevice.wgpu');
  assert.deepEqual(receipt.selected_device.effective_limits, deviceLimits);
  assert.deepEqual(receipt.canonical_adapter, {
    schema: Q1_CANONICAL_ADAPTER_SCHEMA,
    selection_class: Q1_ADAPTER_SELECTION_CLASS,
    backend_class: 'browser_webgpu',
    hardware_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
    supported_limits_schema: Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
    supported_limits: limits()
  });
});

test('environment receipt retains redaction without inventing a hardware name', () => {
  const receipt = captureWebGpuEnvironmentReceipt({
    isWebGPU: true,
    gpuAdapter: { info: {}, limits: limits() },
    wgpu: { limits: limits(-10) }
  });
  assert.equal(receipt.selected_adapter.info_status, 'browser_redacted');
  assert.deepEqual(receipt.selected_adapter.info, {
    vendor: null,
    architecture: null,
    device: null,
    description: null,
    subgroupMinSize: null,
    subgroupMaxSize: null
  });
  assert.equal(receipt.canonical_adapter.hardware_identity_status, Q1_ADAPTER_IDENTITY_STATUS);
});

test('environment receipt fails closed without either actual owner or every canonical limit', () => {
  assert.throws(() => captureWebGpuEnvironmentReceipt({}), /adapter\/device is unavailable/);
  assert.throws(() => captureWebGpuEnvironmentReceipt({
    isWebGPU: true,
    gpuAdapter: { info: {}, limits: limits() },
    wgpu: { limits: {} }
  }), /device effective limits are unavailable/);
  const incomplete = limits();
  delete incomplete.maxBufferSize;
  assert.throws(() => captureWebGpuEnvironmentReceipt({
    isWebGPU: true,
    gpuAdapter: { info: {}, limits: incomplete },
    wgpu: { limits: limits() }
  }), /missing canonical supported limit maxBufferSize/);
});
