import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { deflateSync } from 'node:zlib';
import {
  computeRawRgba8ImageMetrics,
  decodeRawRgba8Png,
} from '../../../perf/png-image-metrics.mjs';
import { encodeRgba8Png } from '../scripts/renderer-capture-artifact.mjs';

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

function paeth(left, above, upperLeft) {
  const estimate = left + above - upperLeft;
  const distances = [
    Math.abs(estimate - left),
    Math.abs(estimate - above),
    Math.abs(estimate - upperLeft)
  ];
  if (distances[0] <= distances[1] && distances[0] <= distances[2]) return left;
  return distances[1] <= distances[2] ? above : upperLeft;
}

function filterRow(row, previous, filterType, bytesPerPixel = 4) {
  const filtered = Buffer.alloc(row.length);
  for (let index = 0; index < row.length; index += 1) {
    const left = index >= bytesPerPixel ? row[index - bytesPerPixel] : 0;
    const above = previous === null ? 0 : previous[index];
    const upperLeft = previous !== null && index >= bytesPerPixel
      ? previous[index - bytesPerPixel]
      : 0;
    let predictor = 0;
    if (filterType === 1) predictor = left;
    else if (filterType === 2) predictor = above;
    else if (filterType === 3) predictor = Math.floor((left + above) / 2);
    else if (filterType === 4) predictor = paeth(left, above, upperLeft);
    filtered[index] = (row[index] - predictor) & 0xff;
  }
  return Buffer.concat([Buffer.from([filterType]), filtered]);
}

function filteredPng(rows, filterTypes, colorType = 6) {
  const bytesPerPixel = colorType === 0 ? 1 : colorType === 2 ? 3 : 4;
  const width = rows[0].length / bytesPerPixel;
  const height = rows.length;
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = colorType;
  const filtered = Buffer.concat(rows.map((row, index) =>
    filterRow(
      row,
      index === 0 ? null : rows[index - 1],
      filterTypes[index],
      bytesPerPixel
    )));
  return Buffer.concat([
    PNG_SIGNATURE,
    pngChunk('IHDR', header),
    pngChunk('IDAT', deflateSync(filtered)),
    pngChunk('IEND', Buffer.alloc(0))
  ]);
}

function replaceIdat(png, payloads) {
  const ihdrEnd = PNG_SIGNATURE.length + 12 + 13;
  const idatLength = png.readUInt32BE(ihdrEnd);
  const tail = png.subarray(ihdrEnd + 12 + idatLength);
  return Buffer.concat([
    png.subarray(0, ihdrEnd),
    ...payloads.map((payload) => pngChunk('IDAT', payload)),
    tail
  ]);
}

function insertChunkAfterIhdr(png, type, payload) {
  const ihdrEnd = PNG_SIGNATURE.length + 12 + 13;
  return Buffer.concat([
    png.subarray(0, ihdrEnd),
    pngChunk(type, payload),
    png.subarray(ihdrEnd)
  ]);
}

test('raw PNG decoder preserves partial-alpha sample bytes', () => {
  const rgba = Buffer.from([
    19, 87, 201, 1,
    211, 17, 63, 127,
    3, 251, 129, 254,
    41, 59, 26, 255,
  ]);
  const decoded = decodeRawRgba8Png(encodeRgba8Png(rgba, 2, 2));
  assert.deepEqual(decoded.rgba, rgba);
  assert.equal(sha256(decoded.rgba), sha256(rgba));
});

test('raw PNG decoder implements all five non-interlaced RGBA8 filters', () => {
  const rows = [0, 1, 2, 3, 4].map((row) => Buffer.from([
    11 + row, 31 + row, 71 + row, 1 + row,
    131 + row, 151 + row, 191 + row, 121 + row,
  ]));
  const decoded = decodeRawRgba8Png(filteredPng(rows, [0, 1, 2, 3, 4]));
  assert.deepEqual(decoded.rgba, Buffer.concat(rows));
});

test('raw PNG decoder implements all five RGB8 filters and expands opaque alpha', () => {
  const rows = [0, 1, 2, 3, 4].map((row) => Buffer.from([
    13 + row, 33 + row, 73 + row,
    133 + row, 153 + row, 193 + row,
  ]));
  const decoded = decodeRawRgba8Png(filteredPng(rows, [0, 1, 2, 3, 4], 2));
  const expected = Buffer.from(rows.flatMap((row) => [
    row[0], row[1], row[2], 255,
    row[3], row[4], row[5], 255,
  ]));
  assert.equal(decoded.width, 2);
  assert.equal(decoded.height, 5);
  assert.deepEqual(decoded.rgba, expected);
});

