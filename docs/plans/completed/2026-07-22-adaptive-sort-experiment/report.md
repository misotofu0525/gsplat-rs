# Android Direct Ordering Experiment Report

## Decision

Keep CPU ordering as the release default. Retain the new GPU ordering path as
a deterministic, portable baseline and retain Adaptive as an opt-in runtime
measurement policy. Do not introduce a repository-wide point-count threshold.

On the tested Nothing A065 / Snapdragon SM8475 / Adreno 730, forced GPU was
slower in all 18 paired ladder comparisons at sort interval 1 and all 9 paired
comparisons at interval 2. At the original 200k decision point, interval-1 CPU
had a median run mean of 6.668 ms versus 15.365 ms for GPU. CPU had no missed
frames across 180 measured frames; GPU missed the 16.667 ms budget on 10 of
180 frames. The current GPU baseline therefore does not justify replacing the
CPU path on this device.

The branch is still worth closing out and merging because it turns an assumed
CPU/GPU choice into a reproducible measurement, adds a safe runtime selector,
and exposes a larger competitive blocker: degree-3 Direct scenes exceed the
mobile 128 MiB storage-binding limit above about 745k splats. Million-point
release support needs active-atlas/paging work, not another fixed sort rule.

## What was implemented

### Deterministic GPU ordering baseline

- The renderer's existing `wgpu::Device`, queue, source buffers, encoder, and
  submission are reused. Runtime ordering has no map/readback/poll step.
- A refresh performs one depth-key generation pass followed by eight stable
  4-bit LSD radix passes. Each radix pass uses histogram, prefix, and scatter,
  for 25 compute passes per refresh in total.
- Workgroups contain 64 threads, each processing four items, or 256 splats per
  tile. Equal keys retain ascending source-ID order.
- GPU ordering is currently sort-all/draw-all. Near/far-invalid entries receive
  key zero and are clipped by the Direct vertex shader.
- The final `(key, source_id)` buffer is consumed directly by the existing
  Direct draw. It is a normal instance draw, not indirect drawing.
- Scratch storage is approximately `16N + 64*ceil(N/256)` bytes, about 16.25
  bytes per splat, in addition to the existing Direct scene and CPU order
  buffer.

This is a portable correctness/performance baseline, not a claim of a
competitor-grade GPU sorter. In particular, the prefix pass uses one workgroup
and serially scans tile histograms per digit; large point counts predictably
expose that bottleneck.

### Runtime Adaptive policy

Ordering freshness and ordering backend are separate decisions:

1. `sort_interval` decides whether the current camera change needs a new order.
2. When a refresh is needed, Adaptive chooses CPU or GPU from measurements on
   the current renderer/device/workload.
3. CPU is learned first. Refresh and order-reuse frames are stored separately
   in rolling 16-sample windows; the policy uses p75 and combines them with the
   configured refresh cadence.
4. After 6 CPU learning refreshes and 4 more CPU refreshes, GPU is probed for 4
   refreshes. GPU is promoted only when its score is below 88% of CPU. CPU is
   selected when it is below 92% of GPU. A GPU score above 125% of CPU causes
   emergency demotion.
5. Stable GPU is challenged by a CPU probe every 48 refreshes. A lost GPU probe
   backs off from 48 to 96, 192, and 384 refreshes. A GPU execution error uses
   a 96-refresh cooldown.
6. A returned GPU error is included in the outer frame-wall measurement, then
   the same frame is synchronously recomputed on CPU and marked as fallback.

The policy has no point-count cutoff. It can react indirectly to sustained CPU
or GPU load through frame-wall samples and periodic probes. It does **not** yet
consume Android thermal status explicitly, and its windows, hysteresis, probe
lengths, and cooldowns are fixed constants. "Runtime-adaptive" should not be
misread as instant or fully self-tuning.

CPU remains the default. GPU and Adaptive are Direct-only and cannot be mixed
with the existing CPU `AsyncLatest` worker. GPU resources are prewarmed when
the backend is selected so first-use pipeline allocation is outside measured
frames.

### Reproducible ladder and Android collector

- `tests/datasets/ply_ladder.py` creates deterministic, byte-preserving tiers
  from fixed-record binary PLY files using integer midpoint stratification.
- The INRIA helper downloads exact archive ranges, verifies provenance, and
  refuses to imply redistribution rights. The official Truck model used here
  is marked `NOASSERTION` and local research/evaluation only.
- The Android collector records a seeded paired schedule, complete tagged
  logcat, canonical artifact, device properties, thermal status, dataset hash,
  APK hash, and native-library hash.
- It validates the installed debuggable APK against the local APK before every
  experiment. Each PLY is pushed once, copied after each package-data clear to
  `files/imported_scene.ply` with `run-as`, and re-hashed before launch. It does
  not rebuild or reinstall the APK between tiers.
