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
```

- Run this when changing shared types, parsing, render logic, or CLI behavior.

## Day-to-Day Verification Set

These are the current day-to-day commands the repo relies on:

```bash
cargo check --workspace
cargo test --workspace
cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120
bash tests/ffi/run-ffi-smoke.sh
bash apps/android-demo/run-jni-smoke.sh
bash apps/ios-demo/run-swift-smoke.sh
```

## Desktop Smoke

```bash
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
cargo run -p desktop-demo --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```

- Use the PNG path for deterministic local smoke output.
- Use the interactive viewer when changing windowed presentation or camera interaction behavior.

## Mobile Container Builds

```bash
bash apps/ios-demo/build-ios-sim.sh
bash apps/android-demo/build-apk.sh
```

- Run these when changing mobile packaging or build scripts.
- Check the matching app README for platform prerequisites before assuming SDK/NDK/Xcode state.
- `apps/android-demo/build-apk.sh` builds a debug APK container, but compiles the Rust native library with the Rust `release` profile by default. Set `ANDROID_RUST_PROFILE=dev` only for native debugging.

## Android Surface Smoke

Use this when changing Android Surface rendering, JNI surface glue, or `SurfacePresenter` behavior:

```bash
bash apps/android-demo/build-apk.sh
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_SDK_ROOT/platform-tools/adb"
"$ADB" install -r apps/android-demo/app/build/outputs/apk/debug/app-debug.apk
"$ADB" push tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply /data/local/tmp/flowers_1.ply
"$ADB" shell run-as com.gsplat.demo mkdir -p files
"$ADB" shell run-as com.gsplat.demo cp /data/local/tmp/flowers_1.ply files/flowers_1.ply
"$ADB" shell rm -f /data/local/tmp/flowers_1.ply
"$ADB" shell am start -n com.gsplat.demo/.MainActivity
```

- Expected overlay includes `surface=wgpu realtime`, `state=rendering`, and `drawn=<surface_instances>/<visible_instances>`.
- For repeatable perf checks, add the benchmark extras documented in `apps/android-demo/README.md` and read the `BENCHMARK_RESULT` logcat line.
- Android emulator storage can be tight after pushing the flower PLY. If `adb install -r` reports insufficient storage, uninstall `com.gsplat.demo`, reinstall, and push the dataset again.

## Release Bar

Before cutting a release, also run:

```bash
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Targeted Checks

- If you touch `crates/gsplat-ffi-c/`, run `bash tests/ffi/run-ffi-smoke.sh`.
- If you touch `apps/android-demo/` or JNI glue, run `bash apps/android-demo/run-jni-smoke.sh`; for Surface changes, also run the Android Surface smoke above.
- If you touch `apps/ios-demo/` or Swift/FFI integration, run `bash apps/ios-demo/run-swift-smoke.sh`.
- If you touch PLY import or scene normalization, run `cargo test --workspace` and `cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png`.
- If you touch renderer, sorting, or perf-sensitive code, run `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120` and consider the long-stability script.
- For spatial/tile/chunk feasibility checks on a loaded PLY, use:

```bash
cargo run -p bench-runner -- <scene.ply> --analyze-spatial
```

## Structural Checks

- CI entrypoints live in `.github/workflows/ci.yml`, `.github/workflows/perf-smoke.yml`, and `.github/workflows/long-stability.yml`.
- There is no dedicated lint entrypoint in the repo today; do not invent one in docs or task reports.

## Failure Triage

- First inspect the failing script itself. The scripts in `tests/` and `apps/*-demo/` are the canonical source for environment assumptions.
- Common failure modes are missing platform toolchains, missing Android SDK/NDK state, Kotlin/JVM toolchain resolution, dynamic library path issues, and dataset path mistakes.
- If a platform-specific path fails, rerun the exact repo-local script directly from the repo root and inspect the first failing command before widening the investigation.
