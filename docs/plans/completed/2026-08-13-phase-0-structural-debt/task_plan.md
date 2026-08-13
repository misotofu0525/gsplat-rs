# Task Plan: Phase 0 Structural Debt Paydown

## Goal

Pay down the Phase 0 structural debt from `handbook/ROADMAP.md` item 1
without changing render behavior: split the render crate, delete dead
experiment vestiges, demote CPU geometry expansion to tests, and isolate
the Adaptive/async control plane from the default CPU frame path.

## Current Phase

Complete

## Phases

### Phase 1: Requirements & Discovery
- [x] Read ROADMAP Phase 0 / item 1, ARCHITECTURE, VERIFICATION, GOLDEN_PRINCIPLES
- [x] Map lib.rs, FFI no-ops, SortFree, GpuOddEven, Adaptive/async owners
- **Status:** complete

### Phase 2: Delete dead vestiges
- [x] Remove `GpuOddEvenSortBackend` and `odd_even_sort.wgsl`; drop unused sort-crate GPU deps
- [x] Remove `RenderMode::SortFree`
- [x] Remove documented C ABI no-ops (`set_gpu_preproject*`, `set_async_geometry`, `set_instance_buffer_count`)
- **Status:** complete

### Phase 3: Split render crate + isolate experiments
- [x] Extract `math`, `preprocess`, `resident`, `offscreen`, `surface` helpers
- [x] Move Adaptive policy and async sorter into dedicated modules
- [x] Demote `GpuInstance` / `build_sorted_instances*` to `#[cfg(test)]`
- **Status:** complete

### Phase 4: Docs
- [x] Update ARCHITECTURE, PROJECT_CONTEXT, crate READMEs, CHANGELOG, ROADMAP status
- **Status:** complete

### Phase 5: Verification
- [x] `cargo fmt`, `cargo check --workspace`, `cargo test --workspace`, clippy
- [x] rustdoc `-D warnings`, FFI smoke, GPU-required SortedAlpha conformance
- **Status:** complete

## Key Questions

1. Can experimental Surface no-ops be removed from `gsplat.h`? Yes — header
   already marks Surface A/B setters as experimental; JNI/Swift do not call
   the no-ops; `set_async_sort` stays.
2. Keep `RenderMode` with a single variant? Yes — C ABI still carries a mode
   field; `from_u32(1)` becomes `None`.
3. Build empty preprocess/order/draw stage traits now? No — that would be a
   placeholder. Module boundaries are the Phase 0 attachment points.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Keep `Renderer` in `lib.rs` | Public orchestration stays at the crate root; private fields remain visible to child modules |
| Re-export `pub(crate)` GPU types from `lib.rs` | Avoid churn in `resident_gpu_order.rs` / presenter imports |
| `#[cfg(test)]` for CPU geometry | Matches ROADMAP "test-only conformance oracle" |
| Keep Android hidden `set_order_backend` symbol | It is a live benchmark knob, not a no-op |
| Keep `SortError::{BackendUnavailable, BackendFailure}` | Public enum leftover from GPU sort; unused after GpuOddEven deletion, deferred to avoid extra API churn in this pass |
| Do not invent empty stage traits | ROADMAP item 1 remaining rule is attachment-at-stages, not placeholder traits |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| One-shot split script dropped `#[derive]` / `#[cfg(test)]` / `#[repr(C)]` | 1 | Restored attributes on error enums, resident GPU structs, `CameraCovarianceTerms`, and tests |
| `resident_gpu_order` could not see private resident fields | 1 | Made `ResidentGpuSceneOrder` fields and `ensure_gpu_order` `pub(crate)` |
| `GpuSurfaceSourceElem` accidentally copied into `cpu_geometry` | 1 | Removed; tests import `crate::resident::GpuSurfaceSourceElem` |
| `ShColorLayout` missing `Clone + Copy` after the split | 1 | Restored derives |
| `is_visible` / `world_to_camera_with_view_rot` privacy after the split | 1 | `pub(crate)`; camera helper stays `#[cfg(test)]` |
| rustc `private_interfaces` on instance builders | 1 | `build_sorted_instances*` are `#[cfg(test)] pub(crate)` |

## Outcome

Phase 0 landed 2026-08-13. Next ROADMAP item is quantized resident storage
plus per-splat compute preprocessing.
