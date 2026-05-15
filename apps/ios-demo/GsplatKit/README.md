# GsplatKit

Local iOS SDK wrapper for the `gsplat-rs` v0.1 C ABI.

This package is a packaging smoke target, not a published binary distribution
yet. Build the local XCFramework before opening the package from Xcode or
SwiftPM:

```bash
bash apps/ios-demo/build-xcframework.sh
```

That command writes:

- `apps/ios-demo/GsplatKit/Binaries/GsplatFFI.xcframework`

The Swift target exposes a small Swift-first API over the C ABI and keeps the
raw `GsplatContext` / `GsplatSurfaceRenderer` pointers private. The stable
native contract is still `crates/gsplat-ffi-c/include/gsplat.h`.

Current limits:

- local binary package only; no remote SwiftPM release artifact
- iOS 17+ in this validation slice
- scene loading is still file-path based
- `SortedAlpha` is the only release-gated render path
- simulator slice follows the local host architecture by default unless
  `IOS_XCFRAMEWORK_SIM_TARGETS` is overridden
