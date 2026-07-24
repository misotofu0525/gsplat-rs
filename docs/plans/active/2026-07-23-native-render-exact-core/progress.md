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
E7 = Accepted
E8 = Accepted
E9 = Accepted
E10 = Accepted
E11 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: E — Exact prepared plans and native execution, still shadowed.
- Active task: E11 mandatory comparable terminal sampler and sole whole-plan
  controller across the three accepted Exact plans.
- Last completed batch: E10 CPU projection adapter, canonical raster ownership,
  exact frame transaction and one terminal submission/batching experiment.
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

## E7 / E8 closeout and E9 adapter activation

- E7 final state: Accepted. Activation base
  `73548d46f3359eb273a55d76edfe7cefc76a2fe0`, first candidate
  `bac5abf4c5ca9bdf511541cab6a14befb153b518`, corrective candidate and root
  fast-forward `6378cefb53be975294268d950c5c5a13722c677e`.
- E7 result: one native `CpuOrderEngine` now calibrates only accepted packed
  Scalar chunk counts. Empty and small scenes execute serially but remain
  pending because the same renderer can replace its scene; the first valid
  large scene runs the finite private probe transaction and freezes one choice.
  Candidate enumeration and fallback share the same effective OS/Rayon
  parallelism, so fallback never selects an execution outside the candidate
  set. Invalid cameras and probe failures cannot publish an authoritative
  order. WASM remains on its prior scalar/serial path.
- E7 review rejected the first candidate for inconsistent OS/Rayon fallback
  derivation and missing three-chunk coverage. The corrective commit unified
  the capability source, added singleton and failure tests, and covers chunk
  counts 1--4 against the scalar oracle. Final fixed-SHA semantic and scope
  reviews found no P0/P1/P2.
- The finite Apple M4 observation used 500,000 deterministic positions. The
  final environment selected serial, completed one calibration transaction,
  retained order hash `a1ce663566d7ba9f`, and reused the frozen choice. These
  numbers prove finite initialization and exact output only; they are not a
  full-frame, cross-platform or competitor claim. The 500 ms deadline is a
  cooperative check between bounded synchronous probes, not a preemptive wall
  clock guarantee.
- E8 final state: Accepted. Writer candidate
  `778f6e484aa90f0aa1fbc90d6492bf1502af4d73`, activation base
  `73548d46f3359eb273a55d76edfe7cefc76a2fe0`, root integration
  `20b7dbbbcabfb559bf32661d14aed00eee38ff06` after E7.
- E8 result: production GPU admission now publishes a concrete
  `GpuPostSortPlan` only with the matching device-owned scene receipt and
  immutable eligibility. Every execute validates camera, source/capacity/
  resident/addressable counts, SH0--SH3, viewport, owner and scene/contract/
  plan-set generations before encoding the accepted color, exact visibility,
  stable compaction/full32 radix and rank-projection graph. A discarded encoder
  publishes no cache; retry re-encodes. Host numeric V/C/D remain unavailable,
  while sorter-owned indirect arguments carry the exact `D=V` relationship.
- E8 fixed-SHA semantic and scope reviews found no P0/P1/P2. Fresh focused
  execution identified Apple M4/Metal rather than silently skipping; 0/1/257,
  SH0--SH3, invalid-camera transaction, discard/retry, full32 primitives and
  SortedAlpha conformance passed. The plan does not submit, poll, map, read
  back, present, create a second device, change raster/WGSL, or alter a product
  route or public ABI.
- Root combined verification after E7 and E8 integration passed format/diff,
  architecture policy/self-tests, locked workspace check and tests, strict
  all-target Clippy, warning-free Rustdoc, wasm32 Web, C FFI smoke, the focused
  GPU PostSort suite on an identified Apple M4/Metal adapter, and required
  Metal SortedAlpha conformance. Renderer library coverage was 364 passed with
  seven pre-existing ignored research/device tests and no failures.
