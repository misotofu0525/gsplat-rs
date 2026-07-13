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

## Web Constructor Decision

- Preserve generated `createRenderer(canvas, bytes, width, height)` as the
  Direct-only compatibility entry.
- Add `createRendererWithGeometryPath(..., path)` for experimental packed/paged
  construction. The Rust binding sets the path before `Renderer::load_scene`.
- The ESM wrapper validates the selector before construction and no longer
  creates Direct first and calls `setGeometryPath` afterwards.

## Native Constructor Decision

- Keep the existing Android/UIKit C create functions ABI-compatible and fixed
  to Direct; add sibling `*_with_geometry_path` experimental constructors.
- Android JNI and both local SDK wrappers now pass the requested path at create
  time. Android and iOS examples no longer perform their initial path selection
  after the Surface session already exists.
- A preselected paged renderer builds spatial pages without retaining Direct
  covariance or alpha caches.

## Remaining Startup Peak

`PagedAtlasGpu::new` still calls `pack_scene_with_encoding` for the full scene
only to obtain SH scales before creating its fixed-slot placeholder. That
O(scene) hot/SH staging allocation must be replaced by metadata-only scans
before constructor-time paged startup can be called bounded.
