export const PLAYCANVAS_PINNED_RASTER_CONTRACT = 'playcanvas_pinned_v1';
export const GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT =
  'gsplat_rs_direct_f32_footprint_v1';
export const DIAGNOSTIC_MECHANISM_EVIDENCE_CLASS = 'mechanism_attribution_only';
export const DIAGNOSTIC_MECHANISM_TERMINAL_STATUS = 'valid_mechanism_diagnostic';

const SUPPORTED_CONTRACTS = new Set([
  PLAYCANVAS_PINNED_RASTER_CONTRACT,
  GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT
]);

const FRAGMENT_NORMALIZATION = `const EXP4: half = exp(half(-4.0));
const INV_EXP4: half = half(1.0) / (half(1.0) - EXP4);
fn normExp(x: half) -> half {
	return (exp(x * half(-4.0)) - EXP4) * INV_EXP4;
}`;

const DIRECT_F32_FRAGMENT_NORMALIZATION = `// Diagnostic-only gsplat-rs Direct-f32 footprint contract.
// Color/SH representation precision and PlayCanvas sorting remain unchanged.`;

const FRAGMENT_ALPHA = 'var alpha: half = normExp(A) * gaussianColor.a;';
const DIRECT_F32_FRAGMENT_ALPHA =
  'var alpha: half = min(half(0.99), gaussianColor.a * exp(half(-4.5) * A));';

const UNIT_DISC_GUARD = `if (A > half(1.0)) {
\t\tdiscard;
\t}`;
const DIRECT_F32_UNIT_DISC_GUARD =
  '// Direct-f32 uses the alpha cutoff over the full scaled quad, not a unit disc.';

const FORWARD_CUTOFF = `if (alpha < half(uniform.alphaClipForward)) {
			discard;
		}`;
const DIRECT_F32_FORWARD_CUTOFF = `if (alpha < half(1.0 / 255.0)) {
			discard;
		}`;

const HYBRID_FOOTPRINT = `let clip = min(half(1.0), sqrt(max(half(0.0), log(alpha / alphaClipValue))) * half(0.5));
	let cornerClipped = cornerUV * f32(clip);
	let c = vec2f(proj.w) * uniform.viewport_size.zw;
	let pixelOffset = cornerClipped.x * v1 + cornerClipped.y * v2;`;

const DIRECT_F32_HYBRID_FOOTPRINT = `// The cache axes span 2*sqrt(2) sigma. Convert them to physical 3-sigma
	// support, then apply the Direct-f32 alpha-derived support fraction.
	let directF32AxisScale = 3.0 / (2.0 * sqrt(2.0));
	let directF32AlphaExtent = sqrt(clamp(log(f32(alpha) * 256.0) / 4.5, 0.0, 1.0));
	let cornerClipped = cornerUV * directF32AlphaExtent;
	let c = vec2f(proj.w) * uniform.viewport_size.zw;
	let pixelOffset = cornerClipped.x * (v1 * directF32AxisScale) +
		cornerClipped.y * (v2 * directF32AxisScale);`;

function replaceExactlyOnce(source, before, after, label) {
  const first = source.indexOf(before);
  const last = source.lastIndexOf(before);
  if (first < 0 || first !== last) {
    throw new Error(`${label} expected exactly one pinned shader anchor`);
  }
  return source.slice(0, first) + after + source.slice(first + before.length);
}

export function resolveDiagnosticRasterContract(rawValue) {
  const requested = rawValue === null || rawValue === undefined || rawValue === ''
    ? PLAYCANVAS_PINNED_RASTER_CONTRACT
    : String(rawValue).trim();
  if (!SUPPORTED_CONTRACTS.has(requested)) {
    throw new Error(
      `diagnostic_raster_contract must be one of: ${[...SUPPORTED_CONTRACTS].join(', ')}`
    );
  }
  return requested;
}

export function diagnosticArtifactClassification(requestedContract) {
  const contract = resolveDiagnosticRasterContract(requestedContract);
  const diagnostic = contract === GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT;
  return Object.freeze({
    diagnostic,
    evidenceClass: diagnostic ? DIAGNOSTIC_MECHANISM_EVIDENCE_CLASS : null,
    qualificationScope: diagnostic ? DIAGNOSTIC_MECHANISM_EVIDENCE_CLASS : null,
    terminalStatus: diagnostic ? DIAGNOSTIC_MECHANISM_TERMINAL_STATUS : null,
    pairingEligible: !diagnostic
  });
}

