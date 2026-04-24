# ios-demo

iOS integration demo.

## Integration boundary

This directory validates Swift -> C ABI -> Rust. It is not a published
XCFramework or Swift Package wrapper yet.

The public native contract lives in `crates/gsplat-ffi-c/include/gsplat.h`.
Use the helper functions and named constants from that header instead of copying
magic numbers into Swift:

```swift
var config = gsplat_config_default()
var camera = gsplat_camera_default()
let message = String(cString: gsplat_error_message(rc))
```

`GSPLAT_RENDER_MODE_SORTED_ALPHA` is the only release-gated render mode in v0.1.
Scene loading is path-based today; scene-from-memory loading is outside the
current mobile contract.

This demo provides two validation paths:

## 1) Host smoke (Swift)

Validates Swift -> C ABI -> Rust on the host machine.

```bash
bash apps/ios-demo/run-swift-smoke.sh
```

## 2) iOS simulator target build

Cross-compiles the smoke binary and Rust FFI library for iOS simulator.

```bash
bash apps/ios-demo/build-ios-sim.sh
```

Outputs:

- Binary: `target/ios-sim-smoke`
- Rust target: `aarch64-apple-ios-sim` (on Apple Silicon hosts)
