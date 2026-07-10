# Task Plan: Unified Render Pipeline Refactor

## Goal

Unify Web and native Surface rendering behind one shared render session while preserving CPU depth sorting, explicit geometry pipelines, the small C ABI, SortedAlpha output, and verified mobile/Web/desktop behavior.

## Current Phase

Complete

## Phases

### Phase 1: Architecture contract and branch baseline

- [x] Rename the current branch to `refactor/unified-render-pipeline`.
- [x] Record the target architecture, invariants, compatibility boundary, and migration sequence.
- [x] Capture the current duplicate Web/FFI orchestration and existing mobile defaults.
- **Status:** complete

### Phase 2: Shared Surface render session

- [x] Add an explicit geometry-pipeline configuration instead of path booleans in shared Rust code.
- [x] Add a shared `SurfaceRenderSession` that owns renderer, presenter, order/upload state, cache revisions, and frame stats.
- [x] Keep CPU radix sorting and reuse the existing direct, compute-preproject, and CPU-reference pipelines.
- [x] Add unit tests for path selection, sort cadence, and path-aware cached redraw behavior.
- **Status:** complete

### Phase 3: Web and native migration

- [x] Migrate `gsplat-web` to the shared session and remove its duplicated frame scheduler.
- [x] Migrate the C FFI Surface handle used by Android/iOS to the shared session.
- [x] Preserve current C ABI setters and Android/Swift option defaults through compatibility mapping.
- [x] Keep experimental async controls compatible or explicitly isolate them from the shared default path.
- **Status:** complete

### Phase 4: Desktop/offscreen consolidation and observability

- [x] Align desktop/offscreen path naming and configuration with the shared geometry-pipeline model.
- [x] Remove unnecessary sorted-index copies where ownership permits.
- [x] Split or clarify timing metrics so CPU geometry work is not reported as upload time.
- **Status:** complete

### Phase 5: Current docs sync

- [x] Update project context, architecture, verification, and platform/package docs to match the implemented structure.
- [x] Archive superseded/completed plan bundles when their work is no longer active.
- [x] Keep AGENTS.md thin unless a canonical path or load-order rule changes.
- **Status:** complete

### Phase 6: Cross-platform verification and completion

- [x] Run formatting, workspace check/test/clippy/doc, GPU conformance, and desktop image A/B.
- [x] Build and test the Web package/WASM path, including stationary direct-path browser smoke.
- [x] Run FFI, JNI, Android AAR/APK, Swift/XCFramework, and available iOS simulator verification.
- [x] Record device-only gaps honestly and finish only after all locally available required checks pass.
- **Status:** complete

## Key Questions

1. What is the smallest shared session API that removes duplicated scheduling without widening the v0.1 C ABI?
2. Which experimental native options belong inside the shared session, and which should remain compatibility-only adapters?
3. How should cached resources be represented so every acquired Surface texture is redrawn with the active pipeline?
4. Which metrics can be measured consistently across CPU-reference, compute-preproject, and direct-index paths?

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Keep CPU depth sorting as the default on all platforms | Current sort cost is modest, deterministic, and uses otherwise available CPU capacity on mobile while preserving GPU budget for projection, SH, blending, and presentation. |
| Refactor inside existing crates | The project deliberately keeps a small repository shape; no new crate is required. |
| Make geometry path explicit | `SortedIndexDirect`, `SortedIndexGpuPreproject`, and `CpuInstances` express mutually exclusive pipelines better than interacting booleans. |
| Centralize Surface scheduling | The Web stationary-frame regression demonstrates that wrapper-owned cache/path logic has already diverged from native behavior. |
| Preserve the existing C ABI | Android and Apple wrappers depend on the current setters; compatibility mapping is safer than widening the v0.1 boundary during an internal refactor. |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `gsplat-web` match became non-exhaustive after adding the preproject Surface variant | 1 | Expected migration pressure; replace the wrapper-owned path match with the shared `SurfaceRenderSession`. |

## Notes

- Re-read this plan before every major implementation decision.
- Update `findings.md` after architecture/source discoveries and `progress.md` after each phase or failure.
- Completion requires fresh verification evidence; passing compilation alone is insufficient.
