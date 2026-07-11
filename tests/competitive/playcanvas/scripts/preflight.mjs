import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

function requireEqual(actual, expected, label) {
  assert.equal(actual, expected, `${label}: expected ${expected}, received ${actual}`);
}

const expected = await readJson(resolve(harnessRoot, 'expected-engine.json'));
const packageJson = await readJson(resolve(harnessRoot, 'package.json'));
const lock = await readJson(resolve(harnessRoot, 'package-lock.json'));

requireEqual(packageJson.dependencies?.[expected.packageName], expected.version, 'package.json dependency');
requireEqual(lock.lockfileVersion, 3, 'package-lock lockfileVersion');
requireEqual(lock.packages?.['']?.dependencies?.[expected.packageName], expected.version, 'package-lock root dependency');

const lockedEngine = lock.packages?.[`node_modules/${expected.packageName}`];
assert.ok(lockedEngine, `package-lock is missing node_modules/${expected.packageName}`);
requireEqual(lockedEngine.version, expected.version, 'locked PlayCanvas version');
requireEqual(lockedEngine.integrity, expected.integrity, 'locked PlayCanvas integrity');
requireEqual(lockedEngine.license, expected.license, 'locked PlayCanvas license');
assert.match(lockedEngine.resolved ?? '', /^https:\/\/registry\.npmjs\.org\/playcanvas\/-\/playcanvas-[^/]+\.tgz$/, 'locked PlayCanvas tarball must come from the npm registry');

const installedPackage = await readJson(resolve(harnessRoot, 'node_modules', expected.packageName, 'package.json'));
requireEqual(installedPackage.name, expected.packageName, 'installed package name');
requireEqual(installedPackage.version, expected.version, 'installed PlayCanvas version');
requireEqual(installedPackage.license, expected.license, 'installed PlayCanvas license');

const runtime = await import(expected.packageName);
requireEqual(runtime.version, expected.version, 'PlayCanvas runtime version');
requireEqual(runtime.revision, expected.runtimeRevision, 'PlayCanvas runtime revision');
assert.ok(expected.revision.startsWith(`${runtime.revision}`), 'full expected revision must extend the runtime revision');

console.log(JSON.stringify({
  status: 'pass',
  engine: expected.packageName,
  version: runtime.version,
  revision: expected.revision,
  runtimeRevision: runtime.revision,
  license: installedPackage.license,
  integrity: lockedEngine.integrity
}));
