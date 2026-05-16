# Test Coverage Review

## Goal

- Audit whether the current test surface is enough for the v0.1 release boundary.
- Add focused repository-local tests for gaps that can be validated without device-specific setup.
- Keep device, browser, packaging, and long-stability evidence separate from unit and integration tests.

## Current Hypothesis

- Core Rust crates have useful unit coverage, but the coverage is uneven around boundary validation and command behavior.
- Platform paths rely mostly on smoke scripts, so the strongest immediate additions should protect FFI/header-adjacent behavior and script-free logic.
- The canonical release boundary remains PLY import -> `SceneBuffers` -> `SortedAlpha` renderer plus the small C ABI.

## Work Plan

1. Inventory current tests and verification commands.
2. Use subagents to inspect Rust crates, platform bindings, and CI/docs coverage in parallel.
3. Add focused tests in existing crates or scripts without widening the public API.
4. Run targeted verification, then the broader workspace test path as far as practical.
5. Record completion evidence and remaining gaps.

## Findings

- Baseline `cargo test --workspace` was green before edits, but coverage was uneven:
  `bench-runner`, `desktop-example`, and `gsplat-web` had zero Rust tests.
- The current project is not test-complete in the strong sense. It has a useful
  core/smoke matrix, while real Surface/device/browser/perf evidence remains
  environment-dependent and should not be collapsed into `cargo test`.
- Highest-value repository-local gaps found by the subagent audit:
  - PLY parser boundary tests for binary big-endian, truncated binary payloads,
    ASCII non-finite values, and vertex count mismatches.
  - `SceneBuffers` validation for SH rest shape, unsupported degree, and
    non-finite scene fields.
  - `bench-runner` CLI and spatial-analysis helper tests.
  - `desktop-example` CLI/auto-camera/PNG guard tests.
  - FFI null-handle/null-output error-code tests.
  - Node-level tests for the local Web wrapper.
- Added one behavior hardening change: ASCII PLY parsing now rejects `NaN` and
  infinities, matching the existing binary scalar path. `SceneBuffers::validate`
  now rejects non-finite positions, opacity, scale, rotation, color, and SH
  coefficients before renderer upload.
- Remaining non-local gaps:
  - Android `SurfaceView` and iOS `CAMetalLayer` realtime smoke still require
    device/simulator/browser execution paths from `handbook/VERIFICATION.md`.
  - Browser example behavior still needs the documented HTTP + browser benchmark
    smoke; Node tests only cover the package wrapper.
  - Larger real datasets and long stability remain manual/nightly/release-bar
    evidence rather than PR-fast tests.

## Verification

- `cargo fmt`
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
  - Rust tests increased from 39 to 71 and passed.
- `npm --prefix packages/web run check`
- `npm --prefix packages/web test`
  - Web wrapper added 6 Node tests and passed.
- `bash tests/ffi/run-ffi-smoke.sh`
  - Passed with `drawn=2 visible=2`.
- `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 3`
  - Passed with `avg_visible_count=2.00` and `avg_drawn_count=2.00`.
- `cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --frames 1`
  - Passed with `visible_count=2` and `drawn_count=2`.
- `node --check examples/web/src/main.js`
