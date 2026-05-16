# GsplatKit

Local iOS SDK wrapper for the `gsplat-rs` v0.1 C ABI.

This package is a packaging smoke target, not a published binary distribution
yet. Build the local XCFramework before opening the package from Xcode or
SwiftPM:

```bash
bash bindings/apple/scripts/build-xcframework.sh
```

That command writes:

- `bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework`

The Swift target exposes a small Swift-first API over the C ABI and keeps the
raw `GsplatContext` / `GsplatSurfaceRenderer` pointers private. The stable
native contract is still `crates/gsplat-ffi-c/include/gsplat.h`.

Use the package locally by adding `bindings/apple/GsplatKit` as a Swift package
dependency after the XCFramework exists. Minimal offscreen usage:

```swift
import GsplatKit

let renderer = try GsplatContextRenderer(
    configuration: GsplatRenderConfiguration(width: 800, height: 600)
)
try renderer.loadScene(path: sceneURL.path)
try renderer.setAutoCamera()
try renderer.renderFrame()
let stats = try renderer.stats()
renderer.close()
```

UIKit Surface usage keeps the raw native handle private and serializes calls:

```swift
let renderer = try GsplatUIKitSurfaceRenderer(
    view: surfaceView,
    viewController: viewController,
    datasetPath: sceneURL.path,
    width: UInt32(surfaceView.bounds.width),
    height: UInt32(surfaceView.bounds.height)
)
try renderer.renderFrame()
renderer.close()
```

Current limits:

- local binary package only; no remote SwiftPM release artifact
- iOS 17+ in this validation slice
- scene loading is still file-path based
- `SortedAlpha` is the only release-gated render path
- simulator slice builds `aarch64-apple-ios-sim x86_64-apple-ios` by default
  unless `IOS_XCFRAMEWORK_SIM_TARGETS` is overridden
