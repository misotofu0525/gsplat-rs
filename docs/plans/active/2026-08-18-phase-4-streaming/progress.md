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

## 2026-08-19

- Slice 2: additive C ABI `gsplat_context_load_scene_bytes` (PLY/SPZ magic,
  API version stays 0.1). Swift `loadScene(bytes:)`. Streamed SOG JSON rejected.
- Slice 3: `crates/gsplat-io-sog` unbundled SOG + `lod-meta.json` parse.
- Slice 4: `StreamedSogSession` selects leaves from metadata first under
  independent source / decoded / gaussian budgets, then decodes only that
  subset. Desktop/bench-runner assemble `lod-meta.json`; C ABI still rejects it.
- Slice 5: bundled `.sog` ZIP whole-scene import (STORED + DEFLATE, zip-bomb
  bounds), native parallel missing-chunk decode, camera-driven desktop
  two-pass `--auto-camera` and interactive `reload_scene`. C ABI bytes sniffs
  ZIP `PK` without a new symbol or version bump.
- Committed fixtures: `tests/datasets/minimal_sog/`,
  `tests/datasets/minimal.sog`, `tests/datasets/minimal_streamed_sog/`.

### Verification

| Command | Result |
|---------|--------|
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --offline` | pass (`gsplat-io` 9; `gsplat-io-sog` 10; ffi-c 17) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | pass |
| `bash tests/security/run-cargo-deny.sh` | pass (advisories/bans/licenses/sources ok) |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | pass (pre-existing wasm unused warnings only) |
| `bash tests/ffi/run-ffi-smoke.sh tests/datasets/minimal_ascii.ply` | pass, drawn=2 |
| `bash tests/ffi/run-ffi-smoke.sh tests/datasets/minimal_v4_degree0.spz` | pass; default camera drawn=0 |
| `bash tests/ffi/run-ffi-smoke.sh tests/datasets/minimal.sog` | pass, drawn=2 |
| `cargo run -p desktop-example -- tests/datasets/minimal_sog/meta.json --auto-camera --png target/out-sog.png` | pass, drawn=2 |
| `cargo run -p desktop-example -- tests/datasets/minimal.sog --auto-camera --png target/out-bundled-sog.png` | pass, drawn=2 |
| `cargo run -p desktop-example -- tests/datasets/minimal_streamed_sog/lod-meta.json --auto-camera --png target/out-streamed-sog.png` | pass, drawn=2 |
| `bash bindings/apple/scripts/run-swift-smoke.sh tests/datasets/minimal_ascii.ply` | pass, drawn=2 (slice 2–4; slice 5 did not change Swift) |
