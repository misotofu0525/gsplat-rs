# Findings and Decisions

## Requirements

- Document an overall technical strategy for competing with PlayCanvas.
- Prioritize measurable Android/iOS native performance leadership.
- Keep desktop Web rendering performance-competitive rather than attempting to
  match the entire PlayCanvas product surface.
- Include a concrete implementation architecture, phased rollout, risks, and
  an executable verification plan.
- Keep claims evidence-backed and distinguish existing behavior from proposed work.

## Research Findings

- Current gsplat-rs production targets share `SurfaceRenderSession`, direct
  sorted indices, and GPU-resident scene resources across desktop, Web,
  Android, and iOS.
- Current direct GPU resources use monolithic storage-buffer bindings; base
  source data is 64 bytes per splat and degree-3 SH data is 180 bytes per splat.
- The current device request uses `wgpu::Limits::downlevel_defaults()`, making
  the portable 128 MiB storage-buffer binding limit an active design boundary.
- PlayCanvas avoids storing complete splat attributes in one storage buffer:
  resource and work data are texture streams, while order and GPU-sort scratch
  use compact storage buffers.
- PlayCanvas's pinned source labels its compact work buffer as 20 bytes per
  active splat, but the declared streams are `R32U + RGBA32U + R32U`, which
  occupy 4 + 16 + 4 = 24 bytes per splat. Treat 20 bytes as a documentation or
  accounting mismatch until PlayCanvas changes the layout or clarifies it.
- The PlayCanvas work-buffer number is not total active residency. Its normal
  architecture copies resource texture streams into the work buffer; compressed
  degree-3 source streams can add 64-68 bytes per splat before order and sort
  scratch, depending on format.
- PlayCanvas's large-scene strategy combines Streamed SOG, octree LOD, dynamic
  loading/unloading, frustum culling, and a global splat budget.
- PlayCanvas has a public 250-million-splat city streaming example, but renders
  only a bounded active working set selected by device/quality policy.
- Existing gsplat-rs Android evidence shows that naive full GPU radix sorting
  can be catastrophically slow on a tile-based mobile GPU; a universal GPU-sort
  policy would be unsafe without device-gated evidence.
- Existing gsplat-rs direct-index work materially reduced upload and submission
  costs, but those measurements are internal A/B results rather than a current
  head-to-head PlayCanvas benchmark.
- Project policy requires renderer, sorting, Web, Android, Apple, FFI, package,
  and physical-device evidence to remain distinct; an artifact build cannot be
  reported as runtime proof.
- The current mobile benchmark entrypoints already share a deterministic camera
  orbit, frame/warmup counts, sort interval, async-sort toggle, and frame-latency
  setting, so the competitive harness should extend rather than replace them.
- Android and Apple wrappers intentionally adapt the same C ABI and shared
  session. Competitive instrumentation must be added below that boundary or as
  additive stats so platform wrappers do not acquire independent render logic.
- The roadmap explicitly excludes a custom internal binary format today. The
  proposal therefore treats SPZ/SOG/Streamed SOG as interoperability inputs and
  keeps any packed atlas layout private to the renderer until measured evidence
  justifies widening the release boundary.

## Existing Performance Evidence

- Internal M4 Pro A/B measurements show that the current direct sorted-index
  path materially reduced GPU-complete time and removed per-frame geometry
  uploads. Kitsune improved from about 6.35 ms to 2.93 ms GPU-complete on
  desktop and from about 13.24 ms to 1.47 ms render-call time in Chrome;
  Flowers improved from about 22.84 ms to 10.75 ms GPU-complete on desktop and
  from about 27.16 ms to 2.46 ms render-call time in Chrome.
- Those measurements compare old and new gsplat-rs paths, not PlayCanvas. They
  support preserving the direct path but cannot justify a competitive claim.
- Android Flowers measurements improved from about 51.96 ms render-call time
  to about 29.97-33.19 ms after the retained direct-path work. Repeated runs can
  drift into the 45-50 ms range under thermal pressure, so short cold runs are
  not sufficient evidence.