- Artifact validation fails closed when requested/actual backend counts differ,
  a GPU fallback appears in a forced run, the dataset identity differs, or an
  indexed frame is missing.
- The collector defaults to 80 measured frames to remain below conservative
  Android `logd` burst quotas. One attempted 120-frame 500k run lost log lines
  and was rejected; it is not part of any retained result.

## Experiment contract

### Device and binary identity

| Field | Value |
|---|---|
| Device | Nothing A065 (`Pong`) |
| OS | Android 15 / API 35 |
| SoC / GPU | QTI SM8475 / Adreno 730 |
| Surface | 716 × 1600, DPR 2.625 |
| Observed refresh | 60.000004 Hz; 16.667 ms budget |
| Geometry path | release-gated `SortedIndexDirect` |
| APK profile | Android debug container + release Rust native |
| APK SHA-256 | `d3ad6c250ecb50b56160f6dd6e09a67f418169b51fea09bd345a7be1d92948b1` |
| `libgsplat_jni.so` SHA-256 | `9cf918d972f99dbdb3ee9d2b7f07bebd9d9ee00b668e84c2c2c3f4ed55d6a148` |
| Artifact source commit | `68af377b3c47bc578d81efff4a117dc893d845e1`, dirty worktree |
| Camera trace | orbit yaw `0.001` rad/frame |
| Frame latency | 2 |
| Async sort | disabled |

A fresh release-native build from the final source reproduced the recorded
native-library hash. The artifact itself records the base commit as dirty
because the experiment was intentionally performed before committing the
implementation; it does not independently hash the complete worktree source.

### Retained runs

- Primary ladder, interval 1: 6 tiers × 2 backends × 3 repetitions × 60 frames.
- Cadence check, interval 2: 3 tiers × 2 backends × 3 repetitions × 60 frames.
- Adaptive check: 3 tiers × 3 repetitions × 80 frames.
- Full-model anchors: 2 scenes × 2 backends × 3 repetitions × 60 frames.
- Periodic re-probe check: 200k × 3 repetitions × 120 frames.

That is 78 retained runs and 5,040 measured frames, plus 1,560 warmup frames.
All retained run-level thermal before/start/end/after readings were status 0.
Forced backends were randomized within each repetition; seeds were 20260722,
20260723, 20260724, and 20260725 for the four experiment groups.

### Scene coverage

The main curve uses deterministic subsets of the official INRIA Truck model:
50k, 100k, 200k, 300k, 500k, and 700k. The same exact PLY bytes can be reused
on Android, Apple, desktop, and Web benchmarks. These subsets are scaling
fixtures, not substitutes for full-scene quality evaluation.

Two independent full models were retained as anchors:

- Kitsune: 279,199 splats, CC0.
- Flowers: 562,974 splats, CC BY 4.0.

Official Bonsai (1,244,819) and Truck (2,541,226) files were also fetched, and
1M/1.5M/2M Truck tiers were generated, but they exceed this device's Direct
binding capacity. Garden/Bicycle were not required for this Android conclusion.

## Correctness evidence

- GPU tests cover empty, 1, 255, 256, 257, and 4,099 elements; duplicate keys;
  key zero and max; non-multiple tails; and an end-to-end 257-source
  key-generation/radix/readback test.
- Stable descending depth order and ascending source-ID tie order are tested.
- Forced Android artifacts confirm every measured frame used the requested
  backend and that all GPU fallback counters were zero.
- A stationary 50k CPU/GPU render crop on this device was pixel-identical:
  both crop hashes were
  `91dec5b0cf4a5adc6070f448fbc0c95f39be2177db51f22e512c1d0e3126b9ee`,
  with infinite PSNR and SSIM 1.0.

The screenshot check is limited to one device, one tier, one stationary camera,
and the rendered crop; screenshots were not committed as canonical artifacts.
It is not a claim of full-scene or cross-platform image conformance.

## Results

Every table reports the median of the three run means/p95s. Pair deltas are
computed within the same repetition and then summarized by their median. A
positive delta means GPU is slower. Missed frames are totals across 180 frames
per backend unless noted.

### Primary ladder: refresh every camera-change frame

| Splats | CPU mean | CPU p95 | GPU mean | GPU p95 | Paired GPU−CPU | GPU slower | Missed CPU/GPU |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 50k | 1.594 | 1.817 | 3.929 | 4.371 | +2.315 | +147.4% | 0 / 0 |
| 100k | 3.061 | 3.262 | 7.826 | 8.844 | +4.712 | +154.3% | 0 / 0 |
| 200k | 6.668 | 7.090 | 15.365 | 16.703 | +8.681 | +128.4% | 0 / 10 |
| 300k | 15.279 | 17.567 | 29.270 | 30.836 | +13.991 | +91.6% | 9 / 180 |
| 500k | 38.068 | 39.131 | 59.187 | 60.433 | +21.114 | +55.3% | 180 / 180 |
| 700k | 63.738 | 64.908 | 94.258 | 96.041 | +30.545 | +47.9% | 180 / 180 |

