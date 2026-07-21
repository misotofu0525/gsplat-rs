# Competitive Verification Plan

## Purpose

This document defines the evidence required to implement and promote the
[native-first competitive architecture](design.md). It separates correctness,
internal optimization, native product comparison, Web renderer comparison, and
large-scene scalability so that one successful benchmark cannot be used to
claim all five.

## Verification Principles

1. Compare the same scene, camera trace, output resolution, SH degree, clipping
   range, background, and quality policy unless a test is explicitly labeled
   “best-practice product configuration.”
2. Store frame distributions and phase timings; do not publish FPS or averages
   alone.
3. Record adapter, backend, OS, browser, driver, build commit, feature flags,
   source format, cache state, and thermal state with every result.
4. Randomize run order and require sustained tests on mobile devices.
5. Keep raw machine-readable artifacts, screenshots, and logs for every claim.
6. Treat build, simulator, browser smoke, and physical-device runtime evidence
   as separate gates.
7. Fail closed when path/backend signals are missing. A benchmark without proof
   of the intended renderer path is invalid.

## Comparison Tracks

### Track 0: Internal A/B

Compare current gsplat-rs and the proposed path on the same backend. This is the
first gate for every implementation phase and establishes whether a change is
responsible for an observed result.

Valid claim: “the packed path reduced measured GPU scene bytes by X and changed
p95 frame time by Y on the declared device.”

Invalid claim: “gsplat-rs is faster than PlayCanvas.”

### Track 1: Native Product Advantage

On the same Android or iOS device, compare:

- gsplat-rs in the native sample/app integration; and
- a pinned PlayCanvas comparison viewer in the device's normal supported
  browser, using a local or controlled HTTPS server.

This measures the product customers can deploy, including browser/runtime and
integration differences. It is the primary commercial comparison, but it must
be labeled “native app vs mobile browser.”

### Track 2: Web Renderer Parity

In the same desktop browser session, compare:

- gsplat-rs Web/WASM/WebGPU; and
- the pinned PlayCanvas Web viewer.

Use the same canvas backing dimensions and disable browser extensions/devtools
during measured runs. This track is the basis for the “desktop Web does not
fall behind” claim.

### Track 3: Large-Scene Scalability

Compare bounded working-set behavior independently of small-scene speed:

- total source splats and bytes;
- active/resident/drawn splats;
- CPU and GPU byte budgets;
- page fetch/decode/upload/eviction behavior;
- first-frame and refinement timing;
- image continuity during movement and camera jumps.

## Pinned Competitor Harness

The initial reference revision is PlayCanvas Engine commit
`d5fe88878e338936fe763bbce1a58bc315e89cbe`, inspected on 2026-07-11. Before a
qualification run:

1. pin the exact engine commit and viewer/harness commit;
2. vendor or lock package dependencies;
3. record whether PlayCanvas selected WebGPU or WebGL;
4. record sort mode, active budget, compressed format, SH bands, LOD policy,
   and dynamic-resolution state;
5. keep the harness source and build manifest beside the benchmark artifacts.

Upgrading the reference revision starts a new comparison series. Results from
different competitor revisions must not be merged into one table.

## Workload Profiles

### Renderer Parity Profile

- Same numerically equivalent scene data.
- Same camera matrices and deterministic trace.
- Same backing resolution and clipping planes.
- Same SH degree and background.
- All splats active; LOD and adaptive quality disabled.
- Dynamic resolution disabled.
- Warm scene and shader state for steady-state measurements.
- Visible/drawn counts and screenshots must pass before timing is accepted.

This is required for Track 0 and Track 2 small/medium-scene claims.

### Product Best-Practice Profile

- Each product may use its recommended compressed transport, cache, and LOD.
- Both use the same source scene, camera trace, output resolution, and a frozen
  image-quality floor.
- Source bytes, network bytes, active splats, resident splats, SH degree, and
  cache state are reported.
- Default settings are captured, then any tuned settings are listed explicitly.

