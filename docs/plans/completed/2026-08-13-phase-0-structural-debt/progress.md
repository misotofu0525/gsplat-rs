# Progress

## 2026-08-13

- Created plan bundle.
- Mapped deletion and split targets from current sources.
- Deleted `GpuOddEvenSortBackend`, `odd_even_sort.wgsl`, sort-crate GPU deps,
  `RenderMode::SortFree`, and C ABI no-ops. Kept `set_async_sort` and the
  Android hidden order-backend knob.
- Split `gsplat-render-wgpu` into focused modules; demoted `GpuInstance`
  expansion to `#[cfg(test)]`; isolated Adaptive/async from the default CPU
  frame path.
- Synced handbook, CHANGELOG, and crate READMEs. ROADMAP item 1 marked landed.

## Verification (2026-08-13)

| Command | Result |
|---------|--------|
| `cargo check --workspace` | pass |
| `cargo fmt --all` | pass |
| `cargo test --workspace --offline` | pass (46 render unit tests + 1 ignored temporal oracle; ffi-c 14; sort 11; SortedAlpha conformance ok without GPU-required env) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | pass |
| `bash tests/ffi/run-ffi-smoke.sh` | pass (`ffi smoke ok`, drawn=2 visible=2) |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha --offline` | pass |

C header vs Rust: public `gsplat.h` symbols match `gsplat-ffi-c` exports.
`gsplat_android_benchmark_set_order_backend` remains Android-only and is
intentionally absent from the public header.

Not run (out of Phase 0 structural-change gate): Android JNI/AAR, Apple
Swift/XCFramework, Web example/npm, cargo-deny, long perf, PlayCanvas
harness.
