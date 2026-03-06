# API Notes

Public APIs are split into:

- Rust crate APIs (`gsplat-core`, `gsplat-render-wgpu`, etc.)
- C ABI (`gsplat-ffi-c`) for mobile wrappers

## v0.1 contract

- `SortedAlpha` is the only quality-guaranteed path.
- `RenderMode` stays extensible, but anything other than `SortedAlpha` is experimental and out of contract.

## C ABI (v0.1 frozen surface)

Public header is `crates/gsplat-ffi-c/include/gsplat.h`; signatures are frozen in `crates/gsplat-ffi-c/src/lib.rs`.

Key exports:

- Version:
  - `gsplat_version_major() -> u32`
  - `gsplat_version_minor() -> u32`
- Context lifecycle:
  - `gsplat_context_create(config, out_ctx) -> i32`
  - `gsplat_context_destroy(ctx)`
- Camera:
  - `gsplat_context_set_camera(ctx, camera) -> i32`
- Scene:
  - `gsplat_context_load_scene_path(ctx, path) -> i32`
- Rendering:
  - `gsplat_context_render_frame(ctx) -> i32`
  - `gsplat_context_get_stats(ctx, out_stats) -> i32`

All C ABI functions return an `ErrorCode` as `i32` (see `gsplat-core`).

The current C ABI is intentionally small. It does not yet expose scene-from-memory loading, resize/surface integration, or runtime render-mode switching.

## Host smoke validation

- C smoke: `bash tests/ffi/run-ffi-smoke.sh`
- Java/JNI smoke: `bash apps/android-demo/run-jni-smoke.sh`
- Swift smoke: `bash apps/ios-demo/run-swift-smoke.sh`

## Mobile container build validation

- iOS simulator build:
  - `bash apps/ios-demo/build-ios-sim.sh`
- Android APK build:
  - `bash apps/android-demo/build-apk.sh`

## Offline format tooling

- Pack PLY to runtime blob:
  - `cargo run -p gsplat-pack -- <input.ply> <output.gspk> --verify`
- Format primitives:
  - `gsplat-format::pack_scene`
  - `gsplat-format::unpack_scene`
