# Final Report: Full-Quality Native Rendering

> Status: completed on `codex/full-quality-native-rendering`. This report
> records the retained implementation and terminal full-resolution evidence as
> of 2026-07-23. Every available endpoint has a fresh exact-count result;
> unavailable physical-iPhone performance and intentionally deferred follow-up
> work are explicit non-claims, not substitutes for evidence. Failed or
> incomplete artifacts are excluded rather than averaged into the result.

## Executive result

The branch now loads and renders the complete 2,541,226-point SH3 Truck scene
at real display resolution on Mac Metal, Chrome/WebGPU, Nothing A065, and the
available iOS simulator. It does not sample points, lower SH degree, publish
partial pages, enable LOD, lower internal resolution, or upscale. CPU and GPU
order feed the same four-vertex `TriangleStrip` SortedAlpha raster path, and
Adaptive chooses using measured full-frame completion rather than a fixed
point-count rule.

Visual correctness is strong. Mac Direct-vs-Packed at 1920x1080 passes both
reviewed views with SSIM above 0.99996 and exact alpha; Web CPU/GPU/Adaptive
screenshots are byte/pixel exact; Android CPU-vs-GPU at native 2412x1080 has
center SSIM 0.9999999791 and exact alpha. The full Truck image is dense and
recognizable rather than the previously rejected 4.985%-resident sparse
result.

Performance is improved but does not yet match the pinned competitor's
observed throughput. The retained Mac CPU path now runs
43.169--43.748 FPS on the moving 1080p sequence; the PlayCanvas complete-Truck
terminal run observes 56.5846 FPS. That is not a strict same-quality ratio:
PlayCanvas uses fewer depth bits, quantized work attributes, fp16 projected
axes/color and an SH color-update threshold, and the same-camera images are not
pixel matched. At present, the honest conclusion is “correct full-resolution
renderer with a remaining raster/per-frame-work gap and a promising competitor
topology to borrow,” not competitor parity.

The production A/B also established that the current GPU producer is not the
right universal default. With complete Truck, stable full-32-bit order, forced
exact contributor compaction and identical output, `Preproject` is faster than
`PostSort` on every tested GPU endpoint: about 16.6% by paired mean completion
on M4, 12.2% in Chrome/WebGPU, and 46.8% on the A065. It is not promoted
globally yet because it is currently available only for Packed +
ProjectedQuadsExact + Compact, owns additional buffers, and lacks a production
producer-level fallback controller. The next product controller must compare
whole GPU plans, then let the existing outer CPU/GPU Adaptive policy compare
the winning GPU plan against CPU.

Complete Garden (5.835M) and Bicycle (6.132M) now load and render at 1920x1080
on Mac Metal and Chrome/WebGPU. Web Bicycle is important policy evidence: GPU
completion is about 67.6 ms versus CPU about 130.6 ms, and a 100-frame
Adaptive schedule becomes GPU-dominant. Android instead chooses CPU at every
measured rung. The selector must remain runtime-measured.

Fresh A065 capacity receipts extend that statement to the physical Android
device: Garden 5,834,784 and Bicycle 6,131,954 are both fully resident and
drawn at native 2412x1080 with SH3 and no quality fallback. Their four-frame
CPU means are 648.735 ms and 630.352 ms respectively. This proves exact-count
capacity through 6.13M on the device, not interactive performance.

## Retained architecture

### Exact resident scene

Production `PackedAtlas` is an exact-count resident representation. Its
publication invariant is:

```text
source_count == decoded_count == encoded_count
             == resident_count == addressable_count
source_sh_degree == resident_sh_degree
membership=all
sampling=disabled
lod=disabled
partial_scene=false
```

The renderer may cull a mathematically invisible point from one frame's work,
but it remains resident and addressable. A device that cannot admit all final
planes receives a structured failure before the new scene is published.
`PagedActiveAtlas` remains an explicitly selected diagnostic and is never a
silent capacity fallback.

The SH3 logical payload is:

```text
GPU resident static       = 156n + 80 * ceil(n / 256)
CPU upload staging        = 112n + 80 * ceil(n / 256)
CPU before Surface handoff= 124n + 80 * ceil(n / 256)
CPU retained after handoff= 12n
```

