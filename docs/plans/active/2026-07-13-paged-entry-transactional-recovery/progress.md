# Progress: Paged Entry and Transactional Recovery

## 2026-07-13

### Phase 0

- **Status:** complete
- Created the recovery bundle from the post-D–F audit.
- Frozen the single current blocker: paged must be selected before scene-derived
  Direct CPU data and presenter/GPU resource creation.
- Preserved scope: constructor ordering, transactional switching, over-limit
  proof, compact evidence; no production source streaming or telemetry work.

### Phase 1

- **Status:** complete
- Added an additive Web/WASM constructor-time selector while retaining the old
  Direct constructor unchanged.
- Moved packed/paged selection ahead of scene path-specific derivation and
  presenter creation; removed the post-create selector from ESM construction.
- Added additive Android/UIKit C constructors, passed selection through JNI and
  both local wrappers, and retained the legacy Direct constructors unchanged.
- Removed initial post-create switching from Android and iOS examples.
- Replaced full-scene packed staging during paged atlas initialization with
  allocation-free metadata scans plus fixed-slot placeholder allocation.

### Phase 2

- **Status:** complete
- Changed presenter switching to prepare-then-commit without clearing the old
  path on returned errors.
- Added renderer rollback and delayed async/frame-state mutation until commit.
- Added deterministic injected-failure tests for presenter non-commit and
  renderer cache/path restoration.

### Phase 3

- **Status:** complete
- Bound a pure resource plan to the actual Surface constructor before device
  creation and allocation.
- Proved Nandi metadata exceeds Direct limits while four paged slots fit.
- Strengthened the real-GPU local runtime test with `page_count > slots`, fixed
  resident capacity below scene size, and non-zero draw assertions.

### Phase 4

- **Status:** complete
- Added `evidence/paged-entry-v1.json` with source/dataset hashes, exact
  over-limit plan values, terminal commands, runtime receipts, and bounded
  claims.
- Completed workspace, Web/WASM package, FFI/JNI, Android APK/device, Swift,
  iOS simulator build/runtime, and browser WebGPU verification.
- Final claim: constructor-time fixed-slot local paging and returned-error
  transaction safety are proven; source parsing remains full-resident and
  network/10M/long-run/registry work remains deferred.
- Next action: none for this goal.

## Test Results

| Test | Result |
|---|---|
| Baseline `cargo test -p gsplat-render-wgpu paged -- --nocapture` | 9 passed |
| Baseline worktree | clean at `3bc246e` |
| `npm --prefix packages/web test` | 7 passed |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | passed |
| `cargo fmt --check` | passed; existing target-specific dead-code warnings only |
| `cargo test -p gsplat-ffi-c` | 15 passed |
| paged renderer preselection unit test | passed; pages present and Direct CPU caches absent |
| `bash tests/ffi/run-ffi-smoke.sh` | passed; drawn=2, visible=2 |
| `bash bindings/android/scripts/run-jni-smoke.sh` | passed |
| Android sample APK build with minimal fixture | passed |
| `bash bindings/apple/scripts/build-ios-sim-app.sh` | passed |
| packed metadata-only equivalence unit | passed |
| `cargo test -p gsplat-render-wgpu paged_gpu -- --nocapture` | 3 passed |
| `cargo clippy -p gsplat-render-wgpu --all-targets -- -D warnings` | passed |
| presenter prepare failure non-commit unit | passed |
| renderer rollback on presenter failure unit | passed |
| `cargo test -p gsplat-render-wgpu -- --nocapture` | 83 passed, 1 existing ignored oracle; conformance passed |
| over-Direct-limit Surface resource plan unit | passed; 53 pages, 4 slots, 262,144 resident capacity |
| real-GPU Surface paged runtime unit | passed; fixed slots and non-zero draw |
| `npm --prefix packages/web test` after constructor planner | 7 passed |
| WASM target check after constructor planner | passed; existing target-specific warnings only |
| `cargo check --workspace` | passed |
| `cargo test --workspace` | passed; renderer 84 passed, 1 existing ignored oracle; conformance passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed after private helper shape fix |
| Web package build + dry-run pack | passed; tarball shasum `9c98be29bb95d06735a41a00cdfcd9dd8072b7c2` |
| Browser WebGPU paged Kitsune smoke | passed; `2528` visible / `2528` drawn, no console errors |
| Android A065 paged constructor smoke | passed; path id `2`, `3` visible / `3` drawn |
| iOS simulator paged Kitsune smoke | passed; `2528` visible / `2528` drawn |

## Error Log

| Error | Attempt | Resolution |
|---|---:|---|
| `cargo clippy --workspace --all-targets -- -D warnings` rejected an 8-argument private UIKit helper | 1 | Grouped the two UIKit target pointers into one tuple; public constructors unchanged. |
| Android device enumeration resolved `/platform-tools/adb` from an inline assignment | 1 | Assigned the SDK-derived executable first; retry found A065 and paged smoke passed. |
