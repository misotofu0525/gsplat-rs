# Phase 2: exact pre-project contributor architecture

> Status: architecture decision implemented in `006e37e` and validated in the
> clean `7cabb6e` same-binary Metal, WebGPU and Adreno cohorts. The measured
> retain/default decision is in `phase2-production-producer-ab-checkpoint.md`.
> This document preserves the design rationale and falsifiable gates.

## Decision

The next production candidate is:

```text
complete Resident scene S
  -> exact f32 projection and conservative contributor classification
  -> deterministic source-order compaction to C {full32 depth key, source ID}
  -> stable full32 radix over C only
  -> source-indexed f32 projected cache
  -> hardware SortedAlpha indirect draw with D = C
```

This is a GPU-path change first. The current CPU, GPU, and Adaptive product
choices remain. The first production slice keeps the qualified CPU path intact
while the new GPU path is measured against the current GPU path in the same
binary. A later, independent Android-focused experiment may move exact
projection before CPU radix too; it is not assumed to win.

Hardware raster remains the release candidate. Exact tiled compute remains a
diagnostic and a conditional research branch, not the default architecture.
No phase in this plan may gain speed through incomplete residency, sampling,
LOD, reduced SH, reduced key width, fp16 projected geometry, dynamic
resolution, upscaling, top-K work, dropped tile entries, or early blend exit.

## Terms and non-negotiable invariants

The count stages are:

- `S`: complete source/resident/addressable scene count;
- `V`: optional cheap camera-depth candidate count before full projection;
- `C`: exact projected contributor count after the conservative full-quad
  predicate;
- `D`: issued draw instance count.

The production receipt remains `D = C <= V <= S` when the optional `V` stage
is present, or `D = C <= S` for direct `S -> C`. `S` never means a sampled or
paged subset.

The following are correctness gates, not tuning preferences:

1. every source Gaussian remains resident and addressable, with its original
   SH0-SH3 degree;
2. projection uses the established f32 covariance and canonical camera-depth
   operation tree;
3. contributor rejection is no more aggressive than the existing outward-
   rounded full-quad clip predicate and the `1/255` alpha boundary;
4. sorting consumes every bit of the canonical 32-bit depth key and is stable;
5. equal keys retain ascending source-ID order on CPU and GPU;
6. the hardware draw issues exactly `C` instances and retains the existing
   fragment/blend contract;
7. formal evidence uses the requested, internal, surface, and presented
   resolutions without dynamic resolution or upscaling;
8. CPU, GPU, and Adaptive remain explicit runtime choices, and Adaptive uses
   completed-frame evidence rather than a fixed point-count rule.

Count exactness and image similarity remain independent requirements. Passing
one never waives the other.

## What the current implementation actually does

The retained exact contributor implementation is post-sort:

```text
CPU: S -> CPU depth predicate -> stable sort V -> GPU project V -> compact C -> draw C
GPU: S -> GPU depth predicate -> stable radix V -> GPU project V -> compact C -> draw C
```

The projection shader writes two rank-indexed 16-byte f32 planes. A group
count, hierarchical scan, and stable local compaction then write a 4-byte
`contributor_ranks` indirection and indirect draw count. The relevant local
sources are:

- [`projected_quads_gpu.rs`](../../../../crates/gsplat-render-wgpu/src/projected_quads_gpu.rs),
  especially `encode_projection_and_compaction` and
  `create_contributor_compaction`;
- [`projected_quads_project.wgsl`](../../../../crates/gsplat-render-wgpu/shaders/projected_quads_project.wgsl),
  which owns the established projection and conservative rejection math;
- [`projected_quads_compact.wgsl`](../../../../crates/gsplat-render-wgpu/shaders/projected_quads_compact.wgsl),
  which preserves candidate-rank order while producing contributor ranks;
