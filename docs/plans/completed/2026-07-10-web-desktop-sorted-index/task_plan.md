# Task Plan: Web/Desktop Sorted-Index Pipeline

## Status

Complete (opt-in landed; default flip deferred).

## Goal

Expose the existing GPU-resident + sorted-index Surface path (`static_direct` /
`splat_surface_direct.wgsl`) to Web and desktop as an opt-in pipeline, keeping
CPU radix sort and `SortedAlpha` semantics. Prove upload-bound Web frames drop
versus the CPU instance path.

## Acceptance

- Opt-in API/flag on Web and desktop; defaults unchanged.
- `cargo check/test/clippy` green; SortedAlpha conformance still passes.
- Web wasm build succeeds; example can select `gsplat_sorted_index=1`.
- Desktop `--sorted-index-direct` uses GPU-resident preprocess path.
- Fair compare / bench shows lower upload/call cost on the new path for dense PLYs.

## Non-goals

- GPU radix sort / tiled raster
- C ABI or mobile default changes
- Flipping Web/desktop defaults in this branch
