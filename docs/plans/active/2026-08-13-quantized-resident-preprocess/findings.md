# Findings: Quantized Resident + Compute Preprocess

## Current data plane

- `GpuSurfaceSourceElem` is 64 B: position, world covariance (CPU-baked),
  alpha, SH DC.
- Degree-3 SH rest is 180 B f32 in one binding. A065 128 MiB ceiling is
  745,654 splats (`resident_scene_preflight` tests).
- `splat_surface_resident.wgsl` `vs_main` projects and evaluates SH for
  each of 6 quad vertices.

## Compute preprocess contract

```
CPU vis + sort (unchanged) -> compact source IDs
every presented frame:
  compute[i] = project+SH(source[sorted_ids[i]], camera)
  VS reads projected[instance_index] and emits a quad
```

Sort interval 2 can reuse IDs; projection still refreshes with the camera.

## Bind-group budget

`Limits::downlevel_defaults().max_storage_buffers_per_shader_stage == 4`.
Surface/offscreen devices now request the WebGPU default of 8 when the
adapter exposes it.

Full-f32 compute still uses 4 storage buffers:

0. sorted index words (CPU buffer or GPU-order pairs)
1. source
2. SH rest
3. projected (read_write)

Quantized compute uses 7 storage buffers:

0. sorted index words
1. source
2. SH degree-1 sidecar
4. projected
5-7. SH degree-2/3/4 sidecars

Uniform params sit outside that count. Adapters that only allow 4 fail
quantized preflight with `StorageBuffersPerStage`.

## Quantized hot record (Phase 3, 32 B)

SPZ v4 semantics already decoded in `gsplat-io-spz`:

- positions: f16 bits via `pack2x16float` (no `SHADER_F16` feature)
- rotation: smallest-three `u32` (same packing as SPZ)
- scale: `u8 = round((log_scale + 10) * 16)`
- opacity: linear `u8` after sigmoid
- DC: SPZ `COLOR_SCALE = 0.15` u8
- SH rest: per-degree u8 sidecars (deg1=9 B, deg2=15 B, deg3=21 B, deg4=27 B)

World covariance is reconstructed in compute from rot+scale (view-independent
ALU, once per splat).

## Projected record

WGSL `vec4` alignment makes a center/axes/color struct 48 B. That is the
hot *draw* record, not the resident source. Degree-3 1M: 48 MiB projected,
32 MiB quantized source, 21 MiB largest SH sidecar — each binding under
128 MiB. Projected remains the limiter (~2.79M at 128 MiB).

## Android storage-profile knob

Profile is packed at Surface create. GPU order can be switched later
because it is lazy; storage profile is not. `SurfacePresenter` now keeps
the draw bind-group layout and rebuilds only `ResidentSceneResources`.
The Android sample mirrors the hidden order-backend knob: 0=full-f32,
1=quantized. Default create stays FullF32; quantized rebuild happens
before `set_order_backend` so GPU order keygen sees packed f16 positions.

A065 (Adreno 730) created both quantized preprocess (7 storage buffers)
and quantized GPU-order keygen. Sequential Kitsune CPU runs are not a
randomized pair; first presented frames were similar (~7–8 ms) while
sustained call times differed. GPU quantized had 80/80 GPU frames and
zero sort fallbacks.

## Web storage-profile path

WASM cannot safely `poll(wait_indefinitely)` during a mid-session rebuild,
so the profile is packed at `SurfacePresenter::from_canvas` time. The
optional `createRenderer` token defaults to `full-f32`. Chrome WebGPU
accepted the 7-buffer quantized preprocess on Kitsune. The web collector
rejects a quantized run that fell back to WebGL2.
