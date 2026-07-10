# gsplat-rs Verification

## Purpose

- This file defines the canonical verification paths for the repository.
- Prefer these repo-local commands and scripts over ad-hoc command sequences.

## Fast Feedback

- Smallest useful check:

```bash
cargo check --workspace
```

- Typical use: most Rust changes that do not alter platform integration scripts or long-running perf behavior
- Expected runtime: short

## Core Rust Validation

```bash
cargo test --workspace
GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha
```

- Run this when changing shared types, parsing, render logic, or CLI behavior.
- The workspace test may skip pixel conformance when no native adapter exists.
  Set `GSPLAT_REQUIRE_GPU_CONFORMANCE=1` on a GPU-backed runner to make adapter
  absence a failure; CI and release run this requirement on macOS/Metal.
- Linux CI installs Mesa Vulkan/Lavapipe so GPU-required offscreen and C ABI
  paths have a deterministic software adapter; it is compatibility evidence,
  while the macOS jobs provide the required hardware-backed Metal evidence.

## Code Hygiene and Docs

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
node --check examples/web/src/main.js
npm --prefix packages/web run check
npm --prefix packages/web test
npm --prefix packages/web run pack:dry-run
```

- Run these before opening a pull request that changes Rust code, public docs,
  or the Web example.
- `node --check` is syntax validation only; browser behavior still requires
  the Web Example smoke path below.

## Day-to-Day Verification Set

These are the current day-to-day commands the repo relies on:

```bash
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
bash tests/security/run-cargo-deny.sh
node --check examples/web/src/main.js
npm --prefix packages/web run check
npm --prefix packages/web test
npm --prefix packages/web run pack:dry-run
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10 --max-avg-gpu-complete-ms 250
bash tests/ffi/run-ffi-smoke.sh
bash bindings/android/scripts/run-jni-smoke.sh
bash bindings/apple/scripts/run-swift-smoke.sh
bash bindings/apple/scripts/build-xcframework.sh
```

## Desktop Smoke

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
cargo run -p desktop-example --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```

- Use the PNG path for deterministic local smoke output.
- Use the interactive viewer when changing windowed presentation or camera interaction behavior.

## Web Example Smoke

```bash
node --check examples/web/src/main.js
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

- Open `http://127.0.0.1:4173/examples/web/` in a browser.
- Do not use `file:///.../examples/web/index.html`; the example depends on HTTP
  serving from the repository root so wasm imports and `/tests/...` dataset
  fetches resolve correctly.
- Expected startup state loads `tests/datasets/minimal_ascii.ply` and shows
  non-zero `Visible` and `Drawn` counts plus an overlay with `surface=webgl2
  realtime`, `state=rendering`, `camera=auto`, `dataset=minimal_ascii.ply`,
  and `path=/tests/datasets/minimal_ascii.ply`.
- Use the file picker or the `Flowers` button for larger local `.ply` smoke
  checks. Use `?dataset=flowers` for repeatable automation against
  `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`. Without a
  generated wasm package, this is WebGL2 fallback validation rather than proof
  that the Rust `wgpu` renderer is compiled to WebAssembly.
- For benchmark smoke, open:

```text
http://127.0.0.1:4173/examples/web/?gsplat_benchmark=true&gsplat_benchmark_sync=true&gsplat_benchmark_frames=5&gsplat_benchmark_warmup_frames=1&gsplat_surface_sort_interval=2
```

- Expected benchmark output includes `BENCHMARK_RESULT dataset=minimal_ascii.ply`.
- Optional flower fallback smoke:

```text
http://127.0.0.1:4173/examples/web/?dataset=flowers&gsplat_benchmark=true&gsplat_benchmark_sync=true&gsplat_benchmark_frames=2&gsplat_benchmark_warmup_frames=0&gsplat_surface_sort_interval=2
```

- Expected benchmark output includes `BENCHMARK_RESULT dataset=flowers_1.ply`.

## Web WASM Build

Use this when changing `crates/gsplat-web/` or the browser canvas Surface entry
in `crates/gsplat-render-wgpu/`:

```bash
cargo check -p gsplat-web --target wasm32-unknown-unknown
bash packages/web/scripts/build-wasm.sh
bash packages/web/scripts/build.sh
node --check packages/web/dist/index.js
npm --prefix packages/web run pack:dry-run
```

- `cargo check --workspace` still checks the host-side workspace and the
  non-wasm stub for `gsplat-web`.
- The wasm target must be installed separately with
  `rustup target add wasm32-unknown-unknown`.
- `packages/web/scripts/build-wasm.sh` also requires the `wasm-bindgen` CLI and writes
  generated files to ignored `examples/web/pkg/`.
- `packages/web/scripts/build.sh` writes the local ESM wrapper distribution to
  ignored `packages/web/dist/`.
