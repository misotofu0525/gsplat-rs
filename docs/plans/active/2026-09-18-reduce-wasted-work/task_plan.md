# Task Plan: Reduce Wasted Work

## Goal

Cut repeated computation and allocation on the default `SortedAlpha` + CPU
radix path without changing the ordering algorithm, the image contract, or
the v0.1 C ABI. Five items, each with before/after evidence from
`handbook/VERIFICATION.md` commands.

## Scope

- SH early-out: culled projected records skip SH evaluation.
- Stationary projection cache: the compute preprocess runs only when the
  camera, surface size, order upload, or scene changed.
- On-demand rendering: `SurfaceRenderSession` skips encode/submit/present
  when nothing changed and exposes `needs_frame()` so Rust clients can idle.
- Async sort workspace reuse: the native async CPU sort worker keeps its
  depth-key buffer, radix scratch, and recycles the displayed index buffer.
- CPU copy convergence: drop retained derived per-splat arrays
  (`world_covariances`, `world_covariance_terms`, `alpha_values`), share the
  scene with the async worker instead of copying positions, and write GPU
  source records straight into the mapped upload buffer.

## Out of scope

- MobileQualityGovernor, GPU ordering as default, sort-free / stochastic
  blending, training, C ABI additions.
- Kotlin / Swift wrapper edits (cannot be compiled here).

## Base

Branch `cursor/reduce-wasted-work-ac10` from PR #41 head
`b45eb1334aa61bdfc7ed193c86cb5744928f3896` (quaternion sign fix included).

## Evidence environment

Linux x86_64, Mesa Lavapipe (`llvmpipe (LLVM 20.1.2, 256 bits)`, Vulkan),
Xvfb for the interactive viewer, Kitsune CC0 scene (279,199 splats, SH
degree 3) fetched with `tests/datasets/fetch-wakufactory-kitune.sh`.

## Current Phase

All five items landed on `cursor/reduce-wasted-work-ac10` with host evidence
(Lavapipe + Xvfb). Device evidence (Android / iPhone / Metal) is not part of
this bundle. Draft PR open; not merged.

## Phases

### Phase 1: SH early-out
- [x] `project_record` returns a zero-color record for culled splats
- [x] PNG hashes identical before/after (image-exact)
- [x] bench-runner Kitsune GPU-complete before/after

### Phase 2: Stationary projection cache
- [x] `ResidentSceneResources` tracks projected params + order generation
- [x] Skip `encode_project` when the projected buffer is current
- [x] Offscreen device test: cached vs forced re-projection, identical image

### Phase 3: On-demand rendering
- [x] `SurfaceRenderSession::needs_frame`, `request_present`, `presented` output
- [x] Presenter reports whether a frame was actually presented
- [x] Desktop viewer waits for events when idle
- [x] Xvfb CPU-time before/after (stationary vs orbit)

### Phase 4: Async sort workspace reuse
- [x] Worker-owned depth keys + `CpuSortBackend`; index buffer recycled
- [x] Test: steady-state reuse, no per-sort allocation growth

### Phase 5: CPU copy convergence
- [x] Remove retained derived arrays; compute at upload into mapped buffer
- [x] `Arc<SceneBuffers>` shared with the async worker
- [x] bench-runner stability RSS before/after

### Phase 6: Docs and verification
- [x] Handbook updates (ARCHITECTURE, PROJECT_CONTEXT, ROADMAP, VERIFICATION)
- [x] Day-to-day verification set relevant to the change

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Offscreen `Renderer::render_frame` keeps full per-call work | It is the PNG/conformance/benchmark oracle; cadence belongs to the session |
| Skip present entirely on stationary Surface frames | Swapchains retain the last image; ROADMAP already forbids unconditional preprocess per frame |
| No new C symbols | Mobile wrappers keep calling `render_frame`; the call is now a cheap no-op when idle |
| Early-out only on culled/zero-alpha records | Image-exact; data-dependent SH band skipping would change semantics |
| Every `set_camera` requests a frame | Hosts express intent through camera commands on the C ABI / wasm; static benchmark traces (`orbit(0, 0)` per tick) keep measuring presented frames |
| Kotlin / Swift untouched | Cannot compile here; wrappers already skip camera commands while idle, so the session-level skip reaches them unchanged |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| `libxkbcommon-x11.so` missing for winit under Xvfb | first viewer run | `apt-get install libxkbcommon-x11-0` |
