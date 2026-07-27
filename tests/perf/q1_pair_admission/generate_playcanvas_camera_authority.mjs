#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  canonicalPlayCanvasCameraOracle,
  createPlayCanvasCameraReceipt
} from '../../competitive/playcanvas/public/trace-camera.js';

const moduleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(moduleDirectory, '..', '..', '..');
const tracePath = resolve(
  repositoryRoot,
  'tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json'
);
const authorityPath = resolve(
  repositoryRoot,
  'tests/competitive/playcanvas/public/trace-camera.js'
);
const outputPath = resolve(
  moduleDirectory,
  'fixtures/playcanvas-truck-camera-receipts-v1.json'
);

function observationFromOracle(oracle) {
  return {
    position: oracle.position,
    forward: oracle.forward,
    up: oracle.up,
    verticalFovRadians: oracle.verticalFovRadians,
    nearPlane: oracle.nearPlane,
    farPlane: oracle.farPlane,
    aspect: oracle.aspect,
    horizontalFov: false,
    renderTargetFlipY: false,
    webGpuDepthRangeApplied: true,
    viewMatrixColumnMajor: oracle.playCanvas.viewMatrixColumnMajor,
    projectionMatrixOpenGlColumnMajor: oracle.playCanvas.projectionMatrixOpenGlColumnMajor,
    viewProjectionMatrixOpenGlColumnMajor:
      oracle.playCanvas.viewProjectionMatrixOpenGlColumnMajor,
    shaderProjectionMatrixWebGpuColumnMajor:
      oracle.playCanvas.shaderProjectionMatrixWebGpuColumnMajor,
    shaderViewProjectionMatrixWebGpuColumnMajor:
      oracle.playCanvas.shaderViewProjectionMatrixWebGpuColumnMajor
  };
}

const traceBytes = await readFile(tracePath);
const trace = JSON.parse(traceBytes);
const authorityBytes = await readFile(authorityPath);
const receipts = Object.fromEntries(trace.frames.map((_, traceFrameIndex) => {
  const oracle = canonicalPlayCanvasCameraOracle(trace, traceFrameIndex);
  return [String(traceFrameIndex), createPlayCanvasCameraReceipt({
    trace,
    traceFrameIndex,
    phase: 'authority_fixture',
    observation: observationFromOracle(oracle)
  })];
}));
const generated = `${JSON.stringify({
  schema: 'gsplat-playcanvas-camera-authority-fixture/v1',
  source_trace: {
    id: trace.trace_id,
    content_sha256: trace.content_sha256
  },
  authority: {
    path: 'tests/competitive/playcanvas/public/trace-camera.js',
    sha256: createHash('sha256').update(authorityBytes).digest('hex'),
    oracle_export: 'canonicalPlayCanvasCameraOracle',
    receipt_export: 'createPlayCanvasCameraReceipt'
  },
  receipts
}, null, 2)}\n`;

if (process.argv.includes('--check')) {
  const current = await readFile(outputPath, 'utf8');
  if (current !== generated) {
    throw new Error(`stale PlayCanvas camera authority fixture: ${outputPath}`);
  }
} else {
  await writeFile(outputPath, generated);
}
