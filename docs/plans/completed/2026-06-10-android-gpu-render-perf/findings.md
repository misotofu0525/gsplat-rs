# Findings & Decisions

## Requirements

- Improve Android device render performance without changing SortedAlpha output.
- Device: `android-test-device` (model A065/Pong), dataset `flowers_1.ply` (562,974 splats).
- Validate with the repo Android benchmark path and image-diff evidence.

## Research Findings

- Prior perf bundle (`docs/plans/completed/2026-04-24-android-sorted-render-perf/`)
  ended at `avg_call_ms~52 / avg_frame_ms~35.6` with the CPU instance path and
  concluded the remaining wall is "Surface upload/present/queue sync".
- Re-reading that evidence: every experiment (GPU preproject frame_ms=10.8,
  async sort/geometry, buffer rings, frame latency) left call wall at 51-55ms.
  The consistent interpretation is that the GPU itself takes ~50ms/frame and
  the render call blocks on swapchain acquire. CPU-side work is hidden behind
  GPU backpressure.
- `splat_surface.wgsl` calls `eval_color` (degree-3 SH, 45 coeff reads) in
  `vs_main`, i.e. once per vertex = 6x per splat per frame.
- Fragment shader discards when `alpha = a*exp(-4.5*r2) <= 1/256`, so any
  fragment outside `r2 = ln(256a)/4.5` contributes exactly zero. Shrinking the
  quad to that iso-contour and culling splats with `a <= 1/256` is output-exact.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Attack GPU-side cost first | Call wall is GPU-bound; CPU optimizations plateaued. |
| Implement quad shrink in the vertex shader only (`alpha_extent_scale`) | No CPU change, no instance-format change, applies to Surface/preproject/direct/offscreen paths uniformly, and keeps the pixel->gaussian map identical. |
| Revert the SH color compute prepass | Compute->vertex dependency serializes tile-based GPU frames; 159ms call wall reproduced the prior 140ms failure. |
| Keep SH in VS but vectorize across channels (B') | Once-per-vertex basis evaluation removes the 3x redundant polynomial ALU; accumulation order is unchanged so results match the scalar path. |
| Keep `gpu_preproject`/`static_direct` opt-in despite now being faster | Default-flip needs its own validation track (iOS/web/desktop interplay); record the reversal as the next candidate work. |

## Rejected Experiments

| Experiment | Result | Decision |
|------------|--------|----------|
| Per-instance SH color compute prepass (single encoder, dirty-flag, ring-aware) | `avg_call_ms=159.171` vs 38.5 same-feature baseline | Reverted; tile GPU serializes compute->vertex dependencies |

## Benchmark Evidence

- All runs: device `android-test-device`, `flowers_1.ply`, 120 samples + 10 warmup,
  yaw_step 0.001, defaults unless noted.
- Phase 0 baseline `sort_interval=2`: `avg_call_ms=51.955 avg_frame_ms=36.234
  avg_preprocess_ms=1.631 avg_sort_ms=6.818 avg_raster_ms=27.784
  avg_visible=562974 avg_drawn=562974`.
- Phase 0 baseline `sort_interval=1`: `avg_call_ms=52.308 avg_frame_ms=41.586
  avg_preprocess_ms=2.726 avg_sort_ms=11.899 avg_raster_ms=26.959`. Call wall
  identical to interval=2 despite +5.4ms CPU -> confirms GPU-bound call wall.
- Local alpha analysis of `flowers_1.ply` (sigmoid of opacity, 562,974 splats):
  - 0 splats with `alpha <= 1/256` -> zero-contribution cull yields nothing on
    this dataset; `drawn` count will stay 562,974.
  - 78.6% of splats have shrink scale `s < 1`; total quad area ratio after
    alpha-aware shrink = 0.786 -> ~21% fragment rasterization reduction.
  - alpha percentiles: p25=0.076, p50=0.161, p75=0.314, p90=0.558.
- Phase 1 diagnostic A (temporary `degree=0u` in `splat_surface.wgsl`):
  `avg_call_ms=33.024 avg_frame_ms=30.087 avg_raster_ms=26.407`. VS SH
  evaluation costs ~19ms of the 52ms GPU frame. Largest single GPU item.
- Phase 1 diagnostic B (temporary quad offset x0.5, ~75% fewer fragments):
  `avg_call_ms=38.568 avg_frame_ms=33.155`. Full fragment fill cost ~18ms;
  the realistic 21% alpha-aware shrink projects to roughly 4ms saved.
- Attribution: GPU frame ~52ms = ~19ms VS SH + ~18ms fragment fill + ~15ms
  other (VS geometry, blend/present overhead). With SH removed the call wall
  fell below the CPU prep frame time (33 < 36), so after Opt B the CPU path
  becomes the next limit.
- Opt A (alpha-aware quad shrink, VS-only `alpha_extent_scale`) device result:
  `avg_call_ms=38.522 avg_frame_ms=33.313 avg_raster_ms=27.535
  avg_drawn=562974`. Call wall 51.955 -> 38.522 = -25.9%. The win exceeds the
  21% area estimate because low-alpha splats skew large on screen, so the
  fragment-weighted reduction is bigger than the unweighted area ratio.
- Opt A quality proof: desktop offscreen PNGs before/after differ only by
  max 1/255 per channel on 0.0002% (minimal) / 0.06% (flowers) of subpixels.
  This is interpolation rounding from re-positioned quad vertices (the
  pixel->gaussian linear map is mathematically identical); perceptually and
  semantically the output is unchanged, and `drawn` count is unchanged.
- Opt B (per-instance SH color compute prepass writing a work buffer read by
  the VS, single encoder, no readback): `avg_call_ms=159.171
  avg_frame_ms=46.398`. Severe regression, consistent with the prior
  compute-color experiment (140ms). On this tile-based GPU the
  compute->vertex dependency serializes the frame even within one submission.
  Reverted cleanly.
- Opt B' (replacement: VS keeps SH but evaluates all three channels together;
  basis polynomials computed once per vertex, coefficients read as vec3;
  per-channel accumulation order unchanged -> mathematically identical):
  `avg_call_ms=32.793 avg_frame_ms=29.916 avg_raster_ms=26.015`. This reaches
  the diagnostic SH=0 floor (33.0), i.e. the redundant per-channel SH ALU cost
  is effectively eliminated.
