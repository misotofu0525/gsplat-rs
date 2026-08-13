WGSL shader files for `gsplat-render-wgpu`.

- `splat_surface_resident.wgsl`: the production Surface/offscreen shader; it reads
  sorted source IDs and projects/shades Gaussian data from persistent GPU
  buffers.
- `resident_gpu_order.wgsl`: experimental GPU key generation and 4-bit radix
  ordering; not the default production path.
