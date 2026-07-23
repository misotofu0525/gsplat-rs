# iOS Example

UIKit realtime Surface sample app for the local `GsplatKit` wrapper.

The app uses the same Kitsune showcase and editorial first frame as the Web
and Android examples. Compact live telemetry stays on the canvas; tap
`Studio` for the complete ABI, Surface, camera, dataset, and path diagnostics.

Build and run the simulator app from the repository root:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
```

The app bundle includes `camera_trace.json` (override its source with
`GSPLAT_CAMERA_TRACE_PATH`). Select one fixed frame for a simulator benchmark:

```bash
bash bindings/apple/scripts/run-ios-sim-app.sh tests/datasets/minimal_ascii.ply -- \
  --gsplat_benchmark true --gsplat_benchmark_frames 10 \
  --gsplat_camera_trace camera_trace.json --gsplat_camera_trace_frame 0 \
  --gsplat_require_trace_display_match false
```

The bundled 640x360 trace command above is deliberately smoke-only: explicit
native-aspect reprojection marks its artifact as not quality-comparable. Formal
Apple evidence currently targets the iPhone 17 Pro simulator at its native
`2622x1206` drawable and must use a validated trace with exactly that `display`;
a mismatch fails before measurement. The formal benchmark requests landscape
before renderer creation and rejects a portrait/unsettled Surface. Its native
receipt must prove
`requested = Surface = internal render = presented = 2622x1206`, an actually
presented terminal frame,
`source = decoded = encoded = resident = addressable` membership, and the
complete source SH degree. Sampling, LOD, dynamic resolution, and upscaling
are forbidden, and Paged cannot qualify. Physical devices and other simulator
sizes remain exploratory until the formal matrix is deliberately revised.
Physical panel dimensions remain environment metadata only. The benchmark
skips its normal yaw orbit. Trace application is an example-only qualification
hook; ordering and exactness use the public C/Swift Surface APIs.

Replay the trace as a moving-camera benchmark with:

```bash
bash bindings/apple/scripts/run-ios-sim-app.sh tests/datasets/minimal_ascii.ply -- \
  --gsplat_benchmark true --gsplat_camera_trace camera_trace.json \
  --gsplat_camera_trace_sequence true --gsplat_camera_frame_indices 0,1,2 \
  --gsplat_surface_sort_interval 1 --gsplat_surface_order_backend cpu \
  --gsplat_surface_projected_policy adaptive \
  --gsplat_require_trace_display_match false
```

This bundled-trace sequence is also smoke-only. Omit the opt-out for formal
evidence and provide a trace generated for the actual drawable dimensions.

The sequence default is all trace revisions once, no warmup, and one loop.
`gsplat_benchmark_warmup_frames`, `gsplat_benchmark_frames`, and
`gsplat_camera_trace_loops` extend that deterministic schedule. CPU, GPU, and
adaptive selection use the public Surface order-backend API; only the trace
schedule is sample-specific. Adaptive compares the same `FrameCompletion`
interval for both backends, from frame start through queue completion and
including sorting, projection, rasterization, submission, and queued work.
Order-stage timestamps are diagnostic only; a raster-plan change resets its
learned state. Each applied revision logs the trace identity, source frame
index, timestamp, phase, loop, and requested backend. The emitted artifact also
carries the native source-to-GPU exactness receipt, rejects Paged outright, and
requires every source splat and the source SH degree to remain resident and
addressable without sampling or LOD.

The sample records the public CPU/GPU submission ticket after every successful warmup,
measured, and terminal-flush frame, then keeps pumping the renderer until every
issued ticket has exactly one success or failure. CPU receipts use the same
frame-start-to-queue-completion interval as GPU receipts. It refuses to emit an
artifact on ring-busy, Surface-unavailable, queue loss, terminal failure,
fallback, or timeout.

Projected Candidate/Compact selection has an independent launch control:
`gsplat_surface_projected_policy=candidate|compact|adaptive` (default
`adaptive`). Every retained frame records requested policy, actual execution,
Adaptive state, and issued/unsampled identity. Adaptive tickets live in a
separate namespace and each must reach one same-revision/execution/order-lane
terminal with preserved projection/probe generations. The collector drains a
success's V/C/D receipt immediately and refuses a frame that issues both an
order and projected formal ticket. Forced modes execute normally but never
fabricate projected measurement tickets.

Every success is paired with the additive, ticket-addressed `S/V/C/D` count
receipt: complete source/residency `S`, near/far candidates `V`, strict
conservative post-projection contributors `C`, and issued draw count `D`.
Joining requires the same ticket and camera revision. Formal artifacts enforce
`0 <= C <= V <= S`; only an explicit exact-compaction flag permits `D=C`, while
Direct/downlevel execution remains `D=V`. Frame stats are provisional and do
not backfill a missing terminal count.

The build script prefers Kitsune, falls back to the shared NVIDIA Flowers
fixture, and accepts an explicit PLY path as its first argument. `Open PLY +`
continues to use the native document picker.

The Swift package wrapper, Swift smoke path, XCFramework build, and device
scripts live under `bindings/apple/`. See `bindings/apple/README.md` for the
full simulator/device validation matrix.
