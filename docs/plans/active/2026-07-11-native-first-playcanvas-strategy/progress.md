# Progress Log

## 2026-07-13 Goal Breakdown execution restart

- Reset-line branch and HEAD confirmed before staging:
  `refactor/packed-atlas-d-reset` at `05f649e396b2`.
- The prior `refactor/packed-atlas` work remains isolated in
  `stash@{0}: pre-d-reset-2026-07-13`; no telemetry/sidecar/adversarial-validator
  stack will be restored.
- The worktree contains uncommitted D-F candidate slices from the prior run.
  They are treated as pending until individually staged, freshly verified, and
  committed under G2-G5.
- G1 plan gate reform passed plan-scope diff hygiene and consistency checks.
- **Current unique execution item:** G2 — bounded paging correctness.

### Goal Breakdown status

- [x] G1 plan reform
- [x] G2 bounded paging correctness
- [x] G3 native Surface/Web usability
- [ ] G4 competitive evidence
- [ ] G5 local distribution and claim boundaries
- [ ] G6 terminal audit

### G2 acceptance

- Focused paged suite: 9 passed, including 27 pages over four atlas slots,
  eviction, non-resident draw exclusion, zero-error 64-splat degree-3 parity,
  small-motion non-zero cover, cancelled/stale-token rejection, stable local
  Surface preparation, and the deterministic 512-frame bound.
- Residency suite: 6 passed; scheduler suite: 2 passed.
- Full renderer suite: 79 passed, one pre-existing research oracle ignored;
  hardware-backed Metal `SortedAlpha` conformance, focused check, fmt, and diff
  hygiene passed.
- Soft observations retained without becoming blockers: the earlier two-minute
  Android run kept non-zero draw beyond 9,397 frames; PSS grew 4,065 KiB and RSS
  grew 5,532 KiB. No 30-minute or fixed-percentage claim is inferred.
- **Current unique execution item:** G3 — native Surface and Web usability.

### G3 acceptance

- Web API and WASM Surface now accept `direct`, `packed`, or `paged`; direct
  remains the default, invalid values fail, and the runtime receipt reports the
  actual path rather than a fixed label.
- Headless Chrome paged smoke validated eight measured frames on
  `minimal_ascii.ply`: `renderer=wasm_paged_active_atlas`, `visible=3`,
  `drawn=3`, with a valid v1 artifact.
- Physical Nothing A065 / Vulkan paged Surface smoke validated 30 measured
  Kitsune frames: `geometry_pipeline=paged_active_atlas`, average call
  0.720 ms, frame 0.712 ms, and non-zero `visible=drawn=2528`.
- C ABI tests (14), FFI smoke, JNI smoke, Web tests (6), wasm target check,
  Web/WASM builds, npm dry pack, Android AAR, and sample APK all passed.
- These short timings are usability observations, not sustained performance or
  broad device/browser claims.
- **Current unique execution item:** G4 — fair competitive evidence and bounded
  claim.

### G4 execution

- Existing five-pair artifacts revalidate and reproduce their stored report,
  but their manifests identify the old `05f649e` dirty worktree. They remain a
  useful provisional observation, not the final clean-build claim source.
- The matched Phase E static camera contracts are frozen as canonical
  `gsplat-camera-trace/v1` fixtures for Kitsune and the minimal/diagnostic
  quality path.
- **Current unique execution item:** G4 collector slice — land the fail-closed
  PlayCanvas and gsplat-rs collectors against these exact trace receipts.
- G4 collector slice passed: pinned PlayCanvas `2.21.0-beta.14` / `d5fe888`
  resolved WebGPU `GSplatHybridRenderer` with active `raster_gpu_sort` and
  `usesGpuSort=true`; the generated diagnostic rendered non-zero through that
  path.
- A 20-frame gsplat-rs Kitsune collector smoke matched the frozen dataset and
  trace hashes, fixed 640x480 display, full 279,199 drawn count, direct WebGPU
  path, and explicit camera receipt. The short sample is mechanics evidence,
  not a performance result.
- **Current unique execution item:** G4 comparison slice — validate deterministic
  SSIM and fail-closed paired aggregation before clean evidence capture.
