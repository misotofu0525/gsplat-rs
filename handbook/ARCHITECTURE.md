# gsplat-rs Architecture

## Purpose

- This file is a concise architectural map for humans and agents.
- Keep it factual and update it when responsibilities or critical flows move.

## System Boundaries

- The repository owns scene loading, sort backends, `wgpu` rendering, a small C ABI, experimental Rust/WASM Web bindings, and validation examples/tools around those pieces.
- The repository does not own model training, a polished web product surface, or multiple polished render backends.
- Main external dependencies are `wgpu`, platform toolchains for Swift/JNI validation, Android/iOS SDK tooling for mobile container builds and local package artifacts, browser WebGL2 for the static Web example, and `wasm-bindgen`/browser ESM tooling for the experimental Web SDK path.

## Runtime Topology

- Core data types live in `crates/gsplat-core`.
- Stable scene import starts in `crates/gsplat-io-ply`; the isolated
  experimental SPZ v4 loader lives in `crates/gsplat-io-spz`.
- Sorting lives in `crates/gsplat-sort`.
- Rendering and GPU-facing orchestration live in `crates/gsplat-render-wgpu`.
  `lib.rs` owns renderer/public entrypoints, `resident_scene.rs` owns the
  exact-count compact CPU encoding, `resident_gpu.rs` owns its GPU planes and
  coherent SH resolve, `direct_gpu_order.rs` owns portable GPU
  visibility/radix/indirect draw, `gpu_telemetry.rs` owns ticketed completion,
  `projected_quads_gpu.rs` owns the default exact projection cache and
  instanced draw, `preproject_gpu.rs` owns the lazy diagnostic direct
  contributor producer, and `gpu_producer_telemetry.rs` owns its independent
  A/B receipts. `tiled_resident_gpu.rs` is the lazy exact tiled oracle, while
  `surface_presenter.rs`/`surface_session.rs` own presentation and runtime
  CPU/GPU/Adaptive plus Packed GPU-producer selection. Paged files remain an
  explicit diagnostic seam.
- Native embedding goes through `crates/gsplat-ffi-c`.
- Browser WebAssembly embedding goes through `crates/gsplat-web`.
- Runtime validation entrypoints are `examples/desktop`, `examples/android`,
  `examples/ios`, `examples/web`, and `tools/bench-runner`; release and
  dependency-policy checks live under `tests/release/` and `tests/security/`.

## Key Directories

- `crates/`: reusable library crates, the C ABI, and the experimental Web bindings
- `examples/desktop/`: desktop viewer and offscreen output harness
- `examples/android/`: Android Surface sample app
- `examples/ios/`: UIKit realtime Surface sample app
- `examples/web/`: static frontend example for browser PLY loading, WebGL2 point-splat fallback, and generated wasm package hosting
- `bindings/android/`: Android `gsplat-android` library module, JNI bridge, host smoke entrypoint, and AAR/APK scripts
- `bindings/apple/`: local `GsplatKit` Swift package wrapper, Swift smoke source, XCFramework scripts, and iOS simulator/device build/run scripts
- `packages/web/`: local `@gsplat-rs/web` ESM wrapper over generated wasm-bindgen output
- `tools/`: CLI tools for performance validation
- `tests/`: dataset identities plus FFI, benchmark/competitive, release, and dependency-policy scripts
- `handbook/`: current project docs, architecture map, verification guide, roadmap, and project principles
- `docs/plans/`: active and completed task planning bundles
- `.github/`: CI workflows and contributor templates

## Critical Flows

- PLY render flow:
  starts at external `.ply` data or `tests/datasets/minimal_ascii.ply`
  passes through the bounded default or explicit `PlyLoadLimits` APIs in
  `crates/gsplat-io-ply/src/lib.rs`
  continues into `crates/gsplat-render-wgpu/src/lib.rs`
  is exercised by `examples/desktop/src/main.rs`, `tools/bench-runner/src/main.rs`, and `crates/gsplat-ffi-c/src/lib.rs`

