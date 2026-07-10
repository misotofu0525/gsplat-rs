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
- Scene import starts in `crates/gsplat-io-ply`.
- Sorting lives in `crates/gsplat-sort`.
- Rendering and GPU-facing orchestration live in `crates/gsplat-render-wgpu`.
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
- `tests/`: shared dataset plus FFI, perf, release, and dependency-policy scripts
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

- Renderer construction flow:
  native offscreen `Renderer::new` and `Renderer::with_config` acquire a GPU
  adapter/device and fail when rasterization cannot be created
  Surface clients use `Renderer::new_for_surface` or
  `Renderer::with_config_for_surface`, then let `SurfacePresenter` acquire the
  adapter/device compatible with the platform surface
  dimension and instance-buffer limits are checked before `wgpu` resource
  creation, and GPU submission/wait failures remain structured errors

- Native integration flow:
  starts from C, Swift, or Kotlin/JNI host entrypoints
  crosses `crates/gsplat-ffi-c/include/gsplat.h` and `crates/gsplat-ffi-c/src/lib.rs`
  ends in the shared renderer and stats path
  keeps each native handle owned by one serialized thread or queue; wrapper
  APIs add their own locking, while direct C/JNI callers must provide the same
  serialization
  catches Rust panics at every exported C ABI entrypoint so no unwind crosses
  into foreign code

- Android Surface flow:
  starts at the local `bindings/android/gsplat-android` library module or
  sample `examples/android/app/src/main/kotlin/com/gsplat/example/MainActivity.kt`
  obtains a `SurfaceView` `Surface` and wraps it as an `ANativeWindow` in `bindings/android/jni/gsplat_jni.c`
  creates a raw-handle `wgpu::Surface` in `crates/gsplat-render-wgpu/src/lib.rs`
  presents directly to the Android swapchain, not through offscreen readback
  packages the selected build-time scene as `assets/showcase.ply` plus its source-name metadata, preferring the CC0 Kitsune scene and falling back to Flowers
  presents compact showcase telemetry while keeping the complete validation status behind the `Studio` control
  packages the JNI library through `bindings/android/gsplat-android` for local AAR builds

- iOS Surface flow:
  starts at the local `bindings/apple/GsplatKit` wrapper or sample `examples/ios/app/GsplatIOSExample.swift`
  obtains a UIKit `UIView` backed by `CAMetalLayer`
  selects `Documents/imported_scene.ply`, bundled `showcase.ply` with source-name metadata, or a generated minimal PLY
  passes the view through `gsplat_surface_renderer_create_uikit`
  creates a raw-handle `wgpu::Surface` in `crates/gsplat-render-wgpu/src/lib.rs`
  presents directly to the simulator Metal surface, not through offscreen readback
  uses the same Kitsune-first editorial showcase and toggleable `Studio` diagnostics pattern as Android
  uses the same Surface camera-control and benchmark option functions exposed through the C ABI
  packages the C ABI as a local `GsplatFFI.xcframework` through `bindings/apple/scripts/build-xcframework.sh`

- Web WASM renderer flow:
  starts at browser JavaScript that imports the local `packages/web` wrapper or generated `gsplat-web` wasm package
  passes an `HtmlCanvasElement`, PLY bytes, and dimensions through `wasm-bindgen`
  parses the PLY with `gsplat-io-ply::parse_ply_bytes`
  loads the scene into `gsplat-render-wgpu::Renderer`
  creates a browser canvas `wgpu::Surface` through `SurfacePresenter::from_canvas`
  defaults to `SurfaceRasterPath::CpuInstances` (CPU-built Surface ellipse
  instances each sort refresh)
  opt-in `SurfaceRasterPath::SortedIndexDirect` keeps scene data GPU-resident
  and uploads only sorted `u32` indices per refresh (`createRendererWithOptions`
  / `sortedIndexDirect`, same shader family as mobile FFI `static_direct`)
  sorting remains CPU radix for both Surface paths in this slice

- Desktop offscreen raster paths:
  `OffscreenRasterPath::CpuInstances` (default) builds and uploads full
  `GpuInstance` buffers each frame
  opt-in `OffscreenRasterPath::SortedIndexGpuPreproject` keeps scene data on
  GPU and uses `GpuInstancePreprocessor` with sorted indices
  (`desktop-example` / `bench-runner` `--sorted-index-direct`)

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
- PLY input normalization is not optional: quaternion remapping and `RDF -> RUF` conversion happen at load time.
- Mobile examples are integration validators. Android and Apple packaging live
  under `bindings/`, but neither path is a published product SDK yet.
- `crates/gsplat-web` is the active experimental Rust/WASM target; Web renderer changes require the wasm build and browser smoke path before completion is claimed.
- The Web example stays a browser validator and generated wasm package host.
  The local Web package lives under `packages/web`, but it is not a published
  npm package.

## Hotspots

- `crates/gsplat-render-wgpu/src/lib.rs`: render behavior, GPU orchestration, and perf-sensitive logic
- `crates/gsplat-sort/src/lib.rs`: ordering correctness and performance
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
- Read first for import changes: `crates/gsplat-io-ply/src/lib.rs`
- Read first for native integration changes: `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`
- Read first for verification flow: `VERIFICATION.md`
- Read first for release/tag changes: `../RELEASING.md`
- Read first for dependency policy: `../deny.toml` and
  `../tests/security/run-cargo-deny.sh`
