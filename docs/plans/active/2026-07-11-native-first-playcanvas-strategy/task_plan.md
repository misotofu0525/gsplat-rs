# Task Plan: Native-First PlayCanvas Competitive Strategy

## Goal

Implement and qualify the repository-backed native-first architecture: win on
Android/iOS native performance, keep desktop Web performance-competitive, and
advance only when each phase's correctness, memory, performance, stability, and
comparison gates have fresh evidence.

## Current Phase

Implementation Phase B is complete under the matched `sort_interval=4`
sync-orbit qualification protocol. Phase C (compressed sources / bounded
decode) is next; first format candidate is Niantic SPZ (MIT, v4 ZSTD).
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
  stress case. Phases C-F remain gated.
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
- [ ] Prove device/image parity for qualification scenes (offscreen first).
- **Status:** in_progress; Phase B gate cleared; SPZ feasibility accepted (MIT,
  interoperable, not a custom public format); synthetic SH decode covered
  through degree 3; fixture + cancel/attribute gates landed unit-side;
  CPU residency caches bounded with independent budgets

### Phase D: Spatial Pages and Streaming LOD

- [ ] Implement page scheduling, residency generations, and spatial/attribute LOD.
- [ ] Prove bounded memory/queues, continuity, and large-scene gates.
- **Status:** pending; blocked on Phase C exit gate

### Phase E: Policy Optimization and Competitive Qualification

- [ ] Tune device profiles and evaluate optional GPU culling/sort only by evidence.
- [ ] Pass native leadership, Web parity, sustained thermal, and energy gates.
- **Status:** pending; blocked on Phase D exit gate

### Phase F: Distribution and Claim Promotion

- [ ] Qualify package consumption, freeze stable semantics, and publish only
      evidence-backed claims.
- **Status:** pending; blocked on Phase E exit gate

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

- Preserve the existing untracked `bindings/android/build/` directory.
- The user authorized implementation of Phases A-F and parallel subagent help.
- Do not mark a phase complete when a required physical-device, quality, memory,
  or competitor gate lacks fresh evidence; record it as pending or blocked.
- Re-read this plan before major architecture and verification decisions.
