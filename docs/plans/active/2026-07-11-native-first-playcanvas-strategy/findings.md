# Findings and Decisions

## Requirements

- Document an overall technical strategy for competing with PlayCanvas.
- Prioritize measurable Android/iOS native performance leadership.
- Keep desktop Web rendering performance-competitive rather than attempting to
  match the entire PlayCanvas product surface.
- Include a concrete implementation architecture, phased rollout, risks, and
  an executable verification plan.
- Keep claims evidence-backed and distinguish existing behavior from proposed work.

## G3 Acceptance Findings (2026-07-13)

- The renderer-owned local paged Surface resource from G2 was already usable by
  native sessions; the missing cross-platform piece was a Web geometry selector
  and an honest runtime path receipt.
- The C ABI, Android intent parser, WASM binding, and Web package now use the
  same numeric mapping: direct `0`, packed `1`, paged `2`. Direct remains the
  default and the alternatives remain explicitly experimental.
- Fresh physical Android and headless Chrome runs both exercised
  `paged_active_atlas` and produced non-zero draws. This proves local-source
  Surface usability only; it does not imply HTTP source streaming, long-run
  stability, or competitive performance.
- No telemetry sidecar, remote source, network-adversarial validator, or new
  public asset format was introduced for G3.

## G4 Execution Findings (2026-07-13)

- The retained five-pair series is internally valid and deterministic, but its
  `build.dirty=true` receipts correctly prevent treating it as clean-commit
  publication evidence. G4 will land the protocol in small slices, then rerun
  from the committed harness rather than rewriting those receipts.
- Separate Kitsune and minimal static traces are necessary because the shared
  camera must fit each scene while preserving identical 640x480 projection
  semantics on both renderers. Keeping them as trace fixtures avoids embedding
  renderer-specific camera constants in either collector.
- The PlayCanvas manager must be inspected after the splat placement exists:
  requesting the public GPU-sort enum alone is insufficient. The collector
  fails unless the resolved and active enum, concrete renderer instance, and
  `usesGpuSort` signal all agree.
- PlayCanvas exposes browser frame spacing and an application frame boundary,
  but not comparable preprocess/sort/submit/GPU phase timings. Its raw artifact
  therefore records those values as `null` with explicit unavailable paths;
  filling them with zero would create a false advantage.
- A paired comparator must validate raw counts and renderer receipts, not only
  trust matching manifest labels. The accepted static profile requires all
  3,600 frames on each side to report the full dataset count and locks
  PlayCanvas `GSplatHybridRenderer` / `raster_gpu_sort` against gsplat-rs
  `wasm_sorted_index_direct`.
- The image metric is a deterministic 8x8-window luma SSIM over decoded sRGB
  bytes with an internal identity/opposite-image self-test. It is a parity
  guard for this fixed view, not a perceptual-quality claim across scenes.
- Clean evidence from commit `440fb8f` improved the provisional ratios but does
  not change the claim category: p95 ratio median `1.0200` (95% CI upper
  `1.0200`), p99 `1.0291` (upper `1.0388`), and minimum SSIM `0.998657`.
  This supports parity for one desktop static profile, not a general speed win.
- Relative output roots are unsafe when producer scripts resolve from different
  directories. The successful series used one absolute root; the documented
  protocol should continue requiring fresh absolute per-pair destinations.
- Local distribution evidence must distinguish import/build consumption from
  registry availability. The Web tarball, Android AAR/APK, and Apple binary
  target all consume locally, but none proves npm, Maven, remote binary SwiftPM,
  signing/notarization, multi-ABI Android, or public upgrade compatibility.
- The stable claim is intentionally narrower than the artifacts: direct
  `SortedAlpha`, bounded PLY/SceneBuffers, structured lifecycle/errors, and
  serialized native ownership are v0.1 semantics. Paged residency and package
  convenience APIs remain experimental even though their local smokes pass.

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

## Phase D Streaming Findings

