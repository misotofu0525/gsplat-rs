# M7 S3 Session evidence ownership

Status: candidate implementation complete; M7 remains **Active**.

Base: `77761aed8a6b79c3fc986b20a813c0221a5048f7`.

## Scope

This finite slice completes the private pure-state evidence owner in
`evidence/compatibility.rs`. `SessionEvidence` now owns:

- immutable order, projected-draw and producer submission/terminal receipts;
- independent ticket-identity joins for those three namespaces;
- bounded order/projected success and failure queues with oldest eviction;
- bounded compatibility views over the lossless raw producer success/failure
  queues;
- take-once order/projected V/C/D ledgers and their Pending, Failed, Expired,
  Consumed and InvalidTicket results;
- pure mapping from a completed `SurfaceFrameOutput` into compatibility
  submissions; and
- pure mapping from an Exact current-stats terminal into the matching
  compatibility order success/failure.

The initially considered `surface/session_evidence.rs` location was rejected by
the source-architecture gate as `dependency.host.adaptive_state`: files under
`surface/` are platform hosts and may not declare the Adaptive evidence DTOs.
Keeping the complete owner in the existing private `evidence/compatibility.rs`
leaf preserves that dependency direction without changing the policy checker.

## Preserved ownership and behavior

`SurfaceRenderSession` remains the public facade and retains all frame-control
responsibilities: telemetry and current-stats polling, concrete frame
execution, current-stats publication, the sole successful-present commit,
ordering/projected learning, cross-controller arbitration, scheduling and the
native async worker. The evidence owner only receives already-produced copy
values and never acquires a Surface, polls a device, encodes, submits, presents,
requests telemetry or performs readback.

Compatibility methods on `SurfaceRenderSession` retain their public names and
non-blocking behavior. A selected compatibility terminal already queued is
still consumed before Session advances existing callbacks. Count receipts
remain independent and take-once. An unregistered or late terminal cannot
create a success or counts receipt; unresolved counts remain unavailable and
are never replaced by zero, capacity or an older value.

The five bounded external evidence queues retain capacity 64 and oldest
eviction. Raw GPU producer success/failure delivery remains FIFO and lossless
beyond the bounded compatibility window. Duplicate terminals remain ignored,
republished tickets invalidate older counts, and the three ticket namespaces
remain independent.

No Renderer/GPU/WGSL/resource layout/pass order/pixel path, public Rust API,
C ABI, FFI/JNI/Swift/Web wrapper or platform binding changes in S3. Failed or
discarded frame transactions still cannot publish renderer cache/state, and
the evidence ledger additionally ignores a terminal without a matching issued
submission.

## Verification

- `cargo test -p gsplat-render-wgpu evidence::compatibility::tests:: --lib`:
  PASS, 15/15, including bounded retention, lossless producer raw queues,
  ticket namespaces, non-blocking poll/take and no-terminal-without-issue.
- `cargo test -p gsplat-render-wgpu --lib`: PASS, 437 passed and 8 existing
  research/device tests ignored.
- `cargo test -p gsplat-render-wgpu exact_surface --lib`: PASS, 5/5.
- `cargo test -p gsplat-render-wgpu compatibility --lib`: PASS, 19/19.
- `cargo test -p gsplat-render-wgpu current_stats --lib`: PASS, 26/26.
- `CARGO_INCREMENTAL=0 cargo check --workspace`: PASS.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: PASS, including the workspace
  FFI translation tests and Apple M4 SortedAlpha conformance test.
- `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings`:
  PASS.
- `CARGO_INCREMENTAL=0 RUSTDOCFLAGS="-D warnings" cargo doc --workspace
  --no-deps`: PASS.
- `CARGO_INCREMENTAL=0 RUSTFLAGS="-D warnings" cargo check -p
  gsplat-render-wgpu --target wasm32-unknown-unknown`: PASS.
- `CARGO_INCREMENTAL=0 RUSTFLAGS="-D warnings" cargo check -p gsplat-web
  --target wasm32-unknown-unknown`: PASS.
- `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`:
  PASS with the existing grandfather records unchanged.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

The first workspace attempt stopped before a code result with `os error 28`
while writing Cargo metadata. Only this worktree's rebuildable `target/debug`
cache was removed; `target/benchmarks` and all retained artifacts were
preserved. The complete workspace commands then passed with incremental
compilation disabled.

This document is an isolated S3 record. It does not update aggregate progress,
the final M7 acceptance audit or any grandfather entry. No device, browser,
package install or performance experiment was run.
