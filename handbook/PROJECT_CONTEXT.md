# gsplat-rs Context

## Overview

- `gsplat-rs` is a cross-platform Gaussian Splatting rendering library built with Rust + `wgpu`.
- The repo serves three audiences: Rust library consumers, native integrators through the C ABI, and maintainers validating the stack through examples and tooling.
- The project is still on the `0.1.x` line, with a deliberately small release surface.
- The latest verified GitHub prerelease is `v0.1.3`; it provides checksum-listed
  Android AAR, Apple XCFramework ZIP, and Web npm-compatible tarball artifacts
  without publishing them to package registries.

## Canonical Docs

- Agent/project entrypoint: `../AGENTS.md`
- Architecture map: `ARCHITECTURE.md`
- Verification entrypoint: `VERIFICATION.md`
- Release process and remote settings gates: `../RELEASING.md`
- Direction and scope: `ROADMAP.md`
- Project taste guide: `GOLDEN_PRINCIPLES.md`
- Contribution guide: `../CONTRIBUTING.md`
- Code of conduct: `../CODE_OF_CONDUCT.md`
- Security policy: `../SECURITY.md`

## Success Criteria

- The workspace builds and tests cleanly on the supported CI paths.
- `SortedAlpha` remains the only quality-guaranteed render mode.
- FFI smoke paths and mobile smoke integrations stay working.
- Untrusted PLY input and the experimental SPZ loader fail with bounded,
  structured errors before unchecked allocation.
- Production Packed rendering keeps the complete source point set and source
  SH0-SH3 degree; an inadmissible scene fails before publication rather than
  sampling, paging, or silently lowering quality.
- Desktop and mobile examples remain validation surfaces for the shared crates, not separate product lines.

## Current Repository Shape

- `crates/gsplat-core`: shared public types, config, stats, and error codes
- `crates/gsplat-io-ply`: bounded/incremental PLY parsing, wide Direct scene
  construction, and one-splat-at-a-time visitors for compact Resident loading
- `crates/gsplat-io-spz`: experimental bounded SPZ v4 parsing and scene buffer construction
- `crates/gsplat-sort`: stable CPU radix ordering and shared sort utilities
- `crates/gsplat-render-wgpu`: exact-count compact Resident scenes, CPU/GPU
  ordering, measured Adaptive selection, shared Surface/offscreen rendering,
  the Direct-f32 oracle, the explicit diagnostic Paged runtime, and a completed
  Exact prepared-plan core consumed by product Packed offscreen and Surface
  rendering
- `crates/gsplat-ffi-c`: small C ABI surface over the renderer and mobile Surface presenters
- `crates/gsplat-web`: experimental `wasm-bindgen` bindings over the shared `wgpu` Surface renderer
- `examples/desktop`: desktop viewer and offscreen PNG harness
- `examples/android`: Kotlin Android Surface sample app
- `examples/ios`: UIKit realtime Surface sample app
- `examples/web`: browser PLY loader, generated wasm package host, and WebGL2 SortedAlpha-style fallback preview
- `bindings/android`: local `gsplat-android` Android library module, JNI bridge, host-side JNI smoke, and AAR/APK scripts
- `bindings/apple`: local `GsplatKit` Swift package wrapper, Swift smoke path, XCFramework scripts, and iOS simulator/device build/run scripts
- `packages/web`: local `@gsplat-rs/web` browser ESM wrapper
- `tools/bench-runner`: perf and stability runner
- `tests/`: dataset manifests plus FFI, benchmark/competitive, release, and dependency-policy scripts
- `handbook/`: current project docs, architecture map, verification guide, roadmap, and project principles
- `docs/plans/`: task-scoped active and completed planning bundles
- `docs/media/`: rendered images referenced by the README
- `.github/`: CI and release workflows, issue and pull request templates, CODEOWNERS, and Dependabot config

## Common Commands

```bash
cargo check --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
```

For the broader command matrix, use `VERIFICATION.md`.

## Current Focus

- Package E's Exact core and Package M's native offscreen, shared Surface,
  C/Web/Android/Apple consumer migrations are integrated. M7's obsolete
  Packed/Tiled deletion and compatibility work are implemented, while its
  remaining renderer/session/presenter ownership closeout is Active. M8 remains
  unstarted.
- Keep the day-to-day verification paths passing and the release bar lightweight but real.
- Expand conformance and perf coverage with real datasets before widening the public API surface.
- Keep the exact-count Resident path, portable GPU visibility/radix/indirect
  draw, default exact projected-quads raster with strictly invalidated
  stationary-frame cache reuse, and runtime CPU/GPU Adaptive policy covered by
  real-scene evidence.
- Keep Direct-f32 and the retained GlobalQuads compatibility path as explicit
  oracles where they still have consumers. Product Packed uses the canonical
  ProjectedQuadsExact raster; the former TiledExact path is deleted. Promote
  raster optimizations only when they preserve complete membership, SH,
  resolution, order, draw count, and the image gate.
- Improve mobile integration only while the shared C ABI stays simple and stable.
- Turn Android integration into a local AAR/module shape before widening it into a published SDK.
- Harden the local iOS `GsplatKit`/XCFramework slice before treating it as a published SwiftPM binary SDK.
- Harden the local Web `@gsplat-rs/web` wrapper around the shared Rust `wgpu` Surface renderer before treating it as a published npm SDK.
- Keep validated in-memory `SceneBuffers` as the Direct reference owner and
  use direct-to-Resident PLY loading for production Packed. Retain the
  fixed-slot local Paged runtime only as an explicit non-full-quality
  diagnostic.