This profile answers which integration delivers the better user experience. It
cannot be presented as an isolated renderer microbenchmark.

### Fixed-Budget Profile

- Same maximum active splats and maximum GPU scene bytes.
- Same quality floor and camera trace.
- Used to compare LOD choice, streaming stability, and scheduling efficiency.

### Fixed-Frame-Budget Profile

- Same p95 frame target, initially 16.67 ms for 60 Hz and 33.33 ms for 30 Hz.
- Compare the maximum stable active splats or image quality each renderer can
  sustain.
- Report which frame budget was used; do not mix 30 Hz and 60 Hz results.

## Dataset Matrix

All dataset identity is stored as source SHA-256 plus conversion command/tool
revision. Large or restricted assets remain local and are not silently added to
the repository.

| ID | Approximate size | Purpose | Required stage |
|----|------------------|---------|----------------|
| `minimal_ascii` | Repository fixture | Parser, renderer, image, FFI, and smoke correctness | Every change |
| `kitsune` | 279,199 splats / about 65.9 MB PLY | Small-scene latency and regression baseline | Phases A-B |
| `flowers` | 562,974 splats | Medium scene, mobile thermal baseline, degree-3 pressure | Phases A-B |
| `medium_1m` | About 1 million splats | Direct/packed crossover and memory scaling | Phases B-C |
| `nandi` | About 3.45 million splats | Reproduce/remove the current 128 MiB binding failure | Phases B-D |
| `spatial_10m` | At least 10 million splats, reproducibly generated or licensed | Residency, LOD, camera jump, queue, and long-run behavior | Phases D-E |

Each scene must have a manifest containing:

- source hash, format, byte length, splat count, SH degree, bounds;
- conversion tool and exact revision/options;
- expected visible/drawn count for locked static cameras;
- deterministic camera trace hash;
- any redistribution restriction.

## Device and Browser Matrix

The exact model list is frozen before a release qualification and included in
the result manifest. The minimum matrix is:

| Class | Minimum coverage | Primary comparisons |
|-------|------------------|---------------------|
| Android high | One current flagship Vulkan device | Native vs Chrome; sustained 60/30 Hz |
| Android mid | One representative mid-range Vulkan device | Native vs Chrome; memory and thermal |
| Android constrained | One older/lower-memory supported arm64 device | Fallback, allocation failure, 30 Hz |
| iPhone current | One current-generation iPhone | Native Metal vs Safari |
| iPhone older | One supported older iPhone | Native Metal vs Safari; thermal/memory |
| macOS desktop | Apple Silicon, including the existing M4 Pro baseline where available | Chrome and Safari Web-vs-Web; native internal A/B |
| Windows desktop | One representative discrete or integrated GPU | Chrome/Edge Web-vs-Web |

Firefox may be added only when the required WebGPU path is available and its
backend signal is recorded. Simulator results do not satisfy physical-device
rows.

## Controlled Environment

### Build

- Release/optimized renderer in every product.
- Same gsplat-rs commit and feature set across its native and Web builds.
- Pinned PlayCanvas revision and production build.
- Debug validation layers disabled for performance runs and enabled in a
  separate correctness pass where supported.
- Benchmark overlay/logging sampled or buffered so it does not write per frame.

### Display and Camera

- Freeze canvas/render-target backing dimensions, not only CSS size.
- Record DPR, refresh rate, orientation, and presentation mode.
- Run a fixed-resolution parity mode and a full-viewport product mode
  separately.
- Use the same timestamped camera matrices; do not rely on two independently
  implemented orbit formulas.
- Disable dynamic resolution in parity mode.

### Mobile State

- Start within a declared battery range and with external power state recorded.
- Record OS low-power mode, application foreground state, brightness, refresh
  mode, and thermal state.
- Cool devices to the same pre-run thermal state.
- Randomize A/B order across at least five paired runs.
- Stop and repeat a run when the OS reports a thermal state outside the frozen
  protocol before measurement begins.

