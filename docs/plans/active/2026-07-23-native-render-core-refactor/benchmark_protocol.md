# Native Render Core Verification and Benchmark Protocol

> This protocol applies to the tasks in [task_plan.md](task_plan.md). It extends
> the repository-wide commands in `handbook/VERIFICATION.md`; it does not replace
> them.

## 1. Purpose

The protocol has three jobs:

1. fail closed on quality, count, resolution and artifact identity;
2. make CPU/GPU/plan decisions from comparable terminal measurements;
3. prevent an unfair PlayCanvas comparison from driving architecture.

Correctness gates can reject an implementation. Performance measurements decide
promotion scope or Accept/Reject for a performance hypothesis, but they do not
create an endless optimization loop.

The machine authority for existing Exact evidence remains the checked-in schema
and validators named in section 18. Numeric gates summarized here are routing
guidance, not a second independently editable schema. If this document and a
validator disagree, the task stops and reconciles them before collecting new
evidence; it does not choose the more convenient value after seeing a result.

## 2. Evidence classes

Every retained result declares one evidence class.

| Class | Meaning | Allowed claim |
| --- | --- | --- |
| unit | pure CPU/GPU algorithm contract | correctness of the isolated algorithm |
| conformance | image/count/order result on a small deterministic scene | renderer semantics on the tested backend |
| directional | one or a few controlled runs | where to investigate next |
| qualification | repeated, identified, validated runs | enable/promotion decision for the tested endpoint/scope |
| capacity | complete construction/draw proof | scene fits; no FPS or sustained claim |
| stability | sustained run with thermal/memory receipts | sustained behavior for the tested endpoint |

An iOS simulator run is functional/conformance evidence, not iPhone performance.
A four-frame Garden/Bicycle run is capacity evidence, not interactive cadence.
A 640x360 run is diagnostic evidence, not product-readiness evidence.

## 3. Profile tracks

### 3.1 Exact track

Exact qualification requires:

- source, decoded, encoded, resident and addressable counts equal;
- source and resident SH degree equal;
- sampling/LOD/dynamic resolution/upscaling disabled;
- stable full32 order with deterministic ties;
- requested, Surface, internal-render and presented dimensions equal;
- `SortedAlpha` and the pinned blend/covariance/alpha-cutoff contract;
- Direct f32 reference image for the same source, camera and resolution.

Image gates:

| Metric | Gate |
| --- | ---: |
| SSIM | `>= 0.9999` |
| normalized RGB MAE | `<= 0.00005` |
| pixels with RGB error `> 3/255` | `<= 0.001` fraction |
| alpha | exact |

These are inherited from the completed full-quality work. A responsibility-only
refactor cannot renegotiate them.

### 3.2 Balanced track

Balanced still requires complete source membership, source SH degree and full
resolution. It permits explicitly declared precision trade-offs.

Initial qualification gate:

| Metric | Gate |
| --- | ---: |
| SSIM | `>= 0.99` |
| normalized RGB MAE | reported and bounded by the task declaration |
| temporal outlier frames | none beyond the declared moving-sequence gate |
| source/resident membership | complete |
| SH degree | unchanged |
| resolution | full, no upscale |

Before B0 closes, it must replace the placeholder RGB/temporal wording with a
machine-validated schema and representative moving-camera threshold. Each
trade-off is tested independently before a combined plan is qualified.

### 3.3 Scalable track

Scalable never reports itself as full-source Exact/Balanced. Its hard gates are:

- valid root/bootstrap coverage;
- every active region represented by a parent or a complete ready child set;
- atomic parent-to-child replacement;
- no holes caused by missing/failed pages;
- compressed, decoded and GPU caches stay within declared byte budgets;
- active set is globally ordered;
- quality/error, page residency, network/decode/upload latency and memory are
  present in the receipt;
- reference images and quality-memory curves use the same camera and resolution.

Scalable can trade quality for bounded working set only because its profile is
explicitly different.

## 4. Dataset ladder

The matrix separates deterministic point-count pressure from real-scene image
evidence.

### 4.1 Point-count ladder

