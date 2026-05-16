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
readable text with `gsplat_error_message()` or `NativeBridge.errorMessage()`.

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
- `GsplatSurfaceRenderer`: typed Kotlin handle wrapper
- `GsplatSurfaceOptions`: Surface A/B option bundle
- `GsplatSurfaceStats`: typed frame stats
- `GsplatException`: readable native error wrapper

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
- `Import PLY`: open the Android system file picker, copy the selected file into app internal storage, and restart the Surface renderer with that imported scene

Prereqs:

- Android SDK installed. The scripts read `ANDROID_SDK_ROOT`, then
  `ANDROID_HOME`, then fall back to `~/Library/Android/sdk`.
- Android NDK installed (default version used: `29.0.14206865`)

Build steps:

```bash
bash bindings/android/scripts/build-sample-apk.sh
```

Outputs:

- APK: `examples/android/app/build/outputs/apk/debug/app-debug.apk`
- JNI lib: `bindings/android/gsplat-android/src/main/jniLibs/arm64-v8a/libgsplat_jni.so`

Notes:

- This example uses `files/imported_scene.ply` when present, then `files/flowers_1.ply` when present; otherwise it writes a minimal ASCII PLY into app internal storage.
- Imported files come from the Android system picker as `content://` URIs and are copied into `files/imported_scene.ply` before crossing the JNI/C ABI boundary, which still receives a normal local file path.
- On Android emulator, the `SurfaceView` buffer is capped to a 1600px maximum side. The Surface presenter does not sample or cap the sorted splat list; visual stability is preferred over artificial throughput wins.
- The status overlay reports `drawn=<surface_instances>/<visible_instances>` for the Android Surface path.
- Maven publishing, additional ABIs, and a higher-level `GsplatSurfaceView`
  are intentionally not solved here yet. Future Android SDK work should keep
  wrapping the same C ABI rather than introduce a separate render contract.

## 4) Emulator flower smoke

After building the APK, push the shared flower dataset into app storage and launch:

```bash
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_SDK_ROOT/platform-tools/adb"

"$ADB" install -r examples/android/app/build/outputs/apk/debug/app-debug.apk
"$ADB" push tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply /data/local/tmp/flowers_1.ply
"$ADB" shell run-as com.gsplat.example mkdir -p files
"$ADB" shell run-as com.gsplat.example cp /data/local/tmp/flowers_1.ply files/flowers_1.ply
"$ADB" shell rm -f /data/local/tmp/flowers_1.ply
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
  --ei gsplat_surface_sort_interval 2 \
  --ez gsplat_surface_gpu_preproject false \
  --ez gsplat_surface_gpu_preproject_double_buffer false \
  --ez gsplat_surface_static_direct false \
  --ez gsplat_surface_async_sort false \
  --ez gsplat_surface_async_geometry false \
  --ei gsplat_surface_instance_buffers 1 \
  --ei gsplat_surface_frame_latency 2
"$ADB" logcat -d -s GsplatExample:I | grep BENCHMARK_RESULT
```

Benchmark mode forces a tiny camera orbit each frame so it measures sorted
Surface rebuild cost, not cached static presentation.
`gsplat_surface_sort_interval` controls how often the Surface path refreshes
depth sorting during camera changes. The Android example default is `2`, which
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
`gsplat_surface_static_direct=true` enables an experimental path that draws from
sorted splat ids and persistent GPU source buffers directly in the vertex
shader. It removes projected-instance upload, but repeats projection/covariance
work per quad vertex, so it is off by default and intended for A/B checks.
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
