# Task Plan: Android Sorted Rendering Performance

## Goal

Improve Android true-device performance for the `flowers_1.ply` SortedAlpha render path by at least 100% without reducing visual correctness or lowering render quality.

## Current Phase

Phase 12

## Phases

### Phase 1: Requirements & Discovery

- [x] Capture user goal and hard constraints.
- [x] Read project context, architecture, verification, roadmap, principles, and Android demo README.
- [x] Inspect current renderer, sort, FFI, JNI, and Android timing/reporting code.
- **Status:** complete

### Phase 2: Baseline & Profiling

- [x] Confirm connected Android device and install the current APK.
- [x] Use `flowers_1.ply` as the benchmark scene.
- [x] Establish repeatable baseline metrics for surface rendering and sort/render frame timing.
- [x] Identify the dominant bottleneck with fresh evidence.
- **Status:** complete

### Phase 3: Implementation

- [x] Optimize the measured bottleneck without changing `SortedAlpha` semantics.
- [x] Prefer existing crate boundaries and keep C ABI stable unless timing telemetry requires a small synchronized ABI update.
- [x] Keep Android demo as validation surface rather than a new SDK surface.
- **Status:** complete

### Phase 4: Verification

- [x] Run Rust checks/tests relevant to sorting and rendering.
- [x] Run Android/JNI/APK checks relevant to the changed path.
- [x] Re-run true-device flower benchmark and compare against baseline.
- **Status:** complete

### Phase 5: Documentation & Handoff

- [x] Record benchmark evidence, bottleneck findings, implementation notes, and unresolved risks.
- [x] Sync docs only if command surfaces, ABI, or repo responsibilities change.
- [x] Summarize delivered speedup and remaining validation boundaries.
- **Status:** complete

### Phase 6: Full-Scene Follow-Up Optimization

- [x] Remove all Surface sampling/capping and benchmark full 562,974 drawn splats.
- [x] Try SIMD-assisted and CPU hot-path optimizations without reducing visual quality.
- [x] Record rejected experiments with measured evidence.
- [x] Research recent papers/repos for cross-platform architecture directions.
- [x] Test a Surface GPU-SH color path as the first architecture-level slice toward a further 2x improvement.
- [x] Validate and reject the Surface compute-color work-buffer experiment after the device is unlocked.
- [x] Decide whether to keep or revert the GPU-SH experiment based on true-device benchmark evidence.
- [x] Test and reject sorted-index GPU geometry variants that upload indices instead of per-frame Surface instances.
- [x] Add follow-up sort/data-layout optimizations and final true-device validation.
- [x] Continue past the 1.9x result with depth-only preprocess, scratch reuse, dense scene-order Surface instance construction, and Mailbox preference.
- [x] Validate a retained full-scene run that crosses strict 2x on native frame time.
- **Status:** complete for this pass; strict 2x is met for native renderer frame time, while Kotlin/JNI call wall time still shows Surface/present pacing overhead.

### Phase 7: Temporal Sort-Cadence Experiment

- [x] Add a configurable Android Surface sort interval.
- [x] Preserve full drawn splat count and current-camera geometry rebuild on every camera-change frame.
- [x] Compare `sort_interval=1` and `sort_interval=2` on the connected Android device with `flowers_1.ply`.
- [x] Relaunch normal app mode and confirm full-scene rendering still starts.
- **Status:** complete; two-frame sort cadence improves native frame prep but does not materially reduce Kotlin/JNI call wall time.

### Phase 8: PlayCanvas Engine Architecture Comparison

- [x] Inspect `playcanvas/engine` current Gaussian Splatting paths.
- [x] Compare PlayCanvas CPU worker sort, WebGPU GPU-sort raster path, and tiled compute path against our Android Surface path.
- [x] Identify cross-platform improvements that do not depend on a specific SoC and do not rely on sampling/culling away full-scene quality by default.
- **Status:** complete for research; next implementation candidate is a static GPU scene buffer plus sorted-id draw path, not a naive full-GPU global radix sort.

