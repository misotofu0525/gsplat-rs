# Findings: Paged Entry and Transactional Recovery

## Initial Audit

- Web `createGsplatRenderer` calls native `createRenderer` before forwarding
  `geometryPath`; the native call therefore loads and presents Direct first.
- Android creates its Surface renderer and only then calls the geometry setter.
- `SurfacePresenter` negotiates packed sidecar texture dimensions using the
  entire scene unless the renderer is already paged, then allocates full Direct
  scene resources for the default path.
- `Renderer` retains full `SceneBuffers`; this recovery makes GPU/presenter
  startup bounded but does not claim out-of-core CPU source streaming.
- Runtime switching mutates renderer state and clears presenter resources before
  fallible allocations finish, so an error can poison the old working path.
- Crate boundaries, shared `SurfaceRenderSession`, direct default, and stable
  `SortedAlpha` boundary remain intact. No old telemetry/remote stack is present.

## Relevant Paths

- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-render-wgpu/src/surface_session.rs`
- `crates/gsplat-ffi-c/src/lib.rs`
- `crates/gsplat-ffi-c/include/gsplat.h`
- `bindings/android/jni/gsplat_jni.c`
- `bindings/android/gsplat-android/src/main/kotlin/com/gsplat/android/NativeBridge.kt`
- `crates/gsplat-web/src/wasm.rs`
- `packages/web/src/index.js`

## Open Design Question

The smallest safe native entry is likely an additive experimental
`*_with_geometry_path` constructor that preserves existing direct constructor
signatures. Confirm the existing create-function fan-out and JNI call graph
before choosing the exact API shape.