### Browser State

- Fresh profile or documented clean benchmark profile.
- Devtools closed during measurement.
- Service-worker/HTTP/disk cache state explicitly cold or warm.
- One measured viewer tab; background tabs and extensions disabled.
- Actual backend/path signal captured in the result.

## Benchmark Scenarios

### B1: Static Steady State

- Fixed camera after warm-up.
- Isolates raster and presentation costs.
- Minimum 600 measured frames or 30 seconds, whichever is longer.
- First Phase E pair: Kitsune SHA-256 `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`,
  shared trace `phase-e-kitsune-static-640x480-v1` SHA-256
  `edbadff803a3a485ec8467e9623133c6624206568cd4454cda976bda7ab5aab4`,
  640×480 backing surface at DPR 1, dynamic resolution and LOD disabled, 120
  warmup frames, and 3,600 measured rAF frames.

### B2: Deterministic Orbit

- Reuse the existing gsplat-rs trace semantics, but export the actual timestamped
  camera matrices for both harnesses.
- Minimum 1,800 measured frames for qualification.
- Exercises sort interval, async publication, visibility, and upload behavior.

### B3: Camera Jump

- Alternate between distant viewpoints with little overlap.
- Measures cancellation, coarse coverage, page faults, time-to-coarse, and
  time-to-refined frame.

### B4: Cold Load

- Clear product and HTTP/disk caches according to the manifest.
- Measure metadata ready, first request, bytes received, decode complete, first
  upload, first renderable frame, and target-quality frame.
- Run at least five independent loads.

### B5: Warm Load

- Preserve the declared compressed cache but recreate renderer/session state.
- Distinguishes transport/cache value from GPU construction.

### B6: Sustained Motion

- Run deterministic motion for 30 minutes.
- Sample frame distributions, memory, active/resident counts, page queues,
  thermal state, and energy signals.
- Report first 2 minutes, middle 5 minutes, and final 5 minutes separately so
  thermal inversion is visible.

### B7: Memory Pressure and Recovery

- Run with a reduced configured budget and, where safely available, system
  memory pressure.
- Verify bounded eviction, no stale-slot publication, actionable allocation
  failure, and recovery after background/foreground lifecycle changes.

## Required Telemetry

### Already available or partially available

- adapter/backend/driver metadata;
- submit-frame and GPU-complete time;
- CPU preprocess, sort, encode/submit, and GPU-wait phases;
- render-call and frame-wall timing on surface paths;
- visible and drawn splat counts;
- deterministic benchmark configuration;
- RSS min/max/growth in long stability mode.

### Required additions

| Category | Fields |
|----------|--------|
| Distribution | frame count, p50, p90, p95, p99, max, missed-frame counts at the chosen budget |
| Scene memory | compressed CPU bytes, decoded CPU bytes, hot-record bytes, SH-sidecar bytes by degree, atlas bytes allocated/used, order/scratch bytes, peak RSS |
| Streaming | requested/ready/resident/evicted pages, queue depth, cancellations, cache hits, bytes fetched/decoded/uploaded |
| LOD | selected nodes and resident SH pages by degree, active/resident/drawn splats, coarse coverage, refinement latency |
| Color refresh | pages/splats refreshed, SH degree, bytes written, refresh p50/p95 time, angular threshold |
| Sorting | policy, interval, input count, p50/p95 sort time, stale results dropped, residency revision |
| Loading | metadata, fetch, decode, upload, first-renderable, target-quality timestamps |
| Device state | OS/browser/build, backend, resolution/DPR/Hz, battery/power, thermal state, energy signal where available |
| Quality | screenshot IDs, image metrics, visible/drawn agreement, trace and camera hash |

GPU memory may not be directly observable on every Web/browser backend. In that
case report renderer-owned allocated bytes, explicitly label them as such, and
do not present them as total process GPU memory.

