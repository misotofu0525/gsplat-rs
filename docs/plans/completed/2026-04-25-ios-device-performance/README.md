# iOS Device Performance Pass

## Goal

Improve the physical iPhone realtime Surface example performance and keep the comparison with Android honest.

## Baseline

Device: iPhone 17 Pro Max (CoreDevice identifier redacted).
Dataset: `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`.

Current device benchmark before this pass:

```text
BENCHMARK_RESULT dataset=flowers_1.ply samples=60 warmup=5 sort_interval=2 gpu_preproject=false gpu_preproject_double_buffer=false static_direct=false async_sort=false async_geometry=false instance_buffers=1 frame_latency=2 avg_call_ms=81.730 avg_frame_ms=79.518 avg_preprocess_ms=6.717 avg_sort_ms=24.310 avg_raster_ms=48.490 avg_visible=562974 avg_drawn=562974
```

## First Hypothesis

Android APK builds the Rust native renderer with the Rust `release` profile by default, while the new iOS device app script still builds `aarch64-apple-ios` with Cargo's dev profile. This is not a fair Android/iOS performance comparison.

## Planned Changes

- Make `bindings/apple/scripts/build-ios-device-app.sh` default to Rust `release` profile.
- Compile the device Swift app with `-O` by default.
- Keep env overrides for debug investigation.
- Re-run the same benchmark on the same phone before changing renderer internals.

## Results

Changing the device build to Rust `release` + Swift `-O` was the main win:

```text
BENCHMARK_RESULT dataset=flowers_1.ply samples=60 warmup=5 sort_interval=2 gpu_preproject=false gpu_preproject_double_buffer=false static_direct=false async_sort=false async_geometry=false instance_buffers=1 frame_latency=2 avg_call_ms=16.908 avg_frame_ms=8.454 avg_preprocess_ms=0.427 avg_sort_ms=1.591 avg_raster_ms=6.436 avg_visible=562974 avg_drawn=562974
```

This is about 9.4x better than the dev-profile baseline by `avg_frame_ms`, and
about 4.8x better by `avg_call_ms`.

Follow-up A/B sweep on the same release build:

```text
sort1: avg_call_ms=17.813 avg_frame_ms=14.071
sort4: avg_call_ms=19.066 avg_frame_ms=13.752
static_direct: avg_call_ms=17.732 avg_frame_ms=4.446
gpu_preproject: avg_call_ms=17.601 avg_frame_ms=4.834
gpu_preproject_double: avg_call_ms=17.350 avg_frame_ms=5.629
async_sort: avg_call_ms=17.059 avg_frame_ms=10.686
async_geometry: avg_call_ms=16.889 avg_frame_ms=8.204
```

`static_direct` and `gpu_preproject` improve the internal `frame_ms`, but their
wall-clock `avg_call_ms` does not beat the default release path. Keep them as
A/B knobs for now rather than promoting them to default.

Added `bindings/apple/scripts/benchmark-ios-device-app.sh` so future device perf work can
capture `BENCHMARK_RESULT` without repeating a long `devicectl --console`
command by hand.
