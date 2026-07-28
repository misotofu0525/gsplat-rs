import { inflateSync } from 'node:zlib';

const PNG_SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const WINDOW_SIZE = 8;
const C1 = (0.01 * 255) ** 2;
const C2 = (0.03 * 255) ** 2;
const COLOR_MANAGEMENT_CHUNKS = new Set(['cHRM', 'gAMA', 'iCCP', 'sRGB']);
const UNSUPPORTED_SAMPLE_SEMANTICS_CHUNKS = new Set(['tRNS']);
const SUPPORTED_CRITICAL_CHUNKS = new Set(['IHDR', 'IDAT', 'IEND']);

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

function paethPredictor(left, above, upperLeft) {
  const estimate = left + above - upperLeft;
  const distanceLeft = Math.abs(estimate - left);
  const distanceAbove = Math.abs(estimate - above);
  const distanceUpperLeft = Math.abs(estimate - upperLeft);
  if (distanceLeft <= distanceAbove && distanceLeft <= distanceUpperLeft) return left;
  if (distanceAbove <= distanceUpperLeft) return above;
  return upperLeft;
}

function unfilterScanline(filterType, source, previous, bytesPerPixel) {
  const result = Buffer.allocUnsafe(source.length);
  for (let index = 0; index < source.length; index += 1) {
    const left = index >= bytesPerPixel ? result[index - bytesPerPixel] : 0;
    const above = previous === null ? 0 : previous[index];
    const upperLeft = previous !== null && index >= bytesPerPixel
      ? previous[index - bytesPerPixel]
      : 0;
    let value;
    if (filterType === 0) value = source[index];
    else if (filterType === 1) value = source[index] + left;
    else if (filterType === 2) value = source[index] + above;
    else if (filterType === 3) value = source[index] + Math.floor((left + above) / 2);
    else if (filterType === 4) value = source[index] + paethPredictor(left, above, upperLeft);
    else throw new Error(`unsupported PNG filter type: ${filterType}`);
    result[index] = value & 0xff;
  }
  return result;
}

/**
 * Decode the raw sample bytes of a non-interlaced RGB8 or RGBA8 PNG.
 *
 * Q1 compares renderer-owned bytes, not browser-composited pixels. In
 * particular, this avoids Canvas2D's premultiply/unpremultiply round trip for
 * partial alpha. RGB8 is accepted only when it is intrinsically opaque:
 * transparency-bearing tRNS is rejected instead of being expanded to a false
 * alpha value of 255. The checks intentionally mirror the offline Python
 * admission decoder so both sides operate on the same byte domain.
 */