- The active Phase D code boundary is checkpoint `05f649e`; later source
  streaming, telemetry, sidecar, long-run, and artifact machinery is not part
  of the reset branch.
- The checkpoint has useful primitives in `gsplat-render-wgpu`:
  `spatial_pages`, generation-checked `residency`, coarse-cover
  `page_scheduler`, CPU `page_atlas`, GPU `paged_gpu`, and an offscreen
  `PagedActiveAtlas` draw path reusing the packed shader.
- The current offscreen path is not true streaming. `ensure_paged_atlas` sizes
  slots from `pages.page_count()` and force-installs every page; its exact
  minimal parity proves wiring, not bounded active-subset residency.
- Surface construction, path switching, and rendering explicitly reject
  `PagedActiveAtlas`, so a stable non-zero Surface draw remains a D0 correctness
  requirement rather than a later packaging or telemetry task.
- Generation and stale-token checks exist at residency/atlas unit level, but D0
  still needs a direct proof that cancelled or expired work cannot enter the
  active draw set.
- D0 evidence is intentionally short and deterministic: unit tests, offscreen
  count/image parity, and camera-motion short traces. Long RSS runs, network
  adversarial profiles, competitor gates, and SOG decoding are deferred.
- The next and only active blocker is D0-1: lock offscreen count + image parity
  for both minimal and qualification-small scenes before changing residency.
- D0-1 coverage audit on the reset branch found one existing paged parity test,
  `paged_vs_packed_count_parity_on_minimal_scene`. It covers the inline
  two-splat `build_scene()` and already enforces count equality, mean RGB error
  at most `1/255`, and at most `0.1%` of pixels above `3/255`. No paged test yet
  covers the plan's qualification-small scene, so D0-1 remains open.
- The pre-reset verification plan names Kitsune, Flowers, and larger competitive
  datasets but does not define a separate `qualification-small` fixture. The
  checkout currently has committed minimal PLY fixtures plus local/ignored
  Kitsune and Flowers assets. D0-1 must reuse the repository's frozen dataset
  tier/manifest rather than silently inventing a new qualification identity.
- The frozen manifests make Kitsune the smallest external qualified scene
  (279,199 degree-3 splats), but it is still too large for the checkpoint's
  pseudo-full bootstrap and therefore cannot define the pre-D0-2 all-resident
  gate.
- The reset paged renderer force-installs pages with `AttributeLod::Degree0`
  and does not run the packed path's camera-dependent SH hot-color refresh.
  Kitsune is degree 3, so the new D0-1 test must be run before assuming parity;
  a failure would identify a real correctness gap, not a threshold problem.
- The first Kitsune paged parity attempt failed before drawing: bootstrap page
  partitioning produced capacity for 11,468,800 atlas entries (175 slots at
  65,536), and packed preflight correctly returned `PagingRequired` because the
  229,376,000-byte hot buffer exceeded the binding limit. Full Kitsune parity
  cannot be the pre-D0-2 gate: fixed-budget residency intentionally draws an
  active subset, while D0-1 needs an all-resident reference. The
  qualification-small fixture must therefore be a deterministic multi-page
  scene that fits the bootstrap atlas; Kitsune remains a D0-2/D1 dataset.
- The existing synthetic degree-3 packed parity fixture has 64 splats. Under
  `default_spatial_pages` it uses a 16-splat page capacity and a four-axis grid,
  so it exercises multiple pages while remaining all-resident. Reuse it as the
  D0-1 qualification-small fixture instead of adding a new dataset file.
- The deterministic qualification-small gate reaches rendering and preserves
  count exactly (`visible=drawn=64`), but image parity fails with mean RGB error
  `0.014751` and `45.0562%` of pixels above `3/255`; the degree-0 minimal gate
  remains exact. This isolates D0-1's functional gap to paged SH hot-color
  refresh rather than paging coverage, ordering, or packed geometry decode.
- `PackedAtlasResources::write_hot_colors_range` rewrites contiguous full
  records after replacing each record's color word, while paged slots already
  expose their source-scene indices through `active_entries`. A small paged
  refresh can therefore evaluate source SH in atlas order and upload each
  occupied slot without adding a second color pipeline or new module.
