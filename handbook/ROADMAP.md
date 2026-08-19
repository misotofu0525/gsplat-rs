# Roadmap

This file defines the current direction, technical sequencing, and release
boundary for `gsplat-rs`. Operational facts and command entrypoints live in
`handbook/PROJECT_CONTEXT.md` and `handbook/VERIFICATION.md`; task evidence and
transient research belong under `docs/plans/`.

## Project Position

- `gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust +
  `wgpu`.
- The product thesis is an embeddable, mobile-first splat renderer: one small
  Rust/`wgpu` core behind a stable C ABI, packaged as local AAR, XCFramework,
  and npm slices, judged on bytes, frame time, and sustained power on phones.
- On Web the target is renderer-versus-renderer parity with PlayCanvas on
  matched scenes through `tests/competitive/playcanvas`, not engine or editor
  breadth.
- The project remains on the `0.1.x` line and keeps a deliberately small release
  surface while the core render path is validated across real scenes and devices.
- `SortedAlpha` is the only quality-guaranteed render mode.
- Desktop, Android, iOS, and Web are validation and integration surfaces for the
  shared renderer, not separate renderer implementations.
- Android has a local AAR packaging slice, Apple has a local
  `GsplatKit`/XCFramework slice, and Web has a local `@gsplat-rs/web` ESM wrapper.
  Tagged prereleases may attach these artifacts directly to GitHub Releases, but
  they are not Maven, binary SwiftPM, npm, or crates.io publication.

## Stable v0.1 Release Boundary

- The stable contract is bounded PLY, SPZ v4, unbundled SOG, or bundled `.sog`
  whole-scene import into validated in-memory `SceneBuffers`, resident `SortedAlpha`
  rendering, structured errors, and the small C ABI. Streamed SOG is a
  metadata-first subset assembler used by Rust/desktop/bench-runner, not a
  C ABI scene type.
- One resident-scene representation serves every platform. Capacity preflight
  fails explicitly instead of selecting another storage or residency mode.
- CPU radix ordering is the default production ordering backend. GPU and
  Adaptive ordering are experimental Rust controls.
- Native handles are single-owner handles used from one serialized thread or
  queue. Wrapper locking does not make the raw C ABI free-threaded.
- The current C ABI intentionally stays small:
  - version and error reporting: `gsplat_version_major`,
    `gsplat_version_minor`, `gsplat_error_message`,
    `gsplat_last_error_message`
  - defaults: `gsplat_config_default`, `gsplat_camera_default`
  - offscreen lifecycle: `gsplat_context_create`, `gsplat_context_destroy`,
    `gsplat_context_set_camera`, `gsplat_context_set_auto_camera`,
    `gsplat_context_load_scene_path`, `gsplat_context_load_scene_bytes`,
    `gsplat_context_render_frame`,
    `gsplat_context_get_stats`
  - Android and iOS Surface create/resize/camera-control/render/stats/destroy
    functions used by the validation integrations
- The stable C ABI does not cover runtime render-mode switching, raw `wgpu`
  resources, experimental ordering/storage controls, or Streamed SOG.
- Mobile Surface convenience wrappers, Web package APIs, and benchmark artifact
  schemas remain experimental.
- Whole-scene SPZ v4, unbundled SOG, and bundled `.sog` import are consumed
  through `gsplat-io`. Streamed SOG is a separate metadata-first subset assembler.

## Current Technical Baseline

### Shared resident path

- Web, desktop interactive, Android, and iOS Surface clients share
  `SurfaceRenderSession` for camera revisions, ordering cadence, compact order
  uploads, fallback behavior, presentation, and frame telemetry.
- Scene attributes stay GPU-resident. CPU refreshes upload compact
  sorted source IDs. A per-splat compute preprocess writes compact projected
  records once per presented frame; the vertex/fragment stages only emit and
  shade quads. Default resident storage remains full-f32; an explicit
  `ResidentStorageProfile::Quantized` option packs a 32-byte SPZ-aligned hot
  record plus per-degree u8 SH sidecars. The stable C ABI does not expose the
  profile.
- Mobile keeps the default CPU sort interval of 2. Identical redraws reuse the
  existing order, and native CPU ordering can use the bounded `AsyncLatest`
  schedule.

### Experimental GPU and Adaptive ordering

- The experimental GPU baseline now evaluates near/far plus NDC footprint
  visibility on both full-f32 and quantized keygen, compacts visible
  `(key, id)` without CPU readback, sorts only that compacted count with
  eight stable 4-bit radix passes and a two-level hierarchical prefix
  scan, and consumes GPU-written indirect sort-dispatch and draw
  arguments. Host CPU-vs-GPU image parity is exact on the empty / near-far
  / screen-edge / degenerate / tie / degree-3 fixtures. CPU radix remains
  the default. Session telemetry still reports the resident source count,
  not the GPU-visible count.
- Adaptive is an opt-in measurement policy, not a point-count rule. It learns
  CPU behavior first, probes GPU, retains hysteresis/cooldowns, and falls back to
  CPU on a GPU execution error.
- Retained Nothing A065 / Adreno 730 evidence found the current GPU baseline
  slower than CPU in every paired ladder comparison. CPU therefore remains the
  default. See
  [`2026-07-22-adaptive-sort-experiment/report.md`](../docs/plans/completed/2026-07-22-adaptive-sort-experiment/report.md).
- The corrected candidate (compact + hierarchical scan + indirect) has
  host image-parity evidence and paired device evidence on Adreno 730,
  desktop Metal, Chrome WebGPU, and iPhone 17 Pro Max: GPU never beat
  CPU end to end, though the deficit shrinks with device class (Adreno
  1.46×, desktop Metal 1.40×, iPhone 1.07× at Truck 700k). CPU remains
  the default. External kernel evidence (PlayCanvas engine PR #8620) is
  architecture guidance only: it compares GPU sorters against each
  other, and PlayCanvas itself enables GPU sorting only on non-mobile
  devices.

### Scene-aware Surface limits

- Surface device creation computes the largest binding required by the resident
  scene, starts from portable `wgpu` defaults, and raises only
  the required storage/buffer limits when the adapter exposes that headroom.
  `max_storage_buffers_per_shader_stage` is raised to the WebGPU default of 8
  when the adapter allows it, so quantized per-degree SH sidecars can bind.
- The request is deterministic: one representation, one exact capability
  request, one structured result. There is no retry loop or hidden fallback.
- Adapter limits are legality ceilings, not available-memory guarantees. Initial
  geometry allocation still needs validation/OOM error scopes to make memory
  pressure failures as actionable as capability preflight.
- The retained A065 Vulkan evidence caps full-f32 resident degree-3 SH scenes at
  745,654 splats because the physical storage-binding limit is 128 MiB. See
  [`2026-07-22-adaptive-storage-limits/report.md`](../docs/plans/completed/2026-07-22-adaptive-storage-limits/report.md).

## Strategic Execution Sequence

The 2026-08-13 strategy diagnosis reordered this sequence around mobile
memory-bandwidth reality: fix structure, then the data plane, then visibility
and ordering, then streaming. Research evidence and per-idea accept/reject
reasoning live in
[`2026-08-13-sdk-strategy-diagnosis/findings.md`](../docs/plans/completed/2026-08-13-sdk-strategy-diagnosis/findings.md).

### 1. Structural debt paydown and internal render-stage decomposition

The crate split and vestige deletion landed on 2026-08-13. Evidence:
[`2026-08-13-phase-0-structural-debt`](../docs/plans/completed/2026-08-13-phase-0-structural-debt/).

Keep behavior identical while making the render crate safe to iterate on.
The remaining rule for new GPU work:

- implement new GPU work through narrow internal stages rather than another
  platform-specific state machine: a preprocess stage owns visibility,
  compaction, keys, and indirect arguments; an ordering stage owns CPU/GPU
  backend encoding and deterministic fallback; a draw stage owns resident
  render-pass encoding; `SurfaceRenderSession` continues to own scheduling,
  dirtiness, camera revisions, presentation policy, and telemetry.

Stages may accept internal `wgpu` resources and a caller-owned encoder, but raw
`wgpu` types do not enter the stable C, JNI, Swift, or Web package contracts.
Do not create a new crate until the existing `gsplat-render-wgpu` boundaries are
proven insufficient.

### 2. Quantized resident storage and per-splat compute preprocessing

Compute preprocess and an explicit quantized resident profile landed on
2026-08-13 as a Rust-only option. Evidence:
[`2026-08-13-quantized-resident-preprocess`](../docs/plans/completed/2026-08-13-quantized-resident-preprocess/).

In tree today:

- every presented frame runs a per-splat compute preprocess that writes
  compact projected records; the vertex/fragment stages only emit quads
- `ResidentStorageProfile::FullF32` remains the default quality reference
- `ResidentStorageProfile::Quantized` packs a 32-byte SPZ-aligned hot record
  (f16 positions, smallest-three rotation, log-u8 scale, u8 DC/opacity) plus
  per-degree u8 SH sidecars; preflight shows 1M degree-3 splats fit in 128 MiB
  per binding, with the largest SH sidecar at 21 B/splat
- profile selection stays out of the stable C ABI
- CPU ordering is unchanged and composes with the preprocess
- experimental GPU ordering keygen reads quantized f16 positions
- Surface/offscreen devices request WebGPU's 8 storage buffers per stage when
  the adapter exposes them; quantized preprocess uses 7
- Android A065 and desktop Chrome WebGPU Kitsune artifacts exist for the
  quantized layout. Sample/collector extras select the profile
  (`gsplat_surface_storage_profile` / `--storage-profile` /
  `GSPLAT_STORAGE_PROFILE`); the stable C ABI stays on full-f32

This data-plane item is closed. Compressed resident storage is not
streaming.

### 3. GPU-visible compaction, portable GPU ordering, and indirect drawing

Host-side candidate landed on 2026-08-13 as an experimental path. Evidence:
[`2026-08-13-gpu-compact-order-indirect`](../docs/plans/completed/2026-08-13-gpu-compact-order-indirect/).
The device gate is complete (2026-08-18). Host CPU image parity landed,
including exact Kitsune/Flowers renders. Paired evidence on A065
(Kitsune 1.46×; Truck 50k–700k ratio 2.02→1.08, no crossover), desktop
Metal (1.40×), Chrome WebGPU (portability; sync collector sees CPU call
walls only), and iPhone 17 Pro Max Surface (Kitsune vsync-capped both
backends; Truck 700k CPU holds 60 fps vs GPU 17.9 ms, 1.07×) all ran
with zero GPU fallbacks. No platform showed a crossover, so CPU radix
remains the default and the compact+indirect path stays an experimental
backend. Re-opening the default question requires new hardware evidence
(for example a device where Adaptive probing shows GPU winning). Reduced-
width depth keys were measured and demoted; they are not a cheaper GPU
default. Evidence:
[`2026-08-18-depth-key-width`](../docs/plans/completed/2026-08-18-depth-key-width/).

Build on the compute preprocess stage from item 2:

- perform GPU frustum/footprint visibility evaluation, then compact visible
  source IDs and depth keys without CPU readback;
- write sort-dispatch and draw-indirect arguments from the compacted count;
- record into the caller-owned frame encoder and share the existing resident
  scene buffers and shader contract;
- replace the serial single-workgroup global prefix scan in the retired
  baseline with a hierarchical or wait-free scan; keep the multi-pass 4-bit
  radix digit width as the portable primary (no subgroup or forward-progress
  dependency); treat subgroup-accelerated variants as capability-gated
  experiments only;
- retain deterministic back-to-front order with ascending source-ID tie
  order; sort only the compacted visible count and consume the result in the
  resident draw;
- keep pipeline creation outside measured frames and preserve same-frame CPU
  fallback on execution errors; feed the candidate through existing Adaptive
  telemetry rather than adding a repository-wide point-count threshold;
- re-run the retained Adreno ladder after items 1-2 land so the GPU-vs-CPU
  default decision reflects the corrected candidate, not the retired
  baseline.

Promotion evidence must cover empty scenes, dispatch tails, near/far and
screen-edge cases, degenerate covariance, duplicate-depth tie behavior,
swapchain timeout/retry behavior, CPU image parity, and Web/WASM portability.
No GPU backend becomes the default from a kernel microbenchmark alone.
Promotion requires end-to-end paired results and image gates on representative
Android, Apple, desktop, and browser adapters.

### 4. Ecosystem-aligned streaming and level of detail

Slice 1 (landed 2026-08-18): promote whole-scene SPZ v4 import through
`gsplat-io`. Desktop, bench-runner, `gsplat_context_load_scene_path`,
Android/iOS Surface path-create, and wasm `createRenderer` dispatch `.ply` /
`.spz` (or magic). This is still one resident `SceneBuffers` after decode.

Slice 2 (landed 2026-08-18): additive stable C ABI
`gsplat_context_load_scene_bytes` for in-memory PLY / SPZ v4, later extended
to bundled `.sog` ZIP via the same magic-sniffing symbol. API version stays
0.1. Streamed SOG JSON is rejected.

Slice 3–4 (landed 2026-08-18): PlayCanvas unbundled SOG (`meta.json`) and
Streamed SOG (`lod-meta.json`). `StreamedSogSession` reads the spatial tree
first, selects leaves under independent source / decoded / gaussian budgets,
and decodes only that subset. The existing renderer still uploads one
`SceneBuffers` of the subset. This is not Packed/Paged.

Slice 5 (landed 2026-08-19): bundled `.sog` ZIP whole-scene import (STORED
and DEFLATE, with zip-bomb bounds), native parallel missing-chunk decode, and
a camera-driven desktop Streamed SOG session (`reload_scene` when the
selection fingerprint changes).

Slice 6 (landed 2026-08-19): independent GPU-resident gaussian budget
(`StreamingBudgets.max_resident_gaussians`) applied while selecting leaves.
Desktop and bench-runner ask `Renderer::max_resident_gaussians` (real device
limits when an offscreen rasterizer exists; portable `downlevel_defaults`
otherwise) before assemble. The renderer still uploads one selected
`SceneBuffers` and runs resident preflight. This is not a page pool.

Remaining in this item:

- A C ABI for streaming/LOD. Do not invent a proprietary scene format. Track
  the Khronos `KHR_gaussian_splatting` glTF extension and its planned SPZ
  streaming extension as they ratify.

### 5. Mobile-only differentiators

- Add a thermal/power-aware quality governor driven by platform thermal APIs
  and frame telemetry: resolution scale, SH degree clamp, and sort cadence
  under sustained load, with explicit policy controls.
- Extend the benchmark artifact contract with battery and thermal endurance
  runs so sustained-quality claims stay evidence-backed.

### 6. Evidence before policy or API promotion

- Reuse the deterministic dataset ladder and paired benchmark artifact contract.
- Separate performance subsets from full-scene quality anchors.
- Report frame-wall and stage timings; add capability-gated GPU timestamp and
  asynchronous readback telemetry without blocking the render thread.
- Record actual backend, fallback, visible/drawn counts, adapter limits, and
  dataset/binary identity in retained artifacts.
- Require true-device evidence for platform claims. Host compilation, simulator
  launch, or one short smoke does not establish mobile performance or stability.
- Widen the public Rust API only after stage ownership and error semantics have
  survived the cross-platform evidence matrix. Widen the C ABI only through a
  separate release-boundary decision.

## External Architecture Reference

The GPU sequence above was informed by a source review of
[`LioQing/wgpu-3dgs-viewer` at `ed0a76a` (`v0.7.0`)](https://github.com/LioQing/wgpu-3dgs-viewer/tree/ed0a76a777cbbced4a193694dc7efddbf505f324).
The reference is architectural input, not performance evidence for this repo.

Adopt the following ideas through local implementations and verification:

- a high-level owner composed from independently testable preprocess, ordering,
  and draw stages;
- GPU visibility compaction feeding indirect sort and draw counts;
- GPU-resident layout profiles paired with shader specialization;
- focused GPU component tests in addition to end-to-end image tests.

Do not copy these policies into the release contract:

- unconditional preprocess/sort work on every high-level frame;
- raw `wgpu` objects in stable native or Web package APIs;
- source-count-driven allocation without the existing bounded import budgets;
- per-model sorting plus caller draw order represented as global transparent
  ordering;
- performance conclusions without gsplat-rs paired device artifacts.

The 2026-08-13 strategy diagnosis added competitive and research anchors; full
links and reasoning live in
[`2026-08-13-sdk-strategy-diagnosis/findings.md`](../docs/plans/completed/2026-08-13-sdk-strategy-diagnosis/findings.md):

- PlayCanvas engine 2.19 ships a compute WebGPU splat renderer (GPU culling,
  stream compaction, GPU radix sort, indirect draw) and is the Web parity
  target renderer.
- PlayCanvas engine PR #8620 provides cross-device sort portability evidence:
  multi-pass 4-bit radix with a hierarchical scan is the portable winner;
  OneSweep-style decoupled-lookback designs are not portable to mobile GPUs.
- SPZ v4 is the cross-vendor interchange format; Streamed SOG is the Web
  streaming/LOD reference; `KHR_gaussian_splatting` is the pending glTF
  extension.
- 2025-2026 mobile research consensus: per-splat compute preprocessing,
  quantized GPU residency, hierarchical/wait-free scans, and hardware-raster
  splatting on bandwidth-limited devices. Sort-free and stochastic approaches
  change image semantics or require retrained assets; they stay out of the
  default path.

## Retired Packed/Paged Evidence

The current Packed/Paged research track closed on 2026-07-21. Detailed evidence
lives under
[`2026-07-18-render-paging-architecture-convergence`](../docs/plans/completed/2026-07-18-render-paging-architecture-convergence/)
and
[`2026-07-21-packed-atlas-branch-closeout`](../docs/plans/completed/2026-07-21-packed-atlas-branch-closeout/).

- Packed and Paged runtime code and public selectors were removed in August
  2026. Their completed plans remain historical design and device evidence.
- The useful lessons are retained: compact storage needs independent quality
  proof, and fixed GPU slots do not establish bounded source/CPU residency or a
  performance win.
- Capacity preflight reports when the resident representation does not fit; it
  does not silently select an unproven replacement.
- A future streaming track must start from metadata-first loading, bounded
  compressed/decoded caches, asynchronous decode, spatial hierarchy/LOD, and
  measured source/CPU/GPU residency. It is a new architecture, not a revival of
  the retired four-slot prototype by terminology alone.

## Release and Promotion Bar

- The canonical day-to-day and targeted verification sets live in
  `handbook/VERIFICATION.md`.
- Complete manual, artifact, and remote-settings gates live in `RELEASING.md`.
- Before cutting a release, also run:

```bash
RELEASE_VERSION=<major.minor.patch> bash tests/release/check-version.sh
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