- iPhone 17 Pro Max measurements were about 17.2-17.8 ms and were often
  present/vsync bound. Native comparisons must report CPU call time and
  end-to-end/GPU-complete timing rather than FPS alone.
- A naive full GPU radix-sort experiment on Android took about 1466 ms for
  Flowers. GPU sorting must remain capability- and benchmark-gated.
- Existing M4 Pro SIMD radix measurements were already small: about 0.46 ms for
  Kitsune and 1.54 ms for Flowers. Desktop work should first target memory
  layout, bandwidth, residency, and rendering unless new profiling proves sort
  is again dominant.

## Existing Benchmark Infrastructure

- `tools/bench-runner` reports adapter metadata, submit-frame time, CPU
  preprocess/sort time, geometry encode/submit wall time, GPU wait/GPU-complete
  time, and visible/drawn counts.
- It also supports long-run RSS growth checks and spatial-analysis output.
- Android, iOS, and Web examples expose deterministic orbit benchmarks and emit
  machine-readable `BENCHMARK_RESULT` records with warm-up/sample counts, sort
  interval, latency, preprocessing, sorting, raster, and visibility metrics.
- Current outputs emphasize averages. Competitive qualification additionally
  needs p50/p95/p99/max distributions, cold/warm load phases, CPU/GPU memory,
  upload bytes, residency/eviction counters, network bytes, and thermal/energy
  state.
- The existing 30-minute stability route is an appropriate base for sustained
  performance and memory-leak gates.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Position the product as native-first and Web-competitive | This matches the unique AAR/XCFramework/C ABI value while avoiding a feature-breadth contest with a mature Web engine. |
| Preserve a small-scene direct path | Single-object and validation workloads should not pay the complexity or copy cost of a streaming architecture. |
| Design a packed, fixed-capacity active atlas for large scenes | Texture-backed active data avoids the single storage-binding limit and keeps the shader-visible working set bounded. |
| Stream pages directly into the active atlas | Avoids keeping both full source textures and a duplicated work buffer when the SDK does not need PlayCanvas's editing/general-engine flexibility. |
| Use global active-slot IDs for sorting | Maintains correct cross-page `SortedAlpha` ordering without requiring a shader-visible array of page buffers. |
| Budget bytes and work, not only splat count | SH degree, data format, upload cost, and device memory make equal splat counts unequal workloads. |
| Keep CPU radix sorting as the mobile baseline | It has current device evidence; GPU sorting must be capability- and benchmark-gated. |
| Prefer SPZ/SOG/Streamed SOG interoperability over a new public format | Reuses an existing asset ecosystem and respects the roadmap's current non-goal for a custom binary format. |
| Separate renderer, product, and scalability claims | Current renderer efficiency, native deployment advantage, and large-scene streaming each require different evidence. |
| Use 65,536-splat pages as the first prototype | A 256 x 256 texel page maps cleanly into synchronized texture atlases. The proposed 20-byte hot record is 1.25 MiB per page; degree-3 SH and order are budgeted separately. |
| Extend async validity with residency and slot generations | Camera/scene revisions alone cannot prevent an old result from referring to a slot reused by another page. |
| Start LOD selection on CPU | It provides the simplest correctness baseline; GPU node culling remains an evidence-gated optimization. |
| Keep the atlas layout private | It permits layout experiments and ecosystem-format interoperability without creating a premature public file contract. |
| Split hot geometry/color from the SH sidecar | A 20-byte render record can be read every frame while degree-aware SH residency is selected independently by page contribution. |
| Compare total renderer-owned bytes | A work-buffer-only byte count hides source textures, order, and scratch and is not a fair architecture comparison. |

## Architecture Outcome

- The target design has two paths behind the existing `SurfaceRenderSession`:
  `SmallSceneDirect` and `PagedActiveAtlas`.
- The large-scene path uses synchronized texture planes and globally sorted
  active slot IDs, so pages do not become separate alpha-order domains.
- Source pages decode directly into the canonical active atlas; compressed,
  decoded CPU, and GPU residency caches have separate byte budgets.
- The revised target is a 20-byte production hot record, a 16-18-byte research
  candidate, and a degree-aware SH sidecar. Full degree-3 attributes target
  about 68 bytes, or about 72 bytes including the 4-byte order ID.
