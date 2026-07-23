# Desktop Surface Metal evidence

Date: 2026-07-23 (Asia/Shanghai)

## Accepted current result

The accepted desktop evidence uses a real Metal Surface on an Apple M4
(10-GPU-core, 16 GB unified-memory host). Complete Truck establishes the
CPU/GPU/Adaptive policy and Direct image gate; complete Garden and Bicycle now
also establish current-code large-scene admission and rendering at 1080p.

For Truck:

```text
source_count == decoded_count == encoded_count
             == resident_count == addressable_count == 2,541,226
source_sh_degree == resident_sh_degree == 3
requested == surface == internal == presented == 1920x1080
dynamic_resolution == disabled
upscaling == disabled
membership == all
sampling == disabled
lod == disabled
partial_scene == false
```

The moving trace alternates the two official selected views and forces exact
sort plus projection on every scheduled frame. Each sustained Truck run uses
20 warmups and 80 measured frames. It therefore measures the work an
interactive changing camera requires and cannot win by reusing one stationary
order/projection.

The current retained CPU-preprocessing code has three complete runs at
43.321400 / 43.169037 / 43.747669 FPS (median 43.321400 FPS). Mean CPU
preprocessing is 4.54--4.97 ms and mean radix is 4.82--5.12 ms. Every run
presented 100/100 frames, completed 100 terminal tickets, and reported zero
fallback or outstanding ticket. Raw evidence is under
`target/full-quality-final/parallel-preprocess-ab-20260723/cpu-parallel-r*.log`.

The following older same-cohort table remains the clean forced
CPU/GPU/Adaptive policy comparison. It predates only the retained bounded CPU
preprocessing change, so it must not be substituted for a fresh current-binary
three-way cohort in a publication benchmark:

| Mode | Run 1 | Run 2 | Run 3 | Measured backend frames |
| --- | ---: | ---: | ---: | --- |
| forced CPU | 42.463570 FPS | 41.898083 FPS | 41.923603 FPS | 80 CPU each |
| forced GPU | 38.515408 FPS | 38.434029 FPS | 37.812092 FPS | 80 GPU each |
| Adaptive | 42.313347 FPS | 42.390305 FPS | 41.288265 FPS | 76 CPU / 4 GPU each |

Every run presented all 100 scheduled frames (20 warmup + 80 measured),
performed 100 exact sort refreshes, reported zero GPU fallback, and exited with
zero outstanding CPU/GPU tickets. View 0/1 visible and drawn counts are
1,886,298 / 1,672,013 on both ordering backends.

The paired CPU runs are tightly grouped and faster than all three forced-GPU
runs, so CPU is the current choice for this host/workload. Adaptive is
CPU-dominant and pays only a bounded four-frame GPU exploration cost during
this short window. Ending in `gpu_probe` records that a probe was in progress;
it does not mean GPU won. The current CPU-only rerun above strengthens that
choice. This table is retained as the PostSort policy baseline. The later same-binary
PostSort/Preproject cohort is the terminal GPU-plan result; see
`phase2-production-producer-ab-checkpoint.md` and `final-report.md`.

Raw artifacts:

- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/cpu-r1.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/cpu-r2.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/cpu-r3.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/gpu-r1.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/gpu-r2.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/gpu-r3.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/adaptive-r1.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/adaptive-r2.log`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/adaptive-r3.log`

## Adaptive timing semantics

Adaptive's formal production metric is `FrameCompletion`. A CPU sample and a
GPU sample each begin at the sampled frame start and end only when that frame's
submitted GPU work completes. This includes projection/raster queue pressure,
which is essential because GPU ordering competes with raster on the same GPU
while CPU ordering may overlap queued graphics work.

GPU timestamp intervals for key generation and radix are still recorded to
diagnose stages. They are not the Adaptive decision signal. Submission/call
wall is also not renamed as GPU execution or completion. Samples are joined by
backend, measurement ticket, and camera revision; the incumbent serves normal
frames while one challenger receipt is pending. Changing the raster execution
plan resets Adaptive learning because the previous completion samples no
longer describe the same workload.

The logs illustrate why sort-only timing is insufficient. The forced-GPU runs
report mean GPU-order timestamps around 21.49--21.88 ms, while the CPU runs'
mean preprocess+sort is around 13.27--14.84 ms. More importantly, the complete
presented-frame throughput also favors CPU. If a future GPU sorter improves,
Adaptive will remeasure the whole result instead of relying on this table or a
point-count constant.

## Full-resolution Direct oracle

The current four-vertex, opacity-bounded Packed path is compared with the wide
Direct compatibility oracle at the same two 1920x1080 cameras:

| View | Source = resident | Visible = drawn | SSIM | RGB normalized MAE | Alpha |
| --- | ---: | ---: | ---: | ---: | --- |
| 0 | 2,541,226 | 1,886,298 | 0.9999693448 | 0.0000284315 | exact |
| 1 | 2,541,226 | 1,672,013 | 0.9999705553 | 0.0000261173 | exact |

Artifacts:

- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/direct-vs-packed-view0.json`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/direct-vs-packed-view1.json`

This is the desktop visual sign-off. Earlier 640x360 image comparisons remain
codec diagnostics only.

## Retained raster A/B

Two changes remain in all exact quad paths:

1. A four-vertex `TriangleStrip` in canonical `BL, BR, TL, TR` order replaces
   the former six-vertex triangle list. It covers the same two triangles but
   removes two redundant vertex invocations per visible splat.
