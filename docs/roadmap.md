# Roadmap

This is the single source of truth for the current direction of the repository.

## Project position

- `gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust + wgpu.
- `SortedAlpha` is the only quality-guaranteed path for `v0.1.x`.
- Desktop/mobile demos exist to validate the core crates, not to become separate product lines.
- We prefer a small, stable surface over multiple partially-finished tracks.

## Current repository layout

- `crates/gsplat-core`: shared public types, config, stats, error codes
- `crates/gsplat-io-ply`: PLY parsing and scene buffer construction
- `crates/gsplat-sort`: GPU/CPU sort backends
- `crates/gsplat-render-wgpu`: preprocess, raster path, GPU helper APIs
- `crates/gsplat-format`: packed scene encoding/decoding
- `crates/gsplat-ffi-c`: frozen v0.1 C ABI surface
- `apps/desktop-demo`: desktop viewer and offscreen render harness
- `apps/android-demo`: Android demo container and host JNI smoke
- `apps/ios-demo`: Swift smoke path and simulator build scripts
- `tools/gsplat-pack`: PLY -> packed format converter
- `tools/bench-runner`: perf and stability runner

## Working agreements

- Keep this file as the only current planning document.
- Avoid placeholder apps, placeholder docs, or speculative top-level directories.
- Treat experimental render modes/backends as explicitly out-of-contract until promoted here.

## Current priorities

1. Keep the day-to-day commands below passing, and keep the release bar lightweight but real.
2. Expand conformance/perf coverage with real datasets before widening API surface.
3. Improve mobile integration depth only when the shared C ABI remains simple and stable.
4. Let runtime load the packed format directly so PLY becomes an import/input format, not the long-term runtime format.
5. Document repo structure changes immediately when directories or responsibilities move.

## Current API boundary

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

## PLY/runtime assumptions

- Input quaternion properties `rot_0..3` are interpreted as `w,x,y,z` and remapped internally to `x,y,z,w`.
- Input 3DGS PLY coordinates are treated as `RDF` and converted at load time to runtime `RUF`, including quaternion and SH sign transforms.

## Commands we rely on day to day

```bash
cargo check --workspace
cargo test --workspace
cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120
cargo run -p gsplat-pack -- tests/datasets/minimal_ascii.ply target/minimal.gspk --verify
bash tests/ffi/run-ffi-smoke.sh
bash apps/android-demo/run-jni-smoke.sh
bash apps/ios-demo/run-swift-smoke.sh
```

Desktop demo:

```bash
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
cargo run -p desktop-demo --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```

Mobile container builds:

```bash
bash apps/ios-demo/build-ios-sim.sh
bash apps/android-demo/build-apk.sh
```

## Release bar

Before cutting a release, in addition to the day-to-day commands above, also run:

```bash
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Explicitly not active right now

- A Web demo app
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
