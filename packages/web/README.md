# @gsplat-rs/web

Local browser SDK wrapper for the experimental `gsplat-web` Rust/WASM renderer.

Before building Wasm or launching the Chrome/WebGPU collector, use the
read-only prerequisite doctor and reusable command profile in
[`handbook/VERIFICATION.md`](../../handbook/VERIFICATION.md#reusable-launchbook-android-web-and-macos).

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
- `createGsplatRendererFromStream()` for local `File.stream()` and custom
  `ReadableStream<Uint8Array>` transports
- `GsplatWebRenderer` for camera controls, transactional resize,
  constructor-time geometry selection, stats, and disposal
- exact compact GPU-resident `packed` rendering by default, with adaptive
  CPU/GPU full-scene ordering owned by the shared renderer/session; `direct`
  remains the f32 quality oracle and `rasterPath()` reports the active pipeline
- `candidate`, `compact`, and `adaptive` compatibility inputs are validated as
  complete Exact plans rather than learned by a Web-only controller
- allocation-bounded URL loading: `Response.body` chunks are decoded directly
  into the exact resident planes, so the browser never materializes the full
  PLY as an `ArrayBuffer` plus a second WASM copy
- phase-specific frame stats: `renderSubmitMs` and `frameWallMs`;
  `cpuGeometryMs` and `rasterMs` remain zero-valued compatibility fields
- `loadReceipt()` exactness evidence sourced from independent boundaries:
  header-declared, decoder-accepted, Resident-encoded, renderer-resident, and
  GPU-addressable counts, plus source/resident SH degree. `fullQuality` is true
  only when every count and SH degree agrees and no sampling, LOD, or partial
  publication occurred.

Creation and presentation are fail-closed: the package rejects when the
Rust/WASM module, WebGPU Surface, or Exact Packed scene cannot be constructed.
It has no WebGL2 product fallback. The example's sampled WebGL2 diagnostic is a
separate, explicit `gsplat_allow_sampled_webgl=true` opt-in and cannot satisfy
formal qualification. For the default Packed path `rasterPath()` returns
`packed_atlas`, which the example reports as `renderer=wasm_packed_atlas`.

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
  // Defaults shown explicitly; each can be overridden for controlled A/B runs.
  geometryPath: "packed",
  orderBackend: "adaptive",
  projectedPolicy: "adaptive",
  sortInterval: 1,
});

const frame = renderer.renderFrame();
renderer.requestCurrentStats();
renderer.renderFrame();
const current = renderer.pollCurrentStats();
// Poll on later animation frames until this renderer-owned receipt is terminal.
console.log(current);
for (const receipt of frame.completedOrderMeasurements) {
  // GPU timing/count receipts are asynchronous and are joined by
  // receipt.ticket plus receipt.cameraRevision.
  console.log(receipt);
}
for (const failure of frame.failedOrderMeasurements) {
  // A ticket terminates as either a success or this structured failure.
  // Strict benchmark evidence must fail rather than turn this into a sample.
  console.error(failure);
}
for (const receipt of frame.completedProjectedMeasurements) {
  // Projected tickets use an independent JS-safe high namespace and include
  // V/C/D, projection/probe generations, and full frame-completion time.
  console.log(receipt);
}
renderer.dispose();
```

Deploy `dist/index.js` and `dist/wasm/` together. Use `moduleUrl` and `wasmUrl`
when your application serves the wasm-bindgen files from a different base path.
`dispose()` and `free()` are equivalent; both release the native wasm renderer
and mark the wrapper as disposed.

`await renderer.resize(width, height)` is transactional and never calls the
legacy synchronous native resize. Same-size requests are no-ops; a changed-size
request requires the native `resizeAsync()` API and resolves only after the new
Surface size is published. Runtime resize failures use `stage="resize"` and
`scene_published=true`, because the already-published scene remains live at its
last successfully configured size.

`await renderer.setGeometryPathAsync(path)` retains the historical async API
shape, but Packed is selected at construction and owned solely by the shared
Exact runtime. Repeated same-path calls are idempotent; changed runtime
transitions involving Direct/Packed/Paged fail closed with
`stage="geometry_path"` and `scene_published=true`, leaving the published
renderer live. The legacy synchronous `setGeometryPath()` follows the same
compatibility rule.
Calling `dispose()` while either mutation is in flight marks the wrapper
disposed immediately but defers native release until the complete mutation
queue settles.

Creation is transactional. GPU/Adaptive creation requires and awaits the
module's async `prepareGpuOrder()` hook before selecting that backend; an older
module without the hook remains usable in CPU mode but fails closed for
GPU/Adaptive. Any create,
capacity, preparation, or configuration failure frees the native handle before
rejecting. Rejections are `GsplatWebError` instances with `stage`,
`error_code`, `error_message`, and `scene_published=false`; capacity failures
also include `resource.kind`, `resource.required_bytes`, and
`resource.limit_bytes` when the native error exposes those values.

`requestCurrentStats()` reserves one non-blocking observation of the next
presented Exact frame, and `pollCurrentStats()` returns its renderer-owned
terminal. The receipt binds plan, scene/camera/viewport/contract/plan-set/order/
raster generations, encode attempt, presentation sequence, and truthful
`S/V/C/D` semantics. The wrapper validates but does not mirror issued or
terminal tickets, cache generations, or adaptive state. While an indirect
Exact frame awaits that terminal, `visibleCount` and `drawnCount` are `null`;
they are never replaced by zero, capacity, or an older frame's counts.

Legacy `completedOrderMeasurements` drains every GPU-order receipt that completed
since the preceding `renderFrame()` call. Consumers must not treat only the
latest receipt as a complete benchmark sample: more than one ticket can finish
between animation frames. `failedOrderMeasurements` is the matching terminal
failure stream. `drainOrderMeasurementReceipts()` drains both streams without
rendering, which lets a fail-closed caller preserve receipts collected before a
later Surface/presentation error.
Packed Exact benchmark consumers use current-stats instead of requiring this
legacy ticket stream. A retained frame is count-eligible only after the
renderer terminal matches its ticket, complete generation identity, camera,
plan, encode attempt, and presentation sequence.

`setProjectedPolicy("candidate" | "compact" | "adaptive")` is a fail-closed
compatibility input to the complete Exact plan. Legacy projected receipt fields
remain present for API compatibility, but the browser does not run a separate
projected adaptive controller.

`gpuOrderProducer: "post-sort" | "preproject"` is an explicit diagnostic
creation option, not a product default switch. Omitting it keeps PostSort and
leaves producer telemetry disabled. Supplying it requires exact Packed
geometry plus forced Compact projected drawing; formal callers must also force
GPU ordering. Creation transactionally prepares and publishes the complete
producer graph without enabling the retired producer-specific learner or
terminal ledger. Use Exact current-stats to prove the selected whole plan and
its source/contributor/drawn identity. The same compatibility selector is available through
`await renderer.setGpuOrderProducerAsync(producer)`.

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
- `createGsplatRenderer()` accepts in-memory PLY bytes; the recommended
  `createGsplatRendererFromUrl()` Packed path is transport-streamed
- not a stable v0.1 public contract and not published to npm

Package syntax and unit tests do not claim browser execution. Real
Chrome/WebGPU execution at the accepted M7 SHA
`1de3f79fa2fa22955f99c887bea421c918e31ee0` remains **Deferred**.
