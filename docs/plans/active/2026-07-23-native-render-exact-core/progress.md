# Native Render Exact Core Progress

> Parent roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

## Machine task-state registry

<!-- gsplat-program-task-states: begin -->
E0 = Accepted
E1 = Accepted
E2 = Accepted
E3 = Active
E8 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 1ac92e114378065eadbb0537da5a416a68faee0b
E3 = E3-cpu-order
E8 = E8a-gpu-adapter
<!-- gsplat-program-active-lanes: end -->

## Package status

- Package: E — Exact prepared plans and native execution, still shadowed.
- Active tasks: E3 CPU order-engine seam and E8a GPU runtime adapter under the
  exact disjoint parallel contract below.
- Last completed task: E2 — CPU shadow oracle.
- E1 state: Accepted after root fixed-SHA review and fast-forward integration
  of candidate `0496266cc73de5fc84acb7c393606fa68eb77623`.
- Dependency: Package A/A9 Accepted.
- Product default: unchanged legacy renderer/session path.
- E8 feasibility audit: stopped without changes after proving that the E1
  runtime lacks a real device-owned GPU scene/preparation/encoder seam. E8 is
  now Active only as the finite E8a adapter lane; the actual GPU PostSort plan
  resumes from a fresh exact root baseline only after that adapter is Accepted.
  Later E4/NEON and E5/AVX2 lanes require E3's
  platform-leaf interface; E6/E7 remain sequential because both own
  engine/workspace decisions.
- Current safe parallel batch: E3's CPU order-engine seam and E8a's finite GPU
  runtime adapter run as two user-visible tasks from one exact root SHA. Their
  production allowlists and mutable owners are disjoint; root alone owns shared
  architecture policy/ledger edits and the integration matrix. Resume the GPU
  PostSort implementation only after the adapter is Accepted.
  Never create parallel writers for the same mutable owner merely to increase
  concurrency.
- Source-size rule: no fixed LOC quota, split trigger or completion gate. Module
  boundaries follow responsibility, dependency direction, compatibility,
  testability and maintenance risk. A new mixed-responsibility multi-thousand
  owner requires finite review but does not force an artificial split.
- Numeric-policy correction: `800` and the mistyped `890` are not design
  requirements and must not appear in writer prompts. The checker has no
  fixed-line blocking ceiling for new files; its multi-thousand signal is
  review-only. Existing legacy-owner baselines remain one-way no-growth
  evidence until their responsibility exit condition is satisfied.
- Performance rule: no fixed FPS or competitor percentage gate. Correctness,
  exactness, fail-closed publication and truthful evidence are hard; measured
  performance determines scoped admission or rejection.

## E0 closeout

- Hypothesis: the existing exact-count proofs can be mapped to one small,
  immutable prepared-plan vocabulary before implementation, preventing E1 and
  later parallel lanes from inventing competing state or controller models.
- Final state: Accepted.
- Ended: 2026-07-24 CST.
- Baseline and exact parent:
  `7848912803f9601fb598dcca07b532e9d9893407`.
- Baseline worktree: clean isolated worktree before the first edit.
- Candidate: one focused documentation commit containing only the three files
  in this package. Its exact SHA is reported in the handoff because a commit
  cannot truthfully contain its own object ID.
- Deliverables:
  - [contract.md](contract.md) freezes `PreparedRuntime`, closed `PlanSet`,
    concrete prepared plans/`PlanId`, `ProjectedWork`, per-plan caches,
    generations, transaction/fallback, single result/evidence ownership and
    writer-lane sequencing;
  - [oracle.md](oracle.md) maps full membership, SH0--SH3, stable full32 ties,
    `S/V/C/D`, camera/resolution, ProjectedQuads/Global, Surface/offscreen and
    ABI/wasm/platform evidence to target owners and E1--E13.
- Scope result: no production Rust/WGSL, public API/ABI, platform wrapper,
  benchmark schema, JSON policy or product default changed.
- Correctness/performance boundary: hard Exact proof is separate from scoped
  performance observation. Historical timings cannot be combined into a
  final conclusion, and no FPS, competitor percentage, 800/890-line or other
  physical LOC target is a task condition.
