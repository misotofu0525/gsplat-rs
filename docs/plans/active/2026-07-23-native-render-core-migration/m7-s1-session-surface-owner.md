# M7 S1 Session Surface owner extraction

Status: candidate complete; M7 remains **Active**.

Base: `47f9435ee05df7874c86c86d0c59bacfa3b5479a`.

## Scope

This finite slice moves `SessionSurfaceOwner` and its private construction
selector from `surface_session.rs` into
`surface/session_owner.rs`. The private owner selects and holds exactly one of:

- the standalone `SurfacePresenter` used by Direct and diagnostic Paged; or
- the Product Packed `SurfacePresenterHost` used with the renderer-owned Exact
  runtime.

The owner exposes crate-private forwarding methods for Surface/resource
operations already present before the move: construction, size and adapter
queries, resize, native capture, geometry/raster preparation, receipt pumping,
and the matching resource-capability queries. There is no implicit `Deref` to
the standalone presenter, so every Direct/Paged versus Product Packed route
remains explicit.

`SurfaceRenderSession` remains the public facade. It still owns the public
constructors and methods, the `Renderer + SessionSurfaceOwner + Camera`
composition, CPU/GPU Adaptive control, Projected policy, scheduling and async
worker state, raw telemetry polling/submission access, concrete frame execution,
current-stats/evidence interpretation, and the one-frame transaction.
Renderer/session publication still occurs only after the selected owner reports
a successful presentation.

## Preserved contracts

- `SortedIndexDirect` and `PagedActiveAtlas` still construct the standalone
  presenter graph; `PackedAtlas` still constructs only the Product Packed host.
- Native window/raw-handle and wasm canvas constructors select the same owner
  before `SurfaceRenderSession` performs its existing size and runtime setup.
- Resize, frame latency, capture, geometry/raster setters, Direct/Paged render
  calls and Exact Packed render calls forward to the same concrete endpoint as
  before this move.
- Unsupported Product Packed operations retain the same structured errors;
  Direct/Paged switching remains owned by the standalone presenter.
- The Session retains the sole success-publication boundary. Construction,
  preparation, acquisition, render or present failure cannot publish a frame
  candidate through the extracted owner.
- No public Rust API, C ABI, FFI/JNI/Swift/Web wrapper, renderer implementation,
  WGSL, GPU resource layout, pass order, production policy or pixel contract is
  changed.

## Focused evidence

- `cargo test -p gsplat-render-wgpu surface::session_owner::tests:: --lib`:
  PASS, 3/3. The tests lock constructor routing, explicit forwarding and the
  absence of Adaptive/current-stats/evidence/schedule/frame-execution ownership.
- `cargo test -p gsplat-render-wgpu exact_surface_setters_share_the_same_transaction --lib`:
  PASS, 1/1. The Exact setters remain in one Session transaction.
- `cargo test -p gsplat-render-wgpu --lib`: PASS, 426 passed and 8 existing
  research/device tests ignored.
- `cargo check --workspace` and `cargo test --workspace`: PASS. Workspace tests
  include the Apple M4 SortedAlpha conformance test; no device or browser was
  launched by this slice.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`: PASS.
- `RUSTFLAGS="-D warnings" cargo check -p gsplat-render-wgpu --target
  wasm32-unknown-unknown`: PASS. This is compile evidence only, not a browser or
  WebGPU runtime claim.
- `PYTHONDONTWRITEBYTECODE=1 python3
  tests/architecture/check_source_architecture.py`: PASS with the existing
  non-blocking `lib.rs` and `surface_session.rs` grandfather-growth notices.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

This ownership slice does not update the aggregate progress or final acceptance
audit, remove a grandfather record, or accept M7. Device/browser launch,
platform packaging and installation are outside S1 and were not run.
