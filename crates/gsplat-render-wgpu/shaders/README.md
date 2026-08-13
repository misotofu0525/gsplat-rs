WGSL shader files for `gsplat-render-wgpu`.

- `splat_common.wgsl`: shared projection, covariance, and SH evaluation used
  by the per-splat compute preprocess pass.
- `splat_preprocess.wgsl`: compute entry that writes one projected record per
  sorted instance from full-f32 resident source buffers.
- `splat_preprocess_quantized.wgsl`: quantized preprocess; SH rest is four
  per-degree u8 sidecars (degrees 1-4) plus a 32-byte hot record.
- `splat_surface_resident.wgsl`: the production Surface/offscreen shader; it
  reads compact projected records and emits a quad. Projection and SH run
  once per splat in compute, not per quad vertex.
- `resident_gpu_order.wgsl`: experimental GPU key generation and 4-bit radix
  ordering; not the default production path.
- `resident_gpu_order_keygen_quantized.wgsl`: the same keygen for quantized
  f16 positions.