For PlayCanvas, the harness must report source texture streams and work-buffer
streams separately. At the pinned revision, the declared compact work-buffer
formats sum to 24 bytes per splat despite their 20-byte source comment. The
comparison report must show both the upstream label and the byte calculation,
then add source, order, and scratch allocations rather than treating the work
buffer as total residency.

## Artifact Contract

The following layout is **proposed and does not exist yet**:

```text
target/benchmarks/<series>/<device>/<track>/<dataset>/<run>/
  manifest.json
  frames.jsonl
  summary.json
  runtime.log
  screenshots/
  image-diff.json
  trace.json
```

`manifest.json` must include:

- repository and harness commits plus dirty-state flags;
- device/OS/browser/adapter/backend/driver;
- build profile and feature flags;
- dataset and trace hashes;
- source format, cache state, quality, LOD, sort, resolution, and budget settings;
- declared and measured bytes for source, hot record/work buffer, SH sidecar,
  order, page metadata, allocator padding, and sort scratch;
- run start/end time, randomization position, and thermal precondition;
- raw artifact hashes.

`summary.json` is derived from `frames.jsonl`; it is never the only stored
evidence. The result schema must be versioned.

## Correctness and Quality Gates

## Gate Policy Reform

Phase completion uses two classes of evidence:

- **Hard correctness gates must pass:** `SortedAlpha` correctness, matched
  count/small-scene image gates, deterministic no-persistent-hole traces,
  stale/cancel/generation exclusion, fixed atlas-slot bounds, non-resident draw
  exclusion, stable non-zero native Surface/Web output, and no crash.
- **Performance and coverage values are soft observations or claim qualifiers:**
  fixed ratios such as 1.10x or 25%, 30-minute RSS, 40–48-byte averages,
  thermal/energy wins, and complete device/browser/dataset matrices. Record the
  result and narrow the claim when missed or unavailable; do not weaken quality
  or keep optimizing solely to cross the number.

Complete means the capability and raw evidence exist and every outward claim is
within the achieved evidence scope. It does not mean every aspirational number
or device class passed.

### Existing release path

- Workspace tests pass.
- GPU-required `SortedAlpha` conformance passes on a native adapter.
- Direct-path visible/drawn counts and output remain unchanged.
- Web, Android, Apple, FFI, and packaging verification follow
  [the canonical verification guide](../../../../handbook/VERIFICATION.md).

### Packed path

- Static visible/drawn counts equal the direct reference when LOD is disabled.
- For matched gsplat-rs direct-vs-packed images, initial provisional gate:
  mean absolute RGB error no greater than 1/255 and no more than 0.1% of pixels
  exceeding 3/255 absolute error. Freeze or tighten the threshold only after
  inspecting reference images across the dataset matrix.
- No NaN/Inf positions, covariance, color, or sort keys.
- Camera/scene/residency revision tests prove stale async results are dropped.
- Slot reuse stress tests show no page cross-contamination.
- The 20-byte hot-record layout is checked from actual texture allocations, not
  only from Rust structure or source-format sizes.
- Degree 0/1/2/3 sidecar transitions remain within the frozen image thresholds
  and cannot expose uninitialized coefficients.

### Cross-engine parity

Different engines can have small convention and raster differences. Before
timing, freeze a static-camera threshold from inspected reference images. The
initial acceptance target is SSIM at least 0.99 with matched visible/drawn
counts and no systematic holes, color shift, or opacity change. The threshold,
metric implementation, crop/mask, and reference images become part of the
series manifest and cannot be changed after seeing performance results.

The Phase E v1 metric is repository-owned `ssim-luma-srgb-window8`: composite
the 640×480 canvas onto its recorded background, retain sRGB byte values,
convert each pixel to Rec.709 luma (`0.2126 R + 0.7152 G + 0.0722 B`), compute
sample-variance SSIM over non-overlapping 8×8 windows with `K1=0.01`,
`K2=0.03`, and `L=255`, then average all windows. No crop, registration,
rescale, or mask is permitted. `tests/perf/compare-image-ssim.mjs` self-checks
identical input as exactly 1 before evaluating the pair.

