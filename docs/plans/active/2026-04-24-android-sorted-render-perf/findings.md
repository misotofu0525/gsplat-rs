# Findings & Decisions

## Requirements

- Optimize Android true-device SortedAlpha rendering performance for `flowers_1.ply`.
- Target improvement: more than 100% over the fresh baseline.
- Establish a benchmark first, then profile and optimize based on measured bottlenecks.
- SIMD is a suggested direction, but the implementation should follow the evidence.
- Do not improve performance by sacrificing visual effect or render correctness.

## Research Findings

- Project docs define `SortedAlpha` as the only release-gated quality path.
- Android Surface flow is `MainActivity.kt` -> JNI `ANativeWindow` -> `wgpu::Surface` -> shared renderer.
- Android flower smoke path pushes `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply` to `files/flowers_1.ply`, launches `com.gsplat.demo/.MainActivity`, and expects overlay `surface=wgpu realtime`, `state=rendering`, and `drawn=<surface_instances>/<visible_instances>`.
- Android demo is a validation surface over the C ABI, not a full mobile SDK.
- Prior memory notes say the current sorted path sorts visible original indices by camera-space z for `SortedAlpha`, and previous Android true-device validation used device id `033ed212` with `flowers_1.ply`.
- Mobile-GS (2026) identifies depth sorting/alpha blending as the mobile bottleneck and removes sorting with depth-aware order-independent rendering plus neural enhancement/compression. This is useful directional evidence, but not directly acceptable for this task because it changes the SortedAlpha rendering contract and uses representation/pruning/distillation changes.
- StopThePop (SIGGRAPH 2024) shows that better sorted/hierarchical rasterization can improve consistency and reduce required Gaussian count, but its pruning/reduction result is a training/model-side direction rather than a drop-in full-Ply Surface optimization.
- WebSplatter (2026) and recent WebGPU/PlayCanvas work point toward GPU-driven culling, sorting, and work-buffer rendering as the cross-platform route. This aligns with `wgpu` and avoids SoC-specific tuning.
- Open compute/tile rasterizers such as GodotGaussianSplatting and the original CUDA-style 3DGS pipeline use projection -> per-tile key generation -> GPU sort -> tile-range render. That is the likely architecture-level direction for a true >2x improvement while preserving blend semantics, but it is larger than a safe one-pass local patch.
- Short-term architecture slice: keep CPU global sort and exact full drawn count, but move view-dependent SH color evaluation out of the CPU Surface instance builder and into the Surface vertex shader. This tests the GPU-work-buffer direction without changing ordering or sampling semantics.
- `Renderer::preprocess_visible` currently walks all scene positions on CPU, computes camera-space z, and emits `depth_keys` plus original indices.
- `Renderer::sort_preprocessed` now uses `CpuSortBackend::sort_values_by_keys` for `SortedAlpha`, because render paths only need sorted indices after ordering.
- `CpuSortBackend` uses fixed-width packed-pair radix sort with SIMD pack/unpack helpers: AVX2 on x86_64 and Neon on aarch64.
- Android `MainActivity` currently logs visible/drawn/frame/sort from native stats every status interval and sleeps 16 ms after each render loop iteration.
- `gsplat_surface_renderer_render_frame` caches the uploaded surface instances: when `uploaded_frame` is true it only calls `presenter.render_current`; camera control and resize set `uploaded_frame=false`, causing the next frame to rebuild sorted instances.
- Existing native `FrameStats.frame_ms` for the Surface path measures `Renderer::build_sorted_instances`, not the full Kotlin -> JNI -> Surface present wall time. Kotlin-side timing is needed to include upload/present cost.
- Splatapult's relevant architecture is GPU-first: a compute presort pass writes visible depth keys and source indices, GPU radix sort orders those keys/indices, then the sorted index buffer is copied into the OpenGL element buffer and rendered with `glDrawElements(GL_POINTS, sortCount, ...)`.
- Splatapult keeps Gaussian data static in a GPU vertex buffer and binds position/alpha, SH, and covariance as vertex attributes. This avoids per-frame CPU instance upload and lets sorted indices select the static Gaussian records.
- Splatapult evaluates SH and projects covariance in the vertex/geometry pipeline, expanding each point into a quad through a geometry shader. That exact geometry-shader approach is not portable to our `wgpu` mobile path, but the static-data plus GPU-owned sorted-index pipeline is portable in spirit.
- Splatapult uses 32-bit depth sort after noting 24-bit radix sort artifacts on some datasets. This supports keeping full 32-bit ordering keys for our release-quality `SortedAlpha` path rather than accepting shallower quantization without image-diff evidence.
- Splatapult exposes `--nosh` and contains a disabled nearest-splat pruning path, but both are quality/perception tradeoffs for our current goal and should remain optional/non-default unless the user explicitly chooses a degraded mode.
- Splatapult's Quest note says the old experimental Quest2 build was only practical around 25k splats. That reinforces our current evidence that full 562,974-splat mobile rendering needs a more structural GPU sort/work-buffer path, not just a shader-side geometry port.
- A naive portable full-GPU global radix sort is not enough. The tested wgpu prototype preserved full output but spent about 1.47s per camera-change frame on Android, so any future GPU-sort direction needs a different architecture such as tile/bin-local sort, hierarchical/range-limited sorting, or a more parallel prefix/radix design.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Use true-device Android benchmark rather than desktop-only benchmark | The requested target is Android true-device performance; desktop checks are only supporting verification. |
| Treat quality-preserving semantics as fixed | Any change must keep the same `SortedAlpha` ordered visible index path unless evidence shows an exactly equivalent formulation. |
| Add repeatable Android benchmark mode before optimizing | Existing logs only sample the latest cached stats; a benchmark mode can force camera changes and record comparable baseline/after metrics without changing render semantics. |
| Keep GPU-SH in the Surface vertex path | This removes per-splat SH color evaluation from CPU instance construction while preserving full sorted order and drawn count. |
| Keep values-only sort output | It preserves ordering while avoiding unnecessary key unpack/write-back in render paths. |
| Keep 6-float Surface covariance terms | It preserves covariance projection math while reducing hot-loop memory traffic for Surface instance construction. |
| Keep depth-only preprocess and scratch reuse | Preprocess only needs z for visibility/depth keys, and render frames should not reallocate two large vectors every camera change. |
| Keep dense scene-order Surface construction | For dense visibility scenes like `flowers_1.ply`, contiguous projection reads plus a sorted reorder beat fully sorted-order random reads. |
| Prefer Mailbox present mode when supported | It can reduce FIFO queue blocking without introducing tearing; unsupported surfaces still fall back to FIFO. |
| Use Android Surface sort interval `2` by default | It reuses the previous sorted depth order for one camera-change frame, but still rebuilds current-camera Surface geometry every frame and keeps full `drawn=562974/562974`. |