- Experimental SPZ import flow:
  starts from plaintext-header Niantic SPZ v4 bytes
  passes through bounded/cancellable APIs in `crates/gsplat-io-spz/src/lib.rs`
  converts RUB positions, rotations, and SH data into runtime RUF
  produces the same validated `SceneBuffers` shape as PLY, but currently has
  no C, Web, mobile, or default application consumer

- Renderer construction flow:
  native offscreen `Renderer::new` and `Renderer::with_config` acquire a GPU
  adapter/device and fail when rasterization cannot be created
  Surface clients use `Renderer::new_for_surface` or
  `Renderer::with_config_for_surface`, then let `SurfacePresenter` acquire the
  adapter/device compatible with the platform surface
  render dimensions are checked before `wgpu` resource creation, and GPU
  submission/wait failures remain structured errors
  low-level Surface constructors and `GeometryPath::default()` remain Direct
  for compatibility; product examples and SDK wrappers explicitly select
  exact-count Packed, while Direct is the wide-f32 oracle and Paged is
  diagnostic only
  resource preflight reports capacity without silently changing geometry,
  membership, SH degree, or ordering backend

- Shared Surface frame flow:
  `SurfaceRenderSession` in
  `crates/gsplat-render-wgpu/src/surface_session.rs` owns `Renderer`,
  `SurfacePresenter`, camera revisions, CPU/GPU order state, Adaptive probes,
  ticketed measurements, and frame statistics
  changed-camera frames advance the default interval schedule; identical
  redraws do not repeatedly sort
  Direct keeps wide scene-derived positions, covariance, opacity, DC, and SH
  GPU-resident as the f32 image oracle
  production Packed keeps every source point in planar Resident GPU buffers:
  exact f32 position/alpha/covariance, compact DC and complete source SH0-SH3,
  resolved RGB18E8 high-dynamic-range color, and source IDs
  camera-position changes run one coherent all-point SH resolve; rotation-only
  changes can reuse color
  CPU ordering uploads compact sorted IDs; GPU ordering generates exact
  visibility and stable order on the GPU and draws through indirect arguments
  the qualified Packed GPU path remains the default post-sort producer; an
  explicit diagnostic Preproject selector instead projects every source,
  compacts exact contributors in source order, and stable-full32-sorts only
  that contributor prefix. It is lazy, transactionally published, restricted
  to forced Compact projected draws, and does not replace CPU/GPU/Adaptive
  both CPU and GPU order backends feed the same authoritative visible order into the
  default `ProjectedQuadsExact` plan: one compute projection per visible splat
  writes two separate 16-byte rank-indexed planes, followed by a four-vertex
  `TriangleStrip` hardware-instanced premultiplied-alpha draw; CPU order
  supplies an exact direct count and GPU order reuses the sorter's exact
  indirect count without readback
  Adaptive compares CPU and GPU with the same `FrameCompletion` interval,
  measured from frame start through queue completion; that interval includes
  ordering, projection, rasterization, submission, and queued GPU work
  order-stage timestamps remain diagnostics and never drive backend selection;
  changing the raster execution plan resets Adaptive learning because samples
  from different raster plans are not comparable
  the projected planes persist across frames and are reused only while order
  ownership/generation, complete camera, viewport, and draw-count guard remain
  identical; any order refresh, CPU/GPU transition, camera change, resize, or
  count change invalidates them before the next draw
  `GlobalQuads` remains an exact Resident image/performance oracle and
  `TiledExact` is allocated lazily as an exact diagnostic, never as a capacity
  fallback; all plans preserve source membership, SH, resolution, and order
  every acquired swapchain image executes the selected exact draw; a stationary
  Projected frame may reuse its already exact rank-indexed projection instead
  of recomputing identical values
  the experimental paged branch instead schedules a fixed four-slot local
  active atlas in the presenter, densely packs stable spatial cell order across
  page boundaries, pins one globally sampled source-index-disjoint cover page
  when total pages exceed the slot budget, and uses the remaining slots for
  view-ranked refinements; it sorts only resident entries and reuses the packed
  shader for Surface draws, while paged frame stats expose active draw against
  loaded source count so fixed residency remains visible to clients
  Packed Surface construction preflights the actual final planes and eight
  color-resolve storage bindings plus two projected-cache bindings before
  device allocation; each projected plane is `16 * splat_count` bytes so the
  portable 128 MiB per-binding limit reaches 8,388,608 splats; validation/OOM/
  internal scopes publish resources only after complete success
  successful Surface handoff releases Packed upload staging while retaining
  exact CPU positions for CPU/Adaptive sorting; failed handoff is transactional
  `LocalScenePageSource` extracts and packs one transient decoded payload at a
  time, while `PagedAtlasGpu` consumes only that payload; the local adapter
  still depends on fully resident `SceneBuffers` for source data, global sort,
  and view-dependent color refresh, so this path is not end-to-end streaming

