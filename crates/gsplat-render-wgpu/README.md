# gsplat-render-wgpu

`wgpu` renderer and surface presentation paths for `gsplat-rs`.

This is the rendering heart of the workspace. It implements the release-gated
`SortedAlpha` path: CPU visibility/depth sorting (via `gsplat-sort`) followed by
direct GPU projection and splat rasterization from resident scene buffers.

Main entry points:

- `Renderer::new` / `Renderer::with_config`: GPU-required offscreen rendering
  into a texture, with `FrameStats` reporting and RGBA readback (used by the
  desktop example for PNG output)
- `Renderer::new_for_surface` / `Renderer::with_config_for_surface`: scene,
  preprocessing, and sorting state for a separate `SurfacePresenter`; these
  constructors intentionally do not create an offscreen GPU device
- `SurfaceRenderSession` / `SurfacePresenter`: shared CPU-sort, compact-order
  upload, and realtime presentation onto Android `Surface`, iOS
  `CAMetalLayer`, desktop windows, or an HTML canvas
- `GpuInstance` CPU projection helpers: reference/conformance oracle only; they
  are not a selectable production renderer

Shader sources live in [`shaders/`](shaders/) and are documented in
[`shaders/README.md`](shaders/README.md).

## Position in the workspace

Consumes `gsplat_core::SceneBuffers` (typically produced by `gsplat-io-ply`)
and powers `gsplat-ffi-c` (C ABI), `gsplat-web` (WASM), and the example apps.

## License

MIT OR Apache-2.0, at your option.
