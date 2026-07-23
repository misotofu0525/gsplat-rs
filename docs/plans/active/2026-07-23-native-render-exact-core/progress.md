# Native Render Exact Core Progress

> Parent roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

## Machine task-state registry

<!-- gsplat-program-task-states: begin -->
E0 = Accepted
E1 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: E — Exact prepared plans and native execution, still shadowed.
- Active task: E1 — private transactional Exact runtime skeleton.
- Last completed task: E0 — Exact prepared-plan contract and migration oracle.
- E1 state: Active after root fixed-SHA review and fast-forward integration of
  E0 candidate `787068f3621bb51b81c5f25e3a5f2138109be6ee`.
- Dependency: Package A/A9 Accepted.
- Product default: unchanged legacy renderer/session path.
- Parallel writer lanes: none. E0 and E1 are serial shared foundations.
  Disjoint E2/E8 lanes begin only after E1 is integrated. Later E4/NEON and
  E5/AVX2 lanes require E3's platform-leaf interface; a non-CPU E8/E9 lane is
  allowed alongside them only with an exact non-overlap proof. E6/E7 remain
  sequential because both own engine/workspace decisions.
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

## E1 active contract

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
