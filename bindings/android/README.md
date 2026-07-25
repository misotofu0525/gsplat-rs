# gsplat-android

Android binding, JNI bridge, and local packaging scripts.

## Integration boundary

This directory contains the local Android library module, JNI bridge, host smoke
path, and packaging scripts over the public C ABI in
`crates/gsplat-ffi-c/include/gsplat.h`. The runnable Android sample app lives
under `examples/android/app`.
`bindings/android/gsplat-android` can build an AAR for local consumption. It is
not published to Maven and is not a full Android product SDK yet.

The stable v0.1 render path is `GSPLAT_RENDER_MODE_SORTED_ALPHA`. Keep errors as
integer `GsplatErrorCode` values at the native boundary and convert them to
readable text with `gsplat_error_message()` / `NativeBridge.errorMessage()`.
Wrappers should prefer `gsplat_last_error_message()` /
`NativeBridge.lastErrorMessage()` for operation-specific details.

This directory provides three validation and packaging paths:

## 1) Android AAR build (arm64-v8a)

Builds the Rust static library, JNI shared library, and Android library module:

```bash
bash bindings/android/scripts/build-aar.sh
```

Output:

- AAR: `bindings/android/gsplat-android/build/outputs/aar/gsplat-android-release.aar`

The library module namespace is `com.gsplat.android`. It packages the generated
`libgsplat_jni.so` and exposes:

- `NativeBridge`: low-level JNI calls matching the C ABI
- `GsplatAndroidVersion`: runtime ABI compatibility guard
- `GsplatSurfaceRenderer`: typed Kotlin handle wrapper
- `GsplatSurfaceOptions`: exact packed residency plus adaptive ordering by
  default, with independent Candidate/Compact/Adaptive projected-draw policy,
  explicit CPU/GPU/adaptive ordering, cadence, async-sort, and frame-latency
  controls
- `GsplatSurfaceProjectedSubmission`: last successful frame's requested policy,
  actual execution, order backend, Adaptive state, and honest ticket/unsampled
  identity
- `GsplatSurfaceProjectedMeasurement`: terminal projected-draw completion joined
  by ticket with exact visible/contributor/drawn (`V/C/D`) counts
- `GsplatSurfaceProjectedMeasurementFailure`: terminal readback, invalidation,
  or invariant failure for an issued projected-draw ticket
- `GsplatSurfaceOrderMeasurement`: non-blocking GPU timestamp/completion plus
  revision-safe candidate/contributor/issued (`V/C/D`) counts
- `GsplatSurfaceCpuOrderMeasurement`: CPU preprocess/sort plus comparable
  frame-start-to-graphics-queue-completion timing and the same `V/C/D` counts
- `GsplatSurfaceOrderMeasurementFailure`: terminal readback/invalidation
  failures for issued CPU/GPU tickets
- `GsplatSurfaceOrderSubmission`: last successful frame's revision, issued
  ticket, backend context, and explicit ring-busy state
- `GsplatSurfaceOrderStatus`: last-frame backend/revision state plus a typed
  Adaptive GPU-unavailable reason
- `GsplatSurfaceExactness`: source/decoded/encoded/resident/addressable counts,
  SH preservation, no-sampling/no-LOD policy bits, and adapter admission limits
- `GsplatSurfacePresentation`: requested, Surface, internal-render, and actual
  presented pixels, with fail-closed full-resolution/presentation flags
- `GsplatSurfaceStats`: typed frame stats
- `GsplatSurfaceCurrentStats*`: immutable current-stats v1 request,
  submission, full-identity receipt/failure, and consumer state values
- `GsplatException`: readable native error wrapper

Local Gradle consumers can depend on the module directly from this repository,
or on the generated AAR through a local `flatDir` repository. In either case,
build the AAR from this repository first:

```bash
bash bindings/android/scripts/build-aar.sh
```

Minimal wrapper-first usage from an Android `Surface`:

```kotlin
import com.gsplat.android.GsplatSurfaceRenderer

val renderer = GsplatSurfaceRenderer.create(
    surface = surface,
    datasetPath = sceneFile.absolutePath,
    width = width,
    height = height
)

val currentStatsCycle = renderer.renderFrameWithCurrentStats()
val currentStatsState = currentStatsCycle.state
val exactness = renderer.exactness()
val presentation = renderer.presentation()
val orderStatus = renderer.orderStatus()
val orderSubmission = renderer.orderSubmission()
val projectedSubmission = renderer.projectedSubmission()
val projectedReceipts = renderer.drainProjectedMeasurements()
val projectedFailures = renderer.drainProjectedMeasurementFailures()
val cpuOrderReceipts = renderer.drainCpuOrderMeasurements()
val orderReceipts = renderer.drainOrderMeasurements()
val orderFailures = renderer.drainOrderMeasurementFailures()
renderer.close()
```

`renderFrame()` remains the ordinary render path and never requests observer
readback. Call the additive `renderFrameWithCurrentStats()` only at an explicit
sampling point. It performs request -> render -> submission -> one non-blocking
poll; if that poll is pending while ordinary rendering continues, call
`pollCurrentStats()` to observe the already issued ticket without requesting
another sample. A Ready snapshot
remains current only until the next successful ordinary presentation; that
presentation clears it instead of exposing prior-frame counts. If the requested
render fails, or a successful frame cannot issue the ticket yet, the native
request intent remains explicit as `AwaitingSubmission`. The next successful
render reconciles that retained intent by reading one submission and one global
poll even when the caller has returned to ordinary `renderFrame()`. This
recovery never sends another request, does not change the ordinary frame's
render-success result, and stops as soon as the intent becomes Issued or
Unsampled. With no retained explicit intent, ordinary renders perform no
current-stats JNI calls. `currentStats()` only returns the last consumer state
and does not poll.

Only a `GsplatSurfaceCurrentStatsState.Ready` whose non-zero ticket and complete
scene/camera/viewport/contract/plan-set/executed-plan/order/raster/encode/
presentation identity match a previously observed `Issued` submission exposes
current `S/V/C/D` counts. Busy, Pending, NotRequested, GPU/Resource unavailable,
and TicketExhausted are non-fatal states with no fallback counts. Terminal map,
generation, expiry, or drop failures clear their matching pending ticket and
publish no counts. An `Issued` submission remains valid when the same cycle's
request reports Busy because a request intent can transfer across a failed
frame. Renderer admission bounds issued-but-unresolved tickets; the Android
consumer adds only a fixed-size terminal/rejection tombstone window. Rereading
the same Issued snapshot after Ready or Failure is idempotent, while the same
ticket with a different complete identity is rejected. A submission mismatch
never skips a terminal already removed by the native global single-pop; that
terminal is still accounted and the affected pending ticket ends fail-closed.
`GsplatSurfaceCurrentStatsAdapter.pollResult()` preserves that destructive raw
poll beside its presentation-safe state projection. Strict ledgers consume the
raw ticket and identity even when an older Ready is normalized to Pending for a
newer presentation; ordinary `pollCurrentStats()` remains state-only and cannot
redisplay stale counts.

The sample app uses the same additive adapter over its lower-level
`NativeBridge` render loop. Ordinary UI explicitly samples at low frequency and
shows unavailable for every non-Ready state. Strict benchmark frames request
only after their camera/resize command succeeds, then bind Issued submission and
terminal evidence to that measured frame by ticket plus the complete identity.
If a terminal arrives only after a later ordinary presentation, the ledger
retains it for strict accounting but the live UI stays unavailable instead of
displaying that prior frame's counts.
The sample holds one `renderLock` transaction across command, request,
render/present, submission read, and exactly one non-blocking poll, so a
`surfaceChanged` resize cannot split that sequence. A UI request exception is
non-fatal and the ordinary render still runs; a strict request exception rejects
the sample before rendering. If a failed render retains a pre-ticket intent and
its same-frame retry command then fails, the sample explicitly rejects that
intent and closes the native renderer before any different frame can render.
Multiple issued tickets may remain pending and terminate out of order; only one
not-yet-submitted pre-ticket request intent may exist at a time. The bounded
terminal flush stops rendering, uses `pumpSurfaceReceiptsV1()` to wait only for
already-submitted queue work, and never borrows a later frame's receipt. Its
per-pump wait and total retry count are finite; callback-pump failure or flush
exhaustion rejects the run.