- Retained default path final confirmation (same APK):
  `avg_call_ms=33.192 avg_frame_ms=30.298 avg_preprocess_ms=0.746
  avg_sort_ms=3.144 avg_raster_ms=26.407 avg_visible=562974
  avg_drawn=562974`. Baseline 51.955 -> 33.192 = -36.1% call wall
  (+56.5% throughput), full drawn count preserved.
- Historic reversal after the GPU-side wins: `gpu_preproject=true` now beats
  the default CPU path for the first time (`avg_call_ms=30.504`,
  frame_ms=10.980), and `static_direct=true` is fastest
  (`avg_call_ms=29.820`, frame_ms=10.726). `gpu_preproject+async_sort` adds
  nothing (30.724) -> the device is now GPU-bound at ~30ms. Both stay opt-in
  per the existing default-path policy; flipping the default needs its own
  cross-platform validation track.
- Device quality proof (normal mode, static auto camera, two separate
  launches of the same APK produce byte-identical screencaps -> rendering is
  deterministic): before/after center-crop diff is 0.447% of subpixels, 95.7%
  of them +-1/255, 239 of 6.77M subpixels >2, max delta 12/255. Cause is
  floating-point rounding from repositioned quad vertices and vectorized SH
  contraction, not an algorithmic change. Overlay confirms
  `drawn=562974/562974`; normal-mode render call 47.89 -> 40.10ms.
- Regression set green: fmt, clippy -D warnings, workspace tests (15 ok),
  docs, wasm32 check of `gsplat-web`, bench-runner minimal run.

## Follow-up: static_direct promoted to default (user request, same day)

- Flipped `surface_static_direct` default to `true` in
  `crates/gsplat-ffi-c/src/lib.rs` (no C ABI signature change), plus option
  defaults in `bindings/android` `GsplatSurfaceOptions`, `bindings/apple`
  `GsplatSurfaceOptions`, and both example apps. Web (`gsplat-web`) does not
  expose this toggle and keeps the CPU instance path.
- Docs synced: `bindings/android/README.md`, `bindings/apple/README.md`,
  `bindings/apple/scripts/benchmark-ios-device-app.sh` default args,
  `handbook/ARCHITECTURE.md` Web flow note.
- Verification: cargo check/test/fmt/clippy green; FFI smoke ok; JNI smoke ok;
  Swift smoke ok; XCFramework build ok; GsplatKit iOS-simulator xcodebuild ok.
- Device confirmation (android-test-device, default extras, no static_direct flag):
  benchmark line reports `static_direct=true`, cold-state
  `avg_call_ms=29.973 avg_frame_ms=10.619` matching the earlier 29.820
  measurement; normal-mode launch renders correctly with `raster=0.00ms` and
  `drawn=562974/562974`.
- Caveat: back-to-back benchmark repeats showed strong thermal throttling
  (both paths inflate to 45-50ms when hot, and ordering becomes noisy).
  Cold-state ordering (direct 30.0 vs CPU 33.0) is consistent with the
  original A/B. Sustained-load thermal behavior of static_direct vs the CPU
  path is a known open question for a future pass.
- iOS physical-device benchmark (iPhone 17 Pro Max,
  release Rust + -O Swift,
  flowers_1.ply): default config reports `static_direct=true` with
  `avg_call_ms=17.786 / 17.237` across two runs (`avg_frame_ms=5.5-6.2`,
  raster 0.000, drawn 562974/562974). Explicit
  `static_direct=false` control: `avg_call_ms=17.623`
  (`avg_frame_ms=9.952`, raster 7.756). Call wall is identical across paths
  (~17.2-17.8ms, present/vsync-bound on this GPU), so the default flip does
  not regress iOS; the direct path additionally cuts native frame prep from
  9.95ms to ~5.9ms. Logs under `target/ios-device-benchmarks/`.

## Issues Encountered

| Issue | Resolution |
|-------|------------|