For complete Truck this is 397,225,416 B of static GPU payload and
285,411,472 B of upload staging. The largest individual plane is 16 bytes per
point, so a 128 MiB effective per-binding limit admits 8,388,608 points and
rejects 8,388,609. This boundary is not a total-memory promise; allocation
still runs under validation/out-of-memory/internal error scopes.

### Exact ordering with runtime choice

CPU and GPU classify visibility with the same explicit f32 sequence and
preserve stable source-ID ties. CPU uses a four-pass 8-bit radix. The portable
GPU path uses hierarchical scans and eight stable 4-bit passes over the full
32-bit depth key; a separately qualified native-macOS Resident path uses four
stable 8-bit passes. The wider path is not enabled on Android/Web/iOS or other
unqualified targets. Forced GPU failure is explicit; only Adaptive may
continue on CPU.

Adaptive's formal production metric is `FrameCompletion`: sampled CPU and GPU
frames are timed from frame start until that frame's submitted GPU work
completes. This captures the real tradeoff—GPU sorting competes with projection
and raster on the same device, while CPU sorting may overlap queued GPU work.
GPU timestamp queries for key generation/radix are retained as diagnostics
only and cannot select the backend. Paired tickets, hysteresis, bounded probes,
periodic re-probes, and failure cooldown prevent one delayed readback or one
failure from becoming a permanent policy.

### Retained raster optimizations

The current exact raster path keeps:

- a canonical four-vertex triangle strip (`BL, BR, TL, TR`) instead of the
  former six-vertex triangle list;
- conservative opacity-aware quad shrink to the `1/256` iso-contour while the
  fragment discard threshold remains strictly below `1/255`.

The first removes two redundant vertex invocations. The second removes only
outer square area that cannot contribute under the existing fragment rule.
Both retain the full draw-instance count, sorted order, SH data, resolution,
and pixel-to-Gaussian mapping inside contributing support.

On full Truck 1920x1080, the combined Mac A/B moved the earlier no-shrink
six-vertex result from roughly 34.045 FPS to roughly 42.070 FPS. On Android it
moved the corresponding CPU median from 192.953 ms to 173.138 ms. These are
optimization-attribution runs; the current-code platform qualification below
is the release-facing evidence.

An attempted extra fragment guard was rejected and completely removed. The
retained baseline ran at 42.141/41.915 FPS, the per-fragment-log variant at
41.377/41.510 FPS, and the flat-support variant at 40.990/41.137 FPS. See
`early-fragment-support-experiment.md`.

## Resolution and evidence rules

Formal evidence uses:

| Endpoint | Formal tested resolution | Dynamic resolution/upscaling |
| --- | ---: | --- |
| Mac Metal | 1920x1080 | disabled |
| Chrome/WebGPU | 1920x1080 | disabled |
| Nothing A065 | 2412x1080 native | disabled |
| iPhone 17 Pro simulator | 2622x1206 | disabled |

The checked-in 640x360 traces remain useful historical codec, phase-cost,
depth-parity, and quick-regression fixtures. They are too small to validate
what a user actually sees and are not used for formal image quality, product
throughput, or competitor parity. Historical full Garden/Bicycle 640x360
codec results remain in `findings.md`; they are not presented as new
full-resolution platform results.

## Current image-quality evidence

### Mac Direct oracle, complete Truck at 1920x1080

| View | Source = resident = addressable | Visible = drawn | SSIM | RGB MAE | Alpha |
| --- | ---: | ---: | ---: | ---: | --- |
| 0 | 2,541,226 | 1,886,298 | 0.9999693448 | 0.0000284315 | exact |
| 1 | 2,541,226 | 1,672,013 | 0.9999705553 | 0.0000261173 | exact |

Artifacts:

- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/direct-vs-packed-view0.json`
- `target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/direct-vs-packed-view1.json`

### Cross-backend image parity

- Chrome/WebGPU CPU, GPU, and Adaptive final frames are byte/pixel exact for
  the qualified cameras.
- Nothing A065 CPU-vs-GPU center crop: SSIM `0.9999999791`, normalized RGB MAE
  `4.278e-8`, alpha exact.

These checks accompany full-count receipts; image similarity is never used to
excuse a missing point or reduced SH degree.

## Current performance evidence

### Mac Metal, complete Truck, moving 1920x1080

Each run used 20 warmups, 80 measured frames, both official views, every-frame
sort/projection, full SH3, and no dynamic resolution or upscaling.

The original same-binary policy cohort was:

| Mode | Run 1 | Run 2 | Run 3 | Measured backend frames |
| --- | ---: | ---: | ---: | --- |
| forced CPU | 42.464 FPS | 41.898 FPS | 41.924 FPS | 80 CPU per run |
| forced GPU | 38.515 FPS | 38.434 FPS | 37.812 FPS | 80 GPU per run |
| Adaptive | 42.313 FPS | 42.390 FPS | 41.288 FPS | 76 CPU / 4 GPU per run |

All runs presented 100/100 scheduled frames and ended with no outstanding
terminal ticket or fallback. The 76/4 Adaptive split is a bounded GPU probe at
the end of the short run, not a GPU win.

The later retained bounded-preprocessing CPU cohort is
43.321/43.169/43.748 FPS (median 43.321 FPS), with mean preprocessing
4.54--4.97 ms and radix 4.82--5.12 ms. Those three runs also present 100/100
frames and close every terminal ticket. Current evidence favors CPU on this
host/workload, but forced GPU/Adaptive should be rerun with this current binary
before publishing a new three-way ratio.

Artifacts are under
`target/full-quality-final/final-mac-truck-1920x1080-frame-completion-20260723/`.
Current CPU logs are under
`target/full-quality-final/parallel-preprocess-ab-20260723/cpu-parallel-r*.log`.

### Chrome/WebGPU, complete Truck, moving 1920x1080

| Mode | Terminal window | Terminal throughput | Backend mix |
| --- | ---: | ---: | --- |
| forced CPU | 80 frames / 2324.8 ms | 34.412 FPS | 80 CPU |
| forced GPU | 80 frames / 2615.4 ms | 30.588 FPS | 80 GPU |
| Adaptive | 80 frames / 2166.3 ms | 36.929 FPS | 68 CPU / 12 GPU |

The terminal window is used instead of requestAnimationFrame cadence. Current
artifacts are under `target/full-quality-final/final-web-truck-1920x1080-*`.
The Adaptive aggregate being above the isolated forced-CPU run is observed run
variance/mixed scheduling, not evidence for a universal crossover.

### Nothing A065, complete Truck, native 2412x1080

Device: Nothing A065/Pong, Android 15/API 35, SM8475/Adreno. Each forced run
used 20 warmups and 80 measured frames; all source points and SH3 remained
resident/addressable.

| Mode | Per-run mean frame time | Median run | Equivalent FPS |
| --- | --- | ---: | ---: |
| forced CPU | 170.383613 / 179.335038 / 179.656487 ms | 179.335038 ms | 5.576 |
| forced GPU | 199.016450 / 203.805100 / 209.458163 ms | 203.805100 ms | 4.907 |
| Adaptive | 175.663313 / 184.785300 / 183.349937 ms | 183.349937 ms | 5.454 |

Each Adaptive run used 69 CPU and 11 GPU measured frames and ended
`cpu_stable`. Thermal status was 0 before/after all nine runs. CPU is presently
the better backend on this device; point count alone would not have predicted
the cross-platform result.

Artifacts:

- `target/full-quality-final/post-parallel-android-truck-2412x1080-cpu-gpu-adaptive-r3-20260723/`
- `target/full-quality-final/final-android-truck-2412x1080-quality-20260723/`

The exact native ladder provides the scale context. The older complete
three-repeat forced CPU/GPU cohort favors CPU at 50k, 100k, 200k, 300k, 500k,
1M, 1.5M and 2M. Fresh current-code anchors preserve that result:

| source splats | CPU frame wall | GPU frame wall | Adaptive frame wall / result |
| ---: | --- | --- | --- |
| 200,000 | 4.929 / 4.933 ms | 8.422 / 8.585 ms | 5.488 / 5.434 ms, `cpu_stable` |
| 1,000,000 | 27.950 / 29.391 ms | 40.289 / 40.189 ms | 29.620 / 29.591 ms, `cpu_stable` |

At 200k, CPU completion is 10.42--10.44 ms and fits the device's 16.67 ms
60 Hz budget; GPU completion is 17.60--18.09 ms and narrowly misses it. Thus
the user's original CPU decision for roughly 200k was correct on this A065,
but Web Bicycle proves it must not become a universal threshold.

Accepted current anchors are under
`target/full-quality-final/post-parallel-android-adaptive-ladder-2412x1080-20260723/200000-v2/`
and `.../1000000/`. The sibling `.../200000/` run is excluded: experiment
status `failed`, first run unfinished, and no complete artifact. The corrected
`200000-v2` rerun is the evidence.

The first continuous-motion Adaptive run on `7cabb6e` exposed a real
arbitration bug: projected-draw learning retained the shared probe owner while
every frame changed the camera, so order learning remained `cpu_learning` for
240/240 CPU frames. Commit `28f79ee` gives projected learning one grace turn,
then yields without consuming its sample until a stable frame exists; order
learning similarly releases ownership only after its terminal ticket.

Two fresh 120-frame full-Truck runs prove the fix. Each run uses 108 CPU and 12
GPU measured frames, ends `cpu_stable`, schedules and completes 140/140 order
measurements (128 CPU + 12 GPU), has zero failure, fallback, unsampled or
dropped receipts, and keeps maximum revision lag at zero. Frame means are
174.131 and 167.533 ms; GPU completion remains slower than CPU completion in
both runs, so the selected result is evidence-driven rather than a hard-coded
point threshold. The earlier logcat-truncated 240-frame rerun is excluded; only
`target/full-quality-final-v4/android-a065-truck-adaptive-moving-fixed-120x2-28f79ee/`
is retained.

### Exact GPU producer A/B

`PostSort` and `Preproject` were compared in the same binaries with complete
Truck, full SH3, every-frame full-32 stable order, forced Compact draw,
ProjectedQuadsExact, and fixed formal resolution. All formal producer tickets
closed with exact `S/C/D` receipts and no failure.

| Endpoint | Cohort | Preproject / PostSort mean completion | Image gate |
| --- | --- | ---: | --- |
| M4 Metal | 6 balanced AB/BA pairs, 20 + 80 frames | median ratio `0.83405` (about 16.6% faster), 95% bootstrap CI `0.81787--0.86388` | all 12 PNGs byte-identical |
| Chrome/WebGPU on M4 | 4 balanced AB/BA pairs, 20 + 80 frames | median ratio about `0.87797` (about 12.2% faster) | all 8 PNGs byte-identical |
| A065/Adreno 730 | 2 interleaved descriptive pairs, 20 + 80 frames | combined ratio `0.53217` (about 46.8% faster) | Truck/background render crops byte-identical; whole-screen differences are Android UI timing only |

The Android cohort is descriptive rather than a formal paired series because
its artifact pairing field is null. It nevertheless uses identical source,
APK/native hashes, camera, resolution and thermal status 0. The complete paths
are under `target/full-quality-final-v4/*-truck-producer-ab-7cabb6e/`.

This result does not replace CPU/GPU Adaptive with “always Preproject”. It
changes what the GPU lane should eventually compare. The product-level plan is
a composite controller over:

1. `PostSort + Candidate`;
2. `PostSort + Compact`;
3. `Preproject + Compact`.

The best supported GPU plan is then compared with CPU by the existing outer
Adaptive policy. Until that controller has transactional admission, fallback,
failure cooldown and per-device evidence, the universal product default stays
`PostSort`; `Preproject` remains an exact opt-in diagnostic.

### Available Apple runtime

The iPhone 17 Pro simulator loads complete Truck at 2622x1206 through CPU and
Adaptive and reports exact source/decoded/encoded/resident/addressable counts
with SH3. The simulator reports GPU ordering unsupported, so Adaptive stays on
CPU and forced GPU fails explicitly. These are integration/correctness
receipts only; simulator timings are not physical-iPhone performance.

The current v3 CPU and Adaptive artifacts each have two warmups, eight measured
frames, ten successful terminal receipts, zero failure/unsampled measurement,
nominal thermal state, and exact 2622x1206 presentation. CPU and Adaptive final
screenshots are byte-identical (SSIM 1.0, RGB/alpha MAE 0). Forced GPU's three
`rc=4 error=unsupported` records are a capability receipt, not a rendered or
timed sample. Accepted paths:

- `target/full-quality-final/ios-sim-current-truck-2622x1206-cpu-moving-w2-m8-v3-20260723/artifact/`
- `target/full-quality-final/ios-sim-current-truck-2622x1206-adaptive-moving-w2-m8-v3-20260723/artifact/`
- `target/full-quality-final/ios-sim-current-truck-2622x1206-gpu-unsupported-v3-20260723/`

### Current complete Garden and Bicycle

Mac Metal short capacity/runability receipts use both official 1920x1080 views,
forced CPU, two warmups and four measured frames:

| Scene | source = resident | visible/drawn views | mean call wall | isolated completion | max RSS / peak footprint |
| --- | ---: | --- | ---: | ---: | --- |
| Garden | 5,834,784 | 4,609,628 / 4,226,208 | 23.058 ms | 77.644 ms | 1.632 / 2.640 GB |
| Bicycle | 6,131,954 | 4,321,800 / 3,927,910 | 23.893 ms | 74.061 ms | 1.751 / 2.794 GB |

All six frames and terminal tickets succeed per scene. These runs establish
full-scene admission and exact view counts, not sustained FPS. The desktop text
receipt currently omits decoded/encoded/addressable fields, which is an open
telemetry gap even though the fail-closed resident path enforces them.

Chrome 150/WebGPU manifests directly receipt all five counts and SH3 for both
scenes at 1920x1080. Short Garden CPU completion averages 122.325 ms. Short
Bicycle CPU/GPU completion averages 130.575/67.550 ms. A longer Bicycle
Adaptive run (20 warmups + 80 measured) uses 17 CPU and 63 GPU measured frames;
completion means are 129.976/67.805 ms and it ends `cpu_probe`, meaning GPU is
incumbent while CPU is periodically rechecked. Every measured submission has a
terminal, all failure streams are empty, and CPU/GPU/Adaptive final PNG hashes
are identical.

Artifacts:

- `target/full-quality-final/mac-large-scenes-current-20260723/`
- `target/full-quality-final/web-large-scenes-current-20260723/`

The same scenes also pass final-code A065 capacity runs at native 2412x1080:

| Scene | source = resident | mean frame | mean CPU preprocess / sort | mean visible / contributor / drawn |
| --- | ---: | ---: | ---: | --- |
| Garden | 5,834,784 | 648.735 ms | 30.287 / 38.348 ms | 4,417,918 / 2,266,042 / 4,417,918 |
| Bicycle | 6,131,954 | 630.352 ms | 33.817 / 44.794 ms | 4,124,855 / 1,701,309 / 4,124,855 |

Both use two warmups and four measured moving frames, preserve all five scene
counts and SH3, report thermal status 0, and have no quality fallback. They are
capacity/correctness receipts rather than sustained performance cohorts:
`target/full-quality-final-v4/android-a065-{garden,bicycle}-capacity-76a9267/`.

## Competitor comparison

The pinned PlayCanvas run uses complete Truck, 1920x1080, 600 measured terminal
frames, full membership, and no dynamic-resolution/upscaling shortcut. Its
terminal window is 10,603.6 ms, or 56.5846 FPS.

The median of the three current Mac CPU runs is 43.321 FPS, so the observed
throughput remains lower. It is incorrect to turn those two numbers into a
strict equal-quality percentage. At the pinned PlayCanvas revision:

- Truck selects approximately 20 depth-key bits/five radix-nibble passes,
  versus this renderer's complete 32-bit stable key;
- the default compact 24-byte work buffer quantizes color, rotation, scale and
  opacity;
- projected axes/color are packed to fp16 in the projection cache; and
- a default 10-degree SH color-update threshold permits resolved-color reuse,
  while the artifact does not record per-frame SH update count.

The competitor manifest's count source is `totalActiveSplats`, not the
projector `renderCounter`, so it proves full active residency but not actual
per-frame contributor/draw count. The recorded same-camera frame-0 comparison
has SSIM 0.938175, normalized RGB MAE 0.028223, and exact alpha. That does not
rank either image; it confirms they are not the same numerical/visual profile.
Primary-source links and exact artifact fields are recorded in `findings.md`.

The available A065 reference reaches a queue-terminal cadence of 73.821 ms per
frame (13.546 FPS) for complete Truck at native 2412x1080. This renderer's
exact Preproject cohort has a mean frame wall of about 101.620 ms (9.84 FPS).
The fields are not identical—PlayCanvas does not expose per-frame GPU
completion or exact contributor/drawn counts in this path—so this is a
directional gap, not a strict percentage ranking. It still shows that the
large remaining cost is real even after the new producer win.

Competitor research still supports compact planar residency, visible-work
generation, hierarchical scans, stable radix, and tile-aware raster work. It
does not justify sampling, LOD, reduced SH, reduced resolution, or incomplete
pages under this project's full-quality label. The next performance work must
reduce real projection/raster/per-frame cost while preserving the current
count, SH, image, and resolution receipts.

The concrete Phase 2 design documented in
[phase2-preproject-c-architecture.md](phase2-preproject-c-architecture.md)
is now implemented and measured: build exact deterministic `C` before GPU
sorting, run stable full32 radix only over `C`, and retain hardware raster plus
the existing CPU/GPU/Adaptive product choices. Tiled compute remains
conditional no-drop research, not the default.

## Known limits and follow-up work

These items are deliberately outside this branch's completion boundary. None
permits a quality fallback in the completed path:

- Add the composite GPU plan controller described above. `PostSort` remains
  the default until that controller can admit, measure, cool down and fall back
  transactionally; `Preproject` remains limited to Packed +
  ProjectedQuadsExact + forced Compact.
- Continue exact contributor/raster/per-frame-work optimization. Define an
  explicitly named competitor precision and SH-update profile before any
  equal-quality parity claim; reject changes that win by lowering quality.
- Keep radix-8 enabled only on qualified platforms. Android/Web/iOS retain the
  portable exact radix-4/base-16 path until wider-pass qualification proves the
  same source-ID order and image.
- Remove SPZ's duplicate wide `SceneBuffers` peak in a separate loader change.
  PLY already transcodes directly to Resident; SPZ remains exact but not yet
  peak-memory optimal.
- Add decoded/encoded/addressable fields to the desktop text receipt. The
  fail-closed resident path already enforces them, but the text artifact does
  not repeat all five values.
- Obtain a physical iPhone and signing identity before making an iOS device
  performance claim. Simulator integration is terminal evidence only for the
  runtime that was actually available.
- Turn the short Garden/Bicycle capacity cohorts into sustained thermal/power
  cohorts when product tuning begins. Current results prove exact admission
  and rendering, not interactive frame rate.

## Explicit non-claims

- 640x360 is not the target presentation resolution.
- The renderer has not yet caught PlayCanvas's observed throughput, and the
  current PlayCanvas number is not a strict equal-quality denominator.
- Current Garden/Bicycle Mac/Web/Android runs prove full-resolution runability;
  the short capacity cohorts are not sustained product FPS results.
- Current Adaptive selects CPU versus GPU ordering while using the qualified
  `PostSort` default; it does not yet learn the PostSort/Preproject producer
  axis.
- Producer, arbitration and final capacity cohorts use distinct clean evidence
  commits (`7cabb6e`, `28f79ee`, and `76a9267`); they are not misrepresented as
  one final-binary performance cohort.
- The iOS simulator is not an iPhone performance proxy.
- No LOD, sampling, lower SH, lower resolution, or incomplete paging result may
  be reported as full quality.
- A failed or incomplete artifact is not evidence in either direction.
