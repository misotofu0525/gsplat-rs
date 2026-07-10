# gsplat-rs Web Example

This directory hosts the browser validation surface.

There are three Web paths:

- Static WebGL2 fallback: `index.html` + `src/main.js`, useful without a wasm
  build.
- Experimental Rust/WASM renderer package: `crates/gsplat-web`, built into
  `examples/web/pkg/` with the script below.
- Local browser SDK wrapper: `packages/web`, built into
  `packages/web/dist/`.

## Run

Fetch the default CC0 showcase scene and build the Rust/WASM renderer:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash packages/web/scripts/build-wasm.sh
```

Then serve the repository root so the example can fetch shared datasets:

```bash
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

Then open:

```text
http://127.0.0.1:4173/examples/web/
```

Do not open `examples/web/index.html` with `file://`. Browser security rules
block the wasm package and root-relative dataset fetches in that mode.

Open a specific scene directly with:

```text
http://127.0.0.1:4173/examples/web/?dataset=showcase
http://127.0.0.1:4173/examples/web/?dataset=flowers
http://127.0.0.1:4173/examples/web/?dataset=minimal
```

To build the experimental Rust/WASM package for this example:

```bash
bash packages/web/scripts/build-wasm.sh
```

To build the local Web SDK wrapper distribution:

```bash
bash packages/web/scripts/build.sh
```

That script expects `wasm32-unknown-unknown` and the `wasm-bindgen` CLI to
already be installed. It does not install toolchain components for you.

The example first tries the trimmed Wakufactory Kitsune scene at
`tests/datasets/external/wakufactory_kitune/kitune1.ply`. The source model is
CC0, 65.9 MB, and contains 279,199 splats. If it is not installed, startup falls
back to the optional NVIDIA Flowers scene and then to `minimal_ascii.ply`.

Use the scene switcher to move between installed examples, or use `Open your
PLY` to keep a local file entirely in the browser. The Studio panel contains
renderer tuning, live frame timing, the benchmark runner, and diagnostics so
the default view can remain focused on the scene.

Touch and pointer controls mirror the Android validation app:

- one-finger drag / mouse drag: orbit around the loaded scene
- wheel or two-finger pinch: zoom
- two-finger drag: pan
- double tap/click: reset the auto camera

For repeatable browser performance checks, use Android-style query parameters:

```text
http://127.0.0.1:4173/examples/web/?gsplat_benchmark=true&gsplat_benchmark_frames=120&gsplat_benchmark_warmup_frames=10&gsplat_benchmark_yaw_step=0.001&gsplat_surface_sort_interval=2
```

The result is printed in the Benchmark panel and to the browser console as a
`BENCHMARK_RESULT` line. For headless smoke tests that need the result before
the browser exits, add `gsplat_benchmark_sync=true`. Add `dataset=flowers` to
run the same benchmark against
`tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`.

## Scope

- Parses ASCII and binary PLY files in the browser for the WebGL2 fallback.
- Applies the same RDF-to-RUF Y-axis flip used by `gsplat-io-ply`.
- Uses the same DC color and opacity conventions as the Rust renderer.
- CPU-sorts visible splat indices back-to-front before drawing.
- Caps the browser drawing buffer to a 1600px maximum side, matching the Android
  emulator Surface cap.
- Presents an immersive full-viewport showcase with streamed loading progress,
  theme switching, responsive controls, and a collapsible diagnostics studio.
- Reports Android-style realtime state, camera mode, dataset, path, surface
  size, and frame stats inside the Studio panel.
- Imports the generated `examples/web/pkg/gsplat_web.js` package when present
  and routes renderer creation through `packages/web/src/index.js` before
  falling back to the WebGL2 point-splat path.
- Supports benchmark orbit runs with `sort_interval` A/B checks.
- Renders a WebGL2 point-splat preview rather than the full `wgpu` ellipse
  pipeline when the generated wasm package is missing or cannot create a
  browser Surface.
- The Rust/WASM package uses `gsplat-io-ply::parse_ply_bytes`,
  `gsplat-render-wgpu::Renderer`, and `SurfacePresenter::from_canvas` so it can
  share the same Surface ellipse renderer used by Android/iOS.

## Web Integration Boundary

The repo now has a dedicated Rust/WASM boundary in `crates/gsplat-web`. It is
still experimental and must pass the wasm build plus browser smoke path before a
Web renderer change is called complete. `packages/web` is the local ESM
consumer wrapper around that generated wasm package. New Web renderer work
should target `crates/gsplat-web` and the wrapper rather than adding more
rendering logic to `src/main.js`.
