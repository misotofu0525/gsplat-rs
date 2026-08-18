# Progress: Phase 4 streaming / LOD

## 2026-08-18

- Started ROADMAP item 4 after items 1–3 closed.
- Locked slice 1 as whole-scene SPZ productization (not streaming).
- Added `crates/gsplat-io` facade (extension + `NGSP`/`ply` magic).
- Wired desktop-example, bench-runner, C ABI path load (offscreen + Android/iOS
  Surface create), and wasm `createRenderer`.
- Web example accepts `.spz` on the wasm path; WebGL2 fallback stays PLY-only.

### Verification

| Command | Result |
|---------|--------|
| `cargo fmt --check` | pass |
| `cargo check --workspace` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --offline` | pass (`gsplat-io` 5; ffi-c 14; io-spz 20; render-wgpu 71+1 ignored) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | pass |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | pass (pre-existing wasm unused warnings only) |
| `bash tests/ffi/run-ffi-smoke.sh tests/datasets/minimal_ascii.ply` | pass, drawn=2 |
| `bash tests/ffi/run-ffi-smoke.sh tests/datasets/minimal_v4_degree0.spz` | pass; default camera drawn=0 |
| `cargo run -p desktop-example -- tests/datasets/minimal_v4_degree0.spz --auto-camera --png target/out-spz-auto.png` | pass, drawn=8 |
| `node --check examples/web/src/main.js` | pass |
| `npm --prefix packages/web test` | 8 pass |

Default-camera SPZ drawn=0 is framing, not a load failure. `--auto-camera` draws all 8 fixture splats.
