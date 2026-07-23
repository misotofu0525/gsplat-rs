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

1. Keep bounded PLY import, the Direct-f32 oracle, and direct-to-Resident
   exact-count Packed loading correct and well tested.
2. Keep portable GPU visibility/radix/indirect draw and measured CPU/GPU
   Adaptive selection shared across platforms; next compare complete GPU
   execution plans (`PostSort+Candidate`, `PostSort+Compact`, and
   `Preproject+Compact`) before the outer CPU/GPU choice. Do not replace either
   decision with a fixed point-count threshold.
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
- Packed/Direct/Paged geometry selectors, Resident layouts, local page scheduling,
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

## Full-Count Resident Evidence Boundary (completed 2026-07-23)

The exact resident representation, Direct-oracle quality gate, default exact
projected-quads raster, lazy exact tiled diagnostic, terminal Adaptive
evidence, and available-endpoint cross-platform evidence are in place.
Historical global-quad timings below are context, not closeout evidence. The
completed design and experiments live under
`docs/plans/completed/2026-07-22-full-quality-native-rendering/`.

- Product examples and SDK wrappers explicitly select Packed, whose
  implementation is now one exact-count compact Resident scene rather than the
  old 20-byte partial atlas. Direct remains the wide-f32 image oracle and the
  low-level compatibility default.
- Packed keeps exact f32 position/alpha/world covariance, compact DC, complete
  source SH0-SH3, resolved RGB18E8 high-dynamic-range color, and one source ID per point. A 256-point chunk
  is only a color-quantization metadata unit; it is never a draw/residency cap.
- Direct-f32 comparisons on two views each of complete Truck, Garden, and
  Bicycle pass the fixed SSIM/RGB/alpha gate. The 6.132M Bicycle model is a
  required large-scene proof, not an extrapolation from small showcase assets.
- CPU and GPU order share exact visibility/depth/tie semantics and one
  authoritative visible order. The default raster projects each visible splat
  once into two 16-byte-per-splat planes and then uses exact hardware-instanced
  SortedAlpha quads; CPU supplies the direct count and GPU reuses the sorter's
  indirect count without readback. Adaptive uses paired measurement,
  hysteresis, cooldown, and periodic re-probe; it does not hard-code a
  scene-size switch.
- The production diagnostic can A/B PostSort against exact Preproject in the
  same binary. Complete-Truck cohorts preserve byte-identical rendered output
  while Preproject lowers the measured GPU-plan completion cost on M4 Metal,
  Chrome/WebGPU, and the Nothing A065. PostSort remains the universal product
  default because Preproject currently requires Packed + ProjectedQuadsExact +
  Compact, owns an additional lazy graph, and lacks producer-level
  Adaptive/fallback. Current Adaptive learns CPU versus GPU ordering, not the
  producer axis.
- Projected cache reuse is fail-closed: only an identical order generation and
  owner, complete camera, viewport, and draw-count guard may skip projection.
  Motion, refresh, CPU/GPU transition, resize, or count change recomputes the
  exact cache before drawing.
- GlobalQuads remains an exact Resident oracle. TiledExact is lazy and
  diagnostic; neither plan is allowed to sample, lower SH, change resolution,
  or reduce the authoritative visible draw count.
- Packed preflight accounts for the final degree-specific planes, eight
  color-resolve storage bindings, binding/buffer limits, and u32 draw
  addressability. Validation/OOM/internal failures reject before publication;
  forced GPU never silently falls back.
- The fixed four-slot local Paged runtime remains available for explicit
  diagnostics, but further productization is frozen. It retains complete
  `SceneBuffers` and source-index metadata and performs synchronous scheduling,
  extraction, packing, sorting, and color work; it is neither end-to-end
  streaming nor evidence of arbitrary-scale or memory-bounded loading.
- Capacity preflight or allocation failure never silently selects the local
  Paged prototype, samples points, or lowers SH degree.
- Complete SH3 Garden (5.835M) and Bicycle (6.132M) also load and render on the
  physical A065 at native 2412x1080 with exact five-stage counts and no quality
  fallback. Their short runs are capacity evidence, not interactive-FPS or
  sustained-thermal claims.
- Historical physical A065 evidence recorded Direct drawing 279,199 splats at
  11.330 ms/frame and Paged drawing 225,784 active splats at 23.626 ms/frame.
  This proves Paged execution and bounded GPU slots, not a performance win.
- Historical five-pair Chrome/WebGPU Kitsune-static evidence at 640×480
  reported a gsplat-rs/PlayCanvas frame-wall p95 median ratio of `1.0200` and
  minimum SSIM `0.998657`. It predates this closeout commit and does not prove
  broad browser/native leadership, competitor memory leadership, sustained
  thermal behavior, or 10M scalability.
- A future remote/hierarchical streaming track must start from metadata-first loading, bounded
  compressed/decoded caches, asynchronous decode, spatial hierarchy/LOD, and
  measured source/CPU/GPU residency. Any LOD mode must be separately labeled
  and cannot masquerade as this full-quality contract.

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
- Metadata-first remote hierarchy/LOD without a separately labeled quality
  contract and its own real-dataset evidence matrix
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders
- Published Maven, binary SwiftPM, or npm SDK distribution
