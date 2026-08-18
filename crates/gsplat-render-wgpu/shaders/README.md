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
  ordering with a two-level hierarchical prefix scan; not the default
  production path.
- `gpu_order_visibility.wgsl`: shared NDC footprint test concatenated into
  the full-f32 and quantized keygen modules.
- `resident_gpu_order_compact.wgsl`: GPU visibility-flag scan, compact
  scatter, and indirect sort/draw argument writes.
- `resident_gpu_order_keygen_quantized.wgsl`: the same keygen for quantized
  f16 positions; dequants covariance before the shared footprint test.
