# Progress Log

## Session: 2026-07-10

### Phase 1: Baseline and Contracts

- **Status:** complete
- **Started:** 2026-07-10
- Actions taken:
  - Reused the completed open-source audit as the initial issue inventory.
  - Refreshed the current branch/worktree state.
  - Read the `planning-with-files` skill and created this task-scoped bundle.
  - Established a six-phase release-readiness plan centered on crash safety and truthful verification.
  - Enumerated every current PLY call site and the FFI entrypoints affected by parser panics.
  - Measured local validation datasets to keep the upcoming default resource limits compatible with existing large scenes.
  - Recorded the regression tests required for parser, FFI, renderer, and pixel-output semantics.
  - Selected compatible default PLY limits from the actual local dataset corpus.

### Phase 2: Input and FFI Safety

- **Status:** complete
- **Started:** 2026-07-10
- Actions taken:
  - Defined the public `PlyLoadLimits` contract and structured resource/allocation error strategy.
  - Added default-limit wrappers and explicit limit-aware file, byte, text, and summary APIs.
  - Enforced 1 GiB input/decoded-scene, 1 MiB header, 5M vertex, and 128-property defaults.
  - Replaced infallible scene reservations and unchecked size arithmetic with fallible reservations and checked calculations.
  - Moved ASCII/binary body plausibility checks ahead of scene allocation.
  - Kept summary loading header-only while preserving required-field validation.
  - Added regression coverage for oversized input/header/vertex/property/scene budgets, truncated binary bodies, and size overflow.
  - Wrapped all 31 exported C ABI functions in catch-unwind boundaries with return-type-appropriate fallbacks.
  - Added a regression proving a panic becomes `ErrorCode::Internal` and a stable thread-local detail message.
- Files created/modified:
  - `crates/gsplat-io-ply/src/lib.rs`
  - `crates/gsplat-ffi-c/src/lib.rs`
  - `docs/plans/active/2026-07-10-release-readiness-hardening/task_plan.md`
  - `docs/plans/active/2026-07-10-release-readiness-hardening/findings.md`
  - `docs/plans/active/2026-07-10-release-readiness-hardening/progress.md`

### Phase 3: Renderer Outcome Semantics

- **Status:** complete
- Actions taken:
  - Made native offscreen constructors fail explicitly when adapter/device creation fails.
  - Added Surface-only constructors and migrated Android, iOS, Web, and CPU-only tests to avoid redundant offscreen devices.
  - Requested the exact additional texture dimension needed for supported 4K targets and rejected unsupported sizes before resource creation.
  - Validated instance-buffer byte requirements against device limits before buffer creation.
  - Made `render_frame`, `set_size`, and `render_placeholder` propagate rasterizer failures instead of dropping the GPU and returning success.
  - Added tests for Surface/offscreen separation, adapter limits, 4K no-unwind behavior, and instance-buffer limits.
- Files created/modified:
  - `crates/gsplat-render-wgpu/src/lib.rs`
  - `crates/gsplat-render-wgpu/README.md`
  - `crates/gsplat-ffi-c/src/lib.rs`
  - `crates/gsplat-web/src/wasm.rs`
  - `examples/desktop/src/main.rs`

### Phase 4: Executable Conformance and Performance Semantics

- **Status:** complete
- Actions taken:
  - Extended SortedAlpha conformance to read back a 64x64 RGBA target and require non-transparent pixels.
  - Added a cross-backend tolerance baseline over normalized per-channel image means.
  - Added a public GPU completion wait and adapter metadata access for native offscreen renderers.
  - Split benchmark reporting into CPU preprocess, CPU sort, build/encode/submit, GPU wait, and GPU-complete latency.
  - Added warmup and optional GPU-complete threshold flags; long stability now waits for GPU work each frame.
  - Changed canonical perf commands and the manual perf workflow to release mode with a 250 ms portable smoke threshold.
