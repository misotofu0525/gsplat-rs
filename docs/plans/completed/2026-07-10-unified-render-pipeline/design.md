# Unified Render Pipeline Design

## Status

Implemented and locally verified on the `refactor/unified-render-pipeline`
branch. Android/iOS physical-device reruns remain device-side follow-up because
no Android device was attached and the Xcode MCP device workflow was not
enabled in this session.

## Problem

The renderer already has reusable GPU pipelines and persistent scene buffers,
but frame orchestration is duplicated across the C FFI Surface handle and the
Web/WASM wrapper. Each wrapper independently decides when to sort, which buffer
to upload, which pipeline to draw, and whether a previous frame can be reused.

This duplication has produced observable divergence: native static-direct
rendering avoids the CPU-instance cached redraw, while Web used the generic
`render_current()` path and redrew a stationary direct frame through the wrong
pipeline.

The native configuration also expresses mutually exclusive geometry pipelines
through interacting booleans. That makes invalid combinations possible and
forces platform wrappers to understand renderer-internal state.

## Invariants

- `SortedAlpha` remains the only release-gated render mode.
- Depth preprocessing and radix sorting remain CPU operations by default.
- Sorting produces source Gaussian IDs in back-to-front order.
- PLY parsing, coordinate normalization, and `SceneBuffers` stay unchanged.
- Scene/covariance/SH data is uploaded to persistent GPU buffers once per scene.
- Android, iOS, Web, and desktop may choose different geometry pipelines, but
  they must share one scheduling and resource-dirtiness model.
- Every acquired Surface texture receives a render pass. Reusing GPU resources
  must never be confused with reusing a previous swapchain image.
- The v0.1 C ABI remains source-compatible during the refactor.

## Target Model

```text
SceneBuffers
    -> persistent CPU scene/cache
    -> Resident GPU scene buffers

Camera + CPU SortSchedule
    -> depth preprocessing
    -> radix sort
    -> revisioned SortedOrder<u32>

SurfaceRenderSession
    -> upload order only when order revision changes
    -> update camera/viewport parameters only when dirty
    -> select exactly one GeometryPipeline
    -> encode the correct render pass for every Surface frame
    -> report phase-specific statistics
```

## Shared Types

### Geometry pipeline

```rust
pub enum SurfaceGeometryPipeline {
    CpuInstances,
    SortedIndexDirect,
    SortedIndexGpuPreproject,
}
```

- `SortedIndexDirect`: the vertex shader reads the sorted source ID and
  persistent scene buffers directly. This remains the default mobile Surface
  path and is the default interactive desktop Surface path.
- `SortedIndexGpuPreproject`: a compute pass expands sorted IDs into projected
  instances, followed by the common splat raster shader. This remains useful
  for offscreen/desktop and device A/B validation.
- `CpuInstances`: CPU builds projected instances and uploads the
  complete array. This remains the conformance/fallback path.

Offscreen rendering exposes the corresponding `OffscreenGeometryPipeline`
subset (`CpuInstances` and `SortedIndexGpuPreproject`), because the direct
Surface shader targets swapchain presentation. The old Rust `*RasterPath`
names remain type aliases; old C ABI setters map to the Surface enum rather
than directly controlling independent booleans.

### Sort schedule

```rust
pub enum SurfaceSortSchedule {
    Interval(u32),
    AsyncLatest { interval: u32 },
}
```

`Interval(2)` remains the mobile/Web default. The counter advances on changed
camera frames, not on identical redraws. A stationary camera therefore reuses
the current order without repeatedly sorting.

Async sorting remains an experimental native policy. It must not change the
geometry pipeline contract and must preserve the last completed full order.

### Revisioned state

```rust
struct SurfaceFrameRevisions {
    scene: u64,
    camera: u64,
    viewport: u64,
    order: u64,
    pipeline: u64,
}
```

The session tracks resource dirtiness explicitly:

- scene revision changes invalidate GPU scene/order/geometry state;
- camera revision changes may advance sort cadence and always invalidate
  camera-dependent geometry;
- viewport revision changes invalidate projection geometry;
- order revision changes require a compact index-buffer upload;
- pipeline revision changes invalidate path-specific cached geometry.

There is no generic `uploaded_frame` shortcut. CPU/preproject paths may reuse
their existing instance buffers when geometry is clean; direct-index redraws
always use the direct pipeline while reusing scene/order buffers.

## Ownership

`gsplat-render-wgpu` gains a shared `SurfaceRenderSession` that owns:

- `Renderer` and its CPU scene/sort scratch;
- `SurfacePresenter` and GPU Surface resources;
- current camera and viewport state;
- current geometry pipeline and sort schedule;
- current sorted order/upload revision;
- CPU-reference instance scratch;
- path-aware redraw/cache state;
- shared frame statistics.

Platform layers become adapters:

- `gsplat-web` owns browser-facing camera controls and delegates resize,
  camera updates, path selection, and render calls to the session.
- `gsplat-ffi-c` owns the opaque C handle and camera-control ABI, delegating the
  default synchronous lifecycle to the same session.
- Android JNI and Swift wrappers keep their existing public option shapes; the
  options map to shared session configuration.
- Experimental native async sort/geometry controls are preserved behind a
  compatibility layer and may use native-only session hooks until they are
  either promoted or retired by measured evidence.

## Per-frame State Machine

```text
set_camera / resize / set_pipeline
    -> mark revisions dirty

render_frame
    -> if changed-camera cadence requires it: CPU preprocess + radix sort
    -> if order revision changed: upload sorted u32 IDs
    -> if camera/viewport changed: update params
    -> SortedIndexDirect: encode direct render pass
    -> SortedIndexGpuPreproject:
         dirty geometry -> compute + render
         clean geometry -> render existing instance buffer
    -> CpuInstances:
         dirty geometry/order -> CPU build + upload + render
         clean geometry -> render existing instance buffer
    -> submit and present
```

## Metrics

The refactor stops treating `raster_ms` as a universal upload measurement.
`SurfaceFrameOutput::timings` distinguishes:

- `cpu_geometry_ms` for CPU-projected geometry construction;
- `render_submit_ms` for resource updates, encoding, submission, and present;
- `frame_wall_ms` for the end-to-end session call.

Compatibility `FrameStats` still supplies preprocess, sort, visible/drawn, and
`raster_ms`. Offscreen benchmarks separately report geometry/encode/submit CPU
wall, GPU wait, and GPU-complete time. Upload/acquire/present subphases remain
folded into `render_submit_ms` until the presenter has reliable per-phase
instrumentation.

Compatibility `FrameStats` fields remain available, but new benchmark output
must label path-dependent values honestly.

## Default Profiles

| Target | Sort | Geometry pipeline |
|--------|------|-------------------|
| Android Surface | CPU interval 2 | Direct sorted index |
| iOS Surface | CPU interval 2 | Direct sorted index |
| Web/WASM Surface | CPU interval 2 | CPU instances by default; direct sorted index opt-in |
| Desktop realtime Surface | CPU interval 1 unless measured otherwise | Direct sorted index |
| Desktop offscreen | CPU every frame | CPU instances by default; compute preproject opt-in |
| Conformance/reference | CPU every frame | CPU projected reference |

Profiles choose defaults only. Runtime auto-selection by vendor/device is out
of scope until a reproducible cross-device benchmark corpus exists.

## Compatibility Strategy

- Keep existing exported C functions and header declarations.
- Map `set_static_direct(true)` to `SortedIndexDirect`.
- Map `set_gpu_preproject(true)` to `SortedIndexGpuPreproject` when static-direct is
  not selected.
- Map both false to `CpuInstances`.
- Preserve current precedence during migration so existing callers do not
  change behavior unexpectedly.
- Keep Android and Swift wrapper defaults at static-direct/interval 2.
- Keep existing Web `sortedIndexDirect` option as an adapter to the shared path.

## Verification Contract

- Unit-test path selection and setter compatibility.
- Unit-test cadence based on changed camera revisions.
- Unit-test that stationary direct redraw selects the direct pipeline.
- Compare CPU-reference, direct, and compute-preproject offscreen images within
  documented tolerance.
- Browser smoke must pause camera motion and verify a non-black direct frame
  across multiple animation frames.
- Android and iOS verification must build the existing wrappers without C ABI
  changes; true-device performance claims require device runs.
- Benchmarks must include pipeline name, sort schedule, scene, resolution,
  warmup/sample counts, call wall time, and any actual GPU timing available.

## Migration Sequence

1. Add shared types/session and path-aware presenter redraw methods.
2. Migrate Web synchronous rendering and add stationary-frame regression test.
3. Migrate the native default synchronous FFI lifecycle while preserving ABI
   controls and experimental compatibility paths.
4. Align desktop/offscreen naming and reuse the shared pipeline model.
5. Sync current handbook/platform docs and archive superseded task history.
6. Run the complete local verification matrix and document device-only gaps.