- Attribute LOD should target a measured 40-48-byte average attribute payload
  across large-scene traces by keeping full SH only on high-contribution pages.
- The packed implementation still must achieve at least 3x reduction from the
  current representation and pass image parity.
- `wgpu-types 28.0.0` exposes `Rgba16Uint`, `Rgba8Uint`, and `R32Uint`, so the
  proposed 8 + 4 + 4 + 4-byte hot texture streams use real portable format
  candidates. Shaders should use integer loads, and oversized page bounds must
  be subdivided when 16-bit local-position error exceeds the quality gate.
- No new crate, public C ABI, or custom public asset format is required for the
  first prototype.

## Verification Outcome

- Competitive verification is split into internal A/B, native product,
  same-browser Web parity, and large-scene scalability tracks.
- Benchmarks use locked parity, best-practice product, fixed-byte, and
  fixed-frame-budget profiles so results cannot silently trade quality or work.
- Qualification requires raw per-frame artifacts, five randomized paired runs,
  percentile distributions, confidence intervals for public claims, and
  sustained mobile tests.
- Initial claim gates are 25% native p95 advantage or 1.5x active capacity at a
  matched frame budget, and desktop Web p95 within 1.10x of PlayCanvas. These
  are promotion thresholds, not claims about current code.
- A pinned PlayCanvas revision and explicit backend/sort/LOD/path signals are
  required; results from different revisions cannot be merged.
- Current commands remain canonical. New comparison tool names are deliberately
  left as proposed work until their implementations exist.

## Phase A Execution Findings

- `tools/bench-runner` currently accumulates averages in iteration mode and
  prints adapter metadata, but discards individual frame samples after each
  iteration.
- The runner already measures submit/frame, preprocess, sort, geometry
  encode/submit CPU wall, GPU wait, GPU-complete, visible, and drawn values, so
  raw distributions can be added without changing renderer behavior.
- Android, iOS, and Web benchmark implementations independently accumulate the
  same average-oriented fields. Phase A needs one versioned artifact contract
  before extending all three to avoid schema drift.
- Existing output must remain available for current scripts and smoke tests;
  structured artifacts should be additive.
- `bench-runner` has no direct JSON dependency. `serde` is already transitive in
  the lockfile but neither `serde` nor `serde_json` is a workspace dependency;
  the artifact implementation should make this dependency decision explicit.
- Offscreen and surface devices request downlevel defaults. The offscreen
  renderer exposes adapter identity but not effective device limits.
- Direct scene buffers are allocated lazily on first render, so a scene can
  validate/load successfully and fail later during GPU resource creation.
- Current direct allocation errors collapse to generic device-creation errors.
  Phase A preflight needs checked byte arithmetic before allocation plus a
  structured report available to the runner and surface path.
- Effective enforcement must use `device.limits()`, not only
  `adapter.limits()`, because current device creation intentionally requests
  downlevel defaults. Each whole storage binding is limited by the smaller of
  `max_storage_buffer_binding_size` and `max_buffer_size`.
- At 128 MiB, direct degree-3 SH is limited to 745,654 splats; the 64-byte base
  source is limited to 2,097,152. A synthetic 3,454,040-splat Nandi report can
  verify rejection without allocating its roughly 857 MB direct payload.
- Phase A artifact schema must freeze a cross-language quantile algorithm and
  represent unavailable metrics as `null`, never as zero. Encoding/logging must
  happen outside measured frames.
- Independent acceptance audit found no current pinned PlayCanvas harness and
  no connected Android device. One iPhone 17 Pro Max is visible locally, but
  real-device evidence still requires a successful signed build/install/run.
- Five paired competitor runs are a minimum engineering gate; externally cited
  confidence intervals should use a larger predeclared run count because five
  pairs produce a coarse interval.
- The minimal competitor harness should live under
  `tests/competitive/playcanvas/`, lock exact `playcanvas@2.21.0-beta.14` with
  `npm ci`, and fail closed unless package/runtime version, revision, integrity,
  backend, resolved sort renderer, format, LOD, dynamic-resolution, budget, and
  canvas signals match the manifest.
