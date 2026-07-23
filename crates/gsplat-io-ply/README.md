# gsplat-io-ply

PLY import and scene-buffer construction for `gsplat-rs`.

This crate parses 3D Gaussian Splatting `.ply` files (ASCII and binary) and
builds validated `gsplat_core::SceneBuffers` ready for rendering.

```rust,no_run
use gsplat_io_ply::load_ply;
use std::path::Path;

let loaded = load_ply(Path::new("scene.ply")).expect("valid 3DGS ply");
println!("splats: {}", loaded.summary.gaussians);
// loaded.scene is a gsplat_core::SceneBuffers
```

`load_ply` streams file-backed ASCII and binary vertex records directly into
the final scene buffers. `parse_ply_bytes` and `parse_ply_text` cover in-memory
input, and `load_ply_summary` stops after the header without decoding payload.

Consumers that build a compact resident representation can avoid the temporary
wide `SceneBuffers` entirely with `visit_ply_splats` or
`visit_ply_bytes_splats`. Their callbacks receive one fixed-size,
allocation-free `DecodedPlySplat` at a time. The file visitor reads through
`File` + `BufReader`; the byte visitor decodes directly from the caller's
slice. Both use the same vertex decoder and produce bit-identical values:

```rust,no_run
use gsplat_io_ply::{DecodedPlySplat, visit_ply_splats};
use std::path::Path;

let mut decoded = 0_usize;
let summary = visit_ply_splats(Path::new("scene.ply"), |splat: &DecodedPlySplat| {
    // Encode `splat` directly into the caller's compact resident buffers here.
    assert_eq!(splat.sh_rest_coefficients().len(), usize::from(splat.sh_rest_len));
    decoded += 1;
})?;
assert_eq!(decoded, summary.gaussians);
# Ok::<(), gsplat_io_ply::PlyLoadError>(())
```

The visitor is infallible by design. A parse or I/O error is returned by the
loader; if it occurs after valid vertices, their callbacks have already run.
`visit_ply_splats_with_limits` applies the same explicit budgets as
`load_ply_with_limits`, including the logical decoded-scene byte budget. The
matching byte entrypoint is `visit_ply_bytes_splats_with_limits`.

`load_ply_summary` and `parse_ply_bytes_summary` decode no numeric vertex
attributes. Before returning header metadata they validate the available body
shape/size, so an exact-count compact builder can safely reserve its final
planes before the corresponding visitor starts without constructing wide scene
buffers.

The default entrypoints are finite: they accept at most `2 GiB - 1 byte` of
input, 8,388,608 vertices, a 1 MiB header, 128 vertex properties, and
`2 GiB - 1 byte` of logical decoded scene data. The byte ceilings deliberately
do not exceed `isize::MAX` on wasm32 and other 32-bit targets. This envelope
admits the complete validation corpus, including Bicycle at 6,131,954 SH3
splats (1,520,726,124 input bytes and 1,447,141,144 logical decoded bytes),
while rejecting forged headers before unchecked work or allocation.

Allocation can still fail with a structured `AllocationFailed` error when a
device lacks enough memory. Applications with a tighter or deliberately
different memory policy can use the matching `*_with_limits` functions:

```rust,no_run
use gsplat_io_ply::{PlyLoadLimits, load_ply_with_limits};
use std::path::Path;

let limits = PlyLoadLimits {
    max_vertices: 500_000,
    max_scene_bytes: 256 * 1024 * 1024,
    ..PlyLoadLimits::default()
};
let loaded = load_ply_with_limits(Path::new("scene.ply"), limits)?;
# Ok::<(), gsplat_io_ply::PlyLoadError>(())
```

## Input conventions

- Quaternion fields `rot_0..3` are interpreted as `w,x,y,z` and remapped
  internally to `x,y,z,w`.
- Input 3DGS coordinates are treated as `RDF` and converted at load time to the
  runtime `RUF` convention, including quaternion and SH sign transforms.
- If any `f_rest_*` field is present, its indices must be unique and contiguous
  and its total count must be exactly 9, 24, or 45 (SH degree 1, 2, or 3).
- `DecodedPlySplat` contains RUF position, opacity logit, log scale, RUF `xyzw`
  rotation, DC color, and a fixed `[f32; 45]` SH payload. `sh_degree` and
  `sh_rest_len` identify the valid SH0-SH3 prefix; the unused tail is zero.

## Position in the workspace

Produces the `SceneBuffers` consumed by `gsplat-render-wgpu` and exposed
through `gsplat-ffi-c` and `gsplat-web`.

## License

MIT OR Apache-2.0, at your option.