### Phase 9: Persistent GPU Source + Sorted-ID Surface Experiment

- [x] Add persistent Surface source buffers for geometry/covariance/alpha data.
- [x] Add a sorted-index GPU preproject path that uploads compact source ids and generates `GpuSurfaceInstance` data on GPU.
- [x] Keep the retained CPU instance-upload path as the Android default unless the GPU preproject path wins true-device `avg_call_ms`.
- [x] Benchmark both paths on the connected Android device with `flowers_1.ply`.
- **Status:** complete for this experiment; GPU preproject is retained as an opt-in A/B path, but not enabled by default because it does not beat the retained CPU path on current true-device call time.

### Phase 10: Async Sort / Double-Buffered Order Experiment

- [x] Add an opt-in background sort worker for Android Surface.
- [x] Double-buffer sorted order: render uses the latest completed order while the next camera order sorts off-thread.
- [x] Keep full splat count and preserve the existing CPU and GPU-preproject render paths.
- [x] Benchmark async sort on `flowers_1.ply` against same-APK defaults.
- **Status:** complete for this experiment; async sorting reduces main-thread native sort accounting, but only marginally improves total Android render-call wall time, so it stays opt-in.

### Phase 11: Remaining Pacing / Double-Buffer Experiments

- [x] Add opt-in Surface instance buffer ring and benchmark 1/2/3 buffers.
- [x] Add opt-in Surface frame-latency setter and benchmark latency 1/2/3.
- [x] Replace Android normal-mode fixed `Thread.sleep(16)` with adaptive frame pacing.
- [x] Add opt-in async Surface geometry builder and benchmark against the retained path.
- [x] Add opt-in GPU preproject double-buffering and benchmark against single-buffer GPU preproject and retained CPU path.
- [x] Evaluate tiled compute / chunk metadata suitability for the current full flower scene.
- **Status:** complete for this pass; none of the new opt-in architecture experiments beat the retained default on Android `avg_call_ms`, but adaptive normal-mode pacing improves real interaction cadence by removing an extra fixed 16ms sleep.

### Phase 12: Four Remaining Architecture Attempts

- [x] Re-test static GPU scene + sorted-id direct draw as an explicit opt-in path.
- [x] Re-check mature GPU sort options against current `wgpu`/Android limits and record whether a safe patch exists.
- [x] Add a tiled/chunk feasibility probe that quantifies flower-scene screen pressure without changing render output.
- [x] Use the probe to decide whether chunk/octree interval metadata can honestly help the current full flower benchmark.
- [x] Run targeted Rust/Android verification and true-device A/B benchmarks for any code paths added.
- **Status:** complete for this pass; static direct draw regresses call wall time, mature GPU sort needs a dedicated key-value/indirect module, chunk culling has no flower-scene headroom, and tiled compute remains the only plausible larger renderer track.

## Key Questions