- D0-1 is complete. Paged active entries now receive camera-evaluated SH colors
  through the same RGB10 hot-record representation as PackedAtlas. Both the
  two-splat degree-0 gate and the deterministic 64-splat multi-page degree-3
  gate pass with equal visible/drawn counts and zero image error under the
  frozen thresholds. The full crate result is 73 passed, 1 existing ignored
  research oracle, with SortedAlpha conformance passing.
- D0-2 integration can remain inside the existing offscreen rasterizer. The
  residency manager exposes page slot/generation records, and scheduler ticks
  are synchronous at this checkpoint. Snapshot old resident tokens, schedule,
  clear tokens no longer resident, then upload newly resident tokens; draw
  continues to derive solely from `PagedAtlasGpu::active_entries`. This keeps
  eviction generation checks intact and makes non-resident draw exclusion
  directly testable without a parallel runtime stack.
- The first four-slot integration proved active-subset behavior immediately:
  the prior 64-splat fixture occupied more than four pages, so paged drew 16
  splats while packed drew 64. Preserve the four-slot budget and reshape the
  D0-1 fixture to exactly four pages; a separate over-budget scene will prove
  D0-2 exclusion and eviction.
- A four-page fixture with every splat at identical depth is not a valid
  SortedAlpha parity oracle: scheduler slot assignment legitimately changes
  tie order. Give each spatial page group a distinct deterministic depth so the
  gate tests global page sorting rather than unspecified equal-key stability.
- D0-2 is complete with a four-slot fixed atlas. The over-budget 27-page gate
  keeps exactly four resident/occupied slots, changes the resident set after a
  camera jump, and proves every active draw entry belongs to a currently
  resident page while non-resident source splats stay out of draw. Full crate
  evidence is 74 passed, 1 existing ignored oracle, plus passing conformance,
  fmt, check, and diff hygiene.
- D0-3 is complete. Scheduler retention keeps incumbent pages while they remain
  within the target cutoff plus the coarse-cover margin. A four-frame small
  camera-motion trace retains the same resident cover and proves non-zero draw
  plus non-transparent readback on every frame. Full crate evidence is 75
  passed, 1 existing ignored oracle, with conformance passing.
- D0-4 is complete. Inflight cancellation now releases the slot and bumps its
  generation. Guarded GPU upload validates the residency token immediately
  before mutation. Focused gates prove cancelled and stale-generation tokens
  return `StaleToken` and leave `active_entries` empty; full crate evidence is
  77 passed, 1 existing ignored oracle, with conformance passing.
- D0-5 boundary audit: `SurfacePresenter` owns a different device/queue from
  the offscreen rasterizer and currently rejects paged construction, switching,
  and draw. Surface sessions also obtain orders through `Renderer`, whose paged
  preprocess currently assumes an offscreen `GpuRasterizer`. The minimum
  Surface path therefore needs presenter-owned fixed atlas/residency plus a
  session ordering path sourced from the presenter's active page entries; it
  cannot reuse the offscreen GPU objects directly.
- The minimum compatible integration is presenter-owned local pages, fixed
  atlas, residency, and CPU sort scratch. `SurfaceRenderSession::render_with_plan`
  must bypass the renderer's whole-scene order builder only for
  `PagedActiveAtlas`, ask the presenter to schedule/sort/draw its active entries,
  and report the resulting non-zero count. Direct and Packed session behavior
  remains unchanged.
- The Surface packed render pass can be reused verbatim after paged runtime
  preparation because both bind the same packed pipeline/layout. D0-5 only
  needs a paged prepare step plus the existing packed quad draw; no new shader
  or Surface resource layout is required.
- `SurfaceRenderSession::render_with_plan` is the only synchronous choke point
  that assumes renderer-owned whole-scene order. Its paged branch can report
  `presenter.instance_count()` as visible/drawn after presenter preparation;
  async whole-scene sorting stays bypassed for paged frames.
