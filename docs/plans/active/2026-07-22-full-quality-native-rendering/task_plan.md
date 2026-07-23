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
- [ ] Turn the existing point-count ladder into a cross-platform experiment
  matrix, including complete Kitsune, Flowers, Bonsai, and Truck scenes.

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
- [ ] Stream/transcode PLY and supported SPZ inputs without retaining duplicate
  wide float32 scene data after upload-ready data exists. PLY is direct to
  Resident; SPZ still passes through wide `SceneBuffers`.
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

- [ ] Compare CPU, GPU, and Adaptive on the same fixed cameras at increasing
  point counts; retain raw frames and complete benchmark artifacts.
- [ ] Compare compact rendering with the Direct oracle by image diff/SSIM at
  SH0-SH3 and multiple views, including outlier-heavy Truck framing.
- [ ] Run desktop Metal, WebGPU/WASM, Android hardware, iOS simulator, and any
  discoverable physical Apple device. Label simulator/build-only evidence
  honestly where hardware performance is unavailable.
- [ ] Record load time, peak/steady CPU and GPU bytes, sort refresh time,
  render/submit time, frame wall, thermal state, failures, and screenshots.

### 6. Close out only after the evidence is complete

- [ ] Fix every correctness, capacity, quality, and cross-platform regression
  found by the experiment matrix.
- [ ] Run all relevant repository verification gates from
  `handbook/VERIFICATION.md`.
- [ ] Archive this bundle with design, implementation, exact results, known
  limits, and reproduction commands.
- [ ] Review and commit the intentional changes on the independent branch.

## Completion definition

The task is complete only when the implementation renders the complete Truck
source count without a sparse/partial fallback on every endpoint whose
negotiated memory capacity admits it, smaller scenes retain Direct-equivalent
quality, both ordering backends are correct, Adaptive is measured rather than
threshold-driven, all available endpoints have fresh evidence, and the branch
is committed. A platform capacity rejection is acceptable only when it is
explicit and supported by exact resource math; an incomplete image is not.

## Current terminal evidence and remaining cells

| Evidence cell | Current terminal result | Still required |
| --- | --- | --- |
| Truck / Mac / 1920x1080 | Full SH3, Direct gate passes; current CPU median 43.321 FPS | Current-binary forced GPU/Adaptive rerun |
| Truck / Chrome/WebGPU / 1920x1080 | Full five-stage counts; CPU/GPU/Adaptive complete | Final-code point ladder |
| Truck / Nothing A065 / 2412x1080 | Full five-stage counts; current CPU/GPU/Adaptive 3x; CPU wins | Garden/Bicycle success or capacity receipt; final-code missing ladder rungs |
| Truck / iOS simulator / 2622x1206 | CPU/Adaptive exact and byte-identical; forced GPU explicitly unsupported | Physical iPhone performance |
| Garden + Bicycle / Mac / 1920x1080 | Complete source/resident SH3 and exact view draw counts | Sustained cohorts; desktop decoded/encoded/addressable fields |
| Garden + Bicycle / Chrome/WebGPU / 1920x1080 | Complete five-stage counts; Bicycle Adaptive chooses GPU | Sustained multi-run cohorts; Garden GPU/Adaptive |
| PlayCanvas Truck / 1920x1080 | 600 terminal frames, 56.5846 FPS, full active source set | Explicit matched precision/SH-update profile and actual contributor count |

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
