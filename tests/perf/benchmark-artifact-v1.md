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

Phase E paired candidates additionally carry a `pairing` object with the same
non-empty `pair_id` and `run_order` in both engine manifests plus complementary
`position` values 1 and 2. `tests/perf/compare-paired-benchmarks.py` requires at
least five pairs and rejects mismatched dataset, trace, display, WebGPU backend,
sample count, warmup count, order, or image-quality evidence before computing
paired ratios and deterministic bootstrap confidence intervals.

`display.refresh_hz_source` and `display.frame_budget_source` identify whether
each value was configured, observed, or supplied by an external harness. A
configured refresh rate must not be presented as an observed display mode.

Full-quality suites require the manifest's additional `resolution` receipt
defined by `full-quality-experiment-v1.md`. Its requested, Surface, internal
render, and presented dimensions must all equal `display.width/height`, with
dynamic resolution and upscaling disabled. A browser CSS size or physical
panel size is not a substitute for the backing texture dimensions.

When `manifest.image` is present, it contains relative `path`, lowercase
`sha256`, `width`, and `height` fields. The generic artifact validator requires
the path to remain inside the run directory, reads the PNG IHDR dimensions,
requires them and the receipt to equal `display.width/height`, and hashes the
actual file. A missing, replaced, damaged, or mismatched image fails validation.

When `renderer.exact_plan_requested` is present but the producer cannot observe
the terminal actual plan, it omits or nulls `renderer.exact_plan_actual` and
lists that exact field path in `unavailable_fields`; a requested path is not an
actual-plan receipt.

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

Timing values are finite non-negative numbers or `null`; available counts and
`elapsed_ns` are non-negative integers. `call_ms` and `frame_wall_ms` are
required measurements and cannot be `null`. `preprocess_ms`, `sort_ms`,
`geometry_submit_ms`, `gpu_wait_ms`, and `gpu_complete_ms` may be `null` when
the producer cannot observe those boundaries. Each unavailable timing must list
its `frames[*].<metric>` path in `manifest.unavailable_fields`; unavailable
values must never be filled with zero. An external renderer that cannot observe
its actual post-cull `visible` and issued `drawn` counts sets both to `null` and
lists `frames[*].visible` plus `frames[*].drawn` in `unavailable_fields`; the
pair must be unavailable together. It may add `active_splats` for a separately
observed source/resident active count, but that field is `S`, not `V`, `C`, or
`D`. `sort_refreshed` may be `null` when the platform cannot provide it.
`elapsed_ns` must be monotonic.

New exact-count renderer artifacts set
`renderer.count_semantics: "candidate_visible_contributor_issued_v1"` and add
both `contributor` and `exact_contributor_compaction` to every frame. Their
count chain is `S/V/C/D`: `S` is the manifest's complete
source/resident/addressable count, `visible` is near/far candidate `V`,
`contributor` is strictly conservative post-projection `C` in stable candidate
rank order, and `drawn` is issued instance count `D`.

Every new-contract frame proves `0 <= C <= V <= S`. With
`exact_contributor_compaction=true`, it must prove `D=C`; this is exact work
elimination, not a point budget or LOD. With the flag false, Direct/downlevel
execution must issue every candidate and prove `D=V`. The contributor and flag
fields are inseparable. Older artifacts remain valid, but the full-quality
suite retains their legacy `D=V` rule; omitted fields never authorize `D<V`.
The new exact-count contract requires observable `V/C/D`; it cannot be combined
with nullable `visible`/`drawn` fields.

Native Surface producers may additionally emit `cpu_frame_complete_ms` and its
summary distribution. This is a ticketed frame-start-to-graphics-queue-done
measurement used for like-for-like CPU/GPU ordering comparisons; CPU submit or
render-call wall time must not be substituted for it.

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

For full-count CPU/GPU/Adaptive coverage across multiple datasets and
endpoints, wrap these per-run artifacts with
`tests/perf/full-quality-experiment-v1.md`. The suite-level validator adds
exact source/decoded/encoded/resident/addressable count receipts, SH-degree
preservation, fixed trace/display matching, unbiased policy scheduling, image
receipts, and explicit pre-publish capacity rejection without changing this v1
run schema.
