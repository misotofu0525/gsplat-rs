# Progress Log: Render and Paging Architecture Convergence

## 2026-07-18 — Phase 0 Audit and Baseline

- **Status:** complete
- **Starting state:** clean `refactor/packed-atlas-d-reset` at `3150b7b`.
- Actions completed:
  - Created the thread goal before other task actions.
  - Read all canonical project docs required by `AGENTS.md`.
  - Read the active Phase D-F task/verification plan and relevant design,
    findings, and progress sections.
  - Recovered the original Direct-default/oversized-paged intent and the known
    bounded-prototype limitations.
  - Captured branch history, main diff shape, renderer module sizes, public
    item map, and crate dependency tree.
  - Ran current and detached-main Direct checks, conformance, benchmark, and
    exact PNG comparison; then removed the temporary worktree.
  - Froze S1-S5 with independent rollback and verification boundaries.
- Files created:
  - `docs/plans/active/2026-07-18-render-paging-architecture-convergence/task_plan.md`
  - `docs/plans/active/2026-07-18-render-paging-architecture-convergence/findings.md`
  - `docs/plans/active/2026-07-18-render-paging-architecture-convergence/progress.md`

## Test Results

| Test | Expected | Actual | Status |
|------|----------|--------|--------|
| Initial git state | clean reset branch at `3150b7b` | matched | pass |
| Current workspace check | `cargo check --workspace` | passed | pass |
| Current renderer lib suite | Direct/Packed/Paged and safety gates pass | 93 passed, 1 ignored | pass |
| Current SortedAlpha conformance | required GPU path passes | 1 passed on Metal | pass |
| Current minimal Direct benchmark | direct pipeline, non-zero draw, no budget miss | p95 GPU-complete 2.6283 ms; 0 misses | pass |
| Fresh main workspace check | `cargo check --workspace` | passed in detached worktree | pass |
| Fresh main SortedAlpha conformance | required GPU path passes | 1 passed on Metal | pass |
| Main/current minimal PNG | same Direct count and image | identical count and SHA-256 | pass |
| Main/current minimal benchmark | no obvious Direct regression | mean GPU-complete 1.9003/1.8189 ms | observation |
| S1 workspace check | Surface move compiles across consumers | passed | pass |
| S1 renderer lib suite | all path/safety/image gates remain green | 93 passed, 1 ignored | pass |
| S1 SortedAlpha conformance | required GPU path remains correct | 1 passed on Metal | pass |
| S1 FFI smoke | public C consumer renders | `drawn=2 visible=2` | pass |
| S1 hygiene | fmt, strict renderer clippy, diff check | passed | pass |

## 2026-07-18 — S1 Surface Ownership Split

- **Status:** complete
- Moved Surface presentation and resource planning from `lib.rs` into one
  sibling module with the same crate-root `SurfacePresenter` export.
- Unified initial and switched geometry resource construction.
- Code-size result: one new file, root -875 lines, combined renderer Rust -6
  lines.
- Next: S2 shared paged active-set owner; no policy/source behavior changes yet.

## Error Log

| Error | Attempt | Resolution |
|-------|---------|------------|
| S1 first compile: unresolved root import for moved transaction helper | 1 | Removed the stale import and reran the focused check. |
| S1 first format check: import order drift | 1 | Applied the exact rustfmt ordering. |
| S1 post-dedup format check: one call wrapping difference | 1 | Applied rustfmt's requested compaction and reran hygiene. |
| S1 completion-record patch context mismatch | 1 | Re-read the active files and applied smaller exact patches. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Phase 2 / S2 shared paged active-set runtime |
| Where am I going? | S2-S5 incremental implementation and fresh verification |
| What's the goal? | Restore a clear Direct/default vs oversized/Paged architecture while reducing proven waste |
| What have I learned? | See `findings.md` |
| What have I done? | Loaded canonical context and captured initial code/branch facts |