- Native integration flow:
  starts from C, Swift, or Kotlin/JNI host entrypoints
  crosses `crates/gsplat-ffi-c/include/gsplat.h` and `crates/gsplat-ffi-c/src/lib.rs`
  maps active v0.1 controls onto `SurfaceRenderSession`; additive experimental
  constructors can select Direct, full-resident Packed, or local Paged before
  scene derivation and Surface allocation; runtime geometry/backend setters
  preserve the old session when target preparation returns an error; native
  async CPU sorting and lazy GPU-order creation stay behind the shared session
  exposes exactness, adapter-limit, requested/actual backend, and completed GPU
  order receipts without moving scheduling policy into JNI or Swift; the
  versioned read-only camera receipt derives canonical matrices from the live
  f32 session camera and joins them to the current/presented revision
  keeps each native handle owned by one serialized thread or queue; wrapper
  APIs add their own locking, while direct C/JNI callers must provide the same
  serialization
  catches Rust panics at every exported C ABI entrypoint so no unwind crosses
  into foreign code

- Android Surface flow:
  starts at the local `bindings/android/gsplat-android` library module or
  sample `examples/android/app/src/main/kotlin/com/gsplat/example/MainActivity.kt`
  obtains a `SurfaceView` `Surface` and wraps it as an `ANativeWindow` in `bindings/android/jni/gsplat_jni.c`
  creates a raw-handle `wgpu::Surface` in
  `crates/gsplat-render-wgpu/src/surface_presenter.rs`
  presents directly to the Android swapchain, not through offscreen readback
  selects full-resident Packed plus Adaptive ordering in the product example;
  Direct remains available as an oracle and `gsplat_geometry_path=paged`
  remains an explicit non-full-quality diagnostic
  packages the selected build-time scene as `assets/showcase.ply` plus its source-name metadata, preferring the CC0 Kitsune scene and falling back to Flowers
  presents compact showcase telemetry while keeping the complete validation status behind the `Studio` control
  formal trace artifacts read the native post-present camera receipt for every
  measured frame and are checked against the separately supplied trace by
  index, timestamp, pose, intrinsics, matrices, Surface size, revision, and
  exact trace-file hash; trace JSON is not reused as runtime matrix evidence
  packages the JNI library through `bindings/android/gsplat-android` for local AAR builds

- iOS Surface flow:
  starts at the local `bindings/apple/GsplatKit` wrapper or sample `examples/ios/app/GsplatIOSExample.swift`
  obtains a UIKit `UIView` backed by `CAMetalLayer`
  selects `Documents/imported_scene.ply`, bundled `showcase.ply` with source-name metadata, or a generated minimal PLY
  passes the view through the additive constructor-time geometry entry while
  preserving `gsplat_surface_renderer_create_uikit` as the Direct default
  creates a raw-handle `wgpu::Surface` in
  `crates/gsplat-render-wgpu/src/surface_presenter.rs`
  presents directly to the simulator Metal surface, not through offscreen readback
  uses the same Kitsune-first editorial showcase and toggleable `Studio` diagnostics pattern as Android
  uses the same Surface camera-control and benchmark option functions exposed through the C ABI
  packages the C ABI as a local `GsplatFFI.xcframework` through `bindings/apple/scripts/build-xcframework.sh`

