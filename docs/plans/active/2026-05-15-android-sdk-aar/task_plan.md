# Task Plan: Android SDK AAR Slice

## Goal

Move Android from demo-only integration toward a reusable SDK artifact without
changing the small v0.1 C ABI.

## Scope

- Add an Android library module that can build a local AAR.
- Keep the demo app as a sample and validation surface that depends on the
  library module.
- Keep the JNI smoke path working.
- Do not add Maven publishing, multi-ABI distribution, or a broader mobile
  product API in this slice.

## Plan

1. [done] Add `apps/android-demo/gsplat-android` as an Android library module.
2. [done] Move reusable Kotlin native bindings into `com.gsplat.android`.
3. [done] Add typed Kotlin wrappers for Surface renderer options, stats, and
   handle lifetime.
4. [done] Package the generated native library through the library module
   instead of the demo app.
5. [done] Add `apps/android-demo/build-aar.sh`.
6. [done] Sync README and handbook docs.
7. [done] Run Android/Rust verification and record evidence.

## Current Boundary

- `gsplat-android-release.aar` is buildable locally but not published to Maven.
- The AAR currently packages `arm64-v8a` only.
- Scene loading remains path-based through the existing C ABI.
- The public C ABI remains unchanged.

## Evidence

- `cargo fmt --check` passed.
- `git diff --check` passed.
- `node --check apps/web-demo/src/main.js` passed.
- `cargo check --workspace` passed.
- `bash apps/android-demo/run-jni-smoke.sh` passed.
- `bash apps/android-demo/build-aar.sh` passed and produced
  `apps/android-demo/gsplat-android/build/outputs/aar/gsplat-android-release.aar`.
- `bash apps/android-demo/build-apk.sh` passed and produced
  `apps/android-demo/app/build/outputs/apk/debug/app-debug.apk`.
- AAR inspection confirmed `jni/arm64-v8a/libgsplat_jni.so` and the
  `com/gsplat/android` Kotlin wrapper classes are packaged.
