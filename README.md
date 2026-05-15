# gsplat-rs

Cross-platform Gaussian Splatting rendering library built with Rust + `wgpu`.

## Start Here

- Project context: `handbook/PROJECT_CONTEXT.md`
- Architecture map: `handbook/ARCHITECTURE.md`
- Verification commands: `handbook/VERIFICATION.md`
- Current direction and release boundary: `handbook/ROADMAP.md`
- Agent entrypoint: `AGENTS.md`

## Repository Layout

- `crates/`: core library crates, render path, sort backends, format support, C ABI, and experimental Web bindings
- `apps/desktop-demo`: desktop viewer and offscreen PNG harness
- `apps/android-demo`: Android integration demo and JNI smoke path
- `apps/ios-demo`: Swift smoke path, UIKit realtime Surface app, and iOS simulator/device build/run scripts
- `apps/web-demo`: static browser demo for PLY loading and SortedAlpha-style WebGL2 preview
- `tools/`: packaging and performance helpers
- `tests/`: sample dataset plus smoke/perf scripts

## Mobile Integration Status

The current mobile-facing contract is the C ABI in `crates/gsplat-ffi-c/include/gsplat.h`.
Android and iOS directories are validation demos and starter integrations, not packaged
AAR/XCFramework SDK artifacts yet.

- Use `gsplat_config_default()` and `gsplat_camera_default()` instead of hand-writing ABI defaults.
- Use `GSPLAT_RENDER_MODE_SORTED_ALPHA`; it is the only release-gated render mode in v0.1.
- Treat non-zero returns as `GsplatErrorCode` values and pass them to `gsplat_error_message()`.
- Android Surface rendering is demonstrated by `apps/android-demo`.
- Swift/C ABI integration and a UIKit realtime simulator/device Surface demo are demonstrated by `apps/ios-demo`.
- Both mobile demos are realtime validation surfaces with touch camera controls, local PLY import, and benchmark/A-B knobs over the same C ABI Surface functions.
- Not in the v0.1 contract: scene-from-memory loading, runtime render-mode switching, AAR packaging, and XCFramework packaging.

## Web Demo Status

`apps/web-demo` is the browser validation surface and generated wasm package
host. When `apps/web-demo/pkg/` exists it attempts the `crates/gsplat-web`
Rust/WASM renderer first; otherwise it falls back to the WebGL2 point-splat
preview. The formal Web SDK path now starts in `crates/gsplat-web`: it exposes a
`wasm-bindgen` boundary that accepts a browser canvas plus PLY bytes, parses via
`gsplat-io-ply`, and renders through the shared Rust `wgpu` Surface renderer.
Build the generated browser package with `bash apps/web-demo/build-wasm.sh` once
`wasm32-unknown-unknown` and `wasm-bindgen` are installed locally. Use
`http://127.0.0.1:4173/apps/web-demo/?dataset=flowers` for a repeatable browser
flower smoke.

## Common Commands

```bash
cargo check --workspace
cargo test --workspace
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

Use `handbook/VERIFICATION.md` for the full validation matrix.
