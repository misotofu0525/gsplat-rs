# gsplat-rs Architecture

## Purpose

- This file is a concise architectural map for humans and agents.
- Keep it factual and update it when responsibilities or critical flows move.

## System Boundaries

- The repository owns scene loading, sort backends, `wgpu` rendering, a small C ABI, experimental Rust/WASM Web bindings, and validation demos/tools around those pieces.
- The repository does not own model training, a polished web product surface, or multiple polished render backends.
- Main external dependencies are `wgpu`, platform toolchains for Swift/JNI validation, Android/iOS SDK tooling for mobile container builds, browser WebGL2 for the static Web demo, and `wasm-bindgen` for the experimental Web SDK path.

## Runtime Topology

- Core data types live in `crates/gsplat-core`.
- Scene import starts in `crates/gsplat-io-ply`.
- Sorting lives in `crates/gsplat-sort`.
- Rendering and GPU-facing orchestration live in `crates/gsplat-render-wgpu`.
- Native embedding goes through `crates/gsplat-ffi-c`.
- Browser WebAssembly embedding goes through `crates/gsplat-web`.
- Validation entrypoints are `apps/desktop-demo`, `tools/bench-runner`, `apps/android-demo`, `apps/ios-demo`, and `apps/web-demo`.

## Key Directories

- `crates/`: reusable library crates, the C ABI, and the experimental Web bindings
- `apps/desktop-demo/`: desktop viewer and offscreen output harness
- `apps/android-demo/`: Android demo project, JNI bridge, and host smoke entrypoint
- `apps/ios-demo/`: Swift smoke source, UIKit realtime Surface app, and iOS simulator/device build/run scripts
- `apps/web-demo/`: static frontend demo for browser PLY loading, WebGL2 point-splat fallback, and generated wasm package hosting
- `tools/`: CLI tools for performance validation
- `tests/`: shared dataset, FFI smoke harness, and long-stability script
- `handbook/`: current project docs, architecture map, verification guide, roadmap, and project principles
- `docs/plans/`: active and completed task planning bundles
- `.github/`: CI workflows and contributor templates

## Critical Flows

- PLY render flow:
  starts at external `.ply` data or `tests/datasets/minimal_ascii.ply`
  passes through `crates/gsplat-io-ply/src/lib.rs`
  continues into `crates/gsplat-render-wgpu/src/lib.rs`
  is exercised by `apps/desktop-demo/src/main.rs`, `tools/bench-runner/src/main.rs`, and `crates/gsplat-ffi-c/src/lib.rs`

- Native integration flow:
  starts from C, Swift, or Kotlin/JNI host entrypoints
  crosses `crates/gsplat-ffi-c/include/gsplat.h` and `crates/gsplat-ffi-c/src/lib.rs`
  ends in the shared renderer and stats path

- Android Surface flow:
  starts at `apps/android-demo/app/src/main/kotlin/com/gsplat/demo/MainActivity.kt`
  obtains a `SurfaceView` `Surface` and wraps it as an `ANativeWindow` in `apps/android-demo/jni/gsplat_jni.c`
  creates a raw-handle `wgpu::Surface` in `crates/gsplat-render-wgpu/src/lib.rs`
  presents directly to the Android swapchain, not through offscreen readback

- iOS Surface flow:
  starts at `apps/ios-demo/app/GsplatIOSDemo.swift`
  obtains a UIKit `UIView` backed by `CAMetalLayer`
  selects `Documents/imported_scene.ply`, bundled `flowers_1.ply`, or a generated minimal PLY
  passes the view through `gsplat_surface_renderer_create_uikit`
  creates a raw-handle `wgpu::Surface` in `crates/gsplat-render-wgpu/src/lib.rs`
  presents directly to the simulator Metal surface, not through offscreen readback
  uses the same Surface camera-control and benchmark option functions exposed through the C ABI

- Web WASM renderer flow:
  starts at browser JavaScript that imports the generated `gsplat-web` wasm package
  passes an `HtmlCanvasElement`, PLY bytes, and dimensions through `wasm-bindgen`
  parses the PLY with `gsplat-io-ply::parse_ply_bytes`
  loads the scene into `gsplat-render-wgpu::Renderer`
  creates a browser canvas `wgpu::Surface` through `SurfacePresenter::from_canvas`
  renders with the same Surface ellipse instance path used by Android/iOS by default

- Web demo flow:
  starts at `apps/web-demo/index.html`
  loads `apps/web-demo/src/main.js`
  imports generated `apps/web-demo/pkg/gsplat_web.js` when present and attempts the Rust/WASM Surface renderer first
  fetches or uploads a `.ply` file in the browser
  parses ASCII or binary PLY data into frontend buffers
  applies the same RDF-to-RUF Y-axis flip, DC color, and opacity conventions as the Rust import/render path
  CPU-sorts visible indices back-to-front and presents a WebGL2 point-splat preview
  exposes Android-style orbit/zoom/pan/reset camera controls and benchmark query parameters
  falls back to WebGL2 when the generated wasm package is missing or Surface creation fails

## Invariants

- `SortedAlpha` is the only release-gated path and the default mode expected by validation flows.
- The public C header and the Rust FFI implementation must stay in sync.
- PLY input normalization is not optional: quaternion remapping and `RDF -> RUF` conversion happen at load time.
- Mobile demo directories are integration validators, not separate product surfaces.
- `crates/gsplat-web` is the active experimental Rust/WASM target; Web renderer changes require the wasm build and browser smoke path before completion is claimed.
- The Web demo directory stays a browser validator and generated wasm package host, not a polished web product surface.

## Hotspots

- `crates/gsplat-render-wgpu/src/lib.rs`: render behavior, GPU orchestration, and perf-sensitive logic
- `crates/gsplat-sort/src/lib.rs`: ordering correctness and performance
- `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`: integration boundary stability
- `crates/gsplat-web/src/`: browser `wasm-bindgen` API over the shared Surface renderer
- `apps/android-demo/app/src/main/kotlin/` and `apps/android-demo/jni/gsplat_jni.c`: Android Surface lifecycle and JNI bridge
- `apps/ios-demo/app/GsplatIOSDemo.swift`: iOS Surface lifecycle and UIKit gesture bridge
- `apps/web-demo/src/main.js`: browser PLY parsing, wasm-first renderer bootstrap, camera interaction, CPU depth sort fallback, benchmark orbit, and WebGL2 preview rendering
- `tests/perf/run-long-stability.sh` and `tools/bench-runner/src/main.rs`: regression detection for perf and stability

## Useful Entry Points

- Read first for renderer changes: `crates/gsplat-render-wgpu/src/lib.rs`
- Read first for import changes: `crates/gsplat-io-ply/src/lib.rs`
- Read first for native integration changes: `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`
- Read first for verification flow: `VERIFICATION.md`
