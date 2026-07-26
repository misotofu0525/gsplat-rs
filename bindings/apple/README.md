# GsplatKit

Before Swift, XCFramework, or simulator verification, use the read-only
prerequisite doctor and reusable command profiles in
[`handbook/VERIFICATION.md`](../../handbook/VERIFICATION.md#verification-bootstrap).

Apple binding, Swift wrapper, and local packaging scripts.

## Integration boundary

This directory validates Swift -> C ABI -> Rust and includes the local
`GsplatKit` Swift package wrapper. The runnable UIKit Surface sample app lives
under `examples/ios/app`. Tagged GitHub prereleases provide a directly
downloadable XCFramework ZIP, but `GsplatKit` is not a remote binary SwiftPM
package.

The public native contract lives in `crates/gsplat-ffi-c/include/gsplat.h`.
Use the helper functions and named constants from that header instead of copying
magic numbers into Swift:

```swift
var config = gsplat_config_default()
var camera = gsplat_camera_default()
let message = String(cString: gsplat_error_message(rc))
```

Use `gsplat_last_error_message()` through `GsplatKitError` when you need the
most recent operation detail. Raw native Surface handles are single-owner
handles; `GsplatUIKitSurfaceRenderer` serializes access internally, and direct C
callers should use one owner thread or queue.

The C handle adapts the shared Rust `SurfaceRenderSession`; iOS does not own a
separate frame scheduler. CPU sort cadence, compact order uploads, direct
drawing, and optional native async sorting are shared with Android, Web, and
desktop Surface rendering.

`GsplatSurfaceOptions` defaults to exact resident `.packedAtlas`, runtime
`.adaptive` ordering, and sort interval `1`. `.direct` remains the wide-float
oracle. `pollOrderMeasurement()` returns one GPU order receipt or `nil` without
blocking; `drainOrderMeasurements()` drains the current ticket-ordered queue.
Receipts distinguish timestamp-query timing from queue-completion timing and
keep unavailable GPU phase values `nil`.
Every issued CPU/GPU measurement ticket terminates in one success or
`GsplatOrderMeasurementFailure`; use `drainCpuOrderMeasurements()` and
`drainOrderMeasurementFailures()` beside the GPU success drain. Every exact
non-Paged CPU refresh reports frame-start-to-queue-completion timing so forced
CPU/GPU and Adaptive evidence share one metric. `orderSubmission()` snapshots the last successful frame's
revision, ticket, backend context, and explicit ring-busy state; strict
collectors call it after warmup, measured, and terminal-flush frames before
draining receipts; ring-busy and Surface-unavailable are distinct unsampled
reasons. `orderStatus().adaptiveGpuFailure` reports eager GPU
preparation failures even when Adaptive remains on CPU and issues no ticket.
Both Swift success types expose revision-safe `visibleCount`,
`contributorCount`, `drawnCount`, and `exactContributorCompaction`. The wrapper
takes the additive count receipt immediately after its matching terminal timing
receipt and rejects a ticket/revision mismatch. The shared invariant is
`0 <= C <= V`; exact compaction requires `D=C`, while Direct/downlevel
execution requires `D=V`.
`exactness()` returns source/decoded/encoded/resident/addressable counts,
source/resident SH degree, no-sampling/no-LOD policy bits, and the physical
adapter limits used for admission. `isFullQuality` is true only for an exact
full-resident publication; diagnostic `.pagedActiveAtlas` does not claim it.
`presentation()` returns the native pixel path and actual-present receipt. Its
`fullResolution` flag requires the last frame to be presented with requested,
Surface, internal-render, and presented dimensions equal, with dynamic
resolution and upscaling disabled.

The UIKit wrapper also exposes additive current-stats
`requestCurrentStats()`, `currentStatsSubmission()`, and `pollCurrentStats()`
calls plus `GsplatCurrentStatsConsumer`. They translate the native V1 values
without taking ownership of tickets, generations, sampling, or render policy.
Busy and unavailable admission, NotRequested, Empty, and Unsampled are
successful non-fatal values and contain no current counts. Only a Ready receipt
whose nonzero ticket and full identity match an observed Issued submission may
publish S/V/C/D; failure, expiry, drop, or mismatch terminates the matching
pending entry without a count fallback. Identity fields are opaque join values,
so zero is valid outside the ticket requirement.

The realtime iOS example uses this current-stats API as its only live S/V/C/D
source. Ordinary UI sampling is explicit and low frequency; Pending, Busy,
Unavailable, and Empty keep rendering while showing counts unavailable. The
strict benchmark requests every measured sample, binds its Issued ticket plus
complete identity to that sample, polls at most once per rendered frame, and
fails closed on any missing or failed terminal. The historical Surface
`stats()` wrapper remains only as a deprecated compatibility getter and never
requests, renders, polls, or caches current-stats values.

`GSPLAT_RENDER_MODE_SORTED_ALPHA` is the only release-gated render mode in v0.1.
Scene loading is path-based today; scene-from-memory loading is outside the
current mobile contract.

This directory provides six validation paths:

## 1) Host smoke (Swift)

Validates `GsplatKit` -> C ABI -> Rust on the host machine.

```bash
bash bindings/apple/scripts/run-swift-smoke.sh
```

## 2) Local XCFramework and Swift package wrapper

Builds the local C ABI XCFramework used by the `GsplatKit` Swift package:

```bash
bash bindings/apple/scripts/build-xcframework.sh
```

Outputs:

- Swift package wrapper: `bindings/apple/GsplatKit`
- Binary target: `bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework`
- Module name: `GsplatFFI`

The wrapper keeps raw `GsplatContext` and `GsplatSurfaceRenderer` pointers
private and exposes Swift errors, version checks, frame stats, offscreen context
rendering, and a thin UIKit Surface renderer wrapper.

The default simulator slice builds both Apple Silicon and Intel simulator
targets: `aarch64-apple-ios-sim x86_64-apple-ios`. Override
`IOS_XCFRAMEWORK_SIM_TARGETS` only when you deliberately want a narrower or
custom simulator slice, for example:

```bash
IOS_XCFRAMEWORK_SIM_TARGETS="aarch64-apple-ios-sim x86_64-apple-ios" \
  bash bindings/apple/scripts/build-xcframework.sh
```

This command is still a local packaging slice. Tagged GitHub prereleases also
attach the resulting XCFramework as a directly downloadable ZIP; they do not
provide a remote binary SwiftPM package, registry distribution, or polished
iOS product API.

## 3) iOS simulator realtime Surface app

Builds a real iOS simulator app bundle, packages the Kitsune showcase, and
presents through `UIView` -> UIKit raw window handle -> `wgpu::Surface`.

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
```

Outputs:

- App bundle: `target/ios-sim-app/GsplatIOSExample.app`
- Bundle ID: `com.gsplat.example.ios`
- Default dataset: `tests/datasets/external/wakufactory_kitune/kitune1.ply`
- Bundled runtime name: `showcase.ply`

Touch controls in the simulator app:

- one-finger drag: orbit around the loaded scene
- two-finger pinch: zoom
- two-finger drag: pan
- double tap: reset the auto camera
- `Open PLY +`: open the iOS document picker, copy the selected file into the
  app Documents directory, and restart the Surface renderer with that imported
  scene
- `Studio`: reveal or hide the full live diagnostics panel

Expected first frame includes `Kitsune shrine`, `LIVE`, a non-zero splat count,
and frame time. The Studio panel includes `state=rendering`, `camera=<mode>`,
`dataset=kitune1.ply`, and `drawn=<surface_instances>/<visible_instances>`.

If the Kitsune dataset is missing, the build falls back to Flowers. Fetch the
showcase explicitly with:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
```

This is a realtime validation app under `examples/ios/app`. It compiles
alongside the local `GsplatKit` wrapper, but remains an example rather than a
polished iOS product surface.

Dataset priority matches the Android example shape: the app uses
`Documents/imported_scene.ply` when present, then the bundled `showcase.ply`,
then a generated `Documents/minimal_ascii.ply` fallback.

For repeatable Surface performance checks, launch with benchmark args after
`--`:

```bash
bash bindings/apple/scripts/run-ios-sim-app.sh -- \
  --gsplat_benchmark true \
  --gsplat_benchmark_frames 120 \
  --gsplat_benchmark_warmup_frames 10 \
  --gsplat_benchmark_yaw_step 0.001 \
  --gsplat_surface_sort_interval 1 \
  --gsplat_surface_async_sort false \
  --gsplat_surface_frame_latency 2 \
  --gsplat_surface_order_backend adaptive \
  --gsplat_surface_projected_policy adaptive \
  --gsplat_geometry_path packed
```

Benchmark mode forces a tiny camera orbit each frame and prints a
`BENCHMARK_RESULT` line to the simulator log. Measurement samples are stored in
a preallocated numeric buffer; JSON serialization happens after measurement.
Every run uses the shared resident-scene pipeline selected by
`gsplat_geometry_path`; the remaining knobs cover CPU/GPU/adaptive ordering,
sort scheduling, and frame latency.
`gsplat_surface_projected_policy` independently selects `candidate`, `compact`,
or `adaptive` (the default). Forced policies execute without manufacturing a
measurement ticket. Adaptive runs record requested/actual execution and state
on every frame, retain independent projected submission/success/failure
ledgers, and join every success to its immediately consumed V/C/D receipt.
Formal artifact publication rejects ring-busy or Surface-unavailable probes,
dropped or failed terminals, missing counts, ticket/revision/generation
identity drift, an order/projected ticket on the same frame, `Candidate D != V`,
or `Compact D != C`.
Every measured frame additionally owns one current-stats v1 submission and
Ready terminal joined by ticket, the full scene/camera/viewport/contract/
plan-set/order/raster/encode/presentation identity, and its sample/trace key.
Fixed-camera frames may repeat a camera revision; their tickets and
presentation sequences must remain unique. CPU PostSort and GPU PostSort
require `D=V`; GPU Preproject requires `D=C`. Order and projected ledgers remain
independent evidence and never backfill a missing current receipt.
`gsplat_geometry_path` selects exact resident `packed` (default), the `direct`
wide-float oracle, or diagnostic local-source `paged` before scene derivation
and Surface resource creation. A preselected packed path streams the path-backed
PLY directly into exact resident planes instead of constructing a temporary
wide scene. The example records the resulting `renderer.path`
(`sorted_index_direct`, `packed_atlas`, or `paged_active_atlas`) in the emitted
benchmark artifact.

GPU ordering also requires the adapter's indirect-execution capability. On a
simulator adapter that omits it, `.adaptive` remains on CPU and a forced `.gpu`
request returns a structured unsupported error instead of triggering a wgpu
validation failure. Simulator runs qualify API and visual compatibility; use a
physical iOS device for performance conclusions.

For an attested simulator artifact, use a fresh output directory and an
explicit simulator, dataset, and trace:

```bash
python3 bindings/apple/scripts/collect-ios-sim-benchmark.py \
  target/ios-sim-benchmarks/<run-id> \
  --simulator-id <simulator-udid> \
  --dataset <dataset.ply> \
  --trace <camera-trace.json>
```

The collector installs and attests the built app, then gives every launch fresh
stdout and stderr files inside that app's Simulator data container. It never
reuses the blocking `simctl --console` stream, whose previous attachment can
deliver a prior run's trailing output during a consecutive launch. The two
launch-scoped streams are checked together, and collection fails unless they
contain exactly one `BENCHMARK_RESULT`; duplicate or missing terminals are not
selected, ignored, or repaired. After the terminal, the collector terminates
the app before finalizing `raw-console.log`, extracts and validates artifact v1,
and joins the manifest back to the exact commit, dataset, trace, and Simulator
runtime identities. A post-launch failure leaves the requested artifact absent
and retains a diagnostic-only bundle under
`target/ios-sim-benchmark-failures/`, including the observed terminal snapshot,
the post-termination launch streams, and their hashes and terminal counts.

## 4) iOS simulator target build

Cross-compiles the smoke binary and Rust FFI library for iOS simulator.

```bash
bash bindings/apple/scripts/build-ios-sim.sh
```

Outputs:

- Binary: `target/ios-sim-smoke`
- Rust target: `aarch64-apple-ios-sim` (on Apple Silicon hosts)

## 5) iOS simulator offscreen flower smoke

Builds the simulator smoke binary, boots or reuses an iPhone simulator, and runs
the Swift/C ABI smoke inside that simulator with the same flower dataset used by
the Android emulator smoke.

```bash
bash bindings/apple/scripts/run-ios-sim-smoke.sh
```

Defaults:

- Dataset: `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`
- Simulator: `IOS_SIMULATOR_ID` when set, otherwise the first booted iPhone
  simulator, otherwise the first available iPhone simulator

If the flower dataset is missing, fetch it first:

```bash
bash tests/datasets/fetch-nvidia-flowers-1.sh
```

Expected output includes:

```text
swift smoke ok
drawn=<drawn_count> visible=<visible_count> frame_ms=<frame_ms>
```

This is an offscreen Swift -> C ABI -> Rust render smoke spawned inside the iOS
Simulator. Use the realtime Surface app above when validating visual
presentation or touch interaction.

## 6) iOS device realtime Surface app

Builds and signs a real iPhone app bundle, packages the selected showcase dataset,
installs it with `devicectl`, and launches the same realtime UIKit Surface app
on a paired physical device.

```bash
bash bindings/apple/scripts/build-ios-device-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/run-ios-device-app.sh
```

Outputs:

- App bundle: `target/ios-device-app/GsplatIOSExample.app`
- Bundle ID: `com.gsplat.example.ios`
- Rust target: `aarch64-apple-ios`
- Rust profile: `release` by default
- Swift optimization: `-O` by default

Signing is environment-specific. By default the build script searches local
development provisioning profiles for one that matches `IOS_BUNDLE_ID` and
picks an installed `Apple Development:` signing identity. Set these explicitly
when the automatic selection is ambiguous:

- `IOS_PROVISIONING_PROFILE=/path/to/profile.mobileprovision`
- `IOS_CODE_SIGN_IDENTITY="Apple Development: ..."`
- `IOS_BUNDLE_ID=com.example.your.bundle`
- `IOS_DEVICE_ID=<coredevice-id-or-udid>`

`IOS_DEVICE_ID` is required for run and benchmark scripts. Use
`xcrun devicectl list devices` to find the CoreDevice identifier or UDID.
Set `IOS_RUST_PROFILE=dev` and `IOS_SWIFT_OPT_LEVEL=-Onone` only when debugging
symbols or native build issues; the default device path is optimized so it can
be compared with Android's default release-native APK build.

Device benchmark args use the same `--` separator as the simulator app:

```bash
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/benchmark-ios-device-app.sh -- \
  --gsplat_benchmark true \
  --gsplat_benchmark_frames 120 \
  --gsplat_benchmark_warmup_frames 10 \
  --gsplat_benchmark_yaw_step 0.001
```

The benchmark script builds/signs the device app, installs it, launches with
`devicectl --console`, prints the `BENCHMARK_RESULT` line, and stores the raw
log under `target/ios-device-benchmarks/`. It also extracts an atomically
published `gsplat-benchmark/v1` directory containing `manifest.json`,
`frames.jsonl`, and `summary.json`, then runs the shared artifact validator.
Set `IOS_BENCHMARK_ARTIFACT_DIR` to choose a fresh destination; existing
destinations are rejected. The manifest records the thermal state before and
after measurement. Build commit/dirty identity, browser, driver, GPU completion
timing, and sort-refresh visibility are unavailable on this collector and are
therefore emitted as `null` with explicit `unavailable_fields` entries.
The extractor runs the shared benchmark-v1 validator plus the Apple projected
and current-stats validators before atomically publishing the destination.
`call_ms` remains the Swift render host-call wall and `frame_wall_ms` remains
the adjacent host-frame wall. CPU preprocess/sort and CPU/GPU completion values
are emitted only when the same sample owns the matching order terminal;
unsupported legacy geometry/raster timing is `null` and explicitly listed in
`unavailable_fields`.