- G4 comparison slice passed against the retained raw series: deterministic
  8x8 luma SSIM reproduced pair 01 at `0.998659`; the paired comparator
  reproduced the stored five-pair report and now also rejects wrong renderer
  paths, non-640x480 images, or any frame whose counts are not the full 279,199
  splats.
- Artifact fixtures prove unavailable PlayCanvas phase timings must be both
  `null` and declared; the canonical validator rejects undeclared nulls.
- **Current unique execution item:** G4 clean evidence capture — execute five
  sequential randomized-order pairs from committed collectors, then accept or
  narrow the claim from that report.

## Session: 2026-07-13 Phase D Reset

### D0 Functional Correctness Reboot

- **Status:** in_progress; reset to checkpoint `05f649e`
- Preserved boundary:
  - Phase B/C packed atlas and SPZ work remains intact.
  - Phase D keeps `spatial_pages`, `residency`, `page_scheduler`, `page_atlas`,
    `paged_gpu`, and offscreen `PagedActiveAtlas` rendering/parity bootstrap.
- Reopened correctness gaps:
  - The checkpoint allocates `page_count` atlas slots and force-installs every
    page, so it is a parity bootstrap rather than true bounded streaming.
  - Surface still rejects `PagedActiveAtlas` and has no non-zero drawn smoke.
  - Continuity and expired-page draw prevention need deterministic D0 gates.
- Frozen until D0 is complete:
  - long Android RSS/queue runs, network-adversarial validation, telemetry
    sidecars/C ABI receipts, 10M and PlayCanvas qualification, SOG decoders.
- **Current single blocker:** D0-1 offscreen `PagedActiveAtlas` vs `PackedAtlas`
  count + image parity on the minimal scene and deterministic multi-page
  qualification-small scene.
- Working rules: one blocker at a time; unrelated features frozen; prefer less
  than 800 net new lines and at most two new files per slice; evidence must ship
  with the function it proves; no commit or push without user confirmation.
- D0-1 audit: the reset branch has count + image parity only for the inline
  two-splat scene; the qualification-small paged parity gate is missing and is
  the active implementation gap.
- D0-1 completed:
  - Added a deterministic 64-splat, degree-3, multi-page paged-vs-packed gate.
  - Added paged active-slot SH hot-color refresh through the existing packed
    RGB10 hot-record path.
  - Minimal and qualification-small count/image parity are exact.
  - `cargo test -p gsplat-render-wgpu`: 73 passed, 1 existing ignored oracle;
    SortedAlpha conformance passed.
  - `cargo check -p gsplat-render-wgpu`: passed.
- **Current single blocker:** D0-2 fixed atlas-slot budget, active-subset
  residency, eviction, and non-resident-page draw exclusion.
- D0-2 completed:
  - Replaced `page_count * capacity` bootstrap allocation with four fixed slots.
  - Integrated the existing scheduler and generation-checked residency manager
    with GPU clear/upload transitions.
  - Added a 27-page gate proving active-subset draw, camera-jump eviction, and
    exclusion of every non-resident source splat from active draw entries.
  - `cargo test -p gsplat-render-wgpu`: 74 passed, 1 existing ignored oracle;
    conformance passed. Focused fmt/check and `git diff --check` passed.
- **Current single blocker:** D0-3 retained coarse coverage during small camera
  motion with a deterministic short trace and no zero-draw holes.
- D0-3 completed:
  - Added deterministic resident-set hysteresis around the nearest-page cutoff.
  - Added a four-frame small-motion trace proving retained coarse coverage,
    non-zero drawn count, and non-transparent output on every frame.
  - `cargo test -p gsplat-render-wgpu`: 75 passed, 1 existing ignored oracle;
    conformance passed.
- **Current single blocker:** D0-4 reject cancel/stale/generation results before
  an expired page can enter the active draw set.
- D0-4 completed:
  - Added `cancel_inflight` with slot release and generation invalidation.
  - Added residency-guarded GPU upload immediately before slot mutation.
  - Tests prove cancelled and stale tokens cannot enter `active_entries`.
  - `cargo test -p gsplat-render-wgpu`: 77 passed, 1 existing ignored oracle;
    conformance passed.
- **Current single blocker:** D0-5 smallest usable local-source Surface paged
  path with stable non-zero drawn output.
