# Desktop Example

Desktop viewer and offscreen PNG smoke harness.

Run the deterministic PNG smoke from the repository root:

```bash
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Run the interactive viewer when validating windowed presentation or camera
interaction:

```bash
cargo run -p desktop-demo --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```
