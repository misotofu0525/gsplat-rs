# Android Example

Kotlin Android Surface sample app for the local `gsplat-android` binding.

The app opens as the native Kitsune showcase when the CC0 dataset has been
fetched. Its editorial overlay keeps scene/frame telemetry visible while the
full ABI, Surface, camera, dataset, and path diagnostics stay available from
the `Studio` button.

Build the sample APK from the repository root:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash bindings/android/scripts/build-sample-apk.sh
```

The build script prefers Kitsune, falls back to the shared NVIDIA Flowers
fixture, and accepts an explicit PLY path as its first argument. `Open PLY +`
still imports a local scene at runtime.

Formal benchmarks on the Nothing A065 request sensor-landscape orientation and
use its native `2412x1080` `SurfaceView` buffer. A formal camera trace must
declare exactly those drawable dimensions; it never calls
`setFixedSize` to force the device into a smaller reference size. Artifact
emission fails unless the native receipt proves
`requested = Surface = internal render = presented = 2412x1080`, the terminal
frame was actually presented,
`source = decoded = encoded = resident = addressable` membership holds, and
the complete source SH degree is retained. Sampling, LOD, dynamic
resolution, and upscaling are forbidden; Paged cannot qualify. Other devices
and 640x360/640x480 traces remain smoke/exploratory evidence until the formal
matrix is deliberately revised. Physical panel dimensions are recorded only as
environment metadata. Explicit `gsplat_require_trace_display_match=false` is
available only for smoke testing and marks the artifact as native-aspect
reprojection, which is not quality-comparable evidence.

The formal host collector also requests one app-private
`files/benchmark-final-frame.png`. The Activity accepts only that fixed path,
deletes any stale published or temporary file before renderer startup, and
requires the exact 2412x1080 trace plus full-resident Direct/Packed geometry.
After all benchmark, terminal current-stats, order, exactness, and presentation
checks succeed, Android `PixelCopy` copies the renderer's last presented
raw `Surface` buffer at its native drawable size. The app fsyncs a same-directory
temporary PNG and atomically renames it before emitting `BENCHMARK_RESULT` and
the manifest/frame/summary records. It does not use system screencap, host
display capture, UI-composited screenshots, a prior file, or a synthesized
image. A copy, PNG encode, or publication failure emits neither the completion
records nor a final PNG.

Adaptive compares CPU and GPU with the same `FrameCompletion` interval from
frame start through queue completion, including sorting, projection,
rasterization, submission, and queued work. Order-stage timestamps are
diagnostic only, and changing the raster plan resets the learned policy.

The live overlay explicitly requests current-stats only at its low-frequency
status sampling points. Pending, Busy, unavailable, and empty states keep
rendering and show counts as unavailable; they never reuse an older receipt or
display zero, capacity, or a sentinel as a count. A newer ordinary presentation
also invalidates a previously displayed Ready snapshot. A delayed matching
terminal is retained for strict ledger accounting but cannot repopulate the UI
with counts from the prior presentation.

Every measured benchmark frame owns a separate current-stats pre-ticket intent.
After the frame's fallible camera/resize command succeeds, the app requests the
sample, renders/presents, records its Issued ticket and full native identity,
then performs one non-blocking poll. Issued tickets may overlap and terminate
out of order. After measurement, a bounded no-render terminal flush waits only
for callbacks from already-submitted queue work and polls those existing
tickets; it cannot issue a new benchmark ticket. A later frame's receipt can
never satisfy an earlier sample. Missing Ready,
failure, expiry, generation drift, identity drift, or flush exhaustion rejects
the artifact.

When that Exact frame refreshed order, its issued current-stats ticket is also
the required order ticket; the terminal is projected into both strict ledgers
without another render, submit, or ticket. Terminal-drain log lines include
QueueComplete versus Timeout, the render-thread id/name, and issued/terminal/
pending counts for both ledgers. QueueComplete with pending terminals is
reported separately from repeated pump timeout and native pump error.

Command, request, render/present, submission read, and that one poll share the
same render-lock transaction; `surfaceChanged` cannot resize between them. UI
request failures still render and show unavailable, while strict request
failures stop before render. If a failed render leaves an intent and its
same-frame retry command fails, the app rejects the sample and closes that
renderer before another trace/frame can consume the old intent.

Only a matching Ready receipt supplies revision-safe `S/V/C/D`: complete
source/residency `S`, near/far candidates `V`, strict conservative
post-projection contributors `C`, and issued draw count `D`. The artifact
requires `0 <= C <= V <= S`, `D=V` for CPU/GPU PostSort, and `D=C` for GPU
Preproject. Fixed-camera samples still require unique ticket and presentation
sequence identities. Legacy `GsplatSurfaceStats` values are not live evidence.
Host transaction/iteration wall time remains available; preprocess, sort, and
queue-completion timing is emitted only from the same sample's order terminal,
while raster timing is explicitly unavailable.

Android GPU-producer collection is Deferred until M2b provides a real-window
measurement seam. The collector rejects `--gpu-producer` before device work;
its synthetic fixtures validate only the future canonical schema (PostSort =
Candidate with `D=V`, Preproject = Compact with `D=C<=V`, and exact unique
frame/terminal ticket-set equality), not a current device qualification.

Formal trace evidence is also post-present and revision-safe. Each measured
frame records the native session's actual f32 pose/intrinsics plus canonical
row-major view, projection, and `projection * view` matrices. The collector
checks those values, the Surface size, frame index/timestamp, and current versus
presented camera revision against the separately injected trace (including its
exact file SHA-256). Fixed view 0/view 1 and multi-view sequence schedules use
the same receipt contract; source trace matrices are never echoed as a runtime
receipt.

The Android library module, JNI bridge, host smoke, and AAR build live under
`bindings/android/`. See `bindings/android/README.md` for packaging and device
smoke details. The standalone logcat extractor also runs the collector's strict
current-stats ledger validator when the manifest declares strict evidence;
minimal fixtures and short device runs remain smoke/capacity evidence only.

Renderer creation and shutdown use generation-bound Surface requests plus a
retiring owner slot. A resize observed during native creation invalidates the
older request before handle publication; that candidate is destroyed by its
owner without rendering, and the newest valid dimensions are created after the
slot is released. Lifecycle
callbacks wait up to one second, then return with that slot still occupied if
the owner thread remains inside JNI/driver work. A later Surface is remembered
but cannot create a second native session; after the old owner alone destroys
its handle and releases the slot, the app starts the latest still-valid
Surface. There is intentionally no concurrent destroy of an in-flight native
handle. If the native call never returns, this process remains fail-closed and
will not start another renderer session; safe recovery beyond that boundary
requires a future native cancellation contract or process restart.
