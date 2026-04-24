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

## Release Bar

Before cutting a release, also run:

```bash
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Targeted Checks

- If you touch `crates/gsplat-ffi-c/`, run `bash tests/ffi/run-ffi-smoke.sh`.
- If you touch `apps/android-demo/` or JNI glue, run `bash apps/android-demo/run-jni-smoke.sh`.
- If you touch `apps/ios-demo/` or Swift/FFI integration, run `bash apps/ios-demo/run-swift-smoke.sh`.
- If you touch PLY import or scene normalization, run `cargo test --workspace` and `cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png`.
- If you touch renderer, sorting, or perf-sensitive code, run `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120` and consider the long-stability script.

## Structural Checks

- CI entrypoints live in `.github/workflows/ci.yml`, `.github/workflows/perf-smoke.yml`, and `.github/workflows/long-stability.yml`.
- There is no dedicated lint entrypoint in the repo today; do not invent one in docs or task reports.

## Failure Triage

- First inspect the failing script itself. The scripts in `tests/` and `apps/*-demo/` are the canonical source for environment assumptions.
- Common failure modes are missing platform toolchains, missing Android SDK/NDK state, Java toolchain resolution, dynamic library path issues, and dataset path mistakes.
- If a platform-specific path fails, rerun the exact repo-local script directly from the repo root and inspect the first failing command before widening the investigation.
