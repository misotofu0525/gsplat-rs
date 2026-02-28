# Progress Log

## Session: 2026-02-16 (SDK Preprocess Ownership Refactor)

### Phase P4: SDK Surface Preprocess Ownership Refactor
- **Status:** complete
- **Started:** 2026-02-16
- Actions taken:
  - Moved interactive compute preprocess shader from demo app into renderer crate shader directory.
  - Added `GpuInstancePreprocessor` and `Renderer::create_gpu_instance_preprocessor()` in `gsplat-render-wgpu` to own compute preprocess internals in SDK.
  - Replaced `desktop-dev` app-local preprocess pipeline/scene packing code with calls to crate API.
  - Removed now-unused `bytemuck` dependency from `desktop-dev` interactive feature.
- Evidence:
  - `cargo check -p gsplat-render-wgpu`: pass
  - `cargo check -p desktop-dev`: pass
  - `cargo check -p desktop-dev --features interactive-viewer`: pass
  - `cargo test -p gsplat-render-wgpu`: pass

## Session: 2026-02-15 (GPU Compute Preprocess Closure)

### Phase P3: GPU-side Instance Build in Interactive Path
- **Status:** complete
- **Started:** 2026-02-15
- Actions taken:
  - Added interactive compute prepass shader (`preprocess_instances.wgsl`) to build `GpuInstance` from sorted indices on GPU.
  - Replaced multi-buffer compute inputs with packed `GpuSceneElem` storage buffer in `SurfacePresenter`.
  - Reduced compute bind group to 4 bindings and aligned Rust/WGSL layouts.
  - Revalidated build/test targets and ran interactive startup probe for runtime panic checks.
- Evidence:
  - `cargo check -p desktop-dev --features interactive-viewer`: pass
  - `cargo test -p gsplat-render-wgpu`: pass
  - `cargo check --workspace`: pass
  - Startup probe command (6s): `cargo run -p desktop-dev --features interactive-viewer -- tests/datasets/external/nvidia_flowers_1/model.ply --interactive --auto-camera --width 1280 --height 720` no longer reports storage-binding-limit panic.
  - Follow-up fix: changed compute scene element layout from `opacity + vec3` to `opacity_and_pad: vec4` on both Rust/WGSL sides to remove storage stride mismatch artifacts.

## Session: 2026-02-15 (Realtime Preview Pipeline Upgrade)

### Phase P2: Interactive Surface Present + CPU Build Optimization
- **Status:** complete
- **Started:** 2026-02-15
- Actions taken:
  - Added `Renderer::build_sorted_instances()` to expose preprocess/sort/build outputs without forcing offscreen readback.
  - Exposed `GpuInstance` as public data shape for app-side direct GPU upload.
  - Replaced interactive viewer backend from `minifb` to `winit + wgpu surface` present path.
  - Removed per-frame interactive readback in main loop; now rendering goes straight to surface render target.
  - Added scene-load world covariance precompute cache and parallelized per-frame instance build with Rayon.
  - Verified crate tests and desktop build paths after refactor.
- Evidence:
  - `cargo test -p gsplat-render-wgpu`: pass
  - `cargo check -p desktop-dev --features interactive-viewer`: pass
  - `cargo run -p desktop-dev --release -- ... --frames 30 --auto-camera --width 1280 --height 720`:
    - `frame_ms=13.5828`, `preprocess_ms=0.8244`, `sort_ms=4.4245`, `raster_ms=8.3338`
  - `cargo run -p bench-runner --release -- ... 120`:
    - `avg_frame_ms=11.5261`, `avg_preprocess_ms=4.2964`, `avg_sort_ms=2.6315`, `avg_raster_ms=4.5981`

## Session: 2026-02-15 (Interactive FPS Bottleneck Triage)

### Phase P1: Realtime Preview Perf Diagnosis
- **Status:** complete
- **Started:** 2026-02-15
- Actions taken:
  - Pulled latest desktop/renderer/sort code paths and confirmed current sort/raster pipeline behavior.
  - Downloaded `flowers_1` dataset and reproduced release-mode metrics with `desktop-dev`.
  - Ran offscreen stage timing at `1280x720` and `640x360` to isolate bottleneck sensitivity.
  - Confirmed `sort_ms` is no longer dominant after CPU sorter optimization; `raster_ms` dominates.
  - Correlated interactive loop overhead to per-frame GPU readback + CPU format conversion + present path.