- Ownership result: CPU PostSort, GPU PostSort and GPU Preproject are complete
  plans selected by one future whole-plan controller. There is no public
  `RenderPlan` trait, boxed pass vector, second controller or platform-host
  strategy policy.
- Verification:
  - PASS exact three-file scope and frozen parent;
  - PASS all local Markdown links in the E package;
  - PASS architecture checker self-tests and real-tree checker;
  - PASS architecture policy JSON parse;
  - PASS `git diff --check`.
- Performance/device work: none, as required for this documentation-only task.
- Known correctness issues at closeout: none in the frozen E0 document scope.
- Integration: root task owns fixed-SHA review and integration; this writer
  does not merge, rebase, cherry-pick or push.
- E0 handoff left E1 eligible but inactive; root activated E1 only after the
  candidate passed fixed-SHA review and integration.

## E1 closeout

- Objective: introduce the private, shadow-only foundation frozen by E0:
  `PreparedRuntime`, an Exact resident scene owner, a closed fail-closed
  `PlanSet`, reusable CPU PostSort fallback and an accessor-based
  `ProjectedWork` handoff. Product routing and public API/ABI remain unchanged.
- Writer: one user-visible Codex task in an isolated worktree based on the
  exact root activation commit. Collaboration subagents may perform bounded
  read-only review but must not author E1 files.
- Production allowlist:
  - `crates/gsplat-render-wgpu/src/lib.rs` for private module wiring only;
  - `crates/gsplat-render-wgpu/src/scene/mod.rs` for private runtime wiring only;
  - new `crates/gsplat-render-wgpu/src/scene/runtime.rs`;
  - new `crates/gsplat-render-wgpu/src/renderer/mod.rs` and `frame.rs`;
  - new `crates/gsplat-render-wgpu/src/plans/mod.rs` and `cpu_post.rs`.
- Governance allowlist:
  - `tests/architecture/source_architecture_policy.json`;
  - `tests/architecture/test_source_architecture.py`.
  The writer does not edit this progress ledger; root owns activation and
  acceptance records.
- Frozen scope: public exports/errors/ABI, existing CPU/GPU primitives,
  Resident encoding, Surface/session/offscreen product routes, raster owners,
  WGSL, FFI, Web/mobile/desktop consumers, Cargo dependencies and benchmark
  schemas.
- Required invariants:
  - E1 supports Exact + all-resident only and never creates Streamed/LOD/Paged
    placeholders;
  - only CPU PostSort is prepared and eligible, while the closed `PlanId`
    vocabulary reserves GPU PostSort and GPU Preproject identities;
  - empty plan sets, missing fallback and requests for unprepared plans fail
    closed without mutating the live runtime;
  - candidate preparation validates the whole scene/contract/plan set before
    one infallible publication swap; failure preserves old scene, generations,
    fallback and last usable order;
  - CPU workspaces and the sorter are reused, positions are borrowed from the
    resident scene, stable descending full32 depth plus source-ID ties remain
    authoritative, and WASM keeps its existing scalar path;
  - E1 may report source/visible counts only. It must not invent contributor or
    drawn counts before projection/raster work exists;
  - no second mutable `Renderer`, controller, sampler, evidence ring, public
    plan trait or boxed runtime pass graph is introduced.
- Architecture activation: register only the real per-frame boundaries that
  E1 implements in `renderer/mod.rs`, `plans/mod.rs` and `plans/cpu_post.rs`.
  Constructors and per-frame execution stay separate; numeric function-size
  notices are advisory and never an acceptance condition.
- Focused writer gates: architecture checker/self-tests, format/diff, exact
  allowlist and frozen-file hash audit, focused new unit tests, locked renderer
  check/lib tests/all-target Clippy, and locked wasm32 Web check. Root performs
  the shared workspace/GPU/FFI matrix after integration.
- Parallel seam after acceptance:
  - E2 may own renderer tests and a test-only offscreen shadow adapter without
    matching private plan internals;
  - E8 may own `plans/gpu_post.rs` and plan registration without touching
    renderer/offscreen;
  - either task stops and requests a smaller adapter task if it needs the
    other's files.

