# gsplat-web

Browser WebAssembly bindings for the shared Rust `wgpu` Surface renderer.

## Boundary

- This crate is the experimental Web SDK boundary for the `wasm32-unknown-unknown`
  target.
- It uses `wasm-bindgen` to expose a browser API that accepts an
  `HtmlCanvasElement` and PLY bytes.
- Scene loading still goes through `gsplat-io-ply::parse_ply_bytes`.
- Rendering goes through `gsplat-render-wgpu::SurfaceRenderSession`, which owns
  `Renderer`, `SurfacePresenter`, CPU sort cadence, compact order uploads,
  direct rendering, and phase timings. This is the same Surface lifecycle
  used by Android/iOS and the interactive desktop viewer.
- It is not part of the stable v0.1 public contract. Web changes must pass the
  WebGPU/WASM smoke path in `handbook/VERIFICATION.md` before completion is
  claimed.

## Build

The local machine needs the wasm Rust target and the `wasm-bindgen` CLI:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

Then build the example package from the repo root:

```bash
bash packages/web/scripts/build-wasm.sh
```

The generated browser package is written to `examples/web/pkg/` and is ignored
by git.

For a local npm-style wrapper, build:

```bash
bash packages/web/scripts/build.sh
```

That writes `packages/web/dist/` and keeps the generated wasm
module behind the `@gsplat-rs/web` ESM wrapper. This is still local-only and is
not published to npm.