test('raw RGB8 PNG contract rejects tRNS instead of fabricating opaque alpha', () => {
  const rgb = filteredPng([Buffer.from([1, 2, 3])], [0], 2);
  const transparentSample = Buffer.from([0, 1, 0, 2, 0, 3]);
  assert.throws(
    () => decodeRawRgba8Png(insertChunkAfterIhdr(rgb, 'tRNS', transparentSample)),
    /transparency semantics/
  );
});

test('raw PNG contract still rejects unsupported color types', () => {
  const grayscale = filteredPng([Buffer.from([17, 29])], [0], 0);
  assert.throws(() => decodeRawRgba8Png(grayscale), /RGB8 or RGBA8/);
});

test('raw PNG decoder joins split IDAT chunks without changing samples', () => {
  const rgba = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]);
  const png = encodeRgba8Png(rgba, 2, 1);
  const idatOffset = PNG_SIGNATURE.length + 12 + 13;
  const idatLength = png.readUInt32BE(idatOffset);
  const compressed = png.subarray(idatOffset + 8, idatOffset + 8 + idatLength);
  const midpoint = Math.floor(compressed.length / 2);
  const decoded = decodeRawRgba8Png(
    replaceIdat(png, [compressed.subarray(0, midpoint), compressed.subarray(midpoint)])
  );
  assert.deepEqual(decoded.rgba, rgba);
});

test('raw image metrics retain partial alpha instead of compositing it', () => {
  const referenceRgba = Buffer.from([
    200, 100, 50, 1,
    100, 200, 50, 127,
  ]);
  const candidateRgba = Buffer.from([
    0, 0, 0, 255,
    100, 200, 50, 255,
  ]);
  const reference = decodeRawRgba8Png(encodeRgba8Png(referenceRgba, 2, 1));
  const candidate = decodeRawRgba8Png(encodeRgba8Png(candidateRgba, 2, 1));
  const metrics = computeRawRgba8ImageMetrics(reference, candidate);
  assert.equal(metrics.rgbMae8bit, 350 / 6);
  assert.equal(metrics.alphaMae8bit, (254 + 128) / 2);
  assert.ok(metrics.score < 1);
});

test('raw PNG contract rejects color management and unsupported critical chunks', () => {
  const png = encodeRgba8Png(Buffer.from([1, 2, 3, 4]), 1, 1);
  assert.throws(
    () => decodeRawRgba8Png(
      insertChunkAfterIhdr(png, 'gAMA', Buffer.alloc(4)),
      { rejectColorManagement: true }
    ),
    /color-management/
  );
  assert.throws(
    () => decodeRawRgba8Png(insertChunkAfterIhdr(png, 'PLTE', Buffer.from([1, 2, 3]))),
    /unsupported critical/
  );
});

test('raw PNG contract rejects CRC changes and bytes after IEND', () => {
  const png = encodeRgba8Png(Buffer.from([1, 2, 3, 4]), 1, 1);
  const badCrc = Buffer.from(png);
  badCrc[20] ^= 1;
  assert.throws(() => decodeRawRgba8Png(badCrc), /CRC mismatch/);
  assert.throws(
    () => decodeRawRgba8Png(Buffer.concat([png, Buffer.from([0])])),
    /bytes after IEND/
  );
});

test('raw PNG contract rejects wrong dimensions, invalid filters, and trailing zlib bytes', () => {
  const row = Buffer.from([1, 2, 3, 4]);
  const png = filteredPng([row], [0]);
  assert.throws(
    () => decodeRawRgba8Png(png, { expectedWidth: 2, expectedHeight: 1 }),
    /dimensions differ/
  );
  assert.throws(() => decodeRawRgba8Png(filteredPng([row], [5])), /filter type/);

  const idatOffset = PNG_SIGNATURE.length + 12 + 13;
  const idatLength = png.readUInt32BE(idatOffset);
  const compressed = png.subarray(idatOffset + 8, idatOffset + 8 + idatLength);
  assert.throws(
    () => decodeRawRgba8Png(replaceIdat(png, [Buffer.concat([compressed, Buffer.from([1, 2])])])),
    /stream boundary/
  );
});

test('raw metric API rejects incomplete RGBA8 buffers', () => {
  assert.throws(
    () => computeRawRgba8ImageMetrics(
      { width: 1, height: 1, rgba: Buffer.alloc(3) },
      { width: 1, height: 1, rgba: Buffer.alloc(4) }
    ),
    /complete RGBA8 image/
  );
});
