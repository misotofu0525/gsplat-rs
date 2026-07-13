# Task Plan: Native-First PlayCanvas Competitive Strategy

## Goal

Implement and qualify the repository-backed native-first architecture in the
required order: rendering correctness first, boundary and failure semantics
second, then steady-state memory and competitive performance.

## Current Phase

Implementation Phases B and C remain complete at their recorded boundaries.
The reset-line worktree contains candidate Phase D-F implementation and
evidence, but it is not accepted until the Goal Breakdown below is verified and
committed slice by slice. G1 is the only current execution item.

## Goal Breakdown

### Terminal Definition

The initial native-first goal is achieved when the reset line has committed,
verified capabilities for: a page count larger than a fixed atlas-slot budget;
resident-subset rendering with eviction and no non-resident draw; deterministic
continuity and stale/cancel/generation safety; usable native Surface and Web
paths; a fair PlayCanvas comparison with raw evidence; local AAR,
XCFramework/Swift Package, and Web tarball consumption; and canonical claim
language that says exactly what the evidence does and does not support.

Completion means capability landed, evidence retained, and claims bounded. It
does not require winning every performance target, completing every device
class, or restoring the abandoned telemetry/sidecar/adversarial-validator stack.

### Ordered Subgoals

- [x] **G1 — Reform gates and freeze this Goal Breakdown.**
  - Hard acceptance: `task_plan.md`, `verification_plan.md`, `design.md`,
    `progress.md`, and `findings.md` consistently separate correctness gates
    from performance observations and name one current item.
  - Soft observation: none; this slice changes policy only.
  - Commit boundary: planning bundle only.
- [x] **G2 — Accept bounded large-scene paging correctness.**
  - Hard acceptance: total pages exceed four slots; active/resident counts never
    exceed the slot budget; eviction/refinement preserves non-zero coarse cover;
    non-resident pages never draw; cancel/stale token/slot generation cannot
    publish; paged-vs-packed count and small-scene image gates pass; no crash.
  - Soft observation: report the 512-frame bounds and available short Android
    memory/frame measurements without treating a fixed percentage as blocking.
  - Commit boundary: page scheduler/residency/atlas/GPU renderer correctness and
    focused tests.
- [ ] **G3 — Accept native Surface and Web paged usability.**
  - Hard acceptance: local-source paged Surface produces stable non-zero draws;
    the experimental C/Android selector is consistent; Web camera/Surface path
    remains usable and browser smoke is non-zero; direct remains default.
  - Soft observation: report frame timing and unavailable device/browser gaps;
    do not add remote streaming or telemetry infrastructure.
  - Commit boundary: Surface session, FFI/Android selector, Web wrapper/example,
    and their focused tests.
- [ ] **G4 — Accept fair competitive evidence and a bounded claim.**
  - Hard acceptance: pinned PlayCanvas WebGPU GPU-sort path, matched dataset and
    trace receipts, raw v1 artifacts, deterministic image metric, matched counts,
    and at least one reproducible paired report all validate.
  - Soft observation: p95/p99 ratios, confidence intervals, missed frames,
    thermal/energy/device gaps, and additional datasets are reported rather than
    optimized until a number turns green.
  - Commit boundary: comparison harness, collectors, validators, trace/quality
    fixtures, and protocol documentation.
- [ ] **G5 — Accept local distribution and stable claim boundaries.**
  - Hard acceptance: Web tarball external import, Android JNI/AAR/APK, Apple
    host/XCFramework/Swift Package consumer paths pass; stable v0.1 versus
    experimental APIs/formats/lifecycle/errors are explicit.
  - Soft observation: package sizes and unavailable registry/device publication
    are recorded; no npm, Maven, SwiftPM registry, tag, or push is performed.
  - Commit boundary: package wrapper/tests and canonical handbook/binding docs.
- [ ] **G6 — Close the terminal audit.**
  - Hard acceptance: G1-G5 commits exist on this branch, canonical regression
    checks pass, progress contains an achieved/not-achieved claim table, the
    worktree has no uncommitted task changes, and no unsupported broad claim is
    present.
  - Soft observation: retain all measured regressions and environment gaps.
  - Commit boundary: final plan/progress/findings reconciliation only.

### Gate Policy

- **Hard gates:** `SortedAlpha` correctness; count/image parity where declared;
  no persistent short-trace holes; stale/cancel/generation exclusion; fixed slot
  bounds with non-resident draw exclusion; non-zero Surface/Web output; no crash.