| Tier | Count | Use |
| --- | ---: | --- |
| L0 | 50,000 | fixed overhead and small-scene CPU advantage |
| L1 | 200,000 | original mobile CPU-sort decision region |
| L2 | 500,000 | mid-size crossover and memory traffic |
| L3 | 1,000,000 | million-point resident path |
| L4 | 2,541,226 | complete Truck |
| L5 | 5,834,784 | complete Garden |
| L6 | 6,131,954 | complete Bicycle |

Deterministic prefixes or generated ladders may locate a performance crossover,
but they cannot replace real scenes in a quality claim.

### 4.2 Real-scene diversity

At minimum, qualification uses:

- a small object/fixture for byte- and image-exact conformance;
- Flowers or Kitsune for small/mid mobile continuity;
- Bonsai for dense indoor geometry;
- Truck for the main 2.54M matched comparator;
- Garden and Bicycle for large outdoor capacity and, when practical, sustained
  performance.

Every external asset uses the existing manifest, source URL/license metadata and
content hash. Large assets are not committed to Git.

## 5. Camera traces

Every performance artifact names one trace and mode.

| Trace | Purpose |
| --- | --- |
| static | cache reuse, idle overhead and raster-only behavior |
| smooth sequence | ordinary moving-camera sort/project/raster cost |
| fast turn | stale-work handling, p95/p99 and plan-probe behavior |
| authored reference views | image comparison and screenshot binding |

Rules:

- positions, orientations, vertical FOV, near/far and backing dimensions are
  stored in the canonical trace;
- platform coordinate conversion is unit-tested;
- the same frame index/sequence is applied to both renderers;
- an auto camera or scene AABB is not used for a competitor result;
- warmup and measured frames are predeclared;
- camera mutation happens at the same logical point in the measured frame;
- a static result may not be generalized to moving-camera sorting.

## 6. Resolution contract

Qualification resolutions:

- desktop and browser standard: 1920x1080;
- Android/iOS product: actual drawable, with an optional matched 1080p run;
- current A065 comparison: 2412x1080 native drawable;
- optional desktop pressure: 3840x2160;
- 640x360 and 640x480: diagnostic only.

Every artifact records:

```json
{
  "resolution": {
    "requested_width": 1920,
    "requested_height": 1080,
    "surface_width": 1920,
    "surface_height": 1080,
    "internal_render_width": 1920,
    "internal_render_height": 1080,
    "presented_width": 1920,
    "presented_height": 1080,
    "dynamic_resolution": "disabled",
    "upscaling": "disabled",
    "full_resolution": true
  }
}
```

CSS size, physical panel size and internal backing size are distinct. A frame
with no successful Surface acquisition/presentation is not a presented sample.

## 7. Endpoint tiers

### Tier 0 — required for every relevant code task

- workspace format/check/test;
- Clippy and Rustdoc warnings as errors where canonical;
- affected pure/module/contract tests;
- WASM compile if shared renderer code is touched;
- FFI smoke if public C-facing code is touched.

### Tier 1 — available core qualification endpoints

- Apple M4 native Metal;
- Chrome browser WebGPU on the same Mac;
- Nothing A065 / Adreno 730 / Vulkan at native drawable.

Tasks use the endpoint relevant to the hypothesis; simple file extraction does
not rerun the entire device matrix.

### Tier 2 — promotion breadth

- physical iPhone/iPad Metal when available;
- a second Android class when available;
- x86_64 with AVX2/FMA for CPU kernel qualification;
- discrete Vulkan/DX12 where available;
- target browsers needed for Web package promotion.

Tier 2 absence leads to Defer or narrower claims, not endless task execution.

## 8. Timing contract

### 8.1 Product decision interval

CPU/GPU/plan Adaptive uses one terminal interval:

```text
frame input accepted
-> ordering/project/compact/raster encoded
-> queue submitted
-> all work for that sampled frame completed
```

The same interval definition is used for all plans on one endpoint. GPU
timestamp phases may diagnose a bottleneck but cannot drive selection when
another plan lacks the same phases.

### 8.2 Native

- record host monotonic start and queue-terminal completion;
- tag samples with scene/camera/viewport/contract/plan generations;
- no synchronous readback in ordinary frames;
- optional timestamp-query phases are asynchronous and report unsupported when
  unavailable;
- a ring-busy or Surface-unavailable frame is marked unsampled, never zero ms.

### 8.3 Browser/PlayCanvas