1. What is the current flower-scene baseline on the connected Android device?
2. Is the dominant cost CPU sorting, CPU instance preparation, GPU rendering/presentation, JNI/Kotlin scheduling, or lock contention?
3. Can the bottleneck be improved by SIMD or data layout changes while preserving exact `SortedAlpha` ordering semantics?
4. What verification proves the improvement is real and not caused by drawing fewer splats, capping resolution, or changing visual output?

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Use `flowers_1.ply` as the benchmark model | User explicitly requested this scene and project verification already documents the Android flower smoke path. |
| Keep `SortedAlpha` semantics intact | Project release boundary says `SortedAlpha` is the only quality-guaranteed render path; user explicitly disallowed quality-sacrificing shortcuts. |
| Record benchmark/profiling evidence in this plan bundle | The task is multi-phase and likely to span many tool calls; persistent notes prevent losing context. |
| Add repeatable Android benchmark mode before optimizing | Existing logs only sample cached stats; a benchmark mode can force camera changes and record comparable baseline/after metrics without changing render semantics. |
| Use release-profile Rust for Android native perf smoke | Debug APK packaging can stay debug, but the renderer hot path should not be benchmarked with unoptimized Rust code. |
| Replace large CPU comparison sort with packed-key radix sort | Packed depth/index keys have a fixed-width integer ordering, so radix sort preserves the existing ordering contract while avoiding `O(n log n)` comparisons. |
| Remove `SURFACE_INSTANCE_LIMIT` sampling/capping | Sampling improved frame time but visibly destabilized the image, so it violates the no-quality-loss requirement. |
| Keep current accepted optimizations CPU-side and no-sampling | They improve full-scene time while preserving `drawn=562974/562974`; larger gains need a different preparation architecture. |
| Avoid SoC-specific target tuning | The project is a cross-platform SDK; any retained optimization should be generic Rust/wgpu/SIMD where applicable, not tuned for one Snapdragon core. |
| Prefer GPU work-buffer style architecture over quality-changing OIT/pruning | Recent 3DGS/mobile/WebGPU work points to GPU-driven culling/sorting/render preparation, while OIT/pruning/distillation would change the current SortedAlpha/full-PLY contract. |
| Keep the Surface GPU-SH vertex path | It cuts CPU instance-build cost while preserving full sorted order and full drawn count. |
| Add values-only CPU sort output for render paths | Render code only consumes sorted indices, so keys no longer need to be unpacked/written back after radix sort. |
| Keep Surface covariance terms in 6-float form | Surface projection only needs the symmetric covariance terms; this reduces per-frame memory traffic without changing the projected ellipse math. |
| Reuse renderer preprocess scratch and compute only depth during preprocess | Sorting only needs camera-space z, so preprocess avoids rebuilding large Vec allocations and avoids unused x/y camera-space math. |
| Use scene-order Surface construction for dense visibility | Flower keeps nearly every splat visible, so the expensive projection pass can read scene arrays contiguously and then reorder the compact instance buffer by sorted index. |
| Prefer Mailbox present mode when available | Mailbox avoids FIFO queue blocking without tearing; unsupported platforms fall back to FIFO. |
| Add configurable Surface sort cadence with Android default `2` | User reported their OpenGL version sorted every two frames without perceptual regression; this keeps full splat count and current-camera geometry while reusing depth order for one camera-change frame. |
| Treat PlayCanvas as an architecture reference, not a drop-in implementation | Its transferable pieces are static GPU data, order-buffer indirection, async CPU sort, interval compaction, and tiled local compute; quality-changing knobs such as LOD budget, alpha clipping, and min contribution are optional/non-default for our current full-scene benchmark. |
| Keep GPU preproject off by default | The persistent-source + sorted-id preproject path preserves full output and removes per-frame CPU geometry build/upload, but current Android `avg_call_ms` is still slower than the retained CPU path due to added GPU compute/render synchronization. |
| Keep async sort off by default | It preserves full output and decouples sorting from the render call, but measured `avg_call_ms` improvement is small and it introduces order-lag semantics during camera movement. |
| Keep async geometry and GPU preproject double-buffering off by default | Both preserve full drawn count, but they render latest-completed geometry and do not improve Android call wall time enough to justify temporal geometry lag. |
| Keep optional Surface buffer ring lazy | Three buffers showed only a noise-level improvement, so extra large instance buffers are allocated only when the benchmark asks for them. |
| Replace fixed normal-mode sleep with adaptive sleep | The previous Android render loop always added 16ms after each non-benchmark frame; the new loop sleeps only when rendering finishes faster than the target frame interval. |
| Keep Phase 12 default-safe | The remaining ideas are architecture experiments. New paths must be opt-in until true-device flower benchmarks show a no-quality-loss win over the retained default. |
| Keep static direct draw off by default | Same-APK true-device benchmark regressed `avg_call_ms` from `52.801` to `63.271` while preserving full drawn count. |
| Treat tiled compute as the next architecture track, not a patch | The spatial probe shows all flower centers in view and heavy tile pressure; a real win requires bin/sort/blend work buffers plus image-diff validation. |

## Instrumentation Added

