# gsplat-io

Scene import facade for `gsplat-rs`.

This crate dispatches whole-scene loads to `gsplat-io-ply`, `gsplat-io-spz`, or
PlayCanvas SOG (unbundled `meta.json` or bundled `.sog` ZIP) and returns the
same validated `gsplat_core::SceneBuffers` the renderer already consumes.

```rust,no_run
use gsplat_io::load_scene_path;
use std::path::Path;

let loaded = load_scene_path(Path::new("scene.spz")).expect("valid SPZ v4, PLY, or SOG");
println!("{}: {} splats", loaded.summary.format.as_str(), loaded.summary.gaussians);
```

`parse_scene_bytes` covers in-memory PLY / SPZ v4 / bundled `.sog` ZIP. Format
comes from the path extension (`.ply`, `.spz`, `.sog`) or, if the extension is
absent or unknown, from file magic (`ply` header, SPZ v4 `NGSP`, or ZIP `PK`).
Unbundled SOG is a directory of images, so it is path-only.

Streamed SOG (`lod-meta.json`) is **not** whole-scene import. `load_scene_path`
returns a structured `StreamingRequired` error. Use `assemble_streamed_sog` to
select a budgeted subset from spatial metadata, then hand that subset to the
existing resident renderer.

## Position in the workspace

Product loaders (desktop example, bench-runner, C ABI path/bytes load, wasm
`createRenderer`) go through this crate. Packed on-disk SPZ and whole-scene SOG
are still one resident scene after decode.

## License

MIT OR Apache-2.0, at your option.