- This is the proof path for the shared Rust `wgpu` renderer and local Web SDK
  wrapper running in the browser. After the package exists, reload
  `http://127.0.0.1:4173/examples/web/?dataset=flowers`; expected status should
  report `surface=wasm-wgpu`, `renderer=wasm_wgpu_surface` in benchmark output,
  and non-zero `Visible` / `Drawn` counts for `flowers_1.ply`.

## Mobile Builds and Simulator Smoke

```bash
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
bash bindings/apple/scripts/build-ios-device-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/run-ios-device-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/benchmark-ios-device-app.sh
bash bindings/apple/scripts/build-ios-sim.sh
bash bindings/apple/scripts/build-xcframework.sh
bash bindings/apple/scripts/run-ios-sim-smoke.sh
bash bindings/android/scripts/build-aar.sh
bash bindings/android/scripts/build-sample-apk.sh
```

- Run these when changing mobile packaging, simulator run scripts, or build scripts.
- Check `bindings/android/README.md` or `bindings/apple/README.md` for platform
  prerequisites before assuming SDK/NDK/Xcode state.
- iOS device runs require a development provisioning profile whose device list
  includes the target phone. `bindings/apple/scripts/build-ios-device-app.sh`
  can auto-select a matching local development profile and `Apple Development:`
  identity, or you can set `IOS_PROVISIONING_PROFILE`,
  `IOS_CODE_SIGN_IDENTITY`, and `IOS_BUNDLE_ID` explicitly.
- iOS run and benchmark scripts require `IOS_DEVICE_ID=<coredevice-id-or-udid>`.
  Run `xcrun devicectl list devices` to inspect paired device identifiers.
- `bindings/apple/scripts/build-ios-device-app.sh` builds Rust with `release` and Swift
  with `-O` by default so the iPhone path can be compared with Android's
  release-native APK. Use `IOS_RUST_PROFILE=dev` and
  `IOS_SWIFT_OPT_LEVEL=-Onone` only for debugging.
- `bindings/apple/scripts/build-xcframework.sh` builds the local
  `bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework` used by the
  `GsplatKit` Swift package wrapper. It builds both
  `aarch64-apple-ios-sim` and `x86_64-apple-ios` simulator slices by default;
  set `IOS_XCFRAMEWORK_SIM_TARGETS` for a custom simulator slice.
- `bindings/android/scripts/build-sample-apk.sh` builds a debug APK container, but compiles the Rust native library with the Rust `release` profile by default. Set `ANDROID_RUST_PROFILE=dev` only for native debugging.
- `bindings/android/scripts/build-aar.sh` builds the local `gsplat-android` AAR at
  `bindings/android/gsplat-android/build/outputs/aar/gsplat-android-release.aar`.
  It accepts `ANDROID_SDK_ROOT` or `ANDROID_HOME`, packages `arm64-v8a` only in
  this slice, uses Android native API level `24` by default, and is not a Maven
  publishing path.

## Android Surface Smoke

Use this when changing Android Surface rendering, JNI surface glue, or `SurfacePresenter` behavior:

```bash
bash bindings/android/scripts/build-sample-apk.sh
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_SDK_ROOT/platform-tools/adb"
"$ADB" install -r examples/android/app/build/outputs/apk/debug/sample-app-debug.apk
"$ADB" shell am start -n com.gsplat.example/.MainActivity
```

- Expected first frame includes `Kitsune shrine`, `LIVE`, a non-zero splat count,
  and frame time. Open `Studio` and confirm `surface=wgpu realtime`,
  `state=rendering`, and `drawn=<surface_instances>/<visible_instances>`.
- For repeatable perf checks, add the benchmark extras documented in `bindings/android/README.md` and read the `BENCHMARK_RESULT` logcat line.
- The APK packages the selected scene as `assets/showcase.ply`. If `adb install -r`
  reports insufficient storage, uninstall `com.gsplat.example` and reinstall.

## iOS Surface Smoke

Use this when changing iOS realtime rendering, UIKit surface glue, touch
controls, mobile packaging, signing, or `SurfacePresenter` behavior:

```bash
bash bindings/apple/scripts/run-ios-sim-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/run-ios-device-app.sh
```

- Expected first frame includes `Kitsune shrine`, `LIVE`, `279199 SPLATS`, and
  frame time. Open `Studio` and confirm `state=rendering`, `camera=<mode>`,
  `dataset=kitune1.ply`, and `drawn=<surface_instances>/<visible_instances>`.
- The simulator app bundle lives at `target/ios-sim-app/GsplatIOSExample.app`; the
  device app bundle lives at `target/ios-device-app/GsplatIOSExample.app`. Both
  package the selected build-time dataset as `showcase.ply`, preferring Kitsune
  and falling back to Flowers.
