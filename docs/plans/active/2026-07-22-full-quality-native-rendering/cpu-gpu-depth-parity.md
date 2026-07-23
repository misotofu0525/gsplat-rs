# CPU/GPU depth-classification parity

Date: 2026-07-23 (Asia/Shanghai)

## Result

The one-splat Garden view-1 discrepancy is resolved without an epsilon,
sampling, LOD, or a point budget. CPU preprocessing, GPU key generation, and
the exact projection paths use the same explicit f32 depth sequence:

```text
x_product = row.x * relative.x
xy        = fma(row.y, relative.y, x_product)
depth     = fma(row.z, relative.z, xy)
visible   = near <= depth && depth <= far
```

Rust uses `f32::mul_add`; WGSL uses `fma`. The inclusive near/far predicate is
unchanged. Current Direct, Resident, and projected-cache raster paths also
share the canonical depth calculation, so order classification and draw
projection cannot silently disagree at a clip-plane rounding boundary.

## Root cause and exact source

The reproducer uses
`tests/perf/trace/fixtures/quality/candidate-garden-quality-640x360-v1.json`
with hash
`9dd7c8abc4ccfd74f54ae863df2123f3ff28817b4962048a02cafb4bf99a08ec`.
This 640x360 trace is retained as a compact depth-parity diagnostic only; it is
not formal visual-quality, product-throughput, or competitor evidence.

For diagnostic frame 1, the only differing source was:

```text
source_id       4,244,161
position_ruf    (-3.8502328, -0.68630046, 5.1374936)
near_plane      0.01 (bits 0x3c23d70a)
old CPU depth   0.009999994 (bits 0x3c23d704), invisible
GPU/FMA depth   0.010000017 (bits 0x3c23d71c), visible
```

The exact-real dot product in wider arithmetic is approximately
`0.01000001315`, above the f32 near plane. The explicit FMA sequence is both
portable across the two implementations and the more accurate classification.

A first diagnostic forced products through cross-lane workgroup memory. It
made the sets agree but increased Garden key generation from about 4.9 ms to
37 ms, so it was rejected. The retained FMA contract needs no extra barrier.

## Complete source-ID regression

`external_garden_view1_cpu_gpu_visible_sets_are_identical` streams the complete
5,834,784-point PLY, preprocesses every position on CPU, reads every GPU radix
pair plus the indirect count, and compares visible membership by source ID.
Its receipt is:

```text
GARDEN_VIEW1_PARITY trace_sha256=9dd7c8abc4ccfd74f54ae863df2123f3ff28817b4962048a02cafb4bf99a08ec source_count=5834784 cpu_visible=4226208 gpu_visible=4226208 differing_source_ids=0
```

`canonical_fma_matches_gpu_at_an_adversarial_near_plane_boundary` is the small
always-on regression. It embeds the exact source/camera/row bits, proves the
former unfused result and canonical result straddle the near plane, and
requires GPU key/count output to match the canonical CPU result.

## Current full-resolution Truck parity

The current accepted platform runs do not rely on the 640x360 diagnostic.
Complete Truck is measured on Mac/Web at 1920x1080 and on Nothing A065 at
native 2412x1080, with dynamic resolution and upscaling disabled.

On Mac, both CPU and GPU report the same per-view counts:

```text
view 0: visible == drawn == 1,886,298
view 1: visible == drawn == 1,672,013
source == resident == addressable == 2,541,226; SH3
```

Packed-vs-Direct at 1920x1080 yields:

| View | SSIM | Normalized RGB MAE | Alpha |
| --- | ---: | ---: | --- |
| 0 | 0.9999693448 | 0.0000284315 | exact |
| 1 | 0.9999705553 | 0.0000261173 | exact |

Chrome/WebGPU CPU/GPU/Adaptive screenshots are byte/pixel exact for the
qualified cameras. Nothing A065 CPU-vs-GPU center output is SSIM
`0.9999999791`, normalized RGB MAE `4.278e-8`, and alpha exact. These visual
checks complement the exact count receipts; they do not replace source-ID
membership testing.

## Adaptive semantics after parity

Depth parity makes either order producer correct, but it does not say which is
faster in the complete renderer. Adaptive therefore uses paired
`FrameCompletion` samples as its formal production metric. A sample covers the
sampled frame through completion of projection, raster, and queue work. GPU
key/radix timestamp queries remain diagnostic only and cannot select the
backend.

This distinction matters in the current evidence:

- Mac complete Truck 1920x1080: the paired policy cohort has CPU
  42.464/41.898/41.924 FPS versus GPU 38.515/38.434/37.812 FPS and Adaptive
  uses 76 CPU / 4 GPU measured frames. The later current CPU-only cohort is
  43.321/43.169/43.748 FPS; forced GPU/Adaptive still need a current-binary
  rerun.
- Nothing A065 complete Truck 2412x1080: current CPU median 179.335038 ms
  (5.576 FPS) versus GPU median 203.805100 ms (4.907 FPS); each of three
  Adaptive runs uses 69 CPU / 11 GPU measured frames and ends `cpu_stable`.
- Chrome/WebGPU complete Bicycle 1920x1080 demonstrates the opposite ranking:
  CPU completion averages 130.575 ms versus GPU 67.550 ms, and the long
  Adaptive window uses 63 GPU / 17 CPU measured frames with GPU incumbent.

Thus equal point count and equal order correctness do not imply GPU should be
chosen. The decision must include device-specific contention with the exact
raster path.

## Artifacts

- Historical complete Garden ID-set diagnostic:
  `target/full-quality-surface-metal/garden-view1-id-parity-test.log`
- Current Mac full-resolution logs:
  `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/`
- Current Mac CPU preprocessing logs:
  `target/full-quality-final/parallel-preprocess-ab-20260723/cpu-parallel-r*.log`
- Current Android quality metrics:
  `target/full-quality-final/final-android-truck-2412x1080-quality-20260723/cpu-vs-gpu-view0.json`
- Current Android policy artifacts:
  `target/full-quality-final/post-parallel-android-truck-2412x1080-cpu-gpu-adaptive-r3-20260723/`
- Current Web large-scene artifacts:
  `target/full-quality-final/web-large-scenes-current-20260723/`

The external Garden test remains ignored by default because it requires the
large local dataset and a native GPU. It should be rerun when the ordering
shader or canonical depth expression changes.
