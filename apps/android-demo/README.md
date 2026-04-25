# android-demo

Android integration demo.

## Integration boundary

This is a thin Kotlin/JNI demo over the public C ABI in
`crates/gsplat-ffi-c/include/gsplat.h`. It is useful as the Android starter
path, but it is not a published AAR or a full Android SDK wrapper yet.

The stable v0.1 render path is `GSPLAT_RENDER_MODE_SORTED_ALPHA`. Keep errors as
integer `GsplatErrorCode` values at the native boundary and convert them to
readable text with `gsplat_error_message()` or `NativeBridge.errorMessage()`.

This demo provides two validation paths:

## 1) Host smoke (JNI)

Validates Kotlin/JNI -> C ABI -> Rust on the host machine.

```bash
bash apps/android-demo/run-jni-smoke.sh
```

Host-smoke Kotlin sources live under `apps/android-demo/host-smoke/`.

## 2) Android APK build (arm64-v8a)

Builds a real Android app container and packages `libgsplat_jni.so`.
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

Touch controls in the demo:

- one-finger drag: orbit around the loaded scene
- two-finger pinch: zoom
- two-finger drag: pan
- double tap: reset the auto camera
- `Import PLY`: open the Android system file picker, copy the selected file into app internal storage, and restart the Surface renderer with that imported scene

Prereqs:

- Android SDK installed (default path: `~/Library/Android/sdk`)
- Android NDK installed (default version used: `29.0.14206865`)

Build steps:

```bash
bash apps/android-demo/build-apk.sh
```

Outputs:

- APK: `apps/android-demo/app/build/outputs/apk/debug/app-debug.apk`
- JNI lib: `apps/android-demo/app/src/main/jniLibs/arm64-v8a/libgsplat_jni.so`

Notes:

- This demo uses `files/imported_scene.ply` when present, then `files/flowers_1.ply` when present; otherwise it writes a minimal ASCII PLY into app internal storage.
- Imported files come from the Android system picker as `content://` URIs and are copied into `files/imported_scene.ply` before crossing the JNI/C ABI boundary, which still receives a normal local file path.
- On Android emulator, the `SurfaceView` buffer is capped to a 1600px maximum side. The Surface presenter does not sample or cap the sorted splat list; visual stability is preferred over artificial throughput wins.
- The status overlay reports `drawn=<surface_instances>/<visible_instances>` for the Android Surface path.
- Production Android packaging is intentionally not solved here yet. A future AAR should wrap the same C ABI rather than introduce a separate render contract.

## 3) Emulator flower smoke

After building the APK, push the shared flower dataset into app storage and launch:

```bash
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_SDK_ROOT/platform-tools/adb"

"$ADB" install -r apps/android-demo/app/build/outputs/apk/debug/app-debug.apk
"$ADB" push tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply /data/local/tmp/flowers_1.ply
"$ADB" shell run-as com.gsplat.demo mkdir -p files
"$ADB" shell run-as com.gsplat.demo cp /data/local/tmp/flowers_1.ply files/flowers_1.ply
"$ADB" shell rm -f /data/local/tmp/flowers_1.ply
"$ADB" shell am start -n com.gsplat.demo/.MainActivity
```

For repeatable Surface performance checks, launch with benchmark extras:

```bash
"$ADB" logcat -c
"$ADB" shell am force-stop com.gsplat.demo
"$ADB" shell am start -n com.gsplat.demo/.MainActivity \
  --ez gsplat_benchmark true \
  --ei gsplat_benchmark_frames 120 \
  --ei gsplat_benchmark_warmup_frames 10 \
  --ef gsplat_benchmark_yaw_step 0.001 \
  --ei gsplat_surface_sort_interval 2 \
  --ez gsplat_surface_gpu_preproject false \
  --ez gsplat_surface_gpu_preproject_double_buffer false \
  --ez gsplat_surface_async_sort false \
  --ez gsplat_surface_async_geometry false \
  --ei gsplat_surface_instance_buffers 1 \
  --ei gsplat_surface_frame_latency 2
"$ADB" logcat -d -s GsplatDemo:I | grep BENCHMARK_RESULT
```

Benchmark mode forces a tiny camera orbit each frame so it measures sorted
Surface rebuild cost, not cached static presentation.
`gsplat_surface_sort_interval` controls how often the Surface path refreshes
depth sorting during camera changes. The Android demo default is `2`, which
reuses the previous sorted index order for one camera-change frame while still
rebuilding current-camera geometry every frame; use `1` to force sorting every
frame for comparison.
`gsplat_surface_gpu_preproject=true` enables an experimental path that uploads
only sorted splat ids and generates projected Surface geometry from persistent
GPU source buffers. It is off by default because the current Android benchmark
uses it only for A/B validation.
`gsplat_surface_gpu_preproject_double_buffer=true` makes that GPU preproject
path render the latest completed preproject buffer while submitting the next
preproject compute pass. It requires `gsplat_surface_gpu_preproject=true` and
`gsplat_surface_instance_buffers=2` or `3`; it is for A/B checks because it has
one-frame geometry latency and does not currently beat the default path.
`gsplat_surface_async_sort=true` enables an experimental background sort worker
that double-buffers the latest completed order while the render thread continues
with the previous order. It keeps the full splat count and is intended for
interaction A/B checks.
`gsplat_surface_async_geometry=true` enables an experimental background Surface
instance builder. It keeps the full splat count but renders the latest completed
geometry, so it is not a default quality path.
`gsplat_surface_instance_buffers` controls the optional Surface instance buffer
ring used by the A/B paths. The default is `1`; higher values are allocated
only when requested.
`gsplat_surface_frame_latency` maps to wgpu
`desired_maximum_frame_latency`. The default is `2`.