- Evidence:
  - `desktop-dev --release --auto-camera --frames 30 --width 1280 --height 720`:
    - `frame_ms=43.0832`, `preprocess_ms=0.7746`, `sort_ms=4.5063`, `raster_ms=37.8021`, `visible_count=562974`.
  - `desktop-dev --release --auto-camera --frames 30 --width 640 --height 360`:
    - `frame_ms=42.9943`, `preprocess_ms=0.7785`, `sort_ms=4.4905`, `raster_ms=37.7251`.
  - `bench-runner --release ... 120`:
    - `avg_frame_ms=32.0630`, `avg_preprocess_ms=4.2384`, `avg_sort_ms=2.6320`, `avg_raster_ms=25.1924`.

## Session: 2026-02-15 (Artifact Reduction Batch: Rotation + Camera)

### Phase R9: Artifact Reduction Closure (Rotation Semantics + Camera Standoff)
- **Status:** complete
- **Started:** 2026-02-15
- Actions taken:
  - Re-opened render artifact issue for `flowers_1` streaking/micro-spike visuals.
  - Applied multi-track execution: PLY rotation semantics fix, camera standoff fix, validation+render comparison.
  - Updated PLY loader to interpret `rot_0..3` as `wxyz` and remap to internal `xyzw`.
  - Updated parser tests and minimal dataset quaternion fixtures to reflect explicit `wxyz` input semantics.
  - Updated `auto_camera` distance to include scene depth extent (`half_z`) to avoid front-surface over-close framing.
  - Re-ran check/test targets and produced before/after render comparison images.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-io-ply/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tests/datasets/minimal_ascii.ply`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/desktop-dev/src/main.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/src/lib.rs`

## Session: 2026-02-14 (Interactive Viewer Loop Closure Batch)

### Phase R8: Interactive On-Screen Viewer Loop Closure
- **Status:** complete
- **Started:** 2026-02-14
- Actions taken:
  - Re-opened unfinished item: on-screen interactive realtime viewer loop.
  - Re-applied `planning-with-files` workflow for this closure batch.
  - Re-validated current `desktop-dev` mode and renderer app-layer integration points.
  - Split work into parallel tracks: event loop/windowing, camera controls, docs/verification.
  - Added feature-gated interactive desktop viewer path (`--interactive`) with realtime window loop and control bindings.
  - Preserved existing offscreen mode behavior and PNG output path.
  - Added compile-time fallback message when interactive feature is not enabled.
  - Ran formatter, workspace checks/tests, and interactive-feature compile check.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/desktop-dev/Cargo.toml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/desktop-dev/src/main.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/README.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/architecture.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/v0.1.0-subagent-execution.md`

## Session: 2026-02-14 (Full 3DGS Geometry Priority Batch)

### Phase R7: Full 3DGS Geometry Closure
- **Status:** complete
- **Started:** 2026-02-14
- Actions taken:
  - Re-opened unfinished item: simplified raster geometry vs covariance-driven splat geometry.
  - Applied `planning-with-files` workflow for this new high-priority batch.
  - Pulled `hyperlogic/splatapult` source for shader reference.
  - Located and reviewed `splat_vert/splat_geom/splat_frag` covariance projection and Gaussian evaluation paths.
  - Replaced size-based isotropic splat instances with covariance-projected anisotropic ellipse axes.
  - Updated WGSL to render oriented ellipse quads with 3-sigma Gaussian evaluation + alpha cutoff.
  - Added focused unit tests for covariance projection and anisotropic axis generation.
  - Ran renderer + workspace regression and verified offscreen PNG output path.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/shaders/splat.wgsl`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/v0.1.0-subagent-execution.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/architecture.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/README.md`

## Session: 2026-02-12

### Phase 1: Requirements and Discovery
- **Status:** complete
- **Started:** 2026-02-12 14:23 PST
- Actions taken:
  - Read and applied `planning-with-files` skill workflow.
  - Read `docs/v0.1.0-multi-subagent-plan.md` and `PLAN.md`.
  - Scanned full workspace tree and key source files.
  - Identified that all crates are scaffold-level placeholders.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md` (created, then updated)
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md` (created, then updated)
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md` (created, then updated)

