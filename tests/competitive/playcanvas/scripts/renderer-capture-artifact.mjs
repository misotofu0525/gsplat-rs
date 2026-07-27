import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import {
  PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER,
  PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA
} from '../public/webgpu-renderer-capture.js';

const PNG_SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type, payload) {
  const kind = Buffer.from(type, 'ascii');
  const length = Buffer.alloc(4);
  length.writeUInt32BE(payload.length);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE(crc32(Buffer.concat([kind, payload])));
  return Buffer.concat([length, kind, payload, checksum]);
}

export function encodeRgba8Png(rgba8, width, height) {
  if (!Buffer.isBuffer(rgba8) || !Number.isSafeInteger(width) || width <= 0 ||
      !Number.isSafeInteger(height) || height <= 0 || rgba8.length !== width * height * 4) {
    throw new Error('RGBA8 PNG input is invalid');
  }
  const rowBytes = width * 4;
  const filtered = Buffer.alloc((rowBytes + 1) * height);
  for (let y = 0; y < height; y += 1) {
    const targetOffset = y * (rowBytes + 1);
    filtered[targetOffset] = 0;
    rgba8.copy(filtered, targetOffset + 1, y * rowBytes, (y + 1) * rowBytes);
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 6;
  return Buffer.concat([
    PNG_SIGNATURE,
    pngChunk('IHDR', header),
    pngChunk('IDAT', deflateSync(filtered)),
    pngChunk('IEND', Buffer.alloc(0))
  ]);
}

export function materializeRendererCapture({ receipt, rgba8Base64 }) {
  if (receipt?.schema !== PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_SCHEMA ||
      receipt.producer !== PLAYCANVAS_WEBGPU_RENDERER_CAPTURE_PRODUCER ||
      receipt.status !== 'terminal' || receipt.copy_map_complete !== true ||
      receipt.queue_terminal_complete !== true) {
    throw new Error('PlayCanvas renderer capture terminal is invalid');
  }
  if (typeof receipt.camera_receipt_json !== 'string' ||
      sha256(Buffer.from(receipt.camera_receipt_json, 'utf8')) !== receipt.camera_receipt_sha256 ||
      JSON.stringify(JSON.parse(receipt.camera_receipt_json)) !==
        JSON.stringify(receipt.camera_receipt)) {
    throw new Error('PlayCanvas renderer capture camera serialization is invalid');
  }
  if (typeof rgba8Base64 !== 'string' || rgba8Base64.length === 0) {
    throw new Error('PlayCanvas renderer capture omitted its RGBA8 payload');
  }
  const rgba8 = Buffer.from(rgba8Base64, 'base64');
  if (rgba8.length !== receipt.byte_length || rgba8.length !== receipt.width * receipt.height * 4 ||
      receipt.row_bytes !== receipt.width * 4 || sha256(rgba8) !== receipt.rgba8_sha256) {
    throw new Error('PlayCanvas renderer capture RGBA8 payload does not match its producer receipt');
  }
  const png = encodeRgba8Png(rgba8, receipt.width, receipt.height);
  return {
    rgba8,
    png,
    receipt: {
      schema: 'gsplat-playcanvas-renderer-capture-materialization/v1',
      source: 'host_png_from_renderer_owned_webgpu_rgba8',
      source_capture_schema: receipt.schema,
      source_capture_producer: receipt.producer,
      source_rgba8_sha256: receipt.rgba8_sha256,
      rgba8_file: 'final-frame.rgba8',
      rgba8_byte_length: rgba8.length,
      png_file: 'final-frame.png',
      png_byte_length: png.length,
      png_sha256: sha256(png),
      width: receipt.width,
      height: receipt.height
    }
  };
}
