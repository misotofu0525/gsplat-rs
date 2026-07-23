# Native Render Core Refactor Progress

> Program plan: [task_plan.md](task_plan.md)
> Architecture: [architecture.md](architecture.md)
> Verification: [benchmark_protocol.md](benchmark_protocol.md)

## Program status

- Plan bundle: committed at `c478252246733f6dc209686091caf183e1ef7f06`.
- Implementation: not started under this plan.
- Current work package: A — responsibility extraction.
- Active package task: none.
- Last completed task: A0 — Accepted.
- Next eligible task: A1 — add source-size and dependency ratchet.
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
- Commit: this A0 documentation commit; exact SHA is reported in the task
  handoff because a commit cannot contain its own object ID
- Next eligible task: A1 — add source-size and dependency ratchet

## Decision ledger

| Task | State | Commit | Evidence/report | Decision summary |
| --- | --- | --- | --- | --- |
| Plan bundle | Accepted | `c478252` | this directory | complete route, architecture, protocol and finite-task ledger written |
| A0 | Accepted | this commit | [a0-baseline.md](a0-baseline.md) | dedicated integration branch from `c478252`; no merge/rebase/cherry-pick |

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
- A065 complete Truck currently favors CPU PostSort over GPU PostSort, while
  Preproject materially reduces the GPU execution-plan cost.
- Pinned PlayCanvas on A065 is WebGPU and has a lower observed queue-terminal
  interval under a different precision/work contract.
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
