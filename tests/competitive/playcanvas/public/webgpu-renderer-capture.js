export const PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA =
  'gsplat-playcanvas-webgpu-renderer-capture/v1';

export const PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER =
  'playcanvas_webgpu_copy_texture_to_buffer';

const BYTES_PER_PIXEL = 4;
const COPY_BYTES_PER_ROW_ALIGNMENT = 256;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;

function positiveSafeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label} must be a positive safe integer`);
  }
  return value;
}

function requireSha256(value, label) {
  if (!SHA256_PATTERN.test(value ?? '')) throw new Error(`${label} must be a SHA-256 digest`);
  return value;
}

function roundUp(value, alignment) {
  return Math.ceil(value / alignment) * alignment;
}

async function sha256Bytes(bytes) {
  const digest = await globalThis.crypto.subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
}

function validateCaptureIdentity(identity, width, height) {
  if (!identity || typeof identity !== 'object') throw new Error('capture identity is required');
  const { camera_receipt: camera, resolution, source } = identity;
  if (!camera || !Number.isSafeInteger(camera.trace_frame_index) || camera.trace_frame_index < 0) {
    throw new Error('capture identity requires a trace-bound camera receipt');
  }
  for (const stage of ['requested', 'surface', 'internal_render']) {
    if (resolution?.[`${stage}_width`] !== width || resolution?.[`${stage}_height`] !== height) {
      throw new Error(`capture ${stage} resolution does not match the WebGPU backbuffer`);
    }
  }
  if (resolution.dynamic_resolution !== 'disabled' || resolution.upscaling !== 'disabled' ||
      resolution.internal_full_resolution !== true) {
    throw new Error('capture identity does not prove full internal resolution');
  }
  positiveSafeInteger(source?.source_splat_count, 'capture source splat count');
  if (source.decoded_splat_count !== source.source_splat_count ||
      source.resident_splat_count !== source.source_splat_count ||
      source.source_sh_degree !== source.resident_sh_degree ||
      source.source_membership !== 'all' || source.sampling !== 'disabled' ||
      source.lod !== 'disabled' || source.partial_scene_published !== false ||
      source.full_quality !== true) {
    throw new Error('capture identity does not preserve the complete source scene');
  }
  requireSha256(source.dataset_sha256, 'capture dataset');
}

function rgba8FromMappedRows(mapped, width, height, paddedBytesPerRow, textureFormat) {
  const rowBytes = width * BYTES_PER_PIXEL;
  const rgba8 = new Uint8Array(rowBytes * height);
  for (let y = 0; y < height; y += 1) {
    const source = mapped.subarray(y * paddedBytesPerRow, y * paddedBytesPerRow + rowBytes);
    const targetOffset = y * rowBytes;
    if (textureFormat === 'rgba8unorm') {
      rgba8.set(source, targetOffset);
      continue;
    }
    for (let x = 0; x < rowBytes; x += BYTES_PER_PIXEL) {
      rgba8[targetOffset + x] = source[x + 2];
      rgba8[targetOffset + x + 1] = source[x + 1];
      rgba8[targetOffset + x + 2] = source[x];
      rgba8[targetOffset + x + 3] = source[x + 3];
    }
  }
  return rgba8;
}

/**
 * Enqueue a copy of the exact PlayCanvas WebGPU backbuffer assigned to the
 * just-finished renderer frame. This must be called synchronously from the
 * pinned AppBase `frameend` callback: AppBase.render() has already called
 * graphicsDevice.frameEnd()/submit(), while the RAF callback has not returned
 * and the canvas texture has not advanced to the next frame.
 */
export function beginPlayCanvasWebgpuRendererCapture({
  graphicsDevice,
  rendererFrameSequence,
  rendererSubmitVersion,
  identity
}) {
  if (!graphicsDevice?.isWebGPU || !graphicsDevice.wgpu) {
    throw new Error('renderer capture requires the selected PlayCanvas WebGPU device');
  }
  positiveSafeInteger(rendererFrameSequence, 'renderer frame sequence');
  positiveSafeInteger(rendererSubmitVersion, 'renderer submit version');
  if (graphicsDevice.submitVersion !== rendererSubmitVersion) {
    throw new Error('renderer capture was not issued immediately after the rendered frame submit');
  }
  const width = positiveSafeInteger(graphicsDevice.width, 'renderer capture width');
  const height = positiveSafeInteger(graphicsDevice.height, 'renderer capture height');
  validateCaptureIdentity(identity, width, height);

  const texture = graphicsDevice.backBuffer?.impl?.assignedColorTexture;
  if (!texture) throw new Error('PlayCanvas WebGPU backbuffer has no assigned frame texture');
  if (texture.width !== width || texture.height !== height) {
    throw new Error('assigned WebGPU frame texture dimensions do not match the renderer');
  }
  const textureFormat = graphicsDevice.canvasConfig?.format;
  if (!['rgba8unorm', 'bgra8unorm'].includes(textureFormat)) {
    throw new Error(`renderer capture does not support canvas texture format ${textureFormat}`);
  }
  if ((graphicsDevice.canvasConfig.usage & GPUTextureUsage.COPY_SRC) === 0) {
    throw new Error('PlayCanvas WebGPU canvas texture lacks COPY_SRC usage');
  }

  const rowBytes = width * BYTES_PER_PIXEL;
  const paddedBytesPerRow = roundUp(rowBytes, COPY_BYTES_PER_ROW_ALIGNMENT);
  const mappedByteLength = paddedBytesPerRow * height;
  const readback = graphicsDevice.wgpu.createBuffer({
    label: 'gsplat-playcanvas-renderer-capture-readback',
    size: mappedByteLength,
    usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ
  });
  let copySubmitVersionBefore;
  let copySubmitVersionAfter;
  try {
    const encoder = graphicsDevice.getCommandEncoder();
    encoder.copyTextureToBuffer(
      { texture },
      { buffer: readback, offset: 0, bytesPerRow: paddedBytesPerRow, rowsPerImage: height },
      { width, height, depthOrArrayLayers: 1 }
    );
    copySubmitVersionBefore = graphicsDevice.submitVersion;
    graphicsDevice.submit();
    copySubmitVersionAfter = graphicsDevice.submitVersion;
    if (copySubmitVersionBefore !== rendererSubmitVersion ||
        copySubmitVersionAfter !== copySubmitVersionBefore + 1) {
      throw new Error('renderer capture copy did not form one ordered PlayCanvas WebGPU submission');
    }
  } catch (error) {
    readback.destroy();
    throw error;
  }

  const completion = (async () => {
    try {
      await readback.mapAsync(GPUMapMode.READ);
      const mapped = new Uint8Array(readback.getMappedRange(0, mappedByteLength));
      const rgba8 = rgba8FromMappedRows(
        mapped,
        width,
        height,
        paddedBytesPerRow,
        textureFormat
      );
      const cameraReceiptJson = JSON.stringify(identity.camera_receipt);
      const [rgba8Sha256, cameraReceiptSha256] = await Promise.all([
        sha256Bytes(rgba8),
        sha256Bytes(new TextEncoder().encode(cameraReceiptJson))
      ]);
      return {
        rgba8,
        receipt: {
          schema: PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA,
          producer: PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER,
          status: 'readback_complete',
          renderer_frame_sequence: rendererFrameSequence,
          renderer_submit_version: rendererSubmitVersion,
          copy_submit_version_before: copySubmitVersionBefore,
          copy_submit_version_after: copySubmitVersionAfter,
          texture_format: textureFormat,
          render_view_format: graphicsDevice.backBufferViewFormat,
          canvas_color_space: graphicsDevice.canvasConfig.colorSpace,
          canvas_alpha_mode: graphicsDevice.canvasConfig.alphaMode,
          pixel_format: 'rgba8unorm',
          row_origin: 'top_left',
          width,
          height,
          row_bytes: rowBytes,
          byte_length: rgba8.byteLength,
          rgba8_sha256: rgba8Sha256,
          camera_receipt_sha256: cameraReceiptSha256,
          camera_receipt_json: cameraReceiptJson,
          camera_receipt: identity.camera_receipt,
          resolution: identity.resolution,
          source: identity.source,
          copy_map_complete: true,
          queue_terminal_complete: false
        }
      };
    } finally {
      if (readback.mapState === 'mapped') readback.unmap();
      readback.destroy();
    }
  })();
  return { copySubmitVersionAfter, completion };
}

export function finalizePlayCanvasWebgpuRendererCapture(capture, queueDrain) {
  const { receipt, rgba8 } = capture ?? {};
  if (receipt?.schema !== PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA ||
      receipt.status !== 'readback_complete' || receipt.copy_map_complete !== true ||
      rgba8?.byteLength !== receipt.byte_length) {
    throw new Error('renderer capture readback is incomplete');
  }
  if (queueDrain?.phase !== 'post_capture_presentation' ||
      queueDrain.frameLoopStopped !== true || queueDrain.submitVersionStable !== true ||
      queueDrain.submitVersionBefore !== receipt.copy_submit_version_after ||
      queueDrain.submitVersionAfter !== receipt.copy_submit_version_after) {
    throw new Error('renderer capture is not bound to the terminal WebGPU queue drain');
  }
  return {
    rgba8,
    receipt: {
      ...receipt,
      status: 'terminal',
      queue_terminal_complete: true,
      terminal_queue_drain: {
        phase: queueDrain.phase,
        submit_version_before: queueDrain.submitVersionBefore,
        submit_version_after: queueDrain.submitVersionAfter,
        submit_version_stable: queueDrain.submitVersionStable
      }
    }
  };
}

export function rgba8ToBase64(bytes) {
  if (!(bytes instanceof Uint8Array)) throw new Error('RGBA8 payload must be a Uint8Array');
  let binary = '';
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}
