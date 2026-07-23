# Desktop Example

Desktop viewer and offscreen PNG smoke harness.

Run the deterministic PNG smoke from the repository root:

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Reproduce one canonical camera frame (the trace display size is applied
automatically, and conflicting width/height or orbit flags fail closed):

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply \
  --camera-trace tests/perf/trace/fixtures/camera-trace-v1.json \
  --camera-frame 0 --frames 10
```

`--frames` repeats the selected fixed frame; it does not advance or recreate
the path through desktop-specific camera controls.

Replay every camera revision exactly once (the sequence default) with:

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply \
  --camera-trace tests/perf/trace/fixtures/camera-trace-v1.json \
  --camera-sequence
```

For a longer deterministic sequence run, add (for example)
`--camera-frame-indices 0,1,2 --camera-warmup-frames 20
--camera-measured-frames 80 --camera-loops 3`. Warmup runs once; each measured
loop restarts at the first selected index and then follows the declared order.
Sequence mode rejects `--frames` and requires its measured-frame option
instead. Fixed-frame playback also accepts warmup/measured counts, but not
`--camera-loops`; its measured count defaults to `--frames`. Every applied
revision prints the trace ID/hash, phase, loop, source
frame index, trace timestamp, and the requested backend (`cpu` for this
offscreen desktop entrypoint).

For a reproducible native Surface run, enable the viewer feature and combine
the same validated trace with `--interactive`. The automated trace window is
hidden, but it still creates and presents to a real wgpu Surface (Metal on
macOS); it does not use the offscreen texture path. It forces a complete order
refresh for every scheduled warmup and measured sample, uses sort interval 1,
and exits without waiting for a person to close the window. Formal desktop
evidence uses exactly `1920x1080`; 640x360/640x480 traces are smoke or historical
diagnostics only, and 3840x2160 is an additional pressure test rather than a
replacement formal resolution:

```bash
cargo run --release -p desktop-example --features interactive-viewer -- \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path packed --interactive \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json \
  --camera-sequence --camera-warmup-frames 20 \
  --camera-measured-frames 80 --camera-loops 3 \
  --surface-benchmark-mode isolated \
  --order-backend adaptive
```

Use `--order-backend cpu` and `--order-backend gpu` with the identical command
for controlled A/B runs. Surface rendering defaults to `packed` geometry,
the `ProjectedQuadsExact` hardware-raster product path, `adaptive` ordering,
and sort interval 1; `adaptive` reports the backend
actually used and its current learning/probe state on every frame. Offscreen
rendering has no Surface GPU-order path and therefore remains CPU ordered. GPU
measurements are joined asynchronously by ticket and camera revision.
Adaptive's primary comparison is `FrameCompletion` for both backends: frame
start through queue completion, including ordering, projection, rasterization,
submission, and queued GPU work. Order-stage wall/timestamp fields remain
diagnostics and never select the backend. Changing `--surface-raster-plan`
resets Adaptive learning before the new plan is sampled because measurements
from different raster plans are not comparable. The `cpu_render_submit_ms`
field is CPU wall time and is never presented as GPU execution time.

Each scheduled frame emits one `SURFACE_FRAME_RECEIPT` line with trace and
session revisions, requested/actual backend, adaptive state, sort refresh,
source/resident/visible/drawn counts, pending count revision, CPU phases, and
the newest GPU telemetry receipt. The begin, frame, and summary receipts also
record requested, actual Surface, internal-render, and presented dimensions,
the exact raster plan, and the disabled dynamic-resolution/upscaling flags. A
mismatch or a scheduled call that did not actually present fails the native
benchmark instead of being counted as a full-resolution frame. Completed
tickets also emit either
`SURFACE_CPU_MEASUREMENT` or `SURFACE_GPU_MEASUREMENT`; the run ends with one
`SURFACE_BENCHMARK_SUMMARY`. The default `isolated` benchmark does not start a
new trace sample while a ticket is outstanding. It polls the existing Surface
submission without acquiring, drawing, or presenting another frame, so queue
completion is not distorted by artificial drain draws. This mode measures
isolated latency and intentionally does not report FPS.

