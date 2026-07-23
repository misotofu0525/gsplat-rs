# gsplat-web

Browser WebAssembly bindings for the shared Rust `wgpu` Surface renderer.

## Boundary

- This crate is the experimental Web SDK boundary for the `wasm32-unknown-unknown`
  target.
- It uses `wasm-bindgen` to expose a browser API that accepts an
  `HtmlCanvasElement` plus either PLY bytes or allocation-bounded transport
  chunks.
- Direct/Paged scene loading goes through `gsplat-io-ply::parse_ply_bytes`.
  When Packed is selected at construction, the same byte payload is visited
  one decoded splat at a time and written directly into the exact resident
  planes; no temporary wide `SceneBuffers` is constructed.
- `createPackedPlyStream` is the production URL boundary. Arbitrary
  `ReadableStream` chunk splits are accepted; only an incomplete header,
  ASCII row, or binary record is retained while complete splats are appended
  directly to `ResidentSceneBuilder`. This avoids both full-response
  `ArrayBuffer` materialization and a full WASM input copy.
- Rendering goes through `gsplat-render-wgpu::SurfaceRenderSession`, which owns
  `Renderer`, `SurfacePresenter`, CPU sort cadence, compact order uploads,
  direct rendering, and phase timings. This is the same Surface lifecycle
  used by Android/iOS and the interactive desktop viewer.
- Each `renderFrame` result drains the complete queue of newly finished GPU
  order measurements. The ESM layer exposes that queue as
  `completedOrderMeasurements`; ticket and camera revision are both required
  when joining asynchronous evidence to submitted frames.
- Projected drawing is independently selectable through raw
  `setProjectedPolicy(0|1|2)` for Candidate, Compact, or Adaptive. A rejected
  Compact request leaves the previous policy live. Frame results distinguish
  requested policy, actual execution, Adaptive state, and issued/unsampled
  submission identity; forced modes report `not_requested` with no ticket.
- Adaptive projected successes and failures drain as
  `completedProjectedMeasurements` / `failedProjectedMeasurements`, including
  exact V/C/D counts, projection/probe generations, and frame-completion time.
  Their independent high ticket namespace remains exactly representable by a
  JavaScript `number` (`2^52..2^53-1`), and each issued ticket has one terminal.
- GPU/Adaptive selection is two-phase in the raw wasm API: callers first await
  `prepareGpuOrder()`, which constructs the sorter plus draw/projection bindings
  under WebGPU validation/OOM/internal error scopes, and only then call
  `setOrderBackend(...)`. CPU-only clients never allocate those resources.
- Runtime geometry changes are also two-phase: raw callers await
  `setGeometryPathAsync(...)` to move between the Direct oracle and production
  Packed path. Target CPU derivations and the complete GPU graph—including the
  exact projected-contributor path and any sorter required by the selected
  order backend—remain unpublished until validation/OOM/internal scopes finish.
  Changed-path calls to legacy synchronous `setGeometryPath` fail closed;
  repeated same-path calls are idempotent. Paged remains a constructor-time
  diagnostic and is rejected by runtime switching so it cannot invalidate the
  full-quality load receipt.
- Production Packed + Projected resizing is likewise asynchronous through raw
  `resizeAsync(width, height)`. A changed size passed to legacy synchronous
  `resize` fails closed; the async path publishes dimensions only after scoped
  Surface configuration succeeds and restores the old configuration on error.
- `loadReceipt` does not infer completeness from one scene count. It reports
  the PLY header declaration, rows accepted by the decoder, records committed
  by `ResidentSceneBuilder`, records owned by `Renderer`, and records actually
  addressable by the allocated Surface geometry, together with source and
  resident SH degree.
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