- Files created/modified:
  - `crates/gsplat-render-wgpu/tests/conformance_sorted_alpha.rs`
  - `tools/bench-runner/src/main.rs`
  - `tests/perf/run-long-stability.sh`
  - `.github/workflows/perf-smoke.yml`
  - `handbook/VERIFICATION.md`

### Phase 5: Release, Security, and Adoption

- **Status:** complete
- Actions taken:
  - Added `deny.toml`; updated `crossbeam-epoch` to 0.9.20 and wayland-scanner/quick-xml to the latest compatible stable releases.
  - Documented scoped, upstream-blocked RustSec exceptions and verified advisories/licenses/bans/sources pass.
  - Pinned every workflow action to an immutable commit and set read-only default permissions, with write access only on the final release job.
  - Added dependency-policy jobs to CI and release, and made the tag workflow require the 1800-second stability bar.
  - Added tag/package version consistency checking, prerelease artifact checksums, and Web package checks.
  - Added installation/artifact guidance, `RELEASING.md`, dependency security policy, and current changelog entries.
  - Removed the unused Flowers image with unclear dataset redistribution terms; documented source, CC0 status, and hashes for Kitsune media.
  - Archived the completed Android GPU performance plan and recorded its remaining thermal study as a future gap.
  - Documented private vulnerability reporting and protected-main settings as explicit manual gates; no remote settings were changed.