- **Soft observations / claim qualifiers:** fixed performance ratios or
  percentage wins, 30-minute RSS, 40–48-byte averages, complete device/browser
  matrices, energy/thermal wins, and large-scene headline scale. A miss narrows
  the claim and stays visible; it does not trigger endless implementation churn.
- **Current unique execution item:** G3 — accept native Surface and Web paged
  usability without restoring remote streaming or telemetry infrastructure.

## Phases

### Phase 1: Requirements and Evidence Refresh

- [x] Re-read the current project context, architecture, roadmap, principles,
      verification guide, and relevant platform/package documentation.
- [x] Capture the current gsplat-rs renderer, packaging, benchmark, and large-scene constraints.
- [x] Capture the current PlayCanvas comparison baseline from primary sources.
- **Status:** complete

### Phase 2: Competitive Architecture Design

- [x] Define the native-first product boundary and explicit non-goals.
- [x] Define the small-scene direct path and large-scene packed/paged path.
- [x] Define asset formats, residency, LOD, sorting, and device-policy architecture.
- [x] Record tradeoffs, failure modes, and rollback boundaries.
- **Status:** complete

### Phase 3: Verification and Benchmark Design

- [x] Define fair native-vs-PlayCanvas-mobile and Web-vs-Web benchmark tracks.
- [x] Define datasets, devices, camera traces, quality parity, metrics, and artifacts.
- [x] Define correctness, performance, memory, stability, and release gates.
- [x] Define claim language allowed by each level of evidence.
- **Status:** complete

### Phase 4: Documentation Delivery

- [x] Write the complete technical design document.
- [x] Write the executable verification plan and phased implementation checklist.
- [x] Cross-link the plan bundle and keep canonical project facts unchanged unless necessary.
- **Status:** complete

### Phase 5: Document Verification and Handoff

- [x] Review documents for internal consistency and unsupported claims.
- [x] Validate Markdown links, formatting, and repository diff hygiene.
- [x] Update planning records and hand off the resulting documents.
- **Status:** complete

## Implementation Execution State

- Phase A exit gate is satisfied with validated kitsune artifacts on Android,
  iOS, and desktop Web.
- Phase B exit evidence is satisfied for the stated device/dataset matrix under
  matched sync-orbit `sort_interval=4` (Android kitsune 1.078; Android flowers
  0.383) plus existing iOS/desktop passes and `phase-b-peak-rss-index.json`.
  `sort_interval=2` sync residual (~1.12) remains a known non-qualification
  stress case. Phase C is complete under minimal-fixture evidence; Phases D-F
  remain gated.
- Keep this bundle under `docs/plans/active/` while implementation is pending;
  move it to completed planning history only after the execution roadmap is
  delivered or explicitly superseded.

## Execution Phases

### Phase A: Freeze Competitive Baseline

- [x] Add versioned raw-frame distributions and environment metadata for the
      desktop, Web, Android, and iOS collectors; physical-device evidence
      remains a separate baseline gate.
- [x] Freeze dataset and deterministic camera-trace manifests.
- [x] Add structured scene/device resource preflight reports.
- [x] Add the pinned PlayCanvas comparison harness with explicit WebGPU and
      active GPU-sort path signals.
- [x] Capture reproducible desktop Web plus physical Android/iOS kitsune
      baselines under `target/benchmarks/phase-a/`.
- **Status:** complete

### Phase B: Packed Atlas Without Streaming

- [x] Implement and verify the 20-byte hot record and degree-3 sidecar.
- [x] Preserve global CPU sorting and `SortedAlpha` image/count parity
      (desktop offscreen gates; iOS device kitsune A/B ≤10%).
- [x] Prove at least 3x full-degree-3 attribute reduction; desktop p95 ≤10%
      vs direct on minimal + kitsune; Nandi binding failure removed.
- [x] Android kitsune packed p95 ≤10% vs direct under matched sync-orbit
      `sort_interval=4` (`android-a065-sort4-sync-five-paired`, median 1.078).
- [x] Android flowers packed p95 ≤10% vs direct under the same protocol
      (`android-a065-flowers-sort4-sync-five-paired`, median 0.383).
- **Status:** complete under `sort_interval=4` qualification protocol; keep
  `sort_interval=2` sync residual documented as open stress work

### Phase C: Compressed Sources and Bounded Decode

- [x] Add the first bounded SPZ v4 source slice: plaintext header validation,
      independent ZSTD streams, and degree-0 `SceneBuffers` decode.
