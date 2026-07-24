# Native Render Exact Core Final Report

## Outcome

Package E is Accepted at the integrated implementation tip
`d721ea6cd0c334e28d3ad5c28792383524e27935`. It delivers one private Exact
renderer core with complete CPU PostSort, GPU PostSort and GPU Preproject plans,
one whole-plan Adaptive controller, one canonical raster and common offscreen
and Surface-shadow target adapters.

The package remains deliberately shadowed. Public constructors, the C ABI,
Web/Android/Apple wrappers and product defaults still use the legacy renderer
and `SurfaceRenderSession`. Product migration, rollback and legacy deletion are
owned by Package M.

## Exact contract retained

Every admitted plan preserves:

- complete source, encoded, resident and addressable membership;
- complete source SH0--SH3 degree and coherent view-dependent color;
- stable full32 depth ordering with deterministic source-ID ties;
- the requested camera and exact backing resolution;
- the qualified premultiplied `SortedAlpha` canonical projected-quad raster;
- no sampling, LOD, dynamic resolution, upscaling or automatic Paged fallback.

The complete plan set is:

1. CPU PostSort: CPU visibility/depth/full32 stable radix, GPU rank projection,
   direct `D=V` draw.
2. GPU PostSort: GPU visibility, stable compaction/full32 radix and projection,
   indirect `D=V` draw.
3. GPU Preproject: GPU project-all, exact contributor compaction and stable
   contributor-prefix radix, indirect `D=C<=V<=S` draw.

Adaptive compares only these complete same-Exact plans. It begins from the
prepared CPU fallback and uses bounded learning, interleaved probes,
hysteresis, minimum residency, cooldown and reprobe. It does not select by
source-point threshold or tune producer, sorter and raster as independent
axes.

## Ownership result

- `PreparedRuntimeSlot` owns the only shadow `PreparedRuntime`, `FrameState`,
  `PlanSet`, `WholePlanController`, mandatory `PlanSampler` and optional
  evidence ring.
- Each concrete plan owns its reusable cache/workspace and returns one
  `ProjectedWork`; only Renderer invokes the canonical raster.
- `SurfaceLifecycle`, `SurfaceConfigurationOwner` and `SurfaceCapture` retain
  primitive Surface responsibilities. The Surface shadow adapter does not own
  plan policy, renderer generations or a second evidence path.
- Offscreen owns its target and readback mechanics. Surface owns acquire,
  retry and presentation mechanics. Both call the same shadow renderer core.
- Optional evidence receives a copy only after mandatory controller sampling;
  pressure or disablement cannot alter execution.

The repository still contains the legacy product/session owners. That is an
intentional migration boundary, not a claim of repository-wide deletion.
Package M2 performs the Surface cutover and M7 removes obsolete legacy owners.

## Transactional Surface result

E12 corrected the last known Package E correctness gap by separating GPU queue
submission from semantic publication:

```text
begin and validate
-> acquire or retry target
-> encode one plan plus canonical raster
-> append capture
-> submit unpublished
-> primitive present
-> mark capture presented
-> publish frame, result and controller ticket
```

Submission returns an owned `SubmittedGpuFrame`. Only a matching, current
target transaction may publish it. A failed preflight, unavailable target,
abandoned presentation, stale generation, duplicate finalize, resize race or
late completion cannot advance semantic frame state or enter the live
controller/evidence ring. The previous target receipt is cleared before every
fallible attempt.

The terminal result binds actual plan and CPU/GPU order lane, all semantic
generations, source count, count relationship, encode/submission identity,
optional formal sample ticket, and requested/Surface/internal/presented
dimensions. CPU PostSort reports numeric `D=V` with C unavailable. GPU-owned
numeric V/C/D remain unavailable rather than being fabricated, while GPU
PostSort retains `D=V` and GPU Preproject retains `D=C` semantics.

## Correctness evidence

- Common offscreen contract/image tests cover complete membership, stable
  order, equal-depth ties, boundary sizes and SH0--SH3.
- Direct/Global reference images and the canonical ProjectedQuads oracle remain
  in their existing qualified scope.
- Fresh Apple M4 Metal tests exercise complete SH3 CPU PostSort, GPU PostSort,
  GPU Preproject and Adaptive Surface-shadow frames against the same target and
  receipt contract.
- Lifecycle tests cover unavailable/failed acquire, submitted-but-not-presented
  completion, capture retry, resize, stale/duplicate finalize, abandoned
  transactions and old-sample invalidation.
- The real `wgpu::Surface` acquire/present adapter is compiled and received
  fixed-SHA static review. Runtime shadow evidence uses a Metal texture plus an
  injected primitive-present success; no real OS window/swapchain was claimed.

Fresh verification on the integrated implementation tip passed:

- workspace `cargo check` and tests;
- 400 renderer tests passed, with eight existing research tests ignored;
- strict workspace/all-target Clippy and warning-free Rustdoc;
- wasm32 compilation;
- source-architecture checker and self-tests;
- required Apple M4 Metal SortedAlpha conformance;
- C FFI, JNI and Swift smoke routes.

No E12/E13 performance benchmark was run or required.

## Finite task outcomes

- E0--E3 and E6--E13: Accepted.
- E4 AArch64 NEON candidate: Rejected after exact candidates were slower than
  Scalar in the finite Apple M4 experiment; production Scalar behavior stayed
  intact.
- E5 x86_64 AVX2/FMA qualification: Deferred because the available physical
  x86 endpoint lacked the required feature support. Emulation and
  cross-compilation were not presented as performance evidence.
- E10 retained the accepted canonical raster/frame-transaction ownership. Its
  isolated submission/batching performance hypothesis was rejected and did
  not trigger repeated tuning or a production split.

## Claim boundary and next package

Package E proves private Exact-core ownership and parity. It does not prove:

- a real-window `wgpu::Surface` run through the new core;
- browser, Android A065 or iOS consumer cutover;
- a cross-device plan winner;
- an FPS or PlayCanvas advantage.

Those boundaries are explicit. M0 next freezes the cutover/rollback checklist
and artifact set. M1 migrates native offscreen/desktop consumers, M2 migrates
the shared Surface owner, and M4--M6 migrate Web, Android and Apple consumers
only after the shared cutover is accepted.

Known correctness issues at Package E closeout: none.
