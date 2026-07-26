# M7 R4 Renderer semantic facade slice

Status: candidate implementation complete; M7 remains **Active**.

Base: `a5ae6ae41234c8f53ea937bdd50b12ed76db3fd4`.

## Scope

This slice closes the crate-root renderer implementation into the private
`renderer/facade.rs` module while retaining the crate-root `Renderer` type and
all of its inherent public and crate-private entrypoints.

The private module is an implementation location, not a second state owner.
`Renderer` continues to own:

- every public constructor, configuration, scene, geometry-path, preprocessing,
  ordering, rendering, readback and statistics API;
- scene and path replacement transactions through the sole
  `RendererSceneState`;
- the sole `PreparedRuntimeSlot`, complete `PlanSet`, semantic generations,
  mandatory sampler, active Exact policy and terminal publication;
- CPU reference and CPU-order selection, including the stable full32 ordering
  contract and the retained Direct/Paged compatibility dispatch;
- final `FrameStats` publication for offscreen and renderer-owned Surface
  terminals.

The R1 CPU reference, R2 scene state and R3 offscreen host remain the concrete
private owners already established by their slices. R4 adds no replacement
state container, controller, plan cache, generation ledger, sampler, target,
readback owner or host policy.

## Compatibility and transaction boundaries

- The crate-root path remains `gsplat_render_wgpu::Renderer`; the public type,
  method names, signatures, cfg gates and Rustdoc surface are unchanged.
- The moved inherent implementation is text-identical to the base after
  normalizing one Rustdoc link and making four already crate-visible
  internal/test helpers explicitly `pub(crate)` across the new module privacy
  boundary.
- Wide and Resident candidates are still validated before mutation. Exact GPU
  candidates are still fully prepared before `Renderer` publishes the slot and
  clears obsolete scene/host state.
- Failed scene replacement, Exact preparation, resize or frame execution still
  leaves the old scene, path, runtime generations, fallback/order, target image
  and public statistics published.
- No `SortedAlpha` rule, shader/WGSL, resource layout, render pass, submission
  order, pixel behavior, Surface/Session/Presenter implementation, C ABI/header
  or platform binding is changed.

## Focused evidence

- A base/current extraction audit reports the complete moved `impl Renderer`
  identical after the four internal visibility normalizations and absolute
  Rustdoc-link normalization.
- `cargo test -p gsplat-render-wgpu --lib --locked`: PASS, 438 passed and 8
  existing research/device tests ignored.
- `cargo test -p gsplat-render-wgpu offscreen --lib --locked`: PASS, 21/21.
- `cargo test -p gsplat-render-wgpu packed_exact_failed_frame_and_resize_preserve_published_state --lib --locked`:
  PASS, 1/1.
- `cargo test -p gsplat-render-wgpu packed_exact_failed_replacement_stages_preserve_scene_image_generations_and_stats --lib --locked`:
  PASS, 1/1.
- `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha --locked`:
  PASS, 1/1 on the available macOS Metal adapter.
- `bash tests/ffi/run-ffi-smoke.sh`: PASS, including the real C header/client
  render smoke (`drawn=2`, `visible=2`).
- `RUSTFLAGS="-D warnings" cargo check -p gsplat-render-wgpu --target wasm32-unknown-unknown --locked`:
  PASS.
- `RUSTFLAGS="-D warnings" cargo check -p gsplat-web --target wasm32-unknown-unknown --locked`:
  PASS; both WASM results are compile evidence, not browser/WebGPU runtime
  evidence.
- `cargo check --workspace --locked`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`: PASS.
- `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/test_source_architecture.py`:
  PASS, 3/3.
- `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/check_source_architecture.py`:
  PASS, 131 production Rust files, 17 WGSL files and the unchanged five
  grandfather records.
- `cargo fmt --all -- --check`, `cargo metadata --no-deps --format-version 1 --locked`
  and `git diff --check`: PASS.

The first renderer library-test compile exposed a crate-root test-only
`CameraCovarianceTerms` alias that had been removed with now-unused production
imports; restoring it behind `#[cfg(test)]` preserved the compatibility seam
without a non-test warning. The first strict WASM check exposed native-only
facade imports; compile-time cfg gating restored the base target contract.

No Android/iOS device, simulator, browser or external dataset was launched.

## Owned files and non-claims

The candidate allowlist is:

- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-render-wgpu/src/renderer/mod.rs`
- `crates/gsplat-render-wgpu/src/renderer/facade.rs`
- `docs/plans/active/2026-07-23-native-render-core-migration/m7-r4-renderer-facade.md`

This slice does not edit `progress.md` or `m7-final-acceptance.md`, does not
remove a grandfather record, does not accept M7 and does not activate or claim
M8.