- D0-5 completed:
  - Added presenter-owned fixed-slot local paged runtime and Surface draw pass.
  - Added Surface session paged ordering/stats path without restoring old
    source-streaming or telemetry stacks.
  - Extended the existing experimental C/Android geometry selector with paged
    value 2; C FFI smoke, JNI smoke, and APK build passed.
  - Physical Nothing A065 / Vulkan smoke: 8 measured frames, every frame
    `geometry_pipeline=paged_active_atlas visible=3 drawn=3`, zero missed frames.
- D0 completion matrix: all five correctness gates complete. Workspace check,
  fmt, strict clippy, render/FFI tests, conformance, FFI/JNI smoke, APK build,
  and Android Surface smoke passed.
- **Current single blocker:** D1 bounded four-slot/active-set invariants plus a
  two-minute Android Kitsune paged steady-state run; no 30-minute gate.
- D1 completed:
  - Deterministic 512-frame trace kept slot/resident/active counts ≤4 with
    non-zero drawn output on every frame.
  - Physical Android Kitsune local paged Surface ran beyond 9,397 frames.
  - 120-second PSS: `291272 -> 295337 KiB` (+4,065 KiB).
  - Endpoint RSS: `417000 -> 422532 KiB` (+5,532 KiB), below 64 MiB.
  - No network profile was needed; 30-minute qualification remains deferred.
  - Full render suite: 79 passed, 1 existing ignored oracle; conformance, fmt,
    and diff hygiene passed.
- Phase D complete (D0 + D1).
- **Current single blocker:** Phase E revalidate the pinned PlayCanvas harness
  and freeze the first fair paired Kitsune protocol before measurements.
- Phase E harness identity/path revalidation completed:
  - `npm ci --ignore-scripts`: 82 packages, zero vulnerabilities.
  - preflight passed exact version/revision/license/integrity.
  - browser smoke passed WebGPU + active GPU-sort path with 3 visible splats.
- **Current single blocker:** add timed PlayCanvas v1 raw-frame collection with
  frozen identity/dataset/trace/backend/path fields; no claim before this exists.

## Session: 2026-07-11

### Implementation Phase D: Spatial Pages and Streaming LOD

- **Status:** in_progress; offscreen paged SortedAlpha draw path landed
- Actions taken:
  - Added `spatial_pages` module: uniform-grid partition into capacity-capped
    pages with AABB metadata (`DEFAULT_PAGE_CAPACITY = 65536`).
  - Added `ResidencyManager`: Absent→…→Resident→Evicting state machine, atlas
    slot generations, inflight/resident budgets, stale async-token rejection,
    attribute LOD on resident pages, LRU eviction helper.
  - Added `page_scheduler`: CPU distance-ranked coarse cover, request/evict
    under budgets, camera-jump unit gate.
  - Added `page_atlas`: extract page `SceneBuffers`, pack into fixed CPU slots,
    generation-aware install/clear, degree-0 average attribute-byte gate (20 B).
  - Added `paged_gpu`: fixed-capacity GPU atlas, shared-bounds page packing,
    `write_hot_records_at` / clear, stale-token gates; upload test reports
    4 pages / 8 resident splats matching whole-scene packed visible/drawn.
  - `pack_scene_with_encoding` lets pages share parent scene bounds/log scales.
  - Wired `GeometryPath::PagedActiveAtlas` into offscreen `Renderer`: spatial
    pages on load, preprocess over resident `(global, scene)` entries,
    `ensure_paged_atlas` + `render_paged_sorted_indices` reuse packed pipeline.
  - Surface presenter / FFI session explicitly reject paged path for now;
    `bench-runner` accepts `--geometry-path paged`.
  - Parity gate `paged_vs_packed_count_parity_on_minimal_scene`:
    visible/drawn=2, mean_abs_rgb=0.
- Current boundary / remaining:
  - Surface/FFI paged present, streaming residency in the draw loop (not
    full-install bootstrap), SH color-refresh / hysteresis, network traces,
    large-scene attribute/memory/30-min stability gates.
- 5-question reboot:
  - Where am I? Phase D offscreen draw integration complete.
  - Where am I going? Continuity / streaming / large-scene evidence + surface.
  - What's the goal? Bounded paged atlas without persistent holes.
  - What have I learned? Bootstrap full-install gives exact packed parity;
    surface still needs a dedicated paged presenter path.
  - What have I done? Page stack + offscreen SortedAlpha paged draw + parity.

