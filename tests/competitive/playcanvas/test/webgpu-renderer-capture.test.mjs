import assert from 'node:assert/strict';
import { createHash, webcrypto } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import {
  beginPlayCanvasWebgpuRendererCapture,
  finalizePlayCanvasWebgpuRendererCapture,
  PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER,
  PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA,
  rgba8ToBase64
} from '../public/webgpu-renderer-capture.js';
import {
  encodeRgba8Png,
  materializeRendererCapture
} from '../scripts/renderer-capture-artifact.mjs';

globalThis.crypto ??= webcrypto;
globalThis.GPUTextureUsage ??= { COPY_SRC: 1 };
globalThis.GPUBufferUsage ??= { COPY_DST: 2, MAP_READ: 4 };
globalThis.GPUMapMode ??= { READ: 1 };

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function captureIdentity() {
  return {
    camera_receipt: {
      schema: 'gsplat-playcanvas-runtime-camera-receipt/v1',
      trace_frame_index: 1,
      phase: 'presentation_frame_2'
    },
    resolution: {
      requested_width: 2,
      requested_height: 2,
      surface_width: 2,
      surface_height: 2,
      internal_render_width: 2,
      internal_render_height: 2,
      dynamic_resolution: 'disabled',
      upscaling: 'disabled',
      internal_full_resolution: true
    },
    source: {
      dataset_id: 'fixture',
      dataset_sha256: 'a'.repeat(64),
      source_splat_count: 3,
      decoded_splat_count: 3,
      resident_splat_count: 3,
      source_sh_degree: 3,
      resident_sh_degree: 3,
      source_membership: 'all',
      sampling: 'disabled',
      lod: 'disabled',
      partial_scene_published: false,
      full_quality: true
    }
  };
}

function fakeGraphicsDevice({ copySrc = true, failAt = null } = {}) {
  const padded = new Uint8Array(256 * 2);
  // Two BGRA rows. Padding is deliberately non-zero to prove it is stripped.
  padded.fill(0xee);
  padded.set([30, 20, 10, 255, 60, 50, 40, 128], 0);
  padded.set([90, 80, 70, 64, 120, 110, 100, 0], 256);
  let destroyed = false;
  let copied = null;
  const buffer = {
    mapState: 'unmapped',
    async mapAsync() { this.mapState = 'mapped'; },
    getMappedRange() { return padded.buffer; },
    unmap() { this.mapState = 'unmapped'; },
    destroy() { destroyed = true; }
  };
  const device = {
    isWebGPU: true,
    width: 2,
    height: 2,
    submitVersion: 7,
    canvasConfig: {
      format: 'bgra8unorm',
      colorSpace: 'srgb',
      alphaMode: 'opaque',
      usage: copySrc ? GPUTextureUsage.COPY_SRC : 0
    },
    backBufferViewFormat: 'bgra8unorm',
    backBuffer: { impl: { assignedColorTexture: { width: 2, height: 2 } } },
    wgpu: { createBuffer: () => buffer },
    getCommandEncoder() {
      if (failAt === 'encoder') throw new Error('encoder failed');
      return { copyTextureToBuffer: (...args) => {
        if (failAt === 'copy') throw new Error('copy failed');
        copied = args;
      } };
    },
    submit() {
      if (failAt === 'submit') throw new Error('submit failed');
      this.submitVersion += 1;
    }
  };
  return { device, copied: () => copied, destroyed: () => destroyed };
}

test('same-frame renderer capture strips padding, converts BGRA, and terminalizes once', async () => {
  const fake = fakeGraphicsDevice();
  const pending = beginPlayCanvasWebgpuRendererCapture({
    graphicsDevice: fake.device,
    rendererFrameSequence: 42,
    rendererSubmitVersion: 7,
    identity: captureIdentity()
  });
  assert.equal(pending.copySubmitVersionAfter, 8);
  assert.ok(fake.copied(), 'copyTextureToBuffer was not encoded');
  const completed = await pending.completion;
  assert.deepEqual(Array.from(completed.rgba8), [
    10, 20, 30, 255, 40, 50, 60, 128,
    70, 80, 90, 64, 100, 110, 120, 0
  ]);
  assert.equal(completed.receipt.renderer_submit_version, 7);
  assert.equal(completed.receipt.copy_submit_version_after, 8);
  assert.equal(completed.receipt.byte_length, 16);
  assert.equal(completed.receipt.rgba8_sha256, createHash('sha256').update(completed.rgba8).digest('hex'));
  assert.equal(JSON.parse(completed.receipt.camera_receipt_json).trace_frame_index, 1);
  assert.equal(
    completed.receipt.camera_receipt_sha256,
    createHash('sha256').update(completed.receipt.camera_receipt_json).digest('hex')
  );
  assert.equal(fake.destroyed(), true);

  const terminal = finalizePlayCanvasWebgpuRendererCapture(completed, {
    phase: 'post_capture_presentation',
    frameLoopStopped: true,
    submitVersionStable: true,
    submitVersionBefore: 8,
    submitVersionAfter: 8
  });
  assert.equal(terminal.receipt.schema, PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA);
  assert.equal(terminal.receipt.producer, PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER);
  assert.equal(terminal.receipt.status, 'terminal');
  assert.equal(terminal.receipt.queue_terminal_complete, true);
});

