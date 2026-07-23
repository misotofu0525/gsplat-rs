# Task Plan: Full-Quality Native Rendering

## Goal

Redesign the shared native Gaussian-splat rendering flow so every supported
surface can load and render the complete source splat set, from tiny fixtures
through multi-million-splat SH0-SH3 scenes, without sampling, geometry
decimation, or a silent quality fallback. Preserve CPU and GPU ordering as
first-class implementations and select between them from measured runtime
behavior rather than a fixed point-count threshold.

## Non-negotiable contract

- The implementation remains native Rust plus the shared `wgpu` renderer.
- `SortedAlpha` remains the quality contract. Every visible source Gaussian is
  sorted and rendered; frustum/near/far rejection may skip only mathematically
  invisible work and must never change the loaded source count.
- No fixed subset, random thinning, incomplete page cut, SH downgrade, or
  reduced-resolution substitute may be reported as full-quality rendering.
- A device that cannot allocate the exact scene must return an explicit,
  actionable capacity error. It must not produce a sparse image.
- CPU and GPU ordering must produce the same source-ID order semantics and the
  same image. Adaptive mode learns from the current device and workload and
  retains periodic re-evaluation.
- Desktop Metal, browser WebGPU, Android hardware, and the available Apple
  runtime are verified with the same datasets, cameras, and artifact schema.

## Phases

### 1. Freeze the baseline and exact contract

- [x] Create `codex/full-quality-native-rendering` from clean `main`.
- [x] Read project context, architecture, verification, roadmap, taste guide,
  and every affected platform README.
- [x] Record current adapter limits, connected devices, exact datasets, source
  hashes, renderer paths, memory preflight, images, and performance artifacts.
- [x] Turn the existing point-count ladder into a cross-platform experiment
  matrix, including complete Kitsune, Flowers, Bonsai, Truck, Garden and
  Bicycle anchors. The matrix is evidence-driven rather than a needless full
  Cartesian product of every scene, backend and device.

### 2. Reconcile local architecture with competitor evidence

- [x] Audit Direct/Packed/Paged resource ownership, SH evaluation, ordering,
  load-time peaks, bindings, and platform adapters.
- [x] Verify competitor resident layouts, full-scene limits, visibility
  compaction, sorting, and explicit failure behavior against primary sources.
- [x] Write exact per-splat and per-scene CPU/GPU memory equations, including
  sort scratch, uploads, alignment, and peak phases.
- [x] Lock a single cross-platform quality/capability contract before coding.

### 3. Implement the compact exact resident scene

- [x] Introduce one render-owned compact scene representation for full-count
  geometry and SH0-SH3 data, with bindings split below negotiated limits.
- [x] Stream/transcode PLY directly to Resident without retaining duplicate
  wide float32 scene data after upload-ready data exists.
- [x] Preserve exact SPZ loading through the existing wide `SceneBuffers`
  bridge and record removal of that duplicate peak as explicit follow-up work;
  it is not misreported as completed peak-memory optimization.
- [x] Preserve sufficient position/attribute precision and prove image parity
  against Direct on scenes where Direct fits.
- [x] Make capacity admission use actual device limits and phase-correct peak
  accounting; fail closed before partial installation.

### 4. Unify complete CPU/GPU ordering

- [x] Make CPU ordering consume the compact scene without reconstructing all
  other attributes.
- [x] Replace the serial large-count GPU radix bottleneck and keep stable,
  deterministic source-ID ordering for complete visible sets.
- [x] Add visibility compaction and indirect draw only if it preserves exact
  visible-set semantics and wins in measured end-to-end results.
- [x] Retune Adaptive using separately measured refresh/reuse behavior,
  bounded probes, hysteresis, failure cooldown, and periodic re-evaluation.

### 5. Cross-platform quality and performance experiments

- [x] Compare CPU, GPU, and Adaptive on the same fixed cameras at increasing
  point counts; retain raw frames and complete benchmark artifacts.
- [x] Compare compact rendering with the Direct oracle by image diff/SSIM at
  SH0-SH3 and multiple views, including outlier-heavy Truck framing.