- Final state: Accepted.
- Exact parent: `83755c78fa62e4d650bfd9846e47faf5f6a67c08`.
- Accepted candidate: `0496266cc73de5fc84acb7c393606fa68eb77623`.
- Scope: exactly the nine paths in the active contract; 174 frozen files and
  every product route, public API/ABI, GPU primitive, raster owner and WGSL
  blob remained unchanged.
- Root fixed-SHA review: P0/P1/P2 blocking findings all zero. The review
  confirmed single resident-scene ownership, fail-closed `PlanSet`, reusable
  CPU workspace, inclusive visibility, stable full32/source-ID ordering,
  transactional runtime and camera/viewport publication, and unavailable C/D
  rather than fabricated values.
- Fresh evidence: architecture checker/self-tests, format/diff, renderer and
  plan focused tests, all 328 library tests (323 passed, five existing external
  data/GPU tests ignored), strict all-target Clippy and wasm32 Web check passed.
- Honest seam boundary: E2 has enough plan-independent CPU accessors. E8 still
  needs a real device-owned projected-view/preparation adapter; E1 intentionally
  did not invent a GPU owner, fake raster or host-visible indirect count.

## E2 / E8 original activation contract (historical)

- Shared baseline and activation commit:
  `0496266cc73de5fc84acb7c393606fa68eb77623`.
- Writer model: two user-visible Codex tasks, each with its own worktree and
  unbudgeted goal. Collaboration subagents remain read-only reviewers.
- E2-shadow owns only:
  - `crates/gsplat-render-wgpu/src/offscreen/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/offscreen/shadow.rs`;
  - `crates/gsplat-render-wgpu/src/renderer/mod.rs` for test-module wiring only;
  - new `crates/gsplat-render-wgpu/src/renderer/contract_tests.rs`;
  - new `crates/gsplat-render-wgpu/src/renderer/tests.rs`.
- E8-gpu-post owns only:
  - `crates/gsplat-render-wgpu/src/plans/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/plans/gpu_post.rs` and optional
    `plans/gpu_post/tests.rs`;
  - `tests/architecture/source_architecture_policy.json` and
    `tests/architecture/test_source_architecture.py` only to register a real
    per-frame GPU-plan boundary.
- E2 proves membership, SH0--SH3, CPU order, generation/currentness and
  Direct/Packed offscreen pixel parity without changing production routing.
- E8 reuses existing `DirectGpuOrder`, Resident GPU resources and
  `ProjectedRankProjector`; it may not absorb `ProjectedQuadsGpu`, raster,
  submit/readback/present, controller or product policy.
- E8 stop condition: if a complete GPU plan requires renderer/offscreen,
  raster/WGSL or existing GPU-primitive edits, it stops inside its allowlist
  and reports the exact missing adapter. E2 continues independently; root then
  activates one finite adapter before resuming E8.
- Declared integration order was E2-shadow then E8-gpu-post. E8 correctly
  stopped before producing a candidate when it proved the missing adapter;
  E2 continued independently and is now Accepted.
- No fixed LOC, FPS or competitor percentage is a writer or acceptance gate.

## E2 closeout

- Final state: Accepted.
- Exact writer parent:
  `708b538f0f6d4517acfdd98003e3d8573ea2c0ee`.
- Accepted candidate:
  `55c7ab7bfa2a4b1509974e6b17d8ca8e8bf6fd4c`.
- Root integration commit:
  `1ac92e114378065eadbb0537da5a416a68faee0b`.
- Scope: exactly five allowlisted paths. New shadow code is native test-only;
  product routing, public API/ABI, WGSL, GPU primitives and raster semantics
  are unchanged.
- Result: the legacy Direct/Packed raster and the private E1 CPU PostSort handoff
  have byte-identical offscreen RGBA coverage for SH0--SH3 while preserving
  full source/resident/work counts, inclusive near/far, stable full32 depth and
  source-ID ties. Contributor/drawn counts remain unavailable rather than
  fabricated before their owners exist.
