# Mobile SDK Review Task Plan

## Goal

- Review the Android/iOS integration path as a mobile developer would see it.
- Keep the v0.1 public surface small while making the existing C ABI and demos easier to consume and diagnose.

## Findings

- The repo currently exposes a C ABI plus Android/iOS validation demos, not packaged Android/iOS SDK artifacts.
- The public header exposed raw integer contracts but did not name render mode or error codes.
- Callers had to hand-write default config and camera values in C, JNI, and Swift examples.
- Android Surface creation collapsed every failure into a zero handle on the Kotlin side, hiding the native error code from app UI and logs.
- The roadmap ABI list had drifted from the actual header because `gsplat_context_set_auto_camera` and version helpers were missing from the list.

## Plan

1. [done] Add named constants, default helpers, and error-message helper to the C header and Rust implementation.
2. [done] Update C/JNI/Swift smoke paths to use the helper API instead of duplicating defaults.
3. [done] Return Android Surface creation error codes to Kotlin and display readable diagnostics.
4. [done] Refresh README/example docs and roadmap to match the actual mobile contract.
5. [done] Run canonical FFI/mobile smoke verification.

## Evidence

- `cargo check --workspace` passed.
- `cargo test --workspace` passed.
- `bash tests/ffi/run-ffi-smoke.sh` passed.
- `bash bindings/android/scripts/run-jni-smoke.sh` passed.
- `bash bindings/apple/scripts/run-swift-smoke.sh` passed.
- `bash bindings/android/scripts/build-sample-apk.sh` passed.
- `bash bindings/apple/scripts/build-ios-sim.sh` passed.

## Follow-up: Android Touch Controls

- Goal: add real example camera controls for mobile validation.
- Plan:
  1. [done] Add Surface renderer camera-control ABI for reset, orbit, zoom, and pan.
  2. [done] Wire JNI and Kotlin `NativeBridge` functions.
  3. [done] Add Android gestures: one-finger orbit, pinch zoom, two-finger pan, double-tap reset.
  4. [done] Re-run Android build and real-device smoke.
- Evidence:
  - `cargo check --workspace` passed.
  - `cargo test --workspace` passed.
  - `bash tests/ffi/run-ffi-smoke.sh` passed.
  - `bash bindings/android/scripts/run-jni-smoke.sh` passed.
  - `bash bindings/android/scripts/build-sample-apk.sh` passed.
  - `bash bindings/apple/scripts/run-swift-smoke.sh` passed.
  - Real device `android-test-device` installed and launched the APK successfully.
  - Simulated one-finger drag via ADB changed the overlay to `camera=orbit` while rendering `flowers_1.ply`.
  - Screenshot: `target/android-screenshots/gsplat-touch-real-device-20260424-162150.png`.
