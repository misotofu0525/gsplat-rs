# Task Plan: SDK Layout Migration

## Problem

The old mixed application layout grouped three responsibilities together:

- runnable validation examples
- local SDK/package source slices
- build or release-like artifact scripts

That shape was useful while proving Android, iOS, and Web integration quickly,
but it is not ideal for external users. A third-party integrator expects SDK
source and packaging scripts to live outside example app directories, while the
runnable surfaces should read as examples that consume those SDKs.

## Proposed Target Shape

```text
examples/
  desktop/
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

1. Move sample apps to `examples/`.
2. Move Android and Apple reusable bindings plus package scripts to
   `bindings/`.
3. Move the Web ESM wrapper to `packages/web`.
4. Update CI, root docs, handbook docs, local READMEs, and verification
   commands in the same change.
5. Remove old application-container paths instead of adding compatibility wrappers.

## Non-Goals

- Do not publish Maven, npm, or binary SwiftPM artifacts as part of the layout
  move.
- Do not widen the v0.1 C ABI while moving files.
- Do not make example apps product surfaces; they remain examples and validation
  surfaces.

## Acceptance Criteria

- External users can identify SDK/package entrypoints without opening example app
  directories.
- Existing local validation scripts have new canonical paths under
  `bindings/` and `packages/`.
- `handbook/PROJECT_CONTEXT.md`, `handbook/ARCHITECTURE.md`, and
  `handbook/VERIFICATION.md` describe the new current layout only after the
  corresponding files actually move.
- CI covers the moved package build paths.
