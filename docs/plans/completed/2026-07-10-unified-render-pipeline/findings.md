# Findings & Decisions

## Requirements

- Persist the proposed renderer redesign in repository documentation before implementation.
- Rename the current branch using an English name.
- Continue autonomously until the refactor is genuinely complete.
- Preserve CPU sorting as the primary strategy, especially on mobile.
- Unify Web, Android/iOS Surface rendering, and desktop/offscreen configuration without weakening SortedAlpha correctness.

## Research Findings

- Android and iOS already default to `surface_static_direct=true`: CPU depth sort produces source indices, while the Surface direct vertex shader reads persistent GPU scene buffers.
- Web added the same direct path in `gsplat-web`, but copied frame scheduling and cache state into the WASM wrapper.
- Native FFI avoids cached `render_current()` for static-direct; Web omitted that path check, causing stationary direct frames to redraw through the wrong pipeline and go black.
- Native Surface orchestration currently lives in `gsplat-ffi-c`; Web orchestration lives in `gsplat-web`; `gsplat-render-wgpu` owns the low-level presenter and pipelines but not the full frame lifecycle.
- Current native configuration is a boolean matrix (`gpu_preproject`, `static_direct`, double buffering, async sort, async geometry) with hidden compatibility rules.
- Historical Android device work showed CPU sort is a small, acceptable cost after optimization. GPU/present pressure, SH work, overdraw, and Surface backpressure are the more important mobile limits.
- After retained shader optimizations, static-direct became the Android default and reduced native preparation on iOS without regressing call-wall time.
- The native async sort and CPU-geometry workers do not need to live at the ABI boundary. Moving them into the shared session keeps their platform-specific threading while eliminating duplicated render decisions.
- Camera revisions are required for async results: stale sorted orders may remain temporarily usable, but stale CPU-projected geometry must be discarded because it already contains old-camera clip-space data.
- A native async sorter owns a full position snapshot. It must be created lazily when `AsyncLatest` is enabled so the default direct/interval path does not pay duplicate scene-position memory.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Shared session owns frame cadence and path-aware redraw | Resource dirtiness and Surface redraw are different concerns; an acquired Surface texture always needs a render pass even when buffers are unchanged. |
| Sort output becomes a first-class shared value | A revisioned sorted order makes upload decisions explicit and avoids wrapper-specific copies/state. |
| Direct-index remains the mobile/Web Surface default | It already wins current Android device evidence and minimizes per-frame CPU geometry upload. |
| Compute-preproject remains available | It is a useful desktop/offscreen path and a device A/B alternative, but it is not universally faster on tile-based mobile GPUs. |
| CPU-projected remains a reference/fallback | It provides conformance baseline and compatibility without controlling the default architecture. |
| Platform profiles choose policies, not separate implementations | Android, iOS, Web, and desktop may choose different defaults while sharing the same state machine and resource layout. |

## Issues Encountered

| Issue | Resolution |
|-------|------------|
| Existing Web direct cache redraw is incorrect | Treat as a required regression fix and add a stationary multi-frame browser check. |
| Existing timing field `raster_ms` has path-dependent semantics | Redesign stats or relabel metrics before using them for causal performance claims. |

## Resources

- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-ffi-c/src/lib.rs`
- `crates/gsplat-web/src/wasm.rs`
- `crates/gsplat-sort/src/lib.rs`
- `docs/plans/completed/2026-04-24-android-sorted-render-perf/`
- `docs/plans/completed/2026-06-10-android-gpu-render-perf/`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`

## Visual/Browser Findings

- Prior browser review reproduced a black Web canvas after pausing auto-orbit with `SortedIndexDirect`; the CPU-instance path stayed visible and Web telemetry still reported non-zero visible/drawn counts.