All 18 paired differences favored CPU. GPU's relative deficit narrows as point
count grows, but its absolute deficit grows from about 2.3 ms to 30.5 ms and no
crossover appears before the Direct capacity limit. CPU candidate count equaled
source count in these traces, so the result is not explained by CPU filtering
away a large fraction of points.

CPU-only phase means across the three repetitions were:

| Splats | Preprocess means | Sort means |
|---:|---:|---:|
| 50k | 0.143 / 0.144 / 0.143 | 0.525 / 0.526 / 0.528 |
| 100k | 0.306 / 0.306 / 0.302 | 1.180 / 1.179 / 1.161 |
| 200k | 0.809 / 0.816 / 0.769 | 3.238 / 3.329 / 3.174 |
| 300k | 1.445 / 1.360 / 1.349 | 6.606 / 6.413 / 6.343 |
| 500k | 2.952 / 3.132 / 2.811 | 11.002 / 11.384 / 11.231 |
| 700k | 3.391 / 3.377 / 3.438 | 13.370 / 12.597 / 13.328 |

These CPU phases are diagnostic only. GPU phase fields are `null`, not zero,
because the experiment has no timestamp-query/GPU-complete telemetry.

### Full-model anchors

| Scene | Splats | CPU mean/p95 | GPU mean/p95 | Paired GPU−CPU | GPU slower | Missed CPU/GPU |
|---|---:|---:|---:|---:|---:|---:|
| Kitsune | 279,199 | 11.500 / 12.036 | 24.166 / 25.526 | +12.666 | +110.1% | 0 / 180 |
| Flowers | 562,974 | 53.500 / 54.875 | 75.707 / 77.086 | +22.200 | +41.5% | 180 / 180 |

The independent full scenes agree with the ladder conclusion. Their different
frame costs at similar counts also demonstrate why point count alone cannot be
a universal policy input.

### Reuse cadence: refresh every second camera-change frame

| Splats | CPU mean/p95 | GPU mean/p95 | Paired GPU−CPU | GPU slower | Missed CPU/GPU |
|---:|---:|---:|---:|---:|---:|
| 200k | 6.851 / 9.563 | 10.425 / 16.286 | +3.486 | +50.9% | 0 / 0 |
| 500k | 36.141 / 39.303 | 46.954 / 60.582 | +10.630 | +29.2% | 180 / 180 |
| 700k | 60.280 / 64.726 | 75.403 / 95.573 | +15.198 | +25.2% | 180 / 180 |

Reusing order helps the GPU strategy materially because it avoids 25 compute
passes on half the frames, but CPU remains faster in all 9 pairs. At 200k the
CPU mean does not improve relative to interval 1 despite doing half as many
sorts; draw/queue behavior and run noise dominate the saved CPU work. Refresh
cadence must therefore be measured end to end rather than inferred from sort
time alone.

### Adaptive behavior

The 80-frame runs start measurement during the first GPU probe because 20
warmup frames already train 10 interval-2 refreshes. Each run presented 8 GPU
and 72 CPU frames, switched once during measurement, had zero GPU fallback,
and ended `cpu_stable`.

| Splats | Adaptive mean/p95/p99 | Missed (240) | Forced CPU mean | Over forced CPU |
|---:|---:|---:|---:|---:|
| 200k | 7.291 / 13.385 / 15.430 | 1 | 6.851 | +6.4% |
| 500k | 37.943 / 41.557 / 61.173 | 240 | 36.141 | +5.0% |
| 700k | 61.357 / 65.346 / 94.632 | 240 | 60.280 | +1.8% |

The overhead is the intentionally bounded cost of learning that GPU is worse.
It becomes a small fraction of steady frame cost at the larger tiers. A
separate valid 200k series used 120 measured frames per repetition: every run
presented 16 GPU and 104 CPU frames, made three backend switches, had no missed
frames or fallback, re-probed GPU, and again ended `cpu_stable` (means 7.544,
7.470, and 7.504 ms). This confirms periodic reconsideration without a fixed
point threshold.

These measurements do not prove that Adaptive will select GPU on a device
where GPU is faster; the deterministic state-machine tests cover that branch.
A multi-device runtime matrix remains necessary.

### Direct capacity boundary

Degree-3 SH rest data requires 45 floats, or 180 bytes, per splat in one Direct
storage binding. The Surface device deliberately requests downlevel storage
limits, making the effective binding limit 134,217,728 bytes on this path:

`floor(134,217,728 / 180) = 745,654 splats`.

| Tier | SH-rest bytes | Device result |
|---:|---:|---|
| 700k | 126,000,000 | CPU and GPU benchmarks complete |
| 750k | 135,000,000 | renderer creation returns `unsupported` |
| 1M | 180,000,000 | renderer creation returns `unsupported` |
| 1.5M | 270,000,000 | renderer creation returns `unsupported` |
| 2M | 360,000,000 | renderer creation returns `unsupported` |

The failure occurs before CPU/GPU ordering choice and must not be attributed to
sort performance. It is a release-path capacity gap. The experimental packed
and paged paths are potential remediations, but changing the release geometry
path is outside this branch's closeout boundary.

## What the experiment does and does not prove

CPU and GPU share the resident attributes and Direct raster shaders, but this
is a product-path A/B, not an isolated radix-kernel A/B:

- CPU performs near/far candidate construction, stable CPU radix sorting,
  compact ID upload, and candidate drawing.
- GPU performs key generation, 8 radix passes, no ID upload/readback, and
  sort-all/draw-all with shader clipping.
- `frame_wall_ms` ends after queue submission/present and has no GPU timestamp.
  In a sustained loop, queue backpressure makes it useful for throughput and
  product selection, but it is not a GPU-complete kernel duration.

The evidence is one Android device, one Surface resolution, one orbit speed,
and thermal status 0. The shared Rust implementation compiles/tests on the host
Metal path and compiles for Web/WASM, but this task did not run the tier matrix
on iOS, desktop Vulkan, or a browser. The ladder is cross-platform
infrastructure; the retained performance conclusion is Android-specific.

No competitor renderer was measured side by side. The result identifies what
must improve before such a comparison is meaningful; it does not claim parity
or superiority.

## API and release boundary

- The v0.1 C ABI layout and Android AAR public `NativeBridge` API are not
  widened. Existing sort-stat flag bits 9-14 now encode actual order backend,
  fallback, and Adaptive state.
- Backend forcing uses an Android-example-only JNI bridge and is absent from
  the public C header.
- The Rust API does add public backend/state enums, output telemetry fields, and
  `SurfaceRenderSession` backend accessors. This is an intentional pre-1.0
  experimental Rust API expansion, not a stable-API preservation claim.
- `SortedAlpha` / Direct remains the release-gated render path, and CPU remains
  its default ordering backend.

## Recommended closeout and next work

Merge this branch's reusable gains, keep CPU default, and stop expanding this
branch. The next work should be separate and ordered by product impact:

1. **Million-point release capacity.** Turn active-atlas/paging into a bounded,
   measured release candidate so Bonsai/Truck-class scenes can load on mobile.
   This is a larger competitive gap than the current sorter choice.
2. **Production GPU ordering.** Replace the serial global prefix with a
   hierarchical parallel scan or subgroup-capable variant; investigate wider
   radix digits, GPU candidate/screen compaction, fewer passes, and persistent
   scratch. Re-run the same paired ladder after each isolated change.
3. **GPU-complete observability.** Add capability-gated timestamp queries and
   asynchronous readback for benchmark telemetry, never blocking the render
   thread. Keep end-to-end frame wall as the product metric.
4. **Device fleet calibration.** Run the same byte-identical tiers on low/mid/
   high Android, Apple Metal, desktop, and browser WebGPU. Then tune probe
   windows/hysteresis from distributions instead of this single device.
5. **Thermal and load signals.** Feed platform thermal/power status as an
   additional guardrail where available, while retaining measurement-based
   fallback for platforms without it.
6. **Quality traces.** Add stationary, slow orbit, fast orbit, and scene-change
   image/error checks at several real-scene sizes. Keep performance subsets and
   quality anchors labeled separately.

This closes the original 200k question with evidence: CPU was the right choice
for this mobile target, but it should remain a measured default rather than a
permanent universal rule.

## Artifact locations

Raw retained artifacts are intentionally ignored build outputs under:

- `target/android-sort-benchmarks/retained-v2-i1-*`
- `target/android-sort-benchmarks/retained-v2-i2-*`
- `target/android-sort-benchmarks/retained-v2-adaptive80-i2-*`
- `target/android-sort-benchmarks/retained-v2-adaptive-i2-200k`
- `target/android-sort-benchmarks/retained-v2-anchor-i1-*`

Each root contains `experiment.json`, per-run `run.json`, full tagged logcat,
and canonical `manifest.json`, `frames.jsonl`, and `summary.json` artifacts.
External INRIA files and generated tiers are also ignored and must not be
redistributed without clarified model/upstream dataset rights.
