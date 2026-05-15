# Roadmap

This file defines the current direction and release boundary for `gsplat-rs`.
Operational facts and command entrypoints live in `handbook/PROJECT_CONTEXT.md` and `handbook/VERIFICATION.md`.

## Project Position

- `gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust + `wgpu`.
- The project is on the `0.1.x` line and should stay small until the core render path is more thoroughly validated.
- `SortedAlpha` is the only quality-guaranteed render mode.
- Desktop and mobile demos are validation surfaces for shared crates, not separate product lines.

## Near-Term Priorities

1. Keep the PLY import -> `SceneBuffers` -> renderer path correct and well tested.
2. Expand conformance and performance coverage with real datasets before widening APIs.
3. Keep C ABI, JNI, and Swift smoke paths boring, small, and in sync.
4. Improve renderer quality and stability inside the existing crate boundaries.
5. Bring the Rust/WASM Web renderer target up behind the shared `wgpu` Surface path before calling Web parity complete.
6. Keep handbook docs and verification commands aligned with the repository that actually exists.

## Current Release Boundary

- The public contract is centered on PLY import, in-memory scene buffers, `SortedAlpha` rendering, and the small C ABI.
- Experimental Rust APIs may exist only when they stay out of the release contract and do not complicate verification.
- Any backend that requires matched training metadata stays disabled by default until promoted here.
- The current C ABI intentionally stays small:
  - `gsplat_version_major`
  - `gsplat_version_minor`
  - `gsplat_error_message`
  - `gsplat_config_default`
  - `gsplat_camera_default`
  - `gsplat_context_create`
  - `gsplat_context_destroy`
  - `gsplat_context_set_camera`
  - `gsplat_context_set_auto_camera`
  - `gsplat_context_load_scene_path`
  - `gsplat_context_render_frame`
  - `gsplat_context_get_stats`
  - Android and iOS Surface renderer create/resize/camera-control/render/stats/destroy functions for the demo integration paths
- The current C ABI does not cover scene-from-memory loading or runtime render-mode switching.
- Mobile Surface functions are validation/demo support, not a commitment to a full mobile product API.
- `crates/gsplat-web` is the active experimental Rust/WASM Web API boundary. It is not a stable v0.1 release surface; Web renderer changes require verified wasm build and browser smoke evidence.
- The Web demo is validation/demo support for browser PLY loading, the WebGL2 fallback, and hosting the generated wasm package; it is not a polished web product surface.

## Release Bar

- The canonical day-to-day verification set lives in `handbook/VERIFICATION.md`.
- Before cutting a release, also run:

```bash
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Explicitly Not Active Right Now

- A custom internal binary scene/cache format
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
