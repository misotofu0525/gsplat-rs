# Desktop Example

Desktop viewer and offscreen PNG smoke harness.

Run the deterministic PNG smoke from the repository root:

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Opt into the experimental GPU-resident + sorted-index offscreen path (same
family as mobile `static_direct` / Web `sortedIndexDirect`; CPU sort still
applies). The run prints `offscreen_raster_path=sorted_index_gpu_preproject`:

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png --sorted-index-direct
```

`bench-runner` accepts the same flag and reports the path next to GPU adapter
metadata:

```bash
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10 --sorted-index-direct
```

Run the interactive viewer when validating windowed presentation or camera
interaction:

```bash
cargo run -p desktop-example --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```