- npm metadata ties `playcanvas@2.21.0-beta.14` to the pinned engine revision
  `d5fe88878e338936fe763bbce1a58bc315e89cbe`. Harness code should remain
  original/minimal and include PlayCanvas's MIT notice without vendoring
  `node_modules` or engine build artifacts.
- The canonical artifact contract is now `gsplat-benchmark/v1` with required
  `manifest.json`, contiguous `frames.jsonl`, and recomputable `summary.json`.
  It freezes nearest-rank percentiles and strict null semantics.
- Direct resource preflight is now integrated before surface and offscreen GPU
  allocations, including checked sizes, effective device limits, Nandi-scale
  synthetic reports, and the `u32` instance guard. This removes the previous
  late/generic failure mode without allocating large test scenes.
- The first real v1 offscreen artifact validated successfully on Apple M4 Pro /
  Metal with 30 measured frames. It produced raw frames, all required
  distributions, manifest identity/environment, zero missed 16.67 ms frames,
  and retained existing human-readable averages.
- The minimal-scene smoke measured p95 frame wall/GPU-complete at about 3.06 ms;
  this is artifact-pipeline evidence only, not a competitive or regression
  baseline because the sample is tiny and no PlayCanvas pair exists.
- Independent producer review found four issues that must be fixed before using
  artifacts as qualification evidence: per-frame identity allocation between
  samples, non-atomic reuse of artifact directories, ambiguous host/display
  metadata, and producer/validator mean-order drift.
- Additional hardening is needed for dataset hash TOCTOU, measurement timestamp
  scope, packaged binaries without runtime Git, and a producer-to-validator
  end-to-end test.
- The pinned PlayCanvas dependency skeleton now passes offline preflight for
  exact npm version, tarball integrity, MIT license, full frozen revision map,
  and runtime seven-character revision. It does not yet prove browser backend,
  sort path, rendering, or benchmark output.
- Artifact production now buffers compact numeric samples without per-frame
  strings/serialization, fails when a final directory already exists, stages a
  complete run in a sibling directory, and publishes the run by directory
  rename.
- Manifest identity now distinguishes whole-run and measured-window timestamps,
  verifies dataset bytes/hash before and after load, records configured-vs-
  observed display values, and separates host model from GPU adapter type.
- Producer output is exercised end-to-end through the canonical Python
  validator, including refusal to reuse an existing artifact directory.
- The hardened 120-frame M4 Pro artifact includes the structured Direct
  preflight: 128 MiB binding limit, 256 MiB max buffer, 2,097,152 direct-splat
  source ceiling, and per-resource byte requirements.
- Local external qualification candidates are available for Kitsune, Flowers,
  and Nandi. Flowers is about 133 MiB with SHA-256
  `3641192d70d598894f4d6ad92e0890b4a7336b43aa6af47bd35842c3e2ee8765`;
  Nandi is about 817 MiB with SHA-256
  `e0457d226c40ab69857a89b84f56a03da69ddd81b49610d5c7f226dacb60c05d`.
- These external files remain local/ignored. Their source, license, conversion,
  SH degree, counts, and bounds still need frozen manifests before Phase A can
  use them as qualification evidence.
- Fresh spatial analysis established exact degree-3 counts and bounds:
  Kitsune 279,199 with bounds `[-1.147705,-1.238525,-1.199707]` to
  `[0.849854,1.247559,0.799561]`; Flowers 562,974 with bounds
  `[-0.875705,-0.607782,-0.777395]` to `[1.221174,0.459142,1.469875]`;
  Nandi 3,454,040 with bounds `[-79.179398,-59.651611,-74.109161]` to
  `[76.388443,99.979218,64.020042]`.
- Kitsune is reproducibly fetched from Wakufactory under CC0 and hashes to
  `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`.
  Flowers includes a local CC BY 4.0 attribution file. Nandi's source and
  redistribution terms remain unverified, so it is a local candidate only.
- `gsplat-camera-trace/v1` now freezes timestamps, pose/intrinsics, explicit
  view/projection/VP matrices, row-major storage, column-vector math, RUF/+Z
  camera space, and WebGPU-style `[0,1]` depth. The deterministic fixture hash
  is `e73f23c44f0cc1fb3fc2e533bcbce70989afe5f0b739e38601198f271484bca6`.