export function decodeRawRgba8Png(
  input,
  { expectedWidth = null, expectedHeight = null, rejectColorManagement = false } = {}
) {
  const data = Buffer.isBuffer(input) ? input : Buffer.from(input);
  if (data.length < PNG_SIGNATURE.length ||
      !data.subarray(0, PNG_SIGNATURE.length).equals(PNG_SIGNATURE)) {
    throw new Error('image is not a PNG');
  }

  const chunks = [];
  let offset = PNG_SIGNATURE.length;
  let sawIend = false;
  while (offset < data.length) {
    if (data.length - offset < 12) throw new Error('PNG contains a truncated chunk');
    const length = data.readUInt32BE(offset);
    const end = offset + 12 + length;
    if (!Number.isSafeInteger(end) || end > data.length) {
      throw new Error('PNG chunk length exceeds the file');
    }
    const typeBytes = data.subarray(offset + 4, offset + 8);
    const type = typeBytes.toString('ascii');
    const payload = data.subarray(offset + 8, offset + 8 + length);
    const expectedCrc = data.readUInt32BE(offset + 8 + length);
    const actualCrc = crc32(Buffer.concat([typeBytes, payload]));
    if (actualCrc !== expectedCrc) throw new Error(`PNG ${type} CRC mismatch`);
    chunks.push({ type, payload });
    offset = end;
    if (type === 'IEND') {
      sawIend = true;
      if (offset !== data.length) throw new Error('PNG contains bytes after IEND');
      break;
    }
  }
  if (!sawIend) throw new Error('PNG is missing IEND');
  const types = chunks.map(({ type }) => type);
  if (types[0] !== 'IHDR' || types.at(-1) !== 'IEND' ||
      types.filter((type) => type === 'IHDR').length !== 1 ||
      types.filter((type) => type === 'IEND').length !== 1) {
    throw new Error('PNG has an invalid chunk sequence');
  }
  for (const type of types) {
    const critical = type.length === 4 && type[0] === type[0].toUpperCase();
    if (critical && !SUPPORTED_CRITICAL_CHUNKS.has(type)) {
      throw new Error(`PNG uses unsupported critical chunk ${type}`);
    }
    if (rejectColorManagement && COLOR_MANAGEMENT_CHUNKS.has(type)) {
      throw new Error('PNG uses color-management chunks outside the raw sRGB byte contract');
    }
    if (UNSUPPORTED_SAMPLE_SEMANTICS_CHUNKS.has(type)) {
      throw new Error('PNG uses transparency semantics outside the raw RGB8/RGBA8 byte contract');
    }
  }

  const header = chunks.find(({ type }) => type === 'IHDR')?.payload;
  const idatChunks = chunks.filter(({ type }) => type === 'IDAT').map(({ payload }) => payload);
  const iend = chunks.find(({ type }) => type === 'IEND')?.payload;
  if (header?.length !== 13 || idatChunks.length === 0 || iend?.length !== 0) {
    throw new Error('PNG is missing valid required chunks');
  }
  const width = header.readUInt32BE(0);
  const height = header.readUInt32BE(4);
  if (width === 0 || height === 0) throw new Error('PNG dimensions must be positive');
  if ((expectedWidth !== null && width !== expectedWidth) ||
      (expectedHeight !== null && height !== expectedHeight)) {
    throw new Error(`PNG dimensions differ from receipt: ${width}x${height}`);
  }
  const colorType = header[9];
  if (header[8] !== 8 || ![2, 6].includes(colorType) || header[10] !== 0 ||
      header[11] !== 0 || header[12] !== 0) {
    throw new Error('PNG must be non-interlaced RGB8 or RGBA8');
  }

  const bytesPerPixel = colorType === 2 ? 3 : 4;
  const rowBytes = width * bytesPerPixel;
  const expectedFilteredBytes = height * (rowBytes + 1);
  if (!Number.isSafeInteger(rowBytes) || !Number.isSafeInteger(expectedFilteredBytes)) {
    throw new Error('PNG dimensions exceed the safe RGB8/RGBA8 range');
  }
  const compressed = Buffer.concat(idatChunks);
  let inflated;
  let consumed;
  try {
    const result = inflateSync(compressed, {
      info: true,
      maxOutputLength: expectedFilteredBytes + 1
    });
    inflated = result.buffer;
    consumed = result.engine.bytesWritten;
  } catch (error) {
    throw new Error(`PNG has invalid compressed pixel data: ${error.message}`);
  }
  if (inflated.length !== expectedFilteredBytes || consumed !== compressed.length) {
    throw new Error('PNG decompressed byte length or stream boundary is invalid');
  }

  const rgba = Buffer.allocUnsafe(width * height * 4);
  let previous = null;
  for (let y = 0; y < height; y += 1) {
    const filteredOffset = y * (rowBytes + 1);
    const row = unfilterScanline(
      inflated[filteredOffset],
      inflated.subarray(filteredOffset + 1, filteredOffset + 1 + rowBytes),
      previous,
      bytesPerPixel
    );
    if (colorType === 6) {
      row.copy(rgba, y * width * 4);
    } else {
      const outputOffset = y * width * 4;
      for (let x = 0; x < width; x += 1) {
        const sourceOffset = x * 3;
        const destinationOffset = outputOffset + x * 4;
        rgba[destinationOffset] = row[sourceOffset];
        rgba[destinationOffset + 1] = row[sourceOffset + 1];
        rgba[destinationOffset + 2] = row[sourceOffset + 2];
        rgba[destinationOffset + 3] = 255;
      }
    }
    previous = row;
  }
  return { width, height, rgba };
}

function windowSsim(reference, candidate) {
  const count = reference.length;
  let referenceSum = 0;
  let candidateSum = 0;
  for (let index = 0; index < count; index += 1) {
    referenceSum += reference[index];
    candidateSum += candidate[index];
  }
  const referenceMean = referenceSum / count;
  const candidateMean = candidateSum / count;
  let referenceVariance = 0;
  let candidateVariance = 0;
  let covariance = 0;
  for (let index = 0; index < count; index += 1) {
    const referenceDelta = reference[index] - referenceMean;
    const candidateDelta = candidate[index] - candidateMean;
    referenceVariance += referenceDelta * referenceDelta;
    candidateVariance += candidateDelta * candidateDelta;
    covariance += referenceDelta * candidateDelta;
  }
  const denominator = Math.max(count - 1, 1);
  referenceVariance /= denominator;
  candidateVariance /= denominator;
  covariance /= denominator;
  return ((2 * referenceMean * candidateMean + C1) * (2 * covariance + C2)) /
    ((referenceMean * referenceMean + candidateMean * candidateMean + C1) *
      (referenceVariance + candidateVariance + C2));
}