2. The quad extent is shrunk to the conservative `1/256` opacity iso-contour.
   The fragment shader discards only below `1/255`, so every sample that can
   contribute remains covered. Draw instance count and sorted membership do
   not change.

On the complete moving Truck 1920x1080 trace, the no-opacity-shrink
six-vertex baseline was about 34.045 FPS, opacity shrink with six vertices was
about 37.323 FPS, and opacity shrink plus the four-vertex strip was about
42.070 FPS. Thus the combined retained optimization improved the local
baseline by about 23.6% without reducing point count, SH, resolution, or the
fragment contribution domain.

Artifacts are under `target/full-quality-final/opacity-shrink-ab/`.

## Rejected early-fragment guard

An additional vertex/fragment guard attempted to avoid the exponential for
pixels outside the opacity support. It was mathematically conservative but
slower on this workload:

| Variant | Run 1 | Run 2 | Decision |
| --- | ---: | ---: | --- |
| retained baseline | 42.141 FPS | 41.915 FPS | keep |
| per-fragment support logarithm | 41.377 FPS | 41.510 FPS | revert |
| flat-varying support bound | 40.990 FPS | 41.137 FPS | revert |

Both candidate forms were fully removed. Details and artifact mapping are in
`early-fragment-support-experiment.md`.

## Competitor boundary

The pinned PlayCanvas complete-Truck run at 1920x1080 records 600 terminal
frames in 10,603.6 ms, or 56.5846 FPS:

`target/benchmarks/competitive/playcanvas-truck-1080p-sequence-600-terminal-v2/`

The median current Mac CPU run is 43.321400 FPS. The branch therefore has not
yet caught that observed throughput. However, 56.5846 FPS is not a strict
equal-quality denominator: the pinned PlayCanvas path uses a compact quantized
work format, approximately 20 sort-key bits for this 2.541M scene, fp16
projected axes/color, and a default 10-degree SH color-update threshold. Its
full-source/no-LOD receipt is useful, but it is not numerically equivalent to
this branch's full-32-bit stable order and exact projected-cache contract. The
same-camera frame-0 images are also not pixel matched: the recorded comparison
is SSIM 0.938175 and normalized RGB MAE 0.028223, with exact alpha. That diff
does not rank either image as better; it proves that an unqualified
“same-quality FPS” claim would be false.

A prior stationary-cache experiment that sorted/projected once reached a
higher number, but it is not comparable to the current alternating-camera
every-frame work and is not used as a parity claim.

## Current Garden and Bicycle large-scene admission

Both complete official SH3 scenes render successfully with Packed +
ProjectedQuadsExact, forced CPU order, both official 1920x1080 views, two
warmups and four measured frames. Requested, Surface, internal, and presented
resolution are all 1920x1080; dynamic resolution and upscaling are disabled.

| Scene | Source = resident | Visible/drawn views | load-to-benchmark | measured call-wall mean | isolated completion mean | max RSS | peak footprint |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| Garden | 5,834,784 | 4,609,628 / 4,226,208 | 4,166.562 ms | 23.058 ms | 77.644 ms | 1,631,600,640 B | 2,640,021,400 B |
| Bicycle | 6,131,954 | 4,321,800 / 3,927,910 | 2,663.229 ms | 23.893 ms | 74.061 ms | 1,751,252,992 B | 2,794,244,064 B |

Every one of the six frames per scene was presented, every sort was refreshed,
all six terminal tickets completed, and there were zero fallback or
outstanding tickets. The current desktop text receipt prints source/resident
count and SH degree but not decoded/encoded/addressable count. The fail-closed
resident builder and allocation chain enforce those stages, yet the missing
fields remain an artifact-schema gap rather than silently being reported as
directly observed evidence. These short isolated runs prove full-scene
runability/capacity and exact per-view draw counts; they are not sustained FPS
qualifications.

Raw logs and their SHA-256 hashes:

```text
5d6a80ae00047b6ff7aee83481716670fa50cb662226446382bf28913e0b9677  target/full-quality-final/mac-large-scenes-current-20260723/garden/run.timestamped.log
f6d03e3feda1f7317959ce55371ce7ce6b8a6c01522a328f16fe4e0b8e00af08  target/full-quality-final/mac-large-scenes-current-20260723/bicycle/run.timestamped.log
```

## Historical diagnostics, not formal quality/performance

Older 640x360 Surface runs and phase traces remain useful for isolating
projection, scan, scatter, raster, and near-plane parity bugs. They are too low
resolution to answer whether the real output is acceptable and are excluded
from product FPS, image-quality, and competitor comparisons.

The earlier `TiledExact` and 3840x2160 pressure experiments likewise remain
oracle/runability diagnostics. They do not replace the current
`ProjectedQuadsExact` 1920x1080 result and do not establish a backend policy.

## Follow-up desktop work

- Re-run the current point-count ladder with the retained raster code; older
  codec/order-only crossover tables are not final.
- Extend current Garden/Bicycle from short runability receipts to sustained
  CPU/GPU/Adaptive cohorts and add the five-stage exactness fields to desktop
  artifacts.
- Add the composite GPU-plan controller before publishing a new current-binary
  CPU/GPU/Adaptive product ratio.
- Continue exact raster/per-frame optimization and define an explicitly
  matched competitor quality profile before making a parity claim.