The terminal CPU/GPU lines use one count contract: `S` is the complete
resident/addressable source, `V` is the near/far candidate set, `C` is the
strict conservative post-projection contributor set, and `D` is the count
actually issued to drawing. They declare
`count_semantics=candidate_visible_contributor_issued_v1` and require
`0 <= C <= V <= S`. `D=C` is legal only with
`exact_contributor_compaction=true`; Direct and downlevel execution retain
`D=V`. Frame-level V/D fields are provisional telemetry and never substitute
for the terminal ticket-and-camera-revision receipt.

Use continuous mode to measure actual full-quality Surface throughput under
normal queue contention:

```bash
cargo run --release -p desktop-example --features interactive-viewer -- \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path packed --interactive \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json \
  --camera-sequence --camera-warmup-frames 20 \
  --camera-measured-frames 80 --camera-loops 1 \
  --surface-benchmark-mode throughput \
  --order-backend adaptive
```

With the default `--surface-sort-policy every-frame`, continuous mode forces
exact ordering on every scheduled trace frame and keeps rendering while earlier
receipts are pending. It reports
`surface_throughput_fps` from measured Surface frame start-to-start intervals,
then stops drawing and polls until every issued CPU/GPU ticket has exactly one
terminal success or failure. A busy telemetry ring is reported explicitly as
an unsampled frame; an issued ticket is never silently discarded. Automated
full-quality runs accept Direct and Packed; they reject the legacy Paged path
because it does not keep every source splat resident. They also require
`requested = Surface = internal render = presented = 1920x1080`, complete
`source = decoded = encoded = resident = addressable` membership, and full
source SH.
Sampling, LOD, dynamic resolution, and upscaling are forbidden.

The competitor's static-camera throughput mode applies one trace pose and then
reuses its order. Reproduce that distinct workload without weakening geometry,
SH, resolution, or blending by selecting one repeated trace frame and refreshing
only when the camera changes:

```bash
cargo run --release -p desktop-example --features interactive-viewer -- \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path packed --interactive \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json \
  --camera-frame 0 \
  --camera-warmup-frames 20 --camera-measured-frames 120 \
  --surface-benchmark-mode throughput \
  --surface-sort-policy camera-change --order-backend cpu
```

`camera-change` still performs and verifies the initial exact sort; it then
requires `sort_refreshed=false` while the identical pose repeats. Projected's
rank-indexed planes are also reused only while the order owner/generation,
complete camera, viewport, and draw-count guard are identical; every measured
frame still submits the complete hardware draw. The default
`every-frame` policy remains the moving-camera/order stress experiment. Keeping
the policy explicit prevents a static 60 Hz raster result from being compared
to a two-pose benchmark that deliberately performs a full sort each frame.

For a controlled raster A/B, add `--surface-raster-plan projected` (the
product default), `--surface-raster-plan global` (the full-count Resident
vertex-projection reference), or `--surface-raster-plan tiled`. All three
retain the complete resident source, full SH degree, exact CPU/GPU order,
native output resolution, and the same fragment alpha contract. `tiled` is
deliberately an explicit quality oracle and pressure diagnostic; it is not
selected automatically and its software per-pixel composition throughput is
not a product result. The flag requires `--interactive --geometry-path packed`
so an offscreen or Direct run cannot silently claim it exercised the selected
Surface path.

The offscreen harness uses complete CPU ordering. Its default Packed loader
streams the PLY directly into the exact-count resident representation; pass
`--geometry-path direct` for the wide-float reference pipeline. The selected
path is printed as `offscreen_geometry_pipeline`.

```bash
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10
```

Run the interactive viewer when validating windowed presentation or camera
interaction. It uses the shared `SurfaceRenderSession` also used by Web and
mobile. Its product default is full-resident Packed with adaptive CPU/GPU
ordering; `--geometry-path direct --order-backend cpu` selects the explicit
wide-float reference path without changing point count or shading degree:

```bash
cargo run -p desktop-example --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```

Windowed and offscreen runs use the same selected geometry encoding; only the
Surface session can select GPU or adaptive ordering.

When `--geometry-path packed` is selected before loading, the PLY file is
decoded one splat at a time directly into the exact-count resident encoding.
The loader never constructs a temporary wide `SceneBuffers`; Direct and Paged
continue to load the wide representation for reference and diagnostic use.
