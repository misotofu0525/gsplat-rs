# Retained Metal Resident visible-compaction experiment

Date: 2026-07-23 (Asia/Shanghai)

## Decision

Retain the visible-only Resident GPU ordering path on macOS Metal. It preserves
the complete source scene and exact visible draw set while making every radix
pass process only the camera-visible prefix. On the complete Truck 1080p moving
workload it reduced median exact GPU order time by 2.51% and raised median
Surface throughput by 4.23% across three same-workload comparisons.

This is a target-gated optimization, not a quality mode:

- it keeps all source splats resident and evaluates visibility for every source
  splat on every order refresh;
- it keeps the complete 32-bit depth key and stable source-ID tie order;
- it does not sample, apply LOD, lower SH degree, lower resolution, or upscale;
- it publishes the exact visible count to both radix indirect dispatch and draw
  indirect arguments; and
- Android retains its qualified stable base16 x 8 path. iOS and Web retain their
  prior paths until equivalent device/browser gates are available.

No Surface scheduling or platform wrapper was changed by this experiment.

## Architecture

The prior Resident path generated one key and source ID for every source splat,
then sorted all `source_count` entries. Invisible entries had key zero and were
still carried through four full stable radix8 passes before the draw consumed
only the visible count.

The retained path reuses the same N-sized ping-pong key and ID buffers and adds
only small group metadata:

1. A 128-thread x 8-item key pass evaluates the canonical FMA camera depth and
   exact inclusive near/far predicate for every source splat. It writes the raw
   key and one visible count per 1,024-source group.
2. The existing hierarchical exclusive scan processes only `group_count + 1`
   counts. The final sentinel becomes the exact visible count.
3. A stable local compaction pass writes visible `(32-bit key, source ID)` pairs
   in original source order. Four 32-bit lane masks per round and scanned group
   offsets make this deterministic without an N-element visibility-flag scan.
4. A one-thread finalizer writes the exact draw instance count and an indirect
   dispatch of `ceil(visible_count / 1024)` groups.
5. Four stable 8-bit LSD radix passes sort only that visible prefix. Each pass
   clears the fixed-capacity digit-major prefix before its dynamic histogram,
   so inactive groups cannot retain data from a previous camera.

The compaction scan metadata is proportional to one u32 per 1,024 source
splats, rather than another per-splat plane. The existing key ping-pong buffer
that previously held generated input is reused as raw-key staging. There is no
new N-sized allocation and no CPU readback in the render path.

## Exactness gates

All GPU gates below ran on the Apple M4 Metal adapter.

### Adversarial synthetic cases

`resident_visible_compaction_is_stable_at_predicate_and_tile_boundaries`
passed for:

- 2,051 all-visible entries;
- 2,051 all-invisible entries, including a zero-workgroup indirect radix
  dispatch;
- exact near and far boundaries plus adjacent out-of-range IEEE-754 values;
- dense equal-depth ties; and
- 4,099 mixed visible/invisible entries crossing multiple 1,024-item tiles.

The same GPU order object also passed an in-place camera predicate sequence of
all-visible, sparse-visible, all-invisible, then all-visible again. This proves
that a shrinking or zero visible prefix cannot leak stale histogram state into
a later refresh.

Every case required the GPU IDs to equal the stable CPU reference and required
`visible_count == draw.instance_count`.

### Complete external scenes

| scene/view | source splats | visible/drawn | CPU-vs-GPU differing ranks | trace hash |
| --- | ---: | ---: | ---: | --- |
| Truck view 0 | 2,541,226 | 1,886,298 | 0 | `34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3` |
| Garden view 1 | 5,834,784 | 4,226,208 | 0 | `1b1aa0a0e1f4a4b9744a4c681fa46a7cd0f50d5390a7260812712e81721f5518` |

Both comparisons checked every visible rank, all 32 key bits, and stable source
ID ties. They are not checksums of a truncated prefix.

The ProjectedQuadsExact-vs-GlobalQuads RGBA8 byte gate passed, and the
GPU-order projection/draw test proved that the projected cache and indirect draw
consume the same exact visible count. The final renderer library run passed 204
tests with 5 intentional external/research ignores. The first full run had one
pre-existing timing-readback flake in the lazy TiledExact diagnostic; the fresh
final run was clean.

## Complete Truck 1920 x 1080 A/B

Both cohorts used complete Truck, 2,541,226 Resident splats, SH3,
Packed + ProjectedQuadsExact, Metal, forced GPU ordering, the same alternating
two-view trace, sort interval 1, 20 warmup frames, and 80 measured frames.
Receipts prove requested, internal, and presented dimensions were all
1920 x 1080, with `dynamic_resolution=disabled`, `upscaling=disabled`, and
`full_resolution=true`. The two views drew exactly 1,886,298 and 1,672,013
visible splats respectively.