- [x] Run desktop Metal, WebGPU/WASM, Android hardware, iOS simulator, and any
  discoverable physical Apple device. Label simulator/build-only evidence
  honestly where hardware performance is unavailable. No physical iPhone or
  usable signing identity was available, so no phone-performance claim exists.
- [x] Record load time, peak/steady CPU and GPU bytes, sort refresh time,
  render/submit time, frame wall, thermal state, failures, and screenshots.

### 6. Close out only after the evidence is complete

- [x] Fix every correctness, capacity, quality, and cross-platform regression
  found within the retained scope, including Adaptive probe starvation
  (`28f79ee`) and the tiled raster test's incorrect mandatory-timestamp
  assumption (`76a9267`).
- [x] Run all relevant repository verification gates from
  `handbook/VERIFICATION.md`.
- [x] Archive this bundle with design, implementation, exact results, known
  limits, and reproduction commands.
- [x] Review and commit the intentional changes on the independent branch.

## Completion definition

The task is complete only when the implementation renders the complete Truck
source count without a sparse/partial fallback on every endpoint whose
negotiated memory capacity admits it, smaller scenes retain Direct-equivalent
quality, both ordering backends are correct, Adaptive is measured rather than
threshold-driven, all available endpoints have fresh evidence, and the branch
is committed. A platform capacity rejection is acceptable only when it is
explicit and supported by exact resource math; an incomplete image is not.

## Terminal evidence matrix

| Evidence cell | Terminal result | Boundary |
| --- | --- | --- |
| Truck / Mac / 1920x1080 | Full SH3, Direct image gate; CPU/GPU/Adaptive; exact 6-pair PostSort/Preproject A/B, Preproject mean-completion ratio `0.83405` | PostSort remains default pending composite-plan policy |
| Truck / Chrome/WebGPU / 1920x1080 | Full five-stage counts; CPU/GPU/Adaptive; exact 4-pair Producer A/B, median ratio `0.87797` | Browser cohort is M4 Chrome, not every WebGPU adapter |
| Truck / Nothing A065 / 2412x1080 | Full five-stage counts; CPU beats GPU/PostSort; fixed Adaptive exits learning; descriptive Producer A/B ratio `0.53217` favors Preproject | Producer A/B pairing metadata is null; no universal default claim |
| Truck / iOS simulator / 2622x1206 | CPU/Adaptive exact and byte-identical; forced GPU explicitly unsupported | No physical iPhone/signing identity was available; no phone timing claim |
| Garden + Bicycle / Mac / 1920x1080 | Complete source/resident SH3 and exact view draw counts | Short capacity/runability cohorts, not sustained FPS |
| Garden + Bicycle / Chrome/WebGPU / 1920x1080 | Complete five-stage counts; Bicycle Adaptive chooses GPU | Short Garden and longer Bicycle evidence only |
| Garden + Bicycle / Nothing A065 / 2412x1080 | Complete 5.835M/6.132M SH3 scenes, thermal 0 | Four measured CPU frames each; capacity evidence only |
| PlayCanvas Truck / Mac / 1920x1080 | 600 terminal frames, 56.5846 FPS, full active source set | Different precision/SH-update profile and no actual contributor count |
| PlayCanvas Truck / A065 / 2412x1080 | 80-frame queue-terminal cadence 73.821 ms/frame, full active source set | Qualified directional reference; dirty harness, no exact V/D or per-frame GPU timing |

`640x360` remains a diagnostic fixture only. Formal resolutions are 1920x1080
on desktop/Web, native 2412x1080 on the connected Android device, and
2622x1206 on the available iOS simulator, always with requested = Surface =
internal = presented and dynamic resolution/upscaling disabled.

Progress is evidence-driven rather than blocked by one synthetic hard gate.
Each experiment ends in one of three terminal states: accepted with complete
receipts, rejected with a recorded reason, or explicitly open because the
required device/evidence is unavailable. A failed or partial artifact is never
re-run indefinitely as if repetition could make it valid; fix the collection
or implementation cause once, rerun with a new artifact identity, and keep the
invalid directory excluded. The failed
`post-parallel-android-adaptive-ladder-2412x1080-20260723/200000/` collection is
therefore excluded, while its complete `200000-v2/` rerun is accepted.