- Current smoke targets are available: an iPhone 17 Pro simulator is booted and
  Android device `033ed212` is connected. The desktop interactive CLI does not
  expose a geometry-path flag at this checkpoint, so D0-5 device evidence should
  use an existing mobile Surface example after focused crate verification.
- Android already routes an experimental geometry-path integer through the
  existing JNI/C setter. D0-5 only needs value `2` mapped to
  `PagedActiveAtlas` and the sample intent parser to accept `paged`; the local
  bundled PLY remains the page source. Because this touches the C enum, the
  header and Rust implementation must change together and the canonical FFI
  smoke must pass.
- Android physical-device D0-5 smoke passed on Nothing A065 / Vulkan with the
  bundled `minimal_ascii.ply`: `geometry_pipeline=paged_active_atlas`, 8 measured
  frames, `visible=3`, `drawn=3` on every frame, and zero missed frames. The app
  created and destroyed the Surface renderer normally.
- The D0 functional code delta remains within the intended slice budget in
  aggregate (about 781 net code lines, no new files); planning evidence accounts
  for the remaining diff. Canonical architecture and Android README text still
  describe direct/packed-only Surface behavior and must be aligned in the same
  functional slice.
- D0-5 and D0 are complete. The presenter owns its local fixed paged runtime,
  Surface sessions bypass whole-scene ordering only for the paged branch, and
  Android path value 2 selects it through the existing experimental setter.
  Completion evidence: 78 render tests passed with 1 existing ignored oracle,
  14 FFI tests passed, conformance passed, workspace check/fmt/strict clippy
  passed, C and JNI smoke passed, APK build passed, and the physical Android
  8-frame run reported `paged_active_atlas` with `visible=drawn=3` throughout.
- D1 now owns steady-state qualification. Its first gate is a two-minute
  physical Android run on qualified Kitsune plus deterministic assertions that
  slot count, resident pages, and active draw entries remain within the fixed
  budget. Thirty-minute and adversarial-network gates remain deferred.
- D1 and Phase D are complete. The 512-frame deterministic trace held atlas
  slots, occupied slots, resident pages, and active entries at or below four on
  every frame with non-zero draw. The physical Android Kitsune Surface run
  continued beyond 9,397 frames; 120-second PSS grew 4,065 KiB and endpoint RSS
  grew 5,532 KiB (`417000 -> 422532 KiB`), below the 64 MiB short-steady-state
  budget. No local instability justified adding a network profile.
- Phase E audit confirms the pinned PlayCanvas harness is only a fail-closed
  WebGPU/GPU-sort path smoke. Its own README explicitly says it collects no
  timing samples and cannot support a competitive result. Revalidate this
  identity/path boundary first; the next Phase E slice must then add a timed v1
  artifact collector before any native-leadership or Web-parity claim is possible.
- Phase E pinned-harness revalidation passed: PlayCanvas `2.21.0-beta.14`, full
  revision `d5fe88878e338936fe763bbce1a58bc315e89cbe`, MIT/integrity checks,
  Chrome WebGPU, `GSplatHybridRenderer`, active `raster_gpu_sort`, and
  `usesGpuSort=true`. The timed collector is now the sole blocker.
- The existing Puppeteer smoke already supplies Chrome discovery, an ephemeral
  local server, fail-closed runtime logging, backend/path assertions, and a
  screenshot. The timed collector should extend this harness boundary rather
  than create a second browser stack.
- The browser harness already owns the `Application`, camera, active manager,
  backend/path signals, and rAF loop. A timed mode can expose raw rAF intervals
  and deterministic camera samples through a second window result, while the
  Node runner writes canonical manifest/frames/summary and invokes the existing
  Python validator. GPU timing stays null and explicitly unavailable.
- The v1 validator currently requires non-null preprocess/sort/geometry-submit
  metrics, but PlayCanvas does not expose those phase boundaries. Writing zeros
  would fabricate evidence. Phase E must amend the contract so only measured
  call/frame-wall remain mandatory; unavailable phase/GPU metrics stay null and
  are named in `unavailable_fields`.