For a strict Exact frame whose order was refreshed, the already-issued
current-stats ticket is also the compatibility order ticket. Its one atomic
terminal supplies the same V/C/D plus queue-completion timing; CPU plans retain
their same-frame preprocess/sort timing, while GPU phase timings remain absent
under CompletionOnly. This projection issues no second ticket and adds no
render or submit. `pumpSurfaceReceiptsV1()` reports QueueComplete versus Timeout
so device logs can separate pump failure/timeout from a queue-complete but
unconsumed terminal or mismatched ledger.

The product defaults are `PACKED_ATLAS`, `ADAPTIVE`, and sort interval `1`.
Projected execution independently defaults to
`GsplatSurfaceProjectedPolicy.ADAPTIVE`. `setProjectedPolicy()` can force the
exact Candidate path (`D=V`) or exact Compact path (`D=C`) without changing the
CPU/GPU ordering backend. A rejected Compact request throws `GsplatException`
and leaves the live native policy unchanged.
`DIRECT` remains the wide-float oracle. A diagnostic `PAGED_ACTIVE_ATLAS`
configuration must explicitly select the CPU backend; it is not a full-quality
resident fallback. `pollOrderMeasurement()` returns one receipt or `null`
without blocking, while `drainOrderMeasurements()` drains the current native
queue in ticket order. Optional GPU phase fields are populated only when the
device exposes valid timestamp queries; `GPU_COMPLETION` receipts keep those
fields null instead of presenting submit-wall time as GPU work.
`exactness().isFullQuality` is true only when all source splats were decoded,
encoded, resident, and GPU-addressable at the source SH degree with sampling
and LOD disabled. The diagnostic Paged path intentionally does not set it.
Every explicitly sampled exact non-Paged refresh publishes its current-stats
ticket as the same frame's order measurement ticket. CPU tickets report
frame-start-to-queue-completion timing, not submit wall time. Every issued
CPU/GPU ticket must appear in exactly one success or failure queue.
`orderStatus().adaptiveGpuFailure` also exposes an eager GPU
preparation failure when Adaptive correctly stays on CPU and no ticket exists.
Strict benchmark collectors call `orderSubmission()` after every successful
warmup/measured render, then stop issuing frames and boundedly pump callbacks
until every issued ticket has exactly one terminal receipt. Ring-busy,
Surface-unavailable, dropped-prior, failure, and fallback evidence invalidates
the run.
Both typed success receipts expose `visibleCount`, `contributorCount`,
`drawnCount`, and `exactContributorCompaction`. JNI takes the additive count
receipt immediately after the matching terminal timing receipt and verifies
its ticket and camera revision. Consumers enforce `0 <= C <= V`; exact
compaction requires `D=C`, and all other execution requires `D=V`.

The projected `_v1` lane is a separate receipt contract. JNI initializes every
versioned output with its exact native `struct_size` and `version=1` before the
C call. When a projected success is available, JNI immediately takes its V/C/D
receipt by the same non-zero ticket and verifies ticket, camera revision,
execution, and count invariants before publishing one Kotlin object. Missing or
expired counts fail closed; no timing-only success escapes to Kotlin. Forced
Candidate/Compact controls execution but does not request measurement tickets,
so `projectedSubmission().ticket` remains null and is never synthesized.
Adaptive probe tickets must terminate in exactly one projected success or
failure; dropped-prior, ring-busy, Surface-unavailable, or invariant evidence
invalidates strict retained runs.
`GsplatSurfaceGpuProducerDiagnostics` remains an additive compatibility/schema
surface, but M2a rejects enabling its old independent measurement path. It is
not a current Android qualification seam. Product defaults leave this telemetry
disabled; M2b must provide a real-window producer measurement seam before these
DTOs can support retained producer evidence.
`presentation().fullResolution` additionally requires the last frame to have
actually reached `present()`, with requested, Surface, internal-render, and
presented dimensions equal and no dynamic resolution or upscaling.

