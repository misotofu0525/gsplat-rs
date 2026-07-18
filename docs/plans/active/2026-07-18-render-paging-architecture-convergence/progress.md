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
| S2 workspace check | shared owner compiles across consumers | passed | pass |
| S2 renderer lib suite | parity, safety, Surface local gate | 93 passed, 1 ignored | pass |
| S2 SortedAlpha conformance | global quality path unchanged | 1 passed on Metal | pass |
| S2 hygiene | fmt, strict renderer clippy, diff check | passed | pass |
| S3 renderer lib suite | policy plus all existing path/safety gates pass | 95 passed, 1 ignored | pass |
| S3 SortedAlpha conformance | global quality path unchanged | 1 passed on Metal | pass |
| S3 FFI smoke | stable C consumer remains Direct and non-zero | `drawn=2 visible=2` | pass |
| S3 Web check | additive canvas API compiles for wasm32 | passed; existing cfg-only warnings | pass |
| S3 public docs | new Rust API links and safety docs are valid | `-D warnings` passed | pass |
| S3 hygiene | workspace, fmt, strict renderer clippy, diff check | passed | pass |
| S4 payload equivalence | source output matches existing shared encoding and LOD rules | passed | pass |
| S4 renderer lib suite | decoded-payload path preserves all parity/safety gates | 96 passed, 1 ignored | pass |
| S4 SortedAlpha conformance | global quality path unchanged | 1 passed on Metal | pass |
| S4 Surface/paging safety | local Surface, stale/cancel/generation/nonresident gates | passed in full suite | pass |
| S4 FFI smoke | stable C consumer remains non-zero | `drawn=2 visible=2` | pass |
| S4 Web check | source/payload boundary compiles for wasm32 | passed; existing cfg-only warnings | pass |
| S4 hygiene | workspace, fmt, strict renderer clippy, diff check | passed | pass |
| S5 full workspace tests | all crates and doctests pass | passed; renderer 96 passed, 1 retained ignored | historical pass; final rerun pending |
| S5 required SortedAlpha | GPU-required conformance remains correct | 1 passed on Apple M4 Pro / Metal | pass |
| S5 native Direct PNG | final counts/image equal baseline | `visible=2`, `drawn=2`, identical SHA-256 | pass |
| S5 native Direct benchmark | Direct path, non-zero draw, no miss | mean 1.8581 ms; p95 3.1114 ms; 0 misses | observation |
| S5 Android real model | physical-device Direct/Paged runs completed before final commit | Direct 14.969 ms; Paged 36.587 ms | historical observation; provenance gap |
| S5 iOS available route | simulator Direct Surface run completed before final commit | 18.992 ms; 279199 visible/drawn | historical simulator observation; provenance gap |
| S5 Web | earlier wasm build/check/tests and browser Surface | 7 tests; `wasm_sorted_index_direct`, 3 visible/drawn | historical pass; ignored output predates final commit |
| S5 hygiene | fmt, workspace check, strict clippy, rustdoc, diff check | passed | pass |

## 2026-07-18 — S1 Surface Ownership Split

- **Status:** complete
- Moved Surface presentation and resource planning from `lib.rs` into one
  sibling module with the same crate-root `SurfacePresenter` export.
- Unified initial and switched geometry resource construction.
- Code-size result: one new file, root -875 lines, combined renderer Rust -6
  lines.
- Next: S2 shared paged active-set owner; no policy/source behavior changes yet.

## 2026-07-18 — S2 Shared Paged Active Set

- **Status:** complete
- Replaced Surface/offscreen atlas-residency setup with one internal
  `PagedActiveSet` owner while keeping public paging types intact.
- Fresh verification: workspace check, 93 renderer tests, required Metal
  conformance, strict clippy, fmt, and diff hygiene all passed.
- Code-size result: one 105-line file, net +26 renderer Rust lines from S1;
  terminal net deletion remains open.
- Next: S3 Direct-first automatic policy seam.

## 2026-07-18 — S3 Direct-First Automatic Policy

- **Status:** complete
- Added an explicit request policy whose default remains Direct; automatic
  selection is opt-in through new Surface constructors.
- Automatic construction waits for compatible-adapter limits, uses the
  existing structured Direct preflight, and selects Paged only for
  `ActiveAtlasRequired`.
- Failed automatic preparation restores the renderer's prior geometry path.
  Existing constructors, explicit diagnostic paths, and the C ABI are
  unchanged.
- Fresh verification: workspace check; 95 renderer tests plus one retained
  ignored oracle; required Metal conformance; FFI smoke; wasm32 check; strict
  renderer clippy; rustdoc with warnings denied; formatting and diff hygiene.
- Next: S4 local page-source/payload boundary.

## 2026-07-18 — S4 Local Page-Source Boundary

- **Status:** complete
- Added one private `page_source.rs` module. `LocalScenePageSource` explicitly
  borrows the full in-memory scene and page metadata, performs page extraction,
  shared-range packing, and attribute-LOD reduction, and returns a decoded
  payload with stable source indices.
- `PagedAtlasGpu` now has an internal decoded-payload upload path. Existing
  public scene/page upload methods remain unchanged compatibility wrappers.
