# Progress

## 2026-07-10

- Branch `feat/simd-radix8-sort`.
- Added `crates/gsplat-sort/src/radix.rs`: 8-bit radix, NEON/AVX2 multi-hist count.
- `sort_values_by_keys` uses key-bit-only stable radix (4 passes).
- Tests: edge lengths, count-vs-scalar oracle, 200k microbench.
- Verified: clippy `-D warnings`, SortedAlpha conformance, kitune/flowers bench.
