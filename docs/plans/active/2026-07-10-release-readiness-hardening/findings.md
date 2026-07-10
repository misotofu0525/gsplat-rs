# Findings & Decisions

## Requirements

- Begin implementing the optimization work identified by the open-source audit.
- Keep the full release-readiness objective active across turns rather than declaring a narrow partial fix complete.
- Prioritize safety, truthful render outcomes, executable verification, and release integrity over new features.
- Follow the repository's canonical verification paths and keep tracked changes reviewable.

## Research Findings

- The current branch is `agent/kitsune-showcase` and initially had a clean worktree.
- Local fmt, Clippy, rustdoc, 75 Rust tests, FFI smoke, Web tests, wasm check, and desktop PNG smoke passed during the audit.
- `parse_ply_bytes` allocates five scene vectors directly from untrusted `header.vertex_count`; SH capacity also uses unchecked multiplication before binary body-size validation.
- The C ABI has 31 exported functions and no common panic barrier.
- `RendererConfig::validate` only rejects zero dimensions, while the offscreen device requests downlevel limits and can panic when creating a 4096-wide texture.
- `Renderer::with_config` discards GPU creation errors with `.ok()`; `render_frame` drops a failed rasterizer and still returns `Ok(FrameStats)`.
- The existing SortedAlpha conformance and C smoke tests assert counts/return codes but not pixel output.
- Current benchmark timing ends after queue submission, uses the dev profile in canonical scripts, and has no regression threshold.
- Live GitHub inspection found private vulnerability reporting disabled, `main` unprotected, no rulesets, broad release permissions, no action SHA enforcement, and a large Dependabot backlog.
- OpenSSF Scorecard reported 4.8/10 and current lockfile advisories need reachability-aware triage.
- Public adoption is source-only: there are no tags, GitHub Releases, or published Rust/mobile/Web packages.
- Locally available real PLYs range from 65.9 MB (Kitsune) and 139.6 MB
  (Flowers) to an 856.6 MB candidate scene, so default limits must reject
  pathological headers without accidentally excluding the existing large-scene
  validation corpus.
- The 856.6 MB candidate declares 3,454,040 degree-3 vertices; its raw
  `SceneBuffers` payload is roughly 778 MiB, so 1 GiB input and scene limits plus
  a 5,000,000-vertex limit preserve the current corpus while bounding forged
  headers.
- All production PLY consumers route through the same `load_ply` or
  `parse_ply_bytes` functions, so compatible default wrappers plus explicit
  limit-aware variants can harden Rust, FFI, desktop, bench, and Web together.
- `wgpu` 28 documents that a device can only use the limits requested at
  device creation; the previous downlevel request fixed 2D textures at 2048
  even when the adapter supported 4K or larger targets.
- Surface renderers already create a separate device through
  `SurfacePresenter`, so constructing an offscreen device first was redundant
  and made GPU initialization semantics ambiguous on Android, iOS, and Web.
- The old benchmark stopped at `queue.submit`, so its `frame_ms` measured CPU
  preparation/encoding rather than completion of GPU work. A blocking device
  poll per measured frame is needed for honest end-to-end latency.
- The release-mode 120-frame minimal-scene baseline on Apple M4 Pro/Metal was
  0.4387 ms to submit, 1.5688 ms waiting for GPU completion, and 2.0077 ms
  GPU-complete end to end; the portable CI smoke threshold remains a generous
  250 ms rather than treating one machine as a universal performance budget.
- Current RustSec data found one directly actionable vulnerability:
  `crossbeam-epoch` 0.9.18 (`RUSTSEC-2026-0204`), fixed by updating the lockfile
  to 0.9.20.
- The remaining notices are upstream-blocked transitive dependencies: `paste`
  through wgpu/Metal, `ttf-parser` through the optional winit viewer, and
  `quick-xml` through wayland-scanner. The quick-xml code is a build dependency
  parsing dependency-owned Wayland protocol XML, not project/user input.
- The old NVIDIA Flowers render had no independently verifiable dataset
  redistribution grant and was no longer referenced, while the current
  Wakufactory Kitsune scene is explicitly CC0 and checksum-pinned.
- GPU-required offscreen and FFI tests need an explicit headless adapter on
  Linux runners after removing the old false-success fallback; Mesa
  Vulkan/Lavapipe provides that compatibility path, while macOS/Metal remains
  the hardware-backed conformance and performance gate.
- A cold release job compiles both debug conformance and release stability
  binaries before the 1800-second run, so a 40-minute timeout cannot reliably
  contain the documented gate; the job now allows 75 minutes.
- Fresh hosted CI reported that the previously pinned checkout v4 runtime still
  targeted deprecated Node 20 and was being forced onto Node 24. The current
  official checkout v7, setup-node v6, setup-java v5, upload-artifact v7,
  download-artifact v8, and action-gh-release v3 releases all declare Node 24;
  their exact release commits are now pinned.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Introduce explicit load limits rather than infer safety from input length alone | ASCII input can lie about vertex count; callers need predictable memory budgets. |