- require WebGPU for the current competitor track; WebGL is a separate result;
- stop/drain the existing frame loop according to the pinned harness;
- use `GPUQueue.onSubmittedWorkDone()` with stable submission identity;
- record browser frame-wall and queue-terminal cadence separately;
- do not compare JavaScript function-call time with native queue completion;
- retain browser/runtime revision and user agent.

## 9. Count semantics

The shared vocabulary is:

- `S`: source/resident/addressable splats;
- `V`: exact visible prefix after profile-defined clip/visibility;
- `C`: exact fragments/contributors that can affect at least one pixel under the
  raster cutoff;
- `D`: draw instances/work items.

Expected contracts:

| Plan | Count contract |
| --- | --- |
| Candidate/PostSort | `D = V`, while `C` may be measured diagnostically |
| exact Compact/Preproject | `D = C <= V` |
| Scalable | `S_active`, represented hierarchy nodes and coverage are explicit; no full-source equality claim |

Unavailable competitor counts remain `null` with a reason. They are not
inferred from active source count.

## 10. Artifact identity

Every retained qualification artifact includes:

- schema version and evidence class;
- repository commit and dirty flag;
- application/APK/native-library/WASM bundle hash as relevant;
- dataset ID, source count, SH degree and SHA-256;
- trace ID/hash, mode and frame indices;
- adapter name, backend, limits, OS/browser/driver and device identity;
- requested/actual profile and complete PlanId;
- actual CPU preprocess kernel and order backend;
- resident/projected precision contract;
- source/resident/addressable and V/C/D receipts;
- full resolution receipt;
- timing interval source and raw frames;
- thermal/battery/memory fields where available;
- warmup/measured frame counts;
- unavailable fields with explicit reasons;
- screenshot/raw-image hash bound to the same camera/frame;
- terminal success/failure for every issued ticket.

Formal claims require a clean named commit and reproducible binary hash. Dirty
runs may be directional only and must retain the diff identity if possible.

## 11. Pairing and repetition

### Development decision

- at least three interleaved directional runs when variance matters;
- randomize or alternate order;
- keep device, resolution, trace, source, SH, profile and binary fixed;
- reject runs with thermal throttling, missing frames or receipt mismatch.

### Formal comparative claim

- five predeclared randomized AB/BA pairs (or an equivalent predeclared ABBA
  schedule);
- same physical device and as-close-as-practical session;
- matching source/camera/resolution/profile receipts;
- all raw frames and screenshots retained;
- report median pair ratio plus p50/p95/p99 distributions;
- report all exclusions and reasons;
- do not select the best run from a larger unreported set.

## 12. PlayCanvas comparison tracks

### 12.1 Product-throughput track

Question: “What experience does each project deliver with its intended resident
product settings?”

- gsplat-rs Balanced versus pinned PlayCanvas default WebGPU path;
- complete same source model and SH degree;
- same camera and backing resolution;
- LOD/sampling/dynamic resolution/upscaling disabled;
- precision/layout differences recorded;
- result is labelled product-throughput, not equal-contract quality.

### 12.2 Same-contract track

Question: “How do the renderers compare when quality contracts can be aligned?”

- gsplat-rs Exact;
- comparator configured as close as its public implementation allows;
- same source/camera/resolution;
- output compared to the same reference image;
- depth precision, projected precision, SH update and count observability listed;
- if PlayCanvas cannot supply full32/stable-tie or exact count semantics, the
  result explicitly remains near-contract and no strict percentage superiority
  claim is made.

### 12.3 Scalable track

Question: “At a fixed memory/latency budget, what quality and stability are
delivered?”

- compare quality-memory-latency curves;
- report active/represented work and hierarchy error;
- do not compare the FPS of an LOD active set to an Exact full-source run as if
  they were equivalent.

## 13. CPU SIMD qualification

Scalar is the oracle. Each native SIMD kernel covers:

- empty, one-element and non-lane-multiple lengths;
- inclusive near/far boundary values;
- NaN/inf handling as defined by the scalar path;
- FMA-sensitive inputs;
- equal-depth source-ID stability;
- deterministic chunk merge order;
- 50K through complete Truck pressure vectors;
- repeated calls with the same reusable workspace;
- unsupported-feature scalar fallback.

