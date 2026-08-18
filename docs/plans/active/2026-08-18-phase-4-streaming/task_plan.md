# Task Plan: Phase 4 — Ecosystem-aligned streaming / LOD

## Goal

Start ROADMAP item 4 without disguising whole-scene residency as streaming.
First cut: promote the existing bounded SPZ v4 loader into the shared product
import surface (desktop, C path load, Web wasm, mobile path-create). Later
cuts: Streamed SOG, then metadata-first streaming/LOD.

## Current Phase

Slice 2 — C ABI scene-from-memory (not started)

## Phases

### Slice 1: Whole-scene SPZ productization
- [x] Add a thin `gsplat-io` facade over PLY + SPZ v4 (extension + magic)
- [x] Wire desktop-example, bench-runner, C ABI path load, wasm `createRenderer`
- [x] Web example: accept `.spz` on the wasm path only
- [x] Docs: this is whole-scene `SceneBuffers` import, not streaming
- [x] Verify: `gsplat-io` tests, FFI smoke on PLY and SPZ, desktop PNG on SPZ
- **Status:** complete

### Slice 2: C ABI scene-from-memory (separate release-boundary decision)
- [ ] Decide whether `gsplat_context_load_scene_bytes` belongs in v0.1
- **Status:** pending

### Slice 3: Streamed SOG read support
- [ ] PlayCanvas `lod-meta.json` + chunked payloads from splat-transform
- **Status:** pending

### Slice 4: Metadata-first streaming / LOD
- [ ] Independent source / CPU / GPU budgets; no Packed/Paged revival
- **Status:** pending

## Key Questions

1. First cut vs full streaming? Whole-scene SPZ first. Streaming is a new architecture.
2. New C ABI symbol for in-memory load? Not in slice 1. Existing `load_scene_path` gains `.spz`.
3. New crate vs copy dispatch in four consumers? Thin `gsplat-io` facade.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Slice 1 is whole-scene SPZ, not streaming | GOLDEN_PRINCIPLES: never disguise full `SceneBuffers` as streaming |
| No new C ABI function in slice 1 | Path load already exists; memory load is a release-boundary widen |
| Add `crates/gsplat-io` | Four consumers need the same PLY/SPZ dispatch; keep ply/spz crates focused |
| WebGL2 fallback stays PLY-only | No JS SPZ decoder; SPZ requires the wasm renderer |
| Do not replace mobile bundled `showcase.ply` | Path create will load `.spz` when the caller passes one |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
|       | 1       |            |

## Notes

- Kill criterion for later streaming work: if an implementation keeps a full decoded `SceneBuffers` plus a slot/page selector, delete it. That is the retired Paged lesson.
- Do not invent a proprietary scene format. Track Streamed SOG and `KHR_gaussian_splatting`.
