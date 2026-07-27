# Pinned PlayCanvas Competitive Harness

This directory owns the reproducibility boundary for the PlayCanvas side of
the gsplat-rs competitive verification plan. It locks dependency/runtime
identity and includes a fail-closed Chrome smoke that renders a deterministic
minimal scene through WebGPU. It also provides a validated raw-frame collector;
a single collector run is not a competitive result.

## Install and verify

Use the committed lockfile and do not update dependencies during a
qualification run:

```bash
npm ci --prefix tests/competitive/playcanvas
npm test --prefix tests/competitive/playcanvas
npm run smoke --prefix tests/competitive/playcanvas
npm run benchmark:smoke --prefix tests/competitive/playcanvas
npm run benchmark:android-presentation --prefix tests/competitive/playcanvas
npm run benchmark:kitsune-static --prefix tests/competitive/playcanvas
npm run benchmark:truck-1080p --prefix tests/competitive/playcanvas
npm run benchmark:truck-2412x1080 --prefix tests/competitive/playcanvas
```

Before a full Android WebView qualification, the
`benchmark:android-presentation` diagnostic uses the three-splat repository
fixture with the formal 2412x1080 backing. It proves that the natural Android
CSS viewport at the observed device DPR maps to the entire physical surface,
and that the browser canvas and ADB screen captures agree. It is connectivity
and presentation evidence only, never a scene-quality or performance result.

The preflight fails unless all of the following agree with
`expected-engine.json`:

- the exact dependency in `package.json`;
- the version, registry tarball, integrity, and license in `package-lock.json`;
- the installed package metadata;
- the PlayCanvas `version` and abbreviated `revision` exported by the installed
  runtime. The full upstream commit is retained separately because the npm
  runtime exports only its seven-character prefix.

The pinned reference is PlayCanvas Engine `2.21.0-beta.14`, revision
`d5fe88878e338936fe763bbce1a58bc315e89cbe`. PlayCanvas is distributed under
the MIT License; the upstream notice is preserved in `LICENSE.playcanvas`.

## Qualification boundary

The current browser smoke records the selected graphics backend, requested and
resolved splat renderer/sort path, source format, canvas backing dimensions,
and runtime identity. A timed qualification run must additionally freeze LOD
and dynamic-resolution policy, device/browser metadata, dataset hash, shared
camera-trace hash, and the v1 raw-frame artifacts. Those missing signals are
not inferred or fabricated by the path smoke.

Do not commit `node_modules` or generated benchmark artifacts. Raw runs belong
under `target/benchmarks/` as defined by the active verification plan.

