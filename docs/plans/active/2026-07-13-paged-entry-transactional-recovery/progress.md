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

- **Status:** in_progress
- Added an additive Web/WASM constructor-time selector while retaining the old
  Direct constructor unchanged.
- Moved packed/paged selection ahead of scene path-specific derivation and
  presenter creation; removed the post-create selector from ESM construction.
- Added additive Android/UIKit C constructors, passed selection through JNI and
  both local wrappers, and retained the legacy Direct constructors unchanged.
- Removed initial post-create switching from Android and iOS examples.
- Next action: remove the full-scene packed staging allocation from fixed-slot
  paged atlas initialization.

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

## Error Log

| Error | Attempt | Resolution |
|---|---:|---|
| None | 0 | — |
