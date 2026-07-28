import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  applyDiagnosticRasterContract,
  diagnosticArtifactClassification,
  DIAGNOSTIC_MECHANISM_EVIDENCE_CLASS,
  DIAGNOSTIC_MECHANISM_TERMINAL_STATUS,
  diagnosticRasterPolicyReceipt,
  GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT,
  pairingForDiagnosticContract,
  PLAYCANVAS_PINNED_RASTER_CONTRACT,
  resolveDiagnosticRasterContract,
  terminalStatusForDiagnosticContract
} from '../public/diagnostic-raster-contract.js';

const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const FRAGMENT = `
const EXP4: half = exp(half(-4.0));
const INV_EXP4: half = half(1.0) / (half(1.0) - EXP4);
fn normExp(x: half) -> half {
\treturn (exp(x * half(-4.0)) - EXP4) * INV_EXP4;
}
fn fragmentMain() {
\tif (A > half(1.0)) {
\t\tdiscard;
\t}
\tvar alpha: half = normExp(A) * gaussianColor.a;
\tif (alpha < half(uniform.alphaClipForward)) {
\t\t\tdiscard;
\t\t}
}`;

const HYBRID_VERTEX = `
fn vertexMain() {
\tlet clip = min(half(1.0), sqrt(max(half(0.0), log(alpha / alphaClipValue))) * half(0.5));
\tlet cornerClipped = cornerUV * f32(clip);
\tlet c = vec2f(proj.w) * uniform.viewport_size.zw;
\tlet pixelOffset = cornerClipped.x * v1 + cornerClipped.y * v2;
}`;

function registry() {
  const chunks = new Map([
    ['gsplatPS', FRAGMENT],
    ['gsplatHybridVS', HYBRID_VERTEX]
  ]);
  let reads = 0;
  let writes = 0;
  return {
    chunks,
    get reads() { return reads; },
    get writes() { return writes; },
    get(name) { reads += 1; return chunks.get(name); },
    set(name, value) { writes += 1; chunks.set(name, value); }
  };
}

test('default pinned contract is a true no-op', () => {
  const chunks = registry();
  const receipt = applyDiagnosticRasterContract(
    chunks,
    PLAYCANVAS_PINNED_RASTER_CONTRACT
  );
  assert.equal(receipt.actual, PLAYCANVAS_PINNED_RASTER_CONTRACT);
  assert.equal(receipt.diagnostic, false);
  assert.deepEqual(receipt.substitutions, []);
  assert.equal(chunks.reads, 0);
  assert.equal(chunks.writes, 0);
  assert.equal(chunks.chunks.get('gsplatPS'), FRAGMENT);
  assert.equal(chunks.chunks.get('gsplatHybridVS'), HYBRID_VERTEX);
});

test('Direct-f32 diagnostic applies five exact substitutions once', () => {
  const chunks = registry();
  const receipt = applyDiagnosticRasterContract(
    chunks,
    GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT
  );
  assert.equal(receipt.requested, GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT);
  assert.equal(receipt.actual, GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT);
  assert.equal(receipt.diagnostic, true);
  assert.deepEqual(receipt.semantics, {
    cached_axis_span_sigma: 2 * Math.SQRT2,
    physical_axis_span_sigma: 3,
    alpha_extent: 'sqrt(clamp(log(alpha*256)/4.5,0,1))',
    gaussian: 'exp(-4.5*A)',
    alpha_cap: 0.99,
    fragment_cutoff: 1 / 255,
    representation_precision: 'unchanged_pinned_playcanvas',
    chunk_installation_scope: 'global_pinned_wgsl_registry',
    admitted_render_pass_scope: 'forward_color_only',
    incompatible_passes: 'pick_shadow_prepass_not_admitted'
  });
  assert.deepEqual(receipt.substitutions, [
    { chunk: 'gsplatPS', count: 4 },
    { chunk: 'gsplatHybridVS', count: 1 }
  ]);
  assert.equal(chunks.reads, 2);
  assert.equal(chunks.writes, 2);

  const fragment = chunks.chunks.get('gsplatPS');
  assert.doesNotMatch(fragment, /normExp/);
  assert.doesNotMatch(fragment, /A > half\(1\.0\)/);
  assert.match(fragment, /min\(half\(0\.99\), gaussianColor\.a \* exp\(half\(-4\.5\) \* A\)\)/);
  assert.match(fragment, /alpha < half\(1\.0 \/ 255\.0\)/);
  const vertex = chunks.chunks.get('gsplatHybridVS');
  assert.match(vertex, /3\.0 \/ \(2\.0 \* sqrt\(2\.0\)\)/);
  assert.match(vertex, /sqrt\(clamp\(log\(f32\(alpha\) \* 256\.0\) \/ 4\.5, 0\.0, 1\.0\)\)/);

  assert.throws(
    () => applyDiagnosticRasterContract(chunks, GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT),
    /expected exactly one pinned shader anchor/
  );
});

