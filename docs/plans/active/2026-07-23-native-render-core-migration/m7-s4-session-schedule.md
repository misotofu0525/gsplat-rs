# M7 S4 Session schedule ownership

Status: candidate implementation complete; M7 remains **Active**.

Base: `a5ae6ae41234c8f53ea937bdd50b12ed76db3fd4`.

## Scope

This finite slice introduces the private `surface/session_schedule.rs` owner
and moves the shared Session's scheduling mechanics out of
`surface_session.rs`:

- interval, camera-dirty, forced-refresh and order-upload scheduling state;
- the last applied order revision/camera identity and bounded displayed-order
  lag calculation;
- the native Direct + CPU asynchronous worker, its capacity-one request/result
  channels and two-buffer order-ID recycle loop;
- non-blocking request submission, result polling and worker shutdown;
- monotonic revision, maximum-lag and camera-pose admission for completed
  asynchronous orders; and
- stale-result recycling plus the decision to require a synchronous CPU
  fallback when the displayed order exceeds the retained lag/pose envelope.

The generic CPU ordering engine remains in `cpu_order.rs`. The schedule owner
borrows immutable renderer scene positions only when constructing the optional
native worker. It cannot mutate the Renderer, execute a frame, acquire or
present a Surface, poll GPU telemetry/readback, publish receipts/stats/evidence
or advance either Adaptive controller.

## Preserved ownership and behavior

`SurfaceRenderSession` remains the public facade and retains every public Rust
entrypoint, concrete CPU/GPU/Exact frame execution, telemetry and current-stats
collection, compatibility evidence, controller/projected policy state,
fallback choice, and the sole successful-present publication commit. An
admitted async CPU candidate is handed back to Session; Session alone asks the
Renderer to replace the Direct order and commits schedule identity only after
that replacement succeeds.

The scheduling owner returns only private plan/candidate facts. A worker error
keeps the renderer order untouched. A result older than the applied order,
beyond two camera revisions, or outside the translation/rotation envelope is
recycled and reported as dropped; it cannot become the new applied identity.
An unfinished request remains pending and is never treated as a result. When a
fresh result is unavailable beyond the existing envelope, Session retains the
deterministic synchronous CPU fallback.

Async sorting remains native-only and supported only for Direct + CPU. Exact
Packed remains interval 1, Paged remains synchronous, GPU and Adaptive backend
selection retains its existing preparation/fallback semantics, and wasm keeps
the same synchronous schedule. Request submission and result polling use
`try_send`/`try_recv`; worker teardown detaches finite CPU cleanup instead of
joining on the caller. `pump_receipts(Duration)` and all GPU/readback callback
waiting remain outside this owner and issue no new work.

No Renderer/GPU/WGSL/resource layout/pass order/pixel path, public Rust API, C
ABI, FFI/JNI/Swift/Web wrapper or platform binding changes belong to S4. This
slice does not include S5.

## Verification

- `cargo test -p gsplat-render-wgpu surface::session_schedule::tests:: --lib`:
  PASS, 8/8, including interval scheduling, stale revision/lag/pose rejection,
  stale-buffer recycle, fresh recovery, two-buffer reuse and failure without
  renderer publication.
- `cargo test -p gsplat-render-wgpu async --lib`: PASS, 5 passed and the
  existing temporal pixel-tail research oracle ignored.
- `cargo test -p gsplat-render-wgpu current_stats --lib`: PASS, 26/26.
- `cargo test -p gsplat-render-wgpu exact_surface --lib`: PASS, 5/5.
- `CARGO_INCREMENTAL=0 cargo check --workspace`: PASS.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: PASS, including 438
  `gsplat-render-wgpu` unit tests, 8 existing research/device tests ignored,
  and Apple M4 SortedAlpha conformance.
- `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings`:
  PASS.
- `CARGO_INCREMENTAL=0 RUSTFLAGS="-D warnings" cargo check -p
  gsplat-render-wgpu --target wasm32-unknown-unknown`: PASS.
- `CARGO_INCREMENTAL=0 RUSTFLAGS="-D warnings" cargo check -p gsplat-web
  --target wasm32-unknown-unknown`: PASS.
- `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`:
  PASS with the existing grandfather records unchanged.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

The first crate check exposed a mistaken `const fn` annotation and one unused
import; the first strict wasm check exposed one native-only helper without its
target gate. Both were corrected before the passing gates above.

This document is an isolated S4 record. It does not update aggregate progress,
the final M7 acceptance audit or any grandfather entry. No device, browser,
package installation or performance experiment was run.