- Files created/modified:
  - `deny.toml`
  - `Cargo.lock`
  - `.github/workflows/*.yml`
  - `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `RELEASING.md`
  - `docs/media/README.md`, `docs/media/flowers.jpg` (removed)
  - `tests/release/check-version.sh`
  - `docs/plans/completed/2026-06-10-android-gpu-render-perf/`

### Phase 6: Full Verification and Delivery

- **Status:** in_progress
- Actions taken:
  - Ran the complete local automated Rust hygiene/test/doc matrix after the dependency and renderer changes.
  - Built and dry-packed the Web WASM and ESM distributions.
  - Ran C ABI, JNI, and Swift smokes against the hardened ABI.
  - Built the Android AAR and sample APK, iOS simulator binary, three-architecture XCFramework, and GsplatKit simulator package.
  - Produced and validated the desktop PNG output and release benchmark.
  - Ran a 60-second GPU-complete stability preflight with the formal 64 MiB RSS-growth threshold.
  - Rechecked release version consistency, workflow YAML, action SHA pinning, dependency policy, formatting, Clippy, and diff whitespace.
  - Used the XcodeBuildMCP verification workflow to inspect the available Apple project context; the repository has no checked-in Xcode project/workspace for discovery, so the canonical SwiftPM and repo-local Apple scripts remain the executable evidence.
  - Used the current-docs-sync workflow to align `AGENTS.md`, project context, architecture, and verification docs with the new renderer construction, bounded PLY, dependency-policy, and release contracts.
  - Completed the full 1800-second GPU-complete stability bar on Apple M4 Pro/Metal: 770,542 frames at 428.08 FPS with 4,432 KiB peak RSS growth against a 65,536 KiB limit.
  - Added explicit Rust 1.93 setup to dependency-policy jobs and Mesa Vulkan/Lavapipe to Linux jobs so GPU-required FFI/offscreen tests have a deterministic software adapter.
  - Extended release consistency checks to SemVer syntax, internal crate dependency versions, C header macros, and the Web example API version.
  - Prevented zero-iteration and zero-duration benchmark gates from succeeding without work.
  - Expanded checksum-pinned cargo-deny bootstrap support to Linux x86_64/arm64 and macOS arm64/x86_64.
  - Excluded tag pushes from duplicate CI runs, enabled stale-run cancellation, and allowed 75 minutes for cold builds plus the 30-minute release stability gate.
  - Reran the final source matrix after all code/doc/workflow edits; Rust, Web, WASM, FFI, dependency, version, benchmark, short-stability, YAML, action-pin, and diff checks passed.
  - Committed the scoped release-readiness work as `ef4415b` and pushed `agent/kitsune-showcase` to origin.
  - Updated PR #13 to `Prepare gsplat-rs v0.1.0 prerelease`, replaced its body with the complete safety/release scope and current verification evidence, and marked it ready for review.
  - Used the first hosted CI run as feedback: dependency policy, Mesa Vulkan setup, and Metal pixel conformance passed, but checkout v4 emitted a Node 20 deprecation annotation.
  - Refreshed all JavaScript GitHub Actions to their current Node 24 releases while retaining immutable full-SHA pins.
- Remaining external evidence:
  - Fresh GitHub CI/tag workflow execution requires committing and pushing this work.
  - The 1800-second stability bar is enforced in the tag workflow; the local preflight covered 60 seconds.
  - Private vulnerability reporting and protected-main settings remain unchanged pending explicit authorization.

## Test Results

| Test | Expected | Actual | Status |
|------|----------|--------|--------|
| Initial `git status --short --branch` | Clean branch before implementation | `agent/kitsune-showcase` clean and synced with origin | pass |
| `cargo test -p gsplat-io-ply` | Existing formats plus new malicious-input regressions pass | 21 passed, 0 failed | pass |
| `cargo test -p gsplat-io-ply -p gsplat-ffi-c` | Parser and FFI safety regressions pass | PLY 22 passed; FFI 14 passed; 0 failed | pass |
| `cargo fmt --all -- --check` | No formatting drift | clean | pass |
| `cargo clippy -p gsplat-io-ply -p gsplat-ffi-c --all-targets -- -D warnings` | Focused code is warning-free | passed | pass |
| `cargo test -p gsplat-render-wgpu` | Renderer semantics and pixel conformance pass | 14 unit + 1 conformance passed | pass |
| `cargo check --workspace` | All native workspace crates compile with migrated constructors | passed | pass |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | Web Surface constructor path compiles without warnings | passed | pass |
| `cargo clippy -p gsplat-render-wgpu -p gsplat-ffi-c -p gsplat-web --all-targets -- -D warnings` | Changed renderer consumers are warning-free | passed | pass |
| `cargo test -p bench-runner` | New warmup/threshold configuration parses correctly | 8 passed, 0 failed | pass |
| Release-mode benchmark with 120 frames | GPU completion is measured and portable threshold passes | final source: submit 0.4667 ms; GPU wait 1.7372 ms; GPU-complete 2.2042 ms; threshold 250 ms | pass |
| `cargo deny check --hide-inclusion-graph` | Advisories, licenses, bans, and sources satisfy documented policy | all four checks passed; duplicate versions remain warnings | pass |
| `RELEASE_VERSION=0.1.0 bash tests/release/check-version.sh` | Cargo/Web/Android/API versions agree | passed | pass |
| Workflow YAML parse + action-ref scan | YAML is valid and no mutable action tags remain | four workflows parsed; all actions SHA-pinned | pass |
| Full Rust hygiene/test/doc matrix | fmt, warnings-as-errors Clippy/rustdoc, and workspace tests pass | 86 Rust tests passed; no lint/doc warnings | pass |
| Web build/package matrix | JS, WASM target, release WASM/ESM build, tests, and dry pack pass | 6 JS tests; 8-file 0.1.0 package generated | pass |
| FFI/JNI/Swift smokes | Hardened ABI remains usable from C, Kotlin/JNI, and Swift | all three passed; C/Swift drew 2 visible splats | pass |
| Android packaging | Release AAR and debug sample APK build | both artifacts produced | pass |
| Apple packaging | Simulator binary, XCFramework, Swift package description, and Xcode simulator build | all passed; Xcode `BUILD SUCCEEDED` | pass |
| Desktop PNG smoke | Native offscreen renderer produces a valid RGBA image | 1280x720 PNG; visible/drawn 2 | pass |
| 60-second stability preflight | RSS growth remains below the formal 64 MiB limit while waiting for GPU completion | 25,795 frames; 429.91 FPS; 4,256 KiB growth | pass |
| 1800-second release stability bar | Full documented duration waits for every frame and stays below 64 MiB RSS growth | 770,542 frames; 428.08 FPS; 4,432 KiB growth | pass |
| cargo-deny 0.20.2 final policy check | Current cached RustSec DB, licenses, bans, and sources pass | all checks passed; duplicate-version warnings retained | pass |
| Canonical documentation sync | Agent routing, context, architecture, and verification describe current paths | links and paths exist; `git diff --check` clean | pass |
| XcodeBuildMCP project discovery | Detect a checked-in project/workspace if one exists | no project/workspace found; repo uses SwiftPM and generated/scripted Xcode build paths | pass |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-07-10 | `cargo fmt --all -- --check` reported formatting drift in the new PLY code | 1 | Ran `cargo fmt --all`; subsequent tests compiled the formatted source. |
| 2026-07-10 | New test used nonexistent `PlyLoadError::error_code()` | 1 | Switched to the existing `PlyLoadError::code()` method; 21 tests passed. |
| 2026-07-10 | Clippy reported an identity multiplication in scene-byte accounting | 1 | Removed `* 1`; focused Clippy passed with warnings denied. |
| 2026-07-10 | Initial wasm check found unreachable native render code and a dead native-only helper | 1 | Split `render_frame` by target and cfg-gated the helper; wasm check passed without warnings. |
| 2026-07-10 | Pillow was unavailable for local PNG statistics | 1 | Used installed ImageMagick instead; captured stable channel means. |
| 2026-07-10 | Source install of cargo-deny stalled on registry update | 1 | Used the checksum-verified official 0.19.4 binary in `target/`. |
| 2026-07-10 | Initial cargo-deny fetches appeared idle | 2 | Ran `cargo fetch` explicitly, then cargo-deny completed. |
| 2026-07-10 | quick-xml 0.41 is outside latest stable wayland-scanner's semver range | 1 | Updated to latest compatible 0.39.4 and documented build-time-only upstream-blocked advisories. |
| 2026-07-10 | Initial cargo-deny bootstrap verification used zsh's read-only `status` variable | 1 | Replaced the wrapper variable; no repository code was affected. |
| 2026-07-10 | GitHub release/advisory downloads briefly failed with TLS errors | 2 | Added download retries and used the refreshed local advisory DB for the final check. |
| 2026-07-10 | GPU instance precheck was initially placed in a CPU-only builder | 1 | Focused test failed immediately; moved it to offscreen `render_frame`; renderer tests and workspace check passed. |
| 2026-07-10 | Final shell link check used zsh's special `path` array as a loop variable, which cleared command lookup | 1 | Renamed the loop variable to `file`; repository files were unaffected. |
| 2026-07-10 | Tried the removed cargo-deny `--disable-fetch` flag through the wrapper | 1 | Reran the canonical policy script without the unsupported flag; all checks passed. |
| 2026-07-10 | Optional local Linux-container verification could not start because the Docker/Colima daemon was not running | 1 | Did not mutate local daemon state; added the explicit Mesa Vulkan runtime to Linux CI and left fresh hosted-runner proof for the pushed workflow. |
| 2026-07-10 | GitHub PR update returned 422 because `maintainer_can_modify` is only valid for cross-repository PRs | 1 | The title/body update was applied; omitted that field from subsequent same-repository operations and successfully marked PR #13 ready. |
| 2026-07-10 | Hosted CI annotated pinned checkout v4 because its Node 20 runtime is deprecated and forcibly upgraded | 1 | Verified current official releases and upgraded checkout/setup/artifact/release actions to Node 24 versions pinned by full commit SHA. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Phase 6: full verification and final scope review. |
| Where am I going? | Input/FFI safety, renderer semantics, executable conformance, release hardening, full verification. |
| What's the goal? | A safe, verifiable, adoptable prerelease with truthful Rust/C render contracts. |
| What have I learned? | See `findings.md`. |
| What have I done? | Completed code safety, renderer truthfulness, executable conformance/perf, supply-chain policy, and release/adoption documentation. |