## 2026-07-13 Phase E artifact-contract correction

- The v1 validator currently requires `preprocess_ms`, `sort_ms`, and
  `geometry_submit_ms` even when a competitor does not expose those phase
  boundaries. Filling those fields with zero would fabricate evidence.
- Phase E will keep only directly measured `call_ms` and `frame_wall_ms`
  mandatory. Any unavailable phase/GPU timing must be `null` and its
  `frames[*].<metric>` path must be listed in `manifest.unavailable_fields`.
- The fixture suite needs both a valid nullable-phase case and a rejection case
  for a null metric whose path was not declared unavailable.
## 2026-07-13 PlayCanvas collector boundary

- The pinned engine emits `frameupdate(ms)` near the start of its tick and
  `frameend` after update/render submission. The collector can therefore
  measure browser frame-wall spacing from `ms` and CPU tick work from the
  `frameupdate` to `frameend` boundary without patching PlayCanvas.
- PlayCanvas GPU sort does not expose stable public preprocess/sort/submit or
  GPU completion timings in this harness. Those remain null, not zero.
- The first collector run uses the repository's three-splat binary fixture and
  a frozen static-camera descriptor. It validates collection mechanics only;
  it is not the final matched competitive dataset/trace.
## 2026-07-13 Phase E paired-protocol inputs

- The existing verification plan already fixes the parity rules: identical
  timestamped camera matrices, fixed backing resolution, disabled dynamic
  resolution, production builds, explicit backend/path signals, and raw v1
  artifacts. Qualification requires at least 600 static frames and 1,800 orbit
  frames.
- Kitsune and Flowers are locally present as binary little-endian PLY inputs;
  Kitsune is the smallest existing nontrivial degree-3 paired candidate.
- The gsplat-rs Web example already has a v1 browser artifact collector. The
  paired slice should extend the two existing collectors around one frozen
  protocol instead of introducing another runner.
- The current gsplat-rs Web collector forces synchronous mode and its browser
  viewer advances an engine-local yaw orbit. That is useful for internal smoke
  but cannot be paired with PlayCanvas as end-to-end Web parity evidence.
- A fair pair must use asynchronous rAF collection and the exact same exported
  camera input. Renderer-local fit/orbit formulas are not acceptable even when
  they look visually similar.
- The first valid PlayCanvas Kitsune static candidate contains 3,600 measured
  frames over 30.001 seconds after 120 warmups. Dataset, trace, WebGPU,
  `GSplatHybridRenderer`, GPU-sort policy, 640×480 backing size, and DPR 1 all
  match the frozen side of the protocol.
- Its p95 frame-wall value is 10.10 ms with zero configured-budget misses, but
  this number is not comparable evidence until the gsplat-rs artifact and image
  gate use the identical input.
- The gsplat-rs WASM Surface wrapper currently exposes only fit/reset/orbit/
  pan/zoom controls; its auto-fit also uses a different distance factor than
  the Web example. Reproducing a trace through those controls would not prove
  the same camera.
- The minimal correct bridge is an explicit camera setter on the existing WASM
  renderer, with exact position/quaternion/intrinsics and resize preservation.
  This serves the current paired blocker and later orbit trace replay without
  adding a benchmark-only renderer path.
- The first gsplat-rs Kitsune candidate also validates with 3,600 rAF frames
  over 29.985 seconds, the exact shared trace and display, and full 279,199
  visible/drawn counts. Its initial manifest used the source filename as ID and
  omitted locally available git/device identity, so it is retained only as a
  plumbing result and will be recaptured after identity normalization.
- Paired manifest identity now matches exactly for dataset, trace, display,
  repository commit/dirty state, and device. The first screenshot attempt used
  element bounding boxes, which included each viewer's overlay and, for the
  responsive gsplat page, CSS-scaled output. Quality evidence must instead
  serialize the canvas backing buffer directly.
