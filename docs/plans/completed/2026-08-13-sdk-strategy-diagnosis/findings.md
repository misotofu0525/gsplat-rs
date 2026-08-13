# Findings & Decisions: SDK Strategy Diagnosis (2026-08-13)

Status: complete (2026-08-13). Recommendations accepted by the owner; the
direction is recorded in `handbook/ROADMAP.md` (position statement, corrected
GPU-ordering evidence scope, reordered execution sequence, external anchors),
`handbook/GOLDEN_PRINCIPLES.md` (bandwidth-first budgeting, experiment
governance), and `handbook/PROJECT_CONTEXT.md` (current focus).

## Task

Diagnose repository architecture health after the experiment cycles, and
propose a first-principles, research-grounded direction for a distinctive
wgpu-based cross-platform 3DGS SDK: mobile-first performance, Web parity with
PlayCanvas.

## Part 1: Repository Health Assessment

### Verdict

The architecture is not broken. The 2026-08-13 commit `4137634` ("converge
renderer on resident scene path") removed all Packed/Paged runtime code. A
full-source scan confirms:

- No `packed`/`paged`/`atlas` runtime identifiers remain in `.rs`/`.wgsl`.
- No TODO/FIXME/HACK markers in crates; no feature-flag maze.
- One production render path: resident SortedAlpha (GPU-resident scene, CPU
  radix sort, compact `u32` order upload, HW-raster instanced quads).
- Workspace is small (~25k lines total; render crate 6.6k).

### Remaining debt (bounded, addressable in days)

1. `crates/gsplat-render-wgpu/src/lib.rs` is a 3,142-line god-file: public
   API, CPU preprocess/sort scheduling, projection/SH math, resident
   resources, offscreen rasterizer, and ~24 tests in one file.
2. `SurfaceRenderSession` carries a heavy experimental control plane
   (Adaptive CPU<->GPU policy, async sort, bit-packed telemetry) on the
   default path; GPU order and async CPU sort are mutually exclusive and the
   GPU path skips CPU visibility filtering (semantic fork).
3. Dead backends: `GpuOddEvenSortBackend` (gsplat-sort), `RenderMode::SortFree`
   (core, not exposed in C ABI), `GpuInstance`/`build_sorted_instances*` CPU
   geometry expansion (now only a conformance oracle but still public).
4. C ABI scar tissue: documented no-op knobs (`set_gpu_preproject`,
   `set_async_geometry`, `set_instance_buffer_count`) plus an Android-only
   benchmark symbol absent from `gsplat.h`.

### Assets worth protecting

- Single shared `SurfaceRenderSession` across desktop/Android/iOS/Web.
- Verification culture: paired-benchmark artifact contract, PlayCanvas
  competitive harness, dataset ladder tooling, device collectors with thermal
  gating, image SSIM gates. This is rare and is a real moat.
- Small stable C ABI + local AAR / XCFramework / npm packaging slices.
- SPZ v4 bounded loader already implemented (isolated, unconsumed).

## Part 2: First-Principles Cost Model of the Current Path

Mobile GPU scarcity ranking: memory bandwidth (shared LPDDR, ~50-60 GB/s) >
thermal/power > ALU. Current path measured against that:

1. Per-vertex redundancy. `splat_surface_resident.wgsl` `vs_main` runs full
   projection + SH evaluation for each of 6 vertices per splat. Degree-3 SH
   reads 180 B/splat from a storage buffer, nominally x6 per frame. Full-scene
   attribute traffic at 700k splats is ~171 MB/frame (~10 GB/s at 60 Hz)
   before cache effects; ALU for projection/SH is 6x redundant regardless of
   cache. State of the art (PlayCanvas 2.19 compute renderer, WebSplatter,
   wgpu-3dgs-viewer) preprocesses once per splat in compute and keeps VS/FS
   thin.
2. Full-f32 resident storage: ~244 B/splat (64 source + 180 SH + 4 order).
   The single degree-3 SH binding hits the 128 MiB storage-binding limit at
   745,654 splats (retained A065 evidence). Ecosystem norm is quantized GPU
   residency: PlayCanvas keeps SPZ data quantized (~20 B/splat + SH) and
   dequantizes in-shader.
3. Sorting: CPU radix costs ~13 ms at 700k on Adreno 730 (retained ladder).
   The 2026-07-22 GPU-vs-CPU experiment conclusion ("GPU slower everywhere")
   reflects the tested baseline, whose global prefix pass is a single
   workgroup serially scanning tile histograms, and which sorts the full
   scene with no visibility compaction. It is not evidence that portable GPU
   sorting loses on mobile (see PlayCanvas data below).
4. No visibility compaction: draw count equals scene count; invalid splats
   still cost 6 vertex invocations.
5. Whole-scene residency, no LOD/streaming: capacity failures are explicit
   (good) but real scenes (1M-6M) simply do not load on mobile.

Ordered first-order waste: (a) data representation bytes, (b) 6x vertex
redundancy, (c) sort-all-every-refresh, (d) no compaction, (e) no LOD.

## Part 3: External Research Summary

### Competitor baseline: PlayCanvas (June 2026)

- Engine 2.19 ships a compute WebGPU gsplat renderer: GPU frustum culling
  (octree node visibility), stream compaction (flag/prefix-sum/scatter),
  GPU radix sort, indirect draw; pre-projected data feeds lightweight VS/FS.
  Reported 2.6x at 10M splats, 5.7x at 35M vs their WebGL2 path.
  https://blog.playcanvas.com/new-in-supersplat-webgpu-and-streaming-bring-huge-performance-wins/
- Streamed SOG: `lod-meta.json` binary spatial tree + SOG chunks (WebP
  textures, Morton order), automatic LOD generation via open-source
  splat-transform, device-tuned global gaussian budget.
  https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/streamed-sog/
- Sort portability (PR #8620, 2026-04): the portable winner across Apple
  M1/M2, iPhone 13 Pro, Pixel 8 Pro is a 4-bit multi-pass radix sort with
  hierarchical scan (no subgroups required). OneSweep (8-bit, decoupled
  lookback) is NVIDIA-only; it fails/serializes on Apple/Mali/Adreno due to
  missing forward-progress guarantees. Subgroup-based 8-bit variants win only
  on Apple M4. https://github.com/playcanvas/engine/pull/8620
- Their WebGL2 CPU-worker sort path remains the full-feature fallback and
  renders identically.

### Mobile-focused papers (2025-2026)

- WebSplatter (2026-02, arXiv 2602.03207): WebGPU hybrid pipeline; wait-free
  hierarchical radix sort (no global atomics), opacity-aware culling and
  quad sizing; hardware raster beats tile-based compute on bandwidth-limited
  devices; 1.2-4.5x over prior web viewers, far lower VRAM.
- Seele (CVPR 2026): hybrid preprocessing (view-dependent clusters + online
  filtering) and contribution-aware rasterization; up to 6.3x on mobile;
  needs light fine-tuning of assets.
- Texture3dgs (arXiv 2511.16298): texture-cache-optimized mobile sorting,
  up to 4.1x sort speedup; beats VkRadixSort by 1.10-1.15x.
- Neo (ASPLOS 2026): reuse-and-update sorting exploiting temporal coherence;
  94.6% DRAM traffic reduction (HW accelerator, but the incremental-resort
  idea transfers to software).
- StreamingGS (arXiv 2506.09070): hierarchical two-phase attribute fetch
  (coarse cull reads only position+scale, survivors read the rest).
- Voyager (arXiv 2506.02774): temporal-aware LOD search for city-scale on
  mobile; preemptive alpha filtering; exp() LUT.
- Sort-free family: Weighted Sum Rendering (ICLR 2025, 1.23x on Snapdragon 8
  Gen 3, Vulkan), StochasticSplats (ICCV 2025), Mobile-GS (2026, 116 FPS
  Bicycle on 8 Gen 3), Duplex-GS (2025, cell-proxy hybrid WSR). All require
  retraining or change image semantics; unsuitable as the default path of an
  ecosystem-compatible SDK, viable later as an opt-in pipeline for assets
  trained for it.
- LOD/large-scene: Hierarchical 3DGS, Octree-GS, LODGE (2505.23158), FilterGS
  (CVPR 2026), V3DG (SIGGRAPH 2025, Nanite-style clusters), Virtual Memory
  for 3DGS (2506.19415).

### Format landscape (2026)

- SPZ v4 (Niantic, 2026-05): parallel ZSTD streams, 32-byte plaintext header,
  vendor extensions, SH degree 4, ~10x smaller than PLY; adopted by Adobe
  Photoshop (~800k files in two months), Babylon.js; de-facto interchange
  format. gsplat-rs already has a bounded SPZ v4 loader. 
  https://www.nianticspatial.com/blog/spz4
- SOG / Streamed SOG: PlayCanvas's recommended web delivery format.
- KHR_gaussian_splatting glTF extension: release candidate Feb 2026,
  ratification ~Q2 2026 (Google/NVIDIA/Apple/Bentley backing); an SPZ
  streaming extension is planned. 3D Tiles 2.0 adds GS tiles (Cesium/DJI).
- Practical read: consume PLY + SPZ (+ SOG later); watch KHR extension; do
  not invent a proprietary format.

### Competitive gap (the distinctive position)

No credible cross-platform native 3DGS SDK exists: MetalSplatter is
Apple-only; Unity plugins are heavy and mobile-flaky; PlayCanvas is Web-only;
Meta Spatial SDK is Quest-only; Brush focuses on training; wgpu-3dgs-viewer
exposes raw wgpu types, no mobile packaging, no streaming. gsplat-rs's
"one Rust/wgpu core, C ABI, AAR + XCFramework + npm" shape is a real gap.
Nobody ships thermal/power-aware adaptive quality on mobile.

## Part 4: Recommendations

### Strategy statement

"The embeddable, mobile-first Gaussian splat renderer": one small core that
drops into any iOS/Android/Web app; SPZ-native; best-in-class on phones
(bytes, frame time, battery); Web at PlayCanvas rendering parity for matched
scenes (renderer-vs-renderer, not engine-vs-engine).

### Revised execution sequence

- Phase 0 (days): pay down debt. Split `lib.rs` (math / preprocess /
  resident resources / offscreen). Delete `GpuOddEvenSortBackend`,
  `RenderMode::SortFree`; demote `GpuInstance` expansion to test-only.
  Isolate Adaptive/async control plane from the default frame path.
- Phase 1 (highest leverage): change the data plane before the sort.
  1. Quantized resident profile (SPZ-aligned: f16/fixed positions,
     smallest-three rotation, log-u8 scale, u8-quantized SH sidecar split by
     degree), decoded in-shader; target <=32 B hot record + SH sidecar,
     >=1M splats on the A065-class 128 MiB binding budget; SSIM gates vs
     full-f32 reference.
  2. Per-splat compute preprocess pass writing compact projected records;
     thin VS/FS consumes them (removes the 6x projection/SH redundancy).
  Keep CPU sorting unchanged through this phase; it composes.
- Phase 2: visibility compaction + portable GPU sort + indirect draw
  (existing ROADMAP items 1-2, with corrections): hierarchical prefix scan
  (fix the serial single-workgroup scan), sort only the compacted visible
  set, 4-bit multi-pass design validated by PlayCanvas on mobile SoCs; CPU
  sort stays the deterministic fallback. Re-run the retained Adreno ladder;
  expect the 2026-07-22 conclusion to flip once preprocessing/compaction and
  a proper scan exist.
- Phase 3: streaming + LOD aligned with the ecosystem: promote SPZ v4 into
  the product surface (C ABI scene-from-memory + mobile/Web loaders); add
  Streamed SOG read support so splat-transform assets stream directly;
  bounded residency with metadata-first design (honoring the retired-paging
  lessons: independent source/CPU/GPU budgets, no hidden fallback).
- Phase 4: mobile-only differentiators: thermal/power-aware quality
  governor (resolution scale, SH degree clamp, sort cadence) driven by
  platform thermal APIs + frame telemetry; AR session integration samples;
  battery benchmarks in the artifact contract.

### Web parity note

Parity target should be renderer-vs-renderer on matched scenes via the
existing `tests/competitive/playcanvas` harness. The wasm/WebGPU path reuses
the same compute preprocess + GPU sort as native (wgpu compiles it to WGSL
either way); the current CPU-sort path remains the fallback where WebGPU is
absent. Do not chase PlayCanvas engine/editor breadth.

### Experiment governance (to prevent repeat damage)

- Experiments must attach at stage interfaces (preprocess / order / draw),
  never as parallel geometry paths or public selectors.
- Every experiment defines kill criteria and an evidence budget before
  implementation begins (the Packed/Paged cycle lacked the former).
- Keep "evidence before promotion"; add "delete on demotion" (the cleanup
  this morning should be the norm, not a special task).

## Resources

- PlayCanvas WebGPU/Streaming announcement:
  https://blog.playcanvas.com/new-in-supersplat-webgpu-and-streaming-bring-huge-performance-wins/
- PlayCanvas radix sort portability data: https://github.com/playcanvas/engine/pull/8620
- PlayCanvas GPU culling/compaction pipeline: https://github.com/playcanvas/engine/pull/8453
- Streamed SOG spec: https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/streamed-sog/
- SPZ v4: https://www.nianticspatial.com/blog/spz4 and https://github.com/nianticlabs/spz
- WebSplatter: https://doi.org/10.48550/arxiv.2602.03207
- Seele (CVPR 2026): https://openaccess.thecvf.com/content/CVPR2026/papers/Zhu_Seele_A_Unified_Acceleration_Framework_for_Real-Time_Gaussian_Splatting_on_CVPR_2026_paper.pdf
- Texture3dgs: https://arxiv.org/html/2511.16298
- Neo (ASPLOS 2026): https://dl.acm.org/doi/10.1145/3779212.3790192
- StreamingGS: https://arxiv.org/html/2506.09070
- Voyager: https://arxiv.org/html/2506.02774
- Sort-free WSR (ICLR 2025): https://arxiv.org/html/2410.18931
- StopThePop: https://doi.org/10.1145/3658187
- StochasticSplats (ICCV 2025): https://openaccess.thecvf.com/content/ICCV2025/html/Kheradmand_StochasticSplats_Stochastic_Rasterization_for_Sorting-Free_3D_Gaussian_Splatting_ICCV_2025_paper.html
- Mobile-GS: https://arxiv.org/html/2603.11531v1
- LODGE: https://arxiv.org/abs/2505.23158v1
- FilterGS (CVPR 2026): https://openaccess.thecvf.com/content/CVPR2026/html/Wang_FilterGS_Traversal-Free_Parallel_Filtering_and_Adaptive_Shrinking_for_Large-Scale_LoD_CVPR_2026_paper.html
- V3DG (SIGGRAPH 2025): https://xijie-yang.github.io/V3DG/
- Virtual Memory for 3DGS: https://arxiv.org/html/2506.19415v1
- KHR_gaussian_splatting status: https://www.thefuture3d.com/blog/state-of-gaussian-splatting-2026/
- WebGPU subgroups availability: https://web-platform-dx.github.io/web-features-explorer/features/webgpu-subgroups/
- Brush: https://github.com/ArthurBrussee/brush
- wgpu-3dgs-viewer: https://github.com/LioQing/wgpu-3dgs-viewer
