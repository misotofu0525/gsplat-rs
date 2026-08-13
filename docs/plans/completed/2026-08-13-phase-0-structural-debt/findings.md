# Findings: Phase 0 Structural Debt

## Call graph for deletions

- `GpuOddEvenSortBackend`: only used by `gsplat-sort` tests. Render crate
  uses `CpuSortBackend` directly. After deletion, `gsplat-sort` no longer
  needs `wgpu` / `pollster` / `bytemuck`. Shader
  `crates/gsplat-sort/shaders/odd_even_sort.wgsl` is gone.
- `RenderMode::SortFree`: `from_u32(1)` and one core unit test. Renderer
  always sorts the preprocessed scratch. C ABI already rejected mode `1`;
  `from_u32(1)` is now `None`.
- FFI no-ops: removed from `gsplat.h` and `gsplat-ffi-c`. Bindings still
  call `set_async_sort` only. Android-only
  `gsplat_android_benchmark_set_order_backend` stays as a hidden live knob
  (not in the public header).
- Unused `gsplat-sort` dependency removed from `gsplat-ffi-c`.

## lib.rs split map (was 3142 lines)

Post-split line counts (`wc -l`, 2026-08-13):

| Module | Lines | Role |
|--------|------:|------|
| `lib.rs` | 1231 | `Renderer` public API; remaining bulk is unit tests |
| `cpu_geometry.rs` | 787 | `#[cfg(test)]` GpuInstance / SH / ellipse oracle |
| `surface_session.rs` | 781 | default CPU frame path, scheduling, telemetry |
| `resident_gpu_order.rs` | 652 | experimental GPU ordering (pre-existing) |
| `surface_presenter.rs` | 611 | Surface resources (pre-existing) |
| `resident.rs` | 511 | preflight, GPU source/params, resident buffers, pipelines |
| `surface_adaptive.rs` | 507 | Adaptive policy + its unit tests |
| `offscreen.rs` | 300 | `#[cfg(not(wasm))]` `GpuRasterizer` |
| `surface_async.rs` | 187 | native async CPU sorter |
| `draw_pass.rs` | 134 | pre-existing draw encoding |
| `math.rs` | 113 | quat/mat3, world covariance, alpha |
| `error.rs` | 116 | `RendererError`, `SurfacePresenterError` |
| `preprocess.rs` | 85 | visibility + depth keys |
| `surface.rs` | 50 | present mode, fit size, instance |
| `timing.rs` | 40 | timers, `wgpu_label` |

Public API was not widened. `world_covariance_terms` / `alpha_values` stay
`pub(crate)`. `build_sorted_instances` / `build_sorted_instances_into` are
`#[cfg(test)] pub(crate)`.

## Isolation

Default `SurfaceOrderBackend::Cpu` already skips Adaptive choose/observe
except for writing `Disabled`. Extraction is organizational: Adaptive and
async code no longer live in the default-path module body. Circular module
imports (`surface_adaptive` ↔ `surface_session`) are intentional and compile.

## Deferred leftovers (out of this Phase 0 pass)

- `gsplat-sort::SortError::{BackendUnavailable, BackendFailure}` remain
  public but unused after GPU odd-even deletion.
- `lib.rs` still hosts the renderer unit-test module; extracting tests was
  not required by ROADMAP item 1.
- Empty preprocess/order/draw stage *traits* were explicitly not created.

## Docs touched

- `handbook/ARCHITECTURE.md`, `handbook/PROJECT_CONTEXT.md`,
  `handbook/ROADMAP.md` item 1 marked landed
- `CHANGELOG.md` Unreleased
- `README.md`, `crates/gsplat-render-wgpu/README.md`,
  `crates/gsplat-sort/README.md`,
  `crates/gsplat-render-wgpu/shaders/README.md`