### Streaming/LOD

- A resident coarse ancestor covers a page until its selected refinement is
  renderable.
- With a local warm page cache, a camera jump restores coarse coverage within
  two presented frames.
- No page remains permanently requested/uploading after cancellation or error.
- Active slots never exceed the configured limit; atlas-owned bytes never
  exceed the configured budget plus allocator alignment recorded in the
  manifest.
- A deterministic repeated trace produces the same selected-page sequence when
  timing/adaptive policy is disabled.

## Performance and Resource Observations / Claim Qualifiers

These values decide whether the corresponding performance claim may be
promoted. Except where a value also proves a hard safety bound, they do not
block capability completion.

### Internal packed-path observations

- At least 3x lower measured GPU scene attribute bytes than the current
  base-plus-degree-3-SH representation.
- Hot render record is no larger than 20 bytes per active splat.
- Full degree-3 attribute payload is no larger than 68 bytes per active splat
  before the 4-byte order ID and explicitly reported scratch.
- No p95 GPU-complete or end-to-end frame-time regression greater than 10% on
  any release-gated device for Kitsune and Flowers.
- Nandi no longer fails because one attribute binding exceeds 128 MiB.
- Peak decoded CPU memory is bounded and reported; compressed input must not
  create an unbounded duplicate of the whole scene.

### Memory Architecture Leadership Claim Qualifier

A claim that the gsplat-rs active representation is more memory-efficient than
PlayCanvas requires all of the following under the same scene, active count,
SH degree, and quality profile:

- total renderer-owned GPU bytes include source/resource textures, active/work
  textures, order, sort scratch, page metadata, and allocator padding;
- gsplat-rs total is at most 0.80x the measured PlayCanvas total on Kitsune,
  Flowers, and `medium_1m`;
- gsplat-rs does not retain a second permanent GPU copy of active geometry/color;
- CPU compressed/decoded/staging memory is shown beside GPU bytes so duplication
  is not merely moved to system memory;
- the result passes frame-time and image gates, preventing a memory win obtained
  through unacceptable decode ALU or quality loss.

The 20-byte hot-record target alone is an internal layout result. It does not
earn the competitor memory claim until the total-residency gate passes.

### Native Leadership Claim Qualifier

A device-specific “native advantage” claim is allowed when:

- quality gates pass;
- gsplat-rs native has at least 25% lower p95 end-to-end frame-wall time than
  PlayCanvas mobile Web **or** sustains at least 1.5x active splats at the same
  p95 frame budget;
- the advantage appears in at least four of five paired randomized runs;
- the 30-minute final window still has at least a 15% p95 advantage and does
  not enter a worse declared thermal state;
- loading, peak memory, and failures are reported beside frame time.

A broad mobile message requires passing at least two Android classes and both
iPhone classes. Otherwise the claim names only the tested model(s).

GPU-complete time is a required diagnostic where both products expose a
comparable timestamp path, but it is not substituted for end-to-end frame wall
in the product claim. If present timing cannot be observed directly, the
manifest must name the frame-boundary proxy and include missed-frame counts.

### Desktop Web Parity Claim Qualifier

For Kitsune, Flowers, and `medium_1m` in the Renderer Parity Profile:

- gsplat-rs p95 end-to-end frame time is no more than 1.10x PlayCanvas;
- gsplat-rs p99 is no more than 1.20x PlayCanvas;
- renderer-owned GPU bytes and peak process memory are no more than 1.25x
  PlayCanvas unless the report explains an API-observability mismatch;
- no scene/backend/path/quality gate fails.

The claim applies only to browsers/devices that individually pass. Results are
not averaged across browsers to hide a failing backend.

### Large-scene Claim Qualifier

- Total scene size can exceed the active GPU working set.
- A mobile profile completes the 30-minute `spatial_10m` trace with its declared
  active budget and without unbounded queue/memory growth.
