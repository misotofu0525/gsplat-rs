# gsplat-rs

[![CI](https://github.com/misotofu0525/gsplat-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/misotofu0525/gsplat-rs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV: 1.93](https://img.shields.io/badge/MSRV-1.93-orange.svg)](rust-toolchain.toml)

`gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust
and `wgpu`. The project focuses on a small, verifiable core: PLY import,
in-memory scene buffers, `SortedAlpha` rendering, a narrow C ABI, and example
surfaces that validate the stack on desktop, Android, iOS, and browser paths
without overstating SDK maturity.

![SortedAlpha render of the Wakufactory Kitsune scene](docs/media/kitune.jpg)

*279,199 splats rendered offscreen by `examples/desktop` through the
`SortedAlpha` path. The trimmed Kitsune scene is published by
[Wakufactory](https://www.wakufactory.jp/wxr/splats/sample.html) under CC0 and
can be fetched with `bash tests/datasets/fetch-wakufactory-kitune.sh`.*

## Platform Support

| Platform | Validation surface | Distribution status |
| --- | --- | --- |
| Desktop (Metal / Vulkan / DX12) | `examples/desktop` viewer and offscreen PNG harness | Build from source (Rust workspace) |
| Android | `examples/android` app over the `bindings/android` library module | Local AAR build, `arm64-v8a` only; not on Maven yet |
| iOS | `examples/ios` app over the `bindings/apple` `GsplatKit` package | Local XCFramework build; no binary SwiftPM release yet |
| Web (WASM) | `examples/web` host over the `packages/web` ESM wrapper | Local build of experimental `crates/gsplat-web`; not on npm yet |

## Project Status

- Release line: `0.1.x`
- Quality-gated render path: `SortedAlpha`
- Native integration surface: `crates/gsplat-ffi-c/include/gsplat.h`

## Installation and Artifacts

The repository is currently prerelease software. Rust crates are not yet on
crates.io, and the Android, Apple, and Web packages are not published to Maven,
SwiftPM, or npm registries.

For source use, clone the repository, keep the pinned toolchain, and build the
workspace or the validation surface you need:

```bash
git clone https://github.com/misotofu0525/gsplat-rs.git
cd gsplat-rs
cargo check --workspace
```

Tagged `0.1.x` prereleases attach an AAR, XCFramework ZIP, and npm-compatible
tarball to the [GitHub Releases](https://github.com/misotofu0525/gsplat-rs/releases)
page. These are direct prerelease artifacts; use the matching
`bindings/android/README.md`, `bindings/apple/README.md`, or
`packages/web/README.md` for integration details. Maintainers should follow
`RELEASING.md` rather than creating or replacing artifacts manually.

## Quick Start

```bash
cargo check --workspace
cargo test --workspace
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
```

For the browser validation example:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash packages/web/scripts/build-wasm.sh
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

Then open `http://127.0.0.1:4173/examples/web/`.

The same Kitsune showcase is the default native first frame on Android and iOS:

```bash
bash bindings/android/scripts/build-sample-apk.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
```

Both mobile build paths accept an explicit PLY file as their first argument,
and fall back to the shared Flowers fixture when Kitsune is unavailable.

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
- `examples/desktop`: desktop viewer and offscreen PNG harness
- `examples/android`: Android Surface sample app
- `examples/ios`: UIKit realtime Surface sample app
- `examples/web`: browser PLY loader, generated wasm package host, and WebGL2
  fallback preview
- `bindings/android`: local `gsplat-android` library module, JNI bridge,
  host-side JNI smoke, and AAR/APK scripts
- `bindings/apple`: local `GsplatKit` Swift package wrapper, Swift smoke path,
  XCFramework scripts, and iOS simulator/device scripts
- `packages/web`: local `@gsplat-rs/web` browser ESM wrapper
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
node --check examples/web/src/main.js
npm --prefix packages/web run check
npm --prefix packages/web test
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
  `gsplat_error_message()`. Use `gsplat_last_error_message()` when wrappers need
  the most recent operation detail.
- Treat native Surface handles as owned by one serialized thread or queue. The
  Kotlin and Swift wrappers add locking around the raw C handles; direct C/JNI
  callers should provide the same serialization.
- Android Surface rendering is demonstrated by `examples/android`, with the
  wrapper-first entrypoint in `bindings/android`.
- The local Android AAR is built with `bash bindings/android/scripts/build-aar.sh`.
- Swift/C ABI integration, a local `GsplatKit` wrapper, local XCFramework
  packaging, and a UIKit realtime simulator/device Surface example are
  split across `bindings/apple` and `examples/ios`.
- The local iOS XCFramework is built with
  `bash bindings/apple/scripts/build-xcframework.sh`.
- Browser Rust/WASM integration and the local `@gsplat-rs/web` ESM wrapper are
  demonstrated by `examples/web`.
- The local Web SDK wrapper is built with `bash packages/web/scripts/build.sh`.
- Not in the v0.1 contract: scene-from-memory loading, runtime render-mode
  switching, Maven publishing, multi-ABI Android distribution, and
  published binary SwiftPM/XCFramework or npm distribution.

## Known Gaps Before External Release

- Android: the local AAR is not published to Maven and currently packages
  `arm64-v8a` only.
- iOS: `GsplatKit` is a local binary Swift package wrapper; there is no remote
  binary SwiftPM release or published XCFramework artifact yet.
- Web: `@gsplat-rs/web` builds and packs locally, but is not published to npm
  and is not a stable v0.1 public contract.
- Device validation: local build checks cover APK/AAR, iOS simulator, and iOS
  device app signing, but Android true-device launch and iOS physical-device
  launch/benchmark still require explicit device runs.

## Documentation

- Project context: `handbook/PROJECT_CONTEXT.md`
- Architecture map: `handbook/ARCHITECTURE.md`
- Verification commands: `handbook/VERIFICATION.md`
- Current direction and release boundary: `handbook/ROADMAP.md`
- Project principles: `handbook/GOLDEN_PRINCIPLES.md`
- Release process: `RELEASING.md`
- Media provenance: `docs/media/README.md`
- Agent entrypoint: `AGENTS.md`

## Contributing

Read `CONTRIBUTING.md` before opening a pull request. The short version is:
keep diffs small, preserve the documented release boundary, run the relevant
verification path, and update handbook docs when repository structure or
commands change.

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md) code of
conduct.

## Security

Do not open public issues that include exploit details, private datasets,
tokens, or credentials. See `SECURITY.md` for the reporting policy.

## License

Licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
