# Progress Log

## Session: 2026-07-10

### Phase 1: Architecture contract and branch baseline

- **Status:** complete
- **Started:** 2026-07-10
- Actions taken:
  - Created an explicit autonomous goal for the renderer refactor.
  - Loaded the project architecture, verification, platform README, historical mobile benchmark, and shared rendering source context.
  - Confirmed Android/iOS already default to CPU sort plus static direct sorted-index rendering.
  - Defined the shared-session, explicit-pipeline, revision-based cache direction.
  - Renamed the branch from `feat/web-desktop-sorted-index-pipeline` to `refactor/unified-render-pipeline`.
  - Added the implementation contract in `design.md`.
- Files created/modified:
  - `docs/plans/active/2026-07-10-unified-render-pipeline/task_plan.md`
  - `docs/plans/active/2026-07-10-unified-render-pipeline/findings.md`
  - `docs/plans/active/2026-07-10-unified-render-pipeline/progress.md`
  - `docs/plans/active/2026-07-10-unified-render-pipeline/design.md`

### Phase 2: Shared Surface render session

- **Status:** complete
- Actions taken:
  - Added `SurfaceRenderSession` inside `gsplat-render-wgpu`.
  - Added the third explicit Surface path for GPU preprojection.
  - Added changed-camera sort cadence, path-aware resource dirtiness, compact-order upload tracking, and stationary redraw behavior.
  - Added four state-machine unit tests; all 18 renderer library tests pass.
- Files created/modified:
  - `crates/gsplat-render-wgpu/src/lib.rs`
  - `crates/gsplat-render-wgpu/src/surface_session.rs`

### Phase 3: Web and native migration

- **Status:** complete
- Actions taken:
  - Migrated `gsplat-web` synchronous rendering to `SurfaceRenderSession`, removing its duplicated `uploaded_frame` scheduler and sorted-index clone.
  - Moved the native async CPU-sort worker and async CPU-geometry worker behind native-only `SurfaceRenderSession` hooks.
  - Replaced the C FFI Surface handle's renderer/presenter/cache boolean state machine with one shared session.
  - Kept every exported C function intact and mapped the existing static-direct/GPU-preproject booleans to one explicit `SurfaceGeometryPipeline`.
  - Preserved Android/iOS defaults: CPU sort interval 2 and static-direct geometry.
- Files created/modified:
  - `crates/gsplat-web/src/wasm.rs`
  - `crates/gsplat-render-wgpu/src/surface_session.rs`
  - `crates/gsplat-ffi-c/src/lib.rs`

### Phase 4: Desktop/offscreen consolidation and observability

- **Status:** complete
- Actions taken:
  - Replaced the desktop interactive viewer's private WGPU Surface/compute/render stack with `SurfaceRenderSession` and the shared `SurfacePresenter::from_window` constructor.
  - Added explicit `SurfaceGeometryPipeline` and `OffscreenGeometryPipeline` names while retaining the earlier `*RasterPath` Rust type aliases.
  - Removed the offscreen per-frame sorted-index clone and renamed the opt-in flag to the accurate `--gpu-preproject` (the earlier flag remains accepted as a CLI alias).
  - Added `SurfaceSortSchedule` and revision checks that reject stale async CPU geometry after camera or viewport changes.
  - Made the native async sorter lazy, removing its full position-buffer copy from the default synchronous path.
  - Added phase-specific Surface CPU-wall timings and exposed them through the Web package; relabeled Web/desktop benchmark output so compatibility `raster_ms` is not presented as a universal upload metric.

### Phase 5: Current docs sync

- **Status:** complete
- Actions taken:
  - Synchronized project context, architecture, verification, golden principles, desktop, Web, Android, and Apple docs with the implemented shared lifecycle.
  - Kept `AGENTS.md` and `ROADMAP.md` unchanged because canonical paths, load order, and release scope did not change.
  - Archived the completed Web/desktop opt-in plan that this broader refactor supersedes.

### Phase 6: Cross-platform verification and completion