- Local code confirms gsplat-rs converts common 3DGS PLY data from RDF to RUF
  at load time by reflecting Y and adjusting rotations/SH. The pinned
  PlayCanvas path retains the source coordinate basis. A qualification-only
  entity Y reflection is therefore required at its scene boundary before a RUF
  camera trace can be compared; this is input normalization, not quality tuning.
- After RDF-to-RUF normalization, orientation matches but the PlayCanvas solid
  image bounds are about 205×274 pixels versus gsplat-rs 231×311. This closely
  resembles the historical 1.2-vs-1.35 auto-fit distance ratio, so the next
  check must compare an actual PlayCanvas projection receipt and confirm the
  explicit gsplat camera is not being overwritten. Do not tune distance from
  screenshots alone.
- The explicit gsplat camera receipt matches the shared trace to expected f32
  precision, and the PlayCanvas projection receipt has the same 60° vertical
  FOV and 4:3 X/Y projection coefficients. The remaining scale/opacity mismatch
  is therefore in splat raster semantics, not camera overwrite.
- Initial ImageMagick output was misread as similarity; the parenthesized SSIM
  metric is dissimilarity (zero for identical images). After the correct
  PlayCanvas basis conversion, similarity is about `1 - 0.04638 = 0.95362`,
  still below the pre-existing 0.99 target. Timing data remains non-claimable.
- PlayCanvas exposes alpha clip, minimum pixel size/contribution, anti-alias,
  and Gaussian support behavior. These must be mapped against gsplat-rs's
  3-sigma quad and 0.3-pixel low-pass before choosing any quality profile; a
  camera or entity scale compensation would invalidate the shared trace.
- The two rasterizers use equivalent Gaussian parameterizations: gsplat-rs
  uses a 3-sigma quad with `exp(-4.5 r²)`, while PlayCanvas uses a
  `2*sqrt(2)`-sigma quad with normalized `exp(-4 r²)`. Both add the same 0.3
  pixel covariance floor. A global geometry scale change is not justified.
- PlayCanvas's default `minContribution=3` and effective one-pixel minimum can
  cull splats that gsplat-rs retains. The fixed quality profile therefore sets
  contribution/pixel culls to zero, forward alpha clip to 1/256, foveation and
  anti-alias off, and an identical black clear color before rechecking SSIM.
- A probe that changed gsplat-rs from its 3-sigma/exponential support to the
  PlayCanvas normalized support slightly worsened similarity (about 0.95223).
  The product raster change was rejected and fully reverted.
- The apparent raster-scale problem was confounded by a camera-basis error:
  PlayCanvas's optical forward is local -Z. Pointing it toward RUF +Z preserves
  up but reverses the screen-right axis, producing a horizontally mirrored
  image. The correct static bridge reflects source Y/Z and trace camera Z so
  PlayCanvas looks down its native -Z while screen right remains +X.
- Formula-level projection of all 279,199 PLY centers/covariances predicts a
  roughly 211×279 gsplat footprint, close to PlayCanvas and not the observed
  231×311 canvas. The cause is in the viewer loop: static benchmark yaw zero
  still called `orbit(0, 0)`, which cleared the explicit WASM camera override
  and restored the native 1.2 auto-fit camera. Zero yaw must be a true no-op.
- Final camera receipts confirm the PlayCanvas camera is at the converted trace
  position with forward -Z, 60° vertical FOV, 4:3 projection, and the exact
  near/far planes. The gsplat receipt also matches its RUF trace values. Camera
  compensation is neither needed nor allowed.
- After correct basis conversion and fixed quality settings, ImageMagick SSIM
  dissimilarity is about 0.0464, i.e. similarity about 0.9536. The images have
  no holes and matching subject orientation, but their splat footprint/opacity
  differs systematically. This does not pass the frozen 0.99 gate.
- All pre-freeze paired timing candidates are invalidated. They must not be
  cited, and lowering the threshold to fit them would be benchmark gaming.
## 2026-07-13 Phase E SSIM metric audit

