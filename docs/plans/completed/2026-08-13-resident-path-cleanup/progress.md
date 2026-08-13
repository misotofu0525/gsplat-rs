# Progress Log

## Session: 2026-08-13

### Phase 1: Discover the complete change surface

- **Status:** complete
- **Started:** 2026-08-13
- Actions taken:
  - Confirmed branch is `main` tracking `origin/main`.
  - Confirmed the only pre-existing worktree edit is `handbook/ROADMAP.md`.
  - Recovered relevant historical Direct/Paged evidence from project memory.
  - Loaded the planning and current-doc synchronization workflows.
  - Read `PROJECT_CONTEXT.md`, `ARCHITECTURE.md`, and `GOLDEN_PRINCIPLES.md` and identified stale Packed/Paged topology and policy text.
  - Reviewed the pre-existing ROADMAP diff and classified it as prior authorized project-direction work that requires reconciliation.
  - Built a tracked-source inventory covering renderer modules and every public/platform geometry selector.
  - Inspected `Renderer` ownership and rejected reuse of the failed refactor branch because the relevant commit is inseparable from a much larger architecture expansion.
  - Completed the canonical docs and complete tracked-source symbol inventory.
- Files created/modified:
  - `docs/plans/active/2026-08-13-resident-path-cleanup/task_plan.md`
  - `docs/plans/active/2026-08-13-resident-path-cleanup/findings.md`
  - `docs/plans/active/2026-08-13-resident-path-cleanup/progress.md`

### Phase 2: Remove Packed/Paged and rename resident ownership

- **Status:** complete
- Actions taken:
  - Removed renderer module declarations and public exports for Packed/Paged subsystems.
  - Removed `GeometryPath`, path switching, paged preprocessing, packed color refresh, and packed preflight from the core production path.
  - Renamed resident capacity/resource types from `DirectScene*` to `ResidentScene*` and changed over-capacity remediation to explicit scene reduction.
  - Collapsed offscreen and Surface rendering to a single resident pipeline/resource owner.
  - Deleted nine Packed/Paged source modules and the packed shader.
  - Removed geometry selection from C, JNI, Kotlin, Swift, WASM, Web ESM, desktop, and benchmark APIs.
  - Renamed the GPU-order module/shader and resident Surface shader.
- Files created/modified:
  - `crates/gsplat-render-wgpu/src/lib.rs`

### Phase 3: Synchronize tests, examples, scripts, and current docs

- **Status:** complete
- Actions taken:
  - Removed tests that existed only to compare or exercise retired Packed/Paged paths.
  - Updated benchmark artifact naming to `resident_sorted_indices` and resident preflight fields.
  - Updated current architecture, context, verification, roadmap, binding, package, and example docs.
  - Preserved CHANGELOG and completed-plan Packed/Paged references as historical evidence.
  - Evaluated `AGENTS.md`; routing and hard rules remain accurate, so no edit is needed.

### Phase 4: Verify and repair

- **Status:** complete
- Actions taken:
  - Ran workspace format, compile, lint, unit, conformance, rustdoc, and
    dependency-policy gates.
  - Ran C ABI, JNI, Swift, Web SDK/WASM, benchmark-artifact, dataset, trace,
    Android AAR/sample/unit-test, Apple XCFramework/SPM/Xcode, iOS simulator,
    release benchmark, desktop PNG, and headless Chrome render smoke paths.
  - Removed one obsolete `SceneBuffers` presentation parameter discovered by
    the final lint review.
  - Corrected Web benchmark labeling so console output, manifest data, and
    current documentation all report `resident_sorted_indices`.
  - Confirmed the remaining lowercase `packed` identifiers only describe radix
    key packing or SPZ byte decoding; current product references to Packed/Paged
    are explicitly historical/non-goal text.

### Phase 5: Review and commit on main

- **Status:** complete
- Actions taken:
  - Confirmed the worktree remained on `main` tracking `origin/main`.
  - Reviewed the complete change list, whitespace, stale terminology, and
    ignored build artifacts.
  - Prepared one verified resident-only refactor commit.

## Test Results

| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| Workspace quality | `cargo fmt --all -- --check`; workspace check/clippy/test/doc | no warnings or failures | passed; renderer 46 passed, 1 research oracle ignored | pass |
| Physical GPU conformance | required conformance test | render baseline on available adapter | passed on Apple M4 Pro / Metal | pass |
| Dependency policy | `bash tests/security/run-cargo-deny.sh` | all policy sets pass | advisories, bans, licenses, sources passed | pass |
| C and language bindings | FFI, JNI, Swift smoke scripts | compile/link/load/render | all passed; C drew 2/2 splats | pass |
| Web SDK and browser | JS checks/tests, WASM/build/pack, headless collector | package plus resident render | 7 tests passed; Chrome drew 3/3 splats with resident receipt | pass |
| Benchmark data contracts | artifact, dataset, trace, extractor/collector tests | all validators pass | all passed | pass |
| Release benchmark and desktop | release minimal benchmark; desktop PNG smoke | resident path renders | passed on Apple M4 Pro / Metal | pass |
| Android build surface | AAR, sample APK, sample unit tests | build/tests pass | all passed | pass |
| Apple build surface | XCFramework, package describe, Xcode simulator build/app launch | build/launch pass | all passed | pass |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-08-13 | `GpuRasterizer` patch context mismatch | 1 | Patch was atomic; switching to smaller patches against current text |
| 2026-08-13 | Direct `cargo deny check` unavailable | 1 | Used the canonical repository wrapper, which passed |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Complete |
| Where am I going? | Commit handoff on `main` |
| What's the goal? | A verified resident-only renderer committed on `main` |
| What have I learned? | Direct is stable; Packed/Paged are removable experimental dimensions |
| What have I done? | Completed runtime, binding, test, tooling, current-doc convergence, and verification |
