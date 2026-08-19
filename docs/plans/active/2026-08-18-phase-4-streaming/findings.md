# Findings: Phase 4 streaming / LOD

## Current import surface (2026-08-18)

- `gsplat-io-spz` already loads Niantic SPZ v4 (`NGSP` plaintext header, independent ZSTD streams) into validated `SceneBuffers`, with limits, cancellation, RUB→RUF conversion, and `SourceResidencyCaches`.
- Committed fixture: `tests/datasets/minimal_v4_degree0.spz` (8 splats, degree 0).
- Host image parity vs PLY already exists in `gsplat-render-wgpu` (`ply_vs_spz_offscreen_image_parity_gate_on_minimal_fixture`).
- Slice 1 product consumers go through `gsplat-io`: desktop, bench-runner, `gsplat_context_load_scene_path`, Android/iOS Surface create, and wasm `createRenderer`.
- Slice 2 adds `gsplat_context_load_scene_bytes` (PLY/SPZ magic). API version stays 0.1.
- Slice 3–4 add `gsplat-io-sog`. Unbundled `meta.json` is whole-scene. `lod-meta.json` is metadata-first subset assembly.
- Slice 5 adds bundled `.sog` ZIP whole-scene import, native parallel chunk decode, and a camera-driven desktop Streamed SOG session.

## Slice 1 contract

This is **whole-scene import**. Decode produces one resident `SceneBuffers` and the existing renderer uploads it. Capacity preflight still fails explicitly. Compressed on-disk SPZ is not streaming, just as quantized GPU storage is not streaming.

Format detection:

- `.ply` → PLY parser
- `.spz` → SPZ v4 parser
- otherwise sniff magic: `ply\n` / `ply\r` vs `NGSP`

## Slice 2 contract

`gsplat_context_load_scene_bytes(ctx, bytes, byte_count)`:

- null/empty → `InvalidArgument`
- magic `ply` / `NGSP` / ZIP `PK` → same resident import as path load
- Streamed SOG JSON → `Unsupported` (`StreamingRequired`)
- unbundled SOG JSON without images → `ParseFailed`
- no `surface_create_from_bytes`; Surface create stays path-based
- no minor version bump

## Slice 3–4 contract

PlayCanvas SOG v2 (https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/sog/):

- means are 16-bit split across `means_l` / `means_u`, log-domain then unlog
- scales codebook is log-domain and matches `SceneBuffers.scale_xyz`
- sh0 codebook is DC coefficients; opacity byte is alpha, converted to logit
- quats are smallest-three, wxyz, mode in A = 252..=255
- coordinates are RUB; runtime is RUF (same flips as SPZ)
- images may be lossless WebP or other 8-bit formats named in `meta.json`; fixtures use PNG

PlayCanvas Streamed SOG v1 (https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/streamed-sog/):

- identify by filename `lod-meta.json`
- chunks are unbundled SOG directories
- `StreamedSogSession` reads the tree first, then stats/decodes only selected leaves
- independent budgets: `max_source_bytes`, `max_decoded_bytes`, `max_gaussians`
- no camera: every LOD 0 leaf must fit, or `ResourceLimit`
- with camera: keep nearest leaves, drop farther; a single environment or nearest leaf that exceeds the full budget is `ResourceLimit`
- GPU budget remains renderer capacity preflight after upload
- bundled `.sog` ZIP is whole-scene import (STORED + DEFLATE, zip-bomb bounds)
- native missing-chunk decode uses `std::thread::scope`; wasm stays serial
- desktop `--auto-camera` and interactive Streamed SOG reassemble when the
  selection fingerprint changes

Kill criterion: do not load every chunk into one `SceneBuffers` and then pick slots. That is the retired Paged lesson.

## Explicitly later

- Replacing Android/iOS bundled `showcase.ply`
- GPU residency distinct from the CPU subset
- C ABI streaming/LOD
- Custom internal cache format
- Khronos `KHR_gaussian_splatting` + planned SPZ streaming extension (watch-only)
