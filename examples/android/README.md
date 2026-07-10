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

The Android library module, JNI bridge, host smoke, and AAR build live under
`bindings/android/`. See `bindings/android/README.md` for packaging and device
smoke details.