- **Status:** complete
- Actions taken:
  - Passed final formatting, diff hygiene, workspace check/test, strict clippy, rustdoc, dependency policy, and required GPU conformance.
  - Rendered CPU-instance and GPU-preproject desktop PNGs; normalized MAE was `9.87451e-09` and normalized RMSE was `5.15221e-06`.
  - Built final WASM/ESM artifacts, passed six Web package tests and dry-run packaging, then browser-tested a paused direct-index scene for 2,505 frames with `visible=3`, `drawn=3`, and no console errors.
  - Passed C FFI, JNI, Android AAR/APK, Swift host, XCFramework, and iOS simulator app builds.
  - Installed/launched the final simulator app through XcodeBuildMCP; a five-frame Kitsune direct-path benchmark reported `visible=279199` and `drawn=279199`.
  - Confirmed no Android device was attached. XcodeBuildMCP device workflows were unavailable, so new Android/iOS physical-device performance numbers were not claimed.

## Test Results

| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| Session catch-up | planning-with-files recovery script | No stale unsynced context | No output; clean catch-up | ✓ |
| Renderer unit tests | `cargo test -p gsplat-render-wgpu --lib` | Shared scheduler tests and existing renderer tests pass | 18 passed | ✓ |
| Web WASM check | `cargo check -p gsplat-web --target wasm32-unknown-unknown` | Shared session compiles for wasm32 | Passed without warnings | ✓ |
| Workspace check | `cargo check --workspace` | Host crates compile after native migration | Passed | ✓ |
| FFI unit tests | `cargo test -p gsplat-ffi-c --lib` | C ABI adapter behavior remains valid | 14 passed | ✓ |
| Web WASM test compile | `cargo test -p gsplat-web --target wasm32-unknown-unknown --lib --no-run` | WASM test artifact builds after migration | Passed | ✓ |
| Strict Rust validation | fmt + diff check + workspace check/test + strict clippy + rustdoc | All local Rust gates pass | Passed; 91 workspace tests | ✓ |
| GPU conformance | required Metal SortedAlpha test | Hardware-backed reference path passes | 1 passed | ✓ |
| Dependency policy | `tests/security/run-cargo-deny.sh` | Advisory/license/source gates pass | Passed; configured duplicate warnings only | ✓ |
| Desktop A/B | CPU-instance vs GPU-preproject PNG | Visually equivalent within tolerance | normalized MAE `9.87451e-09` | ✓ |
| Web package | WASM build + wrapper build/check/test/pack | Final browser artifacts are consumable | Passed; 6 package tests | ✓ |
| Web stationary direct | Browser, orbit paused | Direct path remains visible after cached-order redraws | 2,505 frames; visible/drawn 3/3; no console errors | ✓ |
| Android packaging | JNI smoke + AAR + APK | Shared native session links in Android artifacts | Passed | ✓ |
| Apple packaging/runtime | Swift smoke + XCFramework + simulator app + launch | Shared native session links and renders on simulator | Passed; Kitsune drawn/visible 279199/279199 | ✓ |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-07-10 | `gsplat-web` non-exhaustive match after adding `SortedIndexGpuPreproject` | 1 | Migrate Web rendering to `SurfaceRenderSession` instead of adding another wrapper-owned branch. |
| 2026-07-10 | Stale-term `rg` audit treated a leading `--` pattern as an option | 1 | Re-ran with `rg -- <pattern>`; no current-doc stale terms remained after one plan wording fix. |
| 2026-07-10 | Strict clippy found one collapsible nested `if` and one needless borrow | 1 | Applied the suggested let-chain and passed the sorted-index slice directly; strict clippy is rerun in final verification. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Complete; the plan is ready to archive. |
| Where am I going? | Device-only performance reruns can follow when physical Android/iOS hardware is available. |
| What's the goal? | One shared CPU-sort Surface render lifecycle across Web/native with explicit geometry pipelines and verified behavior. |
| What have I learned? | Mobile already uses static-direct; duplicated wrapper scheduling caused the Web regression. |
| What have I done? | Unified Web, desktop interactive, Android, and iOS Surface scheduling; aligned offscreen configuration/metrics; synchronized docs; and passed all locally available verification. |
