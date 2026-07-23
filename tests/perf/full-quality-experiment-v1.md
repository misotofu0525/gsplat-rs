# Full-Quality Cross-Platform Experiment Contract v1

The schema identifier is `gsplat-full-quality-experiment/v1`. It turns the
existing per-run `gsplat-benchmark/v1` artifacts into one auditable matrix for
desktop, Web, Android, and Apple endpoints.

This contract has two jobs:

1. prove that each run loaded and addressed every Gaussian and every SH band
   in the selected input file;
2. keep CPU, GPU, and Adaptive ordering measurements comparable without using
   one fixed point-count threshold as product policy.

It deliberately has no FPS or frame-time pass threshold. Performance is a
measured curve. Identity, full residency, SH preservation, fixed camera,
coverage, and explicit capacity failure are correctness requirements and fail
closed.

## Why static and moving-camera runs are separate

A repeated fixed frame is the right input for image parity, resource receipts,
and steady raster cost. It is the wrong input for comparing sorting backends:
after the first frame a correct renderer reuses its order.

The matrix therefore uses two camera modes:

- `fixed_frame`: select one explicit trace frame, repeat it, capture an image,
  and expect one initial sort followed by reuse;
- `trace_sequence`: consume two or more explicit trace frames in order, loop
  the sequence during warmup and measurement, set `sort_interval` to `1`, and
  require a sort refresh for every measured camera revision.

Both modes use the exact matrices in `gsplat-camera-trace/v1`. They do not
recreate an endpoint-specific orbit. The trace dimensions are the drawable
dimensions; DPR metadata does not silently rescale them.

Every formal artifact, including desktop/Web and both mobile integrations, must
report
`require_display_match=true`, `display_policy=trace_display_exact`, and
`quality_comparable=true`. Native test apps may explicitly replay a trace at a
different drawable aspect for smoke testing, but must label that policy
`native_aspect_reprojection`; the collector rejects it from this matrix because
its projection matrix is no longer the trace's frozen matrix.

Low resolutions such as 640x360 and 640x480 are diagnostic only. The suite
validator enforces a formal floor of 1920x1080 pixels, so changing a label
cannot promote one of those runs. They are
useful for separating sort, binning, and raster costs, but cannot support a
product-readiness or competitor-performance claim. Desktop/Web qualification
uses a 1920x1080 backing surface. The current connected A065 protocol uses its
observed 2412x1080 `SurfaceView`; the iPhone 17 Pro simulator protocol uses its
observed 2622x1206 Metal drawable. These native sizes must be probed again just
before collection. A changed drawable requires a regenerated trace and plan,
not an in-run reprojection. A Mac 3840x2160 run is retained as a separate
full-quality pressure tier. Every comparison uses the same source, camera,
backing dimensions, point membership, and SH degree on both renderers.

"Same camera" across different display aspects means identical pose, vertical
FOV, near/far planes, and view matrix. The projection and view-projection
matrices are generated once in the checked-in trace for the target aspect.
Within one endpoint-sized comparison the complete trace, including projection
matrices, is byte-identical across policies/renderers. Each trace declaration
pins a `camera_family` and a SHA-256 over pose, intrinsics, and view matrices;
the validator rejects cross-endpoint family drift. Unrelated scenes still use
their own manually reviewed two-view composition. A global-extrema auto camera
or generic contract trace is insufficient evidence for outlier-heavy scenes.

## Directory shape

A suite may live anywhere under `target/` and contains one index plus fresh
per-run artifacts:

```text
suite.json
runs/
  <run>/
    artifact/
      manifest.json
      frames.jsonl
      summary.json
    final-frame.png       # required by fixed-quality protocols
```

Every path inside `suite.json` is relative to the suite directory and may not
escape it. Dataset and trace `local_path` values are repository-relative.

Validate a plan without running it:

```bash
python3 tests/perf/validate-full-quality-experiment.py \
  tests/perf/full-quality-matrix-plan-v1.json --allow-incomplete
```

Validate a completed retained suite, including all local input hashes:

```bash
python3 tests/perf/validate-full-quality-experiment.py \
  target/benchmarks/full-quality/<suite>/suite.json --verify-inputs
```

## Top-level fields

`suite.json` contains:

- `schema`, `suite_id`, and `status` (`planned`, `running`, or `complete`);
- `pre_run_requirements`, which may list unresolved work only while the suite
  is planned and must be empty before any retained run starts;
- `build`, whose Git commit and dirty flag may both be `null` only while the
  suite is planned; every retained run must later match the resolved values;
- `renderer_path`, the one exact resident draw path under test;
- `quality_contract`, exactly:

```json
{
  "blend_mode": "sorted_alpha",
  "source_membership": "all",
  "sampling": "disabled",
  "lod": "disabled",
  "sh_degree": "source",
  "resolution_scale": 1.0,
  "capacity_failure": "reject_before_publish"
}
```

