# Progress: Adaptive Direct Ordering Experiment

## 2026-07-22

### Setup and scope

- Created an unbudgeted goal: no fixed FPS, speedup, token, or time gate.
- Created `codex/adaptive-sort-experiment` from exact `origin/main`
  `68af377b3c47bc578d81efff4a117dc893d845e1`.
- Installed/verified Android command-line tools, API 35, build-tools 35.0.0,
  NDK 29.0.14206865, Java 21, and Rust `aarch64-linux-android`.
- Identified the connected target as Nothing A065 / Android 15 / SM8475 /
  Adreno 730, with a 716x1600 benchmark Surface and initial thermal status 0.
- Confirmed the previous Direct path used CPU near/far candidates, stable CPU
  radix ordering, compact ID upload, and Direct GPU projection/draw. The old
  4,096-item odd-even GPU test backend was unsuitable for production evidence.

### Dataset infrastructure

- Added deterministic fixed-record PLY tier generation and official INRIA
  range download/provenance tooling with six unit tests.
- Fetched local Kitsune (279,199), Flowers (562,974), Bonsai (1,244,819), and
  Truck (2,541,226) models.
- Generated official Truck scaling tiers at 50k, 100k, 200k, 300k, 500k, 700k,
  750k, 1M, 1.5M, and 2M. External files remain ignored; INRIA pretrained model
  redistribution remains prohibited until rights are clarified.

### Renderer and policy implementation

- Added portable stable GPU key generation plus 8×4-bit radix ordering on the
  renderer device. The path performs 25 compute passes, keeps pairs on GPU, and
  uses sort-all/draw-all.
- Added device/dispatch preflight, validation scopes, timeout submission, same-
  frame CPU fallback, actual-backend telemetry, and resource prewarming.
- Added `Cpu`, `Gpu`, and `Adaptive` Surface order selection. CPU remains
  default; GPU/Adaptive are Direct-only and incompatible with `AsyncLatest`.
- Added separate rolling refresh/reuse p75 windows, cadence scoring, bounded
  probes, hysteresis, emergency demotion, periodic re-probe, loss backoff, and
  GPU-error cooldown.
- Fixed reuse-frame backend attribution, long-interval cost hiding, double
  cooldown, non-atomic schedule updates, first-probe allocation jank, fallback
  wall accounting, and wasm/doc warnings found during review.
- Added GPU unit coverage for empty and boundary sizes, duplicate/extreme keys,
  tail poison, dispatch arithmetic, and 257-source end-to-end key generation.

### Android benchmark path

- Kept the public Android AAR `NativeBridge` unchanged. Added an example-only
  `BenchmarkBridge` and an internal native benchmark setter absent from the C
  header.
- Reused sort-stat flag bits for actual backend/fallback/Adaptive state and
  made unavailable GPU phase timings `null` in canonical artifacts.
- Added the paired collector with thermal gating, exact dataset/APK/native
  identity, full logs, fresh outputs, canonical extraction, and fail-closed
  validation.
- Package-manager installs became unreliable after repeated development
  iterations. Replaced per-tier installs with exact installed-APK validation
  and one-push/run-as-copy dataset injection. Verified internal PLY SHA/bytes
  before every run and cleaned only exact temporary paths.
- Found Android `logd` could drop a burst from a 120-frame artifact on one 500k
  run. The validator rejected it. Changed collector default to 80 measured
  frames; the failed run is excluded from all results.

### Correctness and capacity

- Recaptured stable CPU/GPU 50k screenshots. The rendered crops were byte-
  identical (SHA-256
  `91dec5b0cf4a5adc6070f448fbc0c95f39be2177db51f22e512c1d0e3126b9ee`,
  PSNR infinite, SSIM 1.0) within the recorded single-view boundary.
- Confirmed 700k Direct/SH3 loads and benchmarks. 750k, 1M, 1.5M, and 2M return
  `unsupported` during renderer creation. The calculated Direct limit is
  745,654 splats from the 128 MiB binding limit / 180-byte SH-rest stride.

### Retained Android evidence

- Interval 1: six ladder tiers, 36 runs, 2,160 measured frames.
- Interval 2: three representative tiers, 18 runs, 1,080 measured frames.
- Adaptive 80-frame series: three tiers, 9 runs, 720 measured frames.
- Full Kitsune/Flowers anchors: 12 runs, 720 measured frames.
- 200k periodic re-probe series: 3 runs, 360 measured frames.
- Total: 78 retained runs / 5,040 measured frames / 1,560 warmup frames.
- All retained thermal readings were status 0; all used APK
  `d3ad6c...48b1` and native library `9cf918...a148`; all forced GPU fallback
  counters were zero.
- Forced GPU lost every paired ladder comparison. Adaptive consistently probed
  GPU and returned to `cpu_stable`.

### Closeout

- Added `report.md` and `android-results.json` with exact claim boundaries,
  aggregate values, capacity evidence, policy mechanics, and next priorities.
- Rebuilt the Android release native library from final source and reproduced
  the experiment SHA-256 exactly (`9cf918...a148`); the sample APK also built
  successfully with the tiny bootstrap fixture.
- Passed final workspace tests, required Metal conformance, formatting,
  Clippy, rustdoc warnings, Web/Wasm compilation, dependency policy, C FFI and
  Android JNI smoke tests, Android artifact extraction, dataset manifests,
  camera trace, 18 collector tests, and 6 dataset-tool tests.
- A final independent read-only audit found no P0/P1 issues. Its four P2
  documentation findings were corrected before archive; no unrelated files,
  large tracked artifacts, or credentials were found.
- Archived the completed bundle. Commit, merge, and push are represented by
  the repository history rather than retroactively editing this record.
