# gsplat-sort

CPU and GPU sort backends for `gsplat-rs` render ordering.

`SortedAlpha` rendering requires splats ordered back-to-front by camera-space
depth every time the camera moves. This crate provides that ordering behind a
single `SortBackend` trait:

- `CpuSortBackend`: portable CPU sort
  - packs `(depth_key, index)` into `u64`
  - **8-bit LSD radix** with L1-sized histograms (256 buckets)
  - **NEON / AVX2 multi-histogram count** (4 lane-private histograms, then merge)
  - production `sort_values_by_keys` sorts **key bits only** (high 32); LSD
    stability preserves ascending index order among equal depths
  - `sort_pairs` still runs a full 64-bit descending radix
- `GpuOddEvenSortBackend`: `wgpu` compute-shader odd-even sort
  (`shaders/odd_even_sort.wgsl`)

## Position in the workspace

`gsplat-render-wgpu` selects and drives a backend per frame; this crate stays
independent of scene and camera types so the sort path can be benchmarked and
tested in isolation.

## License

MIT OR Apache-2.0, at your option.
