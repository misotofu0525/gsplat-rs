# M7 standalone Direct runtime ownership slice

Status: candidate complete; M7 remains **Active**.

Base: `7e8c7c6ee5a183736558947b07057ed06e2586fd`.

## Scope

This slice extracts the standalone Direct compatibility execution lane from
`surface_presenter.rs` into the private
`surface/standalone_direct_runtime.rs` owner. The new owner contains:

- the Direct bind-group layout and render pipeline;
- the active Direct GPU scene and Direct-only instance count;
- CPU-order upload and Direct CPU draw encoding;
- transactional Direct GPU-order candidate preparation and publication;
- Direct GPU-order encode, indirect draw, readback reservation, arm, poll,
  generation invalidation, and timestamp capability state.

`SurfacePresenter` remains the public facade and host coordinator. It still
owns Surface acquire, capture, encoder completion, queue submission, primitive
presentation, shared CPU completion telemetry, geometry-path transactions,
and the diagnostic Paged runtime. Product Packed continues to use
`SurfacePresenterHost` plus the renderer-owned Exact runtime.

## Preserved contracts

- Direct/Paged switches still prepare a complete candidate before mutation.
  Publishing Paged releases the old Direct scene; publishing Direct replaces
  Paged only after the Direct candidate succeeds, so this extraction does not
  retain two large scene owners after commit.
- Direct GPU-order construction still uses Validation, OutOfMemory, and
  Internal error scopes and publishes the candidate only after all three
  scopes resolve successfully.
- A GPU telemetry slot is still reserved only after successful Surface
  acquisition. The Direct owner encodes and arms the same ticket, while the
  Presenter preserves acquire -> encode -> capture -> finish/arm -> submit ->
  present ordering.
- Native synchronous preparation and wasm asynchronous preparation boundaries
  are unchanged. Paged remains CPU-only and Product Packed is untouched.
- No public Rust/C/JNI/Swift/Web API, ABI layout, WGSL, GPU resource layout,
  pass label/order, or pixel contract changes in this slice.

## Focused evidence

- `cargo test -p gsplat-render-wgpu --lib`: PASS, 423 passed and 8 existing
  research/device tests ignored.
- `cargo test -p gsplat-render-wgpu surface_presenter::tests::`: PASS, 15/15.
- `cargo test -p gsplat-render-wgpu surface::standalone_direct_runtime::tests::`:
  PASS, 2/2, including the preserved OOM -> Internal -> Validation scope-error
  priority and labels.
- `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`:
  PASS with the existing informational grandfather-growth notices for
  `lib.rs` and `surface_session.rs`.
- `cargo check --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`: PASS.
- `cargo check -p gsplat-web --target wasm32-unknown-unknown`: PASS. This is
  wasm compile evidence, not a Chrome/WebGPU runtime claim.
- `bash tests/ffi/run-ffi-smoke.sh`: PASS (`drawn=2`, `visible=2`).
- `python3 tests/verification_bootstrap.py run macos-metal`: PASS; the required
  Apple M4 Metal SortedAlpha conformance test passed 1/1.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

This ownership slice is not M7 acceptance and does not remove any active
grandfather record. No Android/iOS physical-device or browser runtime claim is
made by this candidate.
