# iOS SDK XCFramework Slice

## Goal

Move the iOS integration one step beyond validation-only checks by adding a local
XCFramework build path and a small Swift-first wrapper over the v0.1 C ABI.

## Scope

- Add a local `GsplatKit` Swift package wrapper under `bindings/apple/`.
- Add a repo-local script that builds `GsplatFFI.xcframework` from
  `gsplat-ffi-c`.
- Route the host Swift smoke through the Swift wrapper.
- Keep the existing realtime UIKit example behavior intact.
- Document that this is still a local packaging slice, not a published SwiftPM
  binary release.

## Verification

- `bash bindings/apple/scripts/run-swift-smoke.sh`
- `bash bindings/apple/scripts/build-xcframework.sh`
- `bash bindings/apple/scripts/build-ios-sim.sh`
- `bash bindings/apple/scripts/build-ios-sim-app.sh`
- `cd bindings/apple/GsplatKit && swift package describe --type json`
- `cd bindings/apple/GsplatKit && xcodebuild -scheme GsplatKit -destination 'generic/platform=iOS Simulator' build`
- `cargo check --workspace`
- `cargo fmt --check`
- `git diff --check`
