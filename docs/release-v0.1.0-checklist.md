# v0.1.0 Release Checklist

Date: 2026-02-12

## Contract

- Quality contract: `SortedAlpha` only.
- Experimental backends are out-of-contract and must be marked as such in release notes.

## Required Gates

1. Build and test:
   - `cargo check --workspace`
   - `cargo test --workspace`
2. Conformance:
   - Renderer conformance baseline test passes.
3. Performance smoke:
   - `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120`
   - `cargo run -p gsplat-pack -- tests/datasets/minimal_ascii.ply target/minimal.gspk --verify`
4. Long stability:
   - `bash tests/perf/run-long-stability.sh` with `STABILITY_SECONDS=1800`
   - RSS growth must stay within configured limit.
5. FFI/mobile adapter checks:
   - `bash tests/ffi/run-ffi-smoke.sh`
   - `bash apps/ios-demo/run-swift-smoke.sh`
   - `bash apps/android-demo/run-jni-smoke.sh`
   - `bash apps/ios-demo/build-ios-sim.sh`
   - `bash apps/android-demo/build-apk.sh`
6. Docs:
   - README/API/ADR/release notes updated.

## Notes for v0.1.x

- WGSL shader path is present for instanced quad SortedAlpha baseline.
- CPU fallback paths remain available for environments without GPU adapter access.
