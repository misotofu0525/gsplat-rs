# gsplat-rs Context

## Overview

- `gsplat-rs` is a cross-platform Gaussian Splatting rendering library built with Rust + `wgpu`.
- The repo serves three audiences: Rust library consumers, native integrators through the C ABI, and maintainers validating the stack through demos and tooling.
- The project is still on the `0.1.x` line, with a deliberately small release surface.

## Canonical Docs

- Agent/project entrypoint: `../AGENTS.md`
- Architecture map: `ARCHITECTURE.md`
- Verification entrypoint: `VERIFICATION.md`
- Direction and scope: `ROADMAP.md`
- Project taste guide: `GOLDEN_PRINCIPLES.md`

## Success Criteria

- The workspace builds and tests cleanly on the supported CI paths.
- `SortedAlpha` remains the only quality-guaranteed render mode.
- The pack format, FFI smoke paths, and mobile smoke integrations stay working.
- Desktop and mobile demos remain validation surfaces for the shared crates, not separate product lines.

## Current Repository Shape

- `crates/gsplat-core`: shared public types, config, stats, and error codes
- `crates/gsplat-io-ply`: PLY parsing and scene buffer construction
- `crates/gsplat-format`: packed scene encoding and decoding
- `crates/gsplat-sort`: GPU and CPU sort backends
- `crates/gsplat-render-wgpu`: preprocessing, raster path, and GPU helper APIs
- `crates/gsplat-ffi-c`: small C ABI surface over the renderer
- `apps/desktop-demo`: desktop viewer and offscreen PNG harness
- `apps/android-demo`: Android container build plus host-side JNI smoke
- `apps/ios-demo`: Swift smoke path plus iOS simulator build scripts
- `tools/gsplat-pack`: PLY to packed-format converter
- `tools/bench-runner`: perf and stability runner
- `tests/`: sample dataset, FFI smoke harness, and perf scripts
- `handbook/`: current project docs, architecture map, verification guide, roadmap, and project principles
- `docs/plans/`: task-scoped research and planning bundles

## Common Commands

```bash
cargo check --workspace
cargo test --workspace
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

For the broader command matrix, use `VERIFICATION.md`.

## Current Focus

- Keep the day-to-day verification paths passing and the release bar lightweight but real.
- Expand conformance and perf coverage with real datasets before widening the public API surface.
- Improve mobile integration only while the shared C ABI stays simple and stable.
- Move runtime usage toward the packed scene format so PLY remains an import/input format.
- Update the docs immediately when repository structure or responsibilities change.

## Constraints and Boundaries

- `SortedAlpha` is the only release-gated render path right now.
- The current C ABI intentionally stays small and does not yet cover scene-from-memory loading, resize/surface integration, or runtime render-mode switching.
- Input PLY quaternion fields `rot_0..3` are interpreted as `w,x,y,z` and remapped internally to `x,y,z,w`.
- Input 3DGS coordinates are treated as `RDF` and converted at load time to runtime `RUF`, including quaternion and SH sign transforms.

## Notes

- Keep this file factual and current.
- Put transient task detail under `docs/plans/active/`, not here.
