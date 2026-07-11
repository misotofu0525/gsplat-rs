# Benchmark Artifact Contract v1

The canonical schema identifier is `gsplat-benchmark/v1`. A run directory
contains at least these required artifacts:

```text
manifest.json
frames.jsonl
summary.json
```

Every object carries the canonical `schema`, a `record_type`, and the same
non-empty `run_id`. JSON numbers must be finite. A metric that cannot be
measured on a platform is `null`; it must never be reported as zero merely
because it is unavailable.

## Manifest

`manifest.json` has `record_type: "manifest"` and these required objects:

- `identity`: `series_id`, whole-run `started_at_utc`/`ended_at_utc`, and
  measurement-only `measurement_started_at_utc`/`measurement_ended_at_utc`;
- `build`: repository commit, dirty flag, profile, package version; commit and
  dirty state may be `null` only when their paths appear in
  `unavailable_fields`;
- `dataset`: ID, SHA-256, byte and splat counts, SH degree;
- `trace`: ID and SHA-256;
- `renderer`: implementation, path, backend, and sort policy;
- `display`: width, height, DPR, refresh rate, and frame budget;
- `environment`: platform, OS, device, browser, adapter, and driver;
- `unavailable_fields`: JSON field paths whose values are unavailable.

Unknown fields are allowed so later Phase A collectors can add metadata without
changing the v1 identity. Required unavailable values use `null` and name their
field path in `unavailable_fields` when the field-specific rule permits it.
Producers should add a `timing.frame_wall_source` field naming the presentation
boundary or proxy. Synchronous throughput loops are smoke/microbenchmark data,
not end-to-end frame-wall evidence.

`display.refresh_hz_source` and `display.frame_budget_source` identify whether
each value was configured, observed, or supplied by an external harness. A
configured refresh rate must not be presented as an observed display mode.

## Frames

Each non-empty line in `frames.jsonl` is one `record_type: "frame"` object.
`frame_index` starts at zero and is contiguous. Required fields are:

```text
elapsed_ns
call_ms
frame_wall_ms
preprocess_ms
sort_ms
geometry_submit_ms
gpu_wait_ms
gpu_complete_ms
visible
drawn
sort_refreshed
```

Timing values are finite non-negative numbers or `null`; counts and
`elapsed_ns` are non-negative integers. `call_ms`, `frame_wall_ms`,
`preprocess_ms`, `sort_ms`, and `geometry_submit_ms` are required measurements
and cannot be `null`. `gpu_wait_ms`, `gpu_complete_ms`, and `sort_refreshed`
may be `null` when the platform cannot provide them. `elapsed_ns` must be
monotonic.

## Summary

`summary.json` has `record_type: "summary"`, `sample_count`, `warmup_count`,
`frame_budget_ms`, `missed_frame_count`, and a `distributions` object.
Distributions are required for:

```text
call_ms
frame_wall_ms
preprocess_ms
sort_ms
geometry_submit_ms
gpu_wait_ms
gpu_complete_ms
```

Each distribution is either `null` when all frame values are unavailable, or:

```json
{"count": 5, "mean": 3.0, "p50": 3.0, "p90": 5.0, "p95": 5.0, "p99": 5.0, "max": 5.0}
```

Percentiles use nearest rank: sort ascending and select
`max(ceil(p * count) - 1, 0)` for `p` in `0.50`, `0.90`, `0.95`, and `0.99`.
Means use a left-to-right `f64` sum in original frame order followed by division
by count. The validator compares derived floating-point values with absolute
tolerance `1e-9`. A missed frame is one whose non-null
`frame_wall_ms` is strictly greater than `frame_budget_ms`.

Run the standard-library validator and deterministic fixture suite with:

```bash
python3 tests/perf/validate-benchmark-artifacts.py tests/perf/fixtures/v1/valid
bash tests/perf/test-benchmark-artifacts.sh
```
