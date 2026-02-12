# android-demo

Android integration demo.

This demo provides two validation paths:

## 1) Host smoke (JNI)

Validates Java/JNI -> C ABI -> Rust on the host machine.

```bash
bash apps/android-demo/run-jni-smoke.sh
```

## 2) Android APK build (arm64-v8a)

Builds a real Android app container and packages `libgsplat_jni.so`.

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

- This demo writes a minimal ASCII PLY into app internal storage at runtime.
