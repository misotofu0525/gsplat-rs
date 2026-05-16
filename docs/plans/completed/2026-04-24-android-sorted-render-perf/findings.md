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
- Android flower smoke path pushes `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply` to `files/flowers_1.ply`, launches `com.gsplat.example/.MainActivity`, and expects overlay `surface=wgpu realtime`, `state=rendering`, and `drawn=<surface_instances>/<visible_instances>`.
- Android example is a validation surface over the C ABI, not a full mobile SDK.
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
- PlayCanvas engine reference inspected at commit `aa8ef62` (`playcanvas/engine`, main as of 2026-04-25). It has three relevant Gaussian Splatting paths:
  - CPU-sort raster path: centers are sorted off-thread by a Web Worker, the latest result is deferred and uploaded once per frame, and shaders read a `splatOrder` buffer/texture to map draw order to source splat IDs.
  - WebGPU GPU-sort raster path: interval compaction culls/compacts visible contiguous splat intervals, GPU key generation launches indirectly over visible IDs, GPU radix sort uses compacted IDs as initial values, and the vertex shader reads sorted IDs directly.
  - WebGPU compute renderer: visible IDs are projected into a cache, binned into screen tiles, sorted per tile, then rasterized and blended in compute with early-out.
- The strongest transferable PlayCanvas idea for our Android Surface path is static GPU scene data plus sorted-ID indirection. Our prior sorted-index experiments uploaded sorted indices but still hit poor GPU-side geometry/present behavior; PlayCanvas suggests the draw path needs a persistent work-buffer/source-data layout and indirect count/id plumbing, not just a one-off shader rewrite.
- PlayCanvas's async CPU-sort model is relevant to interaction feel: it applies the latest worker result and reuses prior order while a new sort is in flight. This matches our new two-frame cadence direction, but would decouple the Rust render thread from sorting more cleanly.
- PlayCanvas's interval compaction is only a win when real bounds/interval metadata exists. For the current single full flower PLY with almost all 562,974 splats visible, scene-level compaction alone may not help much; it becomes attractive once we have chunk or octree metadata.
- PlayCanvas exposes LOD, budget, alpha clip, min pixel size, and min contribution controls. These are useful product-level scalability hooks, but they are quality-affecting and should not be counted toward the current no-degradation benchmark unless explicitly enabled as optional modes.
- The current Surface default already keeps SH/color source data resident on GPU, but it does not keep geometry/covariance source data in a shader-readable Surface path by default. It still builds and uploads the projected `GpuSurfaceInstance` buffer on CPU for every camera-change frame.
- The persistent GPU-source + sorted-ID experiment adds resident geometry/covariance/alpha source buffers and a compute preproject shader. It uploads sorted `u32` ids and camera params, then generates the same Surface instance layout on GPU before the existing blended quad render pass.
- Moving this work to GPU is not automatically faster on Android. The first implementation polluted the default shader with a wider source stride and caused a default-path call-time regression; splitting the compact color buffer from the wider preproject source buffer restored the retained CPU path.
- After cleanup, GPU preproject still did not beat the default path on `avg_call_ms`. It reduces native CPU prep stats because CPU geometry build/upload is removed from `FrameStats`, but total Kotlin/JNI render-call wall time remains slower due to GPU compute/render synchronization.
- Async sorting was added as an opt-in Android Surface experiment. The worker owns only cloned scene positions and CPU sort scratch; it never touches `wgpu` device/surface state. The render thread keeps the latest completed sorted order and schedules a new background order for the current camera when the cadence threshold is reached.
- Async sorting preserves full splat count and exact sorted-index data when applied, but it intentionally allows one completed-order lag during camera movement. This matches the "latest available order" model from PlayCanvas-style worker sorting, but it is a temporal tradeoff and should not be silently enabled without benchmark and visual confirmation.
- Async sorting moves sort cost out of the synchronous render call, but it does not materially shift Android call wall time on the current device. This reinforces that the remaining user-visible FPS bottleneck is not just CPU depth sorting.
- Surface instance buffer rings do not materially change Android call wall time. Three buffers gave only a noise-level improvement (`51.881ms -> 51.802ms` in one same-APK run), so extra large instance buffers should stay lazy and opt-in.
- Changing wgpu `desired_maximum_frame_latency` from the default `2` to `1` or `3` did not improve the flower benchmark on this device.
- Async Surface geometry building improves native accounting by moving instance construction off-thread, but it does not improve call wall time and introduces one-frame projected-geometry lag. This makes it unsuitable as a default no-degradation path.
- GPU preproject double buffering barely changes the already-slower GPU preproject path (`54.592ms -> 54.511ms`) and remains slower than the retained CPU instance path.
- The Android normal render loop previously added an unconditional 16ms sleep after every non-benchmark frame. Adaptive sleep is a real interaction fix because it removes that extra delay when the render call already exceeds the target frame interval.
- Tiled compute remains a separate renderer architecture, not a small patch. The current flower benchmark draws every splat (`visible=562974`, `drawn=562974`), so chunk/interval culling cannot honestly explain a performance win for this scene; tiled local sorting would need a new quality-validation track.
- Phase 12 external MCP refresh:
  - `exa` search found WebGPU 3DGS references where static scene data plus sorted ID indirection is the common raster-path pattern, including `Scthe/gaussian-splatting-webgpu` and `vismaychuriwala/WebGPU-Gaussian-Splat-Viewer`. These are useful design references, but repo claims are not local verification evidence.
  - `playcanvas/engine` PR `#8453` describes a mature WebGPU path built from GPU stream compaction, prefix/scatter, indirect draw/dispatch, and radix-sort-indirect. This reinforces that a portable GPU sort win is likely a compacted/indirect pipeline rather than another naive global radix dispatch chain.
  - `KeKsBoTer/wgpu_sort` and `kishimisu/WebGPU-Radix-Sort` show more serious portable radix designs with prefix sums and optional indirect dispatch. Immediate vendoring remains risky because our previous global GPU radix prototype was 1.47s on Android, while subgroup/indirect support and downlevel limits need a dedicated module and validation track.
  - `context7` for `wgpu` confirms the ordinary storage-buffer and bind-group shape for shader-side data reads; no special mobile-only API appeared that would make the static direct-draw path a guaranteed win.
  - `ref` returned relevant `wgpu` examples/spec pointers but no new direct recipe for mobile Gaussian sort/render.