Required result: keys and IDs match element-for-element. A faster microbenchmark
does not enable the kernel unless complete frame-terminal measurements are
non-regressing on the target endpoint. If AVX gather or a fused packed pass loses
end-to-end, the task ends Reject without weakening the architecture.

Kernel qualification exposes private benchmark-only forced
Scalar/NEON/AVX2 controls; these do not enter the stable API. Every frame receipt
records actual kernel, chunk count and fallback reason. Default enablement uses
at least three interleaved end-to-end runs and their median terminal interval. A
change slower by more than the predeclared 5% noise band is disabled/rejected;
within the band the simpler/fewer-thread choice wins. This is a local experiment
decision rule, not a global FPS gate.

Existing AVX2 pack/histogram support must not be described as a complete AVX2
depth preprocess until E5 proves it. The historical `GpuOddEvenSortBackend`
remains a <=4096-item conformance tool and is never an Adaptive product
candidate.

## 14. GPU primitive qualification

Every scan/radix/compact primitive covers:

- 0/1 and workgroup-boundary lengths;
- arbitrary non-power-of-two lengths;
- repeated/all-equal/0/MAX keys;
- stable source-ID ties;
- untouched poison tails beyond logical length;
- device dispatch and binding limits;
- no implicit readback or second device;
- deterministic repeated output;
- end-to-end image/count oracle on the target backend.

A backend-specific faster kernel is admitted only behind the same primitive
contract and only for backends on which it is qualified.

## 15. Adaptive qualification

For each eligible plan set:

- force every plan first and validate quality/count parity;
- run static and moving traces;
- prove learning exits in bounded time;
- prove hysteresis/minimum residency prevents oscillation;
- prove a failed challenger falls back within the same profile;
- prove probe generation cannot join the wrong camera/scene/contract;
- report exploration overhead;
- prove periodic re-probe can discover a changed winner;
- keep performance samples separate from diagnostic evidence-ring pressure.

No fixed point-count crossover is encoded from the observed matrix.

## 16. Task completion versus profile promotion

A task can complete with limited evidence:

- a refactor task: Tier 0 + affected image/state smoke;
- a CPU NEON task: Tier 0 + A065/M4 scalar parity and relevant end-to-end run;
- a GPU plan task: Tier 0 + one native and one portability backend where
  available;
- an Android task: Tier 0 + A065;
- an Apple task: Tier 0 + simulator functional evidence, with physical-device
  promotion evidence separately required;
- a rejected experiment: correctness evidence plus controlled performance loss.

Profile promotion is broader:

- Exact default: full mandatory quality matrix;
- Balanced opt-in: defined image contract plus Tier 1 endpoints;
- Balanced default on an endpoint: repeated qualification and reliable Exact
  fallback;
- Scalable product: coverage, budget and quality curves on real oversized
  scenes.

This separation prevents an unavailable endpoint or a missed performance target
from keeping one Codex task alive indefinitely.

## 17. Reporting language

Allowed:

- “On A065/Adreno 730, plan A was faster in five matched pairs.”
- “The current PlayCanvas WebGPU product run has lower queue-terminal cadence,
  under a different precision/work contract.”
- “Balanced reduced depth precision passed the declared image gate on these
  endpoints.”
- “Garden loaded and drew fully; this is capacity evidence.”

Not allowed:

- “Native is faster/slower than Web” from one unmatched run.
- “Same quality” without matching membership, SH, resolution and image receipt.
- “60 FPS” from `1000 / render_call_ms` when display cadence was not measured.
- “Streaming” for a path that retains full `SceneBuffers`.
- “GPU sort time” from CPU encode duration.
- “iOS performance” from a simulator.

## 18. Existing protocol inputs

This plan reuses rather than forks:

- `handbook/VERIFICATION.md`;
- `tests/perf/benchmark-artifact-v1.md`;
- `tests/perf/full-quality-experiment-v1.md`;
- `tests/perf/full-quality-matrix-plan-v1.json`;
- `tests/competitive/playcanvas/README.md`;
- the completed full-quality
  [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md);
- the completed competitor
  [findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md).

A0 records which current validators remain canonical. A later task updates the
schemas only before claiming a new evidence field, never after seeing a desired
result.
