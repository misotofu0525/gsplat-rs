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
- `GsplatSurfaceOptions`: CPU sort cadence, async sort, and frame-latency options
- `GsplatSurfaceStats`: typed frame stats
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

renderer.renderFrame()
val stats = renderer.stats()
renderer.close()
```

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
- `GsplatSurfaceOptions.geometryPath` selects `DIRECT` by default or the
  experimental `PACKED_ATLAS` / local-source `PAGED_ACTIVE_ATLAS` before scene
  derivation and Surface resource creation.
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
  --ei gsplat_surface_sort_interval 2 \
  --ez gsplat_surface_async_sort false \
  --ei gsplat_surface_frame_latency 2 \
  --es gsplat_geometry_path direct
"$ADB" logcat -d -s GsplatExample:I | grep BENCHMARK_RESULT
```

Benchmark mode forces a tiny camera orbit each frame so it measures the shared
CPU-sort + direct-render path rather than stationary presentation.
`gsplat_surface_sort_interval` controls how often the Surface path refreshes
depth sorting during camera changes. The Android example default is `2`, which
reuses the previous sorted index order for one camera-change frame while the
vertex shader still projects the current camera; use `1` to force sorting every
camera-change frame for comparison.
The direct pipeline always draws from sorted splat IDs and persistent GPU
source buffers, so Android has no CPU-instance or compute-preproject A/B mode.
`gsplat_surface_async_sort=true` enables an experimental background sort worker
that double-buffers the latest completed order while the render thread continues
with the previous order. It keeps the full splat count and is intended for
interaction A/B checks.
`gsplat_surface_frame_latency` maps to wgpu
`desired_maximum_frame_latency`. The default is `2`.
`gsplat_geometry_path` selects `direct` (default, release-gated
`SortedIndexDirect`), `packed` (experimental `PackedAtlas`), or `paged`
(experimental four-slot local-source `PagedActiveAtlas`) for on-device smoke
and A/B checks.
The example passes the value to the additive constructor-time geometry entry
and records the resulting `renderer.path` (`sorted_index_direct`,
`packed_atlas`, or `paged_active_atlas`) in the emitted benchmark artifact.
