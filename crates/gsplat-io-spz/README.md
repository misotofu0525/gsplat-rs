# gsplat-io-spz

SPZ import and scene-buffer construction for `gsplat-rs`.

This first loader slice accepts Niantic SPZ version 4 files with the plaintext
`NGSP` header and independent ZSTD attribute streams. It builds validated
`gsplat_core::SceneBuffers` ready for rendering.

```rust,no_run
use gsplat_io_spz::load_spz;
use std::path::Path;

let loaded = load_spz(Path::new("scene.spz")).expect("valid SPZ v4");
println!("splats: {}", loaded.summary.gaussians);
// loaded.scene is a gsplat_core::SceneBuffers
```

`load_spz` reads the header and small stream table first, then decompresses one
attribute stream at a time in bounded blocks directly into the final
`SceneBuffers`. It does not retain the whole compressed file or a second copy
of all decoded attributes. `parse_spz_bytes` applies the same decoder to a
caller-owned byte slice without copying that input.

The default entrypoints accept every size representable by SPZ v4 and the host
architecture; checked size arithmetic and fallible scene-buffer reservations
still turn overflow or allocation failure into structured errors. Applications
that intentionally enforce tighter policy budgets can use the matching
limit-aware functions:

```rust,no_run
use gsplat_io_spz::{SpzLoadLimits, load_spz_with_limits};
use std::path::Path;

let limits = SpzLoadLimits {
    max_points: 500_000,
    max_scene_bytes: 256 * 1024 * 1024,
    ..SpzLoadLimits::default()
};
let loaded = load_spz_with_limits(Path::new("scene.spz"), limits)?;
# Ok::<(), gsplat_io_spz::SpzLoadError>(())
```

Cooperative cancellation is available through `load_spz_cancellable` and
`parse_spz_bytes_cancellable`. The cancel callback is polled between header
validation, scene allocation, each ZSTD attribute stream, and bounded decode
blocks. Cancelled loads return `SpzLoadError::Cancelled` and do not publish a
partial scene.

Bounded CPU residency helpers live in `SourceResidencyCaches`: independent
compressed-source and decoded-`SceneBuffers` byte budgets with LRU eviction.
Use them when the same SPZ bytes or decoded scene may be revisited without
widening the per-load `SpzLoadLimits` contract.

## Input conventions

- Extension-free SPZ stores coordinates as `RUB` (right, up, back).
- Runtime `SceneBuffers` use `RUF` (right, up, front). The loader converts
  positions, `xyzw` rotations, and SH rest coefficients from RUB to RUF once
  during loading, matching Niantic's `coordinateConverter(RUB, RUF)` flips.
- Supported SH degrees are 0–3. Degree 0 uses 5 ZSTD streams; degrees 1–3 use
  6 streams and populate `SceneBuffers.sh_rest` in PLY channel-major order.
  Degree 4 returns a structured unsupported-degree error.
- Version 1–3 gzip files and version 4 files with extension ILV records are
  rejected explicitly.

## Format and license

SPZ is an interoperable format from Niantic Labs, distributed under the MIT
license. This crate follows the version 4 layout documented at
<https://github.com/nianticlabs/spz>.

The crate itself is licensed under MIT OR Apache-2.0, at your option.
