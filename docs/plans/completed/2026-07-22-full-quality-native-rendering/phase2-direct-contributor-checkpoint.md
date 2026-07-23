# Phase 2 checkpoint: direct source-order contributor producer

## Scope and publication boundary

This checkpoint implements and qualifies the direct GPU dataflow:

```text
complete Resident scene S
  -> one exact f32 projection per source
  -> source-indexed projected cache
  -> stable source-order contributor prefix C {full32 key, source ID}
  -> reusable external-prefix stable radix over C
  -> source-ID indexed hardware SortedAlpha indirect draw, D = C
```

It is deliberately private and is **not** wired into `SurfacePresenter`.
The current CPU, GPU, and Adaptive product paths remain unchanged while the
new graph is qualified offscreen. No point sampling, LOD reduction, SH
reduction, key truncation, fp16 projected geometry, dynamic resolution, or
upscaling is present.

Implementation:

- `crates/gsplat-render-wgpu/src/preproject_gpu.rs`;
- `crates/gsplat-render-wgpu/shaders/preproject_contributors.wgsl`;
- `crates/gsplat-render-wgpu/shaders/preproject_draw.wgsl`;
- reusable sorter and scan in
  `crates/gsplat-render-wgpu/src/external_prefix_radix.rs`.

## Exact producer contract

`project_count` dispatches sources in ascending source-ID order. Each source
is projected once with the established canonical camera-depth operation tree,
f32 covariance projection, three-sigma axes, `1/255` alpha boundary, and
outward-rounded full-quad clip proof. It writes:

```text
center_alpha_key[source_id] = {center.x, center.y, alpha, bitcast(full32_key)}
axes[source_id]             = {axis_u.x, axis_u.y, axis_v.x, axis_v.y}
```

One count per 128-source workgroup is scanned exclusively. `compact_key_id`
uses a stable lane mask and the scanned group offset, so the radix input is in
ascending source order. The external sorter performs eight stable base-16 LSD
passes in descending full32-key order. Equal depths therefore retain ascending
source IDs. The finalizer writes one 32-byte control record and an independent
16-byte indirect draw record with `D = C`.

Every encode refreshes all `S` projections and the complete sorted prefix.
The group-offset plane and every radix digit prefix are cleared at their fixed
capacity before reuse, so a single allocation is valid across
`full -> sparse -> 0 -> full` frame sequences. The unused `[C..S)` key/ID tail
is never read.

## Non-finite rule resolved from the composed release path

The old release graph has two relevant stages:

1. CPU and direct-GPU source-order producers use positive near/far inclusion
   (`depth >= near && depth <= far`), so a NaN depth never enters `V`;
2. the later projected-quad shader keeps ambiguous alpha and projection/clip
   values fail-open because it may reject only values proven non-contributing.

An isolated projection shader therefore appears depth-fail-open, but the
composed release path has already rejected non-finite depth. Direct `S -> C`
matches the composed contract: positive near/far inclusion rejects NaN depth,
while alpha and conservative projection ambiguity retain the existing
fail-open behavior. This is not a product-semantic change. Valid Resident
scene construction already rejects non-finite source payloads; injected
non-finite buffers exist only in diagnostic oracle coverage.

## Static capacity formula

For logical source capacity `S`, let:

- `A = max(S, 1)`;
- `P = ceil(S / 128) + 1` contributor-count words;
- `G = max(ceil(S / 1024), 1)` radix groups;
- `U = max(16, min_uniform_buffer_offset_alignment)`;
- scan sums/params follow the existing 512-item hierarchical scan formula.

The graph owns:

```text
source center/alpha/key cache   16A bytes
source axes cache               16A bytes
contributor offsets              4P bytes
contributor scan sums/params    hierarchy-dependent
external radix                  16A + 64G + scan/pass/control metadata
indirect draw args                16 bytes
```

This is approximately `48S` plus scan/control metadata for non-empty scenes.
Capacity remains `S` so every valid camera can produce `C = S`; only active
radix traffic is reduced to `C`. This checkpoint does not claim proportional
memory or frame-time improvement.

## Fresh offscreen oracle evidence

The tests run on the local Metal adapter and compare complete outputs rather
than screenshots alone.

1. **Per-source CPU projection oracle**
   - uses the renderer's established CPU camera transform, covariance
     projection, and ellipse construction;
   - exercises an on-screen center, a fully offscreen small ellipse, an
     offscreen center with a large overlapping ellipse, exact/adjacent
     near/far boundaries, and exact/adjacent alpha boundaries;
   - compares `C`, every sorted full32 key/source-ID pair, source-indexed
     center/alpha/key cache, source-indexed axes, and `D = C`;
   - injects NaN depth and confirms it is excluded by the established positive
     source-order depth predicate.
2. **One high-water graph**
   - reuses capacity `S = 1025` for `C = S -> C = 0 -> C = S`;
   - confirms zero indirect X/D in the empty frame and exact restoration of
     the complete sorted output after stale tails remain allocated.
3. **Source-ID draw parity**
   - renders overlapping, differently colored splats through both the
     qualified old `V -> project -> compact C -> draw C` path and the new
     `S -> C -> sort C -> source-ID draw C` path;
   - compares the entire `64x64 RGBA8` target byte-for-byte and confirms the
     reference is non-empty.

The `64x64` target is a deterministic unit-test oracle only. It is not quality
or competitor evidence. Formal acceptance remains native/target resolution:
Android A065 `2412x1080`, desktop/Web `1920x1080`, and the current iOS
simulator surface `2622x1206`, with complete point residency and no dynamic
resolution or upscaling.

Commands and fresh results:

```text
cargo test -p gsplat-render-wgpu preproject_gpu::tests:: --lib
4 passed; 0 failed

cargo test -p gsplat-render-wgpu external_prefix_radix::tests:: --lib
5 passed; 0 failed

cargo test -p gsplat-render-wgpu --lib
263 passed; 0 failed; 5 ignored

cargo check --workspace
passed (the existing desktop-example dead-field warning remains)

cargo check -p gsplat-web --target wasm32-unknown-unknown
passed

cargo clippy -p gsplat-render-wgpu --lib --tests -- -D warnings
passed

cargo fmt --all -- --check
passed
```

## What this checkpoint proves and does not prove

It proves that the new graph preserves contributor classification, stable
full32 order, source-indexed projected data, exact draw count, stale-capacity
safety, and RGBA bytes for the tested oracle cohort. It also proves that the
sorter is reused as a separate component rather than duplicated inside the
projection graph.

At this checkpoint it did not yet prove a frame-time win, cross-platform shader
admission, or formal-resolution parity. The subsequent transactional producer
implementation and same-binary Metal, WebGPU and Adreno experiments completed
those gates; see `phase2-production-producer-ab-checkpoint.md`.

## Subsequent falsifiable slice (completed)

Keep both old and new GPU graphs in one binary behind a diagnostic-only owner,
record `S/C/D`, projection/scan/radix/draw completion timings, and compare
native-resolution RGBA/SSIM plus completed-frame throughput on the existing
scene ladder. CPU/GPU/Adaptive remain explicit choices; no fixed point-count
crossover may be promoted from a single device or scene.