### Implementation Phase C: Compressed Sources and Bounded Decode

- **Status:** complete under minimal-fixture exit evidence; FFI/device follow-ups deferred
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
  - **Slice 1 committed (`7b5943e`):** land Phase C SPZ v4 decoder crate +
    workspace + plan docs (excluding android build / `__pycache__` artifacts).
  - **Slice 2:** cooperative cancel APIs (`load_spz_cancellable` /
    `parse_spz_bytes_cancellable`), committed fixture
    `tests/datasets/minimal_v4_degree0.spz` (8 splats, degree 0, 331 B),
    PLY↔SPZ count/attribute mapping gate (RDF PLY inverse of RUF SPZ), and
    cancel/recovery unit evidence.
  - **Slice 3 (`df59adc`):** `SourceResidencyCaches` with independent
    compressed/decoded LRU byte budgets.
  - **Slice 4 (`e7908c1`):** Offscreen PLY-vs-SPZ image/count parity on the
    minimal fixture (`visible=8`, mean_abs_rgb=0).
  - **Slice 5:** Cold/warm load metrics + TTFF artifacts under
    `target/benchmarks/phase-c/` (SPZ transport 331 B vs PLY 1106 B ≈ 0.30×;
    logical peak 779 vs 1554).
- Current boundary / remaining (deferred, not Phase C exit blockers):
  - FFI/examples SPZ load wiring.
  - Optional device image parity for larger qualification scenes.
  - Degree 4 SH, legacy gzip v1-3, extension ILV.
- 5-question reboot:
  - Where am I? Phase C exit satisfied on the minimal paired fixture.
  - Where am I going? Phase D spatial pages / streaming LOD, plus optional SPZ FFI.
  - What's the goal? Native-first competitive architecture through Phases A-F.
  - What have I learned? See `findings.md` Phase C section.
  - What have I done? Full Phase C decoder + gates + residency + metrics.

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

- 2026-07-13: The first Phase F external Web consumer probe used
  `npm pack --prefix packages/web`; npm still resolved `package.json` from the
  repository root for this subcommand, so no tarball was created and the
  dependent install/import steps failed. Run `npm pack` with `packages/web` as
  the actual working directory, then install its tarball into an isolated
  `target/` consumer.
- 2026-07-13: The static qualification URL requested yaw zero through an HTML
  range whose minimum is `0.0001`; the browser clamped it and the first orbit
  cleared the native trace-camera override. Force yaw zero from qualification
  state rather than routing static trace semantics through the UI control.

## 2026-07-13 Phase E Kitsune five-pair qualification

- The first complete concurrent diagnostic pair produced 3,600 valid frames
  per engine, SSIM `0.9986559132`, PlayCanvas p95/p99 frame wall
  `16.6/17.2 ms`, and gsplat-rs `12.7/15.1 ms`. It is not included in the
  randomized claim series because the engines ran concurrently.
- Predeclared sequential pair order before measurement:
  `01 playcanvas-first`, `02 gsplat-rs-first`, `03 gsplat-rs-first`,
  `04 playcanvas-first`, `05 playcanvas-first`. Each run uses 120 warmups and
  3,600 measured rAF frames and records pair id/order/position in its manifest.
- 2026-07-13: A diagnostic SSIM wrapper tried to assign zsh's read-only
  `status` variable after emitting a valid `0.6931295175` result. The metric
  result is retained; future wrappers use a different exit-code variable.
- 2026-07-13: The generated diagnostic Blob loaded but produced no PlayCanvas
  gsplat resource because its object URL has no `.ply` suffix. Keep the bytes
  unchanged and provide an explicit `raster_diagnostic_v1.ply` asset filename
  so PlayCanvas selects its PLY parser.
- 2026-07-13: The first generated raster-diagnostic smoke timed out because the
  harness server did not declare `.mjs` as JavaScript for the shared module.
  Added the explicit MIME mapping before retrying; no benchmark evidence was
  emitted from the timed-out run.
- 2026-07-13: The first raster-diagnostic patch assumed a runtime signal
  assignment that does not exist in `main.js`; it was rejected atomically.
  Split module/server, PlayCanvas, Web example, and collector edits and used the
  actual signal initialization path.
