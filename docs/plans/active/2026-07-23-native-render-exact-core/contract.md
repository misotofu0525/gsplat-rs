# Exact Prepared-Plan Contract

> Status: frozen E0 design contract
> Baseline: `7848912803f9601fb598dcca07b532e9d9893407`
> Parent architecture:
> [Native Render Core Architecture](../../completed/2026-07-23-native-render-core-refactor/architecture.md)
> Migration oracle: [oracle.md](oracle.md)
> Package ledger: [progress.md](progress.md)

## 1. Scope and truth boundary

This document fixes the private Exact vocabulary, ownership and task seams for
E1--E13. It is a target contract, not a claim that the types or modules already
exist. E0 changes no Rust, WGSL, public API, ABI, platform wrapper, benchmark
schema or product route. The legacy renderer/session remains the product
default until the later migration package performs an atomic cutover.

Exact continues to mean all of the following at once:

- complete source, decoded, encoded, resident and addressable membership;
- complete source SH0--SH3 degree;
- stable full32 depth order with deterministic source-ID ties;
- requested, Surface, internal-render and presented dimensions equal;
- `SortedAlpha` with the qualified covariance, alpha-cutoff, premultiplied
  blend and four-vertex projected-quad contract;
- no sampling, LOD, dynamic resolution, upscaling or automatic Paged fallback.

An execution change may alter cost but not any Exact semantic. Direct f32 and
GlobalQuads remain oracles; they are not extra product plans in this package.

## 2. Frozen vocabulary and owners

### 2.1 `PreparedRuntime`

`PreparedRuntime` is the only publishable runtime bundle:

```rust
struct PreparedRuntime {
    contract: RenderContract,
    scene: SceneRuntime,
    plans: PlanSet,
    raster: CanonicalRaster,
}
```

The fields are a single compatibility unit. `Renderer` prepares a complete
candidate and publishes it with one swap. Scene resources, the immutable Exact
contract, plan resources and raster resources are never published or replaced
independently. `PreparedRuntime` owns no camera, frame scheduling, plan policy,
presentation or evidence policy.

### 2.2 `PlanId`, prepared plans and `PlanSet`

`PlanId` is a closed private enum:

```rust
enum PlanId {
    CpuPostSort,
    GpuPostSort,
    GpuPreproject,
}
```

A prepared plan is one concrete, immutable, contract-compatible resource graph
plus its own reusable workspace and caches. `PreparedPlan` is the collective
term for those concrete plan structs, not a public trait or a boxed interface.
The implementation uses one exhaustive internal `match` over `PlanId`.

`PlanSet` owns the three optional concrete entries, the fixed eligible list and
one present fallback. Construction must prove that:

- at least one Exact plan is fully prepared;
- every eligible ID has a present concrete entry;
- the fallback is present, eligible and Exact-compatible;
- every resource graph passed preflight and scoped creation before publication;
- no entry relies on a controller promise or `unwrap()` for correctness.

There is no public `RenderPlan` trait, no `Vec<Box<dyn pass>>`, no arbitrary
runtime pass DAG and no per-frame graph construction. Adding a plan variant
requires a concrete owner, admission decision, resource receipt, common
contract tests and explicit same-Exact fallback behavior.

### 2.3 `ProjectedWork`

`ProjectedWork` is the single plan-to-raster handoff. It identifies:

- the immutable scene, contract, camera, viewport and plan-set generations;
- actual `PlanId` and actual CPU/GPU order lane;
- the authoritative order owner and order generation;
- rank-indexed projected planes or an equivalent qualified canonical view;
- direct or indirect exact draw-count source;
- `S/V/C/D` values with explicit availability and count semantics;
- the cache guard needed to prove that the work belongs to this frame.

It contains no Surface, submission, presentation, controller or optional
observer state. A plan returns `ProjectedWork`; only `Renderer` passes it to
`CanonicalRaster`, exactly once, and only `Renderer` submits the resulting
command stream.

### 2.4 Per-plan cache

Each prepared plan owns its own order/projection/workspace cache and complete
guard. A cache has no independent scene, camera, viewport or contract revision;
it stores only the authoritative generations on which it depends.

Selecting another `PlanId` does not invalidate a competitor's valid cache.
Otherwise an Adaptive probe would measure an artificial cold start. Reuse is
allowed only when the entire guard matches, including scene, contract, complete
camera, viewport, plan-set, order owner/generation and exact count. A camera
change, resize, scene/contract replacement, order refresh, plan resource
rebuild or count change fails the guard closed before drawing.

