# Roadmap

This file defines the current direction, technical sequencing, and release
boundary for `gsplat-rs`. Operational facts and command entrypoints live in
`handbook/PROJECT_CONTEXT.md` and `handbook/VERIFICATION.md`; task evidence and
transient research belong under `docs/plans/`.

## Project Position

- `gsplat-rs` is a cross-platform Gaussian Splatting renderer built with Rust +
  `wgpu`.
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

- The stable contract is bounded PLY import into validated in-memory
  `SceneBuffers`, resident `SortedAlpha` rendering, structured errors, and the
  small C ABI.
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
    `gsplat_context_load_scene_path`, `gsplat_context_render_frame`,
    `gsplat_context_get_stats`
  - Android and iOS Surface create/resize/camera-control/render/stats/destroy
    functions used by the validation integrations
- The stable C ABI does not cover scene-from-memory loading, runtime render-mode
  switching, raw `wgpu` resources, or experimental ordering/storage controls.
- Mobile Surface convenience wrappers, Web package APIs, and benchmark artifact
  schemas remain experimental.
- The bounded SPZ v4 loader remains an isolated import component until a
  consumer integration is selected and verified separately.

## Current Technical Baseline

### Shared resident path

- Web, desktop interactive, Android, and iOS Surface clients share
  `SurfaceRenderSession` for camera revisions, ordering cadence, compact order
  uploads, fallback behavior, presentation, and frame telemetry.
- Scene attributes stay GPU-resident. CPU refreshes upload compact
  sorted source IDs; projection, covariance use, SH evaluation, and rasterization
  remain on the GPU.
- Mobile keeps the default CPU sort interval of 2. Identical redraws reuse the
  existing order, and native CPU ordering can use the bounded `AsyncLatest`
  schedule.

### Experimental GPU and Adaptive ordering

- The current deterministic GPU baseline generates keys for the full resident
  scene, runs eight stable 4-bit radix passes, and performs a normal instance
  draw. It does not yet compact screen-visible entries or use indirect drawing.
- Adaptive is an opt-in measurement policy, not a point-count rule. It learns
  CPU behavior first, probes GPU, retains hysteresis/cooldowns, and falls back to
  CPU on a GPU execution error.
- Retained Nothing A065 / Adreno 730 evidence found the current GPU baseline
  slower than CPU in every paired ladder comparison. CPU therefore remains the
  default. See
  [`2026-07-22-adaptive-sort-experiment/report.md`](../docs/plans/completed/2026-07-22-adaptive-sort-experiment/report.md).

### Scene-aware Surface limits

- Surface device creation computes the largest binding required by the resident
  scene, starts from portable `wgpu` defaults, and raises only
  the required storage/buffer limits when the adapter exposes that headroom.
- The request is deterministic: one representation, one exact capability
  request, one structured result. There is no retry loop or hidden fallback.
- Adapter limits are legality ceilings, not available-memory guarantees. Initial
  geometry allocation still needs validation/OOM error scopes to make memory
  pressure failures as actionable as capability preflight.
- The retained A065 Vulkan evidence caps full-f32 resident degree-3 SH scenes at
  745,654 splats because the physical storage-binding limit is 128 MiB. See
  [`2026-07-22-adaptive-storage-limits/report.md`](../docs/plans/completed/2026-07-22-adaptive-storage-limits/report.md).

## Strategic Execution Sequence

### 1. GPU-visible compaction and indirect drawing

Build a resident-scene experimental preprocess stage that:

- performs GPU frustum/footprint visibility evaluation;
- compacts visible source IDs and depth keys without CPU readback;
- writes sort-dispatch and draw-indirect arguments from the compacted count;
- records into the caller-owned frame encoder and shares the existing resident
  scene buffers and shader contract;
- preserves CPU ordering as the default and deterministic fallback.

Promotion evidence must cover empty scenes, dispatch tails, near/far and
screen-edge cases, degenerate covariance, duplicate-depth tie behavior,
swapchain timeout/retry behavior, CPU image parity, and Web/WASM portability.

### 2. Portable production-candidate GPU ordering

Evaluate a compacted-input GPU sorter separately from the current baseline:

- compare a four-pass 8-bit radix or another portable parallel-prefix design
  against the existing eight-pass 4-bit implementation;
- retain deterministic back-to-front order with ascending source-ID tie order;
- sort only the compacted visible count and consume the result in the resident draw;
- keep pipeline creation outside measured frames and preserve same-frame CPU
  fallback on execution errors;
- feed the candidate through existing Adaptive telemetry rather than adding a
  repository-wide point-count threshold.

No GPU backend becomes the default from a kernel microbenchmark alone. Promotion
requires end-to-end paired results and image gates on representative Android,
Apple, desktop, and browser adapters.

### 3. Compressed resident storage profiles

Prototype explicit, capability-gated resident profiles:

- retain the current full-f32 profile as the quality reference;
- evaluate f16 and normalized-i8 SH storage with GPU-side SH evaluation;
- evaluate covariance representation only as a separate measured choice;
- keep profile selection explicit and out of the stable C ABI initially;
- report source, CPU, and GPU bytes separately and validate requested binding
  sizes before allocation.

Each profile needs real-scene SSIM/error analysis, capacity measurements,
first-frame cost, sustained frame behavior, and cross-backend shader coverage.
Compressed resident storage may extend capacity, but it is not streaming
and must not be described as such.

### 4. Internal render-stage decomposition

Implement new GPU work through narrow internal stages rather than growing
another platform-specific state machine:

- a preprocess stage owns visibility, compaction, keys, and indirect arguments;
- an ordering stage owns CPU/GPU backend encoding and deterministic fallback;
- a draw stage owns resident render-pass encoding;
- `SurfaceRenderSession` continues to own scheduling, dirtiness, camera
  revisions, presentation policy, and telemetry.

Stages may accept internal `wgpu` resources and a caller-owned encoder, but raw
`wgpu` types do not enter the stable C, JNI, Swift, or Web package contracts.
Do not create a new crate until the existing `gsplat-render-wgpu` boundaries are
proven insufficient.

### 5. Evidence before policy or API promotion

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
- SPZ product integration: select and verify a real desktop, native, or Web
  consumer before widening public APIs.

## Explicitly Not Active Right Now

- A custom internal binary scene/cache format
- Reintroducing the retired Packed/Paged modes
- Metadata-first or remote streaming before the resident GPU pipeline and
  real-dataset evidence matrix are established
- Additional experimental blending/rendering backends
- A public raw-`wgpu` C, JNI, Swift, or Web API
- New top-level apps, crates, or docs-only placeholders without an explicit
  release-boundary reason
- Published Maven, binary SwiftPM, npm, or crates.io distribution