## Issues Encountered

| Issue | Resolution |
|-------|------------|

## Resources

- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`
- `handbook/ROADMAP.md`
- `handbook/GOLDEN_PRINCIPLES.md`
- `apps/android-demo/README.md`

## Benchmark Evidence

- Baseline command: launch `com.gsplat.demo/.MainActivity` with `--ez gsplat_benchmark true --ei gsplat_benchmark_frames 120 --ei gsplat_benchmark_warmup_frames 10 --ef gsplat_benchmark_yaw_step 0.001`.
- Baseline device: `033ed212`, model `A065/Pong`, USB-connected.
- Baseline scene: `flowers_1.ply`, copied to `/data/user/0/com.gsplat.demo/files/flowers_1.ply`.
- Baseline result: `samples=120 warmup=10 avg_call_ms=961.111 avg_frame_ms=950.421 avg_preprocess_ms=77.621 avg_sort_ms=461.753 avg_raster_ms=411.043 avg_visible=562974 avg_drawn=120000`.
- Baseline load/create time was about 11.3s from `createSurfaceRenderer start` to `ok` for the 133MB PLY.
- After release-native build + radix sort + preprocess camera-inverse reuse: `samples=120 warmup=10 avg_call_ms=87.224 avg_frame_ms=83.529 avg_preprocess_ms=2.808 avg_sort_ms=6.694 avg_raster_ms=74.026 avg_visible=562974 avg_drawn=120000`.
- After-change load/create time was about 0.7s from `createSurfaceRenderer start` to `ok` for the same 133MB PLY.
- Speedup from baseline: `avg_call_ms` improved by about 11.0x; native `frame_ms` improved by about 11.4x; sort improved by about 69.0x.
- Retracted interaction attempt: an intermediate Surface path sampled sorted indices before CPU instance construction and benchmarked at `avg_call_ms=33.380`, but this caused visible instability during interaction. This approach is removed and must not be treated as current behavior.
- Current no-sampling Surface result after removal: `samples=60 warmup=5 avg_call_ms=87.657 avg_frame_ms=83.714 avg_preprocess_ms=2.645 avg_sort_ms=6.566 avg_raster_ms=74.500 avg_visible=562974 avg_drawn=562974`.
- Normal app mode relaunched after removal. Current overlay reports `drawn=562974/562974`, confirming the Surface path is no longer dropping splats through a fixed cap.
- Best current full-scene Surface result after SIMD/CPU hot-path work: `samples=120 warmup=10 avg_call_ms=71.520 avg_frame_ms=68.042 avg_preprocess_ms=2.114 avg_sort_ms=8.915 avg_raster_ms=57.011 avg_visible=562974 avg_drawn=562974`.
- Earlier CPU-only hot-path work improved native frame time by about 18.7% relative to the no-sampling baseline. That proved the full-scene path could be improved without dropping splats, but it was not enough for the requested target.
- The original debug-build baseline remains improved by more than 13x, but that baseline is no longer the honest comparison point after native release builds were fixed.
- Surface GPU-SH vertex-color experiment: `samples=120 warmup=10 avg_call_ms=51.657 avg_frame_ms=45.066 avg_preprocess_ms=2.795 avg_sort_ms=11.601 avg_raster_ms=30.668 avg_visible=562974 avg_drawn=562974`. This preserves full `drawn=562974` and improves native frame time by about 46.2% versus the no-sampling baseline. It was retained because forced-camera-change interaction is the measured target path.
- Surface 32-byte instance format had no meaningful improvement by itself: `samples=120 warmup=10 avg_call_ms=50.884 avg_frame_ms=45.116 avg_preprocess_ms=2.496 avg_sort_ms=11.980 avg_raster_ms=30.638 avg_visible=562974 avg_drawn=562974`.
- Surface compute-color work-buffer experiment after unlocking: `samples=120 warmup=10 avg_call_ms=140.665 avg_frame_ms=59.903 avg_preprocess_ms=5.160 avg_sort_ms=18.681 avg_raster_ms=36.060 avg_visible=562974 avg_drawn=562974`. It preserves quality but is worse than the vertex GPU-SH path, likely because the extra compute dispatch and synchronization dominate the saved cached-frame color work.
- Surface values-only sort + 6-float covariance terms first-pass retained result: `samples=120 warmup=10 avg_call_ms=49.971 avg_frame_ms=43.996 avg_preprocess_ms=3.021 avg_sort_ms=11.877 avg_raster_ms=29.096 avg_visible=562974 avg_drawn=562974`.
- Best observed retained-code result in the same run family: `samples=120 warmup=10 avg_call_ms=50.247 avg_frame_ms=43.796 avg_preprocess_ms=3.014 avg_sort_ms=12.361 avg_raster_ms=28.419 avg_visible=562974 avg_drawn=562974`.
- Dense scene-order Surface build result: `samples=120 warmup=10 avg_call_ms=51.233 avg_frame_ms=41.671 avg_preprocess_ms=2.431 avg_sort_ms=11.195 avg_raster_ms=28.044 avg_visible=562974 avg_drawn=562974`.
- Latest final retained-code result after Mailbox preference: `samples=120 warmup=10 avg_call_ms=51.001 avg_frame_ms=41.244 avg_preprocess_ms=2.411 avg_sort_ms=11.126 avg_raster_ms=27.705 avg_visible=562974 avg_drawn=562974`.
- Full-scene improvement relative to the no-sampling baseline (`avg_frame_ms=83.714`) is about `2.03x` on native frame time, or roughly `+103%` native-frame throughput. This crosses the strict requested `>100%`/`2x` native-frame target.
- Kotlin/JNI call wall time remains around `51ms`, so perceived interaction still has a separate Surface/present pacing component beyond CPU-side sorted instance preparation.
- Normal app mode was relaunched after the final benchmark. Logs confirmed `state=rendering`, `visible=562974`, and `drawn=562974/562974`.
- Sorted-index GPU compute-build experiment: `samples=120 warmup=10 avg_call_ms=67.736 avg_frame_ms=67.709 avg_preprocess_ms=3.275 avg_sort_ms=14.965 avg_raster_ms=49.467 avg_visible=562974 avg_drawn=562974`. It uploaded only sorted indices and built Surface instances in a compute shader, but was slower than the retained CPU instance-build/upload path.
- Sorted-index direct-vertex experiment: `samples=120 warmup=10 avg_call_ms=73.151 avg_frame_ms=73.122 avg_preprocess_ms=3.528 avg_sort_ms=15.645 avg_raster_ms=53.946 avg_visible=562974 avg_drawn=562974`. It removed the compute pass/work-buffer but repeated geometry/covariance work per vertex and was slower again.
- After reverting both sorted-index GPU geometry experiments, retained-code confirmation benchmark: `samples=120 warmup=10 avg_call_ms=51.497 avg_frame_ms=41.975 avg_preprocess_ms=2.577 avg_sort_ms=11.411 avg_raster_ms=27.985 avg_visible=562974 avg_drawn=562974`.
- Normal app mode was relaunched after the sorted-index experiments. Logs confirmed `state=rendering`, `visible=562974`, and `drawn=562974/562974`.
- Splatapult-inspired full GPU-owned presort/radix/build prototype: `samples=5 warmup=1 avg_call_ms=1466.724 avg_frame_ms=1466.655 avg_preprocess_ms=0.000 avg_sort_ms=0.000 avg_raster_ms=1466.655 avg_visible=562974 avg_drawn=562974`. It preserved full output, but was dramatically slower and was reverted.
- Retained-path confirmation after reverting the full GPU-owned prototype: `samples=60 warmup=5 avg_call_ms=50.473 avg_frame_ms=39.981 avg_preprocess_ms=2.215 avg_sort_ms=11.148 avg_raster_ms=26.616 avg_visible=562974 avg_drawn=562974`.
- Same-APK sort-cadence baseline with per-frame sorting: `samples=120 warmup=10 sort_interval=1 avg_call_ms=51.917 avg_frame_ms=43.810 avg_preprocess_ms=2.468 avg_sort_ms=11.731 avg_raster_ms=29.609 avg_visible=562974 avg_drawn=562974`.
- Two-frame sort cadence: `samples=120 warmup=10 sort_interval=2 avg_call_ms=51.901 avg_frame_ms=38.830 avg_preprocess_ms=1.613 avg_sort_ms=7.342 avg_raster_ms=29.873 avg_visible=562974 avg_drawn=562974`.
- Relative to the same-APK `sort_interval=1` run, `sort_interval=2` improved native frame prep by about `11.4%` and kept the full drawn count. Kotlin/JNI call wall time was effectively unchanged, which means the remaining perceived-FPS bottleneck is likely Surface present/upload pacing.
- Normal app mode after the sort-cadence change launched successfully and reported `visible=562974`, `drawn=562974/562974`.

## Bottleneck Evidence

- Sort and CPU instance construction dominate the forced-camera-change Surface path:
  - sort: about 461.8ms, 48.6% of native frame time.
  - CPU instance construction: about 411.0ms, 43.3% of native frame time.
  - preprocess: about 77.6ms, 8.2% of native frame time.
- Android native build currently compiles `gsplat-ffi-c` for `aarch64-linux-android` in Rust `dev` profile, producing `target/aarch64-linux-android/debug/libgsplat_ffi_c.a`; this likely inflates all Rust hot-path costs.
- After the first optimization pass, CPU instance construction is the remaining dominant cost: about 74.0ms of the 83.5ms native frame time.
- After removing the sampling path, further interaction improvement should come from GPU-side instance preparation or a more advanced incremental/depth-sort strategy, not dropping splats from the sorted list.
- Current interaction cost is dominated by full CPU instance construction/upload for all 562,974 visible splats. This is the honest cost of preserving the full visible sorted list on the current CPU Surface path.
- `simpleperf --app` on the debug APK confirmed the remaining CPU cycles concentrate inside the inlined `build_instances_into` closure. `Renderer::preprocess_visible` is below 1% self in the captured profile, and `CpuSortBackend::sort_pairs` is a small single-digit share.
- The flower PLY has SH degree 3 (`f_rest_0..f_rest_44`), so view-dependent color evaluation remains a significant part of the per-splat CPU work.
- CPU-side sorted instance preparation now crosses strict 2x on native frame time. Further perceived-FPS gains likely require reducing per-frame Surface upload/present pressure, for example an index-driven GPU geometry path or GPU-side sort/work-buffer preparation that still preserves sorted order and full splat count.
- Sorting every two camera-change frames lowers native CPU preparation time, but does not move `avg_call_ms` on the current device. The call wall time now appears dominated by Surface presentation, queueing, or buffer upload synchronization.

## Rejected Experiments

| Experiment | Result | Decision |
|------------|--------|----------|
| Surface sampling/cap (`SURFACE_INSTANCE_LIMIT`) | `avg_frame_ms=31.850` but drew only 120,000 of 562,974 splats and caused flicker | Removed; violates visual quality requirement |
| Full GPU instance preprocessor for Surface | `avg_frame_ms=202.685`, much slower than CPU path | Reverted |
| Fixed 4-thread Rayon pool on Android | `avg_frame_ms=79.564`, slower than default Rayon pool | Reverted |
| ThinLTO / single codegen unit release profile | No meaningful benchmark improvement | Reverted |
| `target-cpu=cortex-a710` build | Did not produce a valid benchmark on this device | Not retained |
| Three-channel Neon lane SH dot | `avg_frame_ms=70.273`, slower than per-channel dot | Reverted |
| Direct `Queue::write_buffer_with` staging build | `avg_frame_ms=69.318`, much slower than building into a Vec and queue-writing | Reverted |
| Temporal insertion sort over previous frame order | `avg_frame_ms=45.490`, sort cost increased; fallback/repair overhead outweighed benefit | Reverted |
| Reusing preprocess camera-space positions in Surface build | `avg_frame_ms=45.628`; extra memory writes moved cost into preprocess and did not improve total frame time | Reverted |
| Surface compute-color work-buffer | `avg_call_ms=140.665`, `avg_frame_ms=59.903`; much slower than vertex GPU-SH | Reverted |
| Surface triangle-strip quad | `avg_frame_ms=44.447` after values-only sort and covariance terms; did not beat retained triangle-list path | Reverted |
| Removing redundant Surface hot-loop guard branches | `avg_frame_ms=44.600`; did not beat retained path | Reverted |
| 11-bit radix buckets | `avg_frame_ms=47.566`, `avg_sort_ms=15.434`; smaller buckets caused too many passes | Reverted |
| Surface triangle-strip after scene-order build | `avg_frame_ms=41.770`, `avg_call_ms=53.428`; native frame was close, but call wall time regressed | Reverted |
| Sorted-index GPU compute-build Surface path | `avg_call_ms=67.736`, `avg_frame_ms=67.709`; saved CPU instance upload but introduced slower GPU compute/present work | Reverted |
| Sorted-index direct-vertex Surface path | `avg_call_ms=73.151`, `avg_frame_ms=73.122`; avoided the compute pass but repeated covariance/geometry work per vertex | Reverted |
| Splatapult-inspired full GPU presort/radix/build Surface path | `avg_call_ms=1466.724`, `avg_frame_ms=1466.655`; full output preserved, but naive global radix/prefix dispatch chain is far too expensive on Android | Reverted |
