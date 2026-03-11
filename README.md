# gsplat-rs

Cross-platform Gaussian Splatting rendering library built with Rust + `wgpu`.

## Start Here

- Project context: `docs/PROJECT_CONTEXT.md`
- Architecture map: `docs/ARCHITECTURE.md`
- Verification commands: `docs/VERIFICATION.md`
- Current direction and release boundary: `docs/roadmap.md`
- Agent entrypoint: `AGENTS.md`

## Repository Layout

- `crates/`: core library crates, render path, sort backends, format support, and C ABI
- `apps/desktop-demo`: desktop viewer and offscreen PNG harness
- `apps/android-demo`: Android integration demo and JNI smoke path
- `apps/ios-demo`: Swift smoke path and iOS simulator build scripts
- `tools/`: packaging and performance helpers
- `tests/`: sample dataset plus smoke/perf scripts

## Common Commands

```bash
cargo check --workspace
cargo test --workspace
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Use `docs/VERIFICATION.md` for the full validation matrix.
