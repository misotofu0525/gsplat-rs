# Native Render Core Refactor Progress

> Program plan: [task_plan.md](task_plan.md)
> Architecture: [architecture.md](architecture.md)
> Verification: [benchmark_protocol.md](benchmark_protocol.md)

## Machine task-state registry

The architecture checker reads only the delimited records below, never prose
headings or the decision table. Future E/M/B/S/Q package ledgers use the static
active/completed paths declared in the source-architecture policy and contain
their own single block in the same format.

<!-- gsplat-program-task-states: begin -->
A0 = Accepted
A1 = Accept
A2 = Active
<!-- gsplat-program-task-states: end -->

## Program status

- Plan bundle: committed at `c478252246733f6dc209686091caf183e1ef7f06`.
- Implementation: Package A started; A1 guardrails and the A2a data leaf are complete.
- Current work package: A — responsibility extraction.
- Active package task: A2 — remains Active after the completed A2a subtask.
- Last completed subtask: A2a — strategy-free data/layout/view leaf extraction.
- Next eligible subtask: A2b API/legacy-session boundary under root-task direction.
- Integration branch: `codex/native-render-core-refactor`.
- Frozen source implementation closeout:
  `5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186`.
- Frozen plan/A0 parent:
  `c478252246733f6dc209686091caf183e1ef7f06`.
- Current checked-out branch when the plan was written:
  `codex/full-quality-native-rendering`.
- Current checked-out commit when the plan was written:
  `5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186`.
- `main` / `origin/main` observed at plan creation:
  `28f77d041d70fe3a11713590591ac57a122e1599`.
- Worktree was clean before this plan bundle was added.

The branch relationship is resolved in [a0-baseline.md](a0-baseline.md). A0
created a dedicated integration branch from the complete local full-quality
tip. No merge into `main`, rebase, cherry-pick, push or production change was
performed.

## One-active-task rule

Only one row may have state `Active`. A task cannot start until this file names:

- task ID;
- one hypothesis;
- dependency status;
- baseline commit;
- allowed files/scope;
- forbidden scope;
- hard gates;
- performance observations;
- required endpoints for the intended claim;
- maximum one performance/measurement correction after the first runnable
  implementation;
- known correctness issues (must be zero for Accept).

When the task ends, replace `Active` with exactly one of:

- `Accepted`;
- `Rejected`;
- `Deferred`.

The same closeout change must add exactly one normalized record to that
package ledger's machine task-state block. Narrative state text and the
decision table are human-readable mirrors, not checker inputs.

Then record the code commit (if any), evidence paths, commands, result and next
eligible task. Do not rewrite the architecture in this ledger.

## Current task

### A2a — Extract strategy-free data/layout/view leaves

- Parent task state: A2 Active; A2a completion does not close A2.
- Subtask state: Complete; A2 remains Active
- Started: 2026-07-23 20:18 CST
- Ended: 2026-07-23 20:33 CST
- Baseline commit: `304ab0ee115e7cc1bb939922ac531376b90d3796`
- Worktree before task: clean detached checkout at the exact baseline; branch
  `codex/native-render-a2a-data` was created before the first edit
- Hypothesis: the legacy owner can shed its strategy-free kernel ABI and narrow
  data views without changing render behavior, public paths, asynchronous copy
  timing, shader math, FFI or lifecycle ownership.
- Dependencies: A1 Accepted and integrated at the stated baseline
- Allowed scope:
  - new `crates/gsplat-render-wgpu/src/data/{mod.rs,layout.rs,view.rs}`
  - mechanical `mod`, import and root re-export changes in render-wgpu
  - `lib.rs`-resident `GpuInstance`, `GpuSurfaceSourceElem` and
    `GpuSurfaceRenderParams`
  - the single-definition strategy-free `GpuSortPair` ABI if its move remains
    confined to `direct_gpu_order.rs`
  - private `CameraCovarianceTerms`, `ShColorLayout<'a>`, a real Direct upload
    `SplatSetView<'a>` parameter bundle and the existing native async worker's
    `OwnedCpuOrderInput`
  - this ledger and only the A1 grandfather baselines for legacy files that
    physically shrink in this extraction