`GsplatSurfaceRenderer` serializes access to the native handle internally. If
you call `NativeBridge` directly, keep each native Surface renderer handle owned
by one serialized thread or queue and destroy it only after in-flight work has
returned.

The C handle is an adapter over the shared Rust `SurfaceRenderSession`, not a
separate Android scheduler. CPU sort cadence, compact order uploads, direct
drawing, and optional native async sorting therefore follow the same state
machine as Web, iOS, and desktop Surface rendering.

## 2) Host smoke (JNI)

Validates Kotlin/JNI -> C ABI -> Rust on the host machine.

```bash
bash bindings/android/scripts/run-jni-smoke.sh
```

Host-smoke Kotlin sources live under `bindings/android/host-smoke/`.

## 3) Android sample APK build (arm64-v8a)

Builds a real Android app container that depends on the local
`:gsplat-android` library module.
The app UI is Kotlin-only and renders through `SurfaceView` -> JNI -> `ANativeWindow` -> `wgpu::Surface`.
It does not use the old Android bitmap/readback preview path.
The native Rust library is built with the Rust `release` profile by default so
Surface performance smoke runs exercise optimized renderer code. Set
`ANDROID_RUST_PROFILE=dev` only when debugging native symbols or build issues.

Surface creation returns both a native handle and an error code:

```kotlin
val createError = IntArray(1)
val handle = NativeBridge.createSurfaceRenderer(
    surface,
    datasetPath,
    width,
    height,
    createError
)
if (handle == 0L) {
    error("gsplat create failed: ${NativeBridge.errorMessage(createError[0])}")
}
```

Touch controls in the example:

- one-finger drag: orbit around the loaded scene
- two-finger pinch: zoom
- two-finger drag: pan
- double tap: reset the auto camera
- `Open PLY +`: open the Android system file picker, copy the selected file into app internal storage, and restart the Surface renderer with that imported scene
- `Studio`: reveal or hide the full live diagnostics panel

Prereqs:

- Android SDK installed. The scripts read `ANDROID_SDK_ROOT`, then
  `ANDROID_HOME`, then fall back to `~/Library/Android/sdk`.
- Android NDK installed (default version used: `29.0.14206865`)
- Android native API level defaults to `24`; override with
  `ANDROID_API_LEVEL=<level>` when testing another compatible API level.
- The repo-local scripts use a checksum-verified Gradle distribution helper
  instead of assuming a checked-in wrapper.

Build steps:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash bindings/android/scripts/build-sample-apk.sh
```

The script packages `tests/datasets/external/wakufactory_kitune/kitune1.ply`
as `assets/showcase.ply` when available, falls back to the shared Flowers
fixture, and accepts an explicit PLY path as its first argument.

Outputs:

- APK: `examples/android/app/build/outputs/apk/debug/sample-app-debug.apk`
- JNI lib: `bindings/android/gsplat-android/src/main/jniLibs/arm64-v8a/libgsplat_jni.so`

Notes:

- This example uses `files/imported_scene.ply` when present, then extracts the bundled `assets/showcase.ply` into app storage, then checks `files/flowers_1.ply`; otherwise it writes a minimal ASCII PLY into app internal storage.
- Imported files come from the Android system picker as `content://` URIs and are copied into `files/imported_scene.ply` before crossing the JNI/C ABI boundary, which still receives a normal local file path.
- On Android emulator, the `SurfaceView` buffer is capped to a 1600px maximum side. The Surface presenter does not sample or cap the sorted splat list; visual stability is preferred over artificial throughput wins.
- The compact overlay reports the live splat count and frame time. Direct and
  packed paths retain `drawn=<surface_instances>/<visible_instances>`. The
  experimental paged path reports `drawn=<active_resident>/<loaded_source>` so
  a bounded working set cannot be mistaken for full installation; its compact
  overlay shows the same ratio. The `Studio` panel retains the full Android
  Surface diagnostics.