- `traces`, `datasets`, `endpoints`, and `protocols`, which declare the matrix
  axes. Formal traces pin dataset identity, frame count, camera family,
  pose/intrinsics receipt, and at least 1920x1080 pixels;
- `runs`, one entry for every available expanded matrix cell;
- `capacity_rejections`, explicit dataset/endpoint failures that happened
  before any partial scene was published.

An available endpoint also declares a `formal_display` with exact width,
height, and provenance. Every protocol referencing it must use exactly those
dimensions and an exact-display trace. An endpoint starts as `probe_required`
only in a planned template. Before a
run starts it must become either `available`, or `unavailable` with the exact
probe and reason. Simulator evidence sets `performance_evidence: false`; it is
still useful correctness and integration evidence but is not physical-device
performance.

## Dataset roles

Every dataset pins `sha256`, bytes, splat count, SH degree, and local path.

- `full_scene` is an unmodified source model and is eligible for image-quality
  claims.
- `scaling_tier` is a deterministic point-count derivative. It must name its
  full-scene source, selection algorithm, and
  `claim_scope: "performance_scaling_only"`.
- `smoke` is a tiny functional fixture.

A scaling tier is not presented as the complete source scene. Once selected as
an input, however, every point and the original SH degree in that tier must be
loaded and rendered. This keeps a point-count crossover experiment honest
without confusing it with full-scene quality evidence.

The recommended curve around the original 200k mobile decision is 50k, 100k,
200k, 300k, 500k, 1M, 1.5M, 2M, and full Truck (2,541,226). Full-scene anchors
are Kitsune (279,199), Flowers (562,974), Bonsai (1,244,819), Truck
(2,541,226), Garden (5,834,784), and Bicycle (6,131,954). Garden and Bicycle
remain required planned inputs even when they still need to be fetched; a
missing file is not silently replaced by a smaller tier.

## Protocol expansion

Each formal protocol explicitly lists dataset IDs, endpoint IDs, sort policies,
repetitions, warmup/measured frame counts, exact display dimensions, a
`randomization_seed`, and the three trace policy receipts. A protocol can name
one `trace_id` when every input is a tier of one source scene, or a
`trace_by_dataset` map for scene-specific fixed-quality views. A `fixed_frame`
protocol expands once per selected frame index. A `trace_sequence` protocol
expands once for the whole sequence. Every axis is then crossed with every
available endpoint and repetition.

The checked-in plan splits formal resolution by endpoint instead of asking one
640x360 protocol to stand in for all of them:

| Endpoint group | Formal pixels | Trace policy |
| --- | ---: | --- |
| macOS Metal + Chrome WebGPU | 1920x1080 | exact 1080p scene trace |
| Android A065 Vulkan | 2412x1080 | exact observed `SurfaceView` trace |
| iOS simulator Metal | 2622x1206 | exact observed drawable trace |

All six full-scene anchors have two fixed views at all three sizes. The Truck
family also drives every moving point-count-ladder protocol. The planned matrix
still expands to 339 cells; the higher resolutions replace the former 640x360
cells instead of reducing endpoint, scene, policy, or repetition coverage.

Run ordering should be randomized within each repetition with a recorded seed.
On thermally managed devices, record state before and after each run and gate
the next run on a declared neutral state. These are experiment controls, not
renderer heuristics.

Each run records a suite-wide unique `schedule_index` and a one-based
`policy_position` within its endpoint/dataset/camera/repetition group. This
makes warm-cache or thermal order bias visible and proves that a multi-policy
group contains each requested policy exactly once.

Recommended phases:

1. `fixed-quality`: full scenes, forced CPU and GPU, at least two fixed views,
   one image per run. Compare CPU/GPU images and compare ResidentCompact with a
   Direct-f32 or segmented Direct oracle where available.
2. `forced-sort-crossover`: the point-count ladder, deterministic moving trace,
   forced CPU and GPU, randomized paired repetitions. This locates crossover
   regions on each actual device; it does not create a universal threshold.
3. `adaptive-policy`: the same complete point-count ladder. Record each backend
   decision, probe, cooldown, fallback, and re-evaluation. Fewer repetitions
   than the forced paired curve are acceptable because this phase validates
   policy behavior rather than estimates a new universal threshold.
4. `simulator-sort-ladder`: the complete point-count ladder with a shorter
   moving trace on available simulators. It proves logic and integration only,
   while still honoring the requirement that every available endpoint sees the
   same small-to-large inputs.

## Required exactness receipt

Every rendered run extends its benchmark `manifest.json` with:

```json
"exactness": {
  "source_splat_count": 2541226,
  "decoded_splat_count": 2541226,
  "encoded_splat_count": 2541226,
  "resident_splat_count": 2541226,
  "addressable_splat_count": 2541226,
  "source_sh_degree": 3,
  "resident_sh_degree": 3,
  "source_membership": "all",
  "sampling": "disabled",
  "lod": "disabled",
  "sh_degree_policy": "source",
  "partial_scene_published": false,
  "full_quality": true
}
```