- The app uses `Documents/imported_scene.ply` when present, otherwise the
  bundled showcase dataset, otherwise a generated minimal ASCII PLY fallback.
- Touch smoke should include at least one one-finger swipe/orbit check in the
  simulator. Pinch zoom and two-finger pan use the same C ABI camera-control
  functions.
- For repeatable perf checks, add the benchmark args documented in
  `bindings/apple/README.md` after `--`. Use
  `bindings/apple/scripts/benchmark-ios-device-app.sh` on a physical iPhone to print the
  `BENCHMARK_RESULT` line and keep the raw log under
  `target/ios-device-benchmarks/`.

## Release Bar

Before cutting a release, also run:

```bash
RELEASE_VERSION=<major.minor.patch> bash tests/release/check-version.sh
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Current Manual Validation Gaps

- Android true-device launch and benchmark are not implied by
  `bash bindings/android/scripts/build-sample-apk.sh`; run the Android Surface
  Smoke path on a physical device before claiming Android device validation.
- iOS physical-device launch and benchmark are not implied by
  `bash bindings/apple/scripts/build-ios-device-app.sh`; set
  `IOS_DEVICE_ID=<coredevice-id-or-udid>` and run the iOS Surface Smoke or
  benchmark path before claiming iPhone runtime validation.
- Maven, remote binary SwiftPM/XCFramework, and npm publishing are release
  distribution tasks. Local AAR/XCFramework/npm-pack checks prove packaging
  shape, not public distribution readiness.

## Targeted Checks

- If you touch `crates/gsplat-ffi-c/`, run `bash tests/ffi/run-ffi-smoke.sh`.
- If you touch `bindings/android/`, `examples/android/`, or JNI glue, run
  `bash bindings/android/scripts/run-jni-smoke.sh`. If you touch Android packaging or
  `bindings/android/gsplat-android/`, also run
  `bash bindings/android/scripts/build-aar.sh` and `bash bindings/android/scripts/build-sample-apk.sh`;
  for Surface changes, also run the Android Surface smoke above.
- If you touch `bindings/apple/`, `examples/ios/`, or Swift/FFI integration, run
  `bash bindings/apple/scripts/run-swift-smoke.sh`; for `GsplatKit` or iOS packaging
  changes, also run `bash bindings/apple/scripts/build-xcframework.sh` and
  `cd bindings/apple/GsplatKit && swift package describe --type json` plus
  `cd bindings/apple/GsplatKit && xcodebuild -scheme GsplatKit -destination 'generic/platform=iOS Simulator' build`;
  for realtime Surface or touch changes, also run
  `bash bindings/apple/scripts/run-ios-sim-app.sh`; for offscreen simulator smoke
  changes, run `bash bindings/apple/scripts/run-ios-sim-smoke.sh`.
- If you touch PLY import or scene normalization, run `cargo test --workspace` and `cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png`.
- If you touch renderer, sorting, or perf-sensitive code, run `cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10 --max-avg-gpu-complete-ms 250` and consider the long-stability script. The runner reports CPU preprocessing, CPU sort, build/encode/submit, GPU wait, and GPU-complete frame time separately, together with adapter/backend/driver metadata.
- If you touch `examples/web/`, run `node --check examples/web/src/main.js`
  and the Web Example smoke above. If you touch
  `packages/web/`, also run
  `npm --prefix packages/web run check`,
  `npm --prefix packages/web test`,
  `bash packages/web/scripts/build.sh`, and
  `node --check packages/web/dist/index.js`.
- If you touch `crates/gsplat-web/` or browser Surface creation in `crates/gsplat-render-wgpu/`, run `cargo check --workspace` and the Web WASM Build path above.
- For spatial/tile/chunk feasibility checks on a loaded PLY, use:

```bash
cargo run -p bench-runner -- <scene.ply> --analyze-spatial
```

## Structural Checks

- CI entrypoints live in `.github/workflows/ci.yml`, `.github/workflows/perf-smoke.yml`, and `.github/workflows/long-stability.yml`.
- Contributor issue and pull request templates live under `.github/`.
- The lint and docs entrypoints are `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`.
- Dependency advisory, license, duplicate-version, and source policy is
  configured in `deny.toml` and checked with
  `bash tests/security/run-cargo-deny.sh`.
- The tag release contract and manual GitHub settings gates live in
  `RELEASING.md`.

## Failure Triage

- First inspect the failing script itself. The scripts in `tests/`, `bindings/`,
  and `packages/` are the canonical source for environment assumptions.
- Common failure modes are missing platform toolchains, missing Android SDK/NDK state, Kotlin/JVM toolchain resolution, dynamic library path issues, and dataset path mistakes.
- If a platform-specific path fails, rerun the exact repo-local script directly from the repo root and inspect the first failing command before widening the investigation.
