# Desktop Example

Desktop viewer and offscreen PNG smoke harness.

Run the deterministic PNG smoke from the repository root:

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
```

The renderer keeps PLY-derived scene buffers resident on the GPU, sorts depth
on the CPU, uploads compact source IDs, and prints
`offscreen_geometry_pipeline=sorted_index_direct`. Benchmark the same path with:

```bash
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10
```

Run the interactive viewer when validating windowed presentation or camera
interaction. It uses the shared `SurfaceRenderSession` also used by Web and
mobile, with per-camera-frame CPU sorting and direct sorted indices as its
default policy:

```bash
cargo run -p desktop-example --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```

Windowed and offscreen rendering use the same direct shader/resource layout.