test('renderer capture fails closed before readback when COPY_SRC or source identity is absent', () => {
  const noCopy = fakeGraphicsDevice({ copySrc: false });
  assert.throws(() => beginPlayCanvasWebgpuRendererCapture({
    graphicsDevice: noCopy.device,
    rendererFrameSequence: 1,
    rendererSubmitVersion: 7,
    identity: captureIdentity()
  }), /lacks COPY_SRC/);

  const incomplete = captureIdentity();
  incomplete.source.resident_splat_count = 2;
  const fake = fakeGraphicsDevice();
  assert.throws(() => beginPlayCanvasWebgpuRendererCapture({
    graphicsDevice: fake.device,
    rendererFrameSequence: 1,
    rendererSubmitVersion: 7,
    identity: incomplete
  }), /complete source scene/);
});

test('renderer capture destroys its readback buffer after synchronous encode failures', () => {
  for (const failAt of ['encoder', 'copy', 'submit']) {
    const fake = fakeGraphicsDevice({ failAt });
    assert.throws(() => beginPlayCanvasWebgpuRendererCapture({
      graphicsDevice: fake.device,
      rendererFrameSequence: 1,
      rendererSubmitVersion: 7,
      identity: captureIdentity()
    }), new RegExp(`${failAt} failed`));
    assert.equal(fake.destroyed(), true, `${failAt} failure leaked the readback buffer`);
  }
});

test('host PNG is derived from and cryptographically bound to renderer RGBA8 bytes', async () => {
  const rgba8 = Buffer.from([
    255, 0, 0, 255, 0, 255, 0, 255,
    0, 0, 255, 255, 255, 255, 255, 128
  ]);
  const receipt = {
    schema: PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA,
    producer: PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER,
    status: 'terminal',
    copy_map_complete: true,
    queue_terminal_complete: true,
    width: 2,
    height: 2,
    row_bytes: 8,
    byte_length: rgba8.length,
    rgba8_sha256: createHash('sha256').update(rgba8).digest('hex')
  };
  receipt.camera_receipt = captureIdentity().camera_receipt;
  receipt.camera_receipt_json = JSON.stringify(receipt.camera_receipt);
  receipt.camera_receipt_sha256 = createHash('sha256').update(receipt.camera_receipt_json).digest('hex');
  const materialized = materializeRendererCapture({
    receipt,
    rgba8Base64: rgba8ToBase64(rgba8)
  });
  assert.deepEqual(materialized.rgba8, rgba8);
  assert.deepEqual(materialized.png.subarray(0, 8), Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
  assert.equal(materialized.receipt.source_rgba8_sha256, receipt.rgba8_sha256);
  assert.equal(materialized.receipt.png_sha256, createHash('sha256').update(materialized.png).digest('hex'));
  assert.deepEqual(encodeRgba8Png(rgba8, 2, 2), materialized.png);

  assert.throws(() => materializeRendererCapture({
    receipt,
    rgba8Base64: Buffer.from(rgba8.map((byte, index) => index === 0 ? byte ^ 1 : byte)).toString('base64')
  }), /does not match its producer receipt/);
});

test('pinned PlayCanvas calls frameend after WebGPU frameEnd submit and exposes COPY_SRC backbuffer', async () => {
  const [appBase, webgpu] = await Promise.all([
    readFile(resolve(root, 'node_modules/playcanvas/build/playcanvas/src/framework/app-base.js'), 'utf8'),
    readFile(resolve(
      root,
      'node_modules/playcanvas/build/playcanvas/src/platform/graphics/webgpu/webgpu-graphics-device.js'
    ), 'utf8')
  ]);
  assert.match(appBase, /this\.render\(\);[\s\S]*this\.fire\("frameend"\)/);
  assert.match(appBase, /this\.graphicsDevice\.frameEnd\(\);/);
  assert.match(webgpu, /frameEnd\(\) \{[\s\S]*this\.submit\(\);/);
  assert.match(
    webgpu,
    /usage: GPUTextureUsage\.RENDER_ATTACHMENT \| GPUTextureUsage\.COPY_SRC \| GPUTextureUsage\.COPY_DST/
  );
  assert.match(webgpu, /wrt\.assignColorTexture\(outColorBuffer, attachmentViewFormat\)/);
});
