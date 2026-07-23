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
- `createGsplatRendererFromStream()` for local `File.stream()` and custom
  `ReadableStream<Uint8Array>` transports
- `GsplatWebRenderer` for camera controls, transactional resize/geometry-path
  changes, stats, and disposal
- exact compact GPU-resident `packed` rendering by default, with adaptive
  CPU/GPU full-scene ordering; `direct` remains the f32 quality oracle and
  `rasterPath()` reports the active pipeline
- independent `candidate`, `compact`, or `adaptive` projected drawing, with
  requested policy and actual execution reported separately on every frame
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

`await renderer.setGeometryPathAsync(path)` requests a transactional switch
between the full-quality Direct oracle and Packed production geometry. It
shares one serial mutation queue with `resize()`, blocks frame submission until
the queued mutation settles, and rejects with `stage="geometry_path"` plus
`scene_published=true` without replacing the last working renderer. A
Direct-constructed scene retains the source planes needed for a Direct ->
Packed -> Direct round trip. An allocation-bounded Packed stream intentionally
does not retain that wide Direct source, so its later Packed -> Direct request
can reject while Packed stays live. Paged is a constructor-time diagnostic and
a changed runtime transition involving it is rejected by the native contract.
The legacy synchronous `setGeometryPath()` is retained only for compatibility:
a repeated same-path call is idempotent and a changed-path call fails closed.
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

`completedOrderMeasurements` drains every GPU-order receipt that completed
since the preceding `renderFrame()` call. Consumers must not treat only the
latest receipt as a complete benchmark sample: more than one ticket can finish
between animation frames. `failedOrderMeasurements` is the matching terminal
failure stream. `drainOrderMeasurementReceipts()` drains both streams without
rendering, which lets a fail-closed caller preserve receipts collected before a
later Surface/presentation error.

`setProjectedPolicy("candidate" | "compact" | "adaptive")` is fail closed.
Candidate and Compact are deterministic experiment controls and therefore
report their actual execution with `projectedMeasurementSubmission="not_requested"`
and `projectedMeasurementTicket=null`; only Adaptive formal samples issue
projected tickets. `drainOrderMeasurementReceipts()` retains its existing order
arrays and additionally returns `completedProjectedMeasurements` and
`failedProjectedMeasurements`, with exactly one terminal per issued ticket.

`gpuOrderProducer: "post-sort" | "preproject"` is an explicit diagnostic
creation option, not a product default switch. Omitting it keeps PostSort and
leaves producer telemetry disabled. Supplying it requires exact Packed
geometry plus forced Compact projected drawing; formal callers must also force
GPU ordering. Creation transactionally prepares and publishes the complete
producer graph before returning, then enables an independent ticket stream in
`completedGpuProducerMeasurements` / `failedGpuProducerMeasurements`. Each
successful exact-current receipt proves source/contributor/drawn counts,
producer and camera identity, graph generations, queue-completion time, and
`D=C`; stale-order or unsampled frames are not strict A/B evidence. The same
selector is available after construction through
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
