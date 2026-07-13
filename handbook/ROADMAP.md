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

1. Keep the PLY import -> `SceneBuffers` -> renderer path correct and well tested.
2. Expand conformance and performance coverage with real datasets before widening APIs.
3. Keep C ABI, JNI, Android library packaging, iOS local XCFramework packaging, and Swift smoke paths boring, small, and in sync.
4. Improve renderer quality and stability inside the existing crate boundaries.
5. Harden the local Web wrapper and Rust/WASM renderer target behind the shared `wgpu` Surface path before calling Web parity complete.
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

## Phase D-F Evidence Boundary (2026-07-13)

- The experimental local-source paged path has fixed four-slot active
  residency, eviction, deterministic continuity and stale/cancel/generation
  tests, direct-vs-paged offscreen parity, non-zero Surface output, and a short
  bounded steady-state/device smoke. It is not production network streaming.
- Five sequential randomized-order Chrome/WebGPU pairs pass the Kitsune-static
  desktop parity band at 640×480: gsplat-rs/PlayCanvas frame-wall p95 median
  ratio `1.0200` (bootstrap 95% CI `1.0100-1.0200`), p99 median `1.0291`
  (`1.0097-1.0388`), and minimum SSIM `0.998657`.
- That result is deliberately scoped to the measured local Chrome/WebGPU,
  Kitsune-static profile. It does not establish native leadership, broad
  browser or dataset parity, competitor memory leadership, sustained
  thermal/energy behavior, or 10M scalability.
- Local AAR, XCFramework/Swift Package, and npm-compatible tarball consumption
  paths are qualification surfaces only. Registry publication and a widened
  stable SDK contract remain outside the release boundary.

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
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
- Published Maven, binary SwiftPM, or npm SDK distribution
