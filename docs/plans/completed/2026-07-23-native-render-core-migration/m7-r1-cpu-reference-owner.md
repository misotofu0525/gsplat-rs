# M7 R1 CPU reference owner slice

Status: candidate implementation complete; M7 remains **Active**.

Base: `47f9435`.

## Scope

This slice moves the existing CPU reference raster-data implementation from
`crates/gsplat-render-wgpu/src/lib.rs` into the private
`cpu/reference.rs` owner. The owner contains only the established concrete
implementation for:

- CPU draw-instance construction from an already ordered source-ID list;
- world covariance construction and camera/NDC covariance projection;
- ellipse-axis construction and frustum-overlap rejection;
- opacity preparation and view-dependent SH evaluation;
- the quaternion, matrix, and normalization helpers required by those steps.

`Renderer` remains the public facade and continues to own scene/path
transactions, cached scene publication, Exact runtime state, render dispatch,
and final frame/stat publication. The existing CPU order engine remains in
`cpu_order`/`cpu/preprocess`; this slice does not move or reinterpret sorting.
The crate-root aliases retained for existing private consumers are compatibility
seams, not new public API.

## Preserved contracts

- Stable radix and the Scalar CPU-order oracle are unchanged. The explicit FMA
  depth expression, inclusive near/far visibility, `depth.max(0).to_bits()`,
  descending full-32-bit order, and source-ID tie behavior remain in their
  existing owners.
- Every moved production function is text-identical to baseline after removing
  only the new `pub(crate)` visibility needed by sibling private modules.
- The same world covariance and SH functions continue to feed Direct,
  resident encoding, preprojection tests, and the diagnostic Paged color
  refresh path.
- No public Rust API, C/JNI/Swift/Web ABI, WGSL, GPU resource layout, pass
  order, Surface lifecycle, device path, or pixel rule changes in this slice.

## Focused evidence

- New owner-local tests cover requested source-ID order with invalid-ID
  filtering, degree-zero lower-only SH clamping, and bit-exact covariance cache
  reuse of the single-source oracle.
- A baseline/current extraction audit reports all 33 moved production
  functions identical after visibility normalization.
- `cargo test -p gsplat-render-wgpu cpu::reference::tests --lib`: PASS, 3/3.
- `cargo test -p gsplat-render-wgpu --lib`: PASS, 427 passed and 8 existing
  research/device tests ignored. This includes the established Scalar/radix,
  visibility, covariance, SH, and Direct/Packed image-oracle coverage.
- `cargo check --workspace --locked`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`: PASS.
- `cargo check -p gsplat-render-wgpu --target wasm32-unknown-unknown --locked`:
  PASS; this is compile evidence, not a browser runtime claim.
- `python3 tests/architecture/check_source_architecture.py`: PASS; the existing
  `surface_session.rs` grandfather-growth notice remains informational.
- `cargo fmt --all -- --check`, `cargo metadata --no-deps --format-version 1`,
  and `git diff --check`: PASS.

The first focused compile attempt reached host `ENOSPC`. Only the 114.2 MiB of
rebuildable output created in this worktree by that failed attempt was removed
with `cargo clean`; shared caches were not deleted. Verification resumed after
other work released space. No Android/iOS device or browser run and no
dependency installation was performed.

This ownership slice is not M7 acceptance and does not edit `progress.md` or
`m7-final-acceptance.md`.
