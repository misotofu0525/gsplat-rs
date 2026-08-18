# Findings: GPU Compaction + Portable Order + Indirect Draw

## Current GPU-order baseline (after this task's host work)

`resident_gpu_order.rs` + `resident_gpu_order.wgsl` +
`resident_gpu_order_compact.wgsl`:

- Keygen writes `(key, id)` plus a visibility flag. Full-f32 and
  quantized keygen share `gpu_order_visibility.wgsl` and apply the
  preprocess NDC footprint when fov/width/height are set. Quantized
  keygen dequants position/rotation/scale for that test only.
- Compact exclusive-scans flags (2-level) and scatters dense pairs into
  `pairs_a`. Culled entries are omitted, not sorted with `key = 0`.
- Eight stable 4-bit LSD radix passes sort `order_meta.visible_count`.
- Histogram/scatter dispatch and the instance draw read GPU-written
  indirect args. Project on the GPU-order path uses the same dispatch.
- Tile: `WORKGROUP_SIZE=64`, `ITEMS_PER_THREAD=4`, `TILE_SIZE=256`.
- Compact + radix stay at 4 storage buffers/stage (scratch merges flags
  and tile sums) so downlevel test devices still validate.
- Records into the caller-owned encoder; never submits/maps/polls.
- Session `visible_count` / `drawn_count` still report the resident
  source count. Updating them would need a later-frame readback.

`prepare_gpu` sets `instance_count = scene.len()` and skips CPU
visibility. `encode_project` then projects every sorted slot, including
`key = 0` culled entries.

## CPU visibility contract

`preprocess.rs` `is_visible` is near/far only
(`depth_z >= near && depth_z <= far`). No frustum or footprint test.
GPU keygen adds NDC footprint when fov is set. Off-screen sources that
CPU still emits as `invalid_splat` contribute no pixels; host image
parity on empty / near-far / screen-edge / degenerate / ties / degree-3
is exact (SSIM 1.0, mean abs RGB 0). The CPU path stays near/far until
a separate decision.

## Attachment points (keep one path)

| Stage | Owner | Item-3 change |
|-------|--------|----------------|
| Visibility / compact / keys / indirect args | preprocess + `resident_gpu_order` | GPU flags, compact scatter, write args |
| Ordering | `resident_gpu_order` + Adaptive | Hierarchical scan; sort compacted count |
| Draw | `draw_pass.rs` | Indirect instance draw from GPU count |
| Session | `SurfaceRenderSession` | Unchanged scheduling; same-frame CPU fallback |

No new crate. No C ABI widening. Experiments attach at these stages only.

## Hierarchical scan layout

Keep histogram/scatter. Replace `prefix` with three kernels:

1. `prefix_block` — each workgroup exclusive-scans 256 groups per digit
   and writes a block total.
2. `prefix_top` — one workgroup exclusive-scans the block totals and
   writes `bucket_base` (higher digits first).
3. `prefix_add` — add the exclusive block prefix onto each group.

Meta trailing array:

```
[0, 16*group_count)             group exclusive prefixes
[16*group_count, +16*blocks)    block sums / exclusive block prefixes
```

`PassParams._pad` becomes `block_count`. Two levels cover
`256²` radix groups ≈ 16M items, above the projected-buffer cap.

## PlayCanvas reference (architecture only)

- Engine 2.19: GPU frustum cull, stream compact, 4-bit radix, indirect
  draw. https://github.com/playcanvas/engine/pull/8453
- PR #8620: portable winner is multi-pass 4-bit radix + hierarchical
  Blelloch (`PrefixSumKernel`) on block histograms. No subgroups.
  OneSweep / decoupled lookback is NVIDIA-only.
- Histogram writes `block_sums[digit * workgroupCount + workgroup]`;
  a separate prefix kernel produces global offsets; reorder scatters.
