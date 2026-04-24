# Roadmap

This file is the current direction and release-boundary document for the repository.
Operational facts and command entrypoints live in `handbook/PROJECT_CONTEXT.md` and `handbook/VERIFICATION.md`.

## Project Position

- `gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust + wgpu.
- `SortedAlpha` is the only quality-guaranteed path for `v0.1.x`.
- Desktop/mobile demos exist to validate the core crates, not to become separate product lines.
- We prefer a small, stable surface over multiple partially-finished tracks.

## Current Priorities

1. Keep the day-to-day verification set passing, and keep the release bar lightweight but real.
2. Expand conformance/perf coverage with real datasets before widening API surface.
3. Improve mobile integration depth only when the shared C ABI remains simple and stable.
4. Let runtime load the packed format directly so PLY becomes an import/input format, not the long-term runtime format.
5. Document repo structure changes immediately when directories or responsibilities move.

## Current Release Boundary

- The Rust API may keep room for experimentation, but `SortedAlpha` is the only release-gated render mode.
- Experimental backends are allowed only when they stay explicitly out-of-contract and off the main release path.
- Any backend that requires matched training metadata stays disabled by default until promoted here.
- The current C ABI intentionally stays small:
  - `gsplat_context_create`
  - `gsplat_context_destroy`
  - `gsplat_context_set_camera`
  - `gsplat_context_load_scene_path`
  - `gsplat_context_render_frame`
  - `gsplat_context_get_stats`
- The current C ABI does not yet cover scene-from-memory loading, resize/surface integration, or runtime render-mode switching.

## Release Bar

- The canonical day-to-day verification set now lives in `handbook/VERIFICATION.md`.
- Before cutting a release, also run:

```bash
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Explicitly Not Active Right Now

- A Web demo app
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
