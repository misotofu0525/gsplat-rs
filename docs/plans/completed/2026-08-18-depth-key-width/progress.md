# Progress: 16-bit Depth-Key Experiment

## 2026-08-18 (session resume)

- Recovered after the previous turn was interrupted mid-test.
- Phase 1 core knob was already wired:
  - `gsplat-sort`: `radix_sort_desc_u64_key_high16` (two 8-bit passes)
    and `CpuSortBackend::sort_values_by_key_high_bits`
  - GPU order: `first_radix_pass = 4` when `DepthKeyBits::High16`
  - `Renderer::set_depth_key_bits` + `GSPLAT_DEPTH_KEY_BITS` build default
  - `bench-runner --depth-key-bits {32,16}`
- Moved `set_depth_key_bits` before `load_scene` so the first resident
  create uses the requested width.
- Extended the host image gate to Flowers (skip-if-missing), matching
  the existing Kitsune/Flowers GPU-order parity loop.
- Next: run unit + image + clippy/fmt, then desktop Metal paired perf.

## 2026-08-18 (host image)

- `cargo test -p gsplat-sort -p bench-runner`: 13 + 17 passed, including
  `high_bit_sort_*` and `bench_config_parse_depth_key_bits`.
- GPU high16 unit test: truncated keys break 2.0/2.001 ties by id.
- Image: synthetic exact; Kitsune 16-vs-32 SSIM 0.987 (misses 0.99);
  Flowers 0.996; CPU-16 vs GPU-16 exact on all scenes.
- Promotion image gate failed on Kitsune. Still collecting desktop
  Metal {cpu,gpu}×{32,16} so the speed/quality tradeoff is complete.

## 2026-08-18 (desktop Metal 4-way)

- `cargo fmt --check`, `clippy -D warnings`, `cargo check --workspace` green.
- Kitsune 120+10 on M4 Pro Metal, artifacts valid:
  CPU sort 0.483 → 0.245 ms; CPU complete 3.876 → 3.404;
  GPU complete 4.973 → 3.875. GPU-16 still slower than CPU-16.
- Image kill stands. Mobile 16-bit pairs deferred unless requested.

## 2026-08-18 (weighted 16-bit keys)

- Replaced IEEE truncation with PlayCanvas 32-bin camera-relative
  weights. Range is scene AABB view-Z, shared by CPU and GPU.
- Unit: 2.0 vs 2.001 stay ordered; params layout 384 B / bins at 128.
- Image: Kitsune SSIM 1.0; Flowers 0.999997; cpu16 vs gpu16 exact.
- Desktop 4-way: CPU sort 0.449 → 0.238 ms; complete 2.218 → 1.887;
  GPU 3.124 → 2.080. Clippy `-D warnings` green.
- Image gate now passes. CPU stays default; 16-bit is a viable
  experimental width. Mobile A/B still optional.

## 2026-08-18 (device 4-way wiring)

- Added hidden Android/iOS extras for live 32/16 selection so one
  APK/IPA can collect `{cpu,gpu}×{32,16}` without two native builds.

## 2026-08-18 (A065 + iPhone Kitsune 4-way)

- A065 12/12 validated, thermal 0:
  GPU-16 30.2→24.7 ms call; CPU-16 sort 9.36→8.24 but e2e 20.8→21.1.
- iPhone 17 Pro Max 12/12 validated, vsync-capped ~16.7 ms;
  CPU sort 4.55→3.64. CPU 32-bit stays default.

## 2026-08-18 (Truck 200k / 700k 4-way)

- A065 Truck 200k: CPU-16 sort 7.07→5.87, e2e 16.35→16.70 (worse).
  GPU-16 24.09→20.11, still 1.23× CPU-32.
- A065 Truck 700k: CPU-16 sort 20.44→17.75, e2e 270.9→278.7 (worse).
  GPU-16 median 286.7 vs GPU-32 294.8 vs CPU-32 270.9.
- iPhone Truck 700k: GPU-32 17.77 misses 60 fps; GPU-16 16.66 holds
  the cap and matches CPU-32. CPU stays default.

## 2026-08-18 (close-out)

- Demoted: deleted `DepthKeyBits`, weighted remap, high-16 radix entry,
  hidden Android/iOS extras, and bench/collector `--depth-key-bits`.
- Default remains CPU + 32-bit IEEE keys. GPU compact+indirect stays
  experimental. `sort_interval=2` / `async_sort=false` unchanged.
- Bundle archived to `docs/plans/completed/2026-08-18-depth-key-width/`.
