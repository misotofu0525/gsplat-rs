# Task Plan: Android GPU Render Performance

## Goal

Improve Android true-device SortedAlpha Surface render performance for
`flowers_1.ply` without changing render output, by attacking the GPU-side
~50ms/frame bottleneck that prior CPU-side work could not move.

## Bottleneck Hypothesis

- All prior experiments pinned `avg_call_ms` at 51-55ms regardless of CPU prep
  cost (GPU preproject had ~10ms CPU prep but 55ms call wall). The call wall is
  GPU-bound through swapchain backpressure.
- GPU cost sources:
  1. `splat_surface.wgsl` evaluates SH color in the vertex shader, 6 vertices
     per splat -> ~3.4M degree-3 SH evaluations per frame, 5/6 redundant.
  2. ~563k alpha-blended ellipse quads with no depth test -> heavy fragment
     overdraw concentrated in few screen tiles.

## Phases

1. Phase 0: plan bundle, build/install sample APK, device baselines
   (sort_interval 2 and 1) on device `android-test-device`.
2. Phase 1: GPU attribution diagnostics (temporary builds, not retained):
   forced `sh_degree=0`, global quad axis x0.5; local alpha histogram of
   `flowers_1.ply`.
3. Phase 2: quality-exact optimizations:
   - Opt A: alpha-aware quad shrink + zero-contribution cull
     (`s = min(1, sqrt(ln(256a)/4.5))`, cull `a <= 1/256`).
   - Opt B: per-splat SH color compute prepass (single encoder, no extra
     submit/sync), VS reads precomputed color.
   - Opt C (conditional): instance data packing, only if upload becomes the
     bottleneck after A/B.
4. Phase 3: same-APK A/B benchmarks, PNG bit-exact diff proof, full
   regression set, findings/handbook updates. No commit without confirmation.

## Constraints

- SortedAlpha output must not change: prove with offscreen PNG diff.
- No C ABI changes.
- Do not touch the three uncommitted web files
  (`crates/gsplat-web/src/wasm.rs`, `examples/web/src/main.js`,
  `packages/web/src/index.js`).
