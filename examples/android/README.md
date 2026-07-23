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

Adaptive compares CPU and GPU with the same `FrameCompletion` interval from
frame start through queue completion, including sorting, projection,
rasterization, submission, and queued work. Order-stage timestamps are
diagnostic only, and changing the raster plan resets the learned policy.

Each successful CPU/GPU ticket also carries revision-safe `S/V/C/D` evidence:
complete source/residency `S`, near/far candidates `V`, strict conservative
post-projection contributors `C`, and issued draw count `D`. The artifact joins
the terminal timing and count receipts by both ticket and camera revision,
requires `0 <= C <= V <= S`, and accepts `D=C` only when exact contributor
compaction is explicitly flagged. Direct/downlevel execution remains `D=V`;
stale `GsplatSurfaceStats` values are never used to manufacture a terminal
count.

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
smoke details.
