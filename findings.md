# Findings & Decisions

## Requirements
- User asked to use `planning-with-files` skill and execute work based on `docs/v0.1.0-multi-subagent-plan.md`.
- The plan requires multi-subagent execution style, gate-driven progression, and directory ownership boundaries.
- Mainline quality commitment is `SortedAlpha`; research lanes are non-blocking.
- User explicitly asked to finish remaining items, not just baseline placeholders.

## Research Findings
- 2026-02-16 SDK preprocess ownership refactor:
  - Interactive preprocess shader moved from `apps/desktop-dev/shaders/preprocess_instances.wgsl` to `crates/gsplat-render-wgpu/shaders/preprocess_instances.wgsl`.
  - Added crate-level `GpuInstancePreprocessor` + `Renderer::create_gpu_instance_preprocessor()` so app/demo no longer owns shader source, compute bind layout, or scene packing for preprocess.
  - `desktop-dev` `SurfacePresenter` now consumes SDK API for preprocess; app-layer code only keeps surface present/event loop concerns.
  - Validation: `cargo check -p gsplat-render-wgpu`, `cargo check -p desktop-dev`, `cargo check -p desktop-dev --features interactive-viewer`, and `cargo test -p gsplat-render-wgpu` all pass.
- 2026-02-15 realtime preview architecture update:
  - Interactive viewer no longer depends on `minifb` readback path; switched to `winit + wgpu surface` present path in app layer.
  - Interactive rendering now uses `Renderer::build_sorted_instances()` (preprocess/sort/build only) and submits `GpuInstance` directly to surface render pass.
  - Removed the mandatory per-frame `readback_rgba8()` requirement from interactive loop; this eliminates the previous blocking GPU->CPU sync in preview mode.
- 2026-02-15 GPU compute preprocess closure:
  - Interactive path now dispatches `apps/desktop-dev/shaders/preprocess_instances.wgsl` to build `GpuInstance` directly on GPU from sorted indices.
  - Initial runtime panic (`Too many bindings of type StorageBuffers in COMPUTE`) was caused by 6 compute storage bindings on downlevel limits.
  - Resolved by packing scene attributes (`position/covariance/color_dc/opacity`) into one `GpuSceneElem` storage buffer and reducing compute bindings to 4 total (`sorted_indices`, `scene`, `params`, `instances`).
  - `cargo check -p desktop-dev --features interactive-viewer`, `cargo test -p gsplat-render-wgpu`, and `cargo check --workspace` all pass after this fix.
- 2026-02-15 compute scene layout fix:
  - Artifact screenshot (center streak/cross pattern) matched a storage-layout mismatch between WGSL `SceneElem` and Rust `GpuSceneElem`.
  - WGSL `opacity + vec3` introduced 16-byte alignment holes while Rust struct was tightly packed, causing per-element stride mismatch and corrupted scene reads in compute.
  - Resolved by changing both sides to `opacity_and_pad: vec4` and keeping scene element stride explicitly homogeneous.
- 2026-02-15 CPU build-stage optimization:
  - Added world-covariance precompute on scene load (`precompute_world_covariances`) and reuse in per-frame build path.
  - Parallelized instance construction over sorted indices using Rayon (`par_iter + filter_map + collect`).
  - Bench evidence (`flowers_1`):
    - `desktop-dev --release --frames 30 --auto-camera --width 1280 --height 720` moved from roughly `frame_ms≈43/raster_ms≈38` to `frame_ms=13.58/raster_ms=8.33`.
    - `bench-runner --release ... 120` now reports `avg_frame_ms=11.53`, `avg_sort_ms=2.63`, `avg_raster_ms=4.60`.
- 2026-02-15 interactive perf triage (`flowers_1`, 1280x720, auto camera, 30 frames, release):
  - `frame_ms=43.08`, `preprocess_ms=0.77`, `sort_ms=4.51`, `raster_ms=37.80`, `visible_count=562974`, `drawn_count=562974`.
  - Lowering resolution to `640x360` keeps timings nearly identical (`frame_ms=42.99`, `raster_ms=37.73`), indicating the dominant cost is not fill-rate but CPU-side per-instance build/projection in raster stage.
  - Interactive viewer mode adds per-frame readback/present overhead on top of render:
    - `render_frame()` then `readback_rgba8()` then `rgba_to_xrgb()` then `window.update_with_buffer()`.
    - `readback_rgba8()` currently allocates a new MAP_READ buffer, performs texture copy, and blocks with `device.poll(wait_indefinitely)` every frame.
- Current 500k+ point workload therefore bottlenecks primarily in:
  1) CPU `build_instances` math path (`raster_ms` bucket), 2) interactive per-frame GPU->CPU readback/present path, not sort.
- The only explicitly tracked open gap in current docs is "interactive on-screen realtime viewer loop".
- `apps/desktop-dev` currently supports only offscreen frame sequencing and optional PNG dump.
- Current renderer already exposes per-frame render + readback APIs, so a window-present path can be implemented in app-layer without changing crate contracts.
- `apps/desktop-dev/Cargo.toml` currently has no windowing/event-loop dependency.
- Feature-gating the viewer dependency keeps baseline/headless workspace behavior unchanged while exposing a runnable realtime path when enabled.
- `flowers_1` visual streaking is caused by combined factors: quaternion semantic mismatch risk at PLY ingest and overly close auto-camera framing that amplifies projection anisotropy.
- Current PLY ingest mapped `rot_0..3` directly to internal `xyzw`; this is ambiguous for common 3DGS exports that store quaternions as `wxyz`.
- Auto-camera distance previously fit only x/y extents and ignored z depth in standoff calculation, placing the camera near the front surface for thick scenes.
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
| Implement viewer loop in `desktop-dev` with dedicated interactive mode flag and controls help text | Keeps current command behavior stable for scripts while adding the missing real-time path |
| Keep render backend unchanged and present via per-frame readback in viewer mode | Avoids broad API churn in `gsplat-render-wgpu` and closes gap quickly with minimal risk |
| Gate window dependency behind `desktop-dev` feature `interactive-viewer` | Prevents accidental regressions in non-interactive CI/default builds while still enabling full viewer loop |
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
| Normalize PLY quaternion semantics to `wxyz -> xyzw` at load time | Removes dataset-side ambiguity and keeps downstream math consistently `xyzw` |
| Make auto-camera depth-aware by adding z-extent to standoff distance | Reduces close-up projection streak artifacts for large-thickness point clouds |
| Pack interactive compute scene data into a single storage buffer (`GpuSceneElem`) | Keeps compute prepass within adapter storage-buffer limits and preserves portability |
| Move interactive preprocess compute ownership into `gsplat-render-wgpu` crate | Prevents demo-app-only shader coupling and makes SDK integration path self-contained |

## Issues Encountered
| Issue | Resolution |
|-------|------------|
| `git rev-parse --abbrev-ref HEAD` fails without any commit | Use `git status` + file scans for source of truth |
| Need to prove perf smoke command is executable, not just configured in workflow | Added `bench-runner` real path and ran it locally with dataset |
| Parallel write race when creating header file during multi-tool execution | Switched header creation to sequential command |
| Initial Swift smoke failed due type/import mismatch | Probed generated Swift signature and aligned to `OpaquePointer` |
| `brew install gradle` failed due tap state conflict | Avoided global install and switched to project-local Gradle download flow |
| `wgpu 28` API mismatch errors in new GPU code | Adapted descriptor fields and polling API to current version |
| Runtime panic: `Too many bindings of type StorageBuffers in COMPUTE` during interactive startup | Reduced compute storage bindings from 6 to 3 read + 1 write by packing scene inputs |
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
