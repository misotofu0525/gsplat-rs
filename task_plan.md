# Task Plan: v0.1.0 Multi-Subagent Remaining Work Closure

## Goal
Close the remaining gaps called out after baseline delivery: WGSL render path, non-placeholder GPU sort backend, format/pack tooling, long-stability gate tooling, and mobile demo container-level execution evidence.

## Current Phase
Phase R7 complete

## Phases

### Phase R7: Full 3DGS Geometry Closure (High Priority)
- [x] Re-baseline current simplified splat geometry path and identify exact gap
- [x] Reference `hyperlogic/splatapult` splat shader math for covariance projection
- [x] Implement covariance-driven anisotropic screen-space ellipse geometry in renderer
- [x] Update WGSL to consume ellipse axes and apply Gaussian falloff consistently
- [x] Add focused tests/verification for the new geometry path
- [x] Update docs/planning logs to reflect closure status
- **Status:** complete

### Phase R1: Remaining Gap Re-baseline
- [x] Re-open remaining items from user feedback
- [x] Re-scope subagent tracks for outstanding deliverables
- [x] Record updated execution strategy in findings/progress
- **Status:** complete

### Phase R2: SA-03/05 Render WGSL Closure
- [x] Add WGSL shader files and wire into wgpu pipeline creation
- [x] Replace raster placeholder with real render pass path when GPU is available
- [x] Preserve CPU fallback for headless/no-adapter environments
- **Status:** complete

### Phase R3: SA-04 GPU Sort Closure
- [x] Implement non-placeholder GPU sort backend
- [x] Keep CPU fallback behavior deterministic
- [x] Add backend-level validation tests
- **Status:** complete

### Phase R4: SA-02/Format Tooling Closure
- [x] Implement `gsplat-format` pack/unpack primitives
- [x] Implement `gsplat-pack` CLI to convert PLY -> packed format
- [x] Add roundtrip tests and docs
- **Status:** complete

### Phase R5: SA-07/08 Gate Hardening
- [x] Add long-stability runner/script and release checklist docs
- [x] Update workflows/docs to include new checks
- [x] Re-run full verification suite
- **Status:** complete

### Phase R6: SA-06 Mobile Container Evidence
- [x] Elevate demos from host smoke to container-level runnable projects/scripts
- [x] Validate what can run in current environment
- [x] Document any environment-only blockers with exact commands
- **Status:** complete

### Phase 1: Requirements and Discovery (Completed Baseline)
- [x] Read user request and required skill workflow
- [x] Read `docs/v0.1.0-multi-subagent-plan.md`
- [x] Inspect current workspace layout and crate status
- [x] Capture findings in `findings.md`
- **Status:** complete

### Phase 2: Gate Plan and Subagent Orchestration
- [x] Define subagent tracks mapped to directories and gates
- [x] Build implementation checklist for G0-G4
- [x] Initialize planning files and progress logs
- [x] Freeze API/contracts to reduce cross-crate churn
- **Status:** complete

### Phase 3: Implementation (Parallel Subagent Tracks)
- [x] SA-01 Core: public types, config, error code, frame stats polish
- [x] SA-02 IO: required PLY field parsing + explicit error mapping
- [x] SA-03/05 Render: preprocess + key generation + stable placeholder raster path
- [x] SA-04 Sort: backend abstraction with CPU fallback validation
- [x] SA-06 FFI: frozen C ABI stubs for lifecycle + stats retrieval
- [x] SA-07 QA: conformance/perf smoke scaffolding and gate scripts
- [x] SA-08 Docs: ADR + release contract notes
- **Status:** complete

### Phase 4: Testing and Verification
- [x] `cargo check --workspace`
- [x] `cargo test --workspace`
- [x] Validate conformance scaffold command path
- [x] Validate perf smoke command path
- **Status:** complete

### Phase 5: Gate Summary and Delivery
- [x] Summarize G0-G4 status with evidence
- [x] List incomplete gates and blockers (if any)
- [x] Provide next execution batch
- **Status:** complete

## Key Questions
1. What is the minimal real WGSL path that is executable in this repo without forcing surface integration?
2. Which GPU sort implementation is practical now while keeping crate complexity manageable?
3. How much mobile “container-level” evidence can be produced without full Android SDK/device setup?

## Decisions Made
| Decision | Rationale |
|----------|-----------|
| Use planning-with-files workflow and persistent markdown logs | Task is multi-phase, multi-directory, and exceeds context-only management |
| Execute by gate order (G0 -> G4) while parallelizing independent crate edits | Matches user plan and minimizes interface breakage |
| Prioritize SortedAlpha path for quality contract | Explicit global rule in provided plan |
| Implement a deterministic CPU fallback path before any GPU-specific sort backend | Ensures cross-platform baseline and testability |
| Freeze C ABI now with error-code return contract | Needed for mobile wrapper integration even before full platform demos |
| Treat host-level Swift/JNI smoke as G3 baseline evidence | Provides reproducible platform-adapter validation without requiring full mobile packaging in current environment |
| Restart planning phases for remaining-task closure instead of treating baseline as final | User explicitly asked to finish unfinished parts |
| Close remaining render/sort gaps with offscreen WGSL and compute-based GPU sort | Delivers real GPU paths while keeping integration scope manageable for current repo stage |
| Use local Gradle distribution for Android container build | Avoids host toolchain drift and failed global package installs |

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
| `git rev-parse --abbrev-ref HEAD` failed because repository has no `HEAD` commit yet | 1 | Switched to `git status --short` and direct filesystem inspection for state tracking |
| Parallel file creation/write race for `crates/gsplat-ffi-c/include/gsplat.h` | 1 | Re-ran sequentially with `mkdir -p` before `cat > file` |
| Swift smoke compile failed due wrong context pointer type and malformed string interpolation | 1 | Switched context pointer to `OpaquePointer?` and simplified formatted output |
| WGPU API mismatch compilation errors with v28 descriptors | 1 | Updated descriptors (`experimental_features`, `immediate_size`, `multiview_mask`) and poll API usage |
| Homebrew gradle installation blocked by tap conflict | 1 | Replaced with script-driven project-local Gradle bootstrap |
| `bench-runner` positional parsing caused dataset path to be overwritten by iterations value | 1 | Added explicit parser state to separate dataset and iteration positional args |
| Policy rejected `rm -rf` while preparing reference repo checkout | 1 | Used timestamped clone path without destructive cleanup |
