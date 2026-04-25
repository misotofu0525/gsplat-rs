WGSL shader files for `gsplat-render-wgpu`.

- `splat.wgsl`: instanced quad vertex/fragment shader for SortedAlpha reference rendering.
- `splat_surface.wgsl`: Android Surface quad shader that shades from persistent source buffers.
- `splat_surface_preproject.wgsl`: Surface compute prepass that expands sorted source ids into projected quad geometry.
- `preprocess_instances.wgsl`: compute prepass that expands sorted indices into `GpuInstance` data on GPU.
