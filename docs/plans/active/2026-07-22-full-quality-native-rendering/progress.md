# Progress: Full-Quality Native Rendering

## 2026-07-22

- Created goal and independent branch `codex/full-quality-native-rendering`
  from clean `main` at `28f77d0`.
- Read all canonical architecture, roadmap, taste, verification, Android,
  Apple, Web, and example integration documentation required by `AGENTS.md`.
- Confirmed host Rust toolchain and initial desktop/Apple runtime inventory.
- Confirmed the existing local dataset ladder covers 50k, 100k, 200k, 300k,
  500k, 700k, 750k, 1M, 1.5M, 2M, and complete 2.541M Truck, plus complete
  Kitsune, Flowers, and Bonsai anchors.
- Started parallel read-only audits for renderer layout/SH, exact loader memory
  lifecycle, and complete CPU/GPU ordering/resource limits.
- Built, installed and ran a fresh Android baseline APK with the complete Truck
  file; captured exact counts, frame phases, thermal state and screenshot.
- Ran the complete Truck Packed baseline on Mac Metal and captured timings,
  peak memory and screenshot.
- Completed audits of CPU/GPU ownership, SH refresh, binding limits, sort
  scratch and PLY/SPZ load peaks. Verified competitor packed layouts and recent
  GPU-sort approaches against primary sources.
- Locked `design.md`: exact float32 positions, chunk-local compact attributes,
  degree-specific split SH buffers, coherent GPU SH resolve, one draw path for
  CPU/GPU order, portable hierarchical radix, and explicit capacity failure.
- Added the Packed Surface upload ownership boundary: all compact upload planes
  are validated and kept through GPU resource creation, then released only at
  the final `SurfaceRenderSession` handoff. Exact positions, count/SH/error
  receipts, and CPU sort workspaces remain; failed construction/validation and
  unsupported path switches are transactional. SH3 release receipts are
  194,245,000 B for Truck, 445,996,400 B for Garden, and 468,711,240 B for
  Bicycle.
- Started implementation in parallel: exact bounded PLY loading and shared
  cross-platform camera-trace controls.
- Added a shared v1 camera-trace sequence contract with explicit selected frame
  indices, warmup, measured-frame count, and loops. Desktop, Android, iOS, and
  Web now apply the exact trace camera before every measured render; fixed-frame
  mode remains available for screenshot comparisons.
- Verified the default `0 -> 1 -> 2` trace sequence with timestamps
  `0 -> 16666667 -> 33333334` and `sort_interval=1` on desktop Metal, a Nothing
  A065 Android device using Vulkan, the iOS simulator using Metal, and Headless
  Chrome using WebGPU. Every endpoint reported `drawn=3/3`, requested CPU order,
  and a three-sample benchmark artifact; Android also reported a fresh sort and
  zero presented-order revision lag for every changed camera revision.
- Next: implement and image-gate the ResidentCompact codec/GPU path, then wire
  complete GPU ordering and adaptive timing before the cross-platform ladder.

## 2026-07-23

- Completed the exact Resident SH0--SH3 implementation, transactional
  capacity admission, PLY direct-to-Resident loading, complete CPU/GPU order,
  full-frame-completion Adaptive policy, and the shared four-vertex
  opacity-bounded ProjectedQuadsExact path.
- Rejected Android radix-256 after full-count telemetry masked a corrupt sparse
  image; retained full-32-bit base16 on Android and target-qualified radix8 plus
  visible compaction on Metal.
- Established formal resolutions: 1920x1080 Mac/Web, native 2412x1080 Nothing
  A065, and 2622x1206 iOS simulator. `640x360` is now diagnostic-only evidence.
- Completed current Android Truck CPU/GPU/Adaptive 3x at 2412x1080. Median
  frame wall is 179.335 / 203.805 / 183.350 ms; every Adaptive run ends
  `cpu_stable` with 69 CPU / 11 GPU measured frames.
- Completed exact Android 200k and 1M current-code anchors. At 200k CPU queue
  completion is 10.42--10.44 ms versus GPU 17.60--18.09 ms, confirming the
  original CPU choice on this A065. Preserved the earlier complete 50k--2M
  ladder as directional evidence; CPU wins every rung.
- Fixed the Android chunked-summary collector completion race. Excluded the
  failed `200000/` collection and accepted only the complete `200000-v2/`
  rerun.
- Loaded and rendered complete Garden (5.835M) and Bicycle (6.132M) at
  1920x1080 on Mac Metal and Chrome/WebGPU. Web Bicycle GPU completion is about
  67.6 ms versus CPU 130.6 ms; the 80-frame Adaptive window uses 63 GPU and 17
  CPU frames and keeps GPU as incumbent.
- Completed current iOS-simulator Truck CPU/Adaptive exactness at 2622x1206;
  screenshots are byte-identical. Forced GPU fails explicitly as unsupported,
  so no simulator GPU performance is claimed.
- Reclassified the pinned PlayCanvas 56.5846 FPS Truck result as a valid
  competitive throughput/architecture reference but not a strict equal-quality
  denominator: it uses about 20 depth bits, quantized work attributes, fp16
  projected axes/color, and a default SH color-update threshold. The recorded
  same-camera images are not pixel equivalent.
- Still open: final-code Mac/Web ladders, sustained Garden/Bicycle cohorts and
  Android capacity runs, a physical iPhone, desktop five-stage artifact fields,
  final repository verification, and the branch commit.
- Defined the revision-safe `S/V/C/D` evidence contract in
  `exact-contributor-evidence.md`. CPU and GPU successes now carry the same
  candidate/contributor/issued counts, joined by ticket plus camera revision;
  exact compaction permits `D=C`, while Direct/downlevel and legacy evidence
  retain `D=V`. Validators reject partial/mixed declarations and stale-frame
  substitutions.
