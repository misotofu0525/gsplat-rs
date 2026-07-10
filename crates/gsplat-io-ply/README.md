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

`parse_ply_bytes` and `parse_ply_text` cover in-memory input, and
`load_ply_summary` reads header metadata without building full buffers.

The default entrypoints apply finite input, header, vertex, property, and
decoded-scene budgets. Applications with tighter memory requirements can use
the matching `*_with_limits` functions:

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

## Position in the workspace

Produces the `SceneBuffers` consumed by `gsplat-render-wgpu` and exposed
through `gsplat-ffi-c` and `gsplat-web`.

## License

MIT OR Apache-2.0, at your option.
