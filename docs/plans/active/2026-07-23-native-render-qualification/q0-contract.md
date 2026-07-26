# Q0 Fair Qualification Contract

> Status: **Active candidate**. This document freezes admission for Package Q;
> it does not contain a product benchmark or a winner. Root review and
> integration are still required before Q1--Q3 may collect formal evidence.

## 1. Purpose and non-goals

Q0 makes a native-versus-competitor result reproducible and narrowly true. It
extends the program [benchmark protocol](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md)
and the repository [full-quality contract](../../../../tests/perf/full-quality-experiment-v1.md);
it does not fork either schema.

The contract has four rules:

1. compare the same complete source, SH degree, camera work and presented
   pixels;
2. compare only terminal intervals with the same boundary, leaving unmatched
   phases unavailable;
3. separate Exact near-contract, Balanced product throughput and native-only
   CPU/SIMD/whole-plan evidence;
4. finish every planned cell as Accepted, Rejected, Deferred or, for a missing
   product dependency, NotApplicable. No FPS or lead percentage is required to
   finish Package Q.

Q0 does not change renderer code, public API, product defaults, comparator
code, assets or benchmark schemas. It does not claim that native APIs are
inherently faster than browsers, and it does not make PlayCanvas internals a
design requirement for gsplat-rs.

## 2. Frozen identities

### 2.1 gsplat-rs and local harness

| Item | Frozen value |
| --- | --- |
| Q0 baseline | `6d5bd5442dee31cea24906744dfdd1d7492095ae` |
| native implementation | one clean named commit descended from the Q0 baseline; exact commit and dirty flag in every run |
| comparator harness owner | [`tests/competitive/playcanvas`](../../../../tests/competitive/playcanvas/README.md) at the same clean gsplat-rs commit |
| artifact contract | `gsplat-benchmark/v1` plus `gsplat-full-quality-experiment/v1` receipts |
| render contract | `SortedAlpha`; complete source membership and source SH degree; no sampling, LOD, dynamic resolution or upscaling |

The implementation commit may advance after Q0, but a paired series uses one
immutable native commit and one immutable local harness commit. Mixed commits,
dirty builds or an unrecorded executable replacement are Rejected. A code fix
starts a new series; it never silently repairs an old pair.

### 2.2 PlayCanvas comparator