New resident GPU or storage work is not release-gated merely because it is merged.
The default changes only after its focused correctness, image, device, and
stability gates are documented in `handbook/VERIFICATION.md` and promoted here.

## Open Product and Distribution Gaps

- Million-point mobile capacity: prove compressed resident profiles or a
  future bounded streaming architecture without hiding physical adapter limits.
- GPU ordering: replace the current deterministic research baseline with a
  compacted, portable candidate that wins end to end on a representative device
  matrix.
- GPU observability: add non-blocking GPU-complete timing where supported.
- Allocation diagnostics: wrap initial geometry allocation in validation/OOM
  error scopes and preserve structured failure details through wrappers.
- Android distribution: Maven publishing, multi-ABI packaging, and a higher-level
  Android view/API remain future work.
- Apple distribution: remote binary SwiftPM/XCFramework distribution and a
  polished Apple product API remain future work.
- Web distribution: npm publication waits for target-browser Surface smoke and
  explicit promotion of the package API.
- SPZ / SOG product integration: whole-scene path and memory load cover PLY,
  SPZ v4, and bundled `.sog`; path load also covers unbundled SOG. Streamed
  SOG already applies an independent GPU-resident gaussian budget before
  assemble. A C streaming ABI remains later work.

## Explicitly Not Active Right Now

- A custom internal binary scene/cache format
- Reintroducing the retired Packed/Paged modes
- Disguising a full-scene `SceneBuffers` plus slot selector as streaming.
  Streamed SOG must keep selecting from `lod-meta.json` before decode.
- Additional experimental blending/rendering backends, including sort-free or
  stochastic approximations that change image semantics or require retrained
  assets
- A public raw-`wgpu` C, JNI, Swift, or Web API
- New top-level apps, crates, or docs-only placeholders without an explicit
  release-boundary reason
- Published Maven, binary SwiftPM, npm, or crates.io distribution