export function pairingForDiagnosticContract(requestedContract, pairing) {
  return diagnosticArtifactClassification(requestedContract).pairingEligible
    ? pairing
    : undefined;
}

export function terminalStatusForDiagnosticContract(
  requestedContract,
  { qualification, hasPairing }
) {
  const classification = diagnosticArtifactClassification(requestedContract);
  if (classification.terminalStatus) return classification.terminalStatus;
  if (hasPairing) return 'valid_paired_candidate';
  return qualification ? 'valid_qualification_run' : 'valid_collector_smoke';
}

export function diagnosticRasterPolicyReceipt(contractReceipt, configuredSupportInput) {
  if (!contractReceipt || typeof contractReceipt.diagnostic !== 'boolean' ||
      !Number.isFinite(configuredSupportInput) || configuredSupportInput <= 0) {
    throw new Error('diagnostic raster policy receipt requires a valid contract and support input');
  }
  const effectiveFragmentCutoff = contractReceipt.diagnostic
    ? contractReceipt.semantics?.fragment_cutoff
    : configuredSupportInput;
  if (!Number.isFinite(effectiveFragmentCutoff) || effectiveFragmentCutoff <= 0) {
    throw new Error('diagnostic raster policy receipt lacks an effective fragment cutoff');
  }
  return Object.freeze({
    ...contractReceipt,
    configured_support_alpha_clip_forward: configuredSupportInput,
    effective_forward_fragment_cutoff: effectiveFragmentCutoff
  });
}

export function applyDiagnosticRasterContract(shaderChunks, requestedContract) {
  const contract = resolveDiagnosticRasterContract(requestedContract);
  if (contract === PLAYCANVAS_PINNED_RASTER_CONTRACT) {
    return Object.freeze({
      schema: 'playcanvas-diagnostic-raster-contract/v1',
      requested: contract,
      actual: PLAYCANVAS_PINNED_RASTER_CONTRACT,
      diagnostic: false,
      shader_language: 'wgsl',
      semantics: null,
      substitutions: Object.freeze([])
    });
  }

  if (!shaderChunks || typeof shaderChunks.get !== 'function' ||
      typeof shaderChunks.set !== 'function') {
    throw new Error('diagnostic raster contract requires a mutable WGSL shader chunk registry');
  }

  const fragment = shaderChunks.get('gsplatPS');
  const hybridVertex = shaderChunks.get('gsplatHybridVS');
  if (typeof fragment !== 'string' || typeof hybridVertex !== 'string') {
    throw new Error('pinned PlayCanvas WGSL gsplat chunks are unavailable');
  }

  let patchedFragment = replaceExactlyOnce(
    fragment,
    FRAGMENT_NORMALIZATION,
    DIRECT_F32_FRAGMENT_NORMALIZATION,
    'gsplatPS normalization'
  );
  patchedFragment = replaceExactlyOnce(
    patchedFragment,
    FRAGMENT_ALPHA,
    DIRECT_F32_FRAGMENT_ALPHA,
    'gsplatPS alpha'
  );
  patchedFragment = replaceExactlyOnce(
    patchedFragment,
    UNIT_DISC_GUARD,
    DIRECT_F32_UNIT_DISC_GUARD,
    'gsplatPS unit-disc guard'
  );
  patchedFragment = replaceExactlyOnce(
    patchedFragment,
    FORWARD_CUTOFF,
    DIRECT_F32_FORWARD_CUTOFF,
    'gsplatPS forward cutoff'
  );
  const patchedHybridVertex = replaceExactlyOnce(
    hybridVertex,
    HYBRID_FOOTPRINT,
    DIRECT_F32_HYBRID_FOOTPRINT,
    'gsplatHybridVS footprint'
  );

  shaderChunks.set('gsplatPS', patchedFragment);
  shaderChunks.set('gsplatHybridVS', patchedHybridVertex);
  return Object.freeze({
    schema: 'playcanvas-diagnostic-raster-contract/v1',
    requested: contract,
    actual: GSPLAT_RS_DIRECT_F32_FOOTPRINT_CONTRACT,
    diagnostic: true,
    shader_language: 'wgsl',
    semantics: Object.freeze({
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
    }),
    substitutions: Object.freeze([
      Object.freeze({ chunk: 'gsplatPS', count: 4 }),
      Object.freeze({ chunk: 'gsplatHybridVS', count: 1 })
    ])
  });
}
