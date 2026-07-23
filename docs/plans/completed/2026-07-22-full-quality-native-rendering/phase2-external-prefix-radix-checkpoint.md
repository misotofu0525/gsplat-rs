# Phase 2 checkpoint: external-prefix dynamic full32 radix

## Scope

This checkpoint isolates the sorter from projection and presentation. It does
not change a product default and is not wired into `SurfacePresenter`.

The tested contract is:

```text
external producer writes {full32 key, source ID}[0..C)
  + GPU control {C, active groups, 2D indirect dispatch}
  -> eight stable 4-bit LSD passes
  -> descending full32 keys in A
  -> equal keys retain producer/source order
```

The implementation is in
`crates/gsplat-render-wgpu/src/external_prefix_radix.rs` and
`crates/gsplat-render-wgpu/shaders/external_prefix_radix.wgsl`.

## Capacity and stale-tail contract

Let:

- `A = max(S, 1)` words per key or ID plane;
- `G = max(ceil(S / 1024), 1)` radix group capacity;
- `U = max(16, min_uniform_buffer_offset_alignment)`;
- `g0 = ceil(16G / 512)` and each later `gi = ceil(g(i-1) / 512)` until one.

Actual sorter allocations are:

```text
key A/B                 8A bytes
source-ID A/B           8A bytes
digit/group prefix     64G bytes
scan sums               4 * sum(gi) bytes
scan params             U * scan_level_count bytes
eight pass params       8U bytes
GPU control             32 bytes
```

The input and output tail `[C..S)` is intentionally unspecified. Histogram
and scatter guard every access with GPU-written `C`. Because the scan covers
the fixed capacity prefix, the encoder clears all `64G` prefix bytes before
every radix pass. This is what makes one allocation safe across decreasing and
zero counts without a readback or reallocation.

Eight passes are always encoded, including `C=0`; indirect X is zero for the
empty prefix. An even pass count leaves final keys and IDs in the original A
buffers, so a later source-indexed hardware draw needs no parity-dependent
binding.

## Fresh oracle evidence

The module's CPU-upload harness was run on the local Metal adapter. It compares
the complete GPU key/ID arrays, not only counts.

Passed cases:

- `C = 0, 1, 127, 128, 129, 1023, 1024, 1025, 4099`;
- adversarial values exercising all 32 key bits, including `0`,
  `0xffffffff`, sign-bit boundaries, and repeated equal keys;
- stable equal-key source-ID order;
- one `S=4099` high-water allocation reused as
  `full -> 129 -> 0 -> full`, retaining stale key/ID tails between runs;
- a forced dispatch limit of seven with eight active radix groups, producing
  and executing the two-dimensional indirect command `(x=7, y=2, z=1)`;
- exact byte-plan formulas.

Command and result:

```text
cargo test -p gsplat-render-wgpu external_prefix_radix::tests:: --lib
5 passed; 0 failed
```

Additional gates passed:

```text
cargo check --workspace
cargo check -p gsplat-web --target wasm32-unknown-unknown
cargo clippy -p gsplat-render-wgpu --lib --tests -- -D warnings
cargo fmt --all -- --check
```

The whole render-wgpu library run had `258 passed, 5 ignored` plus one
reproducible failure in the pre-existing tiled timestamp assertion:
`raster_ms` was `None` because Metal returned zero for query slots 8 and 9.
The same tiled test failed when rerun alone, while its project/count/scan/radix
timestamps remained finite. No tiled source is changed by this checkpoint;
this is recorded as an independent verification failure rather than hidden or
attributed to the external radix.

## Next falsifiable slice

Build the direct source-order `S -> C` producer against these external A/control
buffers, then compare the complete `{key, source ID}[0..C)` and source-indexed
f32 cache against the canonical CPU/post-sort projection oracle. Keep it
offscreen until boundary, non-finite, near/far, offscreen-center/large-ellipse,
stale-generation, and RGBA byte-parity gates pass.
