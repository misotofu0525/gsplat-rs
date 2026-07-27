# Desktop Example

Desktop viewer and offscreen PNG smoke harness.

Before running the macOS/Metal conformance or a native Surface check, use the
read-only prerequisite doctor and reusable command profile in
[`handbook/VERIFICATION.md`](../../handbook/VERIFICATION.md#reusable-launchbook-android-web-and-macos).

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

The Q1 comparator may create independent image references with
`tests/perf/collect-q1-truck-direct-reference.py`. That collector is deliberately
not a Web endpoint: it builds this native offscreen executable and selects the
wide-f32 Direct path, stable CPU Full32 ordering, SortedAlpha, the wgpu Direct
global-quad raster, and RGBA8 readback for frozen Truck trace frames 0 and 1.
Its private `--offscreen-reference-receipt` flag fails closed unless all of
those CLI/runtime choices are realized; it does not widen the Rust, C, Swift,
or JavaScript API.

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
diagnostics and never select the backend. Packed Surface rendering is fixed to
the Exact `ProjectedQuadsExact` product raster; the desktop CLI does not expose
the standalone GlobalQuads compatibility plan, and TiledExact has been deleted.
The
`cpu_render_submit_ms` field is CPU wall time and is never presented as GPU
execution time.

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

The Q1 M4 native prerequisite uses a separate feature-gated host because its
terminal-window boundary is stricter than the ordinary throughput diagnostic
and must stay separate from current-stats observer cost.
Do not launch the host directly for retained evidence. First run its focused
tests, then let the one-shot collector build the locked release executable and
claim a fresh ignored output root:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_collect_q1_m4_native.py
python3 tests/perf/collect-q1-m4-native.py \
  --output target/qualification/q1-m4-native-<candidate-sha>-attempt-1
```

The collector admits only clean Apple M4/Metal, complete Truck SH3, the frozen
two-view `1920x1080` trace, Packed `ProjectedQuadsExact`, every-frame sorting,
product Adaptive policies and a 60 Hz host cadence. One untimed correctness
control performs 20 warmup plus 80 measured current-stats members, continuously
polls/recycles the bounded ring and retains complete presentation, submission,
terminal and `V/C/D` ledgers. Adaptive presentations that cannot yet issue the
pending observer ticket are control-only auxiliary presentations. The control
summary explicitly publishes no throughput `N/FPS`.

After the control is fully drained, the same process and frozen identity run a
separate 20+80 presentation stage. That stage never requests or polls
current-stats. After the 20 warmup presents it stops drawing and, outside the
measurement window, completes the warmup queue once through the shared runtime
owner. Only that Ready boundary permits measured trace frame 0 and freezes the
window start. After the 80 measured presents it stops drawing and calls the
existing no-draw `SurfaceRenderSession::pump_receipts` queue-completion owner.
Only this first-measured-camera-input to measured-queue-completion interval
publishes terminal `N/FPS`; it cannot inherit the control's tickets or counts.
Every timed member records the actual Exact whole-plan Adaptive state, executed
PlanId and Candidate/Compact execution. Exact intentionally reports the
independent projected learner as disabled because `WholePlanController` owns
that choice; disabled is recorded rather than rejected, and Compact/Preproject
is not required.
Ring busy or an unsampled/failed control terminal, a missing/duplicate ticket,
timed observer load, incomplete membership, camera/trace/size drift, failed
queue completion, or any draw during a drain fails closed.

The Q1 host is a phase policy, not a second Surface evidence runtime.
`surface_evidence::SurfaceEvidenceRuntime` exclusively owns the live
`SurfaceRenderSession`; both hosts submit narrow request/present/poll/complete
commands and receive owned immutable receipts. It owns current-stats access,
live-camera matrix derivation, render/capture, queue completion and presentation
validation. The Q1 module owns only its frozen identity,
control/timed/capture transitions, cadence and Q1 protocol records.

Only after the timed queue boundary does the host retain view 0 and view 1
PNGs, each joined to the runtime's live-camera receipt, recomputed f32
matrices, camera revision, presentation sequence and Ready control terminal.
The collector binds both stages to one Git SHA, locked binary SHA, scene,
trace, camera and config identity. This command is the native prerequisite
only: PlayCanvas headful external presentation, the common reference-image
gate, and the five outer counterbalanced pairs remain separate Q1 slices.

For the strict M2b real-window evidence route, first fetch/verify the canonical
Kitsune asset and run the focused collector test:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_desktop_surface_evidence.py
```

Then choose a fresh ignored output directory and collect all four Exact plans:

```bash
python3 tests/perf/collect-desktop-surface-evidence.py \
  --dataset-manifest tests/perf/datasets/kitsune.json \
  --trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json \
  --output target/benchmarks/m2b/surface-<candidate-sha>
```

The collector first requires a clean Git tree, pins the qualified Kitsune
asset identity plus the exact two-view trace and 20-warmup/80-measured
schedule, runs the repository trace validator, and builds the locked release
viewer itself. It then requires an actually selected Metal adapter, a
successful real Surface presentation and terminal current-stats receipt for
every scheduled frame, and a final 1920x1080 capture joined to its own terminal
receipt. The collector stages `CpuPostSort`, `GpuPostSort`, `GpuPreproject`,
and `Adaptive` artifacts,
runs `validate-benchmark-artifacts.py` on each, and publishes the suite
directory only after all four pass. An unavailable asset/device/receipt/image
or any contract mismatch is a failed run, never a guessed value. Host-observed
call/frame-wall time is retained; unavailable GPU phase timing remains null.
Published raw capture receipts use artifact-relative `final-frame.png`, not a
staging path.

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
