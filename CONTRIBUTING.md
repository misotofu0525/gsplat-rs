# Contributing to gsplat-rs

Thanks for taking the time to improve `gsplat-rs`. This project values small,
verified changes over broad rewrites.

By participating in this project you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Before You Start

- Read `README.md` for the public project status.
- Read `handbook/PROJECT_CONTEXT.md` and `handbook/ARCHITECTURE.md` before
  changing crate boundaries, render flow, FFI, or repo structure.
- Read `handbook/VERIFICATION.md` before claiming a change is complete.
- Read `handbook/ROADMAP.md` before widening the release surface.
- Keep `SortedAlpha` as the only release-gated render path unless the roadmap
  is explicitly updated first.

## Development Setup

The repository pins its Rust toolchain in `rust-toolchain.toml`.

```bash
cargo check --workspace
cargo test --workspace
```

Platform validation paths require their platform toolchains:

- Android/JNI: Android SDK, NDK, Gradle, and a JDK
- iOS/Swift: Xcode command line tools and simulator/device signing state
- Web/WASM: `wasm32-unknown-unknown` and `wasm-bindgen` for the Rust/WASM
  renderer path

Use the scripts documented in `handbook/VERIFICATION.md`; do not replace them
with ad-hoc command sequences in PR descriptions.

## Code Style

Run these before opening a pull request:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

When changing Rust APIs that may be documented or published, also run:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Verification Expectations

Pick the narrowest verification path that proves your change:

- Shared Rust types, parsing, render logic, or CLI behavior:
  `cargo test --workspace`
- C ABI changes:
  `bash tests/ffi/run-ffi-smoke.sh`
- Android JNI changes:
  `bash bindings/android/scripts/run-jni-smoke.sh`
- iOS Swift/FFI changes:
  `bash bindings/apple/scripts/run-swift-smoke.sh`
- Web example changes:
  `node --check examples/web/src/main.js` plus the browser smoke in
  `handbook/VERIFICATION.md`
- Renderer, sorting, or perf-sensitive changes:
  `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120`

For mobile Surface or true-device work, do not treat a successful build as a
complete validation. Follow the matching Android or iOS smoke path in
`handbook/VERIFICATION.md` and include the observed runtime evidence.

## Pull Request Checklist

- The change stays inside the documented release boundary, or the roadmap was
  updated before the new boundary was treated as stable.
- Public C ABI changes update both `crates/gsplat-ffi-c/src/lib.rs` and
  `crates/gsplat-ffi-c/include/gsplat.h`.
- Repository structure or command changes are reflected in `README.md`,
  `AGENTS.md`, and the relevant `handbook/` files.
- New experimental paths are opt-in until local tests, smoke paths, and
  benchmarks prove they should become default.
- No secrets, credentials, private datasets, or `.env` values are committed.

## Issue Reports

Good bug reports include:

- Platform and toolchain versions
- The exact command or script that failed
- The first failing error message
- Whether the failure is build-time, runtime, visual, or performance-related
- The smallest public dataset or command that reproduces it

Do not attach private datasets or secrets to public issues.
