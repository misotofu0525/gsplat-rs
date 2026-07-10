# Progress

## 2026-07-10

- Created branch `feat/web-desktop-sorted-index-pipeline` and this plan bundle.
- Added `SurfaceRasterPath` / `OffscreenRasterPath` in `gsplat-render-wgpu` and
  wired offscreen `SortedIndexGpuPreproject` through `GpuInstancePreprocessor`.
- Web: `createRendererWithOptions` / `setSortedIndexDirect` / `rasterPath`;
  packages/web + examples/web Studio checkbox and `?gsplat_sorted_index=1`.
- Desktop + bench-runner: `--sorted-index-direct` with path reporting.
- Verification: `cargo check/clippy` on touched crates; SortedAlpha conformance;
  wasm build; desktop PNG A/B (minimal mean RGB diff 0); bench-runner path print.
- Docs: ARCHITECTURE, VERIFICATION, Web/desktop/packages READMEs updated.
- Perf A/B (M4 Pro): Web flowers raf 27.64→8.33 ms (upload 18→0); kitune
  13.89→8.33 ms; desktop flowers gpu_complete 22.8→10.8 ms.
