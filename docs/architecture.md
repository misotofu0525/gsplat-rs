# Architecture

`gsplat-rs` is structured as a GPU data pipeline with pluggable sort and blend backends.

## Main flow (v0.1 baseline)

1. Load scene (`gsplat-io-ply`) into `SceneBuffers`
2. Preprocess visible set and depth keys (`gsplat-render-wgpu`)
3. Sort (`gsplat-sort` GPU compute backend with CPU fallback)
4. Raster/blend WGSL covariance-driven anisotropic ellipse stage (`gsplat-render-wgpu`)
5. Expose stats and error codes via Rust APIs and C ABI (`gsplat-ffi-c`)
6. Optional offline pack (`gsplat-format` + `gsplat-pack`)
7. Optional desktop interactive viewer loop (`apps/desktop-dev`, feature `interactive-viewer`)

## Gate alignment

- G0: API and contract freeze (`gsplat-core`, `gsplat-ffi-c`, ADR)
- G1: Required PLY field closure (`gsplat-io-ply`)
- G2: SortedAlpha reference pipeline and stats surface (`gsplat-render-wgpu` + `gsplat-sort`)
- G3: Mobile integration entry point via C ABI surface
- G4: Workspace checks/tests/perf + long-stability + docs/release notes
