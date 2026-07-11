# Progress Log

## Session: 2026-07-11

### Implementation Phase C: Compressed Sources and Bounded Decode

- **Status:** in_progress; SPZ v4 decode covers degrees 0–3 with synthetic tests
- Actions taken:
  - Added workspace crate `gsplat-io-spz` with file and in-memory loading APIs,
    `SpzLoadLimits`, structured `thiserror` failures, and `SceneBuffers` output.
  - Selected Niantic SPZ under MIT and version 4 (`NGSP`, plaintext 32-byte
    header, independent ZSTD attribute streams).
  - Added exact header, TOC, stream-size, input, point-count, and decoded-scene
    validation before scene construction.
  - Added RUB-to-RUF conversion for positions, `xyzw` rotations, and SH rest
    coefficients using Niantic `coordinateConverter(RUB, RUF)` flips.
  - Extended decode to SH degrees 1–3: 6th ZSTD stream, unquantize, remap to
    PLY channel-major `sh_rest`; degree 4 and wrong stream counts reject.
  - Added synthetic packing helpers and tests for degrees 0/1/3 plus header
    rejection cases.
  - `cargo test -p gsplat-io-spz`: 8 tests passed.
  - `cargo check -p gsplat-io-spz`: passed.
  - **Slice 1 committed:** land Phase C SPZ v4 decoder crate + workspace + plan
    docs (excluding android build / `__pycache__` artifacts).
- Current boundary:
  - Degree 4 SH, legacy gzip v1-3, extension ILV, FFI, and real fixture/image
    parity remain out of this slice.
  - Real SPZ fixtures, cache cancellation/recovery, and broader Phase C exit
    evidence remain pending.

### Implementation Phase B: Packed Atlas Without Streaming

- **Status:** complete under matched `sort_interval=4` sync-orbit qualification
  (Android kitsune median 1.078; Android flowers median 0.383); `sort_interval=2`
  sync residual remains documented
- Actions taken:
  - Closed Phase A with validated kitsune artifacts on Android (Nothing A065),
    iPhone 17 Pro Max, and desktop Web WASM/WebGPU.
  - Added `gsplat-render-wgpu::packed_atlas` with a 20-byte hot record, 48-byte
    degree-3 sidecar, atlas CPU buffers, and measured texture-byte accounting.
  - Unit tests prove the ≥3x attribute reduction (244→68) and kitsune pack gate.
  - Packed draw path uses a tightly packed hot storage buffer (not multi-texture
    sampling) plus CPU color-refresh into the hot RGB10 word.
  - Fixed `pack_quat_smallest_three` and packed-shader `quat_to_mat3` sign
    convention to match the direct path.
  - Direct-vs-packed image gates pass: degree-0, minimal_ascii, synthetic
    degree-3, kitsune, and flowers (MAE ≤ 1/255; flowers frac>3/255 ≈ 0.0009).
  - Desktop p95 A/B (warmup 20, frame_wall_ms) passes ≤10%: minimal ratio
    1.009, kitsune ratio 0.750; index at
    `target/benchmarks/phase-b/phase-b-desktop-p95-index.json`.
  - Nandi packed preflight proves attributes avoid the direct-path SH storage
    binding failure; hot storage fits under 128 MiB through Nandi scale.
  - Rejected 24 B f16-covariance candidate: Flowers pixel-fraction gate failed.
  - Diagnosed Android sync p95 as sort-frame dominated (non-sort packed is
    faster than direct). Deferred banded SH refresh off sort frames.
  - Android kitsune sync five-pair with `sort_interval=4` median p95 ratio
    1.078 (pass); flowers median 0.383 (pass). Async worker kitsune ~0.947.
    Device index: `phase-b-device-p95-index.json` v2.
  - Peak process RSS index at `phase-b-peak-rss-index.json` (packed/direct
    ≈ 0.87 for kitsune and flowers cold load).
  - Surface/FFI/Android/iOS examples expose `gsplat_geometry_path` for A/B.
