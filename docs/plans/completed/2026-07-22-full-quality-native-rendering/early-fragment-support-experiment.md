# Rejected early-fragment-support experiment

Date: 2026-07-23 (Asia/Shanghai)

## Question

The retained opacity-aware quad shrink already restricts each square to the
conservative `1/256` opacity iso-contour. The fragment shader still evaluates
the Gaussian exponential for covered samples and then discards results below
`1/255`. The experiment asked whether passing an opacity-dependent support
bound into the fragment stage could reject guaranteed-empty corners before
the exponential and improve full-resolution throughput.

This was not a quality shortcut. The proposed support radius used the wider
`1/256` boundary while the contribution cutoff stayed at `1/255`, so any
fragment that could survive the existing cutoff remained eligible. Source,
resident, addressable, visible, drawn, and issued instance counts were
unchanged.

## Variants

The A/B tested two forms on the exact same complete-Truck path:

1. **Per-fragment logarithm.** Recompute the opacity support radius in the
   fragment shader and discard outside it before evaluating `exp`.
2. **Flat support value.** Compute the support bound once per splat in the
   vertex stage, pass it as a flat varying, and compare it in the fragment
   shader before `exp`.

Both were layered on top of the retained four-vertex `TriangleStrip` and
opacity-aware quad extent. Neither changed resolution or point/SH membership.

## Method

- Host: Apple M4 Metal.
- Scene: complete Truck, 2,541,226 source/resident/addressable splats, SH3.
- Surface: requested/internal/presented 1920x1080; dynamic resolution and
  upscaling disabled.
- Camera: alternating two-view moving trace, exact sort/projection each frame.
- Backend: forced CPU ordering to hold the ordering producer constant.
- Schedule: 20 warmup + 80 measured frames per run.
- Success conditions: all 100 frames presented, all exact terminal tickets
  closed, zero fallback/outstanding measurements.

## Result

| Variant | Run 1 | Run 2 | Relative conclusion |
| --- | ---: | ---: | --- |
| retained baseline | 42.141 FPS | 41.915 FPS | fastest |
| per-fragment logarithm | 41.377 FPS | 41.510 FPS | regression |
| flat support value | 40.990 FPS | 41.137 FPS | larger regression |

The per-fragment form lost about 1.5% relative to the paired baseline range;
the flat-varying form lost about 2.3%. This is consistent with the added
branch/logarithm or varying/interpolation overhead exceeding the exponential
work saved on this scene, but the measurements establish the decision without
requiring that microarchitectural inference.

## Decision

Both early-fragment variants were completely reverted. The retained code has:

- four canonical triangle-strip vertices;
- conservative vertex-stage extent shrink to the `1/256` iso-contour;
- the original fragment Gaussian evaluation and strict `< 1/255` discard;
- no extra support varying, per-fragment logarithm, or early support branch.

This preserves the already measured benefit of opacity-aware raster bounds
without keeping a theoretically attractive optimization that was slower in
the representative full-resolution workload.

## Artifacts

All artifacts are local ignored benchmark evidence under
`target/full-quality-final/early-fragment-support-ab/`:

- retained controls: `baseline-r1.log`, `flat-baseline-r2.log`;
- per-fragment logarithm: `candidate-r1.log`, `candidate-r2.log`;
- flat-support value: `flat-candidate-r1.log`, `flat-candidate-r2.log`;
- frozen candidate binaries: `desktop-example-candidate` and
  `desktop-example-flat-support`.

The binaries are evidence only. Neither candidate implementation remains in
the source tree.