- Forbidden scope:
  - `api.rs`, `SurfaceFrameOutput`, public signature changes or new public API
  - Resident types/constants, `resident_scene.rs`, `resident_gpu.rs`, their
    ownership/preflight, or either Resident grandfather baseline
  - shaders, render/sort/projection/raster math, precision, membership, camera,
    resolution, C ABI, JNI, Swift, Web API or platform lifecycle
  - `Renderer`, `SceneRuntime`, `PlanSet`, controller, evidence owner,
    `FrameSnapshot`, `InstanceBuildParams` or `PreparedRendererGeometryPath`
  - activating `plans/` or `renderer/mod.rs`, changing product defaults, GPU
    conformance/device collection, merging the integration branch or pushing
- Symbol and dependency inventory before production edits:
  - `GpuInstance` is a 48-byte public `repr(C)`/`Pod` instance record defined
    in `lib.rs`; all in-tree consumers are the legacy CPU reference path. Its
    existing crate-root public path must remain a root re-export.
  - `GpuSurfaceSourceElem` is the 64-byte Direct source upload record defined in
    `lib.rs`; `make_surface_source_elems` packs it and Direct GPU-order tests
    construct it through the parent module. Moving it to a sibling leaf
    requires crate-only field visibility, not public exposure.
  - `GpuSurfaceRenderParams` is the 112-byte shared Direct/Resident/projected
    uniform defined in `lib.rs`; `lib.rs`, `direct_gpu_order.rs`,
    `resident_gpu.rs`, `preproject_gpu.rs` and `projected_quads_gpu.rs` consume
    the existing crate-root path. A crate-root re-export preserves those uses.
  - `GpuSortPair` is one 8-byte `repr(C)`/`Pod` key-ID record defined only in
    `direct_gpu_order.rs`; moving this definition and importing it back is a
    single-file mechanical A5-owner edit with no shader or algorithm change.
  - five private `ScanParams` definitions and three differently named indirect
    draw records span `direct_gpu_order.rs`, `external_prefix_radix.rs`,
    `projected_quads_gpu.rs`, `preproject_gpu.rs`, `tiled_gpu.rs` and
    `tiled_resident_gpu.rs`. Even where byte layouts match, unifying them crosses
    several A5-owned modules, so A2a defers that work to A5.
  - `CameraCovarianceTerms` is a six-f32 private value used by the legacy owner
    and a Preproject CPU oracle through the crate-root path; `ShColorLayout` is
    a borrow-only SH slice/stride view used only during current color packing.
  - current Direct source packing passes `SceneBuffers`, covariance terms and
    alpha slices separately. `SplatSetView` will replace that real packing
    bundle for the duration of the call only; it will not be stored.
  - native async ordering currently clones the Resident position `Arc` or makes
    the Direct position copy exactly once in `SurfaceAsyncSorter::new`, then
    copies `Camera` per request. `OwnedCpuOrderInput` must wrap that worker
    boundary without moving either copy point and without cloning an `Arc` or
    scene per frame.
- Hard gates:
  - architecture checker and its self-test pass; lowered shrink baselines land
    in the same commit
  - GPU ABI retains `repr(C)`, `Pod`/`Zeroable`, exact size/alignment and field
    offset assertions; `Vec3f` is not made an ABI/Pod type
  - `GpuInstance` remains available at the existing public crate-root path and
    all other visibility remains no wider than before
  - async Direct copy occurs once at sorter construction; Resident uses the
    existing `Arc::clone`; no retained borrowed view or per-frame scene clone
  - format/check/workspace tests/clippy/rustdoc and affected wasm/FFI entrypoints
    from `handbook/VERIFICATION.md` pass
  - final diff contains only the declared extraction/ledger/policy scope
