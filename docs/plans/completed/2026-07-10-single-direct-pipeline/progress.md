# Progress: Single Direct Runtime Pipeline

## Session: 2026-07-10

### Phase 1: Baseline and deletion contract

- **Status:** completed
- Actions:
  - Loaded the completed unified-session design and current dirty-worktree scope.
  - Confirmed GitHub CLI availability/authentication and the `origin` repository.
  - Created this task-scoped plan before implementation.
  - Inventoried all surface/offscreen runtime selectors, compatibility wrappers, preprojection resources, and CLI A/B flags.
  - Recorded the production/reference deletion contract.
  - Recovery note: the first findings patch used a stale heading name and failed without changing files; re-read the plan bundle and reapplied against the current headings.
  - Replaced the shared Surface session state machine with CPU-sort + direct-index drawing and retained legacy tuning methods only as compatibility no-ops.
  - First compile checkpoint failed as expected on the two remaining offscreen match arms and non-const `then_some` calls; these are the next migration boundary, not unrelated regressions.

### Phase 2: Direct offscreen implementation

- **Status:** completed
- Actions:
  - Added a reusable resident `DirectSceneResources` owner for sorted IDs, PLY-derived source attributes, SH coefficients, and camera parameters.
  - Reused the same direct WGSL pipeline/resource layout for Surface and native offscreen targets.
  - Replaced native offscreen CPU-instance/GPU-preproject dispatch with direct sorted-index drawing.
  - Removed the compute-preproject shader/resources, CPU Surface-instance builder, async geometry worker, and legacy Surface shaders.
  - Removed desktop and benchmark A/B flags; the tools now report the sole direct pipeline.
  - Preserved the v0.1 C path setters as successful compatibility no-ops.
  - `cargo check --workspace` passes without warnings after the production-path collapse.
  - GPU-required SortedAlpha conformance passes on the direct offscreen renderer, comparing output against the existing CPU reference baseline.

### Phase 3: Runtime pipeline collapse

- **Status:** completed
- Actions:
  - Removed Rust geometry-path enums/selectors rather than retaining a one-value runtime abstraction.
  - Removed Android JNI/option and Apple option surfaces for obsolete geometry tuning; both platforms now enter the shared direct path unconditionally.
  - Kept C ABI and Web compatibility setters, but made their direct-only behavior explicit.

### Phase 4: Reference/test isolation and docs sync

- **Status:** completed
- Actions:
  - Kept CPU-projected `GpuInstance` construction only as reference/conformance support.
  - Synced `PROJECT_CONTEXT`, `ARCHITECTURE`, `VERIFICATION`, renderer/Web/platform READMEs, example controls, benchmark telemetry, and the C header to the single direct pipeline.
  - Verification recovery: the first workspace-test compile found a stale test-only `GpuRasterError` reference after its import was narrowed. Restoring the test-only import resolves the issue without production-code changes.
  - Browser-smoke setup found port 4173 already occupied; a direct HTTP probe confirmed the existing repo-root server is healthy, so the smoke reused it instead of starting a second server.
  - Xcode simulator boot returned "already Booted" as an error; session defaults confirmed the intended iPhone 17 Pro simulator, so installation continued on that already-running device.
  - Built the Android AAR/sample APK, Apple XCFramework/simulator app, and the `GsplatKit` Swift package for a generic iOS Simulator destination.
  - Installed and launched the generated iOS app on the iPhone 17 Pro simulator through XcodeBuildMCP. Its five-frame Kitsune benchmark reported the sole direct pipeline with 279,199/279,199 visible/drawn splats.

### Phase 5: Verification and publication preparation

- **Status:** completed
- Actions:
  - Passed all locally available Rust, GPU, Web, desktop, FFI, Android, Apple packaging, and iOS simulator gates listed below.
  - Reviewed final repository scope and kept the unrelated untracked `bindings/android/build/` directory outside the intended commit.
  - Prepared the verified tree for a single implementation/docs commit followed by branch push and draft PR creation.

## Verification Log

| Check | Result |
|-------|--------|
| `cargo check --workspace` | pass |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | pass |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha` | pass (1 test) |
| `node --check examples/web/src/main.js && npm --prefix packages/web run check && npm --prefix packages/web test` | pass (6 package tests) |
| `npm --prefix packages/web run pack:dry-run` | pass (8 files) |
| `cargo test --workspace` | pass (89 tests) |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | pass |
| `bash tests/security/run-cargo-deny.sh` | pass (configured duplicate warnings only) |
| Desktop PNG smoke | pass (`sorted_index_direct`, 2/2 drawn) |
| Release bench runner | pass (120 frames, 2.1228 ms GPU-complete average) |
| `bash tests/ffi/run-ffi-smoke.sh` | pass (2/2 drawn) |
| `bash bindings/android/scripts/run-jni-smoke.sh` | pass |
| `bash bindings/apple/scripts/run-swift-smoke.sh` | pass (2/2 drawn) |
| Web WASM + SDK build | pass |
| Browser direct-path smoke | pass (`wasm_sorted_index_direct`, 5 samples, stationary 3/3 visible/drawn, no console errors) |
| `bash bindings/android/scripts/build-aar.sh` | pass |
| `bash bindings/android/scripts/build-sample-apk.sh` | pass |
| `bash bindings/apple/scripts/build-xcframework.sh` | pass |
| `bash bindings/apple/scripts/build-ios-sim-app.sh` | pass |
| `swift package describe --type json` | pass |
| `xcodebuild -scheme GsplatKit -destination 'generic/platform=iOS Simulator' build` | pass |
| XcodeBuildMCP iOS simulator install/launch | pass (`sorted_index_direct`, 5 samples, 279199/279199 visible/drawn) |

## Publication Log

| Action | Result |
|--------|--------|
| Commit/push/draft PR | Execute immediately after this verified plan snapshot is archived. |
