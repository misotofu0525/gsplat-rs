# Native Render Core Refactor Progress

> Program plan: [task_plan.md](task_plan.md)
> Architecture: [architecture.md](architecture.md)
> Verification: [benchmark_protocol.md](benchmark_protocol.md)

## Program status

- Plan bundle: committed at `c478252246733f6dc209686091caf183e1ef7f06`.
- Implementation: Package A started; A1 guardrails are Accepted.
- Current work package: A — responsibility extraction.
- Active package task: none.
- Last completed task: A1 — Accepted.
- Next eligible task: A2 — extract `api.rs` and strategy-free
  `data/{layout,view}.rs` types.
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

Then record the code commit (if any), evidence paths, commands, result and next
eligible task. Do not rewrite the architecture in this ledger.

## Current task

No implementation task is active.

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

- State: Accepted
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
- Commit: this follow-up commit; exact SHA is reported in the handoff
- Next eligible task: A1 — add source-size and dependency ratchet

### A1 — Add source-size and dependency ratchet

- State: Accepted
- Final state: Accept
- Started: 2026-07-23 18:59 CST
- Ended: 2026-07-23 19:14 CST
- Baseline commit: `9df0d6cb2e7e7df7ddc95e85d16c416e4e91d4b0`
- Worktree before task: clean detached checkout at the baseline commit; branch
  `codex/native-render-a1-ratchet` was created before the first edit
- Hypothesis: a small standard-library-only checker can freeze A0 physical LOC,
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
  - PASS grandfather entries may shrink and may not exceed their checked
    baseline; owner closeout while still over target is detectable
  - PASS production Rust `<800` target / `1,200` hard ceiling, concrete plan
    `<600` target / `1,000` hard ceiling, render `lib.rs <200`, future renderer
    orchestrator `<800`, and WGSL `<350` are encoded
  - PASS the top-level orchestration `<150` rule is explicitly disabled because
    A0 has no reliable single-function boundary; creation of
    `renderer/mod.rs` or E1 closeout forces exact function activation
  - PASS exceptions require baseline LOC, maximum temporary delta, reason and
    removal task; terminal removal-task closeout expires them
  - PASS future `gpu/` imports of renderer/plans/policy/evidence/platform hosts,
    plan submit/present/poll/map, host plan/cache/adaptive ownership, runtime
    `Vec<Box<dyn ...Pass>>` and public `RenderPlan` are covered by positive and
    negative fixtures
  - PASS plan environment reads and pipeline creation are checked only inside
    exact configured per-frame function bodies; constructors/preparation are
    legal, and a new plan file without an explicit frame/preparation boundary
    fails closed
  - PASS source discovery enumerates configured include globs directly and does
    not walk repository-wide `target/`, `node_modules/` or datasets
  - PASS checker self-tests and checker against the real A0 tree
  - PASS `cargo check --workspace --locked`
  - PASS final whitespace and scope checks
- Observations:
  - real-tree checker reports `38 production Rust, 25 WGSL, 24 grandfathered`
  - warm real-tree timing after the directed-glob/lexer fix was `0.17 s` and
    `0.16 s` in consecutive `/usr/bin/time -p` runs
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
    `grandfather.review_due`; A9 must therefore register a dedicated plan and
    ledger, reassign an explicitly scheduled owner, or remove the entry after
    meeting its exit condition
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
- Result commit: this A1 commit; exact SHA is reported in the handoff
- Next eligible task: A2 — extract `api.rs` and strategy-free
  `data/{layout,view}.rs` types
- A2 input:
  - start from this A1 commit and keep the checker passing before and after the
    extraction
  - keep new production Rust below the declared target, shrink legacy owners,
    and lower a checked grandfather baseline in the same task when it shrinks
  - A2 does not need to activate future `plans/` or `renderer/mod.rs`; if it
    introduces either path, it must configure the exact boundary rather than
    suppressing the activation failure
  - moving embedded tests alone is not evidence that an A2 responsibility moved

## Decision ledger

| Task | State | Commit | Evidence/report | Decision summary |
| --- | --- | --- | --- | --- |
| Plan bundle | Accepted | `c478252` | this directory | complete route, architecture, protocol and finite-task ledger written |
| A0 | Accepted | `2aef9f0` | [a0-baseline.md](a0-baseline.md) | dedicated integration branch from `c478252`; no merge/rebase/cherry-pick |
| A0 evidence audit | Accepted | this follow-up | [a0-baseline.md](a0-baseline.md) | evidence classes and artifact identity limits tightened; integration decision unchanged |
| A1 | Accepted | this commit | `tests/architecture/` and this ledger | physical-LOC/dependency ratchet passes on fixtures and the A0 tree; A2 is eligible |

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
