# Findings: Phase 4 streaming / LOD

## Current import surface (2026-08-18)

- `gsplat-io-spz` already loads Niantic SPZ v4 (`NGSP` plaintext header, independent ZSTD streams) into validated `SceneBuffers`, with limits, cancellation, RUB→RUF conversion, and `SourceResidencyCaches`.
- Committed fixture: `tests/datasets/minimal_v4_degree0.spz` (8 splats, degree 0).
- Host image parity vs PLY already exists in `gsplat-render-wgpu` (`ply_vs_spz_offscreen_image_parity_gate_on_minimal_fixture`).
- Slice 1 product consumers go through `gsplat-io`: desktop, bench-runner, `gsplat_context_load_scene_path`, Android/iOS Surface create, and wasm `createRenderer`.
- Stable C ABI still has path load, not scene-from-memory.

## Slice 1 contract

This is **whole-scene import**. Decode produces one resident `SceneBuffers` and the existing renderer uploads it. Capacity preflight still fails explicitly. Compressed on-disk SPZ is not streaming, just as quantized GPU storage is not streaming.

Format detection:

- `.ply` → PLY parser
- `.spz` → SPZ v4 parser
- otherwise sniff magic: `ply\n` / `ply\r` vs `NGSP`

## Explicitly out of slice 1

- `gsplat_context_load_scene_bytes` (new C symbol)
- Replacing Android/iOS bundled `showcase.ply`
- Streamed SOG / splat-transform
- Async chunk decode, spatial LOD, GPU residency distinct from source caches
- Custom internal cache format

## Later-slice anchors (not started)

- PlayCanvas Streamed SOG: `lod-meta.json` + chunked payloads, open `splat-transform` toolchain
- SPZ v4 remains the cross-vendor interchange; Khronos `KHR_gaussian_splatting` + planned SPZ streaming extension are watch-only until ratified
- Retired Paged lesson: a full CPU `SceneBuffers` plus fixed GPU slots is not bounded streaming