- `GsplatSurfaceOptions.geometryPath` selects exact resident `PACKED_ATLAS` by
  default, or the `DIRECT` oracle / local-source diagnostic
  `PAGED_ACTIVE_ATLAS` before scene
  derivation and Surface resource creation. A preselected `PACKED_ATLAS` path
  streams the local PLY directly into exact resident planes instead of first
  constructing wide scene buffers.
- Maven publishing, additional ABIs, and a higher-level `GsplatSurfaceView`
  are intentionally not solved here yet. Future Android SDK work should keep
  wrapping the same C ABI rather than introduce a separate render contract.

## 4) Emulator flower smoke

After building the APK, push the shared flower dataset into app storage and launch:

```bash
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_SDK_ROOT/platform-tools/adb"

"$ADB" install -r examples/android/app/build/outputs/apk/debug/sample-app-debug.apk
"$ADB" shell am start -n com.gsplat.example/.MainActivity
```

For repeatable Surface performance checks, launch with benchmark extras:

```bash
"$ADB" logcat -c
"$ADB" shell am force-stop com.gsplat.example
"$ADB" shell am start -n com.gsplat.example/.MainActivity \
  --ez gsplat_benchmark true \
  --ei gsplat_benchmark_frames 120 \
  --ei gsplat_benchmark_warmup_frames 10 \
  --ef gsplat_benchmark_yaw_step 0.001 \
  --ei gsplat_surface_sort_interval 1 \
  --ez gsplat_surface_async_sort false \
  --ei gsplat_surface_frame_latency 2 \
  --es gsplat_surface_order_backend adaptive \
  --es gsplat_geometry_path packed
"$ADB" logcat -d -s GsplatExample:I | grep BENCHMARK_RESULT
```

For retained CPU/GPU/adaptive comparisons, use the repository collector instead
of assembling ad-hoc `adb` commands. The example below runs five randomized
pairs with a fixed seed and waits for Android thermal status `0` before each
run:

```bash
python3 bindings/android/scripts/collect-android-sort-benchmarks.py \
  --serial <adb-serial> \
  --ply tests/datasets/external/wakufactory_kitune/kitune1.ply \
  --prepare-apk \
  --backend cpu \
  --backend gpu \
  --repetitions 5 \
  --randomize-order \
  --seed 20260722 \
  --sort-interval 1 \
  --geometry-path packed \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2412x1080-v1.json \
  --camera-frame-indices 0,1 \
  --frames 80 \
  --warmup 20 \
  --cooldown-seconds 10 \
  --max-thermal-status 0 \
  --output target/android-sort-benchmarks/kitsune-paired-v1
```

Add `--backend adaptive` to include the runtime selector in every repetition.
Use `--camera-frame 0` or `--camera-frame 1` instead of
`--camera-frame-indices` for a static-view run. Static and sequence playback
produce the same post-present native camera receipt and pass through the same
external-trace validator.
`--gpu-producer post_sort|preproject` is explicitly Deferred and rejected
before device collection. M2a rejects the old independent producer measurement
path, so synthetic validator fixtures are schema tests rather than Android
runtime qualification. M2b must provide the real-window seam before this option
can collect retained evidence. The preserved future validator contract is
canonical: PostSort uses Candidate with `D=V` (including `C<V`), while
Preproject uses Compact with `D=C<=V`; measured-frame tickets and terminal
ledger tickets must be unique and exactly equal.

The collector defaults to `--geometry-path packed`, which selects the complete
Resident scene and its production `ProjectedQuadsExact` Surface raster plan.
`--geometry-path direct` is retained only for an explicit wide-f32 oracle run;
the collector records and strictly validates the selected renderer path.
The collector defaults to 80 measured frames so the final indexed JSONL burst
stays below conservative Android `logd` per-tag quotas. Larger values are
allowed, but the artifact validator rejects the run if `logd` drops even one
frame record; split longer policy observations into repetitions when needed.
Use `--prepare-apk` only for the first experiment after native/app code changes;
it builds and installs the debuggable sample APK once with only the tiny
`tests/datasets/minimal_ascii.ply` bootstrap asset. The measured `--ply` is
always injected separately and therefore never forces an APK rebuild. For
every additional point-count tier, omit `--prepare-apk`: the collector resolves
the existing local debug APK and requires the device's installed `base.apk`
SHA-256 and byte count to match it exactly. A mismatch fails closed instead of
silently installing, uninstalling, or benchmarking a stale binary. `--apk
<path>` can select an explicit prebuilt APK for this comparison.

