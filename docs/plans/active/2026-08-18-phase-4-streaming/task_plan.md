# Task Plan: Phase 4 — Ecosystem-aligned streaming / LOD

## Goal

Start ROADMAP item 4 without disguising whole-scene residency as streaming.
First cut: promote the existing bounded SPZ v4 loader into the shared product
import surface (desktop, C path load, Web wasm, mobile path-create). Later
cuts: C ABI scene-from-memory, Streamed SOG, then metadata-first streaming/LOD.

## Current Phase

Slice 6 — independent GPU-resident gaussian budget (not a page pool)

## Phases

### Slice 1: Whole-scene SPZ productization
- [x] Add a thin `gsplat-io` facade over PLY + SPZ v4 (extension + magic)
- [x] Wire desktop-example, bench-runner, C ABI path load, wasm `createRenderer`
- [x] Web example: accept `.spz` on the wasm path only
- [x] Docs: this is whole-scene `SceneBuffers` import, not streaming
- [x] Verify: `gsplat-io` tests, FFI smoke on PLY and SPZ, desktop PNG on SPZ
- **Status:** complete

### Slice 2: C ABI scene-from-memory
- [x] Add `gsplat_context_load_scene_bytes` (PLY/SPZ magic; keep API 0.1)
- [x] Swift `loadScene(bytes:)` + FFI smoke path and bytes coverage
- [x] Reject Streamed SOG JSON on this whole-scene ABI
- **Status:** complete

### Slice 3: Streamed SOG read support
- [x] `gsplat-io-sog` unbundled SOG (`meta.json` + 8-bit images)
- [x] PlayCanvas `lod-meta.json` parse + per-chunk range decode
- **Status:** complete

### Slice 4: Metadata-first streaming / LOD
- [x] `StreamedSogSession` with independent source / decoded / gaussian budgets
- [x] Camera-based leaf + LOD selection; no-camera requires all LOD 0 to fit
- [x] Desktop / bench-runner assemble subset; C ABI still rejects `lod-meta.json`
- **Status:** complete

### Slice 5: Bundled `.sog` + camera-driven session
- [x] Whole-scene bundled `.sog` ZIP (STORED + DEFLATE, zip-bomb bounds)
- [x] Native parallel missing-chunk decode; wasm stays serial
- [x] Selection fingerprint; desktop `--auto-camera` two-pass and interactive reload
- [x] GPU streaming residency (not this slice)
- [ ] C ABI streaming/LOD (not this slice)
- **Status:** complete for bundled ZIP + camera-driven desktop session

### Slice 6: Independent GPU-resident budget
- [x] `StreamingBudgets.max_resident_gaussians` + `peek_sh_degree`
- [x] `Renderer::max_resident_gaussians` from device limits or downlevel
- [x] Desktop / bench-runner apply the GPU cap before assemble
- [ ] C ABI streaming/LOD (not this slice)
- **Status:** complete; not a GPU page pool

## Key Questions

1. First cut vs full streaming? Whole-scene SPZ first. Streaming is a new architecture.
2. New C ABI symbol for in-memory load? Slice 2 adds it; version stays 0.1.
3. New crate vs copy dispatch in four consumers? Thin `gsplat-io` facade.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Slice 1 is whole-scene SPZ, not streaming | GOLDEN_PRINCIPLES: never disguise full `SceneBuffers` as streaming |
| Slice 2 adds `load_scene_bytes` without bumping minor | Additive C symbol; existing version checks keep working |
| Streamed SOG stays off the C ABI | Multi-file format; whole-scene bytes API cannot represent it |
| `StreamedSogSession` selects from metadata first | Kill criterion: no full decoded scene plus slot selector |
| Assemble subset into one `SceneBuffers` for the current renderer | Honest about renderer residency; not a page pool |
| Bundled `.sog` ZIP is whole-scene import | Same decode as unbundled; C bytes path sniffs `PK` without a new symbol |
| Native parallel chunk decode is `thread::scope` | wasm32 stays serial; `SogError` is `Send` |
| Camera-driven desktop reuses `reload_scene` | Not a C ABI change; fingerprint skip avoids redundant GPU rebuilds |
| GPU cap is `max_resident_gaussians`, not a page pool | Kill criterion: still one assembled `SceneBuffers` + existing preflight |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
|       | 1       |            |

## Notes

- Kill criterion for later streaming work: if an implementation keeps a full decoded `SceneBuffers` plus a slot/page selector, delete it. That is the retired Paged lesson.
- Do not invent a proprietary scene format. Track Streamed SOG and `KHR_gaussian_splatting`.