- Mature GPU-sort crate feasibility:
  - `wgpu_sort 0.1.0` is key-value and matches the 3DGS depth/index need conceptually, but it depends on `wgpu 0.19.1`, while this repo is on `wgpu 28.0.0`; immediate use would require a port rather than a dependency drop-in.
  - `wgpu-algorithms 0.1.0` uses `wgpu 28.0.0`, but its public sorter is key-only for returned data/resident buffer and its own README reports CPU wins below 1M items on Apple M3 Max. It would need key-value/order support before it can replace our sorted-index path.
- Local spatial probe for `flowers_1.ply` at a 1080x2400 analysis surface:
  - All 562,974 splat centers are visible and in view for the auto-analysis camera.
  - Uniform 16^3 grid has 1,892 non-empty cells and all 1,892 have visible centers, so chunk culling has no honest no-quality-loss headroom for this benchmark view.
  - Original PLY order is not spatially interval-friendly: grid cell index-span/count ratio is p50 `3609.07`, p90 `50762.60`, p99 `143287.67`, max `249134.00`. PlayCanvas-style interval compaction would need reordered/chunked asset metadata to work well here.
  - Screen-center tile pressure is concentrated: only 95/1024 32x32 tiles receive centers, but those tiles are heavy (p50 `4439`, p90 `12995`, p99 `21973`, max `25951`). This supports tiled compute as a real architecture track, but not a small patch.

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
| Prioritize static GPU scene buffers plus sorted-ID draw indirection next | PlayCanvas's stable raster path avoids per-frame full instance uploads by keeping splat data in persistent GPU resources and using order buffers to choose source IDs at draw time. |
| Retain persistent GPU-source preproject only as opt-in A/B path | It validates the architecture direction without sacrificing output, but measured Android call wall time is slower than the retained CPU path. |
| Retain async sort only as opt-in A/B path | It decouples sorting from the render thread, but current same-device benchmarks show only sub-millisecond call-wall improvement and possible order-lag semantics. |
| Keep tiled local compute as a later architecture track | It directly targets overdraw/present pressure, but is much larger than a safe follow-up and includes optional culling thresholds that would need quality controls and image-diff validation. |
| Retain async geometry only as an opt-in experiment | It keeps full count but has projected-geometry lag and does not reduce Android call wall time. |
| Retain GPU preproject double buffering only as an opt-in experiment | It helps test scheduling but remains slower than the CPU default and has geometry latency. |
| Allocate extra Surface instance buffers lazily | Buffer ring experiments did not justify extra default memory use. |
| Use adaptive normal-mode sleeping | It improves actual app cadence without changing the benchmark path or render output. |
| Re-test static direct draw as opt-in only | It should remove projected-instance upload but repeats projection/covariance work per quad vertex; prior reverted evidence was slow, so it needs a fresh retained A/B toggle rather than replacing the default. |
| Do not vendor a GPU sort crate in Phase 12 | Available crates either do not match current `wgpu` or do not provide key-value order buffers yet; a correct integration would be a dedicated module/port, not a quick dependency addition. |
| Use spatial analysis before chunk/tile renderer work | The flower scene has no chunk-culling headroom in the benchmark view, but concentrated tile pressure makes tiled compute worth a separate quality-validated renderer prototype. |

