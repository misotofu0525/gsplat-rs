# Task Plan: 16-bit Depth-Key Experiment (CPU + GPU)

## Goal

Test whether a PlayCanvas-style 16-bit depth key (map camera Z into the
scene range, 32 bins, more buckets near the camera) makes ordering
cheaper without breaking SortedAlpha image quality. Same width on CPU
and GPU: CPU radix 4 passes → 2, GPU radix 8 passes → 4.

Naive IEEE truncation was measured first and missed the Kitsune SSIM
gate. This revision replaces truncation with weighted range mapping.

## Current Phase

Complete and demoted (2026-08-18). Image gate passed for weighted keys;
CPU end-to-end never won on device; GPU-16 never beat CPU-32. The
experimental width was deleted from the tree. CPU 32-bit IEEE keys
remain the default. Evidence stays in this bundle.

## Kill criteria (pre-registered)

- Image gate: CPU-16 vs CPU-32 and GPU-16 vs GPU-32 must hold
  SSIM ≥ 0.99 on the synthetic + real-scene parity fixtures. A visible
  ordering artifact kills the candidate regardless of speed.
- Perf gate: if 16-bit keys do not improve the loser's end-to-end time
  by a measurable margin on at least one device tier, record and stop.
- No new stable API: Rust-only experimental setting; C ABI, JNI AAR, Web
  API unchanged. Mobile A/B uses hidden sample extras matching
  `gsplat_surface_order_backend`, not a published header knob.

## Phases

### Phase 1: Core knob
- [x] `gsplat-sort`: high-16 key-bit sort entry (2 passes)
- [x] GPU order: start radix at shift 16 when 16-bit keys selected
- [x] `Renderer::set_depth_key_bits` (32|16) + build-default override
- **Status:** complete

### Phase 2: Host evidence
- [x] Unit: truncated-key order matches full-key order up to bf16 ties
- [x] Image parity: synthetic + Kitsune/Flowers, both backends, 16 vs 32
- [x] clippy / fmt / workspace check
- **Status:** complete

Kitsune 16-vs-32 SSIM 0.987 misses the promotion gate; Flowers 0.996
passes SSIM. CPU-16 vs GPU-16 is exact. `cargo fmt --check`,
`clippy -D warnings`, and `cargo check --workspace` passed.

### Phase 3: Paired perf
- [x] Desktop Metal Kitsune: {cpu,gpu} × {32,16}
- [x] A065 + iPhone Kitsune 4-way (3 reps, interval 1, thermal 0)
- [x] A065 Truck 200k + 700k, iPhone Truck 700k 4-way
- **Status:** complete

Desktop: CPU sort halves; GPU-complete improves; GPU-16 still loses to
CPU-16. A065: GPU-16 cuts GPU call 30.2→24.7 ms but still loses to
CPU-32 (20.8). CPU-16 sort 9.36→8.24 but e2e is slightly worse.
iPhone Kitsune is vsync-capped; CPU sort 4.55→3.64. CPU stays default.

### Phase 4: PlayCanvas-style weighted 16-bit keys
- [x] Shared 32-bin camera-relative weights (CPU + GPU keygen)
- [x] Range from scene AABB view-Z, identical on both backends
- [x] Host: nearby depths stay ordered; Kitsune/Flowers 16-vs-32 SSIM
- **Status:** complete

Kitsune SSIM 1.0; Flowers 0.999997. Desktop CPU sort still halves.
Mobile Kitsune 4-way collected on A065 and iPhone 17 Pro Max.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Truncate existing f32 keys, no re-quantization | Tried first; Kitsune SSIM 0.987. Replaced by weighted range mapping. |
| PlayCanvas 32-bin camera-relative weights | Near bins get more of the 16-bit budget; same helper on CPU and GPU |
| Scene AABB view-Z as the range | CPU and GPU share one range without a GPU reduction |
| Same width on CPU and GPU | Fair A/B; ties resolved identically (stable, ascending id) |
| Mobile A/B via hidden extras | One APK/IPA, live 32/16 switch; same pattern as order-backend. Build-time `GSPLAT_DEPTH_KEY_BITS` remains the process default. |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