- Web WASM renderer flow:
  starts at browser JavaScript that imports the local `packages/web` wrapper or generated `gsplat-web` wasm package
  passes an `HtmlCanvasElement`, dimensions, and in-memory or streamed PLY
  chunks through `wasm-bindgen`
  production URL/File/custom-stream input feeds the incremental PLY decoder
  directly into `ResidentSceneBuilder`; it does not materialize a second wide
  WASM scene
  defaults to full-resident Packed plus Adaptive ordering while preserving an
  explicit Direct compatibility/oracle constructor and diagnostic Paged choice
  loads the scene into `gsplat-render-wgpu::Renderer`
  creates a browser canvas `wgpu::Surface` through `SurfacePresenter::from_canvas`
  hands both objects to `SurfaceRenderSession`, so the browser wrapper does not
  own a second frame scheduler or sorted-index copy
  prepares GPU-order resources, Direct/Packed runtime geometry changes, and
  production Packed + Projected Surface resizes through raw async wasm
  transactions; geometry CPU derivations and complete target GPU graphs stay
  unpublished until validation/OOM/internal scopes complete, changed-path
  synchronous setters fail closed, and Paged remains a constructor-time-only
  diagnostic; resize failure restores the old Surface configuration, while
  rollback failure makes presentation fail closed
  CPU and GPU order feed the same Resident draw path; Adaptive measures both
  using the shared policy
  every `renderFrame` drains all newly completed GPU receipts, which browser
  evidence joins to submitted frames by ticket and camera revision

- Desktop rendering flows:
  the interactive viewer uses the same `SurfaceRenderSession` as Web/mobile;
  it defaults to Packed/Adaptive and accepts forced CPU/GPU plus Direct oracle
  native offscreen rendering accepts Direct or Packed, reads back its texture,
  and supplies deterministic PNG/conformance comparisons
  CPU-projected `GpuInstance` values remain only as a reference oracle for
  conformance tests, not as a selectable runtime renderer

- Web example flow:
  starts at `examples/web/index.html`
  loads `examples/web/src/main.js`
  imports generated `examples/web/pkg/gsplat_web.js` when present, routes it
  through `packages/web/src/index.js`, and attempts the
  Rust/WASM Surface renderer first
  fetches or uploads a `.ply` file in the browser
  parses ASCII or binary PLY data into frontend buffers
  applies the same RDF-to-RUF Y-axis flip, DC color, and opacity conventions as the Rust import/render path
  CPU-sorts visible indices back-to-front and presents a WebGL2 point-splat preview
  exposes Android-style orbit/zoom/pan/reset camera controls and benchmark query parameters
  presents the default scene through a responsive showcase shell with loading progress,
  scene switching, local PLY upload, and collapsible diagnostics
  falls back to WebGL2 when the generated wasm package is missing or Surface creation fails

## Invariants

- `SortedAlpha` is the only release-gated path and the default mode expected by validation flows.
- The public C header and the Rust FFI implementation must stay in sync.
- Non-zero FFI returns should leave `gsplat_last_error_message()` with
  operation-specific detail for Swift/Kotlin/Web wrapper errors.
- Rust panics must not unwind across the C ABI boundary.
- Default PLY imports must enforce explicit byte, header, vertex, property, and
  decoded-scene budgets before allocation.
- An offscreen renderer must not report successful rendering without a real GPU
  raster path; Surface-only construction is explicit.
- Surface frame scheduling belongs in `SurfaceRenderSession`, not in Web, FFI,
  desktop, Android, or Apple wrapper-specific state machines.
- Production Packed keeps complete source membership and SH degree. CPU and
  GPU order must share visibility/depth/tie semantics, authoritative visible
  order, and the same exact raster contract; backend selection may not alter
  quality, resolution, residency, or draw count. Forced GPU errors are
  structured; only Adaptive may continue on CPU and later re-probe.
- Direct remains the wide-f32 oracle. Paged remains an explicitly selected
  partial-residency diagnostic and cannot emit a full-quality receipt.