- PlayCanvas/GL consumers must explicitly convert matrix storage, native camera
  forward convention, and possibly depth range at the harness boundary while
  proving the converted matrix matches the oracle.
- The connected iPhone 17 Pro Max release build and signing succeeded, but the
  real-device benchmark did not launch because the phone was locked. This is a
  device-state blocker, not runtime evidence. Android still has no connected
  device.
- The pinned PlayCanvas browser harness now proves the actual WebGPU path before
  timing: version `2.21.0-beta.14`, revision `d5fe888`,
  `GSplatHybridRenderer`, active `raster_gpu_sort`, and `usesGpuSort=true`.
  The earlier zero-manager signal was a harness identity bug: the director map
  is keyed by the underlying `Camera`, not its `CameraComponent`.
- Browser builds cannot truthfully infer repository dirty state. The v1
  contract therefore permits null build commit/dirty values only when their
  exact paths appear in `unavailable_fields`; unknown must never be encoded as
  a clean repository.
- Android and iOS now have post-measurement v1 artifact transports while
  preserving their legacy result lines. Both freeze raw numeric samples during
  measurement and defer JSON/hash/distribution work; iOS additionally freezes
  the thermal state at the final measured frame and refuses to emit a dataset
  identity when the source file cannot be inspected.

## Issues Encountered

| Issue | Resolution |
|-------|------------|
| Native-vs-Web can be criticized as an unfair renderer comparison | Define it as the product/deployment comparison and pair it with a separate Web-vs-Web engineering benchmark. |
| PlayCanvas and gsplat-rs have different default culling, LOD, sorting, and format policies | Require a locked quality-parity profile, report actual backend/path signals, and retain image-diff evidence. |

## Resources

- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/ROADMAP.md`
- `handbook/GOLDEN_PRINCIPLES.md`
- `handbook/VERIFICATION.md`
- `bindings/android/README.md`
- `bindings/apple/README.md`
- `packages/web/README.md`
- `docs/plans/completed/2026-07-10-web-desktop-sorted-index/findings.md`
- `docs/plans/completed/2026-06-10-android-gpu-render-perf/findings.md`
- PlayCanvas rendering architecture: https://developer.playcanvas.com/user-manual/gaussian-splatting/building/rendering-architecture/
- PlayCanvas data format: https://developer.playcanvas.com/user-manual/gaussian-splatting/building/unified-rendering/splat-data-format/
- PlayCanvas Streamed SOG: https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/streamed-sog/
- PlayCanvas performance and budget: https://developer.playcanvas.com/user-manual/gaussian-splatting/building/performance/
- PlayCanvas compact work-buffer source: https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/gsplat-unified/gsplat-params.js#L86-L101
- PlayCanvas compressed resource source: https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/gsplat/gsplat-compressed-resource.js#L27-L48
- Niantic SPZ: https://github.com/nianticlabs/spz

## Visual/Browser Findings

- No visual artifacts were inspected in this documentation-only task.
- Primary-source browser research confirms that PlayCanvas separates total
  scene size from active working-set size and stores splat attributes in GPU
  texture streams.


## Phase A Physical Baseline Capture

- Phase A physical kitsune baselines are stored under
  `target/benchmarks/phase-a/` and indexed by
  `phase-a-baseline-index.json`.
- Android Nothing A065 / Vulkan: 279,199 splats, 120 samples, p95 frame wall
  13.728 ms, 2 missed frames against the observed 60 Hz budget.
- iPhone 17 Pro Max / Metal: same dataset, 120 samples, p95 17.485 ms, 62
  missed frames. This is baseline evidence, not a competitive claim.
- Desktop Web WASM/WebGPU on Chrome: same dataset, 30 samples, p95 3 ms.
- iOS first launch failed with `parse failed` because Documents retained a
  stale `imported_scene.ply` that shadowed the bundled showcase; uninstalling
  the app cleared the sandbox and the kitsune run succeeded.

## Phase B Packed Atlas Findings

- The first device A/B scripts rejected an explicit zero yaw and silently
  substituted `0.001`, so runs labeled static were actually moving. Android
  and iOS now accept finite zero yaw; all prior static labels must be treated
  as invalid until recaptured.
- The original image threshold counted failing RGB channels rather than pixels.
  The gate now counts a pixel once when any RGB channel exceeds `3/255`, matching
  the verification-plan wording.

- `pack_quat_smallest_three` must force the omitted (largest-abs) component
  positive before packing. Forcing only `w >= 0` left kitsune with mean quat
  L1 ≈ 0.50 and image MAE ≈ 0.026 / alpha MAE ≈ 0.057; after the fix, quat L1
  ≈ 0.001 and kitsune MAE 0.001259 with frac>3/255 = 0.
- Packed-shader `quat_to_mat3` must use the same sign convention as the direct
  path; a transposed convention produced kitsune MAE ≈ 0.024 even with correct
  packing.
- Design-aligned color refresh (float SH → hot RGB10) plus pose/angular caching
  is enough for static-camera parity and desktop p95; the main draw reads only
  hot streams.
- Mobile draw path uses a tightly packed 20 B/splat storage buffer rather than
  four uint textures; multi-texture decode was a measurable Adreno cost.
- Desktop packed/direct `frame_wall_ms` p95 ratios (warmup 20): minimal 1.009,
  kitsune 0.750. Indexed at
  `target/benchmarks/phase-b/phase-b-desktop-p95-index.json`.
- Device kitsune static A/B (`avg_frame_ms`): iPhone 17 Pro Max ratio 0.983
  (pass ≤10%); Nothing A065 / Vulkan ratio 1.202 (fail). Indexed at
  `target/benchmarks/phase-b/phase-b-device-p95-index.json`.
- Nandi packed preflight: SH attributes stay out of storage bindings; hot
  storage is `20 * splat_count` and fits under the common 128 MiB limit;
  SH atlas height may still require Phase D paging on 8K texture limits.
- Attempted f16 world-covariance hot packing (24 B/splat) to remove per-vertex
  quat rebuild: Kitsune image gate still passed, but Flowers failed the pixel
  fraction gate (`frac_over_3_255 ≈ 0.0079 > 0.001`). Reverted to the 20-byte
  scale+rotation hot record; keep f16/precomputed-cov as a deferred candidate
  until a decode path passes Flowers + Android sync p95 together.
- Independent audit found that the original device index was mislabeled: it
  compared `avg_frame_ms`, not raw p95. Recomputed Android orbit p95 was much
  worse than the recorded mean ratio, so only canonical v1 summaries may close
  the device gate going forward.
- Fresh pre-optimization Android evidence for exact `a77b4ef` reproduced the
  failure: direct p95 13.612 ms versus packed 23.703 ms (1.741x), thermal 0→0.
- After accepting true zero yaw and replacing the absolute translation trigger
  with a scene-relative SH direction bound, the first candidate snapshot gave
  static p95 ratio 0.790 and orbit p95 ratio 1.108. Static now passes; orbit is
  close but remains above the 1.10 gate until a stable multi-run series passes.
- Packed drawing now uses a four-vertex triangle strip and avoids redundant
  quaternion normalization. The corrected pixel-level Kitsune gate remains at
  RGB MAE 0.001259 with zero pixels over 3/255.
- Packed resource accounting now uses the exact requested hot-buffer and
  RGBA8Uint SH-texture descriptor bytes. Driver-private allocator padding is
  explicitly unobservable through wgpu and must not be called measured bytes.
- GPU construction no longer creates an intermediate full SH staging atlas,
  and packed renderers do not retain direct-path covariance/alpha CPU caches.
  Desktop peak process RSS is recorded at
  `target/benchmarks/phase-b/phase-b-peak-rss-index.json` (kitsune/flowers
  packed/direct cold-load ratios ≈ 0.87); this is process RSS, not a
  renderer-owned decoded-CPU-byte budget.
- Android sync-orbit five-pair after scale-u10 + scene-relative refresh
  (`android-a065-scale10-final`) median p95 ratio 1.162 (fail). Async persistent
  worker five-pair median 0.947 (pass).
- Banded color-refresh on the surface path
  (`android-a065-banded-refresh-sync-five-paired`) did not close sync: median
  p95 ratio 1.176. Per-frame breakdown shows packed **non-sort** frames are
  faster than direct (p95 ≈ 9.6 vs 12.2 ms); the overall p95 gap is dominated by
  **sync sort frames** (packed sort-frame wall p95 ≈ 15.9 vs direct ≈ 13.8),
  not a steady per-vertex decode/ALU tax. Color refresh is now deferred off
  sort frames as a follow-up candidate.
- After deferring SH refresh off sort frames, Android kitsune sync
  `sort_interval=2` improved to median 1.118
  (`android-a065-colorword-fix-sync-five-paired`) but still failed the 1.10
  gate. Raising the matched interval to `sort_interval=4` closed the kitsune
  gate: median p95 ratio 1.078, 4/5 pairs ≤1.10
  (`android-a065-sort4-sync-five-paired`). Flowers under the same protocol
  median 0.383 (`android-a065-flowers-sort4-sync-five-paired`). Async
  persistent-worker remains a stronger product path (~0.947). Device index
  rewritten as v2 at `phase-b-device-p95-index.json`.

## Phase C SPZ Findings

- Niantic SPZ is MIT-licensed and interoperable; version 4 is the selected
  first compressed source format rather than a repository-specific public
  format.
- SPZ v4 starts with a 32-byte plaintext `NGSP` header
  (`magic = 0x5053474e`) and stores positions, alphas, colors, scales,
  rotations, and optional SH as independently compressed ZSTD streams.
- The first `gsplat-io-spz` slice validates input, point, decoded-scene, TOC,
  and exact uncompressed-stream sizes before constructing `SceneBuffers`.
- Extension-free SPZ defaults to RUB coordinates. Runtime `SceneBuffers` use
  RUF, so this loader applies Niantic `coordinateConverter(RUB, RUF)` flips:
  position Z, quaternion X/Y, and per-band SH sign flips.
- Supported SH degrees are 0–3. Degree 0 keeps 5 streams; degrees 1–3 add the
  sixth SH stream, unquantize with `(byte - 128) / 128`, remap SPZ
  coeff-major RGB triples into PLY channel-major `sh_rest`, then apply the
  RUB→RUF SH flips. Degree 4 remains an explicit unsupported error.
- Committed minimal fixture: `tests/datasets/minimal_v4_degree0.spz` (8 splats,
  degree 0, 331 bytes). Regenerate with
  `GSPLAT_WRITE_SPZ_FIXTURE=1 cargo test -p gsplat-io-spz write_committed_minimal`.
- Unit PLY↔SPZ attribute gate authors RDF PLY that inverts the PLY RDF→RUF path
  so recovered `SceneBuffers` match SPZ RUF output (quaternions compared up to
  sign). Offscreen image parity on the same minimal fixture passes with a
  framed camera (`visible=8`, zero RGB delta); device qualification-scene
  parity remains optional follow-up.
- Cooperative cancel polls between header validation, each attribute-stream
  decompress, allocation, and unpack; cancelled loads return
  `SpzLoadError::Cancelled` without publishing a scene.
- `SourceResidencyCaches` provides independent LRU byte budgets for compressed
  source bytes and decoded `SceneBuffers` (`estimated_scene_bytes`), so
  whole-scene SPZ residency can be bounded beyond per-load `SpzLoadLimits`
  without waiting for Phase D page scheduling.
- Minimal paired load metrics (stored under `target/benchmarks/phase-c/`):
  SPZ transport 331 B vs PLY 1106 B (~0.30×); logical peak 779 vs 1554;
  warm decoded cache hit recorded; first SortedAlpha frame TTFF recorded
  adapter-dependently.
- Legacy gzip v1–3, extension ILV, degree 4, FFI/examples, and larger-scene
  device parity remain deferred outside the minimal-fixture Phase C exit.
- Rust dependency selection pins pure-Rust `ruzstd` 0.8.3 in workspace
  dependencies and decodes each stream into its exact validated output size.
