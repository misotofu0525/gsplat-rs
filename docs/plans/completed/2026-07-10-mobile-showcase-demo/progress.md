# Progress Log

## Session: 2026-07-10

### Phase 1: Audit Android and iOS

- **Status:** complete
- **Started:** 2026-07-10
- Actions taken:
  - Confirmed the previous Web showcase changes remain in the worktree.
  - Loaded project and platform verification constraints.
  - Created a separate task-scoped mobile plan bundle.
  - Audited the Kotlin/UIKit sources and both platform build scripts.
  - Confirmed the existing gesture, import, diagnostic, and benchmark paths that must remain intact.
- Files created/modified:
  - `docs/plans/active/2026-07-10-mobile-showcase-demo/task_plan.md`
  - `docs/plans/active/2026-07-10-mobile-showcase-demo/findings.md`
  - `docs/plans/active/2026-07-10-mobile-showcase-demo/progress.md`

### Phase 2: Design and asset route

- **Status:** complete
- **Started:** 2026-07-10
- Actions taken:
  - Selected a native editorial overlay inspired by the Web showcase: quiet brand line, two-line hero, compact scene telemetry, and an explicit Studio diagnostics panel.
  - Selected a shared `showcase.ply` build asset, preferring Kitsune and falling back to Flowers.

### Phase 3: Android implementation

- **Status:** complete
- Actions taken:
  - Replaced the default diagnostic-first UI with the editorial hero, compact live scene telemetry, capsule import action, and toggleable Studio panel.
  - Added build-time `showcase.ply` packaging with Kitsune-first selection and Flowers/runtime-minimal fallbacks.
  - Preserved gestures, runtime PLY import, render loop, benchmark extras, and full diagnostics.

### Phase 4: iOS implementation

- **Status:** complete
- Actions taken:
  - Added the matching native UIKit showcase overlay and blur-backed Studio diagnostics panel.
  - Switched simulator/device bundles to `showcase.ply` with Kitsune-first dataset selection.
  - Preserved document import, gestures, render queue, benchmark args, and C ABI status output.
  - Built, installed, and launched the app on an iPhone 17 Pro simulator through the repository build script plus XcodeBuildMCP.

### Phase 5: Verification and docs

- **Status:** complete
- Actions taken:
  - Captured a simulator screenshot confirming the Kitsune first frame, `279199 SPLATS`, live frame telemetry, and both interactive controls.
  - Updated example, binding, architecture, verification, and root README documentation.
  - Confirmed the final iOS runtime log reports `dataset=kitune1.ply` and repeated `drawn=279199 visible=279199` frames without failures.
  - Confirmed Android APK contents include the 65,892,441-byte `assets/showcase.ply`, `showcase.name`, and `arm64-v8a/libgsplat_jni.so`.
  - Re-ran workspace, formatting, shell syntax, plist, and diff hygiene checks.

## Test Results

| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| Initial repository state | `git status --short --branch` | Prior Web showcase changes preserved | Expected modified/untracked showcase files present | Pass |
| Android APK build | `bash bindings/android/scripts/build-sample-apk.sh` | Kotlin/JNI/resources compile and Kitsune packages | Build successful; `assets/showcase.ply` present | Pass |
| Android JNI smoke | `bash bindings/android/scripts/run-jni-smoke.sh` | JNI bridge remains functional | `jni smoke ok` | Pass |
| Android AAR build | `bash bindings/android/scripts/build-aar.sh` | Local library package still builds | Release AAR built successfully | Pass |
| iOS simulator app build | `bash bindings/apple/scripts/build-ios-sim-app.sh` | Swift/Rust app compiles with Kitsune | Build successful; `showcase.ply` bundled | Pass |
| iOS simulator launch | XcodeBuildMCP install + launch | App starts and renders the default scene | Kitsune renders with 279,199 splats | Pass |
| Workspace check | `cargo check --workspace` | Workspace compiles | Completed successfully | Pass |
| Rust formatting | `cargo fmt --check` | No formatting drift | Completed successfully | Pass |
| File hygiene | `git diff --check` + script/plist syntax checks | No whitespace or syntax errors | Completed successfully | Pass |
| Android device launch | `adb devices -l` | Connected device available | No Android devices attached | Not run |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Complete. |
| Where am I going? | Archive this plan and deliver the verified mobile showcase update. |
| What's the goal? | Align Android and iOS with the Kitsune showcase while preserving validation behavior. |
| What have I learned? | Both examples can be redesigned without touching their renderer, gesture, import, or benchmark contracts. |
| What have I done? | Implemented both native showcases, ran the iOS app, built the Android APK, and updated docs. |