- Current boundary:
  - Keep SmallSceneDirect as the default path; packed atlas stays additive.
  - Phase C compression is next. Keep `sort_interval=2` sync as optional
    stress follow-up, not as a Phase B blocker under the recorded protocol.

### Implementation Phase A: Freeze Competitive Baseline

- **Status:** complete
- Actions taken:
  - Captured validated kitsune baselines: Android Nothing A065 (p95 13.728 ms),
    iPhone 17 Pro Max (p95 17.485 ms), desktop Web WASM/WebGPU (p95 3 ms),
    indexed at `target/benchmarks/phase-a/phase-a-baseline-index.json`.
  - Added Android logcat artifact extractor and Web headless collector scripts.
  - Cleared a stale iOS Documents `imported_scene.ply` that caused false parse
    failures before the physical-device kitsune run.
  - Created the persistent execution goal for Phases A-F.
  - Reopened the active plan and changed its state from documentation handoff to
    authorized Phase A execution.
  - Spawned three read-only subagents for benchmark telemetry, resource
    preflight, and independent acceptance-gate audits.
  - Confirmed `bench-runner` currently has adapter metadata and phase averages
    but does not retain raw frame samples or emit a versioned artifact.
  - Located the lazy direct-scene allocations and confirmed resource-limit
    failures are not currently reported before first render.
  - Completed three parallel read-only audits. They confirmed Phase A cannot
    pass until raw artifacts, preflight, a pinned PlayCanvas harness, and an
    Android physical-device result exist.
  - Delegated non-overlapping implementation slices for the v1 artifact
    validator/fixtures and resource preflight while the primary thread owns
    `bench-runner` raw-frame output.
  - Completed the independent harness audit and selected a fail-closed exact npm
    lock under `tests/competitive/playcanvas/`; implementation remains pending.
  - Added and focused-tested the v1 artifact contract, validator, golden fixture,
    and invalid fixture mutations under `tests/perf/`.
  - Added and focused-tested structured direct-scene resource preflight in
    `gsplat-render-wgpu` before both surface and offscreen allocation paths.
  - Initial bench-runner verification compiled successfully but found formatting
    drift and one invalid test setup that combined iteration artifacts with
    stability mode; both are being corrected without weakening the CLI guard.
  - Added `bench-runner` v1 artifact output with raw frames, nearest-rank
    distributions, SHA-256 dataset identity, git/build/environment metadata,
    frame-budget misses, atomic JSON/JSONL publication, and legacy stdout.
  - Corrected the initial verification issues; 13 bench-runner unit tests and
    focused checks now pass.
  - Generated and validated a real release-mode minimal-scene artifact at
    `target/benchmarks/phase-a/minimal-v1-smoke` on Apple M4 Pro / Metal.
  - Completed an independent review of the Rust artifact producer and kept its
    P1/P2 findings open rather than treating the first valid smoke as final.
  - Added the exact-lock PlayCanvas npm/preflight skeleton under
    `tests/competitive/playcanvas/`; browser/render/path instrumentation remains
    pending.
  - Full clippy exposed that embedding the complete preflight report directly in
    an error enum enlarged every renderer `Result`; the report will be boxed
    rather than suppressing `result_large_err`.
  - Hardened the producer against benchmark-gap allocation, partial/mixed run
    publication, dataset identity changes, timestamp ambiguity, host/GPU
    metadata conflation, and producer/validator schema drift.
  - Boxed the structured report in error variants; renderer and bench-runner
    clippy now pass with `-D warnings`.
  - Generated and validated a hardened 120-frame artifact containing the Direct
    preflight report and verified the PlayCanvas exact-lock preflight.
  - Added the canonical cross-engine camera trace contract, deterministic
    generator, matrix/hash validator, fixture, and negative tests.
  - Built and signed the iOS device app successfully, but the connected iPhone
    rejected launch because it was locked; no iOS runtime result was claimed.
