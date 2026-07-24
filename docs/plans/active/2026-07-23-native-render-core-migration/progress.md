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
M1 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: M — atomic product migration and legacy deletion.
- Dependency: E13 Accepted at Exact implementation tip
  `d721ea6cd0c334e28d3ad5c28792383524e27935`.
- M0 state: Accepted after fixed-SHA review of cutover tip
  `e26a1df39780e744112924eb378e098c29be7cd4`.
- Active task: M1, native offscreen, desktop non-Surface and bench-runner
  migration to the accepted Exact core.
- Product state at activation: unchanged legacy renderer and
  `SurfaceRenderSession`; M1 may switch only the native offscreen Packed
  consumer after its complete candidate is accepted.
- Unstarted tasks: M2, M3, M4, M5, M6, M7 and M8. They are pending in
  roadmap order and are not active machine-state entries.
- Interactive Surface, public API signatures, C ABI, Web, Android and Apple
  consumers remain frozen during M1.

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