- The shared active set schedules against source metadata and sends one
  transient payload at a time to fixed GPU slots. No source/decoded cache was
  added; the current adapter remains unbounded at source residency and is not
  described as streaming.
- Fresh verification: workspace check; payload equivalence; 96 renderer tests
  plus one retained ignored oracle; required Metal conformance; local Surface,
  parity and safety gates; FFI smoke; wasm32 check; strict clippy; fmt/diff.
- Code-size result: one 166-line file and +201 renderer Rust lines from S3.
  Final source is currently 11,296 lines versus the 10,796-line start, so S5
  must recover at least 501 lines to satisfy terminal net deletion.
- Next: S5 proven cleanup and final regression.

## 2026-07-18 — S5 Cleanup, Documentation, and Platform Regression

- **Status:** module slice complete; overall acceptance invalidated by later audit
- Unified renderer timing/stat/raster logic, Surface constructor setup, and
  repeated test fixtures. The four touched test modules retain identical test
  counts before/after, and all original image/safety thresholds remain.
- Reconciled project context, architecture, roadmap, and golden principles with
  the implemented Direct/default, opt-in oversized/Paged, and explicit
  diagnostic Packed boundary.
- The slice produced workspace, Metal, PNG, FFI, Web, Android, and simulator
  observations, but Web/mobile binaries and raw logs were not reliably bound to
  final `eb12e68`; they are historical evidence only and must be repeated in F.
- Code-size result: aggregate source is 10,792 versus 10,796, but the corrected
  production-only measure is 7,821 versus 7,621 (+200), while tests/fixtures
  fell from 3,175 to 2,971 (-204). Production cleanup remains open.
- No platform-specific fix was required, no public baseline API/C ABI changed,
  and no push was performed.

## 2026-07-18 — Independent Audit Reopens Overall Acceptance

- **Status:** corrective A complete in this docs-only boundary; overall
  architecture convergence `in_progress`; B is the sole current blocker.
- Confirmed a clean `refactor/packed-atlas-d-reset@eb12e68`; preserved all five
  verified S1-S5 commits and did not switch to the old expanded branch.
- Recorded the corrected production/test split, synchronous full-source page
  contract, 1,011-line Surface responsibility hotspot, absent automatic
  production consumer, effective-device-limits defect, missing payload bounds,
  and final-evidence provenance gaps.
- Froze A-F in strict sequence. No code, API, C ABI, generated artifact, or
  platform binary changes belong to A.
- Next: B starts with a failing pure logic regression for adapter limits above
  the requested downlevel storage limit, followed by the smallest effective
  device-limits selection fix and focused verification.

## Error Log

| Error | Attempt | Resolution |
|-------|---------|------------|
| S1 first compile: unresolved root import for moved transaction helper | 1 | Removed the stale import and reran the focused check. |
| S1 first format check: import order drift | 1 | Applied the exact rustfmt ordering. |
| S1 post-dedup format check: one call wrapping difference | 1 | Applied rustfmt's requested compaction and reran hygiene. |
| S1 completion-record patch context mismatch | 1 | Re-read the active files and applied smaller exact patches. |
| S2 first test compile: old paged field names in fixtures | 1 | Routed assertions through the new shared owner and reran test compilation. |
| S2 first format check: standard ordering/wrapping drift | 1 | Applied rustfmt's requested layout before verification. |
| S2 completion-record patch context mismatch | 1 | Re-read the active files and applied smaller exact patches. |
| S3 first format check: one auto-selection call wrap | 1 | Applied the standard layout and continued with policy proof. |
| S3 focused-test command rejected a second test filter | 1 | Ran the two tests as separate Cargo commands; both passed. |
| S4 audit search included a guessed `paged_atlas_gpu.rs` file | 1 | Inspected the actual `paged_gpu.rs` found by repository search. |
| S4 first format check found import/wrapping drift | 1 | Applied rustfmt before the focused compile and tests. |
| S4 FFI discovery search included a missing `scripts/` root | 1 | Followed the canonical handbook command under `tests/ffi/`. |
| S5 duplicate-search used a backreference unsupported by default `rg` regex | 1 | Re-ran with `rg --pcre2` and used the successful result only. |
| First iOS simulator run did not preserve app stdout | 1 | Used the rebuilt installed app with `simctl --console` to capture the benchmark. |
| Browser documentation exceeded one response | 1 | Read the full 40,171-character contract in seven bounded chunks before navigation. |
| Aggregate `10,796 -> 10,792` was used as an overall cleanup gate | 1 | Independent audit split production from terminal test modules and found production `7,621 -> 7,821`; overall completion was withdrawn. |
| Platform observations were called final without commit-tagged raw provenance | 1 | Reclassified them as historical and added final-HEAD manifests/logs plus baseline/final over-slot comparison to F. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Corrective A docs-only reset; overall in progress |
| Where am I going? | B Auto limits, C payload validation, D production cleanup, E consumer boundary, F final proof |
| What's the goal? | Restore a clear Direct/default vs oversized/Paged architecture while reducing proven waste |
| What have I learned? | See `findings.md` |
| What have I done? | Preserved S1-S5, accepted the audit, withdrew the broad completion claim, and froze corrected gates |
