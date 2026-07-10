# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3] - 2026-07-10

### Added

- Unified direct sorted-index Surface/offscreen path as the shared SortedAlpha
  renderer across desktop, mobile, and Web (GPU-resident scene + sorted `u32`
  indices; CPU radix sort retained).
- 8-bit CPU radix with NEON/AVX2 multi-histogram counting in `gsplat-sort`, plus
  key-bit-only sorting on the production `sort_values_by_keys` path.

### Changed

- Removed the selectable CPU-instance / preproject Surface raster alternatives
  in favor of a single direct sorted-index pipeline.
- Web and desktop validation surfaces now exercise the same resident-index
  draw path used by Android/iOS.

### Fixed

- Web upload-bound frames on dense PLYs by stopping per-frame full instance
  buffer uploads on the production Surface path.

## [0.1.2] - 2026-07-10

### Fixed

- Made the Android AAR release job bootstrap Gradle from a fresh checkout where
  the repository `target/` directory does not yet exist.
- Packed the Web SDK from its package directory so npm emits the tarball at the
  workflow artifact path instead of resolving a nonexistent root package.

## [0.1.1] - 2026-07-10

### Added

- Open-source maintenance docs: `CONTRIBUTING.md`, `SECURITY.md`, issue
  templates, and a pull request template.
- Dual-license files for the existing `MIT OR Apache-2.0` package metadata.
- CI hygiene coverage for rustfmt, clippy, rustdoc warnings, and Web example
  JavaScript syntax.
- `CODE_OF_CONDUCT.md` (Contributor Covenant v2.1), `.github/CODEOWNERS`, and
  Dependabot configuration for cargo, GitHub Actions, npm, and Gradle.
- Tag-triggered release workflow that runs core checks and publishes a GitHub
  Release with AAR, XCFramework, and npm package artifacts.
- Crate-level READMEs and crates.io metadata (`readme`, `keywords`,
  `categories`, `homepage`) for all publishable crates.
- A rendered hero image in the README, produced by the desktop example from
  Wakufactory's CC0 Kitsune scene, with source and checksum provenance.
- Explicit PLY input/header/vertex/property/decoded-scene budgets with checked,
  fallible scene allocation.
- Pixel readback conformance with a tolerant 64x64 `SortedAlpha` image baseline.
- `cargo-deny` policy for advisories, licenses, duplicate versions, and source
  registries.
- A maintainer release checklist and media provenance documentation.

### Changed

- README now describes project status, quick start, verification, integration
  boundaries, contribution flow, security policy, and licensing in one
  external-facing entrypoint, with CI/license/MSRV badges and a platform
  support matrix.
- `SECURITY.md` now points to GitHub Security Advisories as the only
  supported private reporting channel.
- Blank GitHub issues are disabled; reports are routed through the issue
  templates.
- Offscreen renderer construction now requires a real GPU rasterizer, while
  Android, Apple, and Web Surface paths use explicit Surface-only constructors.
- GPU dimension and instance-buffer limits return structured errors before
  `wgpu` resource creation; runtime raster failures are no longer reported as
  successful frames.
- Benchmarks now run in release mode and report CPU preparation, submission,
  GPU wait, and GPU-complete latency with adapter metadata and optional
  thresholds.
- GitHub Actions use least-privilege permissions and immutable action SHAs;
  tag releases include dependency policy and the 1800-second stability bar.

### Security

- All exported C ABI entrypoints catch Rust unwinds and convert error-code
  functions to `GSPLAT_ERROR_INTERNAL` with a thread-local detail message.
- Updated `crossbeam-epoch` to 0.9.20 for `RUSTSEC-2026-0204` and documented
  scoped upstream-blocked advisory exceptions in `deny.toml`.

### Fixed

- Made release version validation portable to stock GitHub-hosted runners by
  using POSIX `grep` instead of assuming `ripgrep` is preinstalled.