test('diagnostic artifacts cannot become qualification or paired candidates', () => {
  assert.deepEqual(
    diagnosticArtifactClassification(GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT),
    {
      diagnostic: true,
      evidenceClass: DIAGNOSTIC_MECHANISM_EVIDENCE_CLASS,
      qualificationScope: DIAGNOSTIC_MECHANISM_EVIDENCE_CLASS,
      terminalStatus: DIAGNOSTIC_MECHANISM_TERMINAL_STATUS,
      pairingEligible: false
    }
  );
  const pairEnvironment = {
    pair_id: 'must-not-survive',
    run_order: 'playcanvas-first',
    position: 1
  };
  assert.equal(
    pairingForDiagnosticContract(
      GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT,
      pairEnvironment
    ),
    undefined
  );
  assert.equal(
    terminalStatusForDiagnosticContract(
      GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT,
      { qualification: true, hasPairing: true }
    ),
    DIAGNOSTIC_MECHANISM_TERMINAL_STATUS
  );
});

test('default artifacts retain existing qualification and pairing classification', () => {
  const pair = { pair_id: 'pair-1' };
  assert.equal(
    pairingForDiagnosticContract(PLAYCANVAS_PINNED_RASTER_CONTRACT, pair),
    pair
  );
  assert.equal(
    terminalStatusForDiagnosticContract(
      PLAYCANVAS_PINNED_RASTER_CONTRACT,
      { qualification: true, hasPairing: true }
    ),
    'valid_paired_candidate'
  );
  assert.equal(
    terminalStatusForDiagnosticContract(
      PLAYCANVAS_PINNED_RASTER_CONTRACT,
      { qualification: true, hasPairing: false }
    ),
    'valid_qualification_run'
  );
});

test('diagnostic receipt separates support input from effective fragment cutoff', () => {
  const chunks = registry();
  const receipt = applyDiagnosticRasterContract(
    chunks,
    GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT
  );
  const policy = diagnosticRasterPolicyReceipt(receipt, 1 / 256);
  assert.equal(policy.configured_support_alpha_clip_forward, 1 / 256);
  assert.equal(policy.effective_forward_fragment_cutoff, 1 / 255);
  assert.notEqual(
    policy.configured_support_alpha_clip_forward,
    policy.effective_forward_fragment_cutoff
  );
  assert.equal(policy.semantics.admitted_render_pass_scope, 'forward_color_only');
});

test('exact substitutions match the installed pinned PlayCanvas WGSL chunks', async () => {
  const runtimeSource = await readFile(
    resolve(harnessRoot, 'node_modules/playcanvas/build/playcanvas.mjs'),
    'utf8'
  );
  const extractChunk = (variable) => {
    const marker = `var ${variable} = \``;
    const start = runtimeSource.indexOf(marker);
    assert.notEqual(start, -1, `missing ${variable}`);
    const bodyStart = start + marker.length;
    const end = runtimeSource.indexOf('\`;', bodyStart);
    assert.notEqual(end, -1, `unterminated ${variable}`);
    return runtimeSource.slice(bodyStart, end);
  };
  const chunks = new Map([
    ['gsplatPS', extractChunk('gsplat_default3')],
    ['gsplatHybridVS', extractChunk('gsplatHybrid_default')]
  ]);
  const receipt = applyDiagnosticRasterContract(
    chunks,
    GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT
  );
  assert.deepEqual(receipt.substitutions, [
    { chunk: 'gsplatPS', count: 4 },
    { chunk: 'gsplatHybridVS', count: 1 }
  ]);
});

test('unknown diagnostic raster contracts fail closed', () => {
  assert.equal(resolveDiagnosticRasterContract(null), PLAYCANVAS_PINNED_RASTER_CONTRACT);
  assert.equal(resolveDiagnosticRasterContract(''), PLAYCANVAS_PINNED_RASTER_CONTRACT);
  assert.throws(
    () => resolveDiagnosticRasterContract('direct-ish'),
    /diagnostic_raster_contract must be one of/
  );
});

test('missing or duplicated pinned shader anchors fail before publication', () => {
  const missing = registry();
  missing.chunks.set('gsplatPS', 'different pinned source');
  assert.throws(
    () => applyDiagnosticRasterContract(missing, GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT),
    /gsplatPS normalization expected exactly one pinned shader anchor/
  );
  assert.equal(missing.writes, 0);

  const duplicated = registry();
  duplicated.chunks.set('gsplatHybridVS', `${HYBRID_VERTEX}\n${HYBRID_VERTEX}`);
  assert.throws(
    () => applyDiagnosticRasterContract(duplicated, GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT),
    /gsplatHybridVS footprint expected exactly one pinned shader anchor/
  );
  assert.equal(duplicated.writes, 0);
});
