# gsplat-rs Architecture

## Purpose

- This file is a concise architectural map for humans and agents.
- Keep it factual and update it when responsibilities or critical flows move.

## System Boundaries

- The repository owns scene loading, sort backends, `wgpu` rendering, a small C ABI, and validation demos/tools around those pieces.
- The repository does not own model training, a web product surface, or multiple polished render backends.
- Main external dependencies are `wgpu`, platform toolchains for Swift/JNI validation, and Android/iOS SDK tooling for mobile container builds.

## Runtime Topology

- Core data types live in `crates/gsplat-core`.
- Scene import starts in `crates/gsplat-io-ply`.
- Sorting lives in `crates/gsplat-sort`.
- Rendering and GPU-facing orchestration live in `crates/gsplat-render-wgpu`.
- Native embedding goes through `crates/gsplat-ffi-c`.
- Validation entrypoints are `apps/desktop-demo`, `tools/bench-runner`, `apps/android-demo`, and `apps/ios-demo`.

## Key Directories

- `crates/`: reusable library crates and the C ABI
- `apps/desktop-demo/`: desktop viewer and offscreen output harness
- `apps/android-demo/`: Android demo project, JNI bridge, and host smoke entrypoint
- `apps/ios-demo/`: Swift smoke source and iOS simulator build scripts
- `tools/`: CLI tools for performance validation
- `tests/`: shared dataset, FFI smoke harness, and long-stability script
- `handbook/`: current project docs, architecture map, verification guide, roadmap, and project principles
- `docs/plans/`: active and completed task planning bundles

## Critical Flows

- PLY render flow:
  starts at external `.ply` data or `tests/datasets/minimal_ascii.ply`
  passes through `crates/gsplat-io-ply/src/lib.rs`
  continues into `crates/gsplat-render-wgpu/src/lib.rs`
  is exercised by `apps/desktop-demo/src/main.rs`, `tools/bench-runner/src/main.rs`, and `crates/gsplat-ffi-c/src/lib.rs`

- Native integration flow:
  starts from C, Swift, or Java/JNI host entrypoints
  crosses `crates/gsplat-ffi-c/include/gsplat.h` and `crates/gsplat-ffi-c/src/lib.rs`
  ends in the shared renderer and stats path

## Invariants

- `SortedAlpha` is the only release-gated path and the default mode expected by validation flows.
- The public C header and the Rust FFI implementation must stay in sync.
- PLY input normalization is not optional: quaternion remapping and `RDF -> RUF` conversion happen at load time.
- Mobile demo directories are integration validators, not separate product surfaces.

## Hotspots

- `crates/gsplat-render-wgpu/src/lib.rs`: render behavior, GPU orchestration, and perf-sensitive logic
- `crates/gsplat-sort/src/lib.rs`: ordering correctness and performance
- `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`: integration boundary stability
- `tests/perf/run-long-stability.sh` and `tools/bench-runner/src/main.rs`: regression detection for perf and stability

## Useful Entry Points

- Read first for renderer changes: `crates/gsplat-render-wgpu/src/lib.rs`
- Read first for import changes: `crates/gsplat-io-ply/src/lib.rs`
- Read first for native integration changes: `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`
- Read first for verification flow: `VERIFICATION.md`
