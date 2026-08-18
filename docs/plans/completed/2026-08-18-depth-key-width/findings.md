# Findings: 16-bit Depth-Key Experiment

## Mechanism

Packed CPU pairs are `key << 32 | !index`. Full-key radix already skips
the low 32 index bits (`32..64`, four 8-bit passes). High-16 uses
`48..64` (two passes). For positive f32 depths this keeps sign, the
full exponent, and the top 7 mantissa bits (~1/128 relative precision).

GPU radix is eight stable 4-bit LSD passes. `High16` starts at pass 4,
so bits 0–15 stay in compact (ascending-id) order and bits 16–31 sort.
`first_radix_pass` is even, so ping-pong still ends in `pairs_a`.

Tie-break is identical on both backends: truncated-key descending,
then ascending source id.

## Scope

- Rust-only. No C ABI / JNI / Web API change.
- Offscreen `Renderer` and Surface GPU-order honor the knob.
- Native `AsyncLatest` still sorts full 32-bit keys. Desktop
  bench-runner evidence is offscreen and unaffected.

## Kill criteria (copied from plan)

- Image: CPU-16 vs CPU-32 and GPU-16 vs GPU-32 (via CPU-32 reference,
  given GPU-32 already matches CPU-32) must hold SSIM ≥ 0.99.
- Perf: 16-bit keys must improve the loser's end-to-end time on at
  least one device tier, or we record and stop.

## Host image evidence (2026-08-18, Metal, 128²)

Synthetic degree-3 and duplicate-depth: CPU-16, GPU-16, and CPU-32 are
pixel-identical (SSIM 1.0, mean abs RGB 0).

| Scene | Pair | SSIM | mean abs RGB | frac >3/255 | max abs RGB |
|-------|------|------|--------------|-------------|-------------|
| Kitsune | cpu16 vs cpu32 | 0.987058 | 0.006991 | 0.131531 | 0.533333 |
| Kitsune | gpu16 vs cpu32 | 0.987058 | 0.006991 | 0.131531 | 0.533333 |
| Kitsune | gpu16 vs cpu16 | 1.000000 | 0.000000 | — | — |
| Flowers | cpu16 vs cpu32 | 0.995640 | 0.003448 | 0.094666 | 0.478431 |
| Flowers | gpu16 vs cpu32 | 0.995640 | 0.003448 | 0.094666 | 0.478431 |
| Flowers | gpu16 vs cpu16 | 1.000000 | 0.000000 | — | — |

Kitsune misses the pre-registered SSIM ≥ 0.99 promotion gate. Flowers
clears SSIM but still has ~10% of pixels off by more than 3/255 and a
max channel error of ~0.48. The unit test keeps synthetic 0.99 and
cpu16/gpu16 consistency as hard asserts; real-scene 16-vs-32 is logged
here as the promotion probe.

Truncation is implemented consistently: CPU-16 and GPU-16 match exactly
on every fixture. The quality miss is the reduced key, not a backend
divergence.

## Desktop Metal Kitsune (2026-08-18, M4 Pro, 120+10, one 4-way)

Artifact root: `target/benchmarks/depth-key-width/desktop-metal-kitsune-{cpu,gpu}-{32,16}`.
All four runs validated. Adapter Apple M4 Pro / Metal. Visible 70402 on
CPU (near/far preprocess); GPU telemetry still reports the 279199 source
count.

| Run | sort mean / p50 (ms) | GPU-complete mean / p50 (ms) | call p50 (ms) |
|-----|----------------------|------------------------------|---------------|
| cpu-32 | 0.483 / 0.471 | 3.876 / 3.382 | 0.876 |
| cpu-16 | 0.245 / 0.229 | 3.404 / 1.993 | 0.614 |
| gpu-32 | 0 / 0 | 4.973 / 5.649 | 0.582 |
| gpu-16 | 0 / 0 | 3.875 / 3.010 | 0.365 |

CPU radix sort scales as expected: 4 passes → 2 almost exactly halves
`sort_ms` (0.483 → 0.245). End-to-end CPU-complete improves ~12% on the
mean because sort is only part of the frame.

GPU-16 (the previous loser) improves 22% mean / 47% p50 vs GPU-32, so
the perf gate is met on this desktop tier. GPU-16 mean matches CPU-32
but still loses to CPU-16 (3.875 vs 3.404 mean; 3.010 vs 1.993 p50).

Single-run, not a 3-rep alternating pair. Means vs p50 disagree on
magnitude because of a heavy tail; direction is the same.

## Verdict so far