| Use checked arithmetic and fallible reservation | Resource exhaustion must become a structured error, not a panic. |
| Add an FFI panic barrier even after removing known panic paths | The C boundary must not rely on every dependency remaining panic-free. |
| Make GPU-required construction fail when no GPU path exists | Returning successful render stats without pixels violates the API name and consumer expectation. |
| Prove rendering via readback | Counts only prove preprocessing/sorting, not raster output. |
| Validate body plausibility before allocating | Tiny bodies with forged vertex counts must fail before any large reservation. |
| Default to 1 GiB input/scene, 5M vertices, 128 properties, and a 1 MiB header | These values admit every local validation scene while creating finite, testable resource contracts. |
| Preserve the existing simple loader APIs as default-limit wrappers | Existing consumers gain protection without a migration; constrained callers can opt into tighter budgets. |
| Keep summary loading header-only but validate required vertex fields | Avoid full `SceneBuffers` allocation without turning malformed schemas into valid summaries. |
| Put every exported C ABI function behind one of three catch helpers | Error-code, void, and value-return entrypoints need different fallbacks, but all must prevent Rust unwind from crossing C. |
| Use a stable panic detail instead of exposing panic payloads | The ABI reports the failed operation without leaking internal payload text or relying on payload formatting. |
| Make `Renderer::new` GPU-required and add explicit Surface constructors | Callers can now distinguish actual offscreen pixel rendering from CPU preprocessing/surface state. |
| Request only the needed 2D texture dimension from the adapter | 4K succeeds when supported, while unsupported dimensions return a structured error before texture creation. |
| Reject oversized instance buffers before calling `wgpu::Device::create_buffer` | Device storage/buffer limits become normal renderer errors instead of validation panics. |
| Use normalized per-channel means with a 0.035 tolerance for the 64x64 golden | The aggregate detects blank or materially wrong output while allowing small backend floating-point differences. |
| Keep submit and GPU-complete latency as separate benchmark fields | Queue submission is useful CPU-side data but must not be presented as completed rendering. |
| Use release mode, warmups, adapter metadata, and an optional threshold | Results become reproducible enough for smoke regression detection without hard-coding local hardware performance. |
| Fail dependency policy on new advisories but keep reasoned upstream-blocked exceptions visible | Security debt stays reviewable without making CI permanently red for unreachable or unfixable transitive notices. |
| Pin every GitHub Action by full commit SHA and scope release write permission to one job | Workflow supply-chain exposure and token blast radius are both reduced. |
| Make private vulnerability reporting and branch protection manual tag gates | They are required remote settings, but repository code must not silently mutate administrator policy. |
| Remove the unused Flowers image and document the CC0 Kitsune derivative | Only media with a clear redistribution basis should ship in the repository. |
| Use software Vulkan for Linux compatibility and Metal for required GPU evidence | Headless Linux FFI tests remain executable without misrepresenting a software adapter as performance proof. |
| Reject zero-work benchmark configurations | A performance gate must not pass by dividing empty measurements into NaN. |
| Upgrade pinned Actions when hosted CI proves their embedded runtime is deprecated | Immutable SHAs prevent drift, but the pinned release still needs to match the supported runner runtime. |

## Required Regression Tests

- A tiny ASCII PLY claiming an enormous vertex count fails before scene allocation.
- A tiny binary PLY claiming an enormous vertex count fails before scene allocation.
- Vertex/property/scene-byte limits return structured `ResourceLimit` errors.
- Checked SH capacity overflow returns a normal error rather than panicking.
- Existing minimal ASCII, little-endian, big-endian, SH, Kitsune, and Flowers paths remain compatible.
- A panic inside an FFI-wrapped operation is converted to `GSPLAT_ERROR_INTERNAL`
  and updates the thread-local detail message.
- A 4096-wide offscreen request returns a structured result based on adapter
  limits and never triggers a wgpu validation panic.
- If offscreen GPU initialization fails, renderer construction fails explicitly.
- A successful conformance test reads back non-transparent pixels.

## Issues Encountered

| Issue | Resolution |
|-------|------------|
| The prior audit used historical memory for repository administration context | All drift-prone GitHub state was refreshed live before planning implementation. |
| A new regression test initially called `error_code()` instead of the existing `code()` method | Corrected the test to the crate's actual API; the full PLY test suite then passed. |
| Focused Clippy rejected `size_of::<[f32; 4]>() * 1` | Removed the identity operation; the focused warnings-as-errors run passed. |
| The first local image-statistics command assumed Pillow was installed | Switched to the available ImageMagick tooling and recorded the 64x64 RGBA baseline. |
| `cargo install cargo-deny` stalled while updating the registry | Switched to the current official 0.20.2 release asset, pinned and verified its published SHA-256, and added the reusable bootstrap script under `tests/security/`. |
| Initial cargo-deny dependency fetches stalled silently | Ran `cargo fetch` directly to expose and complete the missing target-specific downloads, then reran the audit successfully. |
| `quick-xml` cannot reach the fixed 0.41 release through latest stable wayland-scanner | Documented build-time reachability and an upstream-blocked exception instead of forcing an incompatible dependency override. |
| A pre-allocation GPU capacity check was first inserted into the CPU-only instance builder | The focused renderer test caught the contract regression; moved the check into offscreen `render_frame` and reran renderer/workspace verification. |
| GitHub briefly returned TLS errors while refreshing release/advisory assets | The bootstrap script now retries release downloads; after the advisory database refresh completed, the canonical dependency-policy check passed. |

## Resources

- `AGENTS.md`
- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`
- `handbook/ROADMAP.md`
- `handbook/GOLDEN_PRINCIPLES.md`
- `crates/gsplat-io-ply/src/lib.rs`
- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-ffi-c/src/lib.rs`
- `crates/gsplat-render-wgpu/tests/conformance_sorted_alpha.rs`