- 2026-07-13: The first combined minimal-trace patch used stale nearby context
  in `examples/web/src/main.js` and was rejected atomically. Re-read exact
  state/config/collector sections and applied smaller context-accurate patches.
- 2026-07-13: Canonical matched-image SSIM failed at `0.8713640084 < 0.99`.
  Kept Phase E open and moved diagnosis from camera/metric plumbing to raster
  semantics; no threshold or geometry compensation was applied.
- 2026-07-13: System Python did not provide Pillow for image-bound analysis.
  Used the installed ImageMagick CLI read-only instead; no dependency was added.
- 2026-07-13: The first PlayCanvas Kitsune candidate captured all frames but
  failed artifact validation because the shared camera contract uses
  `trace_id`, while the smoke descriptor uses `id`. Accept both explicit forms
  in the producer and rerun; the invalid candidate is not evidence.
- 2026-07-13: The first shared static-trace hash was calculated from integer
  JSON literals while the committed audit-friendly file used equivalent float
  literals. Accepted the validator's canonical hash for the actual file and
  updated the protocol before any run.
- 2026-07-13: A Phase E search included nonexistent `technical_design.md`.
  Continued with the actual plan files and recorded no result from that path.
- 2026-07-13: Phase E status patch expected `**Status:**` without the file's
  list marker. Re-read the exact section and applied a context-accurate patch.

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-07-13 | Phase E harness inspection requested nonexistent `public/main.mjs`; actual file is `public/main.js`, so later chained reads did not run | 1 | Use `rg --files` result and read `main.js` plus `server.mjs` explicitly. |
| 2026-07-13 | D1 512-frame bound assertion required rustfmt wrapping | 1 | Apply rustfmt before running the deterministic bound gate. |
| 2026-07-13 | D0-5 local Surface runtime gate required rustfmt compaction | 1 | Apply rustfmt, then run the focused non-zero-draw gate. |
| 2026-07-13 | D0-5 focused rustfmt check requested standard compaction/wrapping in the new Surface runtime and session branch | 1 | Apply rustfmt before the first D0-5 compile. |
| 2026-07-13 | D0-5 Surface match edit temporarily left both the old rejection arm and new paged render arm | 1 | Remove the old `PagedAtlasUnsupported` arm before compiling; no partial behavior claim. |
| 2026-07-13 | D0-4 focused compile could not resolve `ResidencyManager` in the new guarded GPU upload signature | 1 | Add it to `paged_gpu.rs` residency imports and rerun the identical focused test command. |
| 2026-07-13 | New D0-2 assertions required standard rustfmt wrapping | 1 | Apply rustfmt before running the focused eviction test. |
| 2026-07-13 | Four-page qualification fixture restored count 64 but put every splat at identical depth, so scheduler slot remapping changed undefined equal-depth alpha order; MAE `0.002070`, pixel tail `0.045410` | 1 | Keep four pages but assign the four spatial groups distinct deterministic depths, then rerun the same parity gates. |
| 2026-07-13 | Qualification-small slot-count assertion referenced the private parent constant without qualification | 1 | Use `super::DEFAULT_PAGED_ATLAS_SLOTS` and rerun the focused parity command. |
| 2026-07-13 | First full test after the four-slot D0-2 integration drew 16 of 64 qualification-small splats because the fixture occupied more than four pages | 1 | Keep the fixed budget; reshape the deterministic degree-3 fixture to exactly four spatial pages so D0-1 remains an all-resident multi-page gate. |
| 2026-07-13 | D0-2 focused rustfmt check requested one line wrap in the new resident-page lookup | 1 | Apply rustfmt and rerun before compiling. |
| 2026-07-13 | First combined D0-2 patch found a stale `ensure_paged_atlas` call-site context and was rejected atomically | 1 | Re-locate exact formatted call sites and apply residency, scheduler, and rasterizer changes as smaller patches. |
| 2026-07-13 | Full render-wgpu test compile could not resolve private parent helper `default_spatial_pages` from the nested test module | 1 | Qualify it as `super::default_spatial_pages` and rerun the same full crate test command. |
| 2026-07-13 | Focused `cargo fmt --check -p gsplat-render-wgpu` requested one signature compaction in `packed_gpu.rs` | 1 | Apply repository rustfmt, then rerun the focused check before tests. |
| 2026-07-13 | D0-1 deterministic multi-page degree-3 parity preserved all 64 visible/drawn splats but failed image thresholds: MAE `0.014751`, pixel tail `0.450562` | 1 | Add camera-dependent SH hot-color refresh for occupied paged slots, then rerun the same two paged parity gates. |
| 2026-07-13 | D0-1 Kitsune paged parity could not allocate the bootstrap atlas: 11,468,800 entries required a 229,376,000-byte hot binding and preflight returned `PagingRequired` | 1 | Do not repeat the full-scene bootstrap attempt. Define D0-1 with a deterministic multi-page qualification-small fixture; retain Kitsune for fixed-budget D0-2/D1 evidence. |
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
| 2026-07-11 | PLY↔SPZ attribute gate mismatched identity rotations | 1 | Invert PLY RDF→RUF quaternion flips when authoring paired PLY; compare quaternions up to sign. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Phase A–C remain at their recorded boundaries; Phase D is reset to `05f649e` and D0 is open. |
| Where am I going? | Close D0 in order: offscreen parity, true bounded residency, continuity, failure semantics, then Surface non-zero draw. |
| What's the goal? | A minimal correctly streaming paged product path before steady-state or competitive qualification. |
| What have I learned? | Full-install parity is only a bootstrap; validator volume and long-run telemetry do not substitute for streaming correctness. |
| What have I done? | Preserved the Phase B/C boundary, discarded post-checkpoint code from the active branch, and rewrote the Phase D gates. |
## 2026-07-13 Phase E raw-frame collector blocker

