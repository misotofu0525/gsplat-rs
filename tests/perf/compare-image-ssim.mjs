#!/usr/bin/env node
import { access, readFile, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import process from 'node:process';
import {
  browserOwnershipConfig,
  publishBrowserOwnershipHandshake,
} from './browser-process-ownership.mjs';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, '..', '..');
const playcanvasRoot = resolve(repoRoot, 'tests/competitive/playcanvas');
const chromeCandidates = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium'
].filter(Boolean);

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function findChrome() {
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {}
  }
  return null;
}

async function loadPuppeteer() {
  const candidates = [
    resolve(playcanvasRoot, 'node_modules/puppeteer-core/lib/esm/puppeteer/puppeteer-core.js'),
    resolve(playcanvasRoot, 'node_modules/puppeteer-core/lib/cjs/puppeteer/puppeteer-core.js'),
    resolve(playcanvasRoot, 'node_modules/puppeteer-core/index.js')
  ];
  for (const candidate of candidates) {
    if (await pathExists(candidate)) {
      const module = await import(pathToFileURL(candidate).href);
      return module.default ?? module;
    }
  }
  throw new Error('puppeteer-core is unavailable; run npm ci under tests/competitive/playcanvas');
}

function parseArguments(argv) {
  const positional = [];
  let threshold = null;
  let maxRgbMaeNormalized = null;
  let maxPixelsOver3Fraction = null;
  let requireAlphaExact = false;
  let output = null;
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === '--threshold') {
      threshold = Number(argv[++index]);
    } else if (argv[index] === '--max-rgb-mae-normalized') {
      maxRgbMaeNormalized = Number(argv[++index]);
    } else if (argv[index] === '--max-pixels-over3-fraction') {
      maxPixelsOver3Fraction = Number(argv[++index]);
    } else if (argv[index] === '--require-alpha-exact') {
      requireAlphaExact = true;
    } else if (argv[index] === '--output') {
      output = resolve(argv[++index]);
    } else {
      positional.push(argv[index]);
    }
  }
  if (positional.length !== 2) {
    throw new Error('usage: compare-image-ssim.mjs <reference.png> <candidate.png> [--threshold 0.9999] [--max-rgb-mae-normalized 0.00005] [--max-pixels-over3-fraction 0.001] [--require-alpha-exact] [--output result.json]');
  }
  if (threshold !== null && (!Number.isFinite(threshold) || threshold < -1 || threshold > 1)) {
    throw new Error('--threshold must be a finite number between -1 and 1');
  }
  for (const [name, value] of [
    ['--max-rgb-mae-normalized', maxRgbMaeNormalized],
    ['--max-pixels-over3-fraction', maxPixelsOver3Fraction]
  ]) {
    if (value !== null && (!Number.isFinite(value) || value < 0 || value > 1)) {
      throw new Error(`${name} must be a finite number between 0 and 1`);
    }
  }
  return {
    reference: resolve(positional[0]),
    candidate: resolve(positional[1]),
    threshold,
    maxRgbMaeNormalized,
    maxPixelsOver3Fraction,
    requireAlphaExact,
    output
  };
}

const args = parseArguments(process.argv.slice(2));
const [referenceBytes, candidateBytes, puppeteer, chrome] = await Promise.all([
  readFile(args.reference),
  readFile(args.candidate),
  loadPuppeteer(),
  findChrome()
]);
if (!chrome) throw new Error(`no supported Chrome/Chromium found: ${chromeCandidates.join(', ')}`);

