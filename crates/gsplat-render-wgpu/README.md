# gsplat-render-wgpu

`wgpu` renderer and surface presentation paths for `gsplat-rs`.

This is the rendering heart of the workspace. It implements the release-gated
`SortedAlpha` path with a wide Direct-f32 oracle and an exact-count compact
Resident production path. CPU radix and portable GPU visibility/radix produce
the same authoritative back-to-front source-ID order semantics. The default
`ProjectedQuadsExact` raster plan projects each visible splat exactly once into
two rank-indexed 16-byte planes, then feeds hardware instancing with the full
visible count; Adaptive selects the ordering backend from runtime measurements.
Those planes are a persistent exact cache: a frame reuses them only when the
order owner and generation, complete camera, viewport, and draw-count guard are
unchanged. Camera motion, an order refresh, a CPU/GPU transition, resize, or
count change invalidates the cache before drawing. A stationary frame therefore
does not redo projection work, while moving frames retain the same exact
projection and SortedAlpha result.
`GlobalQuads` remains the wide exact image/performance oracle, while
`TiledExact` is a lazily allocated diagnostic implementation rather than a
quality or capacity fallback.

The projected cache deliberately uses two separate `16 * splat_count` storage
bindings instead of one 32-byte binding. This keeps each binding below wgpu's
128 MiB portable storage-binding ceiling through 8,388,608 resident splats,
without reducing membership, SH degree, raster resolution, or draw count.

Main entry points:

- `Renderer::new` / `Renderer::with_config`: GPU-required offscreen rendering
  into a texture, with `FrameStats` reporting and RGBA readback (used by the
  desktop example for PNG output)
- `Renderer::new_for_surface` / `Renderer::with_config_for_surface`: scene,
  preprocessing, and sorting state for a separate `SurfacePresenter`; these
  constructors intentionally do not create an offscreen GPU device
- `ResidentSceneBuilder`: checked direct-to-resident PLY target that preserves
  every source point and SH0-SH3 degree
- `SurfaceRenderSession` / `SurfacePresenter`: shared CPU/GPU/Adaptive order,
  ticketed timing, compact order upload or indirect draw, and realtime
  presentation onto Android `Surface`, iOS `CAMetalLayer`, desktop windows, or
  an HTML canvas
- `GpuInstance` CPU projection helpers: reference/conformance oracle only; they
  are not a selectable production renderer

Shader sources live in [`shaders/`](shaders/) and are documented in
[`shaders/README.md`](shaders/README.md).

## Position in the workspace

Consumes `gsplat_core::SceneBuffers` for Direct/Paged compatibility or
`ResidentSourceSplat` visitors for production Packed, and powers `gsplat-ffi-c`
(C ABI), `gsplat-web` (WASM), and the example apps. Paged is an explicit
partial-residency diagnostic and is never an automatic capacity fallback.

## License

MIT OR Apache-2.0, at your option.