### Phase 2: Gate Plan and Subagent Orchestration
- **Status:** complete
- **Started:** 2026-02-12 14:27 PST
- Actions taken:
  - Converted user plan into executable G0-G4 phase checklist.
  - Mapped SA tracks to concrete directories and expected deliverables.
  - Logged initial constraints and one environment error (`HEAD` missing).
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`

### Phase 3: Implementation (Parallel Subagent Tracks)
- **Status:** complete
- **Started:** 2026-02-12 14:31 PST
- Actions taken:
  - SA-01: Reworked `gsplat-core` types/config/error-code/stats/scene buffers.
  - SA-02: Implemented required-field ASCII PLY parser with explicit error mapping.
  - SA-03/05: Implemented preprocess -> sort -> raster placeholder pipeline with stage stats.
  - SA-04: Expanded sort backend abstraction and deterministic CPU fallback tests.
  - SA-06: Added frozen C ABI lifecycle/render/stats symbols and context handling.
  - SA-07: Added conformance integration test and benchmark runner on real dataset.
  - SA-08: Added ADR and updated release-facing docs and workflows.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-core/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-sort/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-io-ply/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/Cargo.toml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/tests/conformance_sorted_alpha.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-ffi-c/Cargo.toml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-ffi-c/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tools/bench-runner/Cargo.toml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tools/bench-runner/src/main.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tests/datasets/minimal_ascii.ply`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/adr/0001-v0.1-sortedalpha-only.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/v0.1.0-subagent-execution.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/README.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/api.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/architecture.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/.github/workflows/ci.yml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/.github/workflows/perf-smoke.yml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/README.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/README.md`

### Phase 4: Testing and Verification
- **Status:** complete
- **Started:** 2026-02-12 14:39 PST
- Actions taken:
  - Ran formatter and workspace checks/tests.
  - Verified benchmark smoke command with dataset and 120 iterations.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`

### Phase 5: Gate Summary and Delivery
- **Status:** complete
- **Started:** 2026-02-12 14:42 PST
- Actions taken:
  - Consolidated gate status and subagent execution summary.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`

### Continuation: G3 Mobile Baseline Closure
- **Status:** complete
- **Started:** 2026-02-12 14:48 PST
- Actions taken:
  - Added public C header for frozen ABI surface.
  - Added host C smoke test and runnable script.
  - Added Swift smoke app/script and validated runtime path.
  - Added Java/JNI smoke bridge/script and validated runtime path.
  - Upgraded CI: linux C/JNI smoke + macOS Swift smoke.
  - Updated docs and subagent execution status for G3 evidence.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-ffi-c/include/gsplat.h`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tests/ffi/ffi_smoke.c`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tests/ffi/run-ffi-smoke.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/smoke/main.swift`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/run-swift-smoke.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/jni/gsplat_jni.c`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/src/com/gsplat/demo/GsplatJniSmoke.java`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/run-jni-smoke.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/.github/workflows/ci.yml`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/README.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/api.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/docs/v0.1.0-subagent-execution.md`

### Continuation: Remaining Work Closure Batch
- **Status:** complete
- **Started:** 2026-02-12 15:05 PST
- Actions taken:
  - Rebased planning files to new R1-R6 phases for unfinished tasks.
  - Re-validated remaining gaps from user feedback.
  - Prepared parallel tracks for WGSL/render, GPU sort, format/pack, long-stability, and mobile container evidence.
  - Implemented WGSL shader path and offscreen render pipeline integration.
  - Implemented GPU compute sort backend and integrated CPU fallback behavior.
  - Implemented packed format primitives and `gsplat-pack` CLI.
  - Added stability runner mode and long-stability workflow/script.
  - Added iOS simulator build script and Android APK container build pipeline.
  - Updated docs/workflows and reran full verification suite.
- Files created/modified:
  - `/Users/misotofu/Documents/workspace/gsplat-rs/task_plan.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/findings.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/progress.md`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-render-wgpu/shaders/splat.wgsl`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-sort/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-sort/shaders/odd_even_sort.wgsl`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/crates/gsplat-format/src/lib.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tools/gsplat-pack/src/main.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tools/bench-runner/src/main.rs`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/tests/perf/run-long-stability.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/ios-demo/build-ios-sim.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/build-android-native.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/apps/android-demo/build-apk.sh`
  - `/Users/misotofu/Documents/workspace/gsplat-rs/.github/workflows/long-stability.yml`

