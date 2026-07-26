# M7 R3 Renderer offscreen-host owner slice

Status: candidate implementation complete; M7 remains **Active**.

Base: `77761aed8a6b79c3fc986b20a813c0221a5048f7`.

## Scope

This slice moves the native offscreen GPU host from `lib.rs` into the private
`renderer/offscreen_host.rs` owner. The host now contains:

- adapter/device/queue acquisition and effective offscreen limits;
- the transactionally replaced `OffscreenTarget` and readback initiation;
- Direct, resident compatibility and diagnostic Paged path-local GPU caches;
- the unchanged Direct, resident-color-plus-draw and Paged encoder/submission
  orchestration.

`Renderer` remains the public facade and transaction owner. It still owns the
public constructors and methods, selected `GeometryPath`, `RendererSceneState`,
CPU order engine, sole `PreparedRuntimeSlot`, complete `PlanSet`, semantic
generations, mandatory sampler, Exact encode/submission identity and final
`FrameStats` publication. The offscreen host only supplies the Exact target
view; it does not acquire a second Exact scene, controller, plan cache,
generation ledger, sampler or result owner.

## Transaction and compatibility boundaries

- The complete host is constructed before `Renderer::with_config` installs it.
- Target resize still creates and validates a replacement before swapping the
  live target. A failed candidate therefore preserves the prior target, image,
  readback source, renderer configuration, Exact generations and public stats.
- Direct, resident and Paged scene resources are staged before their cache slot
  is published. Exact scene preparation remains in `PreparedRuntimeSlot`; only
  its successful renderer publication clears obsolete compatibility caches.
- Readback remains a non-semantic host operation. Its temporary copy buffer is
  not retained as renderer policy state and failure cannot publish frame state.
- Existing public Rust signatures and compatibility behavior remain unchanged,
  including `has_gpu_rasterizer`, device/queue accessors and error mapping.
- The three existing pass labels, clear colors, color-resolve-before-draw order,
  single submissions and `Rgba8Unorm` target contract are unchanged. No WGSL,
  raster math, Surface/Session code, C ABI/header or platform binding changes
  are part of this slice.

## Focused evidence

- `cargo test -p gsplat-render-wgpu renderer::offscreen_host::tests --lib --locked`:
  PASS, 1/1. The owner contains target/readback, all three compatibility draws,
  encoder/submission mechanics, and the old facade no longer contains them.
- `cargo test -p gsplat-render-wgpu offscreen --lib --locked`: PASS, 21/21.
  This includes Direct/Paged/Packed Exact offscreen paths, SH0--SH3 byte/image
  parity, readback, stale transaction and compatibility-owner checks.
- `cargo test -p gsplat-render-wgpu packed_exact_failed_frame_and_resize_preserve_published_state --lib --locked`:
  PASS, 1/1. Failed validation-scope resize preserved the live target image,
  Exact frame/order generations, configuration and stats; successful resize
  retained repeatable readback.
- `cargo test -p gsplat-render-wgpu packed_exact_failed_replacement_stages_preserve_scene_image_generations_and_stats --lib --locked`:
  PASS, 1/1 across staged GPU preparation failures.
- `cargo test -p gsplat-render-wgpu renderer::scene_state::tests --lib --locked`:
  PASS, 3/3 for the R2 dependency boundary.
- `cargo test -p gsplat-render-wgpu cpu_order::tests --lib --locked`: PASS,
  10 passed and 2 existing finite benchmark/calibration tests ignored.
- `cargo test -p gsplat-render-wgpu --lib --locked`: PASS, 438 passed and 8
  existing research/device tests ignored.
- `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha --locked`:
  PASS, 1/1 on the available macOS Metal adapter.
- `cargo test --workspace --locked`: PASS after rebuilding the worktree cache.
  The concise rerun reports renderer 438/8 ignored, the SortedAlpha integration
  1/1 and every other workspace suite passing.
- `cargo check --workspace --locked`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`: PASS.
- `RUSTFLAGS="-D warnings" cargo check -p gsplat-render-wgpu --target wasm32-unknown-unknown --locked`:
  PASS; this is compile evidence, not browser/WebGPU runtime evidence.
- `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/check_source_architecture.py`:
  PASS, 130 production Rust files, 17 WGSL files and the unchanged five
  grandfather records.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

The first workspace-test attempt failed while archiving `zerocopy` and `js-sys`
with host `ENOSPC`; it produced no source diagnostic. The filesystem had only
117 MiB free, this worktree's rebuildable `target/` occupied 1.9 GiB, and no
Cargo/Rustc process was active. Only this worktree's Cargo output was removed
with `cargo clean` (2.1 GiB reported), after which the unchanged source passed
the full workspace test. No source, dataset, shared evidence, device or browser
artifact was removed.

## Owned files and non-claims

The candidate allowlist is:

- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-render-wgpu/src/renderer/mod.rs`
- `crates/gsplat-render-wgpu/src/renderer/offscreen_host.rs`
- `crates/gsplat-render-wgpu/src/offscreen/shadow.rs`
- `docs/plans/active/2026-07-23-native-render-core-migration/m7-r3-renderer-offscreen-host.md`

No device or browser was launched. This slice is not R4 facade migration, does
not edit `progress.md` or `m7-final-acceptance.md`, removes no grandfather
record and does not accept M7.