- Renderer-owned GPU bytes stay within configured budget plus 5%; CPU
  compressed/decoded caches stay within their budgets plus 10% transient
  staging headroom.
- The measured average attribute payload across the trace is 40-48 bytes per
  active splat before order/scratch, with the degree 0/1/2/3 residency mix
  included in the artifact.
- Evictions do not monotonically grow memory and stale publications remain zero.
- Cold-load and camera-jump quality gates pass under a recorded network profile.

## Statistical Protocol

- At least five paired runs for competitor comparisons.
- Randomize A/B order within each pair.
- Use the median paired ratio as the headline and show every run.
- Report p50/p95/p99/max within runs.
- Include a bootstrap 95% confidence interval for the paired median ratio when
  the result is used externally.
- Do not remove outliers unless the manifest contains a predeclared exclusion
  rule and the excluded raw run remains stored.
- A result whose confidence interval crosses the claim threshold is
  inconclusive, not a pass.

## Existing Verification Commands

These commands exist today and remain mandatory where their scopes apply:

```bash
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10 --max-avg-gpu-complete-ms 250
bash tests/ffi/run-ffi-smoke.sh
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

Web implementation changes additionally use the current WASM/package and
browser routes from `handbook/VERIFICATION.md`; Android and Apple changes use
their build, smoke, simulator, and physical-device benchmark routes. A build or
simulator run does not replace a real-device result.

## Proposed Harness Work

The following tools/scripts are requirements, not current repository commands:

1. Extend `bench-runner` and platform `BENCHMARK_RESULT` output with histogram or
   raw-frame export and the telemetry fields above.
2. Add a versioned result-schema library shared by native, Web, and analysis.
3. Add a deterministic camera-trace export/import format used by both gsplat-rs
   and the PlayCanvas harness.
4. Add a pinned, self-hosted PlayCanvas comparison viewer with explicit backend,
   sort, LOD, and quality signals.
5. Add a comparison runner that validates manifests, rejects mismatched quality
   profiles, computes paired statistics, and generates tables from raw data.
6. Add screenshot capture and image-diff tooling for static cameras and LOD
   sequences.
7. Add Android/iOS thermal and lifecycle sampling at low frequency.

Proposed command names must be finalized only when the implementation files
exist and are added to `handbook/VERIFICATION.md`.

## Claim Ladder

| Level | Required evidence | Allowed language |
|-------|-------------------|------------------|
| 0: Architecture | Design review only | “Designed to reduce memory / support bounded residency.” |
| 1: Internal proof | Track 0 correctness, memory, and A/B artifacts | “The packed path is X smaller/faster on tested devices.” |
| 1M: Memory architecture | Total-residency comparison passes on the required datasets | “The active representation used X% fewer renderer-owned GPU bytes than the pinned PlayCanvas revision under profile Y.” |
| 2: Device-specific native win | Track 1 paired and sustained gate on named device | “On device X, native gsplat-rs outperformed PlayCanvas in browser under profile Y.” |
| 3: Mobile native advantage | Required Android/iPhone classes pass | “Native Android/iOS SDK is faster under the published mobile matrix.” |
| 4: Desktop Web parity | Track 2 passes per named browser/device | “Desktop Web performance is within the published parity band.” |
| 5: Large-scene scalability | Track 3 memory, LOD, load, quality, and stability gates | “Streams scenes larger than the active GPU working set up to the published tested scale.” |

Do not use “faster,” “more advanced,” “supports N splats,” or “production-ready
streaming” without naming or linking the evidence level, device matrix, dataset,
quality profile, and date.

## Phase Verification Checklist

### Phase A: Baseline

- [ ] Pin PlayCanvas and comparison harness revisions.
- [ ] Freeze dataset/trace manifests.
- [ ] Add raw frame distributions and environment metadata.
- [ ] Capture current native/Web baselines and physical-device thermal runs.
- [ ] Verify no competitor claim is derived from historical internal A/B data.

### Phase B: Packed Atlas

- [x] Direct-vs-packed image/count gates pass.
- [x] Binding/byte preflight and structured errors pass.
- [x] Hot record is at most 20 bytes and full degree-3 attributes are at most 68 bytes before order.
- [x] At least 3x full-degree-3 scene-attribute byte reduction is measured.
- [x] Actual texture allocation bytes match the declared accounting.
- [x] Kitsune/Flowers p95 regression ceiling passes on every release-gated
  device under the matched sync-orbit qualification protocol
  (`sort_interval=4` on Android; existing iOS/desktop passes). Indexed at
  `target/benchmarks/phase-b/phase-b-device-p95-index.json` (v2, raw p95).
  Android `sort_interval=2` sync remains a documented residual (~1.12).
- [x] Nandi binding failure is removed on the target adapter profile (packed preflight; SH not in storage bindings).

### Phase C: Compression

- [x] Format compatibility/licensing decision is recorded (Niantic SPZ MIT, v4).
- [x] Decoder corpus (synthetic degrees 0/1/3), malformed-input, cooperative
      cancellation, and per-load memory-bound tests pass in `gsplat-io-spz`.
- [x] Unit-level PLY-vs-SPZ count/attribute mapping gate passes.
- [x] PLY-vs-compressed offscreen image parity passes on the minimal degree-0
      fixture (device qualification scenes remain optional).
- [x] Cold/warm bytes, time-to-first-frame, and peak memory are stored under
      `target/benchmarks/phase-c/` for the minimal paired fixture.
- [x] Reusable compressed/decoded CPU residency caches are bounded beyond
      single-load `SpzLoadLimits` (`SourceResidencyCaches` unit evidence).

### Phase D: Streaming LOD

- [x] D0 offscreen minimal and deterministic multi-page degree-3 count/image
      parity pass under the frozen thresholds.
- [x] D0 fixed four-slot residency, eviction, and non-resident draw exclusion pass.
- [x] D0 small-motion retained-cover trace has no zero-draw or transparent holes.
- [x] D0 cancel/stale/generation results are rejected before active draw mutation.
- [x] D0 local-source Surface path reports stable non-zero drawn output on Android.
- [x] D1 deterministic slot/resident/active bounds hold across a 512-frame trace.
- [x] D1 physical Android Kitsune paged run remains healthy for two minutes.
- [x] D1 optional network profile is omitted because local steady state is healthy.
- [x] Any 30-minute gate remains deferred to later D1 or Phase E.

### Phase E: Competitive Qualification

- [ ] Native paired runs pass on the declared Android/iPhone matrix.
- [x] Desktop Web parity passes for the only claimed browser/device/scenario:
      local headless Chrome/WebGPU on Kitsune static at 640×480.
- [x] Fixed-quality and fixed-frame-budget tables are generated from raw data.
- [x] Claim language is restricted to that achieved narrow scope; native,
      broad Web, memory, thermal/energy, and large-scene claims remain blocked.
- [x] Canonical verification and release documentation is updated.

### Phase F: Distribution and Claim Promotion

- [x] Local Web tarball installs in an isolated consumer and exposes the
      documented ESM entry points; WASM build, package tests, dry-run pack, and
      browser Surface smoke pass.
- [x] Android JNI smoke, arm64 AAR, and sample APK build pass; the AAR contains
      its manifest, classes, and `libgsplat_jni.so` arm64 payload.
- [x] Apple host smoke, multi-slice XCFramework, Swift package description, and
      generic iOS Simulator package consumer build pass.
- [x] Stable v0.1 and experimental boundaries plus earned/withheld claims are
      explicit in canonical docs. No registry publication was performed.

## Release Decision

The large-scene path remains experimental and opt-in until Phases B-D pass.
SmallSceneDirect remains the default rollback. A failed competitor gate blocks
the marketing claim, not necessarily the internal feature; a failed correctness,
memory-safety, lifecycle, or stability gate blocks release promotion.
