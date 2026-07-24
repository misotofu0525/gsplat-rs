# Native Render Exact Core Progress

> Parent roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

## Machine task-state registry

<!-- gsplat-program-task-states: begin -->
E0 = Accepted
E1 = Accepted
E2 = Accepted
E3 = Accepted
E4 = Rejected
E5 = Deferred
E6 = Accepted
E7 = Active
E8 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = d356409a0245b45b5af3d1ecbfd0892be3c3274e
E7 = E7-cpu-init-calibration
E8 = E8-gpu-post-plan
<!-- gsplat-program-active-lanes: end -->

## Package status

- Package: E — Exact prepared plans and native execution, still shadowed.
- Active tasks: bounded native CPU initialization calibration (E7) and the
  plans-only exact GPU PostSort implementation (E8).
- Last completed batch: E6 direct-packed CPU preprocess and E8a transactional
  GPU runtime admission.
- E1 state: Accepted after root fixed-SHA review and fast-forward integration
  of candidate `0496266cc73de5fc84acb7c393606fa68eb77623`.
- Dependency: Package A/A9 Accepted.
- Product default: unchanged legacy renderer/session path.
- E8 feasibility audit stopped without changes after proving that E1 lacked a
  real device-owned GPU scene/preparation/encoder seam. Accepted E8a now owns
  that transaction and leaves the actual GPU PostSort plan implementable by a
  plans-only writer. E4/NEON remains Rejected and E5/AVX2 remains Deferred;
  E7 calibrates only accepted native CPU choices.
- E4 is Rejected with a clean tree after two bit-exact NEON candidates were
  consistently slower than Scalar in the finite Apple M4 release experiment.
  E5 is Deferred with a clean tree because the reachable physical x86_64
  endpoint lacks AVX2/FMA and Rosetta/cross-compilation cannot qualify native
  performance. Neither outcome changes the scalar production leaves.
- Current safe parallel batch contains E7 and E8. E7 owns only native CPU
  initialization calibration, preprocess/workspace integration and tests. E8
  owns only the concrete GPU PostSort plan and PlanSet registration. Their
  exact files and mutable owners are disjoint. Root alone owns shared
  architecture policy/ledger edits, fixed-SHA review, integration and the
  combined matrix.
  Never create parallel writers for the same mutable owner merely to increase
  concurrency.
- Source-size rule: no fixed LOC quota, split trigger or completion gate. Module
  boundaries follow responsibility, dependency direction, compatibility,
  testability, change locality, navigability and maintenance risk. A mixed
  owner requires finite review only when those semantic properties warrant it;
  physical line count cannot trigger an artificial split.
- Numeric-policy correction: `800` and the mistyped `890` are not design
  requirements and must not appear in writer prompts. The checker has no
  fixed-line target, ceiling or review trigger. Existing legacy-owner snapshots
  retain their physical counts only as non-blocking growth evidence until their
  semantic responsibility exit condition is satisfied.
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

## E3 closeout and E4 / E5 / E8a parallel contract

- E3 final state: Accepted. Writer parent
  `98173cba3b891c21e7901a51c19e2ca8ceabe125`, accepted candidate
  `78347b24cef67561b52e7dcfe7945018fcefdd6e`, root integration
  `c9ddb136eee170d8a222da62a7db6bf21815f1ab`.
- E3 result: one lane-local `CpuOrderEngine`, reusable workspace and persistent
  bounded native worker now serve legacy/offscreen, Surface sync/async and the
  shadow CPU plan. Scalar retains explicit FMA order, inclusive near/far,
  full32 keys and source-ID stability; WASM stays scalar/serial. The AArch64
  and x86_64 leaves remain scalar delegates until E4/E5 qualify them.
- E3 independent fixed-SHA review found no P0/P1/P2. Root workspace tests pass:
  renderer 340 passed with five existing ignored, all workspace crates and
  SortedAlpha conformance green. No public API/C ABI, Adaptive schedule,
  raster, source membership, SH or resolution changed.
