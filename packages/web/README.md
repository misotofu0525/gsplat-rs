# @gsplat-rs/web

Local browser SDK wrapper for the experimental `gsplat-web` Rust/WASM renderer.

This is a local packaging slice, not a published npm release yet. Build the
generated wasm-bindgen package and wrapper dist first:

```bash
bash packages/web/scripts/build.sh
```

That writes:

- `packages/web/dist/index.js`
- `packages/web/dist/index.d.ts`
- `packages/web/dist/wasm/`

The wrapper exposes:

- `initGsplatWeb()` for loading the wasm-bindgen module
- `createGsplatRenderer()` for creating a canvas renderer from PLY bytes
- `createGsplatRendererFromUrl()` for fetch-and-render flows
- `GsplatWebRenderer` for camera controls, resize, stats, and disposal

Current limits:

- browser ESM only
- generated wasm package must be built locally
- rendering still depends on browser WebGPU support through `wgpu`
- scene loading is PLY-bytes based
- not a stable v0.1 public contract and not published to npm
