# Native Render Core Migration Progress

> Program roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
>
> Accepted shadow-core package:
> [Package E final report](../../completed/2026-07-23-native-render-exact-core/final-report.md)

## Machine task-state registry

The architecture checker records only Active or terminal tasks in this block.
Unstarted tasks remain outside the machine registry until activated.

<!-- gsplat-program-task-states: begin -->
M0 = Accepted
M1 = Accepted
M2 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: M — atomic product migration and legacy deletion.
- Dependency: E13 Accepted at Exact implementation tip
  `d721ea6cd0c334e28d3ad5c28792383524e27935`.
- M0 state: Accepted after fixed-SHA review of cutover tip
  `e26a1df39780e744112924eb378e098c29be7cd4`.
- M1 state: Accepted at integrated implementation/evidence tip
  `dc3e0de65073f819b714229b726ca55f47c6e6d6`.
- Active task: M2, migration of the shared real-window Surface to the accepted
  Exact core.
- Product state at M2 activation: native Packed offscreen, desktop
  non-interactive and bench-runner use the Exact runtime; the interactive
  `SurfaceRenderSession` remains legacy until its complete M2 candidate is
  accepted.
- Unstarted tasks: M3, M4, M5, M6, M7 and M8. They are pending in roadmap
  order and are not active machine-state entries.
- Public API signatures, C ABI, Web, Android and Apple consumers remain frozen
  during M2.

## Execution coordination

- Every independent implementation slice after M1 is dispatched as a visible
  Codex task with its own worktree, fixed accepted base, narrow owned paths and
  one reviewable candidate SHA. Concrete implementation is not delegated to a
  subagent.
- The root task owns dependency order, fixed-SHA acceptance, integration and
  critical cross-platform verification. A candidate is never integrated merely
  because its implementation task reports completion.
- Subagents, when useful, are limited to short read-only assistance inside the
  active root turn. They do not own implementation packages, mutate candidate
  worktrees or replace the independently inspectable task handoff.
- Parallel Codex tasks are opened only for slices whose ownership and base make
  concurrent work safe. Dependent migrations remain serial rather than being
  forced into parallel execution.

## M0 closeout

- Final state: Accepted.
- Fixed parent/E13 closeout:
  `328e05c4cb55f4824cc9149c08629dd9d2ad5eed`.
- Cutover commits:
  - `4e3f9414b2fff936fb130e02628d09cc6924f3c1` added the complete migration
    contract;
  - `e26a1df39780e744112924eb378e098c29be7cd4` closed all fixed-SHA review
    findings and is `M0_CUTOVER_SHA`.
- Deliverable: [cutover.md](cutover.md) freezes Exact invariants, serial
  implementation order, read-only parallel audit boundaries, unique owners,
  frozen consumers, rollback identities, fixed Kitsune/trace inputs and honest
  artifact acceptance for M1--M8.
- Scope result: documentation only. No Rust, WGSL, API, ABI, wrapper, build
  script, benchmark protocol or product route changed.
- First independent review found five evidence/rollback P1 issues and two
  fixed-SHA/owned-path P2 issues. The follow-up commit closes all seven; the
  second fixed-tip review reported no remaining P0/P1/P2 findings.
- Verification passed:
  - architecture checker self-tests and real-tree policy;
  - all relative Markdown links;
  - canonical validation and raw/content identity for the three fixed Kitsune
    traces;
  - full-quality validator tests;
  - committed-range `git show --check` and `git diff --check`;
  - exact two-commit scope and clean worktree.
- Deferred facts are explicit rather than blockers: the uncommitted Kitsune
  PLY must be freshly fetched/hashed before retained scene evidence, and a
  physical iPhone is probed only in M6.
- No FPS, PlayCanvas or cross-device performance run was performed or claimed.
- The SHA of this root-owned closeout/activation commit is
  `M0_ACCEPT_SHA == M1_BASE_SHA`; it is reported in the handoff and recorded by
  M1 because a commit cannot contain its own object ID.

## M1 activation contract

- Objective: switch the native Packed offscreen path, desktop non-interactive
  consumer and bench-runner to one Exact runtime without changing public
  signatures or any interactive/platform consumer.
- Unique semantic owner: `Renderer` owns one `PreparedRuntimeSlot` for Packed
  offscreen scene, plans, controller, generations, mandatory sampler and frame
  result. The offscreen host owns only the shared device/queue, target and
  readback. It must not retain or clone a second Packed resident scene.
- Required implementation slices:
  1. renderer/offscreen seam with transactional complete GPU preparation,
     forced per-call CPU PostSort refresh and honest phase timings;
  2. desktop non-Surface host separation while the interactive viewer remains
     on the unchanged M2-owned Surface route;
  3. bench/offscreen collector support for the frozen Kitsune 1920x1080 trace,
     complete Exact receipts and final-frame identity.
- Public compatibility:
  - existing `Renderer` constructors, loading/render/readback/wait/resize and
    inspection signatures remain unchanged;
  - `FrameStats` layout and real preprocess/sort/raster meanings remain
    unchanged;
  - `GeometryPath` public default remains Direct; explicit Direct remains the
    wide-f32 image oracle and explicit Paged remains diagnostic only.
