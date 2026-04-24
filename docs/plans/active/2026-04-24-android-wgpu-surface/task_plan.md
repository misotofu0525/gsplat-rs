# Android WGPU Surface Task Plan

## Goal

- Replace the Android bitmap/readback preview path with a real `SurfaceView` -> JNI -> Rust/wgpu surface presentation path.
- Keep the Android demo as a validation surface for the shared renderer, not a separate product.

## Current Findings

- Desktop interactive already has a working `wgpu::Surface` presenter that renders sorted splat instances into a swapchain.
- Android can hand native code an `ANativeWindow` through `SurfaceHolder.Callback` + `Surface`.
- `wgpu` 28 supports Android surfaces through `SurfaceTargetUnsafe::RawHandle` with an Android display handle and Android NDK window handle.
- The previous Android RGBA preview proved PLY loading and auto-camera on device, but it is not the intended realtime path.
- The Android emulator used here reports a 2048 max texture side and returns `SurfaceError::Other` when asked to draw all 562,974 visible flower splats every present.
- A fixed 720x1600 Surface buffer plus a 120,000-instance Surface LOD keeps the emulator path stable while preserving the real `wgpu::Surface` presentation route.

## Plan

1. [done] Move reusable surface presenter logic into `gsplat-render-wgpu`.
2. [done] Add small C ABI functions for creating, resizing, rendering, stats, and destroying a surface renderer.
3. [done] Replace Android UI with a `SurfaceView` and Kotlin render loop.
4. [done] Keep host JNI smoke working without Android-only surface APIs.
5. [done] Verify with workspace checks, JNI smoke, APK build, emulator install/run, and screenshot.

## Evidence

- `bash apps/android-demo/build-apk.sh` passed after the Surface path changes.
- Emulator run rendered `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply` through `SurfaceView` -> JNI -> `ANativeWindow` -> `wgpu::Surface`.
- Final screenshot: `target/android-screenshots/gsplat-flower-surface-real-final.png`.