- ImageMagick 7.1.2-13 `compare -metric SSIM` and `-metric DSSIM` return the
  same zero-for-identical distortion value in this environment, despite the
  bundled documentation describing SSIM as one-for-identical. The prior
  `1 - normalized_output` conversion is therefore not a sufficiently defined
  qualification metric.
- Before any raster tuning or gate decision, the series needs a repository-
  owned SSIM implementation/fixture with a one-for-identical contract. Existing
  exploratory timing remains invalid regardless of the eventual score.
- The repository SSIM probe scores the current matched static pair at
  `0.8713640084`, below 0.99. This supersedes every ImageMagick-derived score.
- `SurfacePresenter::render_direct_frame` writes camera parameters on every
  frame, and the artifact camera receipt matches the trace. PLY opacity remains
  a logit and is sigmoid-activated exactly once; benchmark yaw zero is accepted
  as zero. Those suspected causes are ruled out.
- The remaining mismatch is now localized to cross-engine raster/covariance/
  blend semantics rather than camera publication, dataset identity, or metric
  ambiguity.
- A diagnostic-only uniform resize of the gsplat PNG peaks near 88% and raises
  canonical SSIM from 0.871 to about 0.957, but still cannot pass 0.99. This is
  evidence of a global projection/scene-scale mismatch, not permission to
  rescale qualification images.
- A 50/50 overlay shows duplicated rigid edges across the whole shrine, not
  merely wider Gaussian tails. The next receipt must therefore compare actual
  loaded center bounds before revisiting covariance support.
- PlayCanvas's raw center bounds exactly match the Kitsune manifest after the
  declared Y/Z basis conversion; camera position/forward/projection are also
  exact. Scene loading does not introduce a global scale.
- The direct Surface uniform layout and WGSL projection formula match the Rust
  receipt (`f=1/tan(vfov/2)`, 4:3 aspect), and camera parameters are written on
  every frame. A minimal shared-camera diagnostic scene is now the cheapest way
  to separate projected-center errors from covariance/support differences.
- The new shared minimal trace validates and both engines render all three
  splats with exact camera receipts. Canonical SSIM is `0.9580970409`, still
  below 0.99.
- Minimal images fill the canvas with a smooth Gaussian field and differ in
  gradient/support rather than projected centers. This cleanly localizes the
  remaining gap to Gaussian raster semantics. Re-testing the PlayCanvas
  normalized support formula is now a controlled single-variable experiment,
  unlike the earlier Kitsune probe that was confounded by camera scale.
- The isolated normalized-support probe worsens minimal SSIM from
  `0.9580970409` to `0.9493355401`; it is rejected and reverted again under a
  now-unconfounded minimal trace.
- The repository minimal fixture has log scales around `+1` (sigma around
  2.7), causing both renderers to hit large-splat support caps across nearly the
  whole canvas. It is useful for blend/falloff regression but not a clean
  uncapped covariance oracle.
- A new small-scale, high-opacity diagnostic fixture is required to measure
  projected center and ellipse radii without cap behavior. That remains within
  the same raster-quality blocker.
# Phase E raster-diagnostic camera finding (2026-07-13)

- The shared three-splat diagnostic rendered count `3` in both engines but raw
  SSIM was only `0.6931295175`.
- Pixel component centers in the gsplat-rs frame exactly match its deterministic
  auto-fit camera, not the requested static trace camera. The manifest receipt
  was cached immediately after `setCamera`, so it did not prove the camera still
  active at capture time.
- Qualification receipts must be read from the native renderer when artifacts
  are emitted. Re-read the receipt at measurement completion before changing
  projection or covariance code.
- The post-measurement receipt proved the override was cleared: position
  `[0.000441, 0.075, -0.703538]` and quaternion Y `-0.0002` exactly match the
  auto-fit camera after four `0.0001` yaw steps. Although the qualification URL
  requested zero yaw, the HTML range control clamps zero to its `0.0001`
  minimum. Qualification traces must bypass that interactive control and force
  `yawStep=0` in benchmark state.
