# M7 R2 Renderer scene-state owner slice

Status: candidate implementation complete; M7 remains **Active**.

Base: `3ac7acd0f14a8d9a06fc2e6538e86601d712c08f`.

## Scope

This slice moves the renderer's concrete scene source, path-derived CPU data,
and reusable sorted-index scratch from `lib.rs` into the private
`renderer/scene_state.rs` owner.

The owner represents source residency as exactly one of `Empty`, `Wide`, or
`Resident`, and represents path-derived data as exactly one of `None`,
`Direct`, or `Paged`. These variants make the former wide/resident and
Direct/Paged cache exclusions structural rather than a convention across
several independent `Option` fields.

`Renderer` remains the facade and transaction coordinator. It still owns:

- public construction, scene-loading, geometry-path and rendering entrypoints;
- mode/configuration and the CPU order engine;
- the sole `PreparedRuntimeSlot`, complete `PlanSet`, semantic generations,
  mandatory sampler and terminal result semantics;
- offscreen GPU ownership and dispatch;
- final `FrameStats` publication.

Scene candidates are validated or encoded before the facade replaces the
private source state. Exact GPU candidates are still fully prepared before
publication. Successful Exact publication clears only scene-state source,
derived data and order contents while retaining the reusable order allocation.

## Preserved contracts

- A scene state cannot contain both wide and resident sources, or both Direct
  caches and Paged metadata.
- Direct retains the same precomputed world covariance, camera covariance
  terms and alpha arrays. Paged retains the same default spatial-page
  partition. Packed retains the same exact resident upload source until the
  existing handoff or Exact publication boundary.
- Order scratch contents and capacity follow the prior load/path behavior.
  Exact publication still invalidates old order contents.
- Invalid wide/resident replacement and injected Exact preparation failures
  leave the old scene, derived caches, runtime generations, image, order and
  public statistics untouched.
- Surface resource construction now reads narrow renderer accessors instead of
  reaching into concrete scene fields. The Presenter/Session edits are wiring
  and test adaptation only; no Surface lifecycle, scheduling, policy,
  presentation or publication ownership moves.
- CPU reference/order algorithms, offscreen target/readback, WGSL, GPU layouts,
  pass order, `SortedAlpha`, public Rust API and all C/Web/mobile ABI remain
  unchanged.

## Focused evidence

- `cargo test -p gsplat-render-wgpu renderer::scene_state::tests --lib --locked`:
  PASS, 3/3. This covers source exclusivity, Direct/Paged derived-state
  replacement, and Exact order invalidation with allocation reuse.
- `cargo test -p gsplat-render-wgpu resident_scene_load_validates_before_replacing_renderer_state --lib --locked`:
  PASS, 1/1.
- `cargo test -p gsplat-render-wgpu invalid_wide_replacement_preserves_scene_caches_order_and_public_stats --lib --locked`:
  PASS, 1/1.
- `cargo test -p gsplat-render-wgpu failed_presenter_prepare_rolls_renderer_back_to_working_path --lib --locked`:
  PASS, 1/1.
- `cargo test -p gsplat-render-wgpu failed_geometry_resource_prepare_does_not_commit_partial_state --lib --locked`:
  PASS, 1/1.
- `cargo test -p gsplat-render-wgpu packed_exact_failed_replacement_stages_preserve_scene_image_generations_and_stats --lib --locked`:
  PASS, 1/1.
- `cargo test -p gsplat-render-wgpu --lib --locked`: PASS, 433 passed and 8
  existing research/device tests ignored.
- `cargo test --workspace --locked`: PASS. This includes 433 renderer tests,
  8 existing renderer ignores, and the one existing SortedAlpha conformance
  test on the available Apple M4 Metal adapter.
- `cargo check --workspace --locked`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`: PASS.
- `RUSTFLAGS="-D warnings" cargo check -p gsplat-render-wgpu --target wasm32-unknown-unknown --locked`:
  PASS; this is compile evidence, not browser/WebGPU runtime evidence.
- `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/check_source_architecture.py`:
  PASS with the existing non-blocking `surface_session.rs` grandfather-growth
  notice.
- `cargo fmt --all -- --check` and `git diff --check`: PASS.

The first post-test verification attempt reached host `ENOSPC`. Only this
worktree's 1.5 GiB of rebuildable Cargo `target/` output was removed with
`cargo clean`. With no Cargo/Rustc process active, later verification reused
the root worktree's existing Cargo target cache; no source, dataset, device or
browser artifact was removed.

No device, browser, package installation or external dataset run is part of
R2. This slice does not edit `progress.md` or `m7-final-acceptance.md`, remove a
grandfather record, or accept M7.