- Performance observations: none planned; A2a is behavior-only extraction
- Required endpoints for claim: none; GPU conformance and real-device evidence
  remain root-task responsibilities and will be reported as not run
- Performance correction used: no
- Known correctness issues: none
- Result:
  - `data/layout.rs` now owns `GpuInstance`, `GpuSurfaceSourceElem`,
    `GpuSurfaceRenderParams` and the single-definition `GpuSortPair`; every ABI
    retains `repr(C)` plus `Pod`/`Zeroable` and compile-time size, alignment and
    per-field offset assertions
  - `data/view.rs` now owns the existing covariance/SH views, one borrow-only
    `SplatSetView` consumed immediately by Direct source packing, and
    `OwnedCpuOrderInput` at the native async sort worker boundary
  - `GpuInstance` remains documented at `gsplat_render_wgpu::GpuInstance`; all
    other moved symbols remain crate-only
  - Resident ordering still performs `Arc::clone(&scene.positions)` and Direct
    still copies positions once in `SurfaceAsyncSorter::new`; the worker moves
    that same Arc into and back out of each owned input without an Arc or scene
    clone per request
  - duplicate `ScanParams` and indirect draw records remain in place and are
    explicitly deferred to A5 rather than unified across A5-owned modules
- Source-size ratchet:
  - `lib.rs`: 5,680 -> 5,601 physical LOC
  - `direct_gpu_order.rs`: 2,862 -> 2,855 physical LOC
  - `surface_session.rs`: 5,044 -> 5,042 physical LOC
  - new `data/mod.rs`, `layout.rs`, and `view.rs`: 10, 98, and 130 physical LOC
  - all three changed grandfather baselines exactly match those final counts;
    Resident baselines are unchanged
