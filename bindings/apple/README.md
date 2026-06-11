# GsplatKit

Apple binding, Swift wrapper, and local packaging scripts.

## Integration boundary

This directory validates Swift -> C ABI -> Rust and includes the local
`GsplatKit` Swift package wrapper. The runnable UIKit Surface sample app lives
under `examples/ios/app`. `GsplatKit` is not a published binary SwiftPM release
yet.

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

This is still a local packaging slice. It does not publish a binary artifact,
tagged SwiftPM release, or polished iOS product API.

## 3) iOS simulator realtime Surface app

Builds a real iOS simulator app bundle, packages the shared flower dataset, and
presents through `UIView` -> UIKit raw window handle -> `wgpu::Surface`.

```bash
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
```

Outputs:

- App bundle: `target/ios-sim-app/GsplatIOSExample.app`
- Bundle ID: `com.gsplat.example.ios`
- Dataset: `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`

Touch controls in the simulator app:

- one-finger drag: orbit around the loaded scene
- two-finger pinch: zoom
- two-finger drag: pan
- double tap: reset the auto camera
- `Import PLY`: open the iOS document picker, copy the selected file into the
  app Documents directory, and restart the Surface renderer with that imported
  scene

Expected overlay includes `state=rendering`, `camera=<mode>`,
`dataset=flowers_1.ply`, and `drawn=<surface_instances>/<visible_instances>`.

If the flower dataset is missing, fetch it first:

```bash
bash tests/datasets/fetch-nvidia-flowers-1.sh
```

This is a realtime validation app under `examples/ios/app`. It compiles
alongside the local `GsplatKit` wrapper, but remains an example rather than a
polished iOS product surface.

Dataset priority matches the Android example shape: the app uses
`Documents/imported_scene.ply` when present, then the bundled `flowers_1.ply`,
then a generated `Documents/minimal_ascii.ply` fallback.

For repeatable Surface performance checks, launch with benchmark args after
`--`:

```bash
bash bindings/apple/scripts/run-ios-sim-app.sh -- \
  --gsplat_benchmark true \
  --gsplat_benchmark_frames 120 \
  --gsplat_benchmark_warmup_frames 10 \
  --gsplat_benchmark_yaw_step 0.001 \
  --gsplat_surface_sort_interval 2 \
  --gsplat_surface_gpu_preproject false \
  --gsplat_surface_gpu_preproject_double_buffer false \
  --gsplat_surface_static_direct true \
  --gsplat_surface_async_sort false \
  --gsplat_surface_async_geometry false \
  --gsplat_surface_instance_buffers 1 \
  --gsplat_surface_frame_latency 2
```

Benchmark mode forces a tiny camera orbit each frame and prints a
`BENCHMARK_RESULT` line to the simulator log. The Surface A/B args map to the
same C ABI controls used by the Android example.

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

Builds and signs a real iPhone app bundle, packages the shared flower dataset,
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
log under `target/ios-device-benchmarks/`.