Use `--dry-run` to inspect the complete schedule and launch arguments without
building, installing, pushing a dataset, clearing app data, or creating the
output directory.
Run `--help` for frame latency, yaw, timeout, thermal polling, and explicit
`adb` options.

Add `--formal-artifact` only for the 2412x1080 Packed device protocol. Formal
mode requires the local release AAR as well as the exact APK, and records the
APK, AAR, APK-member native `.so`, and AAR-member native `.so` identities
before device collection. The two native member hashes must agree. After each
package clear it proves that `files/benchmark-final-frame.png` is absent, asks
the benchmark Activity to publish that exact app-sandbox path, waits for the
unique benchmark result plus complete SHA-verified manifest and summary for the
same run ID, and then pulls the file with `adb exec-out run-as`. It never
accepts a caller-selected host image and never uses `screencap`, host display
capture, or Android screenshot APIs.

The extractor admits the image into the same staging transaction as
`manifest.json`, `frames.jsonl`, and `summary.json` only when the pull receipt
matches the benchmark run ID, fixed package/path, byte count, SHA-256, PNG
signature/IHDR, and exact 2412x1080 dimensions. The receipt retains distinct
device and local byte/SHA identities and both must match the staged bytes. The
generic artifact validator,
strict current-stats validator, and camera validator all run before the four
files are atomically published. After every scheduled run is complete, the
collector writes a staged `gsplat-full-quality-experiment/v1` suite, validates
it with `--verify-inputs`, and only then renames it to `suite.json`. Missing
image output, an old file surviving package clear, malformed/wrong-size bytes,
hash drift, incomplete terminal evidence, non-full resolution, or absent
APK/AAR/native/dataset/trace identity leaves no formal suite.

The collector pushes each experiment's PLY exactly once to
`/data/local/tmp/gsplat-benchmark-<sha256>.ply`. Before every paired run it
clears only `com.gsplat.example`, copies that staged file with `run-as` to the
app-priority path `files/imported_scene.ply`, and verifies the internal file's
SHA-256 and byte count before launch. The exact temporary file is removed on
success and on collection failures; no broad temporary-directory cleanup is
performed. This avoids package-manager replacement broadcasts between tiers
and prevents a bundled or previously imported scene from silently replacing
the intended fixture. Each run retains the complete tagged logcat stream, a
validated `gsplat-benchmark/v1` artifact, and `run.json` under a fresh run
directory. The experiment root also contains the seeded schedule, dataset/APK
hashes, device identity, thermal observations, and progress in
`experiment.json`. Existing output roots and artifact directories are never
overwritten.

The standalone `extract-android-benchmark-artifacts.py` path runs the generic
v1 validator and, whenever `renderer.current_stats_strict=true`, the same
current-stats artifact validator used by the full collector. Missing or
non-Ready ledger entries, incomplete identity, sample/trace join drift, or an
`exactness_receipt_id` different from `manifest.exactness.receipt_id` fails
closed before the destination is published.
Its optional `--final-png` lane is accepted only together with the collector's
`--device-png-pull-receipt`; it is not a general image-import option.

Benchmark mode forces a tiny camera orbit each frame so it measures the selected
ordering backend and exact resident draw path rather than stationary
presentation.
For a cross-platform fixed-camera run, generate or select a validated trace
whose `display` exactly matches the Activity's real `SurfaceView` pixels, copy
it into the app sandbox, and pass its path and frame index. Fixed-camera mode
skips the synthetic orbit and fails closed on a display mismatch:

```bash
TRACE_PATH=/absolute/path/to/trace-matching-the-surface.json
adb push "$TRACE_PATH" /data/local/tmp/camera-trace-v1.json
adb shell run-as com.gsplat.example cp /data/local/tmp/camera-trace-v1.json files/camera_trace.json
adb shell am start -n com.gsplat.example/.MainActivity \
  --ez gsplat_benchmark true \
  --es gsplat_camera_trace_path /data/user/0/com.gsplat.example/files/camera_trace.json \
  --ei gsplat_camera_trace_frame 0
```

The emitted manifest records the trace ID, declared content hash, exact trace
file SHA-256, selected frame/schedule, canonical coordinate and matrix
conventions, and the f32 comparison tolerance. Every measured frame carries a
native post-present camera receipt: actual pose/intrinsics, row-major
view/projection/view-projection matrices, Surface dimensions, and equal
current/presented camera revisions. The formal collector compares all of those
fields against the separately supplied expected trace; a missing field, wrong
index, stale revision, matrix mutation, or trace-file mismatch rejects the
run. The matrices come from the live Surface session through
`GsplatSurfaceCameraReceiptV1`, not from copying trace JSON into the artifact.
The manifest also records `require_display_match=true`,
`display_policy=trace_display_exact`, and `quality_comparable=true`. Trace
scheduling remains a sample-only qualification route; order
backend selection, non-blocking order receipts, and exactness receipts are
public Android AAR and C ABI APIs.

To exercise trace loading on an arbitrary device with a differently sized
fixture, explicitly add `--ez gsplat_require_trace_display_match false`. This
is smoke-only: the trace pose and vertical FOV are reprojected at the native
aspect, and the emitted `native_aspect_reprojection` artifact is rejected by
the full-quality experiment collector.

For the moving-camera sort protocol, add:

```bash
adb shell am start -n com.gsplat.example/.MainActivity \
  --ez gsplat_benchmark true \
  --es gsplat_camera_trace_path /data/user/0/com.gsplat.example/files/camera_trace.json \
  --ez gsplat_camera_trace_sequence true \
  --es gsplat_camera_frame_indices 0,1,2 \
  --ei gsplat_surface_sort_interval 1 \
  --es gsplat_surface_order_backend cpu
```

With no frame list or benchmark counts, sequence mode applies every revision
once, with zero warmup and one loop. Existing
`gsplat_benchmark_warmup_frames`/`gsplat_benchmark_frames` set warmup and
measured revisions; `gsplat_camera_trace_loops` repeats the measured schedule.
Every applied revision logs its trace ID/hash, frame index, timestamp, phase,
loop, and requested backend. Fixed-frame mode remains the screenshot path.
`gsplat_surface_sort_interval` controls how often the Surface path refreshes
depth sorting during camera changes. The Android library and example default
to `1`, so every changed-camera frame requests a current order. CPU refreshes
apply the same visibility contract, sort on CPU, and upload compact source IDs.
GPU refreshes generate and stably sort all `(depth_key, source_id)` pairs on the
renderer device. Adaptive runs bounded repeated CPU/GPU probes and retains
hysteresis instead of hard-coding a point-count crossover. All choices feed the
same exact resident draw path; the public receipt collector exposes which
backend actually ran and whether its GPU timing came from timestamp queries or
queue completion. GPU benchmark frames are joined to asynchronous receipts by
camera revision and ticket; a missing, duplicate, or dropped receipt rejects
the artifact instead of converting pending counts into zero. Any terminal
readback/invalidation failure also rejects the artifact with its ticket,
revision, and reason.
`gsplat_surface_async_sort=true` enables an experimental background sort worker
that double-buffers the latest completed order while the render thread continues
with the previous order. It keeps the full splat count and is intended for
interaction A/B checks.
`gsplat_surface_frame_latency` maps to wgpu
`desired_maximum_frame_latency`. The default is `2`.
`gsplat_geometry_path` selects exact resident `packed` (default), the `direct`
wide-float oracle, or diagnostic `paged` local-source active atlas for
on-device checks.
The example passes the value to the additive constructor-time geometry entry
and records the resulting `renderer.path` (`sorted_index_direct`,
`packed_atlas`, or `paged_active_atlas`) in the emitted benchmark artifact.
