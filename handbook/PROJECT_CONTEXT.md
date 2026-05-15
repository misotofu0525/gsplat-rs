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
- Contribution guide: `../CONTRIBUTING.md`
- Security policy: `../SECURITY.md`

## Success Criteria

- The workspace builds and tests cleanly on the supported CI paths.
- `SortedAlpha` remains the only quality-guaranteed render mode.
- FFI smoke paths and mobile smoke integrations stay working.
- Desktop and mobile demos remain validation surfaces for the shared crates, not separate product lines.

## Current Repository Shape

- `crates/gsplat-core`: shared public types, config, stats, and error codes
- `crates/gsplat-io-ply`: PLY parsing and scene buffer construction
- `crates/gsplat-sort`: GPU and CPU sort backends
- `crates/gsplat-render-wgpu`: preprocessing, raster path, Surface presenter, and GPU helper APIs
- `crates/gsplat-ffi-c`: small C ABI surface over the renderer and mobile Surface presenters
- `crates/gsplat-web`: experimental `wasm-bindgen` bindings over the shared `wgpu` Surface renderer
- `apps/desktop-demo`: desktop viewer and offscreen PNG harness
- `apps/android-demo`: local `gsplat-android` Android library module, Kotlin Android Surface demo, and host-side JNI smoke
- `apps/ios-demo`: Swift smoke path plus UIKit realtime Surface app and iOS simulator/device build/run scripts
- `apps/web-demo`: browser PLY loader, generated wasm package host, and WebGL2 SortedAlpha-style fallback preview
- `tools/bench-runner`: perf and stability runner
- `tests/`: sample dataset, FFI smoke harness, and perf scripts
- `handbook/`: current project docs, architecture map, verification guide, roadmap, and project principles
- `docs/plans/`: task-scoped research and planning bundles
- `.github/`: CI workflows plus issue and pull request templates

## Common Commands

```bash
cargo check --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

For the broader command matrix, use `VERIFICATION.md`.

## Current Focus

- Keep the day-to-day verification paths passing and the release bar lightweight but real.
- Expand conformance and perf coverage with real datasets before widening the public API surface.
- Improve mobile integration only while the shared C ABI stays simple and stable.
- Turn Android integration into a local AAR/module shape before widening it into a published SDK.
- Bring the experimental Web SDK path up through the shared Rust `wgpu` Surface renderer.
- Keep the runtime scene path centered on validated in-memory `SceneBuffers` until a measured asset-pipeline need exists.
- Update the docs immediately when repository structure or responsibilities change.
- Keep contributor-facing maintenance files aligned with the actual verification and release boundary.

## Constraints and Boundaries

- `SortedAlpha` is the only release-gated render path right now.
- The current C ABI intentionally stays small and does not yet cover scene-from-memory loading or runtime render-mode switching.
- Android Surface integration now has a local `gsplat-android` library module
  that builds an AAR, but it is not Maven-published or a broad Android product
  API yet. iOS Surface integration remains a demo/validation path rather than
  an XCFramework or Swift Package.
- The Web demo is a browser validation surface; the Rust/WASM renderer boundary is active in
  `crates/gsplat-web` but remains experimental and requires wasm/browser smoke
  evidence for completion claims.
- Input PLY quaternion fields `rot_0..3` are interpreted as `w,x,y,z` and remapped internally to `x,y,z,w`.
- Input 3DGS coordinates are treated as `RDF` and converted at load time to runtime `RUF`, including quaternion and SH sign transforms.

## Notes

- Keep this file factual and current.
- Put transient task detail under `docs/plans/active/`, not here.
