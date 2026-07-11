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
- GPU-resident direct sorted-index rendering on every frame;
  `rasterPath()` reports `sorted_index_direct`
- phase-specific frame stats: `renderSubmitMs` and `frameWallMs`;
  `cpuGeometryMs` and `rasterMs` remain zero-valued compatibility fields

Minimal browser usage:

```js
import {
  createGsplatRendererFromUrl,
  initGsplatWeb,
} from "./dist/index.js";

await initGsplatWeb({
  moduleUrl: "./dist/wasm/gsplat_web.js",
  wasmUrl: "./dist/wasm/gsplat_web_bg.wasm",
});

const renderer = await createGsplatRendererFromUrl({
  canvas: document.querySelector("canvas"),
  url: "/models/scene.ply",
});

renderer.renderFrame();
renderer.dispose();
```

Deploy `dist/index.js` and `dist/wasm/` together. Use `moduleUrl` and `wasmUrl`
when your application serves the wasm-bindgen files from a different base path.
`dispose()` and `free()` are equivalent; both release the native wasm renderer
and mark the wrapper as disposed.

Package-level checks:

```bash
npm --prefix packages/web run check
npm --prefix packages/web test
npm --prefix packages/web run pack:dry-run
```

Current limits:

- browser ESM only
- generated wasm package must be built locally
- rendering still depends on browser WebGPU support through `wgpu`
- scene loading is PLY-bytes based
- not a stable v0.1 public contract and not published to npm