## Issues Encountered

| Issue | Resolution |
|-------|------------|

## Resources

- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`
- `handbook/ROADMAP.md`
- `handbook/GOLDEN_PRINCIPLES.md`
- `examples/android/README.md`

## Benchmark Evidence

- Baseline command: launch `com.gsplat.example/.MainActivity` with `--ez gsplat_benchmark true --ei gsplat_benchmark_frames 120 --ei gsplat_benchmark_warmup_frames 10 --ef gsplat_benchmark_yaw_step 0.001`.
- Baseline device: `033ed212`, model `A065/Pong`, USB-connected.
- Baseline scene: `flowers_1.ply`, copied to `/data/user/0/com.gsplat.example/files/flowers_1.ply`.
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
- Persistent GPU-source preproject first run: `samples=120 warmup=10 sort_interval=2 avg_call_ms=91.741 avg_frame_ms=12.371 avg_preprocess_ms=2.839 avg_sort_ms=9.531 avg_raster_ms=0.000 avg_visible=562974 avg_drawn=562974`. It preserved output but was much slower, partly because the default shader had been switched to a wider source stride.
- After separating the compact color buffer from the wider GPU preproject source buffer, default CPU path A/B: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false avg_call_ms=51.998 avg_frame_ms=36.521 avg_preprocess_ms=1.514 avg_sort_ms=6.552 avg_raster_ms=28.453 avg_visible=562974 avg_drawn=562974`.
- Same APK after cleanup, GPU preproject enabled: `samples=120 warmup=10 sort_interval=2 gpu_preproject=true avg_call_ms=56.226 avg_frame_ms=10.757 avg_preprocess_ms=2.800 avg_sort_ms=7.957 avg_raster_ms=0.000 avg_visible=562974 avg_drawn=562974`.
- Raising the preproject compute workgroup size from 64 to 128 improved the experiment slightly: `samples=120 warmup=10 sort_interval=2 gpu_preproject=true avg_call_ms=55.352 avg_frame_ms=10.776 avg_preprocess_ms=2.753 avg_sort_ms=8.021 avg_raster_ms=0.000 avg_visible=562974 avg_drawn=562974`.
- Latest same-APK retained default confirmation after the workgroup change: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false avg_call_ms=54.519 avg_frame_ms=35.934 avg_preprocess_ms=1.710 avg_sort_ms=6.386 avg_raster_ms=27.837 avg_visible=562974 avg_drawn=562974`. Earlier same-code default run in the same pass was `avg_call_ms=51.998`, so this appears within device/run variance and still beats GPU preproject on call wall time.
- Same-APK retained default with per-frame sorting after adding the preproject toggle: `samples=120 warmup=10 sort_interval=1 gpu_preproject=false avg_call_ms=52.988 avg_frame_ms=42.336 avg_preprocess_ms=2.804 avg_sort_ms=11.510 avg_raster_ms=28.020 avg_visible=562974 avg_drawn=562974`.
- Normal app mode after the GPU preproject experiment launched successfully and reported `state=rendering`, `visible=562974`, and `drawn=562974/562974`.
- Async sort same-APK default baseline: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false async_sort=false avg_call_ms=52.488 avg_frame_ms=37.419 avg_preprocess_ms=1.831 avg_sort_ms=6.881 avg_raster_ms=28.706 avg_visible=562974 avg_drawn=562974`.
- Async sort enabled with the same retained CPU render path: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false async_sort=true avg_call_ms=51.694 avg_frame_ms=29.851 avg_preprocess_ms=2.596 avg_sort_ms=4.581 avg_raster_ms=29.850 avg_visible=562974 avg_drawn=562974`.
- Per-frame sorting baseline after adding async toggle: `samples=120 warmup=10 sort_interval=1 gpu_preproject=false async_sort=false avg_call_ms=51.667 avg_frame_ms=41.164 avg_preprocess_ms=2.756 avg_sort_ms=10.643 avg_raster_ms=27.763 avg_visible=562974 avg_drawn=562974`.
- Async sort with per-frame scheduling: `samples=120 warmup=10 sort_interval=1 gpu_preproject=false async_sort=true avg_call_ms=51.502 avg_frame_ms=30.454 avg_preprocess_ms=4.673 avg_sort_ms=6.550 avg_raster_ms=30.454 avg_visible=562974 avg_drawn=562974`.
- Async sort plus GPU preproject with `sort_interval=2`: `samples=120 warmup=10 sort_interval=2 gpu_preproject=true async_sort=true avg_call_ms=53.485 avg_frame_ms=0.000 avg_preprocess_ms=2.073 avg_sort_ms=4.801 avg_raster_ms=0.000 avg_visible=562974 avg_drawn=562974`.
- Async sort plus GPU preproject with `sort_interval=1`: `samples=120 warmup=10 sort_interval=1 gpu_preproject=true async_sort=true avg_call_ms=53.613 avg_frame_ms=0.000 avg_preprocess_ms=2.788 avg_sort_ms=7.341 avg_raster_ms=0.000 avg_visible=562974 avg_drawn=562974`.
- Normal app mode after async-sort benchmarks launched successfully and reported `state=rendering`, `visible=562974`, and `drawn=562974/562974`.
- Surface buffer/frame-latency matrix after adding the knobs:
  - Default: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false async_sort=false instance_buffers=1 frame_latency=2 avg_call_ms=53.228 avg_frame_ms=36.100 avg_visible=562974 avg_drawn=562974`.
  - `instance_buffers=2`: `avg_call_ms=53.144 avg_frame_ms=37.072`.
  - `instance_buffers=3`: first run `avg_call_ms=52.732 avg_frame_ms=35.296`, final lazy-allocation confirmation `avg_call_ms=51.802 avg_frame_ms=34.851`.
  - `frame_latency=1`: `avg_call_ms=53.388 avg_frame_ms=37.855`.
  - `frame_latency=3`: `avg_call_ms=53.144 avg_frame_ms=36.377`.
