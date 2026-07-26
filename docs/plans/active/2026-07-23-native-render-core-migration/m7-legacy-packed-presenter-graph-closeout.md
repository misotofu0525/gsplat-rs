# M7 legacy Packed presenter-graph deletion closeout

## Scope and status

- Parent: `6befc3b52624f82f0baf1c582aab99431eada1d0`.
- Scope: one isolated candidate deleting the unreachable standalone Packed
  `SurfacePresenter` graph after standalone Packed construction had already
  become a pre-allocation structured rejection.
- Status: candidate-local implementation and verification are complete. This
  note does not accept M7, activate M8, or replace root independent review.
- No Android collector/Kotlin, C header/implementation, JNI, Swift, Web SDK,
  shader, or public configuration file changed.

## Ownership result

Before this candidate, `SurfaceRenderSession::from_*` already selected
`SessionSurfaceOwner::ExactPacked(SurfacePresenterHost)` for product Packed,
but `SurfacePresenter` still compiled a second Packed semantic graph. That
graph retained `SurfacePackedRuntime`, resident global/projected pipelines,
`ProjectedQuadsGpu`, `PreprojectedGpuOrder`, post-sort/preproject selectors,
projected cache and compaction state, and two eight-slot readback rings.

After this candidate:

- `SurfacePresenter` owns only standalone Direct and diagnostic Paged geometry,
  their pipelines/bindings, Direct GPU-order telemetry, CPU completion
  telemetry, and its `SurfacePresenterHost`.
- `SurfacePresenterHost` owns only adapter/device/queue, Surface configuration
  and lifecycle, present/capture, and capability facts.
- Product Packed scene, complete Exact PlanSet, CPU/GPU order, projection,
  canonical raster, generations, policy, current-stats receipts, and
  publication remain owned by `Renderer::PreparedRuntimeSlot`.
- The obsolete `ProjectedQuadsGpu`, legacy preproject raster adapter,
  `StableContributorCompactor`, and standalone projected/producer WGPU
  readback-ring implementations are deleted. Their public measurement,
  failure, producer, and execution types remain re-exported at the same crate
  paths.
- `ResidentGpuResources` no longer creates a global draw bind group for Exact
  product construction. The retained native offscreen compatibility route
  creates that bind group only when it actually enters offscreen Packed.
- Standalone Direct no longer requests eight storage bindings as headroom for
  the deleted Packed graph. Product Packed preflight and its eight-binding
  admission remain unchanged.

## Preserved behavior and structured errors

- All public `SurfacePresenter::from_*` families still admit Direct/Paged and
  reject Packed with `StandalonePackedPresenterUnsupported` before host or GPU
  allocation. Repeated rejection remains `ErrorCode::Unsupported`.
- `SurfaceRenderSession::from_*` still constructs product Packed with
  `SurfacePresenterHost` and publishes the renderer-owned Exact candidate.
- Public native `SurfaceRenderSession::new(renderer, presenter, camera)` is
  unchanged: it consumes the already-built presenter and neither converts nor
  discards it.
- Direct/Paged native switching remains transactional. Entering/leaving Packed
  remains rejected before mutation; browser geometry remains constructor-only.
- Direct keeps GlobalQuads and PostSort GPU ordering. Paged remains diagnostic
  GlobalQuads with its fixed resource/path policy. Neither path auto-selects
  Paged.
- Exact Packed retains Candidate/Compact and CPU PostSort/GPU PostSort/GPU
  Preproject through renderer plans. Current-stats and compatibility receipt
  types/queues remain renderer/session owned.
- SortedAlpha pass order, SH0-SH3 decode/color, visibility/membership,
  resolution, stable full32 order, and source-id ties are unchanged. No shader
  changed.

## API and ABI classification

- Public Rust API: **Compatible**. No public item, signature, enum variant, or
  re-export was removed. Only crate-private legacy graph types and methods were
  deleted. Existing public `SurfacePresenter` Direct/Paged methods and
  `SurfaceRenderSession` controls retain their paths and structured errors.
- C ABI/header: **Unchanged**. No file under `crates/gsplat-ffi-c/` changed;
  the smoke test compiles the real C header/client against the candidate.
- JNI/Kotlin/Swift: **Unchanged source surface**. No binding or platform wrapper
  changed. Device/UI qualification remains outside this isolated task.
- WASM/JS: **Compatible build**. No exported Web API changed; standalone Packed
  remains the already-established rejection and product Packed remains the
  host-owned Exact construction route.
- `SurfacePresenterError` compatibility variants were retained even when this
  candidate removed their former crate-private allocation sites.

## Focused proof