- Keep release checks reproducible: pinned CI actions, checksum-verified policy tooling, version consistency, and GPU-backed conformance evidence.
- Update the docs immediately when repository structure or responsibilities change.
- Keep contributor-facing maintenance files aligned with the actual verification and release boundary.

## Constraints and Boundaries

- `SortedAlpha` is the only release-gated render path right now.
- Native offscreen `Renderer::new`/`Renderer::with_config` require a usable GPU;
  Surface integrations use the surface-only constructors and acquire the device
  through `SurfacePresenter`.
- Default PLY loading is bounded by `PlyLoadLimits`; callers that intentionally
  need a different budget must opt into the limit-aware APIs.
- The current C ABI intentionally stays small and does not yet cover scene-from-memory loading or runtime render-mode switching.
- Android Surface integration now has a local `gsplat-android` library module
  that builds an AAR, but it is not Maven-published or a broad Android product
  API yet. iOS integration now has a local `GsplatKit` Swift package wrapper
  and local `GsplatFFI.xcframework` build path, but it is not a published
  binary SwiftPM release or polished iOS product API yet.
- Native Surface handles are single-owner handles. Public wrappers serialize
  access before calling the C ABI; direct C or JNI integrations should use the
  same one-thread-or-queue ownership rule.
- Web, desktop interactive, Android, and iOS Surface clients delegate frame
  cadence, camera revisions, CPU/GPU ordering, Adaptive probes, ticketed GPU
  timing, and presentation to the shared `SurfaceRenderSession`. Product
  examples and SDK wrappers select full-resident Packed with sort interval 1
  and Adaptive ordering; Direct remains the explicit wide-f32 oracle.
- The Exact core under `renderer/`, `plans/`, `cpu/`, `gpu/` and `raster/`
  prepares CPU PostSort, GPU PostSort and GPU Preproject as complete plans and
  has one whole-plan controller. Product Packed offscreen and Surface rendering
  consume this runtime. Surface target mechanics live in
  `SurfacePresenterHost`; renderer semantics and evidence remain in the sole
  `PreparedRuntimeSlot`.
- Existing low-level Surface constructors and `GeometryPath::default()` stay
  Direct for compatibility. Packed and Paged are explicit selections, but only
  Packed can emit the production exactness receipt. An oversized scene is
  never silently promoted into Paged.
- Local paging decodes page payloads behind `LocalScenePageSource` before
  fixed-slot GPU upload. The adapter still borrows the complete `SceneBuffers`,
  page metadata still stores source indices, and scheduling is synchronous, so
  this is an architecture seam rather than end-to-end streaming.
- Packed PLY loading is end-to-end incremental into the final resident planes.
  SPZ still constructs wide `SceneBuffers` before Resident conversion and has
  a correspondingly higher documented CPU peak.
- The Web example is a browser validation surface. The Rust/WASM renderer boundary
  is active in `crates/gsplat-web`, and `packages/web` provides
  a local ESM wrapper, but the Web SDK is not published to npm or stable in the
  v0.1 contract yet.
- The bounded SPZ v4 loader is isolated in `crates/gsplat-io-spz`; no C, Web,
  mobile, or default application entrypoint consumes it yet.
- Input PLY quaternion fields `rot_0..3` are interpreted as `w,x,y,z` and remapped internally to `x,y,z,w`.
- Input 3DGS coordinates are treated as `RDF` and converted at load time to runtime `RUF`, including quaternion and SH sign transforms.

## Known Open Gaps

- Exact-core migration closeout: legacy Packed presenter ownership and
  TiledExact are deleted, the platform consumers use the shared session, and
  the completed M7 behavior range has aggregate rollback evidence. M7 remains
  Active until the `lib.rs`, `surface_session.rs` and `surface_presenter.rs`
  ownership obligations are actually satisfied. Chrome/WebGPU at the final M7
  SHA, physical iPhone, and Windows/Linux runtime remain explicit Deferred;
  M8 remains unstarted.
- Android external distribution: the GitHub prerelease attaches an AAR, but it
  is not published to Maven and the current package slice is `arm64-v8a` only.
- iOS external distribution: the GitHub prerelease attaches an XCFramework ZIP,
  but `GsplatKit` is not a remote binary SwiftPM package.
- Web external distribution: the GitHub prerelease attaches an npm-compatible
  tarball, but `@gsplat-rs/web` is not published to npm or treated as a stable
  v0.1 public API.
- SPZ product integration: the loader is tested, but choosing where it enters
  desktop, C, mobile, or Web APIs remains a separate product decision.
- Device runtime evidence: the latest validation covered Android APK/AAR build,
  Android true-device launch and benchmark (an Android test device, flowers
  dataset), iOS simulator app launch, iOS simulator smoke, iOS device app
  build/sign, and iOS physical-device benchmark (iPhone 17 Pro Max, flowers
  dataset).

## Notes

- Keep this file factual and current.
- Put transient task detail under `docs/plans/active/`, not here. Move
  completed task history to `docs/plans/completed/`.