| ordinal | path | key/compact ms | radix ms | exact order ms | throughput FPS | frame wall ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | prior full-source radix | 0.665795 | 27.300420 | 27.966589 | 25.874599 | 38.491154 |
| 1 | visible-only radix | 0.940202 | 26.779507 | 27.658649 | 31.043312 | 32.148327 |
| 2 | prior full-source radix | 0.669214 | 27.719772 | 28.389349 | 30.347561 | 32.880001 |
| 2 | visible-only radix | 0.938270 | 26.738133 | 27.676794 | 31.045942 | 32.131195 |
| 3 | prior full-source radix | 0.692533 | 28.136615 | 28.829494 | 29.783491 | 33.493610 |
| 3 | visible-only radix | 0.929474 | 26.848265 | 27.778101 | 30.946514 | 32.235497 |
| median | prior full-source radix | 0.669214 | 27.719772 | 28.389349 | 29.783491 | 33.493610 |
| median | visible-only radix | 0.938270 | 26.779507 | 27.676794 | 31.043312 | 32.148327 |

At the median, compaction raised preprocessing cost by 40.20%, but saved 3.39%
inside radix. The complete exact order stage improved by 2.51%, throughput
improved by 4.23%, and frame wall time improved by 4.02%. Each ordinal pair
favored the retained variant in exact GPU order time. The first prior-path
Surface run was a compositor/throughput outlier and was retained rather than
discarded; the GPU timestamp comparison does not depend on that outlier.

Aggregated across all three runs, both trace views improved:

| visible/drawn | prior exact order ms | visible-only exact order ms |
| ---: | ---: | ---: |
| 1,886,298 | 27.642245 | 27.066793 |
| 1,672,013 | 29.145341 | 28.342235 |

## Resolution interpretation

640 x 360 is useful only as a low-cost smoke or sorting-isolation diagnostic.
It is not acceptable evidence that the product can visually match a competitor
at normal viewing size. This retention decision is based on native 1920 x 1080
Surface rendering with no resolution shortcut. A broad competitor claim still
requires paired same-device, same-resolution, same-camera, same-model image and
timing evidence; this experiment proves a full-quality internal improvement,
not competitor leadership by itself.

## Reproduction command

```text
target/release/desktop-example \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path packed --interactive \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json \
  --camera-sequence --camera-warmup-frames 20 \
  --camera-measured-frames 80 --camera-loops 1 \
  --surface-benchmark-mode throughput --surface-raster-plan projected \
  --order-backend gpu
```

## Final verification

```text
WGPU_BACKEND=metal cargo test -p gsplat-render-wgpu --lib projected_quads_gpu::tests -- --nocapture
WGPU_BACKEND=metal cargo test -p gsplat-render-wgpu --lib -- --nocapture
cargo fmt --check -p gsplat-render-wgpu
cargo clippy -p gsplat-render-wgpu --all-targets -- -D warnings
cargo check --workspace
cargo check -p gsplat-render-wgpu --target wasm32-unknown-unknown
```

All final commands passed. The wasm check proves the target still compiles; it
does not qualify this macOS-only runtime optimization in a browser. The
workspace check retains one existing desktop-example dead-code warning for two
interactive-only parsed fields; the changed renderer crate is warning-clean
under Clippy with warnings denied.

## Raw evidence

Artifacts live under `target/full-quality-surface-metal/visible-compaction/`.

```text
7d5622b360324284f94f0418cfa32c1aaee745e2cc31960650d5b9fe93a0b1bd  adversarial-final.log
92901ccb514ddfe8d64d852eeb04c7d12203ff889edfe42464ecae4c953fb3e1  garden-view1-visible-order.log
be3474862f58412f54a6b294a012454975a8ba940de6e587f95014212351ef65  truck-view0-visible-order.log
93f1f5a2566be624e71d3dbb6e7a4c4f487d72bb07ead5aaf3cd1f8f73c6f92f  projected-vs-global-final.log
774184b30f2147c2bc038e91a00af57ce671d8c1f64bd3d688f590d7c08fe316  render-lib-final.log
6663e2ebba173620d26446930224fbd7b6792ee20e07934369e02fe97184ad91  clippy.log
d48c1aa07c8c0e524b224bd99638ee26059bf812aa7107b3dcff65b7330c77f1  workspace-check.log
0f8ba7512d43d25a6a701f40f52d0d61348ac55b0e3d3f1e0661bcd5d326a0bb  wasm-check.log
ba08bac104d26adf3a0fef6ed685efe97a71214dc70466fbd0fe7193a0d897ac  truck-1080-moving-visible-r1.log
4e789ff619024a3f7729c8ae19b93f67538e9fd8a6e5e443e4cb0de917956b84  truck-1080-moving-visible-r2.log
d9a74d2ca1561a058ccf623dc43215e2612cbe027383e3fccabe7bdcd272f98d  truck-1080-moving-visible-r3.log
```