- `standalone_presenter_source_contains_no_packed_graph_resources` guards the
  presenter, crate module graph, preproject adapter, and compatibility telemetry
  modules against reintroducing Packed graph/resource constructors.
- `exact_packed_session_construction_skips_standalone_presenter_resources`
  proves the constructor split; Direct and Paged still select the standalone
  presenter.
- `small_direct_scene_does_not_request_obsolete_packed_headroom` and
  `direct_device_without_resident_bindings_still_constructs_direct_limits`
  prove Direct no longer inherits Packed binding policy.
- The hidden-window `surface_geometry_entry` test proves public Direct/Paged,
  repeatable pre-allocation Packed rejection, public `Session::new` state
  preservation, and one successfully presented product Packed frame on Metal.

## Verification

| Command | Result | Evidence boundary |
| --- | --- | --- |
| `cargo fmt --check` | Pass | Formatting. |
| `cargo check --workspace` | Pass | Host workspace compile. |
| `cargo test --workspace` | Pass | Workspace tests; render crate 420 passed, 8 explicitly ignored benchmark/external-asset cases. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass | All-target lint. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | Pass | Public docs compile. |
| `cargo test -p desktop-example` | Pass: 17 | Desktop argument/route tests. |
| `cargo test -p desktop-example --features interactive-viewer --test surface_geometry_entry` | Pass: `M7D_SURFACE_GEOMETRY_ENTRY=PASS backend=Metal adapter="Apple M4" public_presenter=true public_session=true standalone_packed_rejected=true product_packed_host=true` | Real hidden macOS window/Metal Surface proof. |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha` | Pass: 1 | Forced Metal SortedAlpha conformance. |
| `bash tests/ffi/run-ffi-smoke.sh` | Pass: `ffi smoke ok`, drawn=2, visible=2 | Real C header/client smoke. |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | Pass, zero warnings | Browser target compile; not browser runtime proof. |
| `PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py` | Pass: 3 | Checker self-tests. |
| `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py` | Pass: 120 production Rust, 17 WGSL, 5 grandfathered | Existing two grandfather-growth notices only. No policy exception was added or widened. |
| `bash tests/security/run-cargo-deny.sh` | Pass | Advisories, bans, licenses, sources; configured duplicate-version warnings remain non-failing. |

## Exact changed paths

- `crates/gsplat-render-wgpu/src/surface_presenter.rs`
- `crates/gsplat-render-wgpu/src/surface_session.rs`
- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-render-wgpu/src/gpu_producer_telemetry.rs`
- `crates/gsplat-render-wgpu/src/projected_draw_telemetry.rs`
- `crates/gsplat-render-wgpu/src/gpu_telemetry.rs`
- `crates/gsplat-render-wgpu/src/projected_quads_gpu.rs` (deleted)
- `crates/gsplat-render-wgpu/src/preproject_gpu.rs`
- `crates/gsplat-render-wgpu/src/preproject_gpu/raster.rs` (deleted)
- `crates/gsplat-render-wgpu/src/gpu/compact.rs` (deleted)
- `crates/gsplat-render-wgpu/src/gpu/mod.rs`
- `crates/gsplat-render-wgpu/src/gpu/project.rs`
- `crates/gsplat-render-wgpu/src/gpu/scan.rs`
- `crates/gsplat-render-wgpu/src/resident_gpu.rs`
- `crates/gsplat-render-wgpu/src/renderer/gpu_prepare.rs`
- `crates/gsplat-render-wgpu/src/raster/canonical/tests.rs`
- `crates/gsplat-render-wgpu/src/scene/mod.rs`
- `docs/plans/active/2026-07-23-native-render-core-migration/m7-legacy-packed-presenter-graph-closeout.md`

## Residual M7 boundaries

- Root independent review and integration are still required; this candidate
  does not update root-owned `progress.md` or the public ledger's acceptance
  state.
- Android device/window terminal-ledger qualification was not run and no
  Android collector/Kotlin source changed.
- iOS device/simulator, browser runtime/Playwright, Windows, Linux, and physical
  Android presentation were not run. Their build/API surfaces remain covered
  only by the repository matrix applicable on this macOS worktree.
- External Kitsune/Flowers/Garden/Truck asset benchmarks and explicitly ignored
  finite release observations were not run; they are not substituted by the
  focused hidden-window smoke.
- The existing architecture policy still grandfathers the large
  `surface_presenter.rs` and `surface_session.rs` files because Direct/Paged and
  session policy remain there. This candidate neither removes nor broadens
  those exceptions; factual policy reduction can be handled only where a
  checker exception is demonstrably obsolete.
