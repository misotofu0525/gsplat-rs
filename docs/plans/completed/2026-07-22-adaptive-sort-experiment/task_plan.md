# Task Plan: Adaptive Direct Ordering Experiment

## Goal

Replace the roughly 200k mobile CPU-sort assumption with reproducible evidence,
build a cross-platform point-count ladder, add a deterministic GPU ordering
baseline and safe runtime selection, verify the connected Android target, and
close the work without turning one device's result into a universal threshold.

## Release boundary

- `SortedAlpha` / Direct remains the only release-gated render path.
- CPU remains the default order backend.
- GPU and Adaptive are opt-in Direct strategies and cannot use CPU
  `AsyncLatest`.
- The v0.1 C ABI layout and Android AAR public API remain unchanged. Existing
  sort-stat flag bits carry additional telemetry; the Android selector is
  example-only.
- The experimental Rust API is intentionally widened with backend/state/output
  telemetry. It is not described as a stable pre-1.0 contract.
- Active-atlas/paging release work, tile rasterization, training, streaming,
  and a competitor side-by-side benchmark remain separate tasks.

## Completed phases

### 1. Establish the experiment contract

- [x] Install and verify Rust/Android dependencies and reconnect device
  `033ed212`.
- [x] Pin device, display, APK, native library, trace, thermal, and artifact
  identity.
- [x] Build deterministic 50k/100k/200k/300k/500k/700k scaling tiers and
  750k/1M/1.5M/2M capacity probes from official INRIA Truck data.
- [x] Retain Kitsune and Flowers as independent full-model anchors.
- [x] Label source provenance and redistribution constraints; keep external
  data ignored.

### 2. Build a measurable GPU ordering path

- [x] Reuse the renderer device, source buffers, encoder, and submission.
- [x] Add deterministic key generation and stable 8-pass 4-bit radix sorting.
- [x] Feed final GPU pairs directly to the Direct draw without runtime
  readback.
- [x] Preflight dispatch/resource limits and synchronously fall back to CPU on
  a returned GPU error.
- [x] Add GPU order unit tests for boundaries, duplicate keys, tails, and an
  end-to-end key-generation/sort case.

The implemented path is sort-all/draw-all with a normal instance draw. It is
not compacted, indirect, or presented as a production-optimal GPU sorter.

### 3. Measure the Android ladder

- [x] Add a collector with exact APK/native/data identity, run-as dataset
  injection, randomized pairs, thermal gating, complete logs, and canonical
  artifact validation.
- [x] Run 6 interval-1 ladder tiers, 3 interval-2 tiers, 3 Adaptive tiers, 2
  full-scene anchors, and a longer periodic re-probe series.
- [x] Retain 78 valid runs / 5,040 measured frames; reject a log-incomplete run
  instead of repairing it.
- [x] Probe the Direct capacity boundary at 700k/750k/1M/1.5M/2M.
- [x] Record the CPU/GPU image check boundary precisely.

### 4. Implement and verify Adaptive

- [x] Separate order freshness from backend selection.
- [x] Track CPU/GPU refresh and reuse p75 windows independently and combine
  them by configured cadence.
- [x] Add bounded bootstrap/probes, hysteresis, emergency demotion, periodic
  reconsideration, exponential backoff, and GPU-error cooldown.
- [x] Attribute reuse frames to the actually presented backend.
- [x] Validate configuration atomically and test interval/backoff/fallback
  behavior.
- [x] Verify on Android that this device is probed, rejected for GPU, and
  returns to `cpu_stable` without fallback.

Adaptive has no explicit thermal input and is not an EWMA. Its constants are
fixed; it adapts from measured frame-wall behavior at bounded probe points.

### 5. Close out

- [x] Write the detailed report and machine-readable result summary.
- [x] Run final repository/platform verification after documentation closeout.
- [x] Perform final diff review; resolve all four documentation P2 findings,
  with no P0/P1 findings remaining.
- [x] Archive this bundle and prepare the verified branch for commit, merge to
  `main`, and push under the user's original authorization.

## Deferred evidence, not hidden failures

- [ ] Run the same tier matrix on iOS/Metal, desktop, and browser WebGPU.
  Shared code and data compile/test evidence is not runtime performance proof.
- [ ] Add timestamp-query/GPU-complete telemetry. Current GPU phase values are
  unavailable; end-to-end frame wall is the product metric.
- [ ] Demonstrate Adaptive choosing GPU on a device where GPU wins. Unit tests
  cover the branch; this Android device consistently favors CPU.
- [ ] Turn packed/paged geometry into a release candidate for million-point
  mobile scenes.
- [ ] Compare against competitor renderers under the same scenes/cameras.

## Evidence

- Detailed conclusions: `report.md`
- Aggregated values: `android-results.json`
- Raw local artifacts:
  - `target/android-sort-benchmarks/retained-v2-i1-*`
  - `target/android-sort-benchmarks/retained-v2-i2-*`
  - `target/android-sort-benchmarks/retained-v2-adaptive80-i2-*`
  - `target/android-sort-benchmarks/retained-v2-adaptive-i2-200k`
  - `target/android-sort-benchmarks/retained-v2-anchor-i1-*`