- Async geometry benchmark:
  - Same-APK default: `avg_call_ms=52.148 avg_frame_ms=35.861 avg_visible=562974 avg_drawn=562974`.
  - `async_geometry=true`: `avg_call_ms=52.015 avg_frame_ms=27.158 avg_visible=562974 avg_drawn=562974`.
  - `async_geometry=true instance_buffers=3`: `avg_call_ms=52.132 avg_frame_ms=26.471`.
  - `async_geometry=true sort_interval=1`: `avg_call_ms=52.210 avg_frame_ms=35.300`.
- GPU preproject double-buffer benchmark:
  - Same-APK default CPU path: `avg_call_ms=52.330 avg_frame_ms=36.050`.
  - GPU preproject single-buffer: `avg_call_ms=54.592 avg_frame_ms=10.774`.
  - GPU preproject double-buffer with 2 buffers: `avg_call_ms=54.530 avg_frame_ms=10.722`.
  - GPU preproject double-buffer with 3 buffers: `avg_call_ms=54.511 avg_frame_ms=10.729`.
- Final retained default benchmark after lazy async-geometry creation: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false gpu_preproject_double_buffer=false async_sort=false async_geometry=false instance_buffers=1 frame_latency=2 avg_call_ms=52.491 avg_frame_ms=35.572 avg_preprocess_ms=1.795 avg_sort_ms=7.159 avg_raster_ms=26.617 avg_visible=562974 avg_drawn=562974`.
- Final normal-mode launch after adaptive sleeping: logs reached `frames=100` about 4.8s after the first render status, with `visible=562974`, `drawn=562974/562974`, and render calls around `48ms`; the render loop no longer adds a fixed 16ms after those calls.
- Phase 12 same-APK retained default baseline: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false gpu_preproject_double_buffer=false static_direct=false async_sort=false async_geometry=false instance_buffers=1 frame_latency=2 avg_call_ms=52.801 avg_frame_ms=35.311 avg_preprocess_ms=1.739 avg_sort_ms=6.989 avg_raster_ms=26.582 avg_visible=562974 avg_drawn=562974`.
- Static direct draw opt-in path: `samples=120 warmup=10 sort_interval=2 gpu_preproject=false gpu_preproject_double_buffer=false static_direct=true async_sort=false async_geometry=false instance_buffers=1 frame_latency=2 avg_call_ms=63.271 avg_frame_ms=10.952 avg_preprocess_ms=2.806 avg_sort_ms=8.145 avg_raster_ms=0.000 avg_visible=562974 avg_drawn=562974`. It preserves full output but is slower than default call wall time.

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
- PlayCanvas comparison reinforces the same conclusion: reducing CPU sort alone is insufficient once the frame is dominated by Surface upload/present pacing. The next high-leverage target is to stop uploading full per-splat projected instance data and instead render from static GPU scene/work-buffer data plus a compact order/id buffer.

