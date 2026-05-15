# gsplat-web

Browser WebAssembly bindings for the shared Rust `wgpu` Surface renderer.

## Boundary

- This crate is the experimental Web SDK boundary for the `wasm32-unknown-unknown`
  target.
- It uses `wasm-bindgen` to expose a browser API that accepts an
  `HtmlCanvasElement` and PLY bytes.
- Scene loading still goes through `gsplat-io-ply::parse_ply_bytes`.
- Rendering still goes through `gsplat-render-wgpu::Renderer` and
  `SurfacePresenter`, using the same Surface instance path as Android/iOS by
  default.
- It is not part of the stable v0.1 public contract. Web changes must pass the
  WebGPU/WASM smoke path in `handbook/VERIFICATION.md` before completion is
  claimed.

## Build

The local machine needs the wasm Rust target and the `wasm-bindgen` CLI:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

Then build the demo package from the repo root:

```bash
bash apps/web-demo/build-wasm.sh
```

The generated browser package is written to `apps/web-demo/pkg/` and is ignored
by git.