- [x] Extend SPZ v4 decode to SH degrees 1–3 with RUB→RUF SH sign flips and
      PLY channel-major `sh_rest` layout.
- [x] Add a committed minimal SPZ v4 fixture plus unit-level PLY↔SPZ
      count/attribute mapping and cooperative cancel evidence.
- [x] Bound compressed/decoded CPU caches as a reusable residency layer
      (`SourceResidencyCaches` / LRU byte budgets in `gsplat-io-spz`).
- [x] Prove offscreen PLY-vs-SPZ image/count parity on the minimal degree-0
      fixture (`ply_vs_spz_offscreen_image_parity_gate_on_minimal_fixture`).
- [x] Record cold/warm load metrics and first-frame timing under
      `target/benchmarks/phase-c/` for the minimal paired fixture.
- **Status:** complete under minimal-fixture evidence (transport ~0.30× PLY,
  logical peak lower, cancel + attribute + offscreen image gates). Deferred
  outside this exit: FFI/examples wiring, device parity on larger scenes,
  legacy gzip v1–3 / degree 4 / extension ILV.

### Phase D: Minimal Correct Streaming Pages

#### D0: Functional Correctness

- [x] Prove offscreen `PagedActiveAtlas` vs `PackedAtlas` count and image parity
      on the minimal scene and a deterministic multi-page, degree-3
      qualification-small scene using the existing thresholds.
- [x] Replace bootstrap full-install with a fixed atlas-slot budget, active-subset
      residency, eviction, and a unit gate proving non-resident pages never draw.
- [x] Prove no persistent holes during small camera motion through coarse-cover,
      ancestor retention, or an equivalent deterministic short-trace policy.
- [x] Prove cancel, stale-token, and generation failures cannot publish or draw
      an expired page.
- [x] Add the smallest usable Surface paged path, initially with a local page
      source, and prove stable non-zero drawn output.
- **Status:** complete; all five D0 correctness gates have fresh local and
  Android Surface evidence

#### D1: Steady-State Qualification

- [x] After D0 is fully green, prove bounded memory and queues over a short
      steady-state run of several minutes; add an optional network profile only
      if it serves this gate.
- [x] Keep any 30-minute gate in D1 or Phase E; it does not define functional
      correctness.
- **Status:** complete; deterministic 512-frame bounds and a two-minute Android
  Kitsune steady state pass without requiring a network profile

#### Deferred Until D0 Completion

- 30-minute Android RSS or queue runs.
- Network-adversarial validators, telemetry sidecars, C ABI telemetry receipts,
  and artifact-hardening work.
- 10M qualification datasets and PlayCanvas competitive gates (Phase E).
- SOG and Streamed SOG decoders.

### Phase E: Policy Optimization and Competitive Qualification

- [x] Pin and revalidate the PlayCanvas WebGPU GPU-sort path.
- [x] Add a validated v1 PlayCanvas raw-frame collector without fabricating
      unavailable engine-internal timings.
- [x] Freeze and execute a matched dataset, camera, display, quality, backend,
      warmup, and sampling protocol for gsplat-rs and PlayCanvas.
- [x] Evaluate optional GPU culling/sort only by evidence; no additional policy
      was needed for the qualified Kitsune static Web scope.
- [x] Resolve the claim decision: desktop Chrome/WebGPU Kitsune-static parity
      passes five randomized pairs; native leadership, broad dataset/browser
      parity, sustained thermal, energy, memory-leadership, and large-scene
      claims remain unearned and must not be promoted.
- **Status:** complete for the explicitly qualified narrow claim scope
- **Exit evidence:** `target/benchmarks/phase-e/kitsune-five-paired/` records
  five 3,600-frame pairs, per-pair SSIM, and deterministic bootstrap statistics.

### Phase F: Distribution and Claim Promotion

- [x] Qualify package consumption, freeze stable semantics, and publish only
      evidence-backed claims.
- **Status:** complete within the existing local-artifact release boundary
- **Exit evidence:** Web tarball external install/import, Android JNI/AAR/APK,
  Apple host smoke/XCFramework/Swift Package simulator build, browser WASM
  smoke, full workspace tests/conformance/clippy/docs, and claim-scope docs all
  pass. Public npm/Maven/binary SwiftPM publication remains out of scope.

## Key Questions

1. Which advantages are product-level native-vs-browser advantages, and which
   are renderer-level advantages that require Web-vs-Web proof?
2. How should gsplat-rs avoid the 128 MiB storage-binding limit without copying
   PlayCanvas's full resource-to-work-buffer architecture?