## PlayCanvas Comparison Notes

| PlayCanvas feature | How it works | Fit for gsplat-rs |
|--------------------|--------------|-------------------|
| Static source data + order buffer | Vertex shader maps draw order to source splat ID through `splatOrder` or `compactedSplatIds`, then reads persistent source streams | High priority; this is the cleanest way to attack the remaining per-frame upload/present bottleneck without reducing splat count |
| Async CPU sort worker | Worker receives camera params, returns latest sorted order, and the renderer applies pending sorted data once per frame | Medium-high priority; port as a Rust worker thread/double-buffered order result if sorting or interaction lock contention resurfaces |
| GPU interval compaction | Cull/count/prefix/scatter over contiguous intervals, then sort/render only compacted visible IDs | Medium priority; needs chunk/octree interval metadata to matter on dense full-scene flower |
| GPU sort raster path | Generate keys indirectly over visible IDs, radix sort compacted IDs, render with sorted-ID storage buffer | Medium priority; our naive global GPU sort failed, so reuse only with a more mature indirect/compacted pipeline |
| Tiled compute rasterizer | Project to cache, bin into tiles, sort locally, blend front-to-back in compute | High potential but large risk; best as a separate prototype after static GPU data and ID draw path |
| Compact/SOG/LOD formats | 20-byte or 32-byte GPU work-buffer formats plus streaming LOD and budgets | Useful long-term, but outside current full-PLY/no-quality-loss benchmark unless introduced as optional asset modes |

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
| Persistent GPU-source preproject as default | Best cleaned-up run was `avg_call_ms=55.352`, slower than the retained CPU path's `51.998` to `54.519` same-pass range | Kept as opt-in A/B path, not default |
| Async sort as default | Best same-APK retained render improvement was `52.488 -> 51.694ms` with `sort_interval=2`; the gain is too small to justify default order-lag semantics | Kept as opt-in A/B path, not default |
| Surface instance buffer ring as default | Best same-APK confirmation was `51.881 -> 51.802ms`, within run noise and not worth extra default memory | Kept lazy/opt-in only |
| `desired_maximum_frame_latency=1` or `3` as default | Neither beat latency `2` in same-APK checks | Kept configurable only |
| Async Surface geometry as default | `52.148 -> 52.015ms` call-wall improvement was noise-level and the path has one-frame geometry lag | Kept as opt-in A/B path, not default |
| GPU preproject double-buffer as default | Best double-buffer run was `54.511ms`, still slower than the retained CPU path | Kept as opt-in A/B path, not default |
| Tiled compute in this patch | Requires a new renderer and image-diff/quality validation; current flower scene has no culling headroom because all 562,974 splats draw | Deferred to a separate architecture track |
| Static direct draw as default | `avg_call_ms=63.271`, slower than the same-APK default `52.801`; the path repeats projection/covariance work per quad vertex | Kept as an opt-in A/B path, not default |
