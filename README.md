# gsplat-rs

Cross-platform Gaussian Splatting rendering library built with Rust + wgpu.

## Current status

`SortedAlpha` is the only quality-guaranteed path for `v0.1.x`.

- Stable docs live in `README.md`, `docs/architecture.md`, `docs/api.md`, and `docs/roadmap.md`.
- Historical planning and execution logs live under `docs/archive/` and `docs/agent-notes/`.

## Documentation

- ADR: `docs/adr/0001-v0.1-sortedalpha-only.md`
- Architecture: `docs/architecture.md`
- API notes: `docs/api.md`
- Roadmap: `docs/roadmap.md`
- Release checklist: `docs/releases/v0.1.0/checklist.md`

## Workspace layout

- `crates/gsplat-core`: shared public types, config, stats, and error codes
- `crates/gsplat-io-ply`: required-field PLY parser
- `crates/gsplat-sort`: sort backend abstraction + CPU fallback + GPU compute sort backend
- `crates/gsplat-render-wgpu`: preprocess/sort + WGSL covariance-driven anisotropic splat render path
- `crates/gsplat-format`: packed scene format primitives
- `crates/gsplat-ffi-c`: stable C ABI surface for mobile wrappers
- `apps/desktop-demo`: desktop viewer and development harness
- `apps/android-demo`: Android demo app + host JNI smoke
- `apps/ios-demo`: iOS smoke/build scripts
- `tools/gsplat-pack`: PLY -> packed format converter
- `tools/bench-runner`: perf + stability runner
- `tests/datasets`: test datasets

PLY convention note:
- Input quaternion properties `rot_0..3` are interpreted as `w,x,y,z` (common 3DGS export order) and mapped internally to `x,y,z,w`.
- Input 3DGS PLY coordinates are treated as `RDF` (`+X` right, `+Y` down, `+Z` forward) and converted at load time to runtime `RUF` (`+X` right, `+Y` up, `+Z` forward), including quaternion and SH sign transforms.

## Gate checks

```bash
cargo check --workspace
cargo test --workspace
cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120
cargo run -p gsplat-pack -- tests/datasets/minimal_ascii.ply target/minimal.gspk --verify
bash tests/ffi/run-ffi-smoke.sh
bash apps/android-demo/run-jni-smoke.sh
bash apps/ios-demo/run-swift-smoke.sh
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Desktop demo

Render an offscreen frame and write a PNG (requires a GPU adapter):

```bash
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Run interactive on-screen realtime viewer loop (feature-gated):

```bash
cargo run -p desktop-demo --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
```

Interactive controls:

- Mouse left drag / arrow keys: look
- `W`/`S`: forward/backward
- `A`/`D`: strafe left/right
- `Q`/`E`: move down/up
- `Shift`: faster movement
- `Ctrl`: slower movement
- `Esc`: exit viewer

Fetch a real 3DGS dataset (binary PLY with `f_rest_0..44`) and render it:

```bash
bash tests/datasets/fetch-nvidia-flowers-1.sh
cargo run -p desktop-demo -- tests/datasets/external/nvidia_flowers_1/model.ply --auto-camera --png target/flowers_1.png
```

## Mobile container builds

```bash
bash apps/ios-demo/build-ios-sim.sh
bash apps/android-demo/build-apk.sh
```

## C ABI (v0.1 frozen surface)

- `gsplat_context_create`
- `gsplat_context_destroy`
- `gsplat_context_set_camera`
- `gsplat_context_load_scene_path`
- `gsplat_context_render_frame`
- `gsplat_context_get_stats`

Header path: `crates/gsplat-ffi-c/include/gsplat.h`
