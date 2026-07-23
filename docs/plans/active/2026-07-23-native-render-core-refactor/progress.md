# Native Render Core Refactor Progress

> Program plan: [task_plan.md](task_plan.md)
> Architecture: [architecture.md](architecture.md)
> Verification: [benchmark_protocol.md](benchmark_protocol.md)

## Program status

- Plan bundle: written locally on 2026-07-23.
- Implementation: not started under this plan.
- Current work package: A — responsibility extraction.
- Active package task: none.
- Next eligible task: A0 — freeze integration baseline and evidence inventory.
- Current checked-out branch when the plan was written:
  `codex/full-quality-native-rendering`.
- Current checked-out commit when the plan was written:
  `5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186`.
- `main` / `origin/main` observed at plan creation:
  `28f77d041d70fe3a11713590591ac57a122e1599`.
- Worktree was clean before this plan bundle was added.

The branch relationship is intentionally not resolved by the documentation
task. A0 must choose and record the implementation baseline before A1 or any
source extraction begins.

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

Use this template when A0 starts:

```markdown
### A0 — Freeze integration baseline and evidence inventory

- State: Active
- Started: YYYY-MM-DD HH:MM TZ
- Baseline commit: `<sha>`
- Worktree before task: clean / described changes
- Hypothesis: one sentence
- Dependencies: none
- Allowed scope:
  - ...
- Forbidden scope:
  - production renderer changes
  - new performance optimization
- Hard gates:
  - ...
- Observations:
  - ...
- Required endpoints for claim:
  - ...
- Performance correction used: no
- Known correctness issues: none / list
- Commands:
  - pending
- Evidence:
  - pending
- Decision: pending
- Decision reason: pending
- Commit: pending
- Next eligible task: pending
```

## Decision ledger

| Task | State | Commit | Evidence/report | Decision summary |
| --- | --- | --- | --- | --- |
| Plan bundle | Accepted | uncommitted at creation | this directory | complete route, architecture, protocol and finite-task ledger written |
| A0 | Not started | — | — | freeze implementation baseline before code work |

## Baseline evidence inherited, not rerun by default

A0 should validate identities and links, then reuse these completed records:

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
