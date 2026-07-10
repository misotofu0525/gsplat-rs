# Findings

## Requirements

- Wire GPU-resident scene + sorted u32 index draw to Web and desktop.
- Reuse `SurfacePresenter::render_direct_sorted_indices` and offscreen
  `GpuInstancePreprocessor`; do not invent a third rasterizer.
- Keep SortedAlpha ordering; CPU sort remains for this pass.

## Research Findings

- Android/iOS FFI already defaults `surface_static_direct=true`.
- Web `gsplat-web` still builds/uploads full `GpuSurfaceInstance[]` each refresh;
  Chrome fair compare showed flowers `avg_upload_ms≈18` of `avg_call_ms≈27`.
- Desktop offscreen `render_frame` CPU-builds `GpuInstance` then uploads; interactive
  viewer already uses `GpuInstancePreprocessor` (sorted indices → GPU compute).

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| `SurfaceRasterPath::{CpuInstances, SortedIndexDirect}` | Explicit path enum instead of scattered bools for Web/Surface clients. |
| Offscreen `OffscreenRasterPath::{CpuInstances, SortedIndexGpuPreproject}` | Desktop PNG/bench reuse existing preprocess compute; no Surface required. |
| Opt-in only | Preserve release defaults until measured evidence lands. |
| Keep CPU sort | Upload was the measured Web bottleneck; GPU sort is a later track. |

## Verification Notes (2026-07-10)

- `GSPLAT_REQUIRE_GPU_CONFORMANCE=1` SortedAlpha conformance passed.
- Desktop minimal PNG: CPU instances vs sorted-index mean abs RGB = 0.0.
- `bench-runner --sorted-index-direct` reports `offscreen_raster_path=sorted_index_gpu_preproject`.
- `bash packages/web/scripts/build-wasm.sh` succeeded.
- Chrome A/B via `gsplat_sorted_index=1` completed for kitune and flowers (below).

## Perf A/B (2026-07-10, Apple M4 Pro / system Chrome)

Protocol: 1600×900, warmup 10 + 60 frames, yaw_step 0.001.

### Desktop `bench-runner` (Metal)

| Dataset | Path | avg_submit_ms | avg_build_encode_submit_ms | avg_gpu_complete_ms |
|---------|------|---------------|----------------------------|---------------------|
| kitune | cpu_instances | 4.24 | 3.31 | 6.35 |
| kitune | sorted_index | 1.15 | 0.13 | 2.93 |
| flowers | cpu_instances | 16.22 | 11.85 | 22.84 |
| flowers | sorted_index | 4.65 | 0.29 | 10.75 |

### Web Chrome (gsplat A/B only)

| Dataset | Path | avg_raf_ms | avg_call_ms | avg_upload_ms |
|---------|------|------------|-------------|---------------|
| kitune | cpu_instances | 13.89 | 13.24 | 8.90 |
| kitune | sorted_index | **8.33** (120Hz vsync) | 1.47 | **0.00** |
| flowers | cpu_instances | 27.64 | 27.16 | 18.25 |
| flowers | sorted_index | **8.33** (120Hz vsync) | 2.46 | **0.00** |

Artifacts: `target/web-perf-compare/ab-kitune-*.json`, `ab-flowers-*.json`.
Upload-bound hypothesis confirmed: Web upload ≈0 on sorted-index; both scenes
hit display refresh. Desktop `build_encode_submit` drops ~10–40×.