3. Which data layout is portable across Vulkan, Metal, and browser WebGPU while
   remaining efficient on tile-based mobile GPUs?
4. Which benchmark protocol prevents quality reduction, backend differences,
   caching, or thermal state from invalidating the comparison?
5. What evidence is required before claiming native leadership or Web parity?

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Treat native leadership and Web parity as separate verification tracks | Native-vs-browser is a valid product comparison, while Web-vs-Web isolates renderer efficiency. |
| Keep `SortedAlpha` as the release-gated quality path | This is the current project boundary and prevents performance claims from weakening correctness. |
| Create a task-scoped active plan bundle | The work is multi-phase and must remain resumable and auditable. |
| Keep the completed design bundle active | The documentation task is complete, but it is now the execution source for unstarted implementation phases A-F. |
| Revise the packed target around a 20-byte hot record | PlayCanvas's pinned compact stream declarations sum to 24 bytes, and its work buffer excludes source SH/order/scratch; gsplat-rs can target lower total residency through a canonical atlas and attribute LOD. |
| Execute sequentially from Phase A | Later performance and memory claims require a versioned baseline and raw artifact contract first. |
| Use subagents for isolated exploration and independent acceptance review | The user explicitly authorized parallel subagents; read-only audits avoid write conflicts while the primary agent integrates implementation. |
| Reset Phase D to `05f649e` and make D0 correctness the only active goal | The later streaming/telemetry proof stack grew before functional streaming and Surface output were established. |
| Enforce one D0 blocker at a time | Feature work unrelated to the current blocker stays frozen; each slice should add fewer than 800 net lines and at most two new files unless justified first. |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| Initial findings patch used an outdated section heading | 1 | Re-read the generated planning files and applied a context-accurate patch. |
| Broad unsupported-claim scan matched explicit warning sentences | 1 | Treat as an expected false positive and use a placeholder-only scan for the final check. |
| Initial bench-runner verification found rustfmt drift | 1 | Apply repository formatting, then rerun `cargo fmt --check`. |
| Artifact CLI test combined `--artifact-dir` with stability mode | 1 | Keep iteration artifacts fail-closed and split artifact parsing into its own iteration-mode test. |
| Clippy rejected the enlarged structured preflight error | 1 | Box the preflight report inside the error variant so public result types remain small while preserving structured diagnostics. |
| Clippy rejected the expanded artifact context helper signature | 1 | Group run identity, timestamps, dataset facts, and file identity in one input struct instead of allowing or suppressing the lint. |
| Initial minimal dataset manifest used assumed bounds | 1 | Run the canonical spatial analysis and replace them with measured bounds before accepting the manifest. |
| iPhone benchmark launch denied because device was locked | 1 | Preserve the successful build separately; rerun the physical-device launch only after the device is unlocked. |
| Browser harness binary-fixture patch used stale README wording | 1 | Re-read the agent-produced README and apply a context-accurate update. |
| PlayCanvas harness found a placement but reported no manager | 1 | Query `GSplatDirector.camerasMap` with the underlying `Camera` (`camera.camera.camera`), not the `CameraComponent`; the fixed harness proves `GSplatHybridRenderer` with GPU sorting active. |
| Web collector represented unavailable git dirty state as `false` | 1 | Permit build commit/dirty to be null only when explicitly listed unavailable, update the validator, and add positive/negative fixtures. |
| iOS create failed with parse error despite valid bundled PLY | 1 | Stale `imported_scene.ply` in Documents shadowed the bundle; uninstall cleared sandbox and kitsune benchmark passed. |
| Phase C dependency fetch could not reach `index.crates.io` over TLS | 1 | Use a per-command reachable sparse registry endpoint without changing repository or user Cargo configuration. |
| Initial Phase C formatting check reported rustfmt drift | 1 | Format only `gsplat-io-spz`, then rerun the focused checks. |
| PLY↔SPZ attribute gate mismatched identity rotations | 1 | Invert PLY RDF→RUF quaternion flips when authoring paired PLY; compare quaternions up to sign. |

## Notes

- Phase D work follows unit tests, offscreen parity, deterministic short traces,
  then device smoke; device long runs come last.
- Evidence ships with the functional slice it proves; documentation-only
  evidence recording is not progress.
- Do not restore the post-`05f649e` streaming telemetry stack before D0 is
  complete and the user explicitly requests it.
- Never commit or push Phase D work without explicit user confirmation.
- Re-read this plan before major architecture and verification decisions.
