WGSL shader files for `gsplat-render-wgpu`.

- `splat_surface_direct.wgsl`: the production Surface/offscreen shader; it reads
  sorted source IDs and projects/shades Gaussian data from persistent GPU
  buffers.
