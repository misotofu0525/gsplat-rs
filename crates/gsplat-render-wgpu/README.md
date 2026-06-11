# gsplat-render-wgpu

`wgpu` renderer and surface presentation paths for `gsplat-rs`.

This is the rendering heart of the workspace. It implements the release-gated
`SortedAlpha` path: per-frame preprocessing, depth sorting (via `gsplat-sort`),
and instanced splat rasterization with WGSL shaders.

Main entry points:

- `Renderer`: offscreen rendering into a texture, with `FrameStats` reporting
  (used by the desktop example for PNG output)
- `SurfacePresenter` / `SurfaceInstanceBuilder`: realtime presentation onto a
  native or browser surface (Android `Surface`, iOS `CAMetalLayer`, HTML
  canvas)
- `GpuInstancePreprocessor`: GPU-side instance preprocessing helpers

Shader sources live in [`shaders/`](shaders/) and are documented in
[`shaders/README.md`](shaders/README.md).

## Position in the workspace

Consumes `gsplat_core::SceneBuffers` (typically produced by `gsplat-io-ply`)
and powers `gsplat-ffi-c` (C ABI), `gsplat-web` (WASM), and the example apps.

## License

MIT OR Apache-2.0, at your option.
