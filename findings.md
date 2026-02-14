# Findings & Decisions

## Requirements
- User asked to use `planning-with-files` skill and execute work based on `docs/v0.1.0-multi-subagent-plan.md`.
- The plan requires multi-subagent execution style, gate-driven progression, and directory ownership boundaries.
- Mainline quality commitment is `SortedAlpha`; research lanes are non-blocking.
- User explicitly asked to finish remaining items, not just baseline placeholders.

## Research Findings
- Workspace is currently a scaffold with minimal placeholders in all key crates.
- Current files show stubs for core types, PLY loader, renderer, sort backend, FFI symbols, and tools.
- CI currently runs only `cargo check --workspace`; perf workflow runs a placeholder bench command.
- No planning files existed before this session; they were initialized and replaced with gate-specific content.
- Repository currently has no initial commit (`HEAD` missing), so branch/commit-based traceability is unavailable in this session.
- Required-field ASCII PLY parsing can satisfy G1 without binary support as long as unsupported formats map to explicit error codes.
- A deterministic CPU fallback sort path is sufficient to unblock G2 conformance while GPU radix remains a placeholder backend.
- C ABI can be frozen now as lifecycle + scene load + render + stats, even before native iOS/Android runtime wiring.
- Swift bridge imports opaque C context handles as `OpaquePointer` (not concrete struct type), which matters for smoke code signatures.
- Host-level JNI smoke can be run without Gradle/NDK by compiling JNI C with `clang` and Java class with `javac`, sufficient for baseline ABI adapter verification.
- No `.wgsl` files exist yet in `crates/gsplat-render-wgpu/shaders`, so SA-05 is still placeholder-level for shader deliverables.
- `gsplat-format` and `gsplat-pack` remain mostly unimplemented and are still explicit closure gaps.
- WGSL render path can be integrated without surface creation by rendering into an offscreen texture target.
- `wgpu 28` API requires `experimental_features` in `DeviceDescriptor`, `immediate_size` in `PipelineLayoutDescriptor`, and `PollType::wait_indefinitely()`.
- Android container build is feasible in this environment by combining Rust cross-compilation (`aarch64-linux-android`), NDK clang JNI linking, and local Gradle distribution bootstrap.
- The current WGSL path uses isotropic quad size from `max(exp(scale_*))`; it does not project full 3D covariance into screen-space ellipse axes.
- `hyperlogic/splatapult` projects 3D covariance with Jacobian-based affine approximation (`V' = J W V (J W)^T`), then derives ellipse extents from 2D covariance eigen decomposition.
- `splatapult` fragment evaluation uses Gaussian form `exp(-0.5 * d^T * cov2_inv * d)` and alpha cutoff, which is directly portable to our SortedAlpha blend contract.

## Technical Decisions
| Decision | Rationale |
|----------|-----------|
| Treat each SA as a parallel implementation track mapped by directory ownership | Aligns directly with user-provided plan and reduces cross-track conflicts |
| Start from G0 API freeze work before deeper implementation | Downstream crates depend on shared contracts |
| Add explicit error enums/codes and deterministic stats surfaces early | Needed for gate criteria and later QA automation |
| Keep `SortedAlpha` as default mode in core config and FFI config | Reinforces v0.1 quality contract in all entry points |
| Put conformance baseline in renderer crate integration tests with shared dataset file | Keeps tests close to render behavior while reusing global test assets |
| Add public C header in-repo instead of waiting for generator tooling | Unblocks FFI consumers and cross-language smoke tests immediately |
| Add Swift/JNI smoke scripts as first-class gate checks | Moves G3 from interface-only to executable evidence |
| Track “remaining work closure” as a new execution batch in planning files | Avoids mixing baseline-complete state with unfinished scope |
| Implement GPU sort backend as compute odd-even pass for v0.1 closure | Provides real GPU execution path with manageable complexity and deterministic fallback behavior |
| Use offscreen WGSL render pass for baseline correctness path | Unblocks shader delivery before window/surface integration |
| Build Android APK via project-local Gradle distribution | Avoids global tooling dependency conflicts on host machine |
| Prioritize “full 3DGS geometry” before interactive viewer loop | User explicitly raised this as higher-priority unfinished work |
| Port covariance-projection math from `splatapult` into current wgpu path | Delivers geometry correctness while preserving existing crate boundaries |
| Keep implementation in current instanced draw model (no geometry shader) | Aligns with WebGPU/wgpu constraints and existing renderer architecture |

## Issues Encountered
| Issue | Resolution |
|-------|------------|
| `git rev-parse --abbrev-ref HEAD` fails without any commit | Use `git status` + file scans for source of truth |
| Need to prove perf smoke command is executable, not just configured in workflow | Added `bench-runner` real path and ran it locally with dataset |
| Parallel write race when creating header file during multi-tool execution | Switched header creation to sequential command |
| Initial Swift smoke failed due type/import mismatch | Probed generated Swift signature and aligned to `OpaquePointer` |
| `brew install gradle` failed due tap state conflict | Avoided global install and switched to project-local Gradle download flow |
| `wgpu 28` API mismatch errors in new GPU code | Adapted descriptor fields and polling API to current version |
| Bench-runner positional argument parsing regression after stability mode extension | Added explicit dataset/iteration parse state and revalidated both modes |
| Local policy rejected `rm -rf` while cloning external reference repo | Switched to timestamped temporary clone path without destructive cleanup |

## Resources
- `/Users/misotofu/Documents/workspace/gsplat-rs/docs/v0.1.0-multi-subagent-plan.md`
- `/Users/misotofu/Documents/workspace/gsplat-rs/PLAN.md`
- `/Users/misotofu/Documents/workspace/gsplat-rs/.github/workflows/ci.yml`
- `/Users/misotofu/Documents/workspace/gsplat-rs/.github/workflows/perf-smoke.yml`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-core/src/lib.rs`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-io-ply/src/lib.rs`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/src/lib.rs`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-sort/src/lib.rs`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-ffi-c/src/lib.rs`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/tests/conformance_sorted_alpha.rs`
- `/Users/misotofu/Documents/workspace/gsplat-rs/tests/datasets/minimal_ascii.ply`
- `/Users/misotofu/Documents/workspace/gsplat-rs/docs/adr/0001-v0.1-sortedalpha-only.md`
- `/Users/misotofu/Documents/workspace/gsplat-rs/docs/v0.1.0-subagent-execution.md`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-ffi-c/include/gsplat.h`
- `/Users/misotofu/Documents/workspace/gsplat-rs/tests/ffi/ffi_smoke.c`
- `/Users/misotofu/Documents/workspace/gsplat-rs/tests/ffi/run-ffi-smoke.sh`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/smoke/main.swift`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/run-swift-smoke.sh`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/jni/gsplat_jni.c`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/src/com/gsplat/demo/GsplatJniSmoke.java`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/run-jni-smoke.sh`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/shaders/splat.wgsl`
- `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-sort/shaders/odd_even_sort.wgsl`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/build-android-native.sh`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/build-apk.sh`
- `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/build-ios-sim.sh`
- `/Users/misotofu/Documents/workspace/gsplat-rs/docs/release-v0.1.0-checklist.md`
- `https://github.com/hyperlogic/splatapult`
- `/tmp/splatapult_ref_1771062563/shader/splat_vert.glsl`
- `/tmp/splatapult_ref_1771062563/shader/splat_geom.glsl`
- `/tmp/splatapult_ref_1771062563/shader/splat_frag.glsl`
