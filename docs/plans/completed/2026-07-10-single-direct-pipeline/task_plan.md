# Task Plan: Single Direct Runtime Pipeline

## Goal

Collapse all production rendering onto CPU depth sort plus `SortedIndexDirect`, add direct offscreen rendering, retain CPU projection only as test/reference code, remove GPU-preproject and runtime CPU-instance branches, then verify, commit, push, and publish a draft PR.

## Current Phase

Complete

## Phases

### Phase 1: Baseline and deletion contract

- [x] Inventory every runtime geometry-pipeline branch, public adapter, shader/resource owner, CLI option, and verification dependency.
- [x] Define compatibility behavior for the existing C/Web experimental toggles.
- [x] Record the exact code and documentation deletion boundary.
- **Status:** completed

### Phase 2: Direct offscreen implementation

- [x] Add a direct sorted-index render path to the native offscreen rasterizer.
- [x] Keep CPU sort and persistent scene buffers unchanged.
- [x] Prove direct offscreen output against the current CPU reference before deleting alternatives.
- **Status:** completed

### Phase 3: Runtime pipeline collapse

- [x] Make `SortedIndexDirect` the only Surface and offscreen production pipeline.
- [x] Remove GPU preproject, async geometry, runtime CPU instance construction, pipeline selection, and obsolete buffer/config branches.
- [x] Preserve source-compatible C/Web compatibility setters as documented direct-path shims where required.
- **Status:** completed

### Phase 4: Reference/test isolation and docs sync

- [x] Move or constrain CPU projection helpers to conformance/reference use rather than runtime selection.
- [x] Update handbook, platform/package docs, examples, and metrics to the single production path.
- [x] Archive this plan only after verification succeeds.
- **Status:** completed

### Phase 5: Verification and publication

- [x] Run Rust/Web/desktop/GPU/FFI/Android/Apple verification appropriate to the final diff.
- [x] Review final scope and exclude unrelated `bindings/android/build/` content.
- [x] Prepare the verified refactor for one intentional commit, push, and draft PR against the default branch.
- **Status:** completed

## Decisions

| Decision | Rationale |
|----------|-----------|
| Keep CPU depth sorting | It is deterministic, already shared, and preserves mobile GPU budget for rendering. |
| Keep `SortedIndexDirect` as the sole production geometry path | It shares resident scene buffers across targets, uploads only compact order IDs, and removes both full instance uploads and a compute-preproject pass. |
| Retain CPU projection only as a reference | A non-runtime oracle remains valuable for shader/image conformance without preserving a selectable production branch. |
| Preserve the v0.1 C ABI | Existing experimental setters can map to the sole path without widening or deleting exported symbols in this change. |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| Findings patch heading mismatch | First plan update | Re-read the bundle and applied against current headings. |
| Removed enum variants still referenced | First workspace compile | Replaced offscreen runtime match and removed CLI/FFI branch references. |
| Used non-existent `SceneBuffers::color_rest` | Direct scene resource compile | Switched to `SceneBuffers::sh_rest` with a one-float fallback. |
| Test-only `GpuRasterError` import removed too broadly | First workspace test compile | Restored the enum in the native test-only import. |

## Notes

- Re-read this plan before each major deletion decision.
- Log every failed compile/test and update phase status immediately.
- Publishing means commit + push + draft PR; no release tag or GitHub Release is implied.
- The archived plan records the verified repository state immediately before the external commit/push/PR steps.