- `MainActivity` accepts debug intent extras:
  - `gsplat_benchmark=true`
  - `gsplat_benchmark_frames=<n>`
  - `gsplat_benchmark_warmup_frames=<n>`
  - `gsplat_benchmark_yaw_step=<float>`
  - `gsplat_surface_sort_interval=<n>`
  - `gsplat_surface_gpu_preproject=<bool>`
  - `gsplat_surface_gpu_preproject_double_buffer=<bool>`
  - `gsplat_surface_static_direct=<bool>` (Phase 12 opt-in)
  - `gsplat_surface_async_sort=<bool>`
  - `gsplat_surface_async_geometry=<bool>`
  - `gsplat_surface_instance_buffers=<n>`
  - `gsplat_surface_frame_latency=<n>`
- Benchmark mode applies a tiny orbit before each frame to force a quality-preserving SortedAlpha rebuild, then logs `BENCHMARK_RESULT` with average call/frame/preprocess/sort/raster/visible/drawn metrics.

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `rg` returned an error because `.cargo` did not exist | 1 | Ignored the optional path and continued with the repository files that exist. |
| Parallel cargo verification waited on artifact/package locks | 1 | Commands completed successfully; future verification should avoid parallel cargo jobs when timing matters. |

## Notes

- Do not count performance wins from reducing visible splats, lowering resolution, disabling sorting, or changing blending quality.
- Prefer repo-local verification commands from `handbook/VERIFICATION.md`.
- Latest final benchmark for retained code: `avg_call_ms=52.491 avg_frame_ms=35.572 avg_preprocess_ms=1.795 avg_sort_ms=7.159 avg_raster_ms=26.617 avg_visible=562974 avg_drawn=562974`.
- Relative to the full-scene no-sampling baseline (`avg_frame_ms=83.714`), the retained code is about `2.35x` faster on native frame time, or about `+135%` native-frame throughput. Strict `2x` required roughly `<=41.86ms`.
- Kotlin/JNI call wall time is still around `51ms`, so the remaining perceived-FPS work is likely Surface/GPU queue pacing and upload/render submission, not just CPU instance construction.
- Sort-cadence A/B in the same APK: `sort_interval=1` gave `avg_frame_ms=43.810`, while `sort_interval=2` gave `avg_frame_ms=38.830`; both kept `avg_drawn=562974`.
- `sort_interval=2` did not improve `avg_call_ms` (`51.917` vs `51.901`), reinforcing that the remaining interaction feel is gated by Surface present/upload pacing rather than only CPU sort/prep.
- Persistent GPU-source preproject A/B in the same APK: default CPU path with `gpu_preproject=false` remained around `avg_call_ms=51.998` to `54.519`, while `gpu_preproject=true` measured `avg_call_ms=55.352` after shader/layout cleanup. It is useful evidence, but not a default win yet.
- Async sort A/B in the same APK: with `sort_interval=2`, default measured `avg_call_ms=52.488`, while `async_sort=true` measured `avg_call_ms=51.694`; both kept `avg_drawn=562974`. With `sort_interval=1`, default measured `51.667` and async measured `51.502`.
- Surface pacing matrix: default `instance_buffers=1 frame_latency=2` measured `avg_call_ms=51.881 avg_frame_ms=35.265` in the same run family and final retained default measured `52.491/35.572`; `instance_buffers=3` measured `avg_call_ms=51.802 avg_frame_ms=34.851`; latency `1` and `3` did not beat latency `2` in earlier same-APK runs.
- Async geometry builder: `async_geometry=true` measured `avg_call_ms=52.015` with `instance_buffers=1` and `52.132` with `instance_buffers=3`; native stats improve because geometry build is off-thread, but call wall time does not, and the path has geometry latency.
- GPU preproject double buffering: single-buffer GPU preproject measured `avg_call_ms=54.592`; double-buffer preproject measured `54.530` with 2 buffers and `54.511` with 3 buffers, still slower than the retained CPU default.
- Normal Android mode after adaptive pacing starts full-scene rendering and reports roughly 100 frames over about 4.8s after load, with `visible=562974` and `drawn=562974/562974`; the old unconditional 16ms sleep would have added an avoidable delay after every render call.