- After forcing qualification yaw to zero and reading the final native receipt,
  the receipt stayed exactly at position `[0, 0, -3]`, identity rotation, and
  the requested intrinsics. The unchanged three-splat frames then reached SSIM
  `0.9973430421 >= 0.99` with count `3` on both engines. This validates the base
  projection, color, opacity, and uncapped covariance path without image-space
  registration or threshold changes.

# Phase E clean five-pair result (2026-07-13)

- Five sequential randomized-order Kitsune static pairs each retained 120
  warmups, 3,600 measured frames, raw summaries, final images, and matched
  pair/order receipts. All ten v1 artifacts validate.
- Clean commit `440fb8f` produced gsplat-rs / PlayCanvas frame-wall p95 median
  ratio `1.0200000048` with a
  deterministic 100,000-sample bootstrap 95% CI of
  `[1.0099999905, 1.0200000048]`, below the `1.10` claim qualifier.
- The p99 median ratio is `1.0291262275`, CI
  `[1.0097087471, 1.0388349561]`, below the `1.20` qualifier. Minimum pair SSIM
  is `0.9986570102`, above `0.99`; every run reported zero missed frames and
  every manifest records `build.dirty=false`.
- This earns only a local Chrome/WebGPU, Kitsune-static desktop parity result.
  The repository still has no current five-pair native-vs-mobile-Web matrix,
  broad Flowers/medium-1m browser matrix, 30-minute thermal/energy evidence,
  competitor total-residency accounting, or 10M qualification. Those claims
  remain explicitly unearned rather than blocking internal Phase F packaging.

# Phase F local distribution result (2026-07-13)

- The rebuilt `@gsplat-rs/web` package passed wrapper tests and dry-run packing;
  its real tarball installed into an isolated `target/` consumer and exposed
  `initGsplatWeb`, both renderer factories, and `GsplatWebRenderer`. A rebuilt
  WASM browser smoke rendered three visible/drawn splats through
  `wasm_sorted_index_direct`.
- Android host JNI smoke, arm64 release AAR, and sample APK passed. The AAR
  contains `AndroidManifest.xml`, `classes.jar`, and
  `jni/arm64-v8a/libgsplat_jni.so`.
- Apple host Swift smoke, device plus arm64/x86_64 simulator XCFramework slices,
  `swift package describe`, and generic iOS Simulator `xcodebuild` all passed.
- These are local consumption proofs, not npm, Maven, or binary SwiftPM
  publication. They do not widen the stable v0.1 boundary beyond PLY,
  `SceneBuffers`, `SortedAlpha`, and the small serialized C ABI.

# Goal gate reform rationale (2026-07-13)

- The earlier plan mixed capability correctness with aspirational performance
  wins. That made a fixed ratio, long run, or unavailable device capable of
  blocking a functionally correct phase indefinitely.
- The reset-line terminal rule is now: hard correctness and bounded-safety
  failures must be fixed; measured performance or matrix gaps remain visible and
  restrict only the corresponding claim.
- Existing uncommitted D-F code is not accepted merely because prior notes say
  complete. Each Goal Breakdown slice must receive focused verification and its
  own commit before the terminal audit can mark the initial goal achieved.

# G2 bounded paging acceptance (2026-07-13)

- The renderer slice is one compile-time unit: fixed-slot GPU resources,
  scheduler/residency semantics, offscreen gates, and presenter-owned paged
  resources meet in `gsplat-render-wgpu`. Splitting `lib.rs` merely to separate
  Surface construction from paging tests would create an unbuildable commit, so
  the renderer-owned local Surface resource stays in G2 while platform/Web
  integration remains G3.
- Hard evidence passes with page count greater than four slots, resident/active
  bounds at four, deterministic eviction and retained cover, no draw from
  non-resident pages, and generation-checked rejection before GPU mutation.
- The 512-frame and Android memory values are useful boundedness observations,
  not a requirement to pass a 30-minute or percentage-growth headline.