- Verification:
  - PASS `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`
  - PASS `PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py`
  - PASS `cargo fmt --all -- --check`
  - PASS `cargo check --workspace --locked`
  - PASS `cargo test -p gsplat-render-wgpu --lib --locked` (280 passed, 5 ignored)
  - PASS `cargo test --workspace --locked`, including the available non-required
    SortedAlpha conformance invocation and all workspace/doc tests
  - PASS `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - PASS `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`
  - PASS `cargo check -p gsplat-web --target wasm32-unknown-unknown --locked`
  - PASS `bash tests/ffi/run-ffi-smoke.sh` (`drawn=2`, `visible=2`)
  - PASS policy JSON parse, public rustdoc-path inspection, exact LOC audit,
    async Arc/copy-site audit and `git diff --check`
- Environment note: the first workspace check failed with `No space left on
  device (os error 28)`. Only rebuildable Cargo caches were cleared; after the
  disk-space issue was resolved, the exact workspace check and all later gates
  passed.
- Not run by A2a: `GSPLAT_REQUIRE_GPU_CONFORMANCE=1`, formal GPU conformance,
  browser runtime smoke, Android/iOS physical-device runs and performance
  collection; the root task owns those endpoints.
- Root acceptance:
  - independent fixed-SHA review found no P0, P1 or P2 issue
  - PASS `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p
    gsplat-render-wgpu --test conformance_sorted_alpha --locked -- --nocapture`
    with one test executed and no skip
  - PASS exact Direct GPU key/radix complete-element and Resident position
    stride parity tests
  - PASS external crate compile probe for
    `gsplat_render_wgpu::GpuInstance`
  - PASS root reruns of architecture policy/self-tests, declared-scope diff,
    shader/Resident/FFI/binding zero-diff checks and FFI smoke
- Scope audit: no `api.rs`, `SurfaceFrameOutput`, Resident source/baseline,
  shader, FFI source/header, platform wrapper, renderer owner, policy owner or
  product-default change is present.
- Decision: complete the independently verifiable A2a result and leave the
  machine registry at `A2 = Active`; do not claim overall A2 acceptance.
- Result identity: `3758fd614bc09c5f330210cf87a120dd9fd0ccdd`, accepted and
  fast-forwarded unchanged into `codex/native-render-core-refactor`.

### A0 — Freeze integration baseline and evidence inventory

- State: Accepted
- Started: 2026-07-23 18:38 CST
- Ended: 2026-07-23 18:43 CST
- Baseline commit: `c478252246733f6dc209686091caf183e1ef7f06`
- Worktree before task: clean detached checkout at the baseline commit
- Hypothesis: because full-quality is a strict linear descendant of the
  unchanged `main`, a dedicated integration branch can preserve evidence
  identities and isolate Package A without a merge or history rewrite.
- Dependencies: none
- Allowed scope:
  - this progress ledger
  - `a0-baseline.md`
  - read-only Git, evidence and document inspection
- Forbidden scope:
  - production renderer or platform changes
  - other plan contracts, handbook, CI or benchmark-schema changes
  - new performance optimization or device performance collection
- Hard gates:
  - PASS local and live-remote ref identities are explicit
  - PASS ancestry and all seven post-main commit identities are unambiguous
  - PASS all observed worktrees were clean before branching
  - PASS inherited accepted/rejected/deferred evidence is indexed
  - PASS canonical evidence authorities exist at the frozen commit
  - PASS final diff contains only the two A0-owned documentation files
- Observations:
  - `main`, cached `origin/main` and live remote `main` all equal `28f77d0`
  - full-quality is seven commits ahead and zero behind `main`
  - the full-quality ref has no live remote branch
  - no device or performance observation was collected
- Required endpoints for claim:
  - none; A0 makes a repository-topology and document-identity claim
- Performance correction used: no
- Known correctness issues: none
- Commands:
  - `git status --porcelain=v2 --branch --untracked-files=all`
  - `git worktree list --porcelain`
  - `git ls-remote --heads origin main codex/full-quality-native-rendering`
  - `git merge-base` and `git rev-list --left-right --count` for the three refs
  - `git log --reverse --format=... main..codex/full-quality-native-rendering`
  - `git cat-file -t` for the seven named commits
  - `shasum -a 256` for canonical evidence authorities
  - active-plan local Markdown link validation
  - `python3 -m json.tool tests/perf/full-quality-matrix-plan-v1.json`
  - `python3 tests/perf/validate-full-quality-experiment.py
    tests/perf/full-quality-matrix-plan-v1.json --allow-incomplete`
  - `git diff --check`
- Evidence:
  - [A0 baseline and evidence inventory](a0-baseline.md)
- Decision: use `codex/native-render-core-refactor`, created from `c478252`;
  do not merge to `main`, rebase or cherry-pick before A1
- Decision reason: the history is already linear; the dedicated branch keeps
  `main` unchanged, preserves artifact-linked commit identities and provides a
  reversible package boundary.
- Commit: `2aef9f0e209c36fc24c706c284e02a181d275b8c`
- Next eligible task: A1 — add source-size and dependency ratchet

### A0 evidence-boundary follow-up

- Outcome: Accepted
- Started: 2026-07-23 18:52 CST
- Ended: 2026-07-23 18:57 CST
- Baseline: `2aef9f0e209c36fc24c706c284e02a181d275b8c`
- Scope: documentation-only correction to the A0 evidence classification and
  artifact identity ledger
- Hard gates:
  - PASS semantic/correctness, directional performance and capacity-only are
    distinct inherited classes
  - PASS dirty A065/PlayCanvas, clean descriptive Preproject and capacity-only
    Garden/Bicycle boundaries are explicit
  - PASS the full-quality matrix is recorded as schema-valid but 0/339 complete
  - PASS ignored artifact locators are workspace-relative and not claimed as
    worktree contents
  - PASS A1 cannot combine cross-commit performance; final qualification belongs
    to Package Q
- Device/performance work: none
- Decision: A0 remains Accepted with narrower evidence claims; the integration
  baseline and A1 eligibility are unchanged
- Commit: `9df0d6cb2e7e7df7ddc95e85d16c416e4e91d4b0`
- Next eligible task: A1 — add source-size and dependency ratchet

### A1 — Add source-size and dependency ratchet

- Final state: Accept
- Started: 2026-07-23 18:59 CST
- Ended: 2026-07-23 19:14 CST
- Baseline commit: `9df0d6cb2e7e7df7ddc95e85d16c416e4e91d4b0`
- Worktree before task: clean detached checkout at the baseline commit; branch
  `codex/native-render-a1-ratchet` was created before the first edit
- Hypothesis: a deterministic standard-library-only checker can freeze A0 physical LOC,
  prevent new giant source files and enforce future render-core dependency
  directions without changing production behavior or scanning legacy owners as
  if the target module tree already existed.
- Dependencies: A0 Accepted
- Allowed scope:
  - one executable checker, policy, fixture matrix and self-test under
    `tests/architecture/`
  - this A1 ledger entry
- Forbidden scope:
  - renderer, shader, API, FFI or platform-consumer changes
  - benchmark schema, CI or handbook changes
  - responsibility movement, device/browser runs or performance collection
- Hard gates:
  - PASS physical LOC counts comments, blank lines and embedded tests
  - PASS all current target breaches are checked exactly: 21 production Rust
    files plus 3 WGSL files, each with A0 LOC, owner task and exit condition
  - PASS every grandfather entry retains immutable A0 LOC plus a checked
    ratchet baseline; shrink without lowering that baseline in the same change
    fails, growth above it fails, and owner closeout while over target is
    detectable
  - PASS production Rust `<800` target / `1,200` hard ceiling, concrete plan
    `<600` target / `1,000` hard ceiling, render `lib.rs <200`, future renderer
    orchestrator `<800`, and WGSL `<350` are encoded
  - PASS the top-level orchestration `<150` rule is explicitly disabled because
    A0 has no reliable single-function boundary; creation of
    `renderer.rs` / `renderer/mod.rs` or E1 closeout forces exact function
    activation
  - PASS exceptions require baseline LOC, maximum temporary delta, reason and
    removal task; terminal removal-task closeout expires them
  - PASS future `gpu.rs` / `gpu/` imports of
    renderer/plans/policy/evidence/platform hosts, plan submit/present/poll/map,
    host plan/cache/adaptive ownership, runtime `Vec<Box<dyn ...Pass>>` and
    public `RenderPlan` are covered by positive and negative fixtures
  - PASS future GPU modules reject aliases that hide `crate/self/super` module
    roots; ordinary item aliases remain legal and grouped imports resolve before
    direction checks
  - PASS any plan file with a per-frame function forbids environment reads and
    pipeline creation across the entire file; only an exclusively
    `preparation_only_files` entry may contain them, and an unclassified plan
    file fails closed
  - PASS function boundaries accept array-return semicolons plus
    generic/lifetime signatures instead of using a first-semicolon heuristic
  - PASS `surface.rs` / `surface/` and `offscreen.rs` / `offscreen/` cover named
    and tuple ownership of PlanId/Controller/FrameState plus explicit renderer
    generation fields/types without banning presentation generations
  - PASS the static A/E/M/B/S/Q task catalog aggregates one active/completed
    ledger per opened package; A is always required, E follows A9, M follows
    E13, and B/S/Q each follow M8; duplicate package ledgers, multiple Active
    tasks, duplicate/conflicting tasks, unknown task/state and wrong-package
    records fail closed
  - PASS every policy task reference is cataloged, including both activation
    tasks; E1 terminal state requires a real per-frame plan boundary
  - PASS task state is read only from one explicit machine block per ledger;
    `Accept/Reject/Defer` and `Accepted/Rejected/Deferred` normalize to terminal
    states without scanning narrative prose
  - PASS source discovery enumerates configured source-root globs directly and
    does not walk repository-wide build output; binding-root build products use
    explicit path exclusions without hiding legitimate production modules such
    as `crates/*/src/build/`
  - PASS the checker itself is 1,192 physical lines and self-ratcheted below
    1,200 after deleting the incomplete helper call graph
  - PASS checker self-tests and checker against the real A0 tree
  - PASS `cargo check --workspace --locked`
  - PASS final whitespace and scope checks
- Observations:
  - real-tree checker reports `38 production Rust, 25 WGSL, 24 grandfathered`
  - warm real-tree timing after the directed-glob/lexer fix was `0.17 s` and
    `0.17 s` in consecutive `/usr/bin/time -p` correction runs
  - generic Rust and plan targets are reported notices until their hard
    ceilings; specialized render `lib.rs`, renderer and WGSL targets are errors
  - no renderer timing, FPS, device, browser or competitor observation was
    collected
- Required endpoints for claim: none; A1 makes a deterministic repository
  policy and fixture-test claim
- Performance correction used: no; no performance hypothesis exists in A1
- Known correctness issues: none
- Commands:
  - `PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py`
  - `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`
  - `/usr/bin/time -p env PYTHONDONTWRITEBYTECODE=1
    tests/architecture/check_source_architecture.py`
  - `python3 -m json.tool` for the policy and fixture JSON
  - `cargo check --workspace --locked`
  - `git diff --check`
- Evidence:
  - `tests/architecture/source_architecture_policy.json`
  - `tests/architecture/fixtures/cases.json`
  - `tests/architecture/test_source_architecture.py`
- External owner tracking:
  - `IO-PLY-1` and `IO-SPZ-1` are deliberately outside the current native-core
    work-package map; each entry has `review_task: A9`
  - once A9 reaches any terminal closeout, an unchanged entry fails with
    `grandfather.review_due`; A9 must therefore add that IO task and a static
    active/completed ledger pair to `program_task_state`, reassign an already
    cataloged owner, or remove the entry after meeting its exit condition
- Claim boundary:
  - dependency checks are deterministic lexical architecture checks, not a
    complete Rust type resolver; comments and literals are removed before
    matching, target directories and exact frame bodies are configured, and
    missing future boundaries fail closed
  - A1 moves no production responsibility and changes no render behavior
- Decision: Accept
- Decision reason: every declared A1 guardrail is executable on fixtures and
  the real A0 tree, all required local gates pass, and the diff remains inside
  the checker/test/config/ledger boundary.
- Result identity: `9df0d6c..codex/native-render-a1-ratchet`; resolve the
  branch tip SHA when integrating the correction
- Next eligible task: A2 — extract `api.rs` and strategy-free
  `data/{layout,view}.rs` types
- A2 input:
  - start from accepted A1 commit
    `f6180844bbaf910b74ff5ecfe81c9b9588c88561` and keep the checker passing
    before and after the extraction
  - keep new production Rust below the declared target, shrink legacy owners,
    and lower a checked grandfather baseline in the same task when it shrinks
  - A2 does not need to activate future `plans/` or `renderer/mod.rs`; if it
    introduces either path, it must configure the exact boundary rather than
    suppressing the activation failure
  - moving embedded tests alone is not evidence that an A2 responsibility moved

### Reviewer correction after A1

- Outcome: Accepted
- Baseline: `b87c17fd901012ea5ca583e25af125902d100036`
- Scope: checker, static policy, adversarial fixtures/self-tests and this ledger
  only; no production, shader, API, FFI, platform, benchmark-schema, CI or
  handbook change
- Result identity: `9df0d6c..codex/native-render-a1-ratchet`; the integration
  handoff must record the resolved branch-tip SHA because a commit cannot
  truthfully contain its own object ID
- Verification:
  - PASS 39-case fixture matrix
  - PASS real-tree checker: `38 production Rust, 25 WGSL, 24 grandfathered`
  - PASS two real-tree timing observations at `0.17 s`
  - PASS `cargo check --workspace --locked`
  - PASS JSON, Python syntax and whitespace checks
- Decision: keep A1 Accepted after the reviewer-blocking ratchet, dependency,
  function-boundary and cross-package expiry cases pass locally
- Next eligible task: A2, from the resolved correction tip

### Final acceptance correction after A1

- Outcome: Accepted
- Baseline: `d85312fcac16576131f716eb78c1e5dc3e9017b4`
- Scope: the same five A1-owned checker/policy/fixture/self-test/ledger files;
  no production, shader, API, FFI, platform, benchmark-schema, CI or handbook
  change
- Simplification result:
  - deleted the incomplete plan helper call graph and made per-frame plan files
    whole-file pipeline/environment-free
  - replaced alias expansion with a conservative ban on GPU root-module aliases
  - retained only bounded signature parsing for exact configured function LOC
    and existence checks
  - reduced the checker from 1,318 to 1,194 physical lines and added a `<1,200`
    self-ratchet
- Registry result: static package lifecycle, one-Active enforcement, complete
  policy task-reference validation and consumed E1 plan activation are covered
  by negative fixtures
- Verification:
  - PASS 52-case fixture matrix
  - PASS real-tree checker: `38 production Rust, 25 WGSL, 24 grandfathered`
  - PASS `cargo check --workspace --locked`
  - PASS JSON, Python execution/syntax, diff/show whitespace and bytecode checks
- Result identity: `d85312f..codex/native-render-a1-ratchet`; resolve the final
  branch-tip SHA during integration
- Decision: keep A1 Accepted; the final rules are simpler, fail closed at
  package/file boundaries and have no remaining reviewer-blocking parser path
- Next eligible task: A2, from the resolved final correction tip

### Root acceptance correction after A1

- Outcome: Accepted
- Baseline: `958587121370e89a077f32f8dafcb5b7f05faf87`
- Scope: the same five A1-owned checker/policy/fixture/self-test/ledger files;
  the root coordinator took over final acceptance and changed no production,
  shader, API, FFI, platform or benchmark behavior
- Closed review gaps:
  - binding-root build products are excluded by exact paths without hiding a
    legitimate production `src/build/` module
  - platform hosts reject domain-prefixed cache generations while continuing
    to allow presentation identity generations
  - valid outer-group and `super::super::{self as root}` aliases cannot hide a
    forbidden GPU dependency
  - a completed package ledger cannot retain an Active task
- Verification:
  - PASS 56-case fixture matrix and all checker self-tests
  - PASS real-tree checker: `38 production Rust, 25 WGSL, 24 grandfathered`
  - PASS checker self-ratchet at 1,192 physical lines
  - PASS `cargo check --workspace --locked`
  - PASS JSON, in-memory Python compilation and whitespace checks
- Result identity: `f6180844bbaf910b74ff5ecfe81c9b9588c88561`
- Integration: fast-forwarded without conflict into
  `codex/native-render-core-refactor`
- Decision: A1 is eligible for fast-forward integration only with this root
  acceptance correction included
- Next eligible task: A2a, the strategy-free data/layout/view extraction

## Decision ledger

| Task | State | Commit | Evidence/report | Decision summary |
| --- | --- | --- | --- | --- |
| Plan bundle | Accepted | `c478252` | this directory | complete route, architecture, protocol and finite-task ledger written |
| A0 | Accepted | `2aef9f0` | [a0-baseline.md](a0-baseline.md) | dedicated integration branch from `c478252`; no merge/rebase/cherry-pick |
| A0 evidence audit | Accepted | `9df0d6c` | [a0-baseline.md](a0-baseline.md) | evidence classes and artifact identity limits tightened; integration decision unchanged |
| A1 | Accepted | `f6180844bbaf910b74ff5ecfe81c9b9588c88561` | `tests/architecture/` and this ledger | root-accepted physical-LOC/dependency ratchet passes 56 fixtures and the A0 tree; A2a is eligible |
| A2a | Complete (A2 Active) | `3758fd614bc09c5f330210cf87a120dd9fd0ccdd` | `data/{layout,view}.rs` and this ledger | root-accepted strategy-free ABI and real data views; API/Resident/shader/lifecycle unchanged |

## Baseline evidence inherited, not rerun by default

A0 should validate identities and links, then reuse these completed records:

The validation and terminal inventory are now recorded in
[a0-baseline.md](a0-baseline.md). The links below remain the detailed evidence
sources.

- [completed task plan](../../completed/2026-07-22-full-quality-native-rendering/task_plan.md)
- [completed final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md)
- [completed design](../../completed/2026-07-22-full-quality-native-rendering/design.md)
- [completed findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md)
- [CPU parallel radix](../../completed/2026-07-22-full-quality-native-rendering/cpu-parallel-radix.md)
- [Preproject architecture](../../completed/2026-07-22-full-quality-native-rendering/phase2-preproject-c-architecture.md)
- [Android evidence](../../completed/2026-07-22-full-quality-native-rendering/android-surface-evidence.md)

Key inherited facts:

- Exact Resident and full-count SH3 Truck/Garden/Bicycle rendering are already
  established.
- Stable CPU full32 ordering, portable GPU ordering, Exact ProjectedQuads and
  S/V/C/D evidence already exist.
- Existing NEON/AVX2/Rayon code must be consolidated, not marketed as greenfield.
- Preproject/Compact already exists and is image-exact; it needs clean ownership
  as a complete plan.
- The A065 nine-run CPU/GPU/Adaptive result and A065 PlayCanvas result are
  dirty directional observations, not clean final-binary qualification.
- A065 Preproject is clean descriptive evidence at `7cabb6e`, but its pairing
  metadata is null; Garden/Bicycle results at `76a9267` are capacity-only.
- The full-quality matrix is a schema-valid plan with 0 rendered of 339
  expected cells, not a completed unified-binary qualification.
- A1 may inherit semantic or correctness proofs, but performance numbers from
  distinct commits/binaries cannot be stitched together; Package Q owns clean,
  same-protocol, final-binary qualification.
- Fixed-slot Paged is not streaming and is not the Scalable foundation.

## Rejected experiments that require new evidence before reopening

The following are not silently rescheduled under a new name:

- Adreno radix8 without complete order and image qualification;
- the slower portable radix8 local-rank variant;
- early-fragment support variants that regressed full-plan performance;
- random point subsampling as LOD;
- four-slot Paged productization;
- treating 640x360 as product qualification;
- treating iOS simulator timings as device performance;
- using one fixed point-count threshold for CPU/GPU selection.

A future task may reopen one only if it states what material condition changed
and why the prior evidence no longer answers the question.

## Task closeout template

```markdown
### Xnn closeout

- Final state: Accepted / Rejected / Deferred
- Ended: YYYY-MM-DD HH:MM TZ
- Baseline: `<sha>`
- Result commit: `<sha>` / none
- Performance correction used: yes / no
- Known correctness issues at closeout: none
- Hard gates:
  - PASS ...
  - PASS ...
- Observations:
  - metric, endpoint and scope
- Evidence:
  - `path/to/artifact`
- Removed/reverted code:
  - ...
- Claim boundary:
  - ...
- Next eligible task:
  - Xnn
```

## Program closeout checklist

The program roadmap has no single monolithic implementation goal. Each package
opens its own active plan/progress ledger and closes independently. This master
bundle may move to `docs/plans/completed/` after Package A closes and the future
package entrypoints have been created or linked; later packages continue to
reference it.

Any package may close only when:

- no task in that package is Active;
- every started task in that package has Accepted/Rejected/Deferred state;
- no rejected experiment remains enabled in production;
- the worktree is clean;
- source-size/dependency ratchets pass;
- handbook architecture and verification docs match the implemented tree;
- retained benchmark artifacts validate;
- public/FFI/API changes have their required smoke evidence;
- final report distinguishes Exact, Balanced and Scalable claims;
- remote branch/PR/merge status is recorded without implying checks that did
  not run.