- Transaction boundary:
  - failed resident/GPU/plan/raster preparation retains the old scene, image,
    generations and stats;
  - resize publishes target/config only after successful allocation and the
    Exact viewport generation only after a successful rendered frame;
  - readback does not mutate renderer policy or timing state.
- Frozen paths/consumers: shared Surface/session/presenter, C ABI/header, FFI
  wrappers, Web/WASM, Android/JNI/Kotlin, Apple/Swift/iOS, WGSL/raster math and
  competitor protocol.
- Required acceptance evidence:
  - focused SH0--SH3, stable order, failure rollback, resize/readback and owner
    identity tests;
  - Packed Exact versus Direct oracle image/count parity;
  - one canonical Kitsune 1920x1080 offscreen artifact using the frozen trace,
    complete count/SH/resolution/plan receipts and final frame;
  - workspace, strict Clippy/Rustdoc, forced Metal conformance, architecture,
    benchmark artifact and FFI regression checks required by the cutover
    contract.
- Performance values are observations only. M1 has no fixed FPS, speedup or
  competitor-ratio acceptance threshold.

## M1 closeout

- Final state: Accepted.
- Fixed base: `49821f9df4784e60bf176f4c6762694eb406f710`.
- Integrated implementation/evidence tip:
  `dc3e0de65073f819b714229b726ca55f47c6e6d6`.
- Accepted implementation slices:
  - `3021ef9525490ddae0bd17daba059b72183dcae7` and
    `bf1da9d20568d7e8c3ab13366ca0775c38f07704` route native Packed
    offscreen through one transactional Exact runtime and prove ownership,
    rollback, stable ordering, SH0--SH3 and Direct-oracle behavior;
  - integrated commit `4bd4461` separates desktop CLI, trace, scene,
    offscreen, viewer and image-output host responsibilities without changing
    the interactive Surface route;
  - integrated commits `de8e8eb` and `dc3e0de` add the trace-aware Packed
    bench collector, atomic artifact publication, frozen M1 qualification
    contract and image-aware artifact validation.
- The first fixed-SHA bench review rejected three evidence defects: a guessed
  actual PlanId, an unbound `--full-quality` flag and an unvalidated final
  image. The replacement candidate removed the guessed value, records
  `renderer.exact_plan_actual` as unavailable, binds full-quality runs by
  dataset/trace content identity and validates the actual PNG file, size and
  SHA-256 before accepting the artifact.
- Formal retained local evidence:
  - dataset `kitsune`, SHA-256
    `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`,
    279,199 splats and source/resident SH3;
  - trace `candidate-kitsune-quality-2view-1920x1080-v1`, canonical SHA-256
    `8821c193506cdf7d67aa200248a45088c4750a3cd128ee29f6e2dc2d3a5bdb99`,
    sequence `[0, 1]`, 20 warmup and 80 measured frames;
  - Packed artifact
    `target/benchmarks/migration-m1-kitsune-formal` and completed one-cell
    suite `target/benchmarks/migration-m1-kitsune-suite.json` both pass their
    repository validators with all 279,199 points visible and drawn;
  - the Apple M4 Metal observation was 9.806 ms mean queue-complete frame time,
    12.883 ms p95 and zero missed 16.67 ms frames. These values describe this
    run only and are not an acceptance threshold or competitor claim.
- The same scene, trace and resolution were rendered through the Direct oracle.
  Packed RGB mean absolute error was `0.000004691`, no pixel exceeded `3/255`,
  and the maximum channel difference was `1/255`. Three of 2,073,600 alpha
  pixels differed by one byte step; a repeated Packed run was byte-identical,
  so this deterministic path-rounding boundary is recorded rather than hidden.
  The repository's existing focused Kitsune Direct/Packed image gate also
  passes.
- Final verification passed: formatting, workspace check/tests, strict
  workspace Clippy, Rustdoc, forced Metal SortedAlpha conformance, source
  architecture policy, benchmark and full-quality validators, desktop default
  and interactive-feature tests, plus C ABI smoke. Dependency-policy status is
  recorded separately if its external advisory refresh is unavailable.
- The dependency-policy wrapper made no progress while refreshing remote data
  and was stopped; an offline retry could not resolve uncached conditional
  Windows/Khronos crates. This is recorded as environment-unavailable rather
  than a dependency-policy pass or a source failure. M1 introduced no new
  runtime rendering dependency beyond the bench artifact tooling captured in
  `Cargo.lock`.
- No public signature, C ABI, interactive Surface, Web, Android or Apple
  consumer changed in M1. Direct remains the explicit wide-f32 oracle and
  Paged remains diagnostic only.
- The SHA of this root-owned closeout/activation commit is
  `M1_ACCEPT_SHA == M2_BASE_SHA`; it is reported in the handoff and recorded by
  M2 because a commit cannot contain its own object ID.

## M2 activation contract

- Objective: replace the shared real-window Surface's legacy semantic owner
  with the accepted Exact runtime while preserving its public API, C ABI and
  platform consumer behavior.
- Unique owner, frozen paths, rollback identity and required evidence remain
  exactly those defined by [cutover.md](cutover.md#m2--shared-real-window-surface).
- M2 begins only from this Accepted M1 tree. Its implementation is dispatched
  as a separate Codex task/worktree; the root task retains fixed-SHA acceptance,
  integration and real-Surface validation.