- Inspected the complete v1 validator call path, fixture suite, and artifact
  contract before changing the PlayCanvas harness.
- Found that the contract incorrectly forces competitor-internal phase timings
  to be non-null. Current action: correct nullable timing semantics and prove
  them with positive and negative fixture tests before implementing collection.
- The nullable phase-timing contract correction passed the deterministic v1
  fixture suite, including rejection of an unlisted null `sort_ms`.
- Confirmed the existing production module import and PlayCanvas event
  boundaries to use for the collector. No engine patch or second browser stack
  is needed.
- Added `benchmark:smoke` to the pinned PlayCanvas harness. A fresh headless
  Chrome WebGPU run captured 60 raw frames after 30 warmups and passed the v1
  validator under `target/benchmarks/playcanvas-collector-smoke/`.
- The artifact freezes dataset hash, static camera descriptor hash, display,
  backend, active GPU-sort renderer, dependency identity, and timing sources.
  It is explicitly `collector_smoke_only`; no competitive claim is derived.
- Closed the raw-frame collector blocker. The next and only Phase E blocker is
  the paired qualification protocol shared by gsplat-rs and PlayCanvas.
- Began the paired-protocol blocker by reconciling the existing verification
  plan with available datasets and collectors. Kitsune is selected as the first
  nontrivial degree-3 pair; minimal remains smoke-only.
- Audited the gsplat-rs Web collector and rejected its current synchronous,
  locally generated orbit as paired evidence. The protocol must first add a
  common camera input and rAF mode before collecting competitor numbers.
- Added and validated the shared Kitsune static trace, then captured a valid
  PlayCanvas candidate: 3,600 raw frames, 120 warmups, 30.001 seconds, exact
  dataset/trace/path/display identities, and zero missed configured-budget
  frames. No cross-engine result is claimed yet.
- Found and scoped the gsplat-rs paired-camera gap: the public WASM wrapper
  cannot accept an explicit camera. Current action is the smallest existing-
  module bridge for exact pose/intrinsics, followed by rAF collector wiring.
- Captured the first gsplat-rs candidate successfully, then rejected it as the
  final pair because the dataset ID and available build/device identity were
  not normalized. Added fail-closed normalization and canvas-only capture for
  the next paired quality check.
- The normalized recapture produced an exact dataset/trace/display identity
  match. Rejected overlay-contaminated element screenshots and changed both
  collectors to write the raw canvas backing PNG before running image parity.