- Formal full-quality evidence has one endpoint-specific resolution contract:
  desktop and Web use `1920x1080`, the Nothing A065 uses its native
  `2412x1080` Surface, and the iPhone 17 Pro simulator uses its native
  `2622x1206` drawable. Every accepted frame proves
  `requested = Surface = internal render = presented` pixels and preserves all
  source points, complete source SH degree, and authoritative membership.
  Sampling, LOD, dynamic resolution, and upscaling are forbidden. The Paged
  path and the sampled WebGL preview are smoke/diagnostic paths and cannot
  satisfy this contract; unsupported capacity fails explicitly.
- The SPZ loader is an experimental import component, not part of the v0.1 C,
  Web, or mobile integration contract.
- PLY input normalization is not optional: quaternion remapping and `RDF -> RUF` conversion happen at load time.
- Mobile examples are integration validators. Android and Apple packaging live
  under `bindings/`, but neither path is a published product SDK yet.
- `crates/gsplat-web` is the active experimental Rust/WASM target; Web renderer changes require the wasm build and browser smoke path before completion is claimed.
- The Web example stays a browser validator and generated wasm package host.
  The local Web package lives under `packages/web`, but it is not a published
  npm package.

## Hotspots

- `crates/gsplat-render-wgpu/src/resident_scene.rs`: exact-count compact planes,
  checked byte plans, codec reports, and adapter preflight
- `crates/gsplat-render-wgpu/src/resident_gpu.rs`: Resident uploads, coherent
  color resolve, and shared CPU/GPU-order draw bindings
- `crates/gsplat-render-wgpu/src/projected_quads_gpu.rs`: exact one-projection-
  per-visible-splat cache, CPU direct count, GPU indirect count, and instanced
  SortedAlpha drawing
- `crates/gsplat-render-wgpu/src/tiled_resident_gpu.rs`: lazily allocated exact
  tiled diagnostic and phase telemetry
- `crates/gsplat-render-wgpu/src/direct_gpu_order.rs`: exact GPU visibility,
  hierarchical scan, stable radix, and indirect draw
- `crates/gsplat-render-wgpu/src/surface_session.rs`: shared Surface lifecycle,
  CPU/GPU/Adaptive policy, revisions, failure cooldown, and timings
- `crates/gsplat-sort/src/lib.rs`: ordering correctness and performance
- `crates/gsplat-io-spz/src/lib.rs`: bounded/cancellable SPZ v4 parsing, coordinate conversion, and source caches
- `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`: integration boundary stability
- `crates/gsplat-web/src/`: browser `wasm-bindgen` API over the shared Surface renderer
- `packages/web/src/index.js`: local browser ESM wrapper over the generated wasm-bindgen module
- `bindings/android/gsplat-android/src/main/kotlin/`, `examples/android/app/src/main/kotlin/`, and `bindings/android/jni/gsplat_jni.c`: Android SDK wrapper, Surface lifecycle sample, and JNI bridge
- `bindings/apple/GsplatKit/Sources/GsplatKit/GsplatKit.swift`: Swift wrapper over the v0.1 C ABI
- `examples/ios/app/GsplatIOSExample.swift`: iOS Surface lifecycle and UIKit gesture bridge
- `examples/web/src/main.js`: browser PLY parsing, wasm-first renderer bootstrap, camera interaction, CPU depth sort fallback, benchmark orbit, and WebGL2 preview rendering
- `tests/perf/run-long-stability.sh` and `tools/bench-runner/src/main.rs`: regression detection for perf and stability

## Useful Entry Points

- Read first for renderer changes: `crates/gsplat-render-wgpu/src/lib.rs`
- Read first for import changes: `crates/gsplat-io-ply/src/lib.rs` or
  `crates/gsplat-io-spz/src/lib.rs`, depending on the format
- Read first for native integration changes: `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`
- Read first for verification flow: `VERIFICATION.md`
- Read first for release/tag changes: `../RELEASING.md`
- Read first for dependency policy: `../deny.toml` and
  `../tests/security/run-cargo-deny.sh`