Image gate failed on Kitsune (SSIM 0.987, 13% of pixels >3/255, max
channel error 0.53). Perf gate passed on desktop Metal for the GPU
loser. Pre-registered rule: a visible ordering artifact kills the
candidate regardless of speed. Do not promote 16-bit keys. CPU 32-bit
radix stays default. Mobile A065/iPhone 16-bit pairs are optional
follow-up only; they cannot override the image kill for SortedAlpha.

## 2026-08-18: gate vs PlayCanvas (correction)

CPU-on-mobile and 16-bit keys are independent. Device pairs still say
CPU radix should stay the default *where* to sort. Key width is a
separate approximation on that CPU path.

PlayCanvas does not truncate IEEE f32 bits to 16. Unified GSplat maps
camera distance into the current `[minDist, maxDist]` range, splits it
into 32 bins, and spends a dynamic **10–20 bit** bucket budget with
camera-relative weights (near bins ×40, far bins ×1). Their quality
target is "no nearby flicker", not SSIM vs a 32-bit float key.

Our experiment chopped the existing `depth_z.to_bits()` pattern to the
top 16 bits and then asked for SSIM ≥ 0.99 vs full keys at 128². That
0.99 line is the right gate for "same order, two backends" (CPU vs GPU
already hits 1.0). It is the wrong gate for a deliberate approximate
order: 128² with ~279k overlapping splats inflates local blend
differences, and PlayCanvas never claims pixel identity with 32-bit
keys.

Kitsune 0.987 vs 0.99 is therefore not proof that 16-bit is unusable on
mobile. It is proof that *naive f32 truncation* diverges from our
32-bit oracle at the backend-parity fixture. Whether that divergence is
visible at display resolution, and whether PlayCanvas-style
range-mapped keys would pass a visual bar, is still open.

## 2026-08-18: weighted 16-bit keys (PlayCanvas bins)

Replaced IEEE truncation. `High16` now maps camera Z into the scene
AABB view-Z range, splits it into 32 bins, and spends 65536 buckets
with PlayCanvas weights (near ×40 … far ×1). Keys pack into the top 16
bits so CPU 2-pass / GPU 4-pass radix stay as-is. CPU and GPU share
the same range and table.

Host image (128², Metal):

| Scene | Pair | SSIM | mean abs RGB | frac >3/255 |
|-------|------|------|--------------|-------------|
| Kitsune | cpu16/gpu16 vs cpu32 | 1.000000 | 0 | 0 |
| Flowers | cpu16/gpu16 vs cpu32 | 0.999997 | 0.000029 | 0.000366 |
| all | gpu16 vs cpu16 | 1.000000 | 0 | — |

2.0 vs 2.001 now keep true depth order (they tied under truncation).
Image gate passes.

Desktop M4 Pro Kitsune 120+10 (`target/benchmarks/depth-key-width-weighted`):

| Run | sort_ms | GPU-complete mean |
|-----|---------|-------------------|
| cpu-32 | 0.449 | 2.218 |
| cpu-16 | 0.238 | 1.887 |
| gpu-32 | 0 | 3.124 |
| gpu-16 | 0 | 2.080 |

CPU sort still halves. Weighted keygen is not a measurable extra cost.
GPU-16 is closer to CPU-16 than truncation was. CPU-16 remains fastest.
Default stays CPU; 16-bit is now a quality-passing experimental width
on that CPU path.

## 2026-08-18: mobile 4-way collection

Hidden extras `gsplat_surface_depth_key_bits` (Android `--ei`, iOS
launch arg) plus `gsplat_android_benchmark_set_depth_key_bits` /
`gsplat_apple_benchmark_set_depth_key_bits`. Published `gsplat.h`
unchanged. Collector `--depth-key-bits` is cartesian with `--backend`.

Protocol: Kitsune 279199, 80+20, interval 1, 3 reps, seed 20260818,
randomized `{cpu,gpu}×{32,16}`, cooldown 10s. Android thermal 0.

Artifact roots:
- `target/android-sort-benchmarks/weighted16-kitsune-4way`
- `target/ios-device-benchmarks/weighted16-kitsune-4way`

A065 (Nothing / SM8475 / Adreno, Android 15), mean of 3 run-means:

| Run | call_ms | sort_ms | preprocess_ms |
|-----|---------|---------|---------------|
| cpu-32 | 20.782 | 9.358 | 2.268 |
| cpu-16 | 21.090 | 8.244 | 2.541 |
| gpu-32 | 30.228 | n/a | n/a |
| gpu-16 | 24.677 | n/a | n/a |

GPU-16 vs GPU-32 is an 18% call-time win (the 32-bit GPU loser).
CPU-16 radix is ~12% cheaper (9.36→8.24) but weighted remap raises
preprocess (2.27→2.54) and end-to-end is slightly worse than CPU-32.
CPU-32 still beats GPU-16 (20.8 vs 24.7). All 80 frames missed the
16.6 ms budget.

