# M7 S5 Session frame executor ownership

Status: candidate implementation complete; M7 remains **Active**.

Base: `95525c984d1ef8b8b0b82b1af612fc677cfcf6ad`.

## Scope

This finite slice extracts the private, stateless Surface frame-attempt
executor from `surface_session.rs`. The executor borrows the existing Renderer
and S1 Surface owner only for one attempt and orchestrates the already-owned
prepare, encode, submit and present operations for:

- Product Packed Exact frames;
- standalone Direct GPU-order frames; and
- standalone Direct CPU-order or diagnostic Paged frames.

The attempt result distinguishes a genuinely presented candidate from an
unavailable drawable. Fallible execution returns an error without producing a
committable candidate. `SurfaceRenderSession` consumes that result and remains
the only owner of successful-present publication.

## Frozen boundary

- `SurfaceRenderSession` retains every public Rust entrypoint, lifecycle and
  configuration composition, policy/controller state, telemetry/current-stats,
  compatibility evidence, scheduling/async-worker state and their semantics.
- The executor has no persistent policy, stats, receipt, identity, generation
  or cache fields. It does not poll telemetry and does not publish any public
  state.
- Exact rendering continues through the renderer-owned
  `PreparedRuntimeSlot` and existing Surface shadow transaction. No second
  plan/controller/cache/result owner is introduced.
- Only a real successful primitive present may enter the Session commit path.
  Surface unavailability, preparation/encoding/submission/presentation error,
  or an abandoned encoded/submitted frame cannot publish presented Session
  state, current-stats, plan/presentation identity or an issued compatibility
  ticket. The existing explicit `Unsampled(SurfaceUnavailable)` status remains
  available without masquerading as a presented-frame receipt.
- Renderer/GPU/WGSL/resource layout/pass order/pixel behavior, public API, C
  ABI and all Web/Android/Apple bindings are frozen.

## Verification

- `cargo test -p gsplat-render-wgpu
  surface::session_frame_executor::tests:: --lib`: PASS, 2/2. The typed
  transaction proves unavailable attempts cannot invoke the commit closure and
  the executor is zero-sized with no policy/stats/evidence/cache owner fields.
- `cargo test -p gsplat-render-wgpu surface::shadow::tests:: --lib`: PASS,
  9/9. Failed primitive present, unavailable/failed acquisition, abandoned
  capture, stale target and retry cases preserve the Exact publication fence.
- `cargo test -p gsplat-render-wgpu current_stats --lib`: PASS, 26/26.
- `cargo test -p gsplat-render-wgpu exact_surface --lib`: PASS, 5/5.
- `cargo test -p gsplat-render-wgpu compatibility --lib`: PASS, 20/20,
  including no terminal publication without a presented issued submission.
- `cargo test -p gsplat-render-wgpu --lib`: PASS, 441 passed and 8 retained
  research/device tests ignored.
- `cargo check --workspace` and `cargo test --workspace`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`: PASS.
- `RUSTFLAGS="-D warnings" cargo check -p gsplat-render-wgpu --target
  wasm32-unknown-unknown` and the matching `gsplat-web` command: PASS. These
  are compile results only; no browser/WebGPU runtime is claimed.
- `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test
  conformance_sorted_alpha`: PASS on Apple M4 Metal.
- `cargo test -p desktop-example --features interactive-viewer --test
  surface_geometry_entry`: PASS with
  `M7D_SURFACE_GEOMETRY_ENTRY=PASS backend=Metal adapter="Apple M4"
  public_presenter=true public_session=true standalone_packed_rejected=true
  product_packed_host=true`.
- `PYTHONDONTWRITEBYTECODE=1 python3
  tests/architecture/check_source_architecture.py`: PASS, 133 production Rust,
  17 WGSL and the same 5 grandfather records.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

The first compile exposed a generic result type named `Result`; renaming it to
`Output` restored the intended `std::result::Result` transaction. The next
check identified the now-unused S1 `instance_count` forwarding method, which
was removed before strict lint and the full verification matrix passed.

No device, simulator or browser is launched. This record does not update
`progress.md`, `m7-final-acceptance.md`, any grandfather entry, or the final
Presenter responsibility audit.
