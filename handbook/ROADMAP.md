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
5. Keep handbook docs and verification commands aligned with the repository that actually exists.

## Current Release Boundary

- The public contract is centered on PLY import, in-memory scene buffers, `SortedAlpha` rendering, and the small C ABI.
- Experimental Rust APIs may exist only when they stay out of the release contract and do not complicate verification.
- Any backend that requires matched training metadata stays disabled by default until promoted here.
- The current C ABI intentionally stays small:
  - `gsplat_context_create`
  - `gsplat_context_destroy`
  - `gsplat_context_set_camera`
  - `gsplat_context_load_scene_path`
  - `gsplat_context_render_frame`
  - `gsplat_context_get_stats`
  - Android Surface renderer create/resize/render/stats/destroy functions for the demo integration path
- The current C ABI does not cover scene-from-memory loading or runtime render-mode switching.
- Android Surface functions are validation/demo support, not a commitment to a full mobile product API.

## Release Bar

- The canonical day-to-day verification set lives in `handbook/VERIFICATION.md`.
- Before cutting a release, also run:

```bash
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Explicitly Not Active Right Now

- A custom internal binary scene/cache format
- A Web demo app
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
