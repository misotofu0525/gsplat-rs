# gsplat-rs

Cross-platform Gaussian Splatting rendering library built with Rust + `wgpu`.

## Start Here

- Project context: `handbook/PROJECT_CONTEXT.md`
- Architecture map: `handbook/ARCHITECTURE.md`
- Verification commands: `handbook/VERIFICATION.md`
- Current direction and release boundary: `handbook/ROADMAP.md`
- Agent entrypoint: `AGENTS.md`

## Repository Layout

- `crates/`: core library crates, render path, sort backends, format support, and C ABI
- `apps/desktop-demo`: desktop viewer and offscreen PNG harness
- `apps/android-demo`: Android integration demo and JNI smoke path
- `apps/ios-demo`: Swift smoke path and iOS simulator build scripts
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
- Swift/C ABI integration is demonstrated by `apps/ios-demo`.
- Not in the v0.1 contract: scene-from-memory loading, runtime render-mode switching, AAR packaging, and XCFramework packaging.

## Common Commands

```bash
cargo check --workspace
cargo test --workspace
cargo run -p desktop-demo -- tests/datasets/minimal_ascii.ply --png target/out.png
```

Use `handbook/VERIFICATION.md` for the full validation matrix.
