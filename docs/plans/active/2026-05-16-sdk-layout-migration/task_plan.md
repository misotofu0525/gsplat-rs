# Task Plan: SDK Layout Migration

## Problem

`apps/*-demo` currently mixes three responsibilities:

- runnable validation demos
- local SDK/package source slices
- build or release-like artifact scripts

That shape was useful while proving Android, iOS, and Web integration quickly,
but it is not ideal for external users. A third-party integrator expects SDK
source and packaging scripts to live outside demo app directories, while demos
should read as examples that consume those SDKs.

## Proposed Target Shape

```text
examples/
  android/
  ios/
  web/
bindings/
  android/
  apple/
packages/
  web/
crates/
  gsplat-web/
```

- `examples/`: runnable sample apps and validation fixtures only.
- `bindings/android/`: Android library module, JNI bridge, and AAR build
  scripts.
- `bindings/apple/`: Swift package wrapper, XCFramework scripts, and Swift
  smoke entrypoints.
- `packages/web/`: browser ESM wrapper package and package-level build script.
- `crates/gsplat-web/`: Rust/WASM renderer bindings stay in the Rust workspace.

## Migration Strategy

1. Document the split and keep existing `apps/*-demo` paths working.
2. Move the Web wrapper first from `apps/web-demo/gsplat-web-sdk` to
   `packages/web`, because it has the smallest native-toolchain coupling.
3. Move iOS package files to `bindings/apple` while keeping the UIKit sample
   under `examples/ios`.
4. Move Android library and JNI build scripts to `bindings/android` while
   keeping the Kotlin sample app under `examples/android`.
5. Update CI and handbook verification commands after each move.
6. Keep compatibility wrapper scripts under the old `apps/*-demo/*.sh` paths
   for one release cycle, then remove them in a documented breaking cleanup.

## Non-Goals

- Do not publish Maven, npm, or binary SwiftPM artifacts as part of the layout
  move.
- Do not widen the v0.1 C ABI while moving files.
- Do not make demos product surfaces; they remain examples and validation
  surfaces.

## Acceptance Criteria

- External users can identify SDK/package entrypoints without opening demo app
  directories.
- Existing local validation scripts keep working during the migration through
  compatibility wrappers.
- `handbook/PROJECT_CONTEXT.md`, `handbook/ARCHITECTURE.md`, and
  `handbook/VERIFICATION.md` describe the new current layout only after the
  corresponding files actually move.
- CI covers the moved package build paths before old wrappers are removed.