- New activation code commit:
  `c9ddb136eee170d8a222da62a7db6bf21815f1ab`.
- Writer model: three user-visible Codex tasks in separate worktrees. No
  collaboration subagent edits production code; subagents are limited to
  bounded read-only audits and fixed-SHA reviews.
- E4 owns only `cpu/preprocess/aarch64.rs`. It may implement and qualify one
  NEON leaf against the scalar oracle on available AArch64 endpoints; it does
  not change dispatch, calibration, stable API or another architecture leaf.
- E5 owns only `cpu/preprocess/x86_64.rs`. It may implement an AVX2/FMA
  candidate and exact tests, but without a real x86 endpoint its performance
  admission is Deferred and the unqualified candidate cannot become the
  default. Endpoint absence is a terminal truthful result, not a loop.
- E8a repair owns `renderer/mod.rs`, `renderer/frame.rs`,
  `renderer/gpu_prepare.rs`, `scene/runtime.rs`, `plans/mod.rs` and
  `resident_gpu.rs`. It must make the
  future plans-only E8 consumer reachable from the renderer execution boundary,
  replace raw `wgpu::Device` equality with renderer-owned identity, and make
  color encoding safe when an encoder is discarded. It still cannot submit,
  poll, map, read back, present, change raster/WGSL or alter product policy.
- Root alone owns this ledger and architecture-policy edits. The three writer
  allowlists have no exact-file or mutable-owner overlap. Each candidate gets a
  fixed-SHA review; root integrates the finite accepted results and then runs
  one combined shared matrix.
- Module size is reviewed by responsibility cohesion, dependency direction,
  compatibility, navigability, test seams, change locality and maintenance
  risk. No physical line count is a hard gate, soft target, split trigger or
  completion condition.

### E4 / E5 finite closeout

- E4 final state: Rejected. Exact parent and final clean tip:
  `17cc0e578bf8af774a68635adafa90fb79e6bbee`; no candidate commit exists.
  Both experimental AArch64 leaves passed bit-for-bit scalar parity over
  empty/1/3/4/5/257, inclusive boundaries, FMA-sensitive values, NaN/inf,
  equal-depth ties, non-zero source base and tail cases. Three interleaved
  release observations over 2,541,226 positions showed both candidates slower
  than Scalar for preprocess and preprocess-plus-sort, so the finite protocol
  stopped and restored the scalar delegate. Renderer tests, strict Clippy,
  Rustdoc, wasm32, architecture checks, format and diff checks passed.
- E5 final state: Deferred. Exact parent and final clean tip:
  `17cc0e578bf8af774a68635adafa90fb79e6bbee`; no candidate commit exists.
  The available Pentium Silver N6005 endpoint exposes neither AVX2 nor FMA;
  the WSL endpoint was unavailable and Rosetta/cross-compilation provide only
  partial semantic/compile evidence. Adding a production guard without a
  qualifying endpoint would publish an unverified path, while a permanently
  disabled implementation would be dead code. Cross-target compile, Rosetta
  CPU-order tests, strict Clippy, wasm32, architecture, format and diff checks
  passed. A future real AVX2/FMA endpoint may reopen E5 as a new finite task.

## E6 direct-packed activation contract

- Dependency state: E3 Accepted, E4 Rejected and E5 Deferred. E6 does not
  depend on E8/E8a and may run concurrently because their mutable owners are
  disjoint.
- Objective: test one exact position-to-packed `(depth_key, source_id)` input
  path feeding the existing CPU radix backend, eliminating avoidable split
  staging without changing radix semantics, stable ties, full membership or
  product selection.
- Exact writer allowlist:
  - `crates/gsplat-sort/src/cpu.rs` only for an entry consuming already packed
    key/ID input; the radix algorithm remains unchanged;
  - `crates/gsplat-render-wgpu/src/cpu/preprocess.rs`;
  - new `crates/gsplat-render-wgpu/src/cpu/preprocess/packed.rs`;
  - `crates/gsplat-render-wgpu/src/cpu/workspace.rs`;
  - `crates/gsplat-render-wgpu/src/cpu_order.rs`.
