# Exact CPU Parallel Radix Evidence

## Scope and invariant

The production CPU path still sorts the complete positive-f32 depth key: four
stable base-256 LSD passes over all 32 high key bits. Equal-depth entries keep
input order, and the renderer supplies ascending source IDs, so the tie remains
source-ID ascending. `sort_pairs` still performs all eight passes over the full
packed 64-bit `(key, !value)` total order.

Native inputs below 262,144 entries retain the existing single-thread
NEON/AVX2 path. Larger inputs use at most four Rayon chunks: SIMD per-chunk
histograms, global descending bucket starts, input-ordered per-chunk offsets,
and disjoint stable parallel scatter. This adds 1,024 `usize` counters, or
8,192 bytes on the supported 64-bit native targets. The renderer already uses
Rayon on native targets; this change does not add another pool. WASM keeps the
serial implementation and has no Rayon dependency.

Modified files:

- `crates/gsplat-sort/src/radix.rs`
- `crates/gsplat-sort/src/lib.rs`
- `crates/gsplat-sort/Cargo.toml`
- `crates/gsplat-sort/README.md`
- the `gsplat-sort` dependency entry in `Cargo.lock`

## Full-Truck 1080p A/B

Host: Apple M4 Mac mini, 10 CPU cores, 10 GPU cores, 16 GB unified memory.
The same release executable was used for every run; its SHA-256 was
`6e401ab17b363ed131805f70b333ec1237ff71edfd6191b565982293f9ded6c4`.
`RAYON_NUM_THREADS=1` makes the sorter take its unchanged scalar/SIMD branch;
`RAYON_NUM_THREADS=4` exercises the bounded production parallel branch.

The workload is complete Truck (2,541,226 source = resident splats, SH3),
Packed + ProjectedQuadsExact, forced CPU order, true 1920x1080, alternating
two-view trace SHA-256
`34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3`,
sort interval 1, 20 warmups, and 80 measured frames. Pair order was
scalar/parallel, parallel/scalar, scalar/parallel.

Build and run shape:

```bash
cargo build --release -p desktop-example --features interactive-viewer
RAYON_NUM_THREADS=<1-or-4> target/release/desktop-example \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path packed --interactive \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json \
  --camera-sequence --camera-warmup-frames 20 \
  --camera-measured-frames 80 --camera-loops 1 \
  --surface-benchmark-mode throughput --surface-raster-plan projected \
  --order-backend cpu
```

Raw summary values:

| pair | path | mean preprocess ms | mean sort ms | preprocess + sort ms | throughput FPS |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | scalar | 9.033195 | 8.037492 | 17.070687 | 34.693817 |
| 1 | parallel | 9.562121 | 4.696636 | 14.258757 | 34.022144 |
| 2 | scalar | 9.228941 | 8.309121 | 17.538062 | 33.692745 |
| 2 | parallel | 9.515366 | 4.663328 | 14.178694 | 33.908943 |
| 3 | scalar | 9.154687 | 8.161238 | 17.315925 | 34.253353 |
| 3 | parallel | 9.576396 | 4.640183 | 14.216579 | 34.426304 |
| median | scalar | 9.154687 | 8.161238 | 17.315925 | 34.253353 |
| median | parallel | 9.562121 | 4.663328 | 14.216579 | 34.022144 |

The retained result is a 42.9% reduction in radix wall time and a 17.9%
reduction in the complete CPU order stage. Throughput changed by -0.7% at the
median, while pairwise differences ranged from -1.9% to +0.6%; the 1080p run
is Surface/GPU limited, so this is deliberately not presented as an FPS gain.
Every run presented 100/100 frames, refreshed 100 exact orders, completed all
100 tickets, kept source = resident and SH3, and reported zero fallback,
unsampled order, or outstanding tickets.

Raw logs and SHA-256:

```text
dc8c20dd0d7362cf3e25188f34c4648863a1b01f504f865b58c2dc2202e53738  target/full-quality-surface-metal/cpu-parallel-radix/truck-1080-moving-pair1-parallel.log
717f550b1faa46aa6237065d238fbbb39fbd8d9b51487aaf7ef3b2c530c1dcf7  target/full-quality-surface-metal/cpu-parallel-radix/truck-1080-moving-pair1-scalar.log
5d1793deee6af6a58c2db73943de04561cb6bf783da7249baa625853a1765978  target/full-quality-surface-metal/cpu-parallel-radix/truck-1080-moving-pair2-parallel.log
cec523669d760e323c6ad3573888db15e96a2cb720a91bb7d97e591a45110f11  target/full-quality-surface-metal/cpu-parallel-radix/truck-1080-moving-pair2-scalar.log
f198745016ac601e5ca1596d91181b5fa474256ee2e93ea7a34319bfa82773c9  target/full-quality-surface-metal/cpu-parallel-radix/truck-1080-moving-pair3-parallel.log
b31203f8c56ab7a86b2d68876165003504797520750e988f1986c44085d186ad  target/full-quality-surface-metal/cpu-parallel-radix/truck-1080-moving-pair3-scalar.log
```

## Later current-code CPU cohort

A later renderer-side bounded-parallel preprocessing change is separate from
the radix A/B above. On the same complete-Truck, every-frame, moving 1920x1080
workload, three current-code forced-CPU runs report 43.321400 / 43.169037 /
43.747669 FPS. Mean preprocessing is 4.973609 / 4.935868 / 4.542908 ms and
mean radix is 5.119680 / 5.080678 / 4.819148 ms. Every run presents 100/100
frames and closes all 100 order tickets with no fallback or outstanding work.

Raw evidence is under
`target/full-quality-final/parallel-preprocess-ab-20260723/cpu-parallel-r*.log`.
These runs establish the current forced-CPU result; they are not a replacement
for the older paired scalar/parallel experiment or for the still-required
current-binary forced-GPU/Adaptive cohort. The sibling `cpu-logalpha-r*.log`
files came from a rejected candidate whose code was reverted, so they are
excluded from current product evidence.

## Size crossover

The ignored native ladder benchmark reuses allocated scratch and records the
median of nine sorts at each size:

```bash
RAYON_NUM_THREADS=1 cargo test -p gsplat-sort --release \
  cpu_backend_sort_values_size_ladder_microbench -- --ignored --nocapture
RAYON_NUM_THREADS=4 cargo test -p gsplat-sort --release \
  cpu_backend_sort_values_size_ladder_microbench -- --ignored --nocapture
```

| entries | scalar ms | parallel ms | change |
| ---: | ---: | ---: | ---: |
| 200,000 | 0.575 | 0.533 | serial in both runs; below threshold |
| 300,000 | 0.860 | 0.706 | -17.9% |
| 500,000 | 1.589 | 1.113 | -30.0% |
| 700,000 | 2.322 | 1.408 | -39.4% |
| 1,000,000 | 3.722 | 1.844 | -50.5% |
| 1,800,000 | 7.181 | 4.407 | -38.6% |

This keeps the user's original approximately 200k mobile workload on the
low-overhead SIMD path and establishes a measured crossover above it.

## Verification

Fresh passing commands:

```bash
cargo fmt --check
cargo test -p gsplat-sort --lib
RAYON_NUM_THREADS=4 cargo test -p gsplat-sort cpu_backend_parallel -- --nocapture
RAYON_NUM_THREADS=1 cargo test -p gsplat-sort cpu_backend_parallel -- --nocapture
cargo clippy -p gsplat-sort --all-targets -- -D warnings
cargo clippy -p gsplat-sort --target wasm32-unknown-unknown -- -D warnings
cargo check -p gsplat-sort --target wasm32-unknown-unknown
cargo check -p gsplat-sort --target aarch64-linux-android
cargo check -p gsplat-sort --target aarch64-apple-ios
cargo check -p gsplat-sort --target aarch64-apple-ios-sim
cargo check -p gsplat-sort --target x86_64-apple-ios
cargo check -p gsplat-render-wgpu
```

The 16 active unit tests plus one ignored manual ladder cover random keys,
equal keys across chunk boundaries, non-sequential values, full 64-bit total
order, serial fallback, and byte-for-byte scalar/parallel equivalence for every
radix digit.
