# gsplat-rs

`gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust
and `wgpu`. The project focuses on a small, verifiable core: PLY import,
in-memory scene buffers, `SortedAlpha` rendering, a narrow C ABI, and demo
surfaces that validate the stack on desktop, Android, iOS, and browser paths
without overstating SDK maturity.

## Project Status

- Release line: `0.1.x`
- Quality-gated render path: `SortedAlpha`
- Native integration surface: `crates/gsplat-ffi-c/include/gsplat.h`
- Android status: a local Android library module can build an AAR at
  `apps/android-demo/gsplat-android/build/outputs/aar/gsplat-android-release.aar`;
  it is not published to Maven yet.
- iOS status: `apps/ios-demo/GsplatKit` is a local Swift package wrapper and
  `bash apps/ios-demo/build-xcframework.sh` builds a local
  `GsplatFFI.xcframework`; it is not a published binary SwiftPM release yet.
- Web status: `apps/web-demo/gsplat-web-sdk` is a local browser ESM wrapper
  around the experimental `crates/gsplat-web` Rust/WASM renderer; it is not
  published to npm yet.

## Quick Start

```bash
cargo check --workspace
cargo test --workspace
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

For the browser validation demo:

```bash
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

Then open `http://127.0.0.1:4173/apps/web-demo/`.

## Repository Layout

- `crates/gsplat-core`: shared public types, config, stats, and error codes
- `crates/gsplat-io-ply`: PLY parsing and scene buffer construction
- `crates/gsplat-sort`: CPU and GPU sort backends
- `crates/gsplat-render-wgpu`: preprocessing, raster path, Surface presenter,
  and GPU helper APIs
- `crates/gsplat-ffi-c`: small C ABI surface over the renderer and mobile
  Surface presenters
- `crates/gsplat-web`: experimental `wasm-bindgen` bindings over the shared
  `wgpu` Surface renderer
- `apps/desktop-demo`: desktop viewer and offscreen PNG harness
- `apps/android-demo`: Android Surface demo plus host-side JNI smoke
  and the local `gsplat-android` library module
- `apps/ios-demo`: local `GsplatKit` Swift package wrapper, Swift smoke path,
  UIKit realtime Surface app, and iOS simulator/device scripts
- `apps/web-demo`: browser PLY loader, local `@gsplat-rs/web` wrapper,
  generated wasm package hosts, and WebGL2 fallback preview
- `tools/bench-runner`: perf and stability runner
- `tests/`: sample dataset, FFI smoke harness, and perf scripts
- `handbook/`: current project docs, architecture map, verification guide,
  roadmap, and project principles

## Verification

The fast check is:

```bash
cargo check --workspace
```

The CI-level local hygiene checks are:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
node --check apps/web-demo/src/main.js
node --check apps/web-demo/gsplat-web-sdk/src/index.js
```

Use `handbook/VERIFICATION.md` for the full validation matrix, including FFI,
JNI, Swift, desktop, Web, mobile Surface, and long-stability smoke paths.

## Integration Boundaries

The current mobile-facing contract is the C ABI in
`crates/gsplat-ffi-c/include/gsplat.h`.

- Use `gsplat_config_default()` and `gsplat_camera_default()` instead of
  hand-writing ABI defaults.
- Use `GSPLAT_RENDER_MODE_SORTED_ALPHA`; it is the only release-gated render
  mode in v0.1.
- Treat non-zero returns as `GsplatErrorCode` values and pass them to
  `gsplat_error_message()`.
- Android Surface rendering is demonstrated by `apps/android-demo`.
- The local Android AAR is built with `bash apps/android-demo/build-aar.sh`.
- Swift/C ABI integration, a local `GsplatKit` wrapper, local XCFramework
  packaging, and a UIKit realtime simulator/device Surface demo are
  demonstrated by `apps/ios-demo`.
- The local iOS XCFramework is built with
  `bash apps/ios-demo/build-xcframework.sh`.
- Browser Rust/WASM integration and the local `@gsplat-rs/web` ESM wrapper are
  demonstrated by `apps/web-demo`.
- The local Web SDK wrapper is built with `bash apps/web-demo/build-web-sdk.sh`.
- Not in the v0.1 contract: scene-from-memory loading, runtime render-mode
  switching, Maven publishing, multi-ABI Android distribution, and
  published binary SwiftPM/XCFramework or npm distribution.

## Documentation

- Project context: `handbook/PROJECT_CONTEXT.md`
- Architecture map: `handbook/ARCHITECTURE.md`
- Verification commands: `handbook/VERIFICATION.md`
- Current direction and release boundary: `handbook/ROADMAP.md`
- Project principles: `handbook/GOLDEN_PRINCIPLES.md`
- Agent entrypoint: `AGENTS.md`

## Contributing

Read `CONTRIBUTING.md` before opening a pull request. The short version is:
keep diffs small, preserve the documented release boundary, run the relevant
verification path, and update handbook docs when repository structure or
commands change.

## Security

Do not open public issues that include exploit details, private datasets,
tokens, or credentials. See `SECURITY.md` for the reporting policy.

## License

Licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