const chromeBytes = await readFile(chrome);
const browserIdentity = {
  executablePath: resolve(chrome),
  sha256: createHash('sha256').update(chromeBytes).digest('hex')
};
const browserOwnership = browserOwnershipConfig(process.env, false);
const browserLaunchOptions = {
  executablePath: browserIdentity.executablePath,
  headless: true,
};
if (browserOwnership !== null) {
  browserLaunchOptions.userDataDir = browserOwnership.userDataDir;
}
const browser = await puppeteer.launch(browserLaunchOptions);
try {
  await publishBrowserOwnershipHandshake(browser, browserOwnership);
  const page = await browser.newPage();
  const result = await page.evaluate(async ({ referenceBase64, candidateBase64 }) => {
    const WINDOW_SIZE = 8;
    const C1 = (0.01 * 255) ** 2;
    const C2 = (0.03 * 255) ** 2;

    function selfTest() {
      const same = windowSsim([0, 64, 128, 255], [0, 64, 128, 255]);
      const opposite = windowSsim([0, 0, 0, 0], [255, 255, 255, 255]);
      if (Math.abs(same - 1) > 1e-12 || !(opposite >= 0 && opposite < 0.001)) {
        throw new Error(`SSIM self-test failed: same=${same} opposite=${opposite}`);
      }
    }

    function windowSsim(a, b) {
      const count = a.length;
      let sumA = 0;
      let sumB = 0;
      for (let index = 0; index < count; index += 1) {
        sumA += a[index];
        sumB += b[index];
      }
      const meanA = sumA / count;
      const meanB = sumB / count;
      let varianceA = 0;
      let varianceB = 0;
      let covariance = 0;
      for (let index = 0; index < count; index += 1) {
        const deltaA = a[index] - meanA;
        const deltaB = b[index] - meanB;
        varianceA += deltaA * deltaA;
        varianceB += deltaB * deltaB;
        covariance += deltaA * deltaB;
      }
      const denominator = Math.max(count - 1, 1);
      varianceA /= denominator;
      varianceB /= denominator;
      covariance /= denominator;
      return ((2 * meanA * meanB + C1) * (2 * covariance + C2)) /
        ((meanA * meanA + meanB * meanB + C1) * (varianceA + varianceB + C2));
    }

    async function decode(base64) {
      const image = new Image();
      image.src = `data:image/png;base64,${base64}`;
      await image.decode();
      const canvas = document.createElement('canvas');
      canvas.width = image.naturalWidth;
      canvas.height = image.naturalHeight;
      const context = canvas.getContext('2d', { willReadFrequently: true });
      context.drawImage(image, 0, 0);
      return { width: canvas.width, height: canvas.height, rgba: context.getImageData(0, 0, canvas.width, canvas.height).data };
    }

    selfTest();
    const [reference, candidate] = await Promise.all([decode(referenceBase64), decode(candidateBase64)]);
    if (reference.width !== candidate.width || reference.height !== candidate.height) {
      throw new Error(`image dimensions differ: ${reference.width}x${reference.height} vs ${candidate.width}x${candidate.height}`);
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
              0.2126 * reference.rgba[offset] + 0.7152 * reference.rgba[offset + 1] + 0.0722 * reference.rgba[offset + 2]
            );
            candidateLuma.push(
              0.2126 * candidate.rgba[offset] + 0.7152 * candidate.rgba[offset + 1] + 0.0722 * candidate.rgba[offset + 2]
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
  }, {
    referenceBase64: referenceBytes.toString('base64'),
    candidateBase64: candidateBytes.toString('base64')
  });

  const checks = {
    ssim: args.threshold === null ? null : result.score >= args.threshold,
    rgbMae: args.maxRgbMaeNormalized === null
      ? null
      : result.rgbMaeNormalized <= args.maxRgbMaeNormalized,
    rgbTail: args.maxPixelsOver3Fraction === null
      ? null
      : result.pixelsOver3Over255Fraction <= args.maxPixelsOver3Fraction,
    alphaExact: args.requireAlphaExact ? result.alphaMae8bit === 0 : null
  };
  const configuredChecks = Object.values(checks).filter((value) => value !== null);
  const output = {
    schema: 'gsplat-image-parity/v1',
    metric: 'ssim-luma-srgb-window8',
    browser: browserIdentity,
    constants: { k1: 0.01, k2: 0.03, dynamicRange: 255 },
    reference: args.reference,
    candidate: args.candidate,
    ...result,
    threshold: args.threshold,
    maxRgbMaeNormalizedThreshold: args.maxRgbMaeNormalized,
    maxPixelsOver3FractionThreshold: args.maxPixelsOver3Fraction,
    requireAlphaExact: args.requireAlphaExact,
    checks,
    pass: configuredChecks.length === 0 ? null : configuredChecks.every(Boolean)
  };
  if (args.output) await writeFile(args.output, `${JSON.stringify(output, null, 2)}\n`);
  console.log(JSON.stringify(output));
  if (output.pass === false) process.exitCode = 1;
} finally {
  await browser.close();
}