iPhone 17 Pro Max (iPhone18,2), UIKit Surface, mean of 3 run-means:

| Run | call_ms | sort_ms | preprocess_ms |
|-----|---------|---------|---------------|
| cpu-32 | 16.728 | 4.552 | 1.554 |
| cpu-16 | 16.677 | 3.641 | 1.641 |
| gpu-32 | 16.720 | 0 | 0 |
| gpu-16 | 16.699 | 0 | 0 |

Kitsune is vsync-capped (~16.7 ms) on every backend. CPU sort drops
~20% (4.55→3.64); that does not move presented frame time. GPU e2e
is indistinguishable from CPU at this cap.

## Device verdict

Keep CPU 32-bit as the default. Weighted 16-bit is a quality-passing
experimental width: it helps the GPU-order loser on A065 Kitsune and
cuts CPU sort on both phones, but it does not beat CPU-32 end-to-end
on the default path, and GPU-16 still loses to CPU-32 on A065. Do not
promote GPU order or 16-bit keys.

## 2026-08-18: scale-up (Truck 200k / 700k)

Same 4-way protocol, seed 20260818. Artifacts:
- `target/android-sort-benchmarks/weighted16-truck-200k-4way`
- `target/android-sort-benchmarks/weighted16-truck-700k-4way`
- `target/ios-device-benchmarks/weighted16-truck-700k-4way`

A065 means of 3 run-means (700k GPU-16 mean is pulled by a 376 ms
first-run outlier; medians in parentheses):

| Scene | cpu-32 call / sort | cpu-16 call / sort | gpu-32 call | gpu-16 call |
|-------|--------------------|--------------------|-------------|-------------|
| Kitsune 279k | 20.78 / 9.36 | 21.09 / 8.24 | 30.23 | 24.68 |
| Truck 200k | 16.35 / 7.07 | 16.70 / 5.87 | 24.09 | 20.11 |
| Truck 700k | 270.91 / 20.44 | 278.67 / 17.75 | 294.82 | 316.50 (med 286.7) |

CPU-16 sort savings grow in ms (1.12 → 1.20 → 2.69) and stay ~12–17%
of sort_ms. End-to-end CPU-16 is still worse at every tier: remap
raises preprocess, and at 700k fill dominates (sort is 7.5% of 271 ms).
GPU-16 relative call win shrinks as fill grows (18% → 16% → ~3% on
700k median). GPU-16 never beats CPU-32 on A065.

iPhone 17 Pro Max Truck 700k, mean of 3 run-means:

| Run | call_ms | sort_ms | preprocess_ms |
|-----|---------|---------|---------------|
| cpu-32 | 16.654 | 4.089 | 1.522 |
| cpu-16 | 16.643 | 4.593 | 1.970 |
| gpu-32 | 17.772 | 0 | 0 |
| gpu-16 | 16.660 | 0 | 0 |

GPU-32 misses 60 fps (17.8 ms, same as the compact-v1 pair). GPU-16
holds the 16.7 ms cap on all three runs and matches CPU-32. CPU-16
e2e is still vsync-capped; one cold cpu-16 sort (6.51 ms) inflates
the sort mean — the other two are 3.59 / 3.68 vs cpu-32 4.09.

More points do not make 16-bit a better default-path win. They make
absolute sort savings larger and relative e2e savings smaller, except
for the iPhone 700k GPU loser, which 16-bit pulls under vsync without
beating CPU.

## Final verdict (2026-08-18 close-out)

Do not promote. Default remains **CPU radix + 32-bit IEEE keys**.

- Weighted 16-bit keys passed the 128² SSIM gate vs CPU-32, but A065
  CPU-16 was slower end-to-end at Kitsune, Truck 200k, and Truck 700k
  because remap paid more than the two saved radix passes. Sort savings
  grew with N; relative frame savings shrank.
- GPU-16 helped the experimental GPU path (A065 Kitsune −18% call;
  iPhone Truck 700k back under vsync) and never beat CPU-32.
- PlayCanvas uses 10–20 bit counting-sort keys in a JS worker. That is
  an algorithm constraint, not a reason to keep a reduced-width radix
  on a native SIMD 8-bit sorter that already sorts full 32-bit keys.
- Native `AsyncLatest` and mobile `sort_interval=2` stay as they were:
  interval 2 default, async off, both already on the C ABI / wrappers.
  They are not part of this demotion.

The 16-bit width, remap, `DepthKeyBits`, hidden extras, and
`--depth-key-bits` collector/bench knobs were deleted from the tree in
the same change. GPU compact+indirect remains an experimental backend.
