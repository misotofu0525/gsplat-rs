# Task Plan: Quantized Resident Storage + Compute Preprocess

## Goal

ROADMAP item 2: change the data plane. Run projection / covariance / SH
once per splat in compute, then add an explicit SPZ-aligned quantized
resident profile. Keep full-f32 as the default quality reference. Do not
touch the stable C ABI.

## Current Phase

Phase 7 complete

## Phases

### Phase 1: Layout and attachment points
- [x] Inventory current 64 B source + f32 SH, VS 6x project/SH
- [x] Choose projected-record layout and compute bind-group budget
- **Status:** complete

### Phase 2: Compute preprocess on full-f32 (default path)
- [x] Shared WGSL project/SH; compute writes `ProjectedRecord`
- [x] Thin VS/FS consume projected records by instance index
- [x] Same-encoder compute then draw on offscreen and Surface (CPU + GPU order)
- [x] Preflight accounts for the projected buffer
- **Status:** complete

### Phase 3: Quantized resident profile
- [x] CPU pack/unpack aligned with SPZ (f16 pos, smallest-three rot, log-u8 scale, u8 SH)
- [x] Explicit `ResidentStorageProfile::{FullF32, Quantized}`; default FullF32
- [x] GPU decode in the preprocess compute shader
- [x] Preflight: 1M degree-3 fits 128 MiB
- [x] Per-degree SH sidecar split
- [x] Real-scene SSIM / capacity / first-frame evidence
- **Status:** complete

### Phase 4: Docs and verification
- [x] Handbook / CHANGELOG / crate README
- [x] Conformance, clippy, rustdoc, FFI smoke
- **Status:** complete

### Phase 5: Leftovers (sidecars, GPU order, evidence)
- [x] Raise Surface/offscreen `max_storage_buffers_per_shader_stage` to 8 when the adapter allows
- [x] Split quantized SH into degree 1-4 sidecars (7 storage buffers)
- [x] Quantized GPU-order keygen from packed f16 positions
- [x] Degree-3 synthetic SSIM + Kitsune/Flowers when present
- [x] `bench-runner --storage-profile quantized`
- **Status:** complete

### Phase 6: Android true-device evidence
- [x] Hidden sample-only storage-profile knob (not C ABI)
- [x] Surface resident rebuild after profile change
- [x] Collector `--storage-profile full-f32|quantized`
- [x] A065 Kitsune CPU-order artifacts (plus GPU-order quantized)
- **Status:** complete

### Phase 7: Web/WASM shader coverage
- [x] Create-time `ResidentStorageProfile` on `gsplat-web` (not C ABI)
- [x] Example/collector `gsplat_surface_storage_profile`
- [x] Chrome paired first-frame/sustained artifacts
- **Status:** complete

## Key Questions

1. Dispatch vs downlevel 4 storage buffers/stage? Quantized now requires 7
   and requests WebGPU's 8 when the adapter exposes them. Full-f32 still
   binds 4. Adapters that only have 4 fail quantized preflight explicitly.
2. C ABI? Unchanged. Profile is a Rust-only renderer option.
3. Empty stage traits? No. Compute preprocess attaches at the existing
   resident encode point.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Projected record is 48 B f32 (geometry + RGBA) | Keeps SortedAlpha pixel conformance; 1M still fits 128 MiB |
| Compute gathers by sorted instance index | Composes with CPU sort cadence; VS does not reread source/SH |
| Quantized SH is four per-degree sidecars | Largest degree-3 binding is 21 B/splat; needs 7 storage buffers |
| Default profile remains FullF32 | Quality reference; C ABI and current tests stay on this path |
| GPU order keygen is profile-specialized | Radix stays shared; only position unpack differs |
| Android profile knob is sample-only | Same pattern as `set_order_backend`; published C ABI unchanged |
| Web profile is create-time only | Avoids blocking GPU poll on wasm; same tokens as Android |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
|       | 1       |            |
