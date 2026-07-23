# Rejected Resident radix8 portability experiment

Date: 2026-07-23 (Asia/Shanghai)

## Decision

The portable local-rank variant was correct on Metal but slower in every one
of three same-workload comparisons. It was therefore rejected and fully
removed from the renderer. The retained implementation remains:

- the existing 128-thread × 8-item, four-pass full-32-bit Resident radix8 on
  already-qualified non-Android targets; and
- the existing stable eight-pass full-32-bit base16 path on Android. The
  Android safety gate was not modified.

This experiment does not qualify radix8 on Adreno. The proposed shader was not
advanced to an Android APK after it failed the Metal retention criterion.

## Variant evaluated

The temporary shader used 256 threads × 8 items, or 2,048 input elements per
workgroup tile. It retained four stable 8-bit LSD passes over the complete
32-bit depth key and scattered the key and source ID together. It changed only
the workgroup-local stable rank:

1. explicitly clear all 256 histogram entries before histogramming;
2. explicitly clear 2,048 digit-mask words and 256 ordinary digit offsets
   before scatter;
3. record the current round in eight 32-bit lane masks per digit;
4. let lane `digit` alone total and clear that digit's eight words after every
   round; and
5. carry the resulting ordinary offset into the next round.

This removed the elected-lane `firstTrailingBit` update, the atomic
`digit_prior`, and reliance on implicit zero initialization. Conservative
workgroup storage was 10,240 bytes: `(256 histogram + 2,048 masks + 256
offsets) * 4`.

The temporary Rust integration separated the existing 128-thread keygen and
base16 group count from the 256-thread Resident radix8 group count. Direct and
the four-binding fallback were not changed.

## Correctness gates run before rejection

All commands used Metal on the Apple M4 host. Temporary test code was removed
alongside the rejected shader.

- Adversarial sizes 255/256/257, 2,047/2,048/2,049, and 4,099 passed. Dense
  equal-key runs crossed 32-lane mask words, eight rounds, and the 2,048-item
  tile boundary while retaining stable source order.
- The complete 2,541,226-element synthetic Truck pressure vector matched the
  CPU stable order exactly.
- The full 5,834,784-source Garden view-1 test produced a complete GPU source-ID
  permutation. Its first 4,226,208 visible IDs matched the CPU's exact sorted
  IDs at every rank, with zero differences. The formal 1920×1080 trace hash was
  `1b1aa0a0e1f4a4b9744a4c681fa46a7cd0f50d5390a7260812712e81721f5518`.
- Both Projected-vs-Global RGBA8 byte equality and GPU-order indirect-count
  gates passed.
- The temporary full renderer library suite passed 204 tests with 3 intentional
  ignores.

After the rollback, the retained renderer suite passed 203 tests with 3
intentional ignores, and
`cargo clippy -p gsplat-render-wgpu --all-targets -- -D warnings` passed.

The first Garden attempt in `garden-view1-resident-order.log` intentionally
failed closed before sorting because that ignored test still requested the
four-binding Direct test device. The corrected five-binding run is
`garden-view1-resident-order-final.log`; only the corrected run is evidence.

## Full-Truck 1080p A/B

Both cohorts used complete Truck (2,541,226 source = resident splats, SH3),
Packed + ProjectedQuadsExact, Metal, 1920×1080, forced GPU order, alternating
two-view moving trace, sort interval 1, 20 warmups, and 80 measured frames. The
trace hash was
`34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3`.
No sampling, point reduction, SH downgrade, dynamic resolution, or upscaling
was active.

The portable cohort ran first, followed immediately by a rebuild and restored
implementation cohort. This was not a randomized interleave, so the GPU
timestamp result is the primary retention signal. The first run in both
cohorts also contained a similar Surface/compositor throughput outlier; it was
kept rather than silently discarded.

| ordinal | implementation | mean GPU order ms | throughput FPS |
| --- | --- | ---: | ---: |
| 1 | restored 128×8 | 27.966589 | 25.874599 |
| 1 | portable 256×8 | 28.717400 | 25.040564 |
| 2 | restored 128×8 | 28.389349 | 30.347561 |
| 2 | portable 256×8 | 28.556667 | 30.064529 |
| 3 | restored 128×8 | 28.829494 | 29.783491 |
| 3 | portable 256×8 | 29.154825 | 29.566265 |
| median | restored 128×8 | 28.389349 | 29.783491 |
| median | portable 256×8 | 28.717400 | 29.566265 |

Every ordinal comparison favored the restored implementation. At the median,
the portable variant made the exact GPU order stage 1.16% slower and reduced
end-to-end Surface throughput by 0.73%. A prior same-host retained-radix8
cohort was faster still (27.969784 ms and 30.721285 FPS median), but it is not
needed for the rejection decision.

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

## Raw evidence

Artifacts live under `target/full-quality-surface-metal/radix8-portable/`.

```text
dfad3ee3c69f178300eca145d0d41eac5a75d09d5f5c34a8554951724b9e8a1c  full-truck-order.log
dd084f2e08edc27abbadb564fb2b56b02857cbb55b1d03c15d1756bf053a959b  garden-view1-resident-order-final.log
1b7a6597b386e0a5b3a2c97ed8318d23a39b16a80689c23d67a7f4f286097c37  projected-vs-global-image-gates.log
2b6678c5b542bc4fd3e162a134976ed6536f986be7cab93ea51c55f33d33d381  render-lib.log
a4e261ab2585c42d4c96d2c51b7c8395e101c7efeffec52118e288c6fbb0ebf4  render-lib-restored.log
5fc0535b5fe700e5d8385bdd3d27d3a511476f5c90447e29d2cd5b5e2837b783  truck-1080-moving-portable-r1.log
0384c4a4f05fff166531e03834add677780c98a4a2ff57d91e9baeefd62b381c  truck-1080-moving-portable-r2.log
6af755f9ca551ba9b97dfeef0504e81369ac55548ea8a596ab7ab121c641c972  truck-1080-moving-portable-r3.log
d3e1bf797f2d8d0efac531de56354b59b5e74dff228abcfcd63bbd46f61277f7  truck-1080-moving-restored-r1.log
71774cd64ef46b6b768a2a2e32a926b2af7754912880a39ea78768e1d3f2db9c  truck-1080-moving-restored-r2.log
601373cef829435fa5b531e90fe013caf58975cdf05b806c9273581f43d66d1c  truck-1080-moving-restored-r3.log
```

## Follow-up implication

Making the local-rank structure more explicit is not enough to improve this
workload. A future portable radix8 attempt needs an Android full-order readback
gate and 2412×1080 screenshot gate before safety-gate changes, and it should
also reduce total scatter/local-memory cost enough to beat the retained Metal
path. Until both conditions are met, Android's base16×8 path is the correct
exact-quality choice.
