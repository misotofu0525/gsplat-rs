import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import test from 'node:test';

const execFileAsync = promisify(execFile);
const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(harnessRoot, '..', '..', '..');
const validFixture = resolve(repoRoot, 'tests/perf/fixtures/v1/valid');
const validator = resolve(repoRoot, 'tests/perf/validate-benchmark-artifacts.py');
const runner = resolve(harnessRoot, 'scripts/run-timed-benchmark.mjs');

async function unavailableCountArtifact(root) {
  const artifact = resolve(root, 'artifact');
  await cp(validFixture, artifact, { recursive: true });

  const manifestPath = resolve(artifact, 'manifest.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  manifest.unavailable_fields.push(
    'frames[*].visible',
    'frames[*].contributor',
    'frames[*].drawn'
  );
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

  const framesPath = resolve(artifact, 'frames.jsonl');
  const frames = (await readFile(framesPath, 'utf8'))
    .trim()
    .split('\n')
    .map((line) => ({ ...JSON.parse(line), visible: null, drawn: null }));
  await writeFile(framesPath, `${frames.map((frame) => JSON.stringify(frame)).join('\n')}\n`);
  return { artifact, frames, framesPath };
}

test('PlayCanvas omits optional contributor evidence when the engine does not expose it', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'gsplat-playcanvas-counts-'));
  try {
    const { artifact } = await unavailableCountArtifact(root);
    await execFileAsync('python3', [validator, artifact], { cwd: repoRoot });

    const source = await readFile(runner, 'utf8');
    const frameStart = source.indexOf('const frames = outcome.result.capture.samples.map');
    const frameEnd = source.indexOf('const unavailableFields =', frameStart);
    assert.ok(frameStart >= 0 && frameEnd > frameStart, 'benchmark frame producer block moved');
    const frameProducer = source.slice(frameStart, frameEnd);
    assert.doesNotMatch(frameProducer, /^\s*contributor\s*:/m);
    assert.match(source.slice(frameEnd), /'frames\[\*\]\.contributor'/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('canonical validator rejects a lone null contributor field', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'gsplat-playcanvas-counts-'));
  try {
    const { artifact, frames, framesPath } = await unavailableCountArtifact(root);
    frames[0].contributor = null;
    await writeFile(framesPath, `${frames.map((frame) => JSON.stringify(frame)).join('\n')}\n`);

    await assert.rejects(
      execFileAsync('python3', [validator, artifact], { cwd: repoRoot }),
      /contributor and exact_contributor_compaction must be emitted together/
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