- [`direct_gpu_order.rs`](../../../../crates/gsplat-render-wgpu/src/direct_gpu_order.rs)
  and [the existing ordering design](design.md#complete-cpu-and-gpu-ordering),
  which own full-width stable radix and canonical depth.

This path removes guaranteed-empty raster work and is image exact in the
recorded cohort, but it still sorts `V`. Its compaction result therefore cannot
by itself reduce radix traffic.

At source-capacity `S`, its approximate dynamic payload is:

```text
existing sort scratch       about 16S + scan metadata
rank-indexed projected cache      32S
contributor-rank indirection        4S
total                         about 52S + metadata
```

This is a capacity formula, not process RSS or driver-private memory.

## Observed cross-platform evidence

### Exact counts

Complete Truck at the two formal cameras produces:

| Platform viewport | `S` | `V` | `C = D` | `C / V` |
| --- | ---: | ---: | ---: | ---: |
| Mac/Web 1920x1080, view 0 | 2,541,226 | 1,886,298 | 996,848 | 52.85% |
| Mac/Web 1920x1080, view 1 | 2,541,226 | 1,672,013 | 962,219 | 57.55% |
| Android 2412x1080, two-view mean | 2,541,226 | 1,779,155 | 1,041,477 | 58.54% |

Thus the current sort processes roughly 1.7-1.9 million entries before the
draw consumes roughly 1.0 million. This proves that substantial non-contributor
work reaches sorting and/or projection. It does **not** prove that removing the
same percentage of radix entries yields the same percentage of frame-time
improvement.

### Post-sort compaction results

| Platform | Current exact result | Comparison signal | Evidence boundary |
| --- | --- | --- | --- |
| Chrome/WebGPU, Truck 1920x1080 | CPU 34.404, GPU 33.278, Adaptive 37.213 median terminal FPS; all nine PNGs byte-identical | GPU is +8.79% versus the earlier single run; CPU is flat and Adaptive +0.77% | The new cohort is balanced and complete, but each older value is not a same-binary paired cohort, so the percentage is not a formal causal estimate. |
| Apple M4/Metal, Truck 1920x1080 | CPU 35.833, GPU 34.307, Adaptive 37.407 median FPS | All are lower than the earlier 43.321/38.434/42.390 baselines | Host load varied and the baseline is a different cohort/binary. The regression is a real current-product observation, not isolated causal attribution to compaction. |
| Nothing A065/Adreno, Truck 2412x1080 | CPU 153.646 ms, GPU 200.042 ms, Adaptive 162.247 ms mean frame wall | CPU/Adaptive are materially lower than the earlier three-run medians; GPU is nearly flat | Only one non-randomized new run per backend exists. This is a strong directional signal, not a balanced causal experiment. |

Evidence paths:

- `target/full-quality-final/contributor-compaction-web-truck-1920x1080-20260723/EXPERIMENT_REPORT.md`
  and `aggregate-report.json`;
- `target/full-quality-final/contributor-compaction-mac-truck-1920x1080-20260723/13-analysis.json`;
- `target/full-quality-final/contributor-compaction-android-truck-2412x1080-20260723/experiment.json`;
- pre-change Android cohort:
  `target/full-quality-final/post-parallel-android-truck-2412x1080-cpu-gpu-adaptive-r3-20260723/`.

The important platform conclusion is limited but useful: draw-work reduction,
radix cost, CPU/GPU contention, and scan overhead trade differently on Metal,
WebGPU, and Adreno. A universal “GPU above N points” rule is not supported.

## Proven facts, causal inferences, and open hypotheses

### Proven by code or current artifacts

- The current exact path sorts `V`, then projects and compacts to `C`.
- Every eligible measured frame in the cited Web/Mac cohorts satisfies
  `D = C <= V <= S`.
- The Web post-sort cohort is byte-identical across CPU/GPU/Adaptive and to
  its pre-change screenshots.
- Truck's formal cameras produce `C/V` between 52.85% and 58.54% in the cited
  viewports.
- The portable GPU sorter retains all 32 key bits with eight stable base-16
  passes. Wider base-256/four-pass radix is only qualified on its target-
  allowlisted Metal path; the Adreno attempt remains rejected. See
  [resident-radix8.md](resident-radix8.md) and
  [resident-radix8-portability-experiment.md](resident-radix8-portability-experiment.md).
- The resolved SH color depends on camera position. The current resource owner
  already reuses it only when that position is exactly unchanged; pure camera
  rotation is therefore an exact reuse, not an angular approximation. See
  `last_resolved_camera_position` in
  [`resident_gpu.rs`](../../../../crates/gsplat-render-wgpu/src/resident_gpu.rs).

### Causal inferences used to choose the next experiment

- Moving exact `C` construction before radix should reduce radix dispatch
  traffic because each pass addresses `C` rather than `V`. The benefit should
  grow as `C/V` falls, but scan cost, cache traffic, fixed pass count, and GPU
  occupancy make the relationship non-linear.
- Source-indexed projected data should remove the post-sort rank indirection
  and let the sorted source ID address both projection and resolved color. It
  may reduce bandwidth; whether locality worsens enough to offset that is an
  open measurement.
- A direct `S -> C` projector avoids a preliminary scan, while `S -> V -> C`
  avoids full covariance projection for depth-rejected sources. Which wins is
  a device/scene/camera property, not an architectural constant.
- On Adreno, CPU work can overlap with a GPU already busy projecting and
  rasterizing; on other workloads the upload/synchronization cost can erase
  that advantage. This explains why both backends remain first-class, but it
  is not a proof of a universal scheduling model.

### Still unknown

- Whether pre-project `C` improves completed-frame throughput on M4, WebGPU,
  and Adreno after including every new scan and cache access;
- whether direct `S -> C` or optional `S -> V -> C` is the better GPU producer;
- whether the full32 base-16 path remains the best portable radix after its
  input shrinks to `C`;
- the crossover for Truck prefixes, Flowers, Bonsai, Garden, and Bicycle on
  each device;
- physical-iPhone performance; simulator data cannot answer it;
- whether an exact no-drop tiled raster can beat hardware raster after its
  pair-generation and blend-order costs are included.

These unknowns are the reason for the Phase 2 slices below. They are not gaps
to fill with assumptions.

## Proposed GPU dataflow

### Direct `S -> C` baseline

The first falsifiable implementation should process sources in ascending
source-ID order:

1. `project_count` computes canonical camera depth and the existing exact f32
   projection once per source. It writes a source-indexed cache and one count
   per workgroup.
2. Invalid projected entries retain the current alpha-zero sentinel. The
   direct producer matches the *composed* release path: positive near/far
   inclusion rejects non-finite depth before ordering, while ambiguous alpha
   and projection/clip values preserve the existing fail-open behavior. See
   [the direct-contributor checkpoint](phase2-direct-contributor-checkpoint.md#non-finite-rule-resolved-from-the-composed-release-path).
3. A hierarchical exclusive scan converts workgroup counts to deterministic
   output offsets. It uses no cross-workgroup polling or CPU readback.
4. `compact_key_id` computes each lane's local stable rank and writes
   `{canonical_depth_key, source_id}` into the radix input prefix. Because
   workgroups and lanes cover ascending source IDs, equal keys enter radix in
   ascending source order.
5. The portable stable radix sorts only the GPU-written indirect prefix `C`.
   All eight base-16 passes remain the baseline; the four-pass base-256 path
   remains target-qualified rather than assumed portable.
6. The final sorted source IDs index the source-indexed projected cache and
   resolved-color plane. A GPU-written indirect draw issues exactly `C`
   four-vertex instances.

The proposed two-plane cache remains 32 bytes per source:

```text
projected_center_alpha_key[source_id] = {center.x, center.y, alpha, bitcast(key)}
projected_axes[source_id]             = {axis_u.x, axis_u.y, axis_v.x, axis_v.y}
```

The fourth word no longer needs to carry a redundant source ID because the
array index is the source ID. It can preserve the exact key between projection
and compaction without adding a key plane.

The direct path's first-version capacity is approximately:

```text
source-indexed projected cache     32S
existing ping-pong radix capacity  16S + scan metadata
group counts/indirect arguments    small relative to S
total                              about 48S + metadata
```

Radix allocation may still reserve `S` so that any camera is admissible even
though a frame dispatches only `C`; the immediate claim is lower active
traffic and removal of the `4S` rank plane, not a proportional memory saving.
Every byte formula must be emitted by transactional preflight before product
promotion.

### Optional stable `S -> V -> C`

A second offscreen candidate may first apply only predicates proven incapable
of removing a true contributor, such as the canonical near/far depth and
source-alpha checks. Its scan must emit `V` in source order. Full projection
then writes source-indexed cache entries and compacts `V -> C`, again
preserving source order.

Center-only viewport rejection is not automatically safe: a large Gaussian
whose center is outside the viewport can still overlap it. The optional
prefilter may use only predicates with a conservative proof and an exhaustive
CPU/GPU source-ID oracle.

`S -> V -> C` introduces another pass, scan, and temporary ID stream. It is
accepted only if the measured saved projection work exceeds those costs on a
named target. Direct `S -> C` remains the simpler portable baseline.

## Ordering, cache, and synchronization contract

The new producer must reuse the current canonical f32 depth sequence and key
mapping. It may reorganize work but not change arithmetic to gain speed.
Stable source-order compaction is part of the public image contract because
equal-depth alpha blending is order-sensitive.

Global atomic range reservation is forbidden for the compacted output. It can
produce a correct count while allowing workgroup scheduling to choose the
relative order of equal-key sources. Hierarchical scan plus lane-local rank is
deterministic on Metal, Vulkan, and WebGPU without assuming subgroup width.

All production passes are encoded in one GPU dependency chain:

```text
project/count -> scan -> stable compact -> indirect full32 radix -> indirect draw
```

Compute/render pass boundaries establish GPU visibility. No contributor count
or sort count is mapped to the CPU before the frame can continue. Counts and
stage timestamps may be copied into bounded telemetry rings and consumed
later; telemetry cannot control correctness or introduce a per-frame stall.

Initialization and resize remain transactional: allocate and validate the
complete replacement cache, scan, radix, bind groups, and pipelines before
publishing them. If construction fails, retain the previous qualified path or
return a structured failure. Never publish a half-initialized new path and
never use a smaller fixed buffer as permission to truncate `C`.

Separate planes remain below negotiated per-binding limits. The architecture
does not treat WebGPU's default 128 MiB storage-binding limit as total memory,
nor assume that Vulkan, Metal, or OpenGL ES expose the same negotiated value.

## CPU and Adaptive remain product features

Phase 2 does not replace CPU sorting with GPU sorting:

- **CPU forced** initially keeps the current parallel depth preprocessing,
  stable full32 CPU radix over `V`, upload, exact GPU projection, `C`
  compaction, and hardware draw.
- **GPU forced** gains the experimental pre-project `C` producer and sorts
  only `C` when the adapter passes capability and allocation admission.
- **Adaptive** compares completed CPU and GPU frame tickets, retains
  hysteresis/cooldown, and periodically re-probes. Scene size, contributor
  ratio, resolution, camera motion, thermal state, and contention can all move
  the crossover.

Phase 2.4 may test a parallel CPU implementation of the same conservative
projection predicate, then stable radix over CPU-produced `C`. It must match
the GPU source-ID/key oracle and include projection cost. A GPU projection
followed by synchronous CPU readback is not an acceptable CPU backend.

Exact SH3 resolve is orthogonal to ordering. Resolve all retained bands when
camera position changes; reuse only for an exactly unchanged position. Do not
adopt an angular or time threshold under the full-quality label.

## Why hardware raster is the mainline

The repository's current hardware path already has the desired high-level
shape after sort: stable back-to-front source IDs, four-vertex quads, exact
fragment cutoff, fixed blend state, and no tile-pair materialization.

The current tiled diagnostic is valuable because it exposes real costs, but
it is not production-equivalent:

- [`tiled_resident_gpu.rs`](../../../../crates/gsplat-render-wgpu/src/tiled_resident_gpu.rs)
  first counts every `<tile, source>` pair, copies status to a CPU-mapped
  buffer, allocates checked exact capacity, then submits scatter/sort/raster;
- tile-pair count is camera- and covariance-dependent and can greatly exceed
  `C`, so a guessed fixed capacity risks either waste or truncation;
- [`tiled_resident_raster.wgsl`](../../../../crates/gsplat-render-wgpu/shaders/tiled_resident_raster.wgsl)
  accumulates front-to-back into an `rgba16float` intermediate and stops when
  transmittance falls below `1e-4`;
- front-to-back compute accumulation changes operation order from the retained
  back-to-front hardware blend, so removing early exit alone does not prove
  image identity.

| Tiled proposal | Full-quality disposition |
| --- | --- |
| fixed pair budget with overflow drop | reject |
| top-K per tile, OIT approximation, or point cap | reject |
| fp16 projected attributes or RGBA8 resolved color | reject |
| transmittance early-out | reject for production full-quality |
| per-frame GPU-to-CPU count before raster | correct diagnostic, reject as the production scheduling shape |
| exact pair enumeration, checked capacity, full traversal, fixed image gate | eligible for a later experiment, not assumed faster |

WebSplatter also chose hardware rasterization after identifying the bandwidth
cost of tile/Gaussian pair materialization; see its
[rasterization section](https://arxiv.org/html/2602.03207#S3.SS4). That is a
useful architectural signal, not local completion evidence.

Phase 2.5 opens only if the measured P2.2 hardware result still leaves raster
as the dominant target and a no-drop pair budget is feasible on the named
device. Even then, it competes against hardware raster under the same quality
and count receipts.

## Competitor ideas: adopt the mechanism, not the shortcuts

### Pinned PlayCanvas audit

The reproducible package identity is recorded in
[`tests/competitive/playcanvas/package.json`](../../../../tests/competitive/playcanvas/package.json),
[`package-lock.json`](../../../../tests/competitive/playcanvas/package-lock.json),
and [`expected-engine.json`](../../../../tests/competitive/playcanvas/expected-engine.json):
PlayCanvas `2.21.0-beta.14`, revision
`d5fe88878e338936fe763bbce1a58bc315e89cbe`.

The locally pinned sources show the following:

| Technique | Local primary-source location | Decision here |
| --- | --- | --- |
| project/cull before sort and draw only the projector count | `gsplat-hybrid-renderer.js`, `sortAndProjectForCamera`; `gsplat-projector.js` | adopt the stage order |
| full GPU work generation and indirect draw | `gsplat-projector.js`; `compute-gsplat-projector.js` | adopt without CPU readback |
| portable hierarchical multipass radix; OneSweep only on a narrow capability/vendor path | `graphics/radix-sort/compute-radix-sort.js` and `compute-radix-sort-multipass.js` | adopt the portability principle; retain local qualification |
| 10-20 sort bits selected from element count | `gsplat-hybrid-renderer.js`, `numBits` | reject; keep full32 |
| fp16 axes and packed RGBA in projected cache | `compute-gsplat-projector.js` | reject for this quality path |
| compact 24-byte quantized work format | `gsplat-params.js` and `containerCompactWrite.js` | do not substitute for exact resident geometry |
| default `colorUpdateAngle = 10` | `gsplat-params.js`; `gsplat-world.js` | reject angular SH reuse |
| backend-based AUTO selection | `gsplat-params.js` | retain measured Adaptive instead |

The projector uses workgroup-local counting followed by a global atomic range
reservation. **Source-derived inference:** local order is stable inside a
workgroup, but global workgroup reservation order is scheduling-dependent;
therefore equal quantized keys need not retain ascending source-ID ties. This
is not a reported PlayCanvas defect and was not established by a visual
failure. It is simply insufficient for this project's explicit stable-tie
contract, so the local design uses deterministic scan.

Likewise, PlayCanvas's assertions and interval counts are useful engineering
mechanisms, but the audited sources do not expose this project's structured
`S/V/C/D`, allocation, and overflow receipts. That is a difference in required
evidence, not a claim that normal PlayCanvas scenes silently overflow.

The pinned 56.5846 FPS Truck result remains a product-throughput target and an
architecture reference, not an equal-quality denominator. Its key width,
projected precision, SH-update policy, and recorded image differ from this
renderer, as detailed in [findings.md](findings.md#competitor-audit-from-primary-sources).

### WebSplatter audit

The WebSplatter paper provides primary-source support for three useful choices:

- [pre-project and compact visible contributors](https://arxiv.org/html/2602.03207#S3.SS2);
- a [stable four-pass, 8-bit, full32 radix](https://arxiv.org/html/2602.03207#S3.SS3.SSS2)
  implemented with hierarchical Blelloch scans and no cross-workgroup polling;
- [hardware raster](https://arxiv.org/html/2602.03207#S3.SS4) instead of a
  bandwidth-heavy tile-pair buffer.

It also uses lower-precision projected/color caches and its public evaluation
does not state the exact pixel resolution in the relevant result tables. Its
reported FPS is therefore not a matched local quality/performance result. The
anonymous code URL linked by the paper returned HTTP 401 (`not_connected`)
during this audit, so implementation claims here rely only on the paper, not
on unavailable source code.

WebSplatter validates the architecture direction. Only local full-ID,
full-resolution, image, and terminal experiments can validate this
implementation.

## Phase slices and falsifiable outcomes

Each slice closes with a result even when the optimization loses. Performance
guidance below decides retention, target-gating, or rejection; it is not a
hard task-completion barrier and must not cause an open-ended tuning loop.

### P2.0 — stage telemetry and same-binary control

- Add stable stage receipts for exact projection/count, scan/compact, radix,
  draw/raster, and full frame completion.
- Add a runtime old-post-sort/new-pre-project switch in one binary; forced CPU,
  forced GPU, and Adaptive remain separately visible.
- Record `S`, optional `V`, `C`, `D`, active radix entries, resolution, cache
  generation, backend, fallbacks, and terminal status.
- Do not change default output yet.

Outcome: attribution can be tested without comparing different binaries.

### P2.1 — offscreen deterministic `C` oracle

- Implement direct `S -> C` and optional stable `S -> V -> C` behind an
  offscreen/diagnostic switch.
- Read back every `{key, source_id}` for adversarial sizes, all-visible,
  all-invisible, sparse, equal-key, near/far boundary, non-finite, Truck, and
  Garden/Bicycle cameras.
- Compare complete ordered arrays to the canonical CPU oracle, not only counts
  or screenshots.
- Emit exact resource formulas and transactional-allocation failures.

Outcome: select direct or two-stage production candidate per target evidence,
or reject both with the mismatch/cost recorded. No product default changes.

### P2.2 — GPU production candidate with hardware draw

- Feed the exact `C` prefix to the portable stable full32 radix and indirect
  hardware draw.
- Keep the old GPU path runtime-selectable in the same binary.
- Run image, count, full-order, completed-frame, and p95 frame-time comparisons
  at formal resolution.
- Let Adaptive measure the new GPU path against the unchanged CPU path.

Outcome: retain globally, retain on a qualified target allowlist, or reject.
A performance loss is a completed experiment, not permission to lower quality.

### P2.3 — resource reuse and pass fusion

- Reuse scan/radix high-water allocations transactionally across camera
  changes and resizes.
- Test only fusions that preserve pass visibility, key bits, stable ties, and
  telemetry.
- Keep the source-indexed f32 cache and exact SH scheduling unchanged.

Outcome: accept independently measured reductions; revert neutral or harmful
changes without blocking later slices.

### P2.4 — optional CPU pre-sort `C`

- Parallelize the exact conservative projection/classification on CPU and sort
  only its stable `C` prefix.
- Prioritize Nothing A065 and the 50k-6.13M ladder because current Android
  evidence often selects CPU.
- Include CPU projection, upload, GPU contention, and completion in the metric.

Outcome: retain only where completed-frame evidence wins; otherwise keep the
current CPU `V` sorter.

### P2.5 — conditional no-drop tiled research

- Enter only after P2.2 stage receipts identify raster as the remaining
  dominant cost on a named target.
- Remove early-out and lower-precision intermediates, enumerate every required
  pair, prove overflow handling, and compare the full image/order contract.
- Include pair generation, tile sort, raster, blit, peak allocation, and
  completion in the result.

Outcome: target-gate or reject. It does not delay shipping the qualified
hardware path.

## Experiment matrix

The model ladder should cover complete scenes and deterministic Truck prefixes:

```text
50k, 200k, Flowers 562,974, 1M, Bonsai 1,244,819,
2M, Truck 2,541,226, Garden 5,834,784, Bicycle 6,131,954
```

| Target | Required evidence |
| --- | --- |
| Apple M4/Metal | 1920x1080, old/new GPU paired schedule, CPU/GPU/Adaptive, Truck plus sustained Garden/Bicycle and ladder anchors |
| Chrome/WebGPU on M4 | 1920x1080, same binary and cameras, terminal queue window, complete manifests and PNGs |
| Nothing A065/Adreno | native 2412x1080, randomized backend/order schedule, thermal receipts, current ladder, Truck and capacity-tested Garden/Bicycle |
| iOS simulator | build/integration/count/image only; no performance claim |
| physical iPhone | same formal device protocol when hardware becomes available |
| other desktop adapters | compile/capability receipts first, then the same formal protocol on actual hardware; no extrapolation from M4 |

Every accepted run records source/decoded/encoded/resident/addressable counts,
SH degree, `S/V/C/D`, actual backend, exact resolution, camera/scene/binary
hashes, terminal count, failures, and stage/full-frame timings. Missing
terminal receipts, failed allocation, partial data, or hash mismatch excludes
the run from aggregates but remains a failure artifact.

Default retention heuristics are deliberately modest and non-blocking:

- for pre-project `C`, prefer retention when paired median completed-frame
  throughput improves at least 5%, a bootstrap confidence interval excludes
  zero, and p95 frame time is not worse by more than 3%;
- smaller repeatable wins may be retained when complexity and memory fall, but
  must be labeled rather than rounded up to the target;
- target-specific wins may be allowlisted while other targets keep the old
  path;
- a neutral or losing result closes the slice as rejected/deferred; it does
  not trigger indefinite tuning;
- tiled compute needs a larger, roughly 10% completed-frame win because it
  adds substantial memory and correctness surface. This is a retention
  heuristic, not a completion gate.

Correctness gates are hard. Performance bands are decision aids.

## Promotion checklist

A Phase 2 implementation is eligible for the release-gated SortedAlpha path
only when it has:

- complete ordered `{key, source_id}` equality against the canonical oracle;
- `D = C` and full `S`/SH receipts at every accepted camera;
- Direct/current-path image gates at formal resolution;
- no synchronous control-path GPU readback;
- negotiated binding/allocation admission with structured failure;
- same-binary CPU/GPU/Adaptive terminal evidence on every available real
  target;
- explicit target gating for any non-portable radix or producer variant;
- a recorded rejection, rather than a silent fallback, for overflow,
  unsupported capability, or incomplete initialization.

The design goal is not “always GPU.” It is to remove provably unnecessary
work before the chosen sorter, keep the exact native path, and let current
device evidence select CPU or GPU without sacrificing a Gaussian or a pixel of
requested resolution.