## Test Results
| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| Workspace scan | `rg --files crates apps tests tools docs .github` | List all main files | Returned expected scaffold files | PASS |
| Plan read | `sed -n '1,260p' docs/v0.1.0-multi-subagent-plan.md` | Full gate plan text | Loaded successfully | PASS |
| Format | `cargo fmt --all` | Success | Success | PASS |
| Workspace check | `cargo check --workspace` | Success | Success | PASS |
| Workspace tests | `cargo test --workspace` | Success | Success | PASS |
| Perf smoke command | `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120` | Success with metrics output | Success with avg metrics output | PASS |
| Planning completion | `bash /Users/misotofu/.agents/skills/planning-with-files/scripts/check-complete.sh` | 5/5 complete | 5/5 complete | PASS |
| C ABI smoke | `bash tests/ffi/run-ffi-smoke.sh` | Success | Success (`ffi smoke ok`) | PASS |
| JNI smoke | `bash apps/android-demo/run-jni-smoke.sh` | Success | Success (`jni smoke ok`) | PASS |
| Swift smoke | `bash apps/ios-demo/run-swift-smoke.sh` | Success | Success (`swift smoke ok`) | PASS |
| Regression check | `cargo check --workspace && cargo test --workspace` | Success | Success | PASS |
| Pack tool verify | `cargo run -p gsplat-pack -- tests/datasets/minimal_ascii.ply target/minimal.gspk --verify` | Success | Success | PASS |
| Stability smoke | `STABILITY_SECONDS=5 RSS_GROWTH_LIMIT_KIB=262144 bash tests/perf/run-long-stability.sh` | Success | Success (`rss_growth_kib` under limit) | PASS |
| iOS sim build | `bash apps/ios-demo/build-ios-sim.sh` | Success | Success (`target/ios-sim-smoke`) | PASS |
| Android native build | `bash apps/android-demo/build-android-native.sh` | Success | Success (`libgsplat_jni.so`) | PASS |
| Android APK build | `bash apps/android-demo/build-apk.sh` | Success | Success (`app-debug.apk`) | PASS |
| Perf benchmark | `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120` | Success | Success (`mode=iterations`) | PASS |
| Covariance renderer tests | `cargo test -p gsplat-render-wgpu` | Success | Success (5 unit + 1 conformance passed) | PASS |
| Workspace regression (post-geometry) | `cargo test --workspace` | Success | Success | PASS |
| Offscreen PNG render (post-geometry) | `cargo run -p desktop-dev -- tests/datasets/minimal_ascii.ply --png target/out_covariance.png` | Success | Success (`target/out_covariance.png`) | PASS |
| Final check (post-geometry) | `cargo check --workspace` | Success | Success | PASS |
| Viewer path check (interactive feature) | `cargo check -p desktop-dev --features interactive-viewer` | Success | Success | PASS |
| Viewer CLI help | `cargo run -p desktop-dev --features interactive-viewer -- --help` | Success | Success (help includes `--interactive`) | PASS |
| Workspace regression (post-viewer) | `cargo test --workspace` | Success | Success | PASS |
| Rotation + camera targeted tests | `cargo test -p gsplat-io-ply -p gsplat-render-wgpu -p desktop-dev` | Success | Success | PASS |
| Workspace check (post-R9) | `cargo check --workspace` | Success | Success | PASS |
| Workspace regression (post-R9) | `cargo test --workspace` | Success | Success | PASS |
| Flower render compare (post-R9) | `cargo run -p desktop-dev -- tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply --auto-camera --yaw-deg 0 --png target/flowers_1_fixed_yaw0.png` | Success | Success (`target/flowers_1_fixed_yaw0.png`) | PASS |
| RDF->RUF loader fix tests | `cargo test -p gsplat-io-ply` | Success | Success (9 tests passed) | PASS |
| Workspace regression (post-RDF->RUF) | `cargo test --workspace` | Success | Success | PASS |
| Flower render (post-RDF->RUF) | `cargo run -p desktop-dev -- tests/datasets/external/nvidia_flowers_1/model.ply --auto-camera --yaw-deg 0 --png target/flowers_1_ruf_yaw0.png` | Success | Success (`target/flowers_1_ruf_yaw0.png`) | PASS |

## Error Log
| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-02-12 14:26 PST | `fatal: ambiguous argument 'HEAD'` from `git rev-parse --abbrev-ref HEAD` | 1 | Avoided branch-from-HEAD assumptions; used filesystem and `git status` |
| 2026-02-12 14:50 PST | Header write failed due missing directory during parallel execution | 1 | Re-ran sequentially after directory creation |
| 2026-02-12 14:52 PST | Swift smoke compile failed (`GsplatContext` type / interpolation parse) | 1 | Used `OpaquePointer?` and simplified formatted output |
| 2026-02-12 15:16 PST | `wgpu 28` compile errors (`experimental_features`, `immediate_size`, `PollType`) | 1 | Updated code for current API contracts |
| 2026-02-12 15:23 PST | Homebrew Gradle install blocked by tap conflict | 1 | Switched to project-local Gradle bootstrap in script |
| 2026-02-12 15:31 PST | `bench-runner` positional arg parser misread second positional as dataset | 1 | Added explicit dataset-overridden tracking in parser |
| 2026-02-14 | Policy blocked `rm -rf` for temp clone cleanup | 1 | Switched to timestamped temp clone path without destructive cleanup |

## 5-Question Reboot Check
| Question | Answer |
|----------|--------|
| Where am I? | Phase R9 complete |
| Where am I going? | Final delivery summary with artifact-reduction evidence |
| What's the goal? | Reduce streak artifacts by fixing rotation semantics and camera standoff |
| What have I learned? | PLY quaternion order + camera distance materially affect covariance-projected ellipse appearance |
| What have I done? | Implemented `wxyz` rotation remap, depth-aware auto-camera standoff, and verified with regression + render comparison |