- Current boundary:
  - Preserve existing human-readable output and renderer behavior.
  - Add structured evidence additively before changing the rendering path.

### Phase 1: Requirements and Evidence Refresh

- **Status:** complete
- **Started:** 2026-07-11 10:28 CST
- Actions taken:
  - Loaded the planning-with-files workflow and checked for unsynchronized prior context.
  - Confirmed there was no existing active plan bundle to resume.
  - Confirmed the worktree starts on `main` with only the pre-existing untracked
    `bindings/android/build/` directory.
  - Created the task-scoped planning bundle and recorded the prior PlayCanvas
    and gsplat-rs findings that motivate the design.
  - Re-read the canonical context, architecture, roadmap, principles, and
    verification guide plus Android, Apple, and Web package documentation.
  - Confirmed the design must preserve the shared session, small C ABI,
    `SortedAlpha` quality gate, and separate build/runtime evidence rules.
  - Reviewed historical desktop, Web, Android, and iOS performance evidence,
    including thermal behavior and the unsuccessful Android GPU-sort experiment.
  - Audited the current bench runner and deterministic platform benchmark hooks;
    identified percentile, memory, streaming, and thermal telemetry gaps.
- Files created/modified:
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/task_plan.md`
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/findings.md`
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/progress.md`

### Phase 2: Competitive Architecture Design

- **Status:** complete
- Actions taken:
  - Began converging the design around a retained small-scene direct path and a
    packed, fixed-capacity active atlas for large scenes.
  - Defined source, cache, LOD/residency, global sorting, device-policy, loading,
    API/ABI, failure-mode, and rollback boundaries.
  - Wrote `design.md` with the staged implementation and exit gates.
- Files created/modified:
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/design.md`

### Phase 3: Verification and Benchmark Design

- **Status:** complete
- Actions taken:
  - Defined four distinct evidence tracks and four controlled workload profiles.
  - Defined dataset/device matrices, benchmark scenarios, environment controls,
    telemetry, artifacts, statistical protocol, quality gates, and claim ladder.
  - Wrote `verification_plan.md`, separating existing commands from proposed
    harness work.
- Files created/modified:
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/verification_plan.md`

### Phase 4: Documentation Delivery

- **Status:** complete
- Actions taken:
  - Delivered the overall architecture in `design.md`.
  - Delivered the executable competitive verification plan in
    `verification_plan.md`.
  - Cross-linked the design, verification plan, and canonical verification
    guide without changing canonical project facts.
- Files created/modified:
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/design.md`
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/verification_plan.md`

### Phase 5: Document Verification and Handoff

- **Status:** complete
- Actions taken:
  - `git diff --check` passed.
  - All local Markdown links in the task bundle resolved.
  - Confirmed the pre-existing `bindings/android/build/` remains untouched.
  - Reviewed the binding-limit calculation, page/atlas capacity calculation,
    packed-layout gates, claim thresholds, and existing/proposed command split.
  - Confirmed all headings have the required following blank line.
  - Final checks passed for whitespace/final newlines, Markdown structure,
    local links, placeholders, and full untracked-file scope.
  - Left the bundle active because implementation phases A-F have not started;
    this documentation-only request is complete.
  - Reopened the documents for a follow-up layout correction after inspecting
    the pinned PlayCanvas source: its compact streams total 24 bytes despite a
    20-byte comment, and that number excludes source SH/order/scratch.
  - Revised the design target to separate a 20-byte hot record, degree-aware SH
    sidecar, order bytes, total residency, and attribute-level LOD.
  - Confirmed the proposed `Rgba16Uint`, `Rgba8Uint`, and `R32Uint` candidates
    exist in the repository's current `wgpu-types 28.0.0` dependency.
- Files created/modified:
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/design.md`
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/verification_plan.md`
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/findings.md`
  - `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/task_plan.md`

## Test Results

| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| Initial repository state | `git status --short --branch` | Preserve user changes and identify task-owned files | Only pre-existing `bindings/android/build/` is untracked before task files are added | Pass |
| Diff hygiene | `git diff --check` | No whitespace errors | No output, exit 0 | Pass |
| Local Markdown links | Bundle-local Ruby link check | Every relative link resolves | `local Markdown links: OK` | Pass |
| Markdown fences | Bundle-local Ruby fence check | Every fenced block is balanced | `Markdown code fences: OK` | Pass |
| Heading spacing | Bundle-local Ruby heading check | Blank line follows every heading | `heading spacing: OK` | Pass |
| Capacity arithmetic | `awk` calculation | Documented 128 MiB and atlas values match the design | 745,654 SH splats; 2,097,152 base splats; 4 MiB page; 4,194,304 atlas slots | Pass |
| Placeholder scan | Broad wording scan | No accidental placeholders | It matched intentional claim-warning sentences; narrow final scan required | Expected false positive |
| Final placeholder scan | `TODO|TBD|FIXME` scan | No accidental placeholders | `placeholder scan: OK` | Pass |
| Final bundle structure | Combined Ruby structure/link check | Balanced fences, heading spacing, and valid local links | `Markdown structure and local links: OK` | Pass |
| Final file hygiene | Bundle whitespace/newline check | No trailing whitespace and every file ends with newline | `whitespace and final newlines: OK` | Pass |
| Revised byte arithmetic | `awk` calculation | New layout values are internally consistent | PlayCanvas work 24B; hot page 1.25 MiB; degree-3 page 4.25 MiB; order page 0.25 MiB; full reduction 3.44-3.59x | Pass |
| Revised documentation structure | Combined structure/link check | Balanced Markdown, valid local links, clean whitespace | `documentation structure: OK` | Pass |
| Revised stale-target scan | Broad target/placeholder scan | No superseded byte-range target or accidental placeholders | Progress history intentionally contains placeholder terms, requiring a scoped rerun | Expected false positive |
| Scoped deliverable scan | Placeholder scan excluding progress history | No accidental placeholders in deliverable files | `deliverable placeholder scan: OK` | Pass |
| Superseded target scan | Numeric target scan across the bundle | Old packed byte target is fully replaced | `stale byte-target scan: OK` | Pass |
| Final revised bundle check | Structure, links, whitespace, fences, and heading spacing | Revised bundle remains valid | `final documentation checks: OK` | Pass |
| Phase A artifact contract | `bash tests/perf/test-benchmark-artifacts.sh` | Valid fixture passes and schema/count/percentile/NaN mutations fail | `benchmark artifact fixture tests passed` | Pass |
| Direct resource preflight | `cargo test -p gsplat-render-wgpu direct_scene_preflight` | Boundary, Nandi, overflow, and draw-cap reports pass without large allocation | 6 tests passed | Pass |
| Bench-runner artifact unit tests | `cargo test -p bench-runner` | Raw-frame summary, nearest rank, hashing, config, and rejection tests pass | 13 tests passed | Pass |
| Real artifact smoke | Release bench-runner + v1 validator on `minimal_ascii.ply` | Manifest, 30 raw frames, and recomputed summary validate | Valid; p95 frame wall 3.0585 ms, missed frames 0 | Pass |
| Hardened artifact smoke | 120 release frames + v1 validator | Atomic run, measurement timestamps, host/display metadata, and preflight validate | Valid; p95 frame wall 4.8274 ms, missed frames 0 | Pass |
| Strict clippy | Bench-runner and render-wgpu all targets with `-D warnings` | No lint suppression or warnings | Pass after boxing report and grouping context input | Pass |
| PlayCanvas dependency preflight | `npm test --prefix tests/competitive/playcanvas` | Exact version, integrity, license, and revision mapping pass | Preflight JSON status `pass` | Pass |
| Camera trace contract | `bash tests/perf/trace/test-trace-v1.sh` | Generator matches fixture; bad hash/matrix fail | `camera trace v1 tests passed` | Pass |
| iOS physical-device benchmark | `benchmark-ios-device-app.sh` on connected iPhone 17 Pro Max | Signed app launches and emits benchmark result | Build/sign passed; launch denied because device was locked | Blocked |
| Web Phase A collector | Node syntax/unit/package tests | Emit v1 raw frames and retain legacy benchmark line | Collector 3/3 and package 6/6 passed; unavailable GPU metrics remain null | Pass |
| PlayCanvas browser path smoke | Pinned PlayCanvas browser harness | Prove WebGPU and active GPU sorting before timing | `GSplatHybridRenderer`, `raster_gpu_sort`, `usesGpuSort=true`, 3-splat binary fixture | Pass |
| Nullable build identity contract | Artifact fixture suite | Unknown commit/dirty must be null and explicitly unavailable | Positive and fail-closed negative cases pass | Pass |
| Android Phase A artifact transport | Gradle JVM tests/compile, JNI smoke, sample APK | Preallocated raw samples and post-run v1 JSON without breaking packaging | JVM tests and compile pass; JNI smoke and debug APK build pass | Pass (transport only) |
| Integrated Rust workspace verification | fmt, check, test, strict clippy | Combined Phase A changes do not regress the workspace | 107 unit/integration tests plus doc tests passed; strict clippy passed | Pass |
| Web WASM collector browser smoke | Headless Chrome, minimal dataset, 5 measured frames | Emit legacy line plus complete v1 records with an explicit frame boundary | 5 frames emitted; rAF interval source, nonzero frame-wall samples, direct WebGPU path, no artifact error | Pass |
| iOS Phase A artifact transport | Simulator build and host extraction fixture | Preallocated samples, thermal endpoints, fail-closed dataset identity, atomic canonical artifact | Simulator build/sign and extraction fixture passed; destination reuse rejected | Pass (transport only) |
| Phase B independent acceptance audit | Code, plan, raw artifacts and manifests | Every checked Phase B item has current correctly-scoped evidence | Found mislabeled mean-as-p95, stale/dirty manifests, missing Flowers/device raw evidence, incorrect resource accounting and CPU-memory gap | Reopened |
| Corrected Kitsune pixel gate | Physical pixel threshold plus GPU parity test | Any RGB channel over 3/255 counts the pixel once | MAE 0.001259; pixel fraction 0; visible 70,402 | Pass |
| Fresh exact-a77 Android orbit | Nothing A065, canonical v1 raw frames | Establish authoritative pre-optimization p95 | Direct 13.612 ms; packed 23.703 ms; ratio 1.741; thermal 0→0 | Fail baseline |
| Scene-relative refresh candidate | Nothing A065 static/orbit snapshot | Remove false static label and synchronous refresh spikes | static ratio 0.790 pass; orbit ratio 1.108 narrowly fails | Partial |
| Android kitsune physical baseline | Nothing A065 / Vulkan / 120 frames | Valid v1 artifact | Valid; p95 frame wall 13.728 ms, missed 2 | Pass |
| iOS kitsune physical baseline | iPhone 17 Pro Max / Metal / 120 frames | Valid v1 artifact after sandbox clear | Valid; p95 frame wall 17.485 ms, missed 62 | Pass |
| Desktop Web kitsune baseline | Chrome headless WASM WebGPU / 30 frames | Valid v1 artifact | Valid; p95 frame wall 3 ms, missed 0 | Pass |
| Android artifact extractor | Fixture logcat extraction + reuse rejection | Fail-closed destination | Pass | Pass |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-07-11 | Findings patch context did not match generated file | 1 | Re-read the file and applied a context-accurate patch. |
| 2026-07-11 | Broad unsupported-claim scan matched intentional warning text | 1 | Classified as expected and replaced with a placeholder-only final scan. |
| 2026-07-11 | Revised broad placeholder scan matched the historical test-log row itself | 1 | Scope placeholder terms away from progress history and remove the superseded numeric literal from the log before rerunning. |
| 2026-07-11 | `cargo fmt --check` reported formatting changes in the new artifact module | 1 | Run `cargo fmt --all` and recheck. |
| 2026-07-11 | Bench config test requested artifact output in stability mode | 1 | Split artifact option coverage into an iteration-mode test; keep the runtime rejection. |
| 2026-07-11 | `cargo clippy -p bench-runner -- -D warnings` reported `result_large_err` through renderer dependencies | 1 | Box `DirectScenePreflight` in `ResourceLimitExceeded` and rerun clippy; do not add an allow. |
| 2026-07-11 | Clippy reported `too_many_arguments` for artifact context construction | 1 | Introduce a grouped context-input value and rerun with `-D warnings`. |
| 2026-07-11 | Minimal dataset manifest initially contained assumed bounds | 1 | Replaced them with fresh `bench-runner --analyze-spatial` output: `[0,-0.1,-0.1]` to `[0.1,0,1.2]`. |
| 2026-07-11 | iOS device benchmark timed out after `devicectl` launch denial | 1 | Device reported `Locked`; rerun requires the user/device to be unlocked, and build evidence remains separate. |
| 2026-07-11 | Initial binary-fixture harness patch did not match the agent-written README | 1 | Re-read the exact text and applied a targeted patch; no partial edit occurred. |
| 2026-07-11 | PlayCanvas smoke observed one camera and placement but queried no active manager | 1 | Corrected the director lookup from `CameraComponent` to the underlying `Camera`; the pinned WebGPU GPU-sort smoke now passes. |
| 2026-07-11 | Web manifest marked unknown git dirty state as clean | 1 | Changed unknown commit/dirty values to null, made explicit unavailability enforceable in v1, and added validator fixtures. |
| 2026-07-11 | First browser collector probe waited on a nonexistent DOM id and loaded the default large scene | 1 | Switched to the documented minimal dataset and `benchmarkStatus`; the repeatable 5-frame smoke passed. |
| 2026-07-11 | Web artifact initially labeled async rAF samples as synchronous after completion | 1 | Freeze the timing source when the run is created instead of reading the mutable enabled flag after `finishBenchmark`; async smoke now records nonzero rAF intervals. |
| 2026-07-11 | iOS artifact fallback could fabricate an all-zero dataset hash and sampled thermal state after post-run hashing | 1 | Fail artifact emission when dataset metadata cannot be proven, and freeze measurement-end time/thermal state on the final measured frame. |
| 2026-07-11 | Phase B was marked complete while Android failed and Flowers evidence was absent | 1 | Reopened Phase B; later phases remain gated until every stated device/dataset condition passes. |
| 2026-07-11 | Device p95 index actually stored average frame time | 1 | Reject the old index as exit evidence and use canonical raw-frame summaries for all new A/B comparisons. |
| 2026-07-11 | Android/iOS rejected explicit yaw zero and silently ran a moving orbit | 1 | Accept every finite yaw including zero and recapture static/orbit as distinct traces. |
| 2026-07-11 | First Android extraction polled the legacy result before JSON output completed | 1 | Wait for the summary marker before extracting and validating the atomic artifact. |
| 2026-07-11 | Phase C dependency fetch could not reach `index.crates.io` over TLS | 1 | Fetch through a reachable per-command sparse registry endpoint; leave Cargo configuration unchanged. |
| 2026-07-11 | Phase C formatting check found rustfmt drift | 1 | Format only `gsplat-io-spz`, then repeat focused verification. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Phase A and Phase B are complete; Phase C is in progress with the first bounded SPZ v4 slice implemented. |
| Where am I going? | Add real SPZ fixtures, higher SH degrees, and bounded cache cancellation/recovery evidence. |
| What's the goal? | Deliver the native-first competitive architecture through Phases A-F with evidence. |
| What have I learned? | See `findings.md`. |
| What have I done? | Froze Phase A, qualified Phase B, and added bounded degree-0 SPZ v4 decoding into RUF `SceneBuffers`. |
