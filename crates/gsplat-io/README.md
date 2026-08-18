# gsplat-io

Scene import facade for `gsplat-rs`.

This crate dispatches whole-scene loads to `gsplat-io-ply` or `gsplat-io-spz`
and returns the same validated `gsplat_core::SceneBuffers` the renderer already
consumes. It is not a streaming or LOD runtime.

```rust,no_run
use gsplat_io::load_scene_path;
use std::path::Path;

let loaded = load_scene_path(Path::new("scene.spz")).expect("valid SPZ v4 or PLY");
println!("{}: {} splats", loaded.summary.format.as_str(), loaded.summary.gaussians);
```

`parse_scene_bytes` covers in-memory input. Format comes from the path
extension (`.ply`, `.spz`) or, if the extension is absent or unknown, from
file magic (`ply` header vs SPZ v4 `NGSP`).

## Position in the workspace

Product loaders (desktop example, bench-runner, C ABI path load, wasm
`createRenderer`) go through this crate. Packed on-disk SPZ is still one
resident scene after decode.

## License

MIT OR Apache-2.0, at your option.