| Item | Frozen value |
| --- | --- |
| package | `playcanvas@2.21.0-beta.14` |
| upstream revision | [`d5fe88878e338936fe763bbce1a58bc315e89cbe`](https://github.com/playcanvas/engine/commit/d5fe88878e338936fe763bbce1a58bc315e89cbe) |
| runtime revision | `d5fe888` |
| npm integrity | `sha512-qYN8vp9CBRBU8qs9eJudUM3fFkPbXz8qN5IqjFicULJoD0CNnEtlFsGoHVAOQZ5b2+LWlUxUFhXMSzNL86moWg==` |
| requested device | `DEVICETYPE_WEBGPU` |
| requested renderer | `GSPLAT_RENDERER_RASTER_GPU_SORT` |
| required actual path | WebGPU + `raster_gpu_sort` + `usesGpuSort=true` |

Qualification installs only the committed lockfile and runs
`npm test --prefix tests/competitive/playcanvas` before collection. The
preflight must join package version, tarball integrity, license, installed
metadata and runtime revision. PlayCanvas can support both WebGPU and WebGL,
and its device creation may try a fallback, but this Q0 comparator track
accepts only a receipt proving that WebGPU and the GPU-sort renderer were
actually selected. A WebGL result is a separate future comparator contract,
not a fallback result for Q1 or Q2.

The pinned upstream hybrid renderer performs GPU cull/compaction, projection,
radix sort and indirect draw work for the current view. That is a recorded
implementation difference, not permission to remove equivalent work from the
native side. Sources: the pinned
[`GSplatHybridRenderer`](https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/gsplat-unified/gsplat-hybrid-renderer.js),
PlayCanvas' official [graphics backend overview](https://developer.playcanvas.com/user-manual/graphics/),
and its official [WebGPU GPU-sort/culling change](https://github.com/playcanvas/engine/pull/8453).

### 2.3 Launch paths

Q tasks may call only the checked-in launch paths documented by the harness:

- desktop/browser: `tests/competitive/playcanvas/scripts/run-timed-benchmark.mjs`
  with a named `*-quality-1080p-v1` preset and a fresh artifact directory;
- A065: the immersive hardware-accelerated System WebView host in
  [`android-harness/README.md`](../../../../tests/competitive/playcanvas/android-harness/README.md),
  attached through its single remote-CDP page;
- native and gsplat-rs Web/WASM: the existing collectors named by
  [`handbook/VERIFICATION.md`](../../../../handbook/VERIFICATION.md), after their
  doctor/command prerequisites are READY.

A normal Android Chrome tab with browser chrome, a second CDP page, headless
rendering substituted for physical presentation, a sampled WebGL preview, a
simulator substituted for a physical-device timing result, or an ad-hoc launch
command is not admitted. Missing prerequisites yield Deferred; they do not
authorize a different route.

## 3. Common workload admission

Every pair is rejected before timing unless all common fields match.

### 3.1 Source and quality identity

- same manifest ID, PLY bytes and SHA-256;
- `source = decoded = encoded = resident = addressable` splat count;
- source and resident SH degrees equal the manifest SH degree;
- source membership `all` and `partial_scene_published=false`;
- sampling, point budget, LOD, hierarchy, dynamic resolution and upscaling
  disabled;
- same alpha cutoff, blend mode and declared covariance/raster precision where
  the comparator exposes them;
- no capacity failure is converted into a smaller dataset, lower SH degree or
  partial image.

The main paired anchor is complete Truck: 2,541,226 splats at source SH3. The
real-scene breadth schedule retains Flowers/Kitsune, Bonsai, Garden and Bicycle
when their committed manifests and exact assets are available. Deterministic
50K/200K/500K/1M tiers are scaling evidence only; they never replace a full
scene in a quality claim.

PlayCanvas' active source/resident count does not prove its post-projection
`V/C/D` counts. Unexposed `V`, `C` or `D` remains `null` with a reason. Q1 may
still be a near-contract comparison when source membership is proven, but it
may not claim identical draw work.

### 3.2 Camera and frame schedule

- same checked-in `gsplat-camera-trace/v1` bytes and SHA-256;
- same pose, vertical FOV, near/far, aspect, view/projection matrices and frame
  indices after the unit-tested coordinate conversion;
- `fixed_frame` for image/cache/raster observations;
- `trace_sequence`, `sort_interval=1`, for moving-camera throughput and sort
  work;
- same predeclared warmup/measured counts and camera mutation point;
- measurement restarts at trace frame zero after warmup;
- no auto camera, endpoint-specific orbit or global-AABB framing.

Every captured image is bound to its trace frame, live camera receipt,
successful terminal render and presented-frame receipt. A trace JSON reused as
if it were runtime camera evidence is Rejected.

### 3.3 Backing and presentation

Formal dimensions are:

| Endpoint | Required pixels | Evidence |
| --- | ---: | --- |
| macOS native Metal / Chrome WebGPU | 1920x1080 | requested, Surface/canvas, internal and presented receipt |
| Nothing A065 native Vulkan / WebView WebGPU | 2412x1080 | the same four receipts plus WindowManager, zero-inset and ADB screen binding |
| physical Apple mobile, when available | actual probed drawable | a regenerated same-family trace and physical presentation receipt |

For every accepted frame,
`requested = Surface/canvas backing = internal render = presented`. CSS size,
device-pixel ratio and physical panel dimensions are recorded separately and
cannot substitute for backing pixels. A failed acquire/present, cropped tab,
overlay, nonzero inset, rescale or upscaled image is Rejected.

## 4. Timing semantics

### 4.1 The only cross-implementation performance interval

The common comparator metric is one terminal measurement window:

```text
warmup queue drained and frame loop controlled
-> first measured camera input accepted
-> N predeclared frames ordered, projected, rasterized and submitted
-> all work submitted for measured frame N completed
```

Both sides report the terminal-window duration, `N`, and sustained throughput
derived from those two values. Queue backlog before the first frame and untimed
post-measurement screenshot work are outside the window. A missing queue drain,
extra submission during the drain, dropped frame or mismatched submission
identity rejects the run.

The paired product track uses presented Surface loops with the same declared
cadence policy. It never compares a browser rAF-limited run with an unthrottled
native offscreen loop. A separate raw-throughput experiment is admissible only
after both sides expose the same unthrottled, terminal-drained boundary; until
then that metric is Deferred.

WebGPU `GPUQueue.onSubmittedWorkDone()` establishes completion for work
submitted before the call; it is not a per-stage GPU timer. The harness must
also prove stable submission identity while draining. The normative source is
the [WebGPU queue completion contract](https://www.w3.org/TR/webgpu/#dom-gpuqueue-onsubmittedworkdone).

### 4.2 Metrics that remain separate

| Metric | Native | PlayCanvas/browser | Cross-implementation use |
| --- | --- | --- | --- |
| terminal window | host monotonic start to queue terminal | first measured submission window through final queue drain | comparable when all admission fields match |
| presentation cadence | actual presented-frame timestamps | rAF/frame-wall plus external presentation receipt | report separately; compare only like-for-like cadence |
| per-frame whole-plan completion | ticketed `FrameCompletion` | unavailable unless the comparator exposes an equivalent isolated terminal | native plan/Adaptive decisions only |
| CPU call/encode | native CPU preprocess/sort/encode | JS `frameupdate` to `frameend` | diagnostic; never compared as GPU completion |
| GPU phases | timestamp query when supported | unavailable in the pinned harness | diagnostic and nullable |

Unavailable values are `null` and listed in `unavailable_fields`. CPU encode
duration is never called GPU sort time; `1000 / call_ms` is never called
presented FPS. Q1/Q2 compare distributions only for metrics whose start/end
semantics match, and otherwise compare the common terminal window plus images.

## 5. Environment, pairing and thermal control

Every run records device model/serial pseudonym, SoC/GPU/adapter, backend,
driver, OS build, browser or WebView package/version, power source, battery
level, refresh rate, orientation, drawable, memory limits and available
thermal status. CPU architecture and actual feature detection are mandatory
for SIMD claims.

Formal comparison uses five predeclared randomized AB/BA pairs, or an
equivalent predeclared ABBA schedule, on the same physical device and as close
to one session as practical. Each pair keeps dataset, trace, resolution,
profile, binary and warmup/measured counts fixed. The schedule, seed, pair ID,
run order and every exclusion are retained; best-of selection is forbidden.

On thermally observable devices, a run starts only at the predeclared neutral
state and records pre/post status and temperature. A thermally inadmissible
attempt is Rejected for performance evidence. The collector does not silently
retry it: a later attempt requires a new output directory and an explicit
schedule entry. Where the platform exposes no trustworthy thermal field, it is
Deferred for thermal qualification and the claim is narrowed; a synthetic zero
is forbidden.

## 6. Three independent evidence lanes

### 6.1 Q1 — Exact near-contract

Question: with complete membership, source SH, identical camera and pixels,
how does native Exact compare with the closest publicly configurable pinned
PlayCanvas WebGPU path?

Native remains under the Exact image/count/order contract and its Direct-f32
oracle. PlayCanvas is evaluated against the same reference images, but reduced
precision, stable-tie behavior, SH update policy and unavailable `V/C/D` fields
remain explicit. Therefore Q1 may report:

- image metrics and defects against the common reference;
- terminal-window throughput under the matched external workload;
- implementation differences that explain the result;
- **near-contract**, never universal same-contract or same-work leadership,
  when internal precision/order/count semantics cannot be aligned.

The pinned PlayCanvas hybrid path prepares sort/project work on every forward
frame, including a fixed camera, while native Exact may reuse a fully guarded
stationary result. That difference is retained as product-plan behavior. A
fixed-frame result may describe cache/raster cost, but only the moving sequence
is eligible for an ordering-throughput comparison.

An image miss does not trigger open-ended tuning. The run remains a valid
quality observation but is Rejected from a same-quality performance claim. Q1
closes with a scoped Accepted report, or a finite Rejected/Deferred matrix.

### 6.2 Q2 — Balanced product throughput

Q2 starts only after B6 is Accepted with a machine-validated Balanced quality
contract. It compares the accepted gsplat-rs Balanced product plan with the
frozen PlayCanvas WebGPU GPU-sort configuration under complete membership,
source SH, matched camera and full resolution. Precision/layout differences
are part of the product contract, while LOD, sampling and resolution scaling
remain disabled.

Q2 answers product throughput, not Exact parity. If B6 is Rejected, Q2 is
NotApplicable and terminal. If B6 or a required endpoint is Deferred, Q2 is
Deferred. Q0 never holds Package B open to manufacture a comparator result.

### 6.3 Q3 — native CPU/SIMD/whole-plan advantage

Q3 is a native-only controlled matrix; PlayCanvas numbers do not select a CPU
kernel or a native plan.

| Endpoint | CPU kernel cells | complete-plan cells | Evidence boundary |
| --- | --- | --- | --- |
| Apple M4 | Scalar and NEON when exact | `CpuPostSort`, `GpuPostSort`, `GpuPreproject`, Adaptive when eligible | Metal on this machine only |
| Nothing A065 | Scalar and NEON when exact | same eligible plans | Adreno/Vulkan on this device only |
| physical x86_64 | Scalar and AVX2/FMA when detected | eligible CPU/GPU plans | Deferred until real hardware exists |
| physical iPhone/iPad | Scalar/NEON and eligible Metal plans | device-only | simulator is correctness evidence only |

Scalar is the element-for-element oracle. A SIMD cell must preserve keys,
source IDs, boundary/NaN/FMA semantics and stable ties before timing. Kernel
microbenchmarks diagnose compute work; promotion uses interleaved native
whole-plan terminal measurements on the same endpoint. Unsupported hardware,
an inconclusive result or an end-to-end loss closes that cell as Deferred or
Rejected and retains Scalar. Cross-compilation, Rosetta and simulator timing
do not qualify a target feature.

Adaptive is evaluated only among already correct complete plans. It must prove
bounded learning, hysteresis, cooldown, exact fallback, generation-safe
receipts and periodic re-probe. No measured point-count crossover becomes a
hard-coded product threshold.

## 7. Endpoint schedule

| Lane | Endpoint pair or native endpoint | Required workload | Terminal result |
| --- | --- | --- | --- |
| Q1 | M4 native Metal vs Chrome WebGPU on the same M4 | Truck fixed views + moving 1920x1080 sequence | Accepted, Rejected or Deferred |
| Q1 | A065 native Vulkan vs immersive WebView WebGPU on the same A065 | Truck fixed views + moving 2412x1080 sequence | Accepted, Rejected or Deferred |
| Q1 isolation | gsplat-rs WASM/WebGPU vs PlayCanvas WebGPU in the same Chrome, if both formal collectors are ready | Truck 1920x1080 | Accepted or Deferred; never substituted by smoke |
| Q1 breadth | available real-scene anchors on admitted M4/A065 pairs | fixed quality views; moving runs only when predeclared | per-cell Accepted/Rejected/Deferred |
| Q2 | admitted M4/A065 pairs after B6 | B6-declared scenes and sequences | Accepted/Rejected/Deferred or package NotApplicable |
| Q3 | M4 and A065 native | point ladder plus complete Truck | per-kernel/per-plan terminal decision |
| Q3 breadth | physical x86_64 and physical Apple mobile | same eligible native matrix | Deferred when hardware is absent |

Windows/Linux discrete GPUs, other browsers and a second Android class are
promotion breadth. Their absence narrows the report; it does not block Q4.

## 8. Retained evidence

Every accepted run keeps immutable raw evidence below a fresh output root:

- `manifest.json`, `frames.jsonl`, `summary.json` and `runtime.log`;
- native commit/dirty state and executable/APK/native-library/WASM hashes;
- PlayCanvas package, lock integrity, full upstream revision, browser/WebView
  and local harness commit receipts;
- dataset manifest/path identity, bytes, SHA-256, complete count and SH receipt;
- trace bytes/hash, camera mode/frame indices and live camera receipts;
- requested/Surface-or-canvas/internal/presented resolution receipts;
- adapter/backend/limits plus device, OS, power and thermal receipts;
- every issued submission ticket and exactly one joined terminal success or
  failure; no pending terminal is converted to zero;
- raw timing samples, timing source, common terminal-window receipt and all
  nullable diagnostic fields;
- final native/canvas PNG, external physical screen PNG where required,
  screenshot binding, common-reference diff/SSIM and image dimensions;
- pairing schedule, exclusions and a validator result.

A blocker or rejection retains the available identity, failure stage and raw
log, but is never published as a successful artifact. Generated output stays
outside Git.

## 9. Admission state machine

```text
Planned
  -> prerequisites and immutable identities admitted
  -> common workload receipts matched
  -> one predeclared run and terminal drain
  -> artifacts validated
  -> Accepted

Any contract mismatch on an available endpoint -> Rejected
Missing device/asset/tool/capability/permission -> Deferred
B6 terminal Rejected -> Q2 NotApplicable
```

- **Accepted** means the retained artifact satisfies this lane's declared
  contract on the named endpoint. It is not a backend-wide claim.
- **Rejected** means an available attempt violated identity, workload,
  presentation, timing, thermal or artifact admission. Its performance values
  do not enter aggregate comparisons.
- **Deferred** names the exact unavailable prerequisite and next official
  probe. Compile success, READY doctor output, simulator output or historical
  artifacts cannot replace it.
- **NotApplicable** is limited to Q2 when no Accepted Balanced product exists.

Each planned run is a one-shot attempt. A proven infrastructure repair may
create a newly named attempt with a fresh destination, but Q0 contains no
automatic retry loop and no target percentage. Q4 may close with slower,
inconclusive, Rejected, Deferred or NotApplicable cells as long as every cell
has a truthful terminal state.

## 10. Q0 acceptance

Q0 itself is ready for root review when:

- comparator revision, integrity and existing launch paths are immutable;
- common source, SH, camera, resolution, presentation and terminal timing
  admission is explicit;
- Q1, Q2 and Q3 cannot consume one another's claims;
- endpoint absence and incomparable evidence have finite terminal states;
- the contract contains no renderer change, product benchmark result, fixed
  FPS target or required lead percentage;
- links, Markdown formatting, active-state ledger and two-file allowlist pass.

Q0 acceptance authorizes collection design only. Product benchmark execution
belongs to Q1--Q3 after their own prerequisites are admitted.