- Frozen: AArch64/x86 leaves, radix implementation, plans/renderer/scene,
  legacy `lib.rs`, Surface/offscreen product routing, WGSL/raster, public ABI,
  policy and this ledger.
- Exactness: Scalar/Rayon only in this task; the rejected NEON and unqualified
  AVX2 leaves cannot be enabled. Empty/one/tail, inclusive near/far adjacent
  ULPs, NaN/inf, equal-depth source-ID ties, invalid-camera transactions,
  serial/parallel equivalence and workspace reuse must match the E3 oracle
  element-for-element.
- Finite exit: Accepted only if exactness and ownership hold and a finite
  predeclared interleaved M4 release observation shows a clear repeatable
  direction of benefit. Any correctness issue or inconclusive/no-benefit
  result is Rejected and restored to a clean production tree; endpoint/toolchain
  loss may terminate as Deferred. No fixed FPS, percentage or LOC gate, and no
  unbounded tuning.

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
- The first adapter candidate
  `3cb0a697ca60fe90676f2ceb582c3140f88a5432` was rejected rather than
  integrated. It prepared valid GPU resources, but the future plans-only E8
  could not consume its queue/encoder seam; an unsubmitted encoder could poison
  the resolved-color cache; and native `wgpu::Device` equality could collide
  across independent Instances. Compile and GPU tests passing did not override
  these semantic blockers.
- The repaired candidate
  `63eb4bee5979cf35ec6ab526621b61b947c06ac9` was also rejected rather than
  integrated. It fixed discard-safe uncached color encoding and independent
  Instance owner identity, and routed a queue plus caller encoder through the
  renderer and `PlanSet`. However, GPU preparation still published only
  `SceneRuntime` resources while `PlanSet` permanently classified both GPU
  entries as unprepared. A later plans-only E8 therefore still could not
  transactionally publish a concrete GPU plan, immutable eligibility and a new
  plan-set generation together with the resource candidate.
- The accepted finite repair used this exact implementation allowlist:
  - `crates/gsplat-render-wgpu/src/renderer/mod.rs`;
  - `crates/gsplat-render-wgpu/src/renderer/frame.rs`;
  - new `crates/gsplat-render-wgpu/src/renderer/gpu_prepare.rs`;
  - `crates/gsplat-render-wgpu/src/scene/runtime.rs`;
  - `crates/gsplat-render-wgpu/src/plans/mod.rs`;
  - `crates/gsplat-render-wgpu/src/resident_gpu.rs`.
  Root, not the writer, owns any later architecture policy registration.
- Adapter contract: build the complete GPU scene and a forward-compatible
  `PlanSet` admission candidate privately, advance the plan-set generation,
  and publish resources/plan admission/frame identity/owner in one infallible
  transaction only after every fallible step succeeds. The current adapter
  must leave an actual call seam that a later plans-only E8 can extend with its
  concrete `GpuPostSortPlan` without modifying renderer/frame code again. It
  also requires async scoped device preparation, renderer-issued owner
  identity, discard-safe uncached color encoding, capacity/count/SH0--SH3
  agreement, and a queue plus caller-owned encoder without submit, poll, map,
  readback or present. Unsupported preparation preserves the Exact CPU
  fallback and the prior generation/membership.
- The GPU PostSort implementation resumes from the accepted E2+adapter root SHA
  in a fresh worktree. Its original plans-only allowlist must still prove
  complete membership, original SH degree, inclusive visibility, stable
  full32 ordering, `D=V`, exact generation guards and unchanged product routing.

## E6 / E8a closeout and E7 / E8 parallel contract

