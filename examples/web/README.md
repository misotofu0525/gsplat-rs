# gsplat-rs Web Example

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
Chrome WebGPU, disables the sampled WebGL fallback, and records renderer-owned
Exact current-stats plus source/SH/camera/resolution/presentation facts:

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
sampled preview into quality or performance evidence.

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
forbidden. The sampled WebGL2 point-splat fallback and Paged path remain useful
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
`cpu`, `gpu`, or `adaptive`; a non-CPU request fails closed if only the WebGL2
fallback is available. Console receipts identify every applied source frame
and timestamp.

Projected drawing is selected independently with
`gsplat_surface_projected_policy=candidate|compact|adaptive` (default
`adaptive`). Forced Candidate/Compact frames report the requested policy and
actual execution but never manufacture a projected ticket. Adaptive frames
report issued, not-requested, or explicitly unsampled status; issued high-range
JavaScript-safe tickets terminate exactly once as a success or structured
failure.

For a moving-camera throughput run, set
`gsplat_order_completion_protocol=sustained_window`. This keeps submitting the
trace at animation-frame cadence and drains every ordering receipt after the
last measured submission. `isolated_terminal` instead waits for each ticket
before accepting the corresponding frame and is useful for per-frame terminal
latency, not sustained FPS. The collector derives `submit_span_ms`,
`terminal_tail_ms`, and `terminal_window_ms` exclusively from the page's
monotonic `performance.now()` clock; UTC timestamps are run identity metadata
and are never subtracted for performance results.

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
Ordering artifacts require every submitted CPU or GPU measurement ticket to
complete and match its backend and camera revision; one animation frame may
drain multiple receipts. Every frame with `sort_refreshed=true` must carry a
positive namespaced ticket. A ticket must terminate exactly once as either a
successful measurement or a structured `readback_map` /
`generation_invalidated` failure; strict GPU/Adaptive benchmarks reject the
failure instead of manufacturing a timing/count sample. An explicit Adaptive
GPU setup fallback is likewise recorded and rejected by the strict collector.
The separate `order-measurement-submissions.jsonl` ledger includes preflight,
warmup, and measured CPU/GPU submissions, so every ticket issued during the
benchmark must have exactly one terminal receipt even when it is not part of
the measured-frame summary. The first browser-only Packed GPU-order preparation
is hidden: it presents no frame, allocates no ticket, and is recorded separately
in `gpu-order-preparations.jsonl` before the same camera revision is retried.

Successful CPU and GPU terminal receipts expose the same `S/V/C/D` evidence:
complete source/residency `S`, near/far candidates `V`, strict conservative
post-projection contributors `C`, and issued draw count `D`. The collector
joins them only by the same ticket and camera revision, declares
`candidate_visible_contributor_issued_v1`, and enforces
`0 <= C <= V <= S`. Exact compaction must explicitly report `D=C`; otherwise
the portable Direct/downlevel rule remains `D=V`. Provisional frame counters
cannot fill a missing terminal receipt.
Because an asynchronous frame can initially expose the preceding receipt's
counts, the collector replaces GPU submission-frame `visible`, `drawn`, and
GPU-order timing fields from the matching terminal receipt, records the count
source/revision/ticket, and rebuilds `summary.json` from those post-join
frames. Frames without a current CPU revision or a joined GPU submission are
marked ineligible and excluded from `summary.json.count_evidence`; harvested
GPU completion timing is likewise cleared from non-submitting frames to avoid
double-counting. Only the collector-written `frames.jsonl` and rebuilt
`summary.json` are final evidence; the corresponding live console-frame counts
are provisional. `ordering-window-monotonic.json` is the page-side window, and
the collector independently recomputes and exactly field-checks the same window
before accepting the artifact.
Adaptive compares CPU and GPU with the same `FrameCompletion` interval from
frame start through queue completion, including sorting, projection,
rasterization, submission, and queueing. Order-stage timestamps remain
diagnostic only. A raster-plan change resets the learned comparison state.

The retained browser artifact adds `projected_policy`,
`projected_execution`, `projected_adaptive_state`, and projected submission
identity to every frame. Separate `projected-measurement-submissions.jsonl`,
`projected-measurements.jsonl`, and `projected-measurement-failures.jsonl`
files preserve the one-ticket/one-terminal ledger with V/C/D,
projection/probe generations, and frame-completion timings.

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
- The Rust/WASM package uses the incremental `gsplat-io-ply` decoder,
  `ResidentSceneBuilder`, `SurfacePresenter::from_canvas`, and
  `SurfaceRenderSession`, so it shares scene ownership, order policy, and the
  complete Surface lifecycle used by Android/iOS and the desktop viewer.

## Web Integration Boundary

The repo now has a dedicated Rust/WASM boundary in `crates/gsplat-web`. It is
still experimental and must pass the wasm build plus browser smoke path before a
Web renderer change is called complete. `packages/web` is the local ESM
consumer wrapper around that generated wasm package. New Web renderer work
should target `crates/gsplat-web` and the wrapper rather than adding more
rendering logic to `src/main.js`.