- E9 preflight result: `needs-adapter`. The existing Preproject graph already
  implements exact projection, stable contributor compaction, full32 radix and
  indirect draw, but one legacy owner also holds target-format raster state.
  The adapter must first separate compute ownership, stage it in the same
  atomic GPU scene transaction as PostSort, and expose an owner-bound
  `SceneRuntime` encode seam. It may return only GPU buffers, count sources and
  currentness receipts; it may not submit, poll, map/read back, present, add a
  controller or change product selection.
- E9 adapter activation baseline:
  `20b7dbbbcabfb559bf32661d14aed00eee38ff06`. The writer may edit only the
  registered Preproject compute/raster split, GPU preparation, scene-runtime
  seam and the minimum PlanSet admission shape. The later plans-only E9 writer
  starts from the accepted adapter SHA and is reviewed separately. E10 remains
  blocked until both complete.

## E9 finite ownership adapter closeout

- Final state: Accepted; plans-only E9 is now active.
- Fixed parent: `6be80c07967eb1acb4fc92124a8d8184736e869c`.
- Accepted candidate: `52f15f65b03fc1feb0722b2b4ef883206c936f05`.
- Root integration commit: `dca240e`.
- Scope stayed within the four registered adapter paths: the Preproject
  compute/raster ownership split, GPU preparation, and the scene-runtime seam.
  No plan admission, product route, controller, public API, C ABI, WGSL or GPU
  algorithm changed.
- The accepted seam atomically stages PostSort and target-independent
  Preproject compute on one existing device owner. It validates owner, full
  scene/contract/plan-set identity, count, SH degree, camera and viewport, then
  exposes borrowed stable IDs, indirect arguments, projected planes, resolved
  color and GPU V/C count sources. It never submits, polls, maps, reads back,
  presents or publishes encoder-dependent cache state.
- Two independent fixed-SHA reviews reported no P0/P1/P2 findings. Root fresh
  verification passed the 0/1/127/128/129/1025 and SH0--SH3 Metal seam test,
  discard/retry, architecture self-tests/real-tree policy, and required Metal
  SortedAlpha conformance. The writer's broader focused matrix also passed.
- `PlanId::GpuPreproject` remains unprepared and ineligible at this boundary.
  The active plans-only writer may consume only this accepted seam and the
  existing plan abstractions; it may not reopen primitive algorithms or legacy
  Surface ownership.

## E9 exact GPU Preproject plan closeout

- Final state: Accepted; E10 is now active.
- Fixed parent: `89365241646fa11bf51a4261ec9c146af0267b4a`.
- Accepted cumulative candidate:
  `88e45af465dc6e63d9b39d2cd200dfa5bd62ce2f`.
- Root integration commits: `cbc807e` and `b13f33d`.
- Scope was exactly `plans/mod.rs`, `plans/gpu_pre.rs` and its focused tests.
  The plan consumes only the accepted SceneRuntime Preproject seam and carries
  GPU V/C count sources plus the exact indirect relation `D=C`; numeric host
  V/C/D remain unavailable instead of using source, capacity or zero.
- GPU PostSort and CPU fallback remain prepared as before. Production GPU
  admission constructs PostSort and Preproject together before one infallible
  commit; Preproject execution revalidates owner, complete count/SH receipt,
  scene/contract/plan-set identity, camera and viewport.
- A fixed-SHA review found one candidate-side policy violation: the first test
  helper selected required GPU execution through environment variables. The
  corrective commit removed those reads; macOS now requires an adapter/device
  and Metal through compile-time target gating, while non-macOS may report an
  honest optional skip.
- Both final fixed-SHA reviews accepted with no P0/P1/P2. Root independently
  observed Apple M4/Metal execution and three focused passes. The integrated
  matrix passed format/diff, architecture policy/self-tests, locked workspace
  check/tests, 369 renderer tests with seven existing ignored research tests,
  strict all-target Clippy, warning-free Rustdoc, wasm32 Web, required Metal
  SortedAlpha conformance and C FFI smoke.
