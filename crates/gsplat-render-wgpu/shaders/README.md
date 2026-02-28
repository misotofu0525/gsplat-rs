WGSL shader files for `gsplat-render-wgpu`.

- `splat.wgsl`: instanced quad vertex/fragment shader for SortedAlpha reference rendering.
- `preprocess_instances.wgsl`: compute prepass that expands sorted indices into `GpuInstance` data on GPU.