export function computeRawRgba8ImageMetrics(reference, candidate) {
  for (const [label, image] of [['reference', reference], ['candidate', candidate]]) {
    if (!Number.isSafeInteger(image?.width) || image.width <= 0 ||
        !Number.isSafeInteger(image?.height) || image.height <= 0 ||
        !Buffer.isBuffer(image?.rgba) || image.rgba.length !== image.width * image.height * 4) {
      throw new Error(`${label} is not a complete RGBA8 image`);
    }
  }
  if (reference.width !== candidate.width || reference.height !== candidate.height) {
    throw new Error(
      `image dimensions differ: ${reference.width}x${reference.height} vs ` +
      `${candidate.width}x${candidate.height}`
    );
  }
  const pixelCount = reference.width * reference.height;
  let rgbAbsoluteError = 0;
  let rgbSquaredError = 0;
  let alphaAbsoluteError = 0;
  let pixelsOverThree = 0;
  let maxRgbError = 0;
  for (let pixel = 0; pixel < pixelCount; pixel += 1) {
    const offset = pixel * 4;
    let pixelOverThree = false;
    for (let channel = 0; channel < 3; channel += 1) {
      const error = Math.abs(reference.rgba[offset + channel] - candidate.rgba[offset + channel]);
      rgbAbsoluteError += error;
      rgbSquaredError += error * error;
      maxRgbError = Math.max(maxRgbError, error);
      pixelOverThree ||= error > 3;
    }
    alphaAbsoluteError += Math.abs(reference.rgba[offset + 3] - candidate.rgba[offset + 3]);
    pixelsOverThree += Number(pixelOverThree);
  }
  const rgbMse8bit = rgbSquaredError / (pixelCount * 3);

  const scores = [];
  for (let top = 0; top < reference.height; top += WINDOW_SIZE) {
    for (let left = 0; left < reference.width; left += WINDOW_SIZE) {
      const referenceLuma = [];
      const candidateLuma = [];
      const bottom = Math.min(top + WINDOW_SIZE, reference.height);
      const right = Math.min(left + WINDOW_SIZE, reference.width);
      for (let y = top; y < bottom; y += 1) {
        for (let x = left; x < right; x += 1) {
          const offset = (y * reference.width + x) * 4;
          referenceLuma.push(
            0.2126 * reference.rgba[offset] +
            0.7152 * reference.rgba[offset + 1] +
            0.0722 * reference.rgba[offset + 2]
          );
          candidateLuma.push(
            0.2126 * candidate.rgba[offset] +
            0.7152 * candidate.rgba[offset + 1] +
            0.0722 * candidate.rgba[offset + 2]
          );
        }
      }
      scores.push(windowSsim(referenceLuma, candidateLuma));
    }
  }
  return {
    width: reference.width,
    height: reference.height,
    windowSize: WINDOW_SIZE,
    windowCount: scores.length,
    score: scores.reduce((sum, score) => sum + score, 0) / scores.length,
    rgbMae8bit: rgbAbsoluteError / (pixelCount * 3),
    rgbMaeNormalized: rgbAbsoluteError / (pixelCount * 3 * 255),
    rgbPsnrDb: rgbMse8bit === 0 ? null : 10 * Math.log10((255 * 255) / rgbMse8bit),
    maxRgbError8bit: maxRgbError,
    pixelsOver3Over255: pixelsOverThree,
    pixelsOver3Over255Fraction: pixelsOverThree / pixelCount,
    alphaMae8bit: alphaAbsoluteError / pixelCount,
    alphaMaeNormalized: alphaAbsoluteError / (pixelCount * 255)
  };
}

export function assertImageMetricSelfTest() {
  const same = windowSsim([0, 64, 128, 255], [0, 64, 128, 255]);
  const opposite = windowSsim([0, 0, 0, 0], [255, 255, 255, 255]);
  if (Math.abs(same - 1) > 1e-12 || !(opposite >= 0 && opposite < 0.001)) {
    throw new Error(`SSIM self-test failed: same=${same} opposite=${opposite}`);
  }
}
