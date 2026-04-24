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

- This demo uses `files/flowers_1.ply` when present; otherwise it writes a minimal ASCII PLY into app internal storage.
- On Android emulator, the `SurfaceView` buffer is capped to a 1600px maximum side and the Surface presenter caps the drawn splats to 120,000 instances. This keeps the ranchu Vulkan path stable while still exercising real `wgpu::Surface` presentation.
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
