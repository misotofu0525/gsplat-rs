# gsplat-rs Web Example

Before building Wasm or launching the Chrome/WebGPU collector, use the
read-only prerequisite doctor and reusable command profile in
[`handbook/VERIFICATION.md`](../../handbook/VERIFICATION.md#verification-bootstrap).

This directory hosts the browser validation surface.

There are three Web paths:

- Static sampled WebGL2 diagnostic: `index.html` + `src/main.js`, available
  only through the explicit `?gsplat_allow_sampled_webgl=true` opt-in.
- Experimental Rust/WASM renderer package: `crates/gsplat-web`, built into
  `examples/web/pkg/` with the script below.
- Local browser SDK wrapper: `packages/web`, built into
  `packages/web/dist/`.

## Run

Fetch the default CC0 showcase scene and build the Rust/WASM renderer:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash packages/web/scripts/build-wasm.sh
```

Then serve the repository root so the example can fetch shared datasets:

```bash
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

Then open:

```text
http://127.0.0.1:4173/examples/web/
```

The repository collector has a bounded M4 functional smoke that requires real
Chrome WebGPU, keeps the sampled WebGL diagnostic disabled, and records
renderer-owned Exact current-stats plus
source/SH/camera/resolution/presentation facts:

```bash
GSPLAT_M4_SMOKE=1 node examples/web/scripts/collect-web-benchmark-artifact.mjs
```

Its `m4_functional_smoke` result is browser behavior evidence only. It is not a
formal Kitsune performance run or broad device qualification.

The default/product path fails closed when WASM/WebGPU construction or exact
scene admission fails. To inspect the non-equivalent sampled WebGL2 diagnostic
when WebGPU is unavailable, opt in explicitly:

```text
http://127.0.0.1:4173/examples/web/?gsplat_allow_sampled_webgl=true
```

This flag is ignored for formal benchmark/qualification runs; it cannot turn a
sampled diagnostic into a product fallback or quality/performance evidence.

This documentation update does not claim a fresh browser run. Real
Chrome/WebGPU execution at the accepted M7 SHA
`1de3f79fa2fa22955f99c887bea421c918e31ee0` remains **Deferred**; build and
policy-test results cannot substitute for that fixed-SHA endpoint evidence.

Exact construction failures emit `SCENE_LOAD_FAILURE_JSON` with the stable
`stage`, `error_code`, `error_message`, and `scene_published=false` fields.
Capacity failures also carry the limiting resource and byte requirement when
the native error reports them.

Live canvas resizing uses the native asynchronous Surface transaction. Resize
events are coalesced to the newest requested backing size and processed one at
a time; rendering is paused until the transaction publishes. Runtime failures
emit `RUNTIME_RESIZE_FAILURE_JSON` with `stage="resize"` and
`scene_published=true`, reflecting that the complete scene remains owned at
the last successfully published size even though presentation is fail-closed.

Do not open `examples/web/index.html` with `file://`. Browser security rules
block the wasm package and root-relative dataset fetches in that mode.

Open a specific scene directly with:

```text
http://127.0.0.1:4173/examples/web/?dataset=showcase
http://127.0.0.1:4173/examples/web/?dataset=flowers
http://127.0.0.1:4173/examples/web/?dataset=minimal
```

To build the experimental Rust/WASM package for this example:

```bash
bash packages/web/scripts/build-wasm.sh
```

To build the local Web SDK wrapper distribution:

```bash
bash packages/web/scripts/build.sh
```

That script expects `wasm32-unknown-unknown` and the `wasm-bindgen` CLI to
already be installed. It does not install toolchain components for you.

The example first tries the trimmed Wakufactory Kitsune scene at
`tests/datasets/external/wakufactory_kitune/kitune1.ply`. The source model is
CC0, 65.9 MB, and contains 279,199 splats. If it is not installed, startup falls
back to the optional NVIDIA Flowers scene and then to `minimal_ascii.ply`.

Use the scene switcher to move between installed examples, or use `Open your
PLY` to keep a local file entirely in the browser. The Studio panel contains
renderer tuning, live frame timing, the benchmark runner, and diagnostics so
the default view can remain focused on the scene.

Touch and pointer controls mirror the Android validation app:

- one-finger drag / mouse drag: orbit around the loaded scene
- wheel or two-finger pinch: zoom
- two-finger drag: pan
- double tap/click: reset the auto camera

For repeatable browser performance checks, use Android-style query parameters:

```text
http://127.0.0.1:4173/examples/web/?gsplat_benchmark=true&gsplat_benchmark_frames=120&gsplat_benchmark_warmup_frames=10&gsplat_benchmark_yaw_step=0.001&gsplat_surface_sort_interval=2
```

Use the same `gsplat-camera-trace/v1` file and frame as native runs with:

```text
http://127.0.0.1:4173/examples/web/?dataset=minimal&gsplat_camera_trace_url=/tests/perf/trace/fixtures/camera-trace-v1.json&gsplat_camera_frame=0&gsplat_benchmark=true
```

The browser validates the trace contract, locks the camera, uses the trace's
device-pixel display size, and applies the exact camera-to-world quaternion to
the Rust/WASM path. Qualification flags always require that exact path; the
sampled WebGL2 diagnostic cannot satisfy a trace run. There is no preview-size
cap, dynamic-resolution fallback, or implicit upscale. Unsupported dimensions
fail capability admission instead of being silently reduced.

Formal Web evidence uses exactly `1920x1080` and only the Rust/WASM Direct or
full-resident Packed path. Every accepted frame must prove
`requested = Surface = internal render = presented = 1920x1080`,
`source = decoded = encoded = resident = addressable` membership, and the
complete source SH degree. Sampling, LOD, dynamic resolution, and upscaling are
forbidden. The sampled WebGL2 point-splat diagnostic and Paged path remain useful
smoke/diagnostic paths, but neither can produce formal quality or competitor
evidence. The 640x360/640x480 traces and historical comparisons are likewise
smoke-only.

Use ordered moving-camera playback for sort benchmarks with:

```text
http://127.0.0.1:4173/examples/web/?dataset=minimal&gsplat_camera_trace_url=/tests/perf/trace/fixtures/camera-trace-v1.json&gsplat_camera_trace_sequence=true&gsplat_surface_sort_interval=1&gsplat_surface_order_backend=cpu&gsplat_benchmark=true&gsplat_benchmark_sync=true
```

The default sequence applies every trace revision once, with no warmup and one
loop. `gsplat_camera_frame_indices=0,1,2`,
`gsplat_camera_trace_warmup_frames`,
`gsplat_camera_trace_measured_frames`, and `gsplat_camera_trace_loops` make the
schedule explicit. The standard benchmark warmup/frame parameters are aliases
for the trace warmup/measured counts when the sequence-specific values are
absent. Sequence mode requires sort interval `1`. The requested backend may be
`cpu`, `gpu`, or `adaptive`; a non-CPU request fails closed if only the sampled
WebGL2 diagnostic route is active. Console receipts identify every applied
source frame and timestamp.

Projected drawing is selected independently with
`gsplat_surface_projected_policy=candidate|compact|adaptive` (default
`adaptive`). Forced Candidate/Compact frames report the requested policy and
actual execution but never manufacture a projected ticket. Adaptive frames
report issued, not-requested, or explicitly unsampled status; issued high-range
JavaScript-safe tickets terminate exactly once as a success or structured
failure.

For a moving-camera current-stats evidence window, set
`gsplat_order_completion_protocol=sustained_window`. This overlaps a bounded
ledger of renderer current-stats tickets, keeps submitting the trace at
animation-frame cadence, stops drawing after the last measured submission,
then drains every issued ticket. Ring saturation, a missing issue, an unknown,
duplicate, mismatched, unsampled or failed terminal, or final-drain timeout
fails closed. `isolated_terminal` retains its previous behavior and waits for
each ticket before accepting the corresponding frame. The collector derives
`input_to_first_submit_ms`, `submit_span_ms`, `terminal_tail_ms`, and
`terminal_window_ms` exclusively from the page's
monotonic `performance.now()` clock; UTC timestamps are run identity metadata
and are never subtracted for performance results.

This current-stats window is control/correctness evidence, not the Q0
cross-implementation throughput interval. Its manifest uses
`benchmark_window.mode=current_stats_evidence_window`, sets
`performance_evidence=false`, and records a configuration digest plus control
artifact identity. `benchmark-window-mode.mjs` separately defines the
`terminal_queue_throughput_window` state seam. Warmup and the first N-1 timed
frames request no per-frame current stats. The final warmup draw alone carries
an untimed renderer-owned current-stats receipt in its same command buffer;
drawing stops until its Ready `map_async` Result proves the warmup queue is
empty. Only then does the page accept the first measured camera input. Measured
frames submit continuously with no intervening terminal wait; immediately
before the final measured draw, the page requests one separate terminal
receipt whose copy/map is encoded in that draw's command buffer. Drawing then
stops and the page polls only that ticket. Its Ready Result proves the final
submission, and therefore all prior measured work on the same ordered queue,
completed. Missing issue, ring busy, ticket reuse, unknown, duplicate, failed,
mismatched, or timed-out terminal evidence rejects the run without retry. No
benchmark-only queue-fence API is added to the renderer, WASM, or Web package.

Each boundary receipt uses the existing 8-byte readback buffer, a 4-byte
CPU-plan or 8-byte GPU-plan copy, one map operation, and no extra queue
submission. The warmup receipt is untimed. The measured receipt overhead is
included and disclosed as conservative, non-identical terminal proof versus
PlayCanvas's `queue.onSubmittedWorkDone()` Promise. The common monotonic window
starts at `first_measured_input_monotonic_ms`, frozen before the first measured
`setCamera`/order/render call, and ends at the final measured receipt terminal.
Post-render `first_measured_submit_monotonic_ms` and
`last_measured_submit_monotonic_ms` remain separate boundaries;
`input_to_first_submit_ms` makes the first frame's CPU/order/encode cost
explicit. The warmup terminal must precede the first measured input, so no
residual warmup queue tail is admitted.

The standard collector admits B only with an existing A control artifact:

```bash
GSPLAT_BENCHMARK_WINDOW_MODE=terminal_queue_throughput_window \
GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT=/absolute/path/to/control-artifact \
GSPLAT_ORDER_COMPLETION_PROTOCOL=sustained_window \
node examples/web/scripts/collect-web-benchmark-artifact.mjs
```

The canonical Truck 1080p launchbook does not rely on the collector default.
`tests/verification_bootstrap.py command web-webgpu-truck-1080p` prints two
standard-collector commands after the WASM build. The first explicitly selects
`current_stats_evidence_window` with untimed `isolated_terminal` progression
and publishes `control-current-stats/` plus the validated full-quality suite.
Only on success does the second explicitly select
`terminal_queue_throughput_window`, bind
`control-current-stats/manifest.json`, and publish
`throughput-terminal-queue/` with sustained submission. Both use the identical
Truck/trace/resolution/Packed/Adaptive/20+80 workload configuration. The first
collector owns the fresh root; a validated completion marker admits one
throughput stage claim. Any failure stops the sequence, preserves the root, and
forbids automatic retry or overwrite.

The configuration digest must match the control exactly. The final warmup and
final measured tickets must be distinct. The first N-1 timed frames must report
`current_stats_submission=not_requested`; only the final timed frame may report
one `issued` ticket, and its complete identity must join one Ready terminal.
Every frame must use renderer-owned Exact raster, an active
Exact `adaptive_state`, and the corresponding actual
CpuPostSort/GpuPostSort/GpuPreproject plan identity. Exact may intentionally
report `projected_adaptive_state=disabled` because WholePlanController owns the
closed plan; B does not require a particular Candidate/Compact selection.

The result is printed in the Benchmark panel and to the browser console as a
`BENCHMARK_RESULT` line. For headless smoke tests that need the result before
the browser exits, add `gsplat_benchmark_sync=true`. Add `dataset=flowers` to
run the same benchmark against
`tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`.

The Rust/WASM product path streams into an exact-count GPU-resident Packed
scene and supports forced CPU, forced GPU, or measured Adaptive ordering.
Benchmark output reports `renderer=wasm_packed_atlas`. When motion stops, leave
the page visible for at least three frames and confirm the canvas remains
non-black with non-zero Visible/Drawn counts; this guards cached-order redraw.
Packed Exact current-stats control artifacts request a renderer-owned
current-stats receipt for every retained frame. `current-stats-submissions.jsonl` and
`current-stats-terminals.jsonl` must form a one-submission/one-terminal ledger;
the collector joins the complete plan/generation/camera/encode/presentation
identity and rejects missing, stale, mismatched, unsampled, or failed
terminals. `sort_refreshed=true` does not manufacture or require a retired
legacy order ticket. Direct/downlevel compatibility routes may still expose
their historical order-measurement streams.

At the artifact boundary, `dataset.id` is the exact filename identity used by
the collector (for example, `kitune1.ply`), while `dataset.logical_id` records
the dataset-manifest identity (`kitsune`). `dataset.source_path`,
`dataset.sha256`, and the matching streamed-load receipt bind those names to
one input. A logical alias, alternate path, or conflicting hash is rejected;
it is not normalized into acceptable evidence.

Successful renderer current-stats terminals expose `S/V/C/D`: complete
source/residency `S`, near/far candidates `V`, strict conservative
post-projection contributors `C`, and issued draw count `D`. The collector
declares `candidate_visible_contributor_issued_v1`, enforces
`0 <= C <= V <= S`, and requires either `D=V` or explicit exact compaction
with `D=C`. Provisional indirect V/D are unavailable (`null`) in WASM, the ESM
wrapper, live UI, and pending-frame receipts; zero, capacity, and stale counts
cannot fill them. Only the matching terminal populates final frame V/C/D and
marks `visible_count_source=renderer_current_stats_terminal` before the
collector rebuilds `summary.json`. `ordering-window-monotonic.json` is the
page-side window, and the collector independently recomputes and exactly
field-checks the same window before accepting the artifact.
Adaptive compares CPU and GPU with the same `FrameCompletion` interval from
frame start through queue completion, including sorting, projection,
rasterization, submission, and queueing. Order-stage timestamps remain
diagnostic only. A raster-plan change resets the learned comparison state.

The retained browser artifact adds `projected_policy`,
`projected_execution`, `projected_adaptive_state`, and projected submission
identity to every frame. Separate `projected-measurement-submissions.jsonl`,
`projected-measurements.jsonl`, and `projected-measurement-failures.jsonl`
files remain legacy compatibility artifacts. Renderer-owned Exact frames use
the current-stats terminal plan to report actual projected execution and do
not create a second projected learner or terminal owner.

PostSort/Preproject qualification is opt-in and does not alter the normal Web
product default. Set `GSPLAT_GPU_ORDER_PRODUCER=post-sort` or `preproject` on
the headless collector together with `GSPLAT_GEOMETRY_PATH=packed`,
`GSPLAT_ORDER_BACKEND=gpu`, `GSPLAT_PROJECTED_POLICY=compact`, sort interval
`1`, asynchronous benchmark progression, and `isolated_terminal`. The
collector passes the selector through the
strict `gsplat_surface_gpu_order_producer` query, waits for transactional graph
publication, and rejects any frame without matching ProjectedQuadsExact,
forced-Compact, forced-GPU, exact-current producer evidence. It writes the
independent submissions/successes/failures to
`gpu-producer-measurement-submissions.jsonl`,
`gpu-producer-measurements.jsonl`, and
`gpu-producer-measurement-failures.jsonl`. Omitting the environment variable
keeps PostSort, allows CPU/Adaptive frames to report no actual GPU producer,
and requires the producer ticket stream to stay disabled. A formal producer
comparison uses a moving camera trace (or the collector's non-qualification
orbit), because every retained sample must rebuild order and issue a producer
ticket; fixed-camera reuse intentionally fails this strict experiment.

## Scope

- Parses ASCII and binary PLY files in the browser. Packed URL, File, and custom
  stream inputs feed the incremental Rust decoder directly into final Resident
  planes; the sampled WebGL2 diagnostic retains its JavaScript parser.
- Applies the same RDF-to-RUF Y-axis flip used by `gsplat-io-ply`.
- Uses the same DC color and opacity conventions as the Rust renderer.
- Uses shared CPU/GPU/Adaptive exact ordering in the Rust/WASM path. The
  sampled WebGL2 diagnostic is CPU-sorted and is not accepted as GPU benchmark
  evidence.
- Uses the canvas's actual device-pixel dimensions for interactive and fixed
  trace rendering. Unsupported dimensions fail capability admission instead
  of being silently reduced.
- Presents an immersive full-viewport showcase with streamed loading progress,
  theme switching, responsive controls, and a collapsible diagnostics studio.
- Reports Android-style realtime state, camera mode, dataset, path, surface
  size, and frame stats inside the Studio panel.
- Imports the generated `examples/web/pkg/gsplat_web.js` package when present
  and routes renderer creation through `packages/web/src/index.js`. The
  WebGL2 point-splat diagnostic requires explicit opt-in and is never selected
  by the default/product path.
- Supports benchmark orbit runs with `sort_interval` A/B checks.
- Uses one exact-count Resident scene and one SortedAlpha draw path for both
  CPU-uploaded IDs and GPU-generated indirect order.
- Renders a WebGL2 point-splat preview rather than the full `wgpu` ellipse
  pipeline only after `gsplat_allow_sampled_webgl=true`, when the generated
  wasm package is missing or cannot create a browser Surface before an exact
  scene becomes active. Once an exact-count
  WASM scene exists, a render error is terminal for that scene: the example
  retains the WASM handle for receipt draining, reports a structured failure,
  and does not switch to the sampled WebGL2 preview. The standalone receipt
  drain preserves both successes and failures even when presentation fails
  after telemetry collection.
- The Rust/WASM package uses the incremental `gsplat-io-ply` decoder and
  `ResidentSceneBuilder`, then enters the complete product route through
  `SurfaceRenderSession::from_canvas` -> `SurfacePresenterHost` -> the
  renderer-owned `PreparedRuntimeSlot` / Exact runtime. Standalone
  `SurfacePresenter` constructors remain Direct/Paged-only and reject Packed
  before allocation. This shares scene ownership, order policy, and the
  complete Surface lifecycle used by Android/iOS and the desktop viewer.

## Web Integration Boundary

The repo now has a dedicated Rust/WASM boundary in `crates/gsplat-web`. It is
still experimental and must pass the wasm build plus browser smoke path before a
Web renderer change is called complete. `packages/web` is the local ESM
consumer wrapper around that generated wasm package. New Web renderer work
should target `crates/gsplat-web` and the wrapper rather than adding more
rendering logic to `src/main.js`.
