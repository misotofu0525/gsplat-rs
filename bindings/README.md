# Bindings

Platform bindings over the small v0.1 C ABI in
`crates/gsplat-ffi-c/include/gsplat.h`.

- `android/`: local `gsplat-android` library module, JNI bridge, host smoke,
  and AAR/sample APK scripts
- `apple/`: local `GsplatKit` Swift package wrapper, Swift smoke,
  XCFramework build, and iOS simulator/device scripts

These bindings are local integration slices. They are not published Maven or
binary SwiftPM artifacts yet.
