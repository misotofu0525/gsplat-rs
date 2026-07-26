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
- Rendering goes through `gsplat-render-wgpu::SurfaceRenderSession`, whose
  shared Exact runtime owns the complete Packed plan, policy, cache generations,
  order/raster publication, and current-stats receipts. The WASM crate is only
  a compatibility/translation boundary; it has no Web-only renderer controller.
- WebGPU Surface and Exact scene-construction failures return an error without
  publishing a renderer. This crate has no WebGL2 fallback; the example's
  sampled WebGL2 diagnostic is a separate explicit opt-in and cannot satisfy
  formal qualification.
- Each `renderFrame` result retains the legacy order-measurement fields for
  compatibility, but renderer-owned Exact Packed frames do not manufacture a
  legacy order ticket. Exact benchmark evidence joins `requestCurrentStats`
  submissions to `pollCurrentStats` terminals instead.
- Raw `setProjectedPolicy(0|1|2)` remains a compatibility input for Candidate,
  Compact, or Adaptive, but the session validates it as part of one closed
  Exact plan. There is no browser-local projected learner.
- Adaptive projected successes and failures drain as
  `completedProjectedMeasurements` / `failedProjectedMeasurements`, including
  exact V/C/D counts, projection/probe generations, and frame-completion time.
  Their independent high ticket namespace remains exactly representable by a
  JavaScript `number` (`2^52..2^53-1`), and each issued ticket has one terminal.
- GPU/Adaptive selection is two-phase in the raw wasm API: callers first await
  `prepareGpuOrder()`, which constructs the sorter plus draw/projection bindings
  under WebGPU validation/OOM/internal error scopes, and only then call
  `setOrderBackend(...)`. CPU-only clients never allocate those resources.
- Geometry is selected at construction. Raw `setGeometryPathAsync(...)` and
  legacy `setGeometryPath(...)` retain their compatibility shape, but only a
  repeated same-path request succeeds; changed Direct/Packed/Paged transitions
  fail before mutation so the browser cannot publish a legacy Packed owner
  beside the shared Exact runtime.
- Production Packed + Projected resizing is likewise asynchronous through raw
  `resizeAsync(width, height)`. A changed size passed to legacy synchronous
  `resize` fails closed; the async path publishes dimensions only after scoped
  Surface configuration succeeds and restores the old configuration on error.
- `loadReceipt` does not infer completeness from one scene count. It reports
  the PLY header declaration, rows accepted by the decoder, records committed
  by `ResidentSceneBuilder`, records owned by `Renderer`, and records actually
  addressable by the allocated Surface geometry, together with source and
  resident SH degree.
- `requestCurrentStats` and `pollCurrentStats` expose the renderer/session's
  non-blocking Exact observer receipt. WASM translates its identity and
  `S/V/C/D` semantics without owning a second ticket or generation ledger.
  Pending indirect V/D counts serialize as JavaScript `null`; only a matching
  renderer terminal may populate them.
- The default Packed geometry reports `rasterPath() == "packed_atlas"`; the Web
  example prefixes that identity as `renderer=wasm_packed_atlas`.
- It is not part of the stable v0.1 public contract. Web changes must pass the
  WebGPU/WASM smoke path in `handbook/VERIFICATION.md` before completion is
  claimed.

## Build

The local machine needs the wasm Rust target and the locked `wasm-bindgen` CLI.
The repository doctor verifies the exact tool version without installing it:

```bash
python3 tests/verification_bootstrap.py doctor --profile web-webgpu
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.121 --locked
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

Build and unit-policy results do not claim browser execution. Real
Chrome/WebGPU execution at the accepted M7 SHA
`1de3f79fa2fa22955f99c887bea421c918e31ee0` remains **Deferred**.
