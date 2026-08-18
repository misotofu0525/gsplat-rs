# Progress: GPU Compaction + Portable Order + Indirect Draw

## 2026-08-13

- Started ROADMAP item 3 after item 2 (`6328f30`) closed.
- Inventoried GPU-order encode, CPU near/far visibility, and the
  instance draw path.
- Replaced the serial 1-workgroup prefix with a 2-level hierarchical
  scan. Existing radix tests plus a 257-group / 2-block case pass.
- Added GPU visibility flags, compact scatter, and `order_meta` indirect
  args. Full-f32 keygen also applies NDC footprint when fov is set.
- Surface GPU-order project/draw consume those indirect args. CPU path
  unchanged. C ABI unchanged. GPU order is still experimental.
- Host evidence: `cargo test -p gsplat-render-wgpu resident_gpu_order`
  (7 tests) and `cargo clippy -p gsplat-render-wgpu --all-targets -- -D warnings`.
- Still open: CPU image parity, quantized footprint, Adaptive telemetry
  of GPU-visible counts, and the A065 / Web / Apple device ladder.

## 2026-08-13 (image parity)

- Shared `gpu_order_visibility.wgsl` across full-f32 and quantized keygen.
- Quantized keygen dequants covariance for the same fov-gated footprint.
- Test-only offscreen GPU-order render compares CPU `render_frame` vs
  GPU-order readback. Empty, near/far, screen-edge, degenerate cov,
  duplicate-depth, and degree-3 (full-f32 + quantized) all reported
  SSIM 1.0 and mean abs RGB 0.
- Host evidence: `cargo test -p gsplat-render-wgpu --lib gpu_order`
  (11 tests), `cargo clippy -p gsplat-render-wgpu --all-targets -- -D warnings`,
  `cargo check --workspace`.
- Still open: Adaptive telemetry of GPU-visible counts, and the A065 /
  Web / Apple device ladder. GPU order stays experimental.

## 2026-08-13 (A065 reconnect)

- Device `033ed212` / A065 / SM8475 / Adreno is attached again.
- Starting Kitsune paired CPU/GPU collection for the compact+indirect
  candidate: `target/android-sort-benchmarks/compact-v1-kitsune-i1`.
- Full 50k–700k Truck ladder is still later; this slice is the first
  true-device check on the corrected path.

## 2026-08-13 (A065 Kitsune pair complete)

- 6/6 runs valid, thermal 0 throughout, `gpu_sort_fallback_count=0`.
- CPU mean call 20.705 ms; GPU mean call 30.256 ms; ratio 1.461.
- Both miss all 80 frames vs the 16.67 ms budget.
- Conclusion for this scene: corrected GPU path works on Adreno 730
  and is still slower than CPU. Do not promote. Truck ladder and
  desktop/Web/Apple pairs remain open.

## 2026-08-13 (Truck ladder start)

- Fetched official Truck (`65ecf405…`, 2,541,226) and generated
  50k/100k/200k/300k/500k/700k tiers under
  `tests/datasets/external/ladder/truck/`.
- Collecting interval-1 paired CPU/GPU on A065 without rebuilding the
  APK: `target/android-sort-benchmarks/compact-v1-truck-i1-<tier>`.

## 2026-08-13 (Truck ladder complete)

- 6 tiers × 6 runs = 36 valid artifacts, thermal 0, zero GPU fallbacks.
- GPU slower at every tier (ratio 2.02 → 1.08). No crossover.
- CPU remains default. Desktop/Web/Apple pairs still open.

## 2026-08-13 (desktop Metal pairs + real-scene parity)

- Added experimental offscreen order-backend control: public Rust-only
  `Renderer::set_order_backend` and `bench-runner --order-backend`
  (`cpu`/`gpu`; Adaptive stays Surface-only). C ABI unchanged. GPU-order
  offscreen telemetry reports the source count for visible/drawn.
- New test `gpu_order_matches_cpu_image_on_real_scenes_when_present`:
  Kitsune and Flowers CPU vs GPU-order are exact on Metal (SSIM 1.0).
- Desktop M4 Pro Kitsune pairs (3×2, alternating, 120 frames, all
  artifacts valid): CPU median wall 2.291 ms, GPU 3.212 ms (1.40×).
  GPU order loses on desktop Metal too; CPU stays default.
- wasm32 `gsplat-web` check passes (pre-existing warnings only).
- Host evidence: `cargo test -p gsplat-render-wgpu -p bench-runner`
  (72 + conformance green), clippy `-D warnings` on both crates.
- Still open: browser (Web collector has no order-backend knob yet)
  and Apple Surface pairs.

## 2026-08-13 (Chrome WebGPU pairs)

- Wired the experimental order backend through the Web stack the same
  way as the storage profile: wasm `setOrderBackend`/`orderBackend`,
  `@gsplat-rs/web` `orderBackend` option + wrapper methods, example
  `gsplat_surface_order_backend`, collector `GSPLAT_ORDER_BACKEND`,
  manifest `order_backend_requested` (fail-closed check).
- Rebuilt the wasm package; wrapper unit tests, artifact tests,
  `pack:dry-run`, `node --check`, and `cargo check --workspace` green.
- Headless Chrome Kitsune pairs (3×2 alternating, 80+20 sync frames,
  all valid): CPU avg call 1.13–1.52 ms (sort 0.73–0.86 ms); GPU avg
  call ~0.06 ms with `sort_ms` exactly 0 (no fallback signature).
- Claim scope: WebGPU portability of compact+indirect proven; sync
  collector cannot see GPU-complete, so no browser winner claim.
- Remaining Phase 6 slice: Apple Surface pairs (iOS example has no
  order-backend extra yet).

## 2026-08-18 (Apple Surface pairs; task complete)

- Added the Apple hidden knob `gsplat_apple_benchmark_set_order_backend`
  (shared body with the Android one; `gsplat.h` unchanged), the iOS
  example `gsplat_surface_order_backend` launch extra, and
  `order_backend_requested` in the iOS artifact manifest/result line.
- iPhone 17 Pro Max Kitsune pairs (3×2): both backends vsync-capped at
  ~16.7–16.9 ms; GPU runs show zero CPU ordering cost, zero failures.
- Truck 700k pair: CPU 16.646 ms (holds 60 fps), GPU 17.885 ms (~56 fps).
  GPU/CPU 1.074 — smallest gap of all devices, still no crossover.
- Verdict recorded: item-3 device gate is complete on Android, desktop
  Metal, Chrome WebGPU, and Apple Surface. CPU radix stays the default;
  GPU order remains an experimental, image-exact, zero-fallback backend.
- Host checks: `cargo check` (host + aarch64-apple-ios), fmt clean.