- No submit, poll, map/readback, presentation, controller, public API/ABI,
  product default, WGSL or primitive GPU algorithm changed.

## E10 finite adapter activation

- Preflight result: `needs-adapter`. `PreparedRuntime` has no canonical raster,
  and CPU PostSort currently returns only CPU IDs and host S/V. Before raster
  ownership migrates, the existing device-owned projection graph must accept
  the authoritative CPU order and return owner/generation-bound projected
  planes with direct `D=V` semantics.
- The adapter is not a new renderer or experiment. It may reuse the staged
  Resident/project resources, encode into the caller-owned command encoder and
  expose borrowed projected buffers. It may not create target-format raster
  state, submit, poll, map/read back, present or publish encoder-dependent
  cache state.
- After this finite seam passes fixed-SHA review and image/count/currentness
  oracles, E10 can migrate the accepted four-vertex canonical raster. The one
  submission/batching hypothesis will be declared separately with an explicit
  baseline and finite Accept/Reject/Defer result; it is not invented inside
  the ownership adapter.

## E10 canonical raster and frame-transaction closeout

- Final state: Accepted; E11 is now active.
- CPU PostSort projection adapter integration: `f07b02c`.
- Dormant plan-neutral CanonicalRaster integration: `212e8c6`.
- Exact frame transaction integration: `e4bc368`; corrective fixed tip after
  two independent reviews: `2339c5b`.
- GPU admission now stages scene, all three complete plans and the target-format
  canonical raster before one infallible publication. Each frame encodes one
  selected plan and exactly one canonical raster. CPU PostSort reports host-
  known `D=V`; GPU PostSort retains GPU-owned `D=V`; GPU Preproject retains
  GPU-owned `D=C`; unavailable GPU numeric counts remain unavailable.
- The pending transaction owns its command encoder. Hosts may append capture or
  readback work only to that exact encoder; Renderer alone finishes, submits
  once and then publishes `FrameState`. Every later encode attempt, including a
  failed one, invalidates older pending work before any plan or queue side
  effect. Wrong-owner, stale-base and stale-attempt submissions fail closed.
- Both fixed-tip reviewers accepted with no P0/P1/P2 after the transaction
  correction. Root verification passed 379 renderer tests with seven existing
  ignored research tests, all-workspace check/test/strict Clippy/Rustdoc,
  wasm32, architecture policy/self-tests and required Apple M4/Metal
  SortedAlpha conformance.

## E10 finite submission/batching experiment

- Hypothesis: placing `Plan + CanonicalRaster + capture copy` in one command
  buffer would reduce terminal completion time versus two ordered command
  buffers in the same single `Queue::submit` call. The comparison changed only
  command-buffer organization; scene, SH3, 65,537 points, 640x480 target,
  camera sequence, plan, copy, submission count and completion wait were equal.
- Hard correctness passed first: CPU PostSort, GPU PostSort and GPU Preproject
  produced byte-identical images, equivalent frame/plan/order/count receipts
  and identical published FrameState in both organizations on Apple M4/Metal.
- Protocol: 10 warmup pairs followed by 100 AB/BA-interleaved measured pairs in
  five blocks, using exact `SubmissionIndex` completion. Performance was an
  observation, never an E10 completion gate.
- Result: Rejected for a scoped performance claim. The single-buffer layout won
  only 3/5 blocks; terminal median was 59.536709 ms versus 59.448750 ms and p95
  was 74.515791 ms versus 73.444583 ms. Encode/submit medians were 0.570167 ms
  versus 0.546083 ms. No repeat tuning is authorized by this hypothesis.
- The one-buffer production shape remains for stronger transaction ownership
  and straightforward host append semantics, not because of a claimed speedup.
  The performance rejection does not block E11.
