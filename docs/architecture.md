# Architecture

`gsplat-rs` is organized around one release-gated rendering pipeline plus thin integration/demo layers.

## Release-gated pipeline (v0.1)

1. Load scene data with `gsplat-io-ply` into `SceneBuffers`
2. Build visibility and depth keys in `gsplat-render-wgpu`
3. Sort with `gsplat-sort` when the active mode requires ordering
4. Rasterize and blend via the WGSL covariance-driven anisotropic splat path in `gsplat-render-wgpu`
5. Surface stats and error codes through Rust APIs and the frozen C ABI in `gsplat-ffi-c`
6. Optionally pack scenes offline with `gsplat-format` and `gsplat-pack`

## Workspace responsibilities

- `crates/gsplat-core`: shared types, render config, stats, error codes
- `crates/gsplat-io-ply`: PLY parsing and scene buffer construction
- `crates/gsplat-sort`: GPU/CPU sort backends
- `crates/gsplat-render-wgpu`: preprocess, raster path, GPU helper APIs
- `crates/gsplat-format`: packed scene encoding/decoding
- `crates/gsplat-ffi-c`: stable C ABI entry point
- `apps/desktop-demo`: desktop viewer and offscreen render harness
- `apps/android-demo`: Android demo container and host JNI smoke path
- `apps/ios-demo`: Swift smoke path and simulator build scripts

## Scope boundaries

- `SortedAlpha` is the only release-gated quality path for `v0.1.x`.
- Experimental modes may exist in the Rust API surface, but they are not treated as release criteria.
- Web support stays out of the main tree until there is runnable code, not just placeholder docs.
