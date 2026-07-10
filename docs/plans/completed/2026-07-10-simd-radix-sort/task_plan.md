# Task Plan: SIMD 8-bit CPU Radix

## Status

Complete.

## Goal

Speed up `CpuSortBackend` hot path (count/scatter) with 8-bit radix + NEON/AVX2
multi-histogram count, without changing SortedAlpha semantics.

## Acceptance

- Correctness tests + SortedAlpha conformance pass.
- Production `sort_values_by_keys` sort_ms improved on kitune/flowers vs prior
  16-bit scalar radix.
- Docs updated.
