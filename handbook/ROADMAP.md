# Roadmap

This file defines the current direction and release boundary for `gsplat-rs`.
Operational facts and command entrypoints live in `handbook/PROJECT_CONTEXT.md` and `handbook/VERIFICATION.md`.

## Project Position

- `gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust + `wgpu`.
- The project is on the `0.1.x` line and should stay small until the core render path is more thoroughly validated.
- `SortedAlpha` is the only quality-guaranteed render mode.
- Desktop and mobile examples are validation surfaces for shared crates. Android
  now has a local library/AAR packaging slice, and iOS has a local
  `GsplatKit`/XCFramework packaging slice, but neither is a published product
  SDK.
- Web has a local `@gsplat-rs/web` ESM wrapper over the experimental Rust/WASM
  renderer, but it is not a published npm SDK.
- Tagged `0.1.x` prereleases may attach direct AAR, XCFramework ZIP, and
  npm-compatible tarball artifacts to GitHub Releases. These artifacts are a
  source-integration aid, not Maven, binary SwiftPM, npm, or crates.io
  publication and do not widen the stable API boundary.

## Near-Term Priorities

1. Keep the bounded PLY -> `SceneBuffers` -> Direct renderer path correct and well tested.
2. Move Direct toward capability-gated GPU visible compaction, portable radix
   sorting, and indirect drawing without forking platform renderers.
3. Expand conformance and performance evidence across real scenes and
   representative desktop/mobile resolutions, including stage timings and
   image-quality comparisons, before widening APIs or making competitor claims.
4. Keep C ABI, JNI, Android packaging, Apple packaging, and Web wrappers small,
   boring, and synchronized around the shared renderer lifecycle.
5. Decide SPZ consumer integration separately from its isolated loader; do not
   widen C/Web/mobile APIs merely because the parser exists.
6. Keep handbook docs and verification commands aligned with the repository that actually exists.

## Current Release Boundary

- The public contract is centered on PLY import, in-memory scene buffers, `SortedAlpha` rendering, and the small C ABI.
- Experimental Rust APIs may exist only when they stay out of the release contract and do not complicate verification.
- Any backend that requires matched training metadata stays disabled by default until promoted here.
- The current C ABI intentionally stays small:
  - `gsplat_version_major`
  - `gsplat_version_minor`
  - `gsplat_error_message`
  - `gsplat_last_error_message`
  - `gsplat_config_default`
  - `gsplat_camera_default`
  - `gsplat_context_create`
  - `gsplat_context_destroy`
  - `gsplat_context_set_camera`
  - `gsplat_context_set_auto_camera`
  - `gsplat_context_load_scene_path`
  - `gsplat_context_render_frame`
  - `gsplat_context_get_stats`
  - Android and iOS Surface renderer create/resize/camera-control/render/stats/destroy functions for the example integration paths
- The current C ABI does not cover scene-from-memory loading or runtime render-mode switching.
- Stable v0.1 semantics are the bounded PLY-to-`SceneBuffers` path, offscreen
  context lifecycle and structured errors, `SortedAlpha` direct rendering, and
  single-owner native handles used from one serialized thread or queue.
- Packed/paged geometry selectors, atlas layouts, local page scheduling,
  benchmark artifact schemas, Web package APIs, and mobile Surface convenience
  wrappers remain experimental. They may change without widening the stable
  v0.1 contract; direct remains their default.
- `crates/gsplat-io-spz` is an experimental, bounded import component. It is
  not yet connected to the stable C, Web, mobile, or default application path.
- Native handles are single-owner handles and should be used from one serialized
  thread or queue. Public wrappers may add locking, but this does not make the
  raw C ABI a free-threaded API.
- Mobile Surface functions are validation example support, not a commitment to a
  full mobile product API. The local Android AAR wraps the same C ABI for
  starter consumption, and the local iOS `GsplatKit` wrapper packages the same
  C ABI for Swift consumption. Maven publishing, multi-ABI Android
  distribution, published binary SwiftPM/XCFramework distribution, and polished
  mobile view APIs are still outside the current release contract.
- `crates/gsplat-web` plus `packages/web` form the local
  experimental Web API boundary. They are not a stable v0.1 release surface;
  Web renderer changes require verified wasm build and browser smoke evidence.
- The Web example is validation example support for browser PLY loading, the WebGL2 fallback, and hosting the generated wasm package; it is not a polished web product surface.

## Packed/Paged Evidence Boundary (closed 2026-07-21)

- Packed remains an explicit experimental geometry path. Its retained value is
  the 20-byte hot record, resource preflight, image/count gates, and a reusable
  resource layout—not an automatic replacement for Direct.
- The unread 48-byte-per-splat SH GPU texture, its full-scene CPU staging, and
  the fictitious hot-texture dimension gate were removed. Packed now evaluates
  view-dependent color into its hot record, completes that color before the
  first presented frame, and freezes one camera across a banded refresh.
- The fixed four-slot local Paged runtime remains available for explicit
  diagnostics, but further productization is frozen. It retains complete
  `SceneBuffers` and source-index metadata and performs synchronous scheduling,
  extraction, packing, sorting, and color work; it is neither end-to-end
  streaming nor evidence of arbitrary-scale or memory-bounded loading.
- Unused automatic Surface constructors were removed. Capacity preflight can
  report that Direct does not fit, but the library does not silently select
  the local Paged prototype as product policy.
- Historical physical A065 evidence recorded Direct drawing 279,199 splats at
  11.330 ms/frame and Paged drawing 225,784 active splats at 23.626 ms/frame.
  This proves Paged execution and bounded GPU slots, not a performance win.
- Historical five-pair Chrome/WebGPU Kitsune-static evidence at 640×480
  reported a gsplat-rs/PlayCanvas frame-wall p95 median ratio of `1.0200` and
  minimum SSIM `0.998657`. It predates this closeout commit and does not prove
  broad browser/native leadership, competitor memory leadership, sustained
  thermal behavior, or 10M scalability.
- A future streaming track must start from metadata-first loading, bounded
  compressed/decoded caches, asynchronous decode, spatial hierarchy/LOD, and
  measured source/CPU/GPU residency. It should not grow out of the current
  four-slot prototype by terminology alone.

## Release Bar

- The canonical day-to-day verification set lives in `handbook/VERIFICATION.md`.
- The complete manual and remote-settings gates live in `RELEASING.md`.
- Before cutting a release, also run:

```bash
RELEASE_VERSION=<major.minor.patch> bash tests/release/check-version.sh
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Open Release Gaps

- Publishable Android SDK: add Maven publishing, multi-ABI packaging, and a
  higher-level Android view/API only after the current C ABI wrapper remains
  stable under device validation.
- Publishable iOS SDK: add a remote binary SwiftPM/XCFramework distribution and
  polished iOS product API only after the local `GsplatKit` slice is stable.
- Publishable Web SDK: publish `@gsplat-rs/web` to npm only after the WASM
  renderer has browser smoke evidence across target browsers and the package
  API is promoted into the release contract.
- Device proof: keep Android true-device launch and iOS physical-device
  launch/benchmark as explicit release evidence, not implied by local
  APK/app build success.

## Explicitly Not Active Right Now

- A custom internal binary scene/cache format
- Further optimization of the fixed four-slot local Paged prototype as a
  primary performance track
- Metadata-first or remote streaming before the Direct GPU pipeline and
  real-dataset evidence matrix are established
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
- Published Maven, binary SwiftPM, or npm SDK distribution
