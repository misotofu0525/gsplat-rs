# ios-demo

iOS integration demo.

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