- Diagnosed the inverted PlayCanvas canvas from local source: gsplat-rs performs
  RDF-to-RUF conversion during PLY load, while the PlayCanvas scene retained
  RDF. Added and recorded an entity Y reflection for the paired input boundary;
  the candidate must be recaptured and rechecked visually/numerically.
- The recaptured PlayCanvas image is now upright, but the projected bounds are
  still smaller. Added a PlayCanvas camera/projection receipt and a
  qualification query option to the existing fast smoke so the mismatch can be
  diagnosed without another 30-second timing run.
- Added a tested WASM camera receipt and recaptured gsplat-rs. The receipt
  proves exact pose/intrinsics application, so the remaining paired blocker is
  raster-quality alignment. After correcting the metric interpretation and
  camera basis, similarity is about 0.95362 versus the frozen 0.99 target; no
  performance gate is evaluated.
- Reconciled the two Gaussian support formulas and avoided an invalid scale
  compensation. Added a fixed PlayCanvas quality profile matching gsplat-rs's
  no-LOD/no-contribution-cull, 1/256 alpha, 0.3-pixel-floor semantics and black
  background; a fast raw-canvas smoke will decide whether this closes SSIM.
- The fixed PlayCanvas quality/basis profile improved the pair substantially.
  A gsplat-rs Gaussian-support rewrite did not close the remaining gap and was
  reverted after a one-frame quality probe.
- Found that the upright PlayCanvas image was still horizontally mirrored.
  Replaced the incomplete Y-only bridge with a source Y/Z plus camera-Z basis
  conversion and will re-run the fast raw-canvas quality probe before timing.
- Projection audit found the remaining scale mismatch: the static Web benchmark
  inadvertently replaced the trace camera via `orbit(0, 0)`. Changed the frame
  loop to preserve the explicit trace when yaw is zero; rebuilding and probing
  this fix is the current quality gate.
- Completed the camera/projection audit with receipts on both engines. The
  remaining cross-engine similarity is about 0.9536 and fails the existing
  0.99 gate due to systematic raster footprint differences.
- Invalidated the exploratory Phase E timing candidates and retained only the
  collector/contract implementation. Phase E remains open on one blocker:
  achieve honest raster-quality parity without camera/geometry compensation or
  threshold gaming.
- Removed all invalid exploratory Phase E paired artifacts from `target/` so
  they cannot be mistaken for claim evidence. Collector smoke artifacts remain
  clearly scoped as smoke.
- Fresh post-diagnosis verification passes: workspace check; render-wgpu/Web/C
  FFI tests (79 render tests passed, one research oracle ignored; conformance
  passed; 14 FFI tests passed); strict clippy; Web package tests; pinned
  PlayCanvas preflight/path smoke; v1 artifact fixtures; both camera-trace
  validators; formatting and diff hygiene.
- Reopened the quality score itself as part of the same blocker: local
  ImageMagick aliases SSIM/DSSIM output semantics, so the previously reported
  0.9536 conversion is not qualification-grade. Current action is to freeze a
  deterministic one-for-identical SSIM implementation before further tuning.
- Ran the first canonical `ssim-luma-srgb-window8` pair: score
  `0.8713640084`, gate fail. Confirmed Surface camera uniforms refresh every
  frame and ruled out double opacity activation plus zero-yaw fallback.
- Ran a diagnostic-only scale sweep and overlay. Best resize is ~88% with SSIM
  ~0.957, while rigid features remain doubled at native scale. No transformed
  image is accepted as evidence; next action is a source-center-bounds receipt.
- Added a PlayCanvas source-center-bounds receipt; it matches Kitsune exactly
  after basis conversion. Also verified Rust/WGSL uniform layout. Next probe is
  the existing minimal scene under a new shared static trace, not more Kitsune
  tuning.
- Added and validated a shared minimal static trace. Its exact-camera pair
  scores `0.9580970409`; visual inspection shows only Gaussian gradient/support
  differences. Current experiment changes that one variable on direct Surface.
- Re-tested normalized PlayCanvas-style support under the minimal trace; score
  dropped to `0.9493355401`, so the shader experiment was fully reverted.
- Identified the minimal fixture's positive log scales as cap-saturating. Closed
  this diagnostic slice (canonical SSIM + shared minimal trace) and moved to an
  uncapped small-scale fixture as the next single action.
