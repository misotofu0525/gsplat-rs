# gsplat-sort

CPU and GPU sort backends for `gsplat-rs` render ordering.

`SortedAlpha` rendering requires splats ordered back-to-front by camera-space
depth every time the camera moves. This crate provides that ordering behind a
single `SortBackend` trait:

- `CpuSortBackend`: portable CPU sort
- `GpuOddEvenSortBackend`: `wgpu` compute-shader odd-even sort
  (`shaders/odd_even_sort.wgsl`)

## Position in the workspace

`gsplat-render-wgpu` selects and drives a backend per frame; this crate stays
independent of scene and camera types so the sort path can be benchmarked and
tested in isolation.

## License

MIT OR Apache-2.0, at your option.
