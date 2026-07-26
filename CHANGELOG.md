# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added the internal Exact prepared-plan core used by the product Packed route:
  complete CPU PostSort, GPU PostSort, and GPU Preproject plans, one canonical
  projected-quads `SortedAlpha` raster, transactional publication, and
  renderer-owned current-stats receipts.
- Added direct-to-Resident incremental PLY loading for exact-count Packed
  rendering while retaining complete source membership and SH0-SH3 degree.
- Added additive experimental Surface evidence APIs across C, Web, Android,
  and Apple. Pending GPU `S/V/C/D` values remain unavailable until a matching
  renderer-owned terminal rather than being reported as zero, capacity, or a
  stale frame.
- Added an experimental, bounded Niantic SPZ v4 loader with cancellation,
  coordinate/SH conversion, and isolated source-residency helpers.
- Added versioned benchmark artifacts, dataset manifests, a shared camera
  trace contract, extraction tools for Web/Android/iOS, and a pinned
  PlayCanvas comparison harness with paired statistics and SSIM checks.
- Added the fixed four-slot local Paged geometry path as an explicit
  partial-residency diagnostic; capacity failure never selects it
  automatically.

### Changed

- M1 routed native Packed offscreen rendering, the non-interactive desktop
  path, and bench-runner through one transactional Exact runtime, with Direct
  retained as the image oracle.
- M2 moved the shared real-window Surface path onto that Exact runtime. Desktop,
  Web, Android, and Apple product entrypoints explicitly select Packed with
  Adaptive ordering; `GeometryPath::default()` and low-level compatibility
  constructors remain Direct.
- M3 made `SurfaceRenderSession` the owner of compatibility submissions,
  terminals, ticket counts, and producer evidence. The C ABI translates that
  state without a second queue, ticket ledger, or policy owner, while its
  existing v0.1 layouts and signatures remain intact.
- M4 migrated the experimental Rust/WASM consumer to
  `SurfaceRenderSession -> SurfacePresenterHost -> PreparedRuntimeSlot`.
  Packed URL/File/stream loading writes directly into Resident planes and
  exact benchmark counts come from matching renderer-owned current-stats
  terminals.
- Made sampled WebGL2 an explicit diagnostic opt-in rather than an automatic
  product fallback.
- M5 migrated the Android/JNI/AAR consumer and strict current-stats accounting
  to the shared route. Its retained Nothing A065 forced-CPU, forced-GPU, and
  Adaptive runs are separate functional/directional observations, not a
  general performance or device claim.
- M6 migrated the Apple/GsplatKit/XCFramework consumer and functional evidence
  path. Simulator/host-side results do not substitute for a physical-iPhone
  run.
- M7 completed the renderer/session/presenter ownership split, leaving one
  product Packed Surface route and standalone Direct/Paged compatibility
  owners. It preserved classified Rust/C/Web/mobile compatibility entrypoints
  and the small stable v0.1 boundary.

### Removed

- Removed the obsolete `static_direct` / `sortedIndexDirect` selectors while
  retaining Direct as the wide-f32 oracle and compatibility default.
- Removed the TiledExact implementation and public variant.
- Removed the unreachable standalone Packed presenter graph and the duplicate
  C-side Surface receipt queues, ledgers, and producer-state mirrors.

### Fixed

- Removed an unread 48-byte-per-splat Packed SH GPU texture, the unconsumed
  CPU sidecar staging on the GPU hot-record path, and obsolete texture-dimension
  preflight so valid storage-buffer scenes are not forced into Paged.
- Ensured Packed Surface rendering applies complete view-dependent color on
  its first frame and uses one frozen camera across every band of a refresh.
- Made default Web renderer failure fail closed; the sampled WebGL2 diagnostic
  is available only with `gsplat_allow_sampled_webgl=true` and cannot satisfy
  full-quality or performance evidence.

The Unreleased work does not widen the stable v0.1 contract. Packed/Direct/
Paged selectors, Resident layouts, Web package APIs, mobile Surface convenience
wrappers, and benchmark schemas remain experimental. Distribution remains
limited to direct GitHub prerelease AAR, XCFramework ZIP, and npm-compatible
artifacts rather than Maven, binary SwiftPM, npm, or crates.io publication.
Chrome/WebGPU at the accepted M7 SHA, physical iPhone, and Windows/Linux runtime
remain deferred; no performance, competitor, power, thermal, or broad
cross-platform claim is made here.

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
