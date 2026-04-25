# Task Plan: Android Sorted Rendering Performance

## Goal

Improve Android true-device performance for the `flowers_1.ply` SortedAlpha render path by at least 100% without reducing visual correctness or lowering render quality.

## Current Phase

Phase 7

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

## Instrumentation Added

- `MainActivity` accepts debug intent extras:
  - `gsplat_benchmark=true`
  - `gsplat_benchmark_frames=<n>`
  - `gsplat_benchmark_warmup_frames=<n>`
  - `gsplat_benchmark_yaw_step=<float>`
- Benchmark mode applies a tiny orbit before each frame to force a quality-preserving SortedAlpha rebuild, then logs `BENCHMARK_RESULT` with average call/frame/preprocess/sort/raster/visible/drawn metrics.

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `rg` returned an error because `.cargo` did not exist | 1 | Ignored the optional path and continued with the repository files that exist. |
| Parallel cargo verification waited on artifact/package locks | 1 | Commands completed successfully; future verification should avoid parallel cargo jobs when timing matters. |

## Notes

- Do not count performance wins from reducing visible splats, lowering resolution, disabling sorting, or changing blending quality.
- Prefer repo-local verification commands from `handbook/VERIFICATION.md`.
- Latest final benchmark for retained code: `avg_call_ms=51.001 avg_frame_ms=41.244 avg_preprocess_ms=2.411 avg_sort_ms=11.126 avg_raster_ms=27.705 avg_visible=562974 avg_drawn=562974`.
- Relative to the full-scene no-sampling baseline (`avg_frame_ms=83.714`), the retained code is about `2.03x` faster on native frame time, or about `+103%` native-frame throughput. Strict `2x` required roughly `<=41.86ms`.
- Kotlin/JNI call wall time is still around `51ms`, so the remaining perceived-FPS work is likely Surface/GPU queue pacing and upload/render submission, not just CPU instance construction.
- Sort-cadence A/B in the same APK: `sort_interval=1` gave `avg_frame_ms=43.810`, while `sort_interval=2` gave `avg_frame_ms=38.830`; both kept `avg_drawn=562974`.
- `sort_interval=2` did not improve `avg_call_ms` (`51.917` vs `51.901`), reinforcing that the remaining interaction feel is gated by Surface present/upload pacing rather than only CPU sort/prep.
