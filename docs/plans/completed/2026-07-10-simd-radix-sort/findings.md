# Findings

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| 8-bit digits (256 buckets) | Histogram fits L1; 16-bit hist (~512KB) hurts mobile locality. |
| Multi-hist count (H=4) + NEON/AVX2 digit extract | Avoids write conflicts; matches prior mobile SIMD wins on count. |
| Scalar scatter | Random stores; full SIMD scatter unstable / low ROI. |
| `sort_values_by_keys` = key-bits only (4 passes) | Packed `key<<32\|!idx` + stable LSD preserves ascending index among equal keys when packed in index order. Cuts pass count vs full 64-bit. |
| `sort_pairs` = full 64-bit | Arbitrary values still need `!value` low bits for tie-break. |

## Rejected / Deferred

- Buffered per-digit scatter: measured slower on M4 Pro (extra copies + stack traffic).
- GPU radix: out of scope for this pass.

## Perf (2026-07-10, Apple M4 Pro)

Baseline (prior 16-bit scalar radix, same bench protocol): kitune `avg_cpu_sort_ms≈0.75`, flowers `≈1.76`.

After change:

| Workload | Metric | Result |
|----------|--------|--------|
| microbench `sort_values_by_keys` n=200k | avg_ms | **0.57** |
| kitune bench-runner | avg_cpu_sort_ms | **0.46** (~1.6×) |
| flowers bench-runner | avg_cpu_sort_ms | **1.54** (~1.14×) |

SortedAlpha conformance: pass.