- Review history: two earlier candidates were rejected, not integrated. The
  first lacked legacy renderer scene/path/mode/config identity. The second did
  not bind the paired shadow scene and could combine renderer A with a fresh
  same-contract frame from replaced slot B. The accepted immutable receipt
  binds both sides, full backing identity and runtime scene generation, and
  rejects every stale combination before external-order raster while
  preserving the prior image.
- Independent final fixed-SHA review: P0/P1/P2 all zero.
- Root shared verification: workspace tests, strict all-target Clippy,
  warning-free Rustdoc, wasm32 Web check, required Metal SortedAlpha, five
  shadow GPU tests, architecture checker/self-tests, format/diff and C FFI
  smoke all pass.

## E3 / E8a parallel active contract

- Activation code commit:
  `1ac92e114378065eadbb0537da5a416a68faee0b`.
- Writer model: two user-visible Codex tasks in separate worktrees. No
  collaboration subagent edits production code.
- E3 owns only the CPU order engine, reusable workspace, scalar dispatcher and
  empty AArch64/x86 platform leaves, its CPU PostSort adapter, legacy sync/
  async CPU-order call sites, and the private `Vec3f` layout proof listed in
  the architecture policy.
- E8a owns only `renderer/mod.rs`, new `renderer/gpu_prepare.rs`, and
  `scene/runtime.rs`. It prepares one exact device-owned GPU scene/plan seam;
  it does not implement the later GPU plan, submit/poll/map/readback/present,
  raster/WGSL, controller or product policy.
- Root alone owns this ledger and architecture-policy edits. The writer
  allowlists have no exact-file or mutable-owner overlap.
- Integration order: E8a adapter, then E3 CPU engine. Each candidate receives
  a fixed-SHA review before integration; root runs the shared matrix once on
  the combined result.
- E3 stop conditions: no public API/C ABI/platform-wrapper changes, no point/
  SH/resolution/order changes, no Adaptive scheduling change and no duplicate
  radix implementation. E8a stops if a truthful adapter requires raster,
  shader, Surface host or submission ownership.
- Module size is reviewed by responsibility cohesion and maintenance risk;
  800, the mistyped 890 and every other fixed LOC number are explicitly absent
  from writer and acceptance gates.

## E8 feasibility stop and adapter dependency

- Audit branch: `codex/e8-gpu-post-plan` at exact activation commit
  `708b538f0f6d4517acfdd98003e3d8573ea2c0ee`.
- Result: no candidate commit and a clean tree. The task did not create a fake
  GPU plan, widen its plans-only allowlist or change product behavior.
- Proven missing seam: `PreparedRuntime`/`SceneRuntime` own CPU resident data
  only; preparation has no device-owned `ResidentGpuResources`, shared
  resolved-color owner or scoped GPU error transaction, while frame execution
  has no queue/encoder input. Consequently a real GPU entry cannot be prepared
  and published by `PlanSet` alone.
- Reusable primitives remain sufficient: resident-SoA `DirectGpuOrder`, stable
  full32 radix and source-ID ties, sorter-owned indirect arguments,
  `ProjectedRankProjector` external projection, and existing Exact resident
  resources. No shader, raster math or GPU algorithm change is required by the
  adapter.
- The finite adapter is now active after E2 because both required
  `renderer/mod.rs`. Its writer implementation allowlist is:
  - `crates/gsplat-render-wgpu/src/renderer/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/renderer/gpu_prepare.rs`;
  - `crates/gsplat-render-wgpu/src/scene/runtime.rs`.
  Root, not the writer, owns any later architecture policy registration.
- Adapter contract: one transactional CPU/GPU scene candidate; async scoped
  device preparation; one resolved-color owner; capacity/count/SH0--SH3
  agreement; a frame seam that receives queue and an existing encoder but can
  never submit, poll, map, read back or present. Unsupported GPU preparation
  omits the GPU entry while preserving the prepared Exact CPU fallback.
- The GPU PostSort implementation resumes only from the accepted E2+adapter
  root SHA in a fresh worktree. Its original plans-only allowlist must still prove
  complete membership, original SH degree, inclusive visibility, stable
  full32 ordering, `D=V`, exact generation guards and unchanged product routing.
