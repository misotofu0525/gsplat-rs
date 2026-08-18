# Task Plan: GPU Compaction, Portable Order, Indirect Draw

## Goal

ROADMAP item 3: GPU frustum/footprint visibility, compact visible `(key, id)`
without CPU readback, hierarchical prefix scan, sort only the compacted
count, and drive sort-dispatch plus draw from GPU-written indirect args.
Keep CPU radix as the default. Do not widen the C ABI. Do not promote GPU
order from a kernel microbenchmark.

## Current Phase

Complete (2026-08-18). All six phases closed. Every end-to-end device
measurement keeps CPU radix as the default; the compact+indirect GPU
path stays in tree as a correct, portable experimental backend.

## Phases

### Phase 1: Inventory and attachment points
- [x] Map current GPU-order encode, draw, and CPU visibility contract
- [x] Choose hierarchical scan layout that histogram/scatter can keep
- [x] Record PlayCanvas portable 4-bit + hierarchical scan reference
- **Status:** complete

### Phase 2: Hierarchical prefix scan
- [x] Replace the serial 1-workgroup `prefix` with a 2-level scan
- [x] Keep 4-bit LSD radix, descending key, stable ID ties
- [x] Extend radix tests to cover more than one scan block
- **Status:** complete

### Phase 3: GPU visibility + compact scatter
- [x] Near/far plus full-f32 NDC footprint (fov-gated so unit tests stay near/far)
- [x] Compact visible `(key, id)` with a 2-level flag scan; no CPU readback
- [x] Sort only the compacted visible count
- **Status:** complete

### Phase 4: Indirect sort dispatch and draw
- [x] Write sort-dispatch and draw-indirect args from the compacted count
- [x] Record into the caller-owned encoder; same-frame CPU fallback stays
- [x] Empty-visible and near/far / footprint unit coverage
- **Status:** complete

### Phase 5: Image parity and docs
- [x] Handbook / CHANGELOG / shader README; Adaptive still opt-in
- [x] Shared NDC footprint for full-f32 and quantized keygen
- [x] CPU image parity for empty, near/far, screen-edge, degenerate cov, ties
- [x] Quantized CPU vs quantized GPU image parity on the degree-3 fixture
- **Status:** complete

### Phase 6: Device evidence
- [x] Inventory hosts/devices/datasets (2026-08-13)
- [x] A065 Kitsune paired CPU/GPU (full-f32, interval 1, 3 reps)
- [x] Desktop Metal paired CPU/GPU offscreen artifacts (Kitsune)
- [x] Real-scene CPU vs GPU-order image parity (Kitsune/Flowers, Metal)
- [x] Re-run A065 Truck 50k–700k ladder
- [x] wasm32 compile check on the shared Surface path
- [x] Chrome WebGPU Kitsune pairs (order-backend knob wired end to end)
- [x] Apple paired Surface artifacts (iPhone 17 Pro Max, Kitsune + Truck 700k)
- **Status:** complete

## Key Questions

1. Default path? Unchanged. GPU order stays experimental.
2. C ABI? Unchanged. No storage/order/indirect knobs in `gsplat.h`.
3. Visibility vs CPU? CPU is near/far only. GPU full-f32 and quantized
   keygen share NDC footprint when fov/width/height are set. Off-screen
   CPU `invalid_splat` quads contribute no pixels, so images still match.
4. OneSweep / subgroups? Capability-gated experiments only; not this track.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Hierarchical scan lands first | Shared by full-scene radix and compact |
| 2-level scan, block = 256 groups | Covers TILE_SIZE² groups (~16M items); no subgroups |
| Compact scratch merges flags + tile sums | Stays at 4 storage buffers/stage (downlevel) |
| Project bind group unchanged | Quantized already uses 7 storage; last-tile over-read is unused |
| Indirect draw/dispatch from `order_meta` | No CPU readback; CPU path stays a normal instance draw |
| Do not promote GPU order here | ROADMAP requires end-to-end device + image gates |
| First A065 Kitsune pair keeps CPU default | GPU compact+indirect ran with 0 fallbacks but was 1.46× slower |
| Truck ladder keeps CPU default | GPU slower at all 6 tiers; no crossover before the 700k cap |
| Desktop Metal pairs keep CPU default | Kitsune GPU wall 3.21 ms vs CPU 2.29 ms (1.40×) on M4 Pro |
| Offscreen `--order-backend` is bench-only | Rust-only experimental knob on `Renderer`; C ABI unchanged |
| Web order-backend mirrors the storage-profile extras | wasm `setOrderBackend`, example query param, collector env; no stable Web API claim |
| Web sync pairs prove portability only | Collector measures CPU call walls; GPU-complete is unobservable there |
| Apple hidden knob mirrors the Android one | `gsplat_apple_benchmark_set_order_backend`, iOS example launch extra; `gsplat.h` unchanged |
| Apple pairs keep CPU default | Kitsune both vsync-capped; Truck 700k CPU holds 60 fps, GPU 17.9 ms |
| Shared `gpu_order_visibility.wgsl` | Same NDC test for full-f32 and quantized keygen |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| Compact BGL needed 5 storage buffers; downlevel limit is 4 | 1 | Merge flags + tile sums into one scratch buffer |