- Indirect mode (PR #8647) reads element count from a storage buffer
  and writes dispatch args. Adopt the idea locally; do not copy JS.

## wgpu indirect APIs (later phase)

- `ComputePass::dispatch_workgroups_indirect(buffer, offset)` — 12 B
  `{x, y, z}` workgroup counts.
- `RenderPass::draw_indirect(buffer, offset)` — 16 B
  `{vertex_count, instance_count, first_vertex, first_instance}`.
- Repo currently has neither.

## A065 Kitsune pair (compact + indirect, 2026-08-13)

Device `033ed212`, Nothing A065 / SM8475 / Adreno, Android 15, thermal 0.
Full-f32, sort interval 1, 80 measured + 20 warmup, yaw 0.001, 3 randomized
pairs, seed 20260813. Artifacts:
`target/android-sort-benchmarks/compact-v1-kitsune-i1`.

| Backend | mean call_ms (3 runs) | missed / 80 | GPU frames | GPU fallbacks |
|---------|----------------------:|------------:|-----------:|--------------:|
| CPU | 20.705 | 80 | 0 | 0 |
| GPU | 30.256 | 80 | 80 | 0 |

GPU/CPU ratio 1.461. CPU matches the earlier same-day full-f32 Kitsune
CPU run (~20.7 ms). Compact+indirect initializes and presents every
forced-GPU frame, but does not beat CPU on this 279,199-splat orbit.
Session telemetry still reports `visible=drawn=279199` (source count).
This is not the Truck ladder and is not a default-path change.

## A065 Truck interval-1 ladder (compact + indirect, 2026-08-13)

Same device and installed APK. Official Truck subsets 50k/100k/200k/300k/
500k/700k, interval 1, 80+20 frames, 3 randomized pairs, seed 20260813,
thermal 0, `gpu_sort_fallback_count=0` on every run. Artifacts:
`target/android-sort-benchmarks/compact-v1-truck-i1-<tier>`.

Medians of the three run `frame_wall_ms` means (80 frames/run, so missed
counts are out of 240, not the 07-22 180):

| Splats | CPU mean | GPU mean | GPU−CPU | GPU/CPU | Missed CPU/GPU |
|-------:|---------:|---------:|--------:|--------:|---------------:|
| 50k | 3.200 | 6.452 | +3.252 | 2.02 | 0 / 0 |
| 100k | 6.651 | 11.828 | +5.178 | 1.78 | 0 / 0 |
| 200k | 16.429 | 24.095 | +7.666 | 1.47 | 70 / 240 |
| 300k | 52.608 | 62.383 | +9.775 | 1.19 | 240 / 240 |
| 500k | 151.653 | 168.307 | +16.654 | 1.11 | 240 / 240 |
| 700k | 269.036 | 291.107 | +22.071 | 1.08 | 240 / 240 |

All 18 paired differences favored CPU. Relative GPU deficit narrows with
count, same shape as 07-22, and still no crossover before the binding cap.

Do not read absolute ms as a compact-only delta versus 07-22. That ladder
predates per-splat compute preprocess; both backends here pay that cost
(CPU 700k wall 269 ms vs sort 20 ms). The GPU−CPU gap at 700k is +22 ms
versus 07-22's +30.5 ms, but both walls are much higher. CPU default stays.

## Desktop Metal Kitsune pairs (compact + indirect, 2026-08-13)

Apple M4 Pro / Metal, offscreen `bench-runner` with the new experimental
`--order-backend` knob (`Renderer::set_order_backend`, Rust-only; the C
ABI is unchanged). Kitsune 279,199, static default camera
(`static-default-camera-v1`), 120 measured + 10 warmup, 3 runs per
backend in alternating order. All six artifacts validate. Artifacts:
`target/benchmarks/gpu-order/desktop-metal-kitsune-*`.

| Backend | median wall mean (ms) | median p95 | missed / 120 |
|---------|----------------------:|-----------:|-------------:|
| CPU | 2.291 | ~3.4 | 0 |
| GPU | 3.212 | ~4.3 | 0 |

GPU/CPU = 1.40. Notes:

- CPU runs report `visible=drawn=70402` (near/far culls the static view);
  GPU runs report the resident source count 279,199 because the compacted
  count is GPU-written and not CPU-observable (same telemetry limitation
  as the Surface path). The actual GPU draw is indirect and compacted.
- Real-scene image parity on the same host: CPU vs GPU-order renders of
  Kitsune and Flowers are exact (SSIM 1.0, mean abs RGB 0) via
  `gpu_order_matches_cpu_image_on_real_scenes_when_present`.
- `cargo check -p gsplat-web --target wasm32-unknown-unknown` passes with
  only the pre-existing native-async dead-code warnings, so the shared
  Surface GPU-order path still compiles for the browser target.

## Chrome WebGPU Kitsune pairs (compact + indirect, 2026-08-13)

Headless Chrome via the repo collector, Kitsune 279,199 deg-3, 1280×720,
sort interval 2, 80 measured + 20 warmup sync frames, 3 runs per backend
in alternating order. Order backend is wired end to end: wasm
`setOrderBackend` → example `gsplat_surface_order_backend` → collector
`GSPLAT_ORDER_BACKEND`, with the manifest echoing
`order_backend_requested` from live session state. All six artifacts
validate. Artifacts: `target/benchmarks/gpu-order/web-kitsune-*`.

| Backend | avg call_ms (3 runs) | avg CPU sort_ms |
|---------|---------------------:|----------------:|
| CPU | 1.13 / 1.42 / 1.52 | 0.73–0.86 |
| GPU | 0.055 / 0.060 / 0.060 | 0.000 |

Interpretation is deliberately narrow:

- This proves portability: compact + hierarchical scan + indirect
  dispatch/draw runs on Chrome WebGPU (`webgpu`, resident path) with no
  CPU-fallback signature (`sort_ms` mean stays exactly 0 across all 240
  forced-GPU frames; any same-frame fallback would raise it).
- It is NOT a GPU-vs-CPU winner claim. The sync collector measures CPU
  call/submit walls only; moving ordering to the GPU makes the CPU call
  near-zero by construction, and GPU-complete time is unobservable in
  this harness. End-to-end browser frame-wall evidence would need the
  rAF-paced qualification mode.

## Apple Surface pairs (compact + indirect, 2026-08-18)

iPhone 17 Pro Max (iPhone18,2), iOS 26.5.2, Metal, UIKit Surface app.
Order backend reaches the sample through a hidden `gsplat-ffi-c` symbol
(`gsplat_apple_benchmark_set_order_backend`, iOS-only cfg, not in
`gsplat.h`) plus the `gsplat_surface_order_backend` launch extra; the
artifact manifest echoes `order_backend_requested`. Protocol: interval
1, 80 measured + 20 warmup, yaw 0.001, frame latency 2. Artifacts:
`target/benchmarks/gpu-order/ios-*`.

Kitsune 279,199 (3 pairs, alternating): both backends sit on the 60 Hz
pacing slot — GPU avg call 16.667/16.750/16.796 ms with zero CPU
ordering cost; CPU 16.850/16.858/16.901 ms (preprocess ~1.3 + sort
~3.9–4.7 ms fit inside the slot). Saturated by vsync; no meaningful
differential at this scale.

Truck 700k tier (1 pair): CPU holds the cap at 16.646 ms (sort 4.06,
preprocess 1.49); GPU misses it at 17.885 ms (~56 fps). GPU/CPU 1.074 —
the smallest deficit of any device, still no crossover.

Notes: the app's benchmark pacing is 60 Hz even on this ProMotion
panel, so sub-16.7 ms differences are unobservable at Kitsune scale;
the Truck tier is the informative point. Zero fallbacks/config errors
across all eight runs; `avg_sort_ms` is exactly 0 on every GPU run.

## Constraints that still apply

- One resident SortedAlpha path; no parallel geometry selectors.
- C header ↔ `gsplat-ffi-c` stay in sync; no GPU-order knobs in ABI.
- 6-vertex quads stay.
- GPU order does not become default from a kernel microbenchmark.
- Promotion needs empty/tail/edge/tie/timeout/image/Web evidence plus
  paired Android/Apple/desktop/browser artifacts.
- Reduced-width depth keys were measured on 2026-08-18 and demoted.
  They do not reopen the GPU-default question. Evidence:
  [`2026-08-18-depth-key-width`](../2026-08-18-depth-key-width/).