All five counts must equal the selected file's manifest count. Both SH degrees
must match. Quantization precision is evaluated by image parity; it is never
allowed to alter membership or drop an SH band.

Every retained run also carries a resolution receipt sourced from the trace,
configured Surface, internal render target, and successful presentation:

```json
"resolution": {
  "requested_width": 1920,
  "requested_height": 1080,
  "surface_width": 1920,
  "surface_height": 1080,
  "internal_render_width": 1920,
  "internal_render_height": 1080,
  "presented_width": 1920,
  "presented_height": 1080,
  "dynamic_resolution": "disabled",
  "upscaling": "disabled",
  "full_resolution": true
}
```

Its `trace.reference_width` and `trace.reference_height` must equal both the
protocol display and the requested resolution. This is independent of the
resolution receipt: the former proves the camera projection contract, while
the latter proves that no smaller internal image was stretched to the native
surface.

All four dimension pairs must equal the protocol display. A waiting WebGPU
preparation turn, failed Surface acquisition, internal low-resolution target,
or upscaled frame is not a presented sample and fails closed. Physical display
dimensions may be recorded separately in `environment`; window insets are not
misreported as internal scaling.

Every measured frame uses the auditable `S/V/C/D` chain. `S` is the complete
source/resident/addressable count, `V` is the near/far candidate count (the
historical `visible` field), `C` is the strictly conservative post-projection
contributor count, and `D` is the issued draw count. A new producer declares
`renderer.count_semantics="candidate_visible_contributor_issued_v1"` and emits
`contributor` plus `exact_contributor_compaction` on every frame. It proves
`0 <= C <= V <= S`; exact contributor-only execution requires `D=C`, while a
Direct or downlevel path requires `D=V`. Thus `C<V` is legal only as proven
conservative projection work elimination, never as sampling, LOD, or a draw
budget.

Legacy artifacts without this explicit contract remain compatible and still
must prove `D=V`. Omitting the flag cannot turn `D<V` into acceptable evidence.
A GPU sort-all/draw-all path may conservatively report `V=S` and clip invalid
depths in its shader, as long as that semantics is explicit.

The renderer section additionally records:

```json
"order_backend_requested": "cpu",
"sort_interval": 1
```

The summary's `sort_telemetry` records `cpu_frame_count`, `gpu_frame_count`,
and `gpu_sort_fallback_count`. Forced CPU/GPU runs may not contain another
backend or a fallback. Adaptive runs may select either backend, but their CPU
and GPU frame counts must sum to the sample count. A moving-trace frame must
set `sort_refreshed: true`.

For the detailed report, retain two additional manifest objects whenever the
endpoint can observe them:

```json
"load": {
  "source_read_ms": 0.0,
  "decode_encode_ms": 0.0,
  "gpu_upload_ms": 0.0,
  "first_present_ms": 0.0
},
"resources": {
  "source_file_bytes": 0,
  "cpu_peak_rss_bytes": 0,
  "cpu_steady_bytes": 0,
  "gpu_static_bytes": 0,
  "gpu_order_bytes": 0,
  "gpu_sort_scratch_bytes": 0,
  "max_storage_binding_bytes": 0,
  "max_buffer_bytes": 0
}
```

These zeros illustrate field types, not permitted substitutes for unavailable
measurements. As in `gsplat-benchmark/v1`, an unavailable value is `null` and
its field path is listed in `unavailable_fields`. Descriptor-derived GPU byte
counts and process RSS are labeled separately; neither is misreported as total
physical device memory.

## Images and parity

A protocol with `require_image: true` gives every rendered run a PNG receipt:

```json
"image": {
  "path": "runs/.../final-frame.png",
  "sha256": "...",
  "width": 1920,
  "height": 1080
}
```

The validator checks the bytes, PNG dimensions, and protocol dimensions.
Retain raw images before producing SSIM/diff reports with
`tests/perf/compare-image-ssim.mjs`. CPU/GPU ordering parity compares identical
resident rendering; compact-vs-Direct comparisons measure codec/rendering
quality. A scaling-tier image is diagnostic and cannot replace a full-scene
quality comparison.

## Explicit capacity rejection

A device that cannot admit a complete scene records one rejection for that
endpoint/dataset pair. It covers the corresponding unstarted run cells and
must include source count/degree, structured error code/message, failing stage,
resource kind, required bytes, known limit when available, and
`scene_published: false`.

This is evidence of an honest limit, not a successful render. Reports must
separate rendered cells, capacity-rejected cells, unavailable endpoints, and
missing cells. Publishing a partial image, sampling points, lowering SH, or
relabeling a smaller tier is never a capacity result.