### 2.5 Generations

`FrameState` under `Renderer` is the sole semantic generation owner:

```text
scene_generation
camera_revision
viewport_generation
contract_generation
plan_set_generation
```

Plan-local resources may retain guards derived from these values, but may not
create competing revisions. `plan_set_generation` changes when resources are
rebuilt, not when the controller merely selects another plan. Replacing a
runtime expires outstanding samples and tickets from the old generations.

### 2.6 `FrameResult` and mandatory evidence

`Renderer` is the single execution-result owner. It finalizes one small
`FrameResult` for one submitted frame from the selected plan outcome, terminal
completion and the host's primitive target outcome. The result contains status,
actual plan/order lane, exact counts, generation identities and completion or
presentation identity. Hosts adapt acquire/present/readback mechanics; they do
not construct a second renderer result, rewrite counts or infer a plan.

The immutable receipt value types live in the strategy-free `evidence/` leaf.
`Renderer` is the only producer that joins those values into one terminal
sample. The mandatory `PlanSampler` always receives every valid comparable
terminal sample needed by the whole-plan controller. An optional bounded
`EvidenceRing` receives a copy after execution. Disabling, filling or dropping
from the optional ring cannot suppress, delay or change mandatory sampling,
fallback or plan selection.

## 3. Complete plan closure

The three `PlanId` variants are complete execution plans, not independent
producer, sorting, projected-draw and raster switches.

### 3.1 CPU PostSort

```text
CPU exact visibility/depth/full32 keys
-> CPU stable radix with source-ID ties
-> upload authoritative V-order IDs
-> GPU project visible ranks
-> ProjectedWork with direct count (D = V)
-> canonical raster
```

This plan preserves the current native CPU advantage and is E1's first
same-Exact fallback. E3 later unifies synchronous, asynchronous and offscreen
engine/workspace ownership; E4/E5 qualify platform leaves without turning the
kernel choice into another cross-frame controller.

### 3.2 GPU PostSort

```text
GPU exact visibility/depth/full32 keys
-> stable source-order compaction
-> stable full32 radix
-> GPU project visible ranks
-> ProjectedWork with exact indirect count (D = V)
-> canonical raster
```

E8 consolidates the existing qualified mechanics into this one prepared plan.

### 3.3 GPU Preproject

```text
GPU project all S source splats
-> exact contributor compaction in source order
-> stable full32 radix over C
-> ProjectedWork with exact indirect count (D = C <= V <= S)
-> canonical raster
```

E9 consolidates the existing Exact Preproject/Compact diagnostic graph and
removes its separate producer-policy system. It is not a free producer toggle
for CPU or GPU sorting.

CPU/GPU ordering therefore is not an independent switch that can be combined
with arbitrary producers or rasters. The existing measured CPU/GPU Adaptive
feature is preserved by having the final sampler/controller compare the three
complete plans. Evidence may still report the actual order lane, but only
`PlanId` is selected.

## 4. Transactional prepare, publish and fallback

Preparation occurs at construction, scene/contract replacement or a resource-
relevant target transition:

1. build and validate the immutable Exact contract and scene receipt;
2. intersect scene/resource requirements with adapter limits;
3. prepare every admitted plan and its cache/workspace privately;
4. prepare canonical raster resources;
5. prove a non-empty eligible set and present same-Exact fallback;
6. publish the complete `PreparedRuntime` in one swap.

Validation, OOM, internal, cancellation or unsupported-device failure discards
the whole candidate and leaves the old runtime usable. A partially built plan,
bind group, cache, fallback or generation is never observable.

Fallback is fail-closed:

- fallback may select only a fully prepared plan satisfying the same Exact
  contract;
- fallback occurs before encode or after discarding a failed unpublished
  command encoding; work from two plans is never spliced into one frame;
- forced diagnostic selection reports a structured error rather than silently
  changing plan;
- Adaptive may move to the prepared fallback, cool down a failed challenger and
  re-probe later;
- no failure selects Paged, samples points, lowers SH, changes resolution,
  weakens full32 ties or changes raster semantics;
- if no Exact fallback remains usable, the frame or preparation fails.

## 5. Controller, host and dependency boundary