The first browser slice requests WebGPU explicitly, loads the repository
`minimal_binary.ply`, verifies the resolved and active GPU-sort renderer, and
writes a pre-timing screenshot plus path signals under
`target/benchmarks/playcanvas-path-smoke/`. If Chrome, WebGPU, asset loading, or
the actual renderer path is unavailable, it writes a blocker and exits nonzero.
The path smoke deliberately collects no timing samples and supports no parity
claim. `benchmark:smoke` adds 30 warmup frames and 60 raw frame samples under
`target/benchmarks/playcanvas-collector-smoke/`, then runs the canonical v1
validator. It measures browser frame-wall spacing and the PlayCanvas
`frameupdate`-to-`frameend` CPU boundary. Neither value is presented as a GPU
phase time. The collector also cancels the scheduled PlayCanvas rAF, waits for
[`app.graphicsDevice.wgpu.queue.onSubmittedWorkDone()`](https://www.w3.org/TR/webgpu/#dom-gpuqueue-onsubmittedworkdone),
whose WebGPU contract covers all work submitted before the call, and verifies
that the engine's monotonic `submitVersion` does not change while the queue is
draining.
Every measured frame also records its pre-render and post-`frameend`
`submitVersion`; the collector rejects missing, non-contiguous, or zero-submit
samples instead of counting an rAF callback as rendered work.
It does this once before measurement, so warmup backlog cannot leak into the
run, and once after the final measured `frameend`, so queued WebGPU work cannot
hide behind the rAF samples. The summary reports the rAF distribution separately
from the submission window, final-drain tail, and sustained terminal throughput.
The first measured frame-wall interval starts only after the warmup drain has
resolved; it therefore records resumed-rAF scheduling latency without charging
the warmup queue tail to the measurement distribution.
Engine-internal per-phase GPU timings that PlayCanvas does not expose remain
null and are declared unavailable. This minimal static-camera run proves
collector mechanics only; it is not a matched competitive qualification result.

`benchmark:kitsune-static` consumes the shared canonical camera trace at
`tests/perf/trace/fixtures/phase-e-kitsune-static-640x480-v1.json`, loads the
exact Kitsune manifest asset, warms up for 120 frames, and measures 3,600
fixed-camera frames. This legacy 640x480 route is diagnostic evidence only; it
cannot support a full-quality or competitor-parity claim. Its output is a
Phase E paired candidate, not a result,
until five predeclared sequential randomized-order pairs have matching receipts,
an SSIM result for every pair, and a passing
`tests/perf/compare-paired-benchmarks.py` result. The comparator intentionally
limits its claim scope to desktop Web Kitsune static; it does not authorize
broader PlayCanvas, native, memory, thermal, energy, or large-scene claims.

Formal competitor qualification uses the `*-quality-1080p-v1` presets. They
load the complete source PLY, require the manifest point count and SH degree to
match both decoded and resident data, and require a 1920x1080 camera trace with
requested, canvas backing, internal-render, CSS, and visual viewport dimensions
all equal. A browser page cannot self-certify physical presentation, so a formal
endpoint collector must add an external presented-frame receipt; the Android
route below does so with a device screencap. LOD, sampling, dynamic resolution,
and upscaling are disabled and recorded in the artifact. The available presets
cover Flowers, Bonsai, Truck, Garden, and Bicycle; `benchmark:truck-1080p` is the
convenient Truck entrypoint.

Use `PLAYCANVAS_CAMERA_MODE=static` for a fixed view or `sequence` to apply the
trace pose and intrinsics before every rendered frame. Warmup and measurement
counts can be overridden with `PLAYCANVAS_WARMUP_FRAMES` and
`PLAYCANVAS_MEASURED_FRAMES`; `PLAYCANVAS_TRACE_FRAME` selects the fixed frame.
`PLAYCANVAS_CAPTURE_TRACE_FRAME` independently selects the frame retained for
the final canvas/device screenshots. When it is unset, the capture frame is the
last measured trace index (the fixed index for `static`, or the final sequence
index for `sequence`).
For example, a moving full-Truck qualification is:

```bash
PLAYCANVAS_CAMERA_MODE=sequence \
PLAYCANVAS_WARMUP_FRAMES=20 \
PLAYCANVAS_MEASURED_FRAMES=600 \
npm run benchmark:truck-1080p --prefix tests/competitive/playcanvas
```

The browser state machine never calls `app.start()` a second time. It cancels
the already-scheduled rAF through the pinned engine's `Application.cancelTick`,
drains the WebGPU queue, and resumes through `requestAnimationFrame()`. Static
and sequence cameras are both applied from the canonical trace in every
captured `frameupdate`, before PlayCanvas update, sort/project, and render.
Warmup and measurement use separate phase indices, so measurement restarts at
the trace's first frame even when warmup ends partway through a trace cycle.
Every measured frame records the live PlayCanvas position, forward/up vectors,
vertical FOV, near/far planes, aspect, view matrix, OpenGL projection and
view-projection matrices, and the WebGPU shader projection and view-projection
matrices. A separate oracle recomputes the canonical trace matrices from its
pose/intrinsics and explicitly converts row-major RUF/+Z/[0,1] into PlayCanvas
column-major RUB/-Z/OpenGL[-1,1], followed by PlayCanvas' WebGPU depth-range
transform. A trace index, FOV, pose, or matrix mismatch fails the run.

The terminal measurement queue drain remains the end of the performance
interval. After it completes, an untimed presentation phase submits the chosen
capture trace frame three times, stops the frame loop, and drains the queue
again. Only the resulting `ready_for_external_capture` state permits the runner
to read `final-frame.png` or invoke ADB screencap. `screenshot-binding.json`
binds both files, their hashes and dimensions, the capture trace index, and the
terminal camera-receipt hash. This extra phase prevents a WebView compositor
from exposing the penultimate sequence frame while the canvas readback already
contains the last frame; it is never included in sustained throughput.

Use `summary.json.sustained_throughput.mean_fps` for the queue-terminal sustained
rate. `distributions.frame_wall_ms` remains useful presentation-cadence evidence,
and `distributions.call_ms` remains a JavaScript/PlayCanvas CPU-boundary metric,
but neither proves GPU completion by itself. The manifest includes both queue
drain receipts, their submission-version boundaries, and the exact timing
source. The generic `gsplat-benchmark/v1` validator accepts these additive
fields and continues to recompute every canonical frame distribution.

The pinned `GSplatHybridRenderer` calls its sort/project path on every forward
frame, including an unchanged camera. Qualification artifacts record that
observed code-path behavior as `sortRefresh=every_frame`; they do not infer it
from camera motion. A single 1080p run is still only a qualification run. A
comparative claim requires matched repeated runs, retained raw frames and
images, and a comparator result whose scope names the exact scene, camera mode,
resolution, browser, and machine.

Formal qualification capture does not use `canvas.toDataURL()`, `toBlob()`, a
Puppeteer screenshot, or a collector-authored renderer receipt. On the final
stable presentation frame, the harness runs inside PlayCanvas' `frameend`
callback, after the pinned `WebgpuGraphicsDevice.frameEnd()` renderer submit
and before the RAF callback returns. It copies that frame's assigned WebGPU
backbuffer texture into a MAP_READ buffer using the engine's own command
encoder/submission path. The terminal producer receipt binds the resulting
RGBA8 digest to the actual renderer/copy submit versions, PlayCanvas frame
sequence, full runtime camera receipt, exact backing resolution, dataset hash,
complete source/resident membership, and SH degree. The stopped frame loop is
then drained before publication.
The camera hash covers the receipt's retained `JSON.stringify` byte string;
cross-language validators hash that exact string and parse it back to the
embedded camera object instead of guessing a different canonical JSON format.

The host writes the renderer bytes to `final-frame.rgba8` and deterministically
encodes `final-frame.png` from those bytes. `renderer-capture.json` keeps the
renderer-owned RGBA receipt and the separate host materialization receipt, so a
PNG encoder cannot impersonate the renderer producer. A device screenshot is
still an independent physical-presentation receipt; it is not substituted for
the renderer image.

## Q1 same-Chrome producer mode

The Q1 producer is an explicit mode of the existing Truck 1080p runner, not a
second renderer or an automatic experiment orchestrator. Each invocation
materializes exactly one fresh artifact:

- a `control` request selects trace 0 or 1 and enables the untimed native
  renderer capture after the 80-frame measurement terminal;
- a `throughput` request binds the exact run ID, manifest SHA-256 and shared
  configuration of both controls, and disables the post-terminal capture/copy
  submission entirely.

Both roles are locked to complete Truck SH3, the two-frame sequence, 20 warmup
plus 80 measured frames, local Chrome/WebGPU and an exact 1920x1080 backing.
They record the actual browser executable and normalized child-process argv,
including a content receipt that redacts only the ephemeral profile path and
CDP port. WebGPU supplies the renderer-selected adapter/device limits; macOS
`sw_vers -buildVersion` supplies the explicit Apple Metal OS driver-stack
identity because WebGPU does not expose a portable driver version. Power and
thermal receipts are sampled around the run. The pinned PlayCanvas runtime tree
and `package-lock.json` are content-addressed before collection and rehashed
after browser/server cleanup. The gsplat-rs repository commit separately pins
the harness source. The artifact directory must be a previously absent child
of `PLAYCANVAS_Q1_SERIES_ROOT`; a failed attempt is not overwritten or retried.

Create a request JSON with schema
`gsplat-q1-playcanvas-producer-request/v1`, then invoke the existing command:

```bash
PLAYCANVAS_Q1_SERIES_ROOT='/fresh/q1-series' \
PLAYCANVAS_Q1_PRODUCER_REQUEST='/fresh/q1-series/requests/playcanvas-control-0.json' \
PLAYCANVAS_ARTIFACT_DIR='/fresh/q1-series/pairs/pair-01/playcanvas/control-0' \
HEADLESS=0 \
PLAYCANVAS_CAMERA_MODE=sequence \
PLAYCANVAS_CAPTURE_TRACE_FRAME=0 \
PLAYCANVAS_WARMUP_FRAMES=20 \
PLAYCANVAS_MEASURED_FRAMES=80 \
npm run benchmark:truck-1080p --prefix tests/competitive/playcanvas
```

For throughput, use `artifact_role: "throughput"`, omit
`PLAYCANVAS_CAPTURE_TRACE_FRAME`, and supply exactly two `control_bindings` in
the request. This path performs zero per-frame presentation-observer reads and
does not issue the post-terminal renderer capture/copy. The producer does not
create a schedule, choose AB/BA order, run five pairs, or authorize a comparison
result. Those remain separate root-owned steps governed by
[`q1-truck-paired-comparison-v1.md`](../../perf/q1-truck-paired-comparison-v1.md).

## Android true-fullscreen WebView remote CDP

Normal Chrome tabs include browser chrome and system navigation in a physical
device screencap. Even when CDP reports a 2412x1080 internal viewport, that tab
is not a 2412x1080 presented competitor result if those controls crop the
canvas. The retained blocker screenshot for such a run is evidence of the
failed presentation gate, not a reason to relax it.

Formal physical Android qualification therefore uses the isolated
[`android-harness`](android-harness/README.md). It hosts the unchanged pinned
page in an immersive, hardware-accelerated System WebView and reuses the
WebView's single CDP page; the runner never creates or closes a second target.
The retained CDP metrics session maps one CSS pixel to one requested backing
pixel for the duration of collection. Pre/post ADB receipts bind the top-resumed
host package, WebView engine package/version, dynamic CDP socket, exact
WindowManager frames, zero insets, and the physical PNG.

On the tested Android device Chromium reports a unit DPR with normal floating
noise and `visualViewport.width=2412.1904296875` for an exact 2412-pixel inner,
canvas, backing, internal target, WindowManager frame, and device PNG. The gate
accepts at most 0.25 CSS pixels for only that visual-viewport report. It samples
the center and four corners one CSS pixel inside the exact surface; the
half-pixel edge samples remain diagnostic because Chromium quantizes the final
right/bottom half pixel out of hit testing. Overlays, wrong backing dimensions,
non-unit scale, nonzero offsets/insets, system bars, and physical size mismatch
still fail closed.

## Android Chrome remote CDP (diagnostic tab route)

The timed runner keeps its desktop launch path by default. Set an explicit
`PLAYCANVAS_CDP_ENDPOINT` to attach to a Chrome instance exposed through ADB.
Remote mode creates and controls one new tab, then closes only that tab and
disconnects; it never closes the device browser or reuses another tab.
It brings that tab to the foreground but never wakes, unlocks, taps, swipes, or
otherwise changes device state. The user must leave the already-unlocked device
with Chrome top-resumed for the whole collection.

Forward Chrome DevTools and reverse the fixed harness port before the run:

```bash
adb -s <serial> forward tcp:9222 localabstract:chrome_devtools_remote
adb -s <serial> reverse tcp:4174 tcp:4174

PLAYCANVAS_CDP_ENDPOINT=http://127.0.0.1:9222 \
PLAYCANVAS_HARNESS_PORT=4174 \
PLAYCANVAS_ADB_SERIAL='<serial>' \
PLAYCANVAS_REFRESH_HZ='<observed refresh rate>' \
PLAYCANVAS_VIEWPORT_WIDTH=2412 \
PLAYCANVAS_VIEWPORT_HEIGHT=1080 \
PLAYCANVAS_CAMERA_MODE=sequence \
PLAYCANVAS_WARMUP_FRAMES=20 \
PLAYCANVAS_MEASURED_FRAMES=80 \
PLAYCANVAS_ARTIFACT_DIR=target/benchmarks/competitive/<fresh-run> \
npm run benchmark:truck-2412x1080 --prefix tests/competitive/playcanvas
```

Remote mode fails closed without an explicit ADB serial, the exact
`serial tcp:9222 -> localabstract:chrome_devtools_remote` forward, an attached
`device` state, an unlocked/awake/on display, and the configured Chrome package
as Android's top-resumed activity. It obtains manufacturer, model, SoC,
fingerprint, Android version, and Chrome package/version through read-only
`get-state`, `get-serialno`, `forward --list`, `getprop`, and `dumpsys` calls;
the CDP user agent and Chrome version must match that ADB receipt. Legacy
`PLAYCANVAS_DEVICE_LABEL` and `PLAYCANVAS_DEVICE_OS` strings are rejected rather
than trusted. `PLAYCANVAS_CHROME_PACKAGE` defaults to `com.android.chrome`.

The page checks `visibilityState=visible`, `hidden=false`, `hasFocus=true`,
unit DPR, exact `innerWidth/innerHeight`, exact unit-scale `visualViewport`,
canvas CSS bounds, canvas backing size, and unobscured canvas hit tests before
measurement, on every measured frame, and after the terminal queue drain. The
benchmark mode hides its own status overlay so it cannot contaminate either the
visible result or the device screen receipt. Browser self-report proves the
page and internal WebGPU surface only; it is not labelled physical presentation.
For remote runs the runner additionally captures the still-visible device with
the read-only `adb exec-out screencap -p` only after the untimed fixed-camera
presentation phase and its terminal drain. It stores `device-screen.png`,
validates its PNG IHDR as exactly `2412x1080`, binds it to the same trace index
as `final-frame.png`, and only then fills the manifest's presented dimensions.
It never sends an unlock or input command.

An earlier `android-device-receipt.json` can be pinned with the recommended
`PLAYCANVAS_DEVICE_RECEIPT_PATH=<path>` input. Stable identity fields must match
the fresh pre/post ADB observations. ADB forward/reverse setup and removal stay
external; after collection remove only the exact mappings with:

```bash
adb -s <serial> forward --remove tcp:9222
adb -s <serial> reverse --remove tcp:4174
```

PlayCanvas exposes the complete active source/resident count but not its GPU
projector's compacted indirect draw count without an intrusive readback. The
artifact therefore records `active_splats=S`, sets `visible` and `drawn` to
`null`, and declares both unavailable; it never relabels `S` as `V` or `D`.

The original repository minimal fixture is ASCII PLY, while the pinned
PlayCanvas parser accepts only binary little-endian PLY. The harness therefore
uses the committed deterministic equivalent generated by
`tests/datasets/generate-minimal-binary.py`; its independent hash and conversion
manifest are under `tests/perf/datasets/`. The Harness does not silently convert
or replace qualification assets at runtime.
