# iOS Example

UIKit realtime Surface sample app for the local `GsplatKit` wrapper.

The app uses the same Kitsune showcase and editorial first frame as the Web
and Android examples. Compact live telemetry stays on the canvas; tap
`Studio` for the complete ABI, Surface, camera, dataset, and path diagnostics.

Build and run the simulator app from the repository root:

```bash
bash tests/datasets/fetch-wakufactory-kitune.sh
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
```

The build script prefers Kitsune, falls back to the shared NVIDIA Flowers
fixture, and accepts an explicit PLY path as its first argument. `Open PLY +`
continues to use the native document picker.

The Swift package wrapper, Swift smoke path, XCFramework build, and device
scripts live under `bindings/apple/`. See `bindings/apple/README.md` for the
full simulator/device validation matrix.