There is exactly one `WholePlanController`, owned by `Renderer`. It consumes
only eligible `PlanId` values and comparable terminal samples. No plan contains
a nested adaptive controller. CPU initialization calibration is bounded and
frozen per scene/device reset; it is a kernel choice inside CPU PostSort, not a
second frame controller.

Surface and offscreen hosts own target lifecycle only. Android, Apple, Web,
desktop and C/FFI wrappers adapt handles, serialized thread/queue ownership and
controls. They do not select plans, own cache generations, sample timing, parse
per-frame strategy overrides or implement fallback. E remains a shadow core;
consumer cutover belongs to Package M.

Lower-layer dependency direction remains:

```text
host -> Renderer -> PreparedRuntime / PlanSet -> cpu|gpu leaves -> data
                  -> CanonicalRaster -> data
Renderer -> mandatory sampler/controller
Renderer -. immutable copy .-> optional evidence ring
```

GPU/CPU leaves know nothing about Renderer, PlanSet, controller, evidence,
Surface or platform wrappers. Plans know nothing about controller, optional
evidence, platform lifecycle, submission or presentation.

## 6. Hard correctness versus performance observation

Hard correctness includes build/contract tests, Exact membership and SH,
stable full32 order/ties, `S/V/C/D`, image/raster parity, complete generation
guards, transactional publication, same-Exact fallback, terminal evidence
identity and target lifecycle parity.

Performance observations include frame time/FPS, a plan winner on one adapter,
percentage difference from another plan or renderer, thermal/energy behavior
and optional endpoint availability. They may determine scoped admission or a
performance hypothesis outcome; they are not E task-completion conditions.

No fixed FPS, fixed competitor lead, fixed file size or physical LOC number is
an E boundary. In particular, 800, 890 or any other number cannot trigger a
split, block review or define completion. Boundaries follow responsibility
cohesion, dependency direction, compatibility, test seams and maintenance risk.
A new multi-thousand-line mixed owner triggers one finite responsibility
review. A cohesive owner may receive a finite documented exception when
another split would worsen ownership; the review cannot become an endless
decomposition loop.

## 7. E-task sequencing and writer lanes

Implementation writers are user-visible independent Codex tasks in isolated
worktrees. Each starts from a fixed SHA with a clean tree, exact write allowlist
and focused verification. Subagents may perform only bounded read-only audits
or fixed-SHA reviews. The root task alone integrates candidates and reruns the
shared matrix.

The frozen schedule is:

1. E0 then E1 are strictly serial shared foundations.
2. After E1 is integrated, E2 and E8 may run in parallel with disjoint
   allowlists; E2 owns common contract/offscreen proof while E8 owns the GPU
   PostSort concrete plan.
3. E3 starts only after E2 and establishes the shared `CpuOrderEngine`,
   workspace and narrow architecture-specific preprocess leaf interface.
4. After E3, E4/NEON and E5/AVX2 may run in parallel. A non-CPU E8 or E9 writer
   may occupy a third lane only when its exact files and mutable owners do not
   intersect either CPU lane and it has no dependency on their unintegrated
   results. Otherwise it remains sequential.
5. E6 and E7 remain sequential because both overlap engine/workspace and
   candidate selection. E6 terminates before E7 calibrates only accepted
   candidates.
6. E10 waits for E8 and E9 terminal results. E11 waits for E7--E10, then adds
   the mandatory sampler and sole controller. E12 and E13 remain the ordered
   Surface-shadow and package-closeout steps.

Parallel activation is never inferred from this document alone. Before writers
start, the package ledger must name the exact active task set, disjoint
allowlists, fixed baseline, separate worktrees, integration order and root-
owned shared gates. Overlapping engine, workspace, plan facade, policy,
evidence, shader contract or lifecycle ownership stays sequential.

## 8. Acceptance questions

Every E candidate must answer:

- Which complete `PlanId` or single shared responsibility changed?
- Are Exact membership, SH, resolution, full32 order/ties and raster unchanged?
- Is the fallback complete, prepared and same-Exact?
- Does one owner control the affected cache and its full generation guard?
- Can failure expose a partial runtime, stale count or cross-generation sample?
- Does any host, plan or evidence sink make a policy decision?
- Are mandatory evidence and optional observation still independent?
- Is the result supported by hard correctness evidence, or only a scoped
  performance observation?

The per-invariant migration gates and inherited evidence sources are frozen in
[oracle.md](oracle.md).
