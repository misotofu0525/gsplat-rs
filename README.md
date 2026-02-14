# gsplat-rs

Cross-platform Gaussian Splatting rendering library built with Rust + wgpu.

## v0.1 quality contract

`SortedAlpha` is the only quality-guaranteed path for `v0.1.x`.

See ADR: `docs/adr/0001-v0.1-sortedalpha-only.md`.
Execution progress: `docs/v0.1.0-subagent-execution.md`.

## Workspace layout

- `crates/gsplat-core`: shared public types, config, stats, and error codes
- `crates/gsplat-io-ply`: required-field PLY parser
- `crates/gsplat-sort`: sort backend abstraction + CPU fallback + GPU compute sort backend
- `crates/gsplat-render-wgpu`: preprocess/sort + WGSL covariance-driven anisotropic splat render path
- `crates/gsplat-format`: packed scene format primitives
- `crates/gsplat-ffi-c`: stable C ABI surface for mobile wrappers
- `tools/gsplat-pack`: PLY -> packed format converter
- `tools/bench-runner`: perf + stability runner
- `tests/datasets`: test datasets

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

## Desktop dev

Render an offscreen frame and write a PNG (requires a GPU adapter):

```bash
cargo run -p desktop-dev -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Fetch a real 3DGS dataset (binary PLY with `f_rest_0..44`) and render it:

```bash
bash tests/datasets/fetch-nvidia-flowers-1.sh
cargo run -p desktop-dev -- tests/datasets/external/nvidia_flowers_1/model.ply --auto-camera --png target/flowers_1.png
```

## Mobile container builds

```bash
bash apps/ios-demo/build-ios-sim.sh
bash apps/android-demo/build-apk.sh
```

## C ABI draft (v0.1 freeze)

- `gsplat_context_create`
- `gsplat_context_destroy`
- `gsplat_context_set_camera`
- `gsplat_context_load_scene_path`
- `gsplat_context_render_frame`
- `gsplat_context_get_stats`

Header path: `crates/gsplat-ffi-c/include/gsplat.h`