- E6 final state: Accepted. Writer parent
  `f3d1a64b39aecb5dbe6d2a2986ff49185477af46`, accepted candidate
  `868d5312c59ece0eac3bfc7880b9011cc3108bef`, root integration
  `d28234789f3d55f536fcb0189d8aae53419c08c6`, followed by the native-test
  wasm guard repair `b12536cea55b69083d0cbb8cedd0e6bd5411fcd0`.
- E6 result: native Direct preprocessing now creates packed
  `(depth_key, !source_id)` records in one pass and feeds the unchanged stable
  high-32 radix. Scalar and Rayon preserve inclusive visibility, full32 depth,
  deterministic source-ID ties and transactional authoritative-order
  publication. Paged and WASM retain their prior split paths.
- E6 finite M4 observation used one release binary, deterministic 2,541,226
  positions, identical camera/order hash, per-path warmup and interleaved
  `B,P,P,B,B,P` runs. Packed median preprocess was `3.051917 ms` versus
  `3.182792 ms`; packed preprocess-plus-sort was `9.896375 ms` versus
  `10.259541 ms`. This admits the native packed input path only; it is not a
  frame-terminal or cross-platform performance claim.
- E8a final state: Accepted. Writer parent
  `98173cba3b891c21e7901a51c19e2ca8ceabe125`, accepted cumulative candidate
  `0d89b42523932584a701b789419b5e0041816222`, root integration
  `d356409a0245b45b5af3d1ecbfd0892be3c3274e`.
- E8a result: renderer-owned GPU identity, complete scene-resource staging,
  forward-compatible PlanSet admission, plan-set generation and SceneRuntime
  owner publish atomically after every fallible step succeeds. Failure leaves
  CPU fallback, prior scene, generations and eligibility usable. Queue plus a
  caller-owned encoder reach PlanSet without submit, poll, map, readback or
  present, and discarded encoders cannot publish color/order/project cache
  state.
- Shared root verification after both integrations passed format/diff,
  architecture policy/self-tests, locked workspace check/tests, strict
  all-target Clippy, warning-free Rustdoc, wasm32 Web check, required Metal
  SortedAlpha conformance and C FFI smoke.
- New activation baseline:
  `d356409a0245b45b5af3d1ecbfd0892be3c3274e`.
- Writer model: two user-visible Codex tasks in independent worktrees.
  Collaboration subagents may only perform bounded read-only audits and
  fixed-SHA reviews. Root owns integration and the combined matrix.
- E7 exact write allowlist:
  - `crates/gsplat-render-wgpu/src/cpu/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/cpu/calibration.rs`;
  - `crates/gsplat-render-wgpu/src/cpu/preprocess.rs`;
  - `crates/gsplat-render-wgpu/src/cpu/preprocess/packed.rs`;
  - `crates/gsplat-render-wgpu/src/cpu/workspace.rs`;
  - `crates/gsplat-render-wgpu/src/cpu_order.rs`.
- E7 runs once during each native lane's initialization, compares only accepted
  packed Scalar execution with deduplicated supported chunk counts, then freezes
  the choice in that lane-local engine. WASM remains Scalar/serial. Invalid or
  incomplete calibration selects the existing accepted static fallback. It
  cannot become a per-frame controller, point-count rule, public policy or
  persistent background tuner.
- E8 exact write allowlist:
  - `crates/gsplat-render-wgpu/src/plans/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/plans/gpu_post.rs`;
  - optional new `crates/gsplat-render-wgpu/src/plans/gpu_post/tests.rs`.
- E8 constructs and stages a concrete `GpuPostSortPlan` through the accepted
  E8a hook, makes it eligible only with matching owner/count/SH/generations,
  and encodes existing exact GPU visibility, stable compaction/full32 radix and
  rank projection into canonical `ProjectedWork` with `D=V`. It may not edit
  renderer/scene/resident primitives, WGSL, raster, Surface/offscreen product
  routes, public API/C ABI, controller or evidence policy.
- Integration order is E7 then E8. Because their allowlists and mutable owners
  do not overlap, either may finish first; root reviews fixed candidates
  independently and runs the shared matrix only after accepted candidates are
  composed.
