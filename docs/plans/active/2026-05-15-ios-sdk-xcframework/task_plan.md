# iOS SDK XCFramework Slice

## Goal

Move the iOS integration one step beyond demo-only validation by adding a local
XCFramework build path and a small Swift-first wrapper over the v0.1 C ABI.

## Scope

- Add a local `GsplatKit` Swift package wrapper under `apps/ios-demo/`.
- Add a repo-local script that builds `GsplatFFI.xcframework` from
  `gsplat-ffi-c`.
- Route the host Swift smoke through the Swift wrapper.
- Keep the existing realtime UIKit demo behavior intact.
- Document that this is still a local packaging slice, not a published SwiftPM
  binary release.

## Verification

- `bash apps/ios-demo/run-swift-smoke.sh`
- `bash apps/ios-demo/build-xcframework.sh`
- `bash apps/ios-demo/build-ios-sim.sh`
- `bash apps/ios-demo/build-ios-sim-app.sh`
- `cd apps/ios-demo/GsplatKit && swift package describe --type json`
- `cd apps/ios-demo/GsplatKit && xcodebuild -scheme GsplatKit -destination 'generic/platform=iOS Simulator' build`
- `cargo check --workspace`
- `cargo fmt --check`
- `git diff --check`
