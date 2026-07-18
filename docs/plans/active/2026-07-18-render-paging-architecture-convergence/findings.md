# Findings and Decisions: Render and Paging Architecture Convergence

## Requirements

- Restore the original dual-path boundary: `SmallSceneDirect` is the default;
  `PagedActiveAtlas` is only for scenes that exceed Direct capacity.
- Reduce architecture and code size without a big-bang rewrite.
- Preserve `SortedAlpha`, public API/C ABI, Surface behavior, and platform
  capabilities with fresh evidence after every accepted slice.
- Do not revive the old telemetry/sidecar/network-validator stack.

## Baseline Facts Confirmed Before Implementation

- The worktree started clean on `refactor/packed-atlas-d-reset` at `3150b7b`.
- The current renderer root is 5,792 lines; the eight sibling renderer modules
  total 5,004 lines, for 10,796 lines under `crates/gsplat-render-wgpu/src`.
- Relative to local `main` (`f4ebe65`), the branch adds about 21,300 lines and
  removes 362 across 104 files. This includes plans, benchmarks, formats, and
  platform work, so it is not itself a deletion target; it establishes why
  path-local code-size accounting is necessary.
- `lib.rs` currently owns public renderer/preflight types, offscreen rendering,
  Surface resource planning/presentation, paged runtime orchestration, CPU
  reference projection, SH evaluation/color refresh, and Direct GPU resource
  creation. This is the primary responsibility hotspot.
- Canonical docs and the original design agree that Direct should be the
  low-overhead default/rollback. The current four-slot path retains complete
  in-memory `SceneBuffers`; it proves local residency correctness but not
  metadata-first or bounded source/CPU/GPU streaming.
- Same-device historical evidence localized the performance regression to the
  forced paged path, not current Direct. Fresh local current/main behavior is
  still required before implementation.

## Audit Targets

- Surface resource planning and geometry-path switching in `lib.rs` and
  `surface_session.rs`.
- CPU reference projection and SH evaluation helpers embedded in `lib.rs`.
- Overlap between packed and paged GPU texture/pipeline construction.
- Full-scene assumptions crossing `spatial_pages`, `page_atlas`, `paged_gpu`,
  and the Surface paged runtime.
- Public exports and FFI/Web call sites that constrain safe moves.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Audit and baseline before implementation | Prevents architecture work from silently changing rendering semantics. |
| Treat the local four-slot implementation as a prototype source adapter | It is useful correctness evidence but is not the final streaming source model. |
| Prefer mechanical module moves before policy changes | Makes behavior-preservation proof smaller and gives later path selection a clearer owner. |

## Issues Encountered

| Issue | Resolution |
|-------|------------|
| The prior active plan marks the bounded Phase D-F scope complete | Preserve it as historical evidence and use this new bundle for architecture convergence. |

## Resources

- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`
- `handbook/ROADMAP.md`
- `handbook/GOLDEN_PRINCIPLES.md`
- `docs/plans/active/2026-07-11-native-first-playcanvas-strategy/`

## Code Audit — Renderer Ownership and Deletion Safety

- `Renderer` owns scene state, path-specific CPU derivation, sort scratch, and
  the native offscreen rasterizer. `SurfacePresenter` separately owns the
  surface device/queue, three GPU path resources, presentation, and the local
  paged scheduler. `SurfaceRenderSession` owns frame cadence and transactional
  path switching across the two.
- `SurfacePresenter::from_surface_async` and
  `SurfacePresenter::prepare_geometry_resources` duplicate the same
  Direct/Packed/Paged resource-construction match. This is a concrete cleanup
  candidate because both use the same renderer scene/caches and presenter GPU
  layouts; one helper can serve initial construction and switching.
- The roughly 1,000-line `SurfacePresenter` block can move mechanically into a
  single sibling module while remaining re-exported from the crate root. Child
  module privacy permits it to use existing private renderer caches/helpers,
  so this need not widen API or change behavior.
- `PageAtlasCpu`, `PageAtlasSlotCpu`, `PageAtlasError`, and
  `attribute_bytes_for_lod` have no production call sites outside their module
  and crate-root exports. They are nevertheless public experimental APIs and
  controlled safety/accounting fixtures. They are not approved deletion
  targets under the compatibility and fixture guardrails.
- `GpuInstance` builders and the covariance/projector helpers are likewise
  runtime-unselected reference/conformance APIs. Their lack of production draw
  calls does not prove they are disposable.
- Timer helpers are duplicated between `lib.rs` and `surface_session.rs`; this
  is low-risk cleanup but too small to justify its own module before the larger
  responsibility split establishes the right home.

## Fresh Current Baseline

- `cargo check --workspace` passed on Apple M4 Pro / Metal.
- `cargo test -p gsplat-render-wgpu --lib` passed 93 tests with one retained
  research oracle ignored. This includes Direct/Packed/Paged count and image
  parity, stale/cancel/generation/nonresident safety, bounded traces, and local
  Surface paged preparation.
- `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test
  conformance_sorted_alpha` passed on the native Metal adapter.
- Canonical 120-frame minimal Direct benchmark passed with
  `offscreen_geometry_pipeline=sorted_index_direct`, p95 GPU-complete
  2.6283 ms, p99 2.8526 ms, and zero missed 16.67 ms frames. These timing
  values are environment observations, not optimization gates.

## Fresh Main vs Current Direct Baseline

- A detached temporary worktree at local `main@f4ebe65` passed
  `cargo check --workspace` and GPU-required `SortedAlpha` conformance on the
  same Apple M4 Pro / Metal environment. The worktree was removed afterwards;
  the active branch never changed.
- Canonical minimal offscreen PNGs from main and current both reported
  `offscreen_geometry_pipeline=sorted_index_direct`, `visible_count=2`, and
  `drawn_count=2`. Both 1280x720 files have the identical SHA-256
  `c17f90b23a73b466348150266b610bbd54b5438408d5ab8975a261cfcb9c3c53`.
- The same 120-frame minimal Direct benchmark reported mean GPU-complete
  1.9003 ms on main and 1.8189 ms on current. This single short run shows no
  obvious current Direct regression but is not a performance claim.
- Renderer Rust source grew from 3,378 lines on main to 10,796 on current.
  Current production-before-test-module counts identify `lib.rs` as the
  dominant hotspot at 4,169 lines; `surface_session.rs` is 753 and the paging
  modules together carry substantial test coverage that should remain.

## Paged Runtime Boundary Findings

- `SpatialPage` embeds `Vec<u32>` indices into the full `SceneBuffers`; page
  bounds are also computed from that full resident source.
- `schedule_pages` is synchronous: it advances Requested through compressed,
  decoded, uploading, and resident states in one call. These state names prove
  transition safety, not real asynchronous I/O or bounded decode.
- `PagedAtlasGpu::upload_page` calls `extract_page_scene` and
  `pack_scene_with_encoding` directly from the full source on demand, then
  stores cloned source indices per slot. This is the concrete coupling the
  page-source slice must isolate.
- Surface and offscreen paths each own atlas/residency setup and call the same
  `sync_paged_active_set`; Surface additionally owns its own sort scratch. A
  shared internal active-set owner can remove duplicated orchestration before a
  future source implementation is introduced.

## S1 Accepted Result

- `SurfacePresenter`, its resource plan, Surface-local paged wrapper, and
  transaction helper now live in `surface_presenter.rs`; the crate root
  continues to re-export `SurfacePresenter` at the same public path.
- Initial Surface construction and runtime Direct/Packed/Paged switching now
  call one `create_geometry_resources` helper. The duplicate match was removed
  without changing resource types, order, or error propagation.
- `lib.rs` decreased from 5,792 to 4,917 lines. The new module is 869 lines;
  total renderer Rust source decreased from 10,796 to 10,790 lines. This is a
  responsibility split with a small proven cleanup, not a cosmetic line move
  presented as overall deletion.
- S1 fresh proof: workspace check passed; renderer lib 93 passed / 1 retained
  oracle ignored; GPU-required Metal conformance passed; FFI smoke rendered
  `drawn=2 visible=2`; strict renderer clippy, formatting, and diff hygiene
  passed.

## S2 Accepted Result

- `PagedActiveSet` is now the single internal owner for page metadata,
  `ResidencyManager`, `PagedAtlasGpu`, scheduling, evicted-slot clearing, and
  generation-checked page publication.
- Surface and offscreen paths both delegate active-set synchronization to that
  owner. The old offscreen `paged_atlas` / `paged_residency` pair and the root
  `sync_paged_active_set` function were removed.
- Public `PagedAtlasGpu`, residency, scheduler, CPU atlas, and fixture APIs were
  preserved. Controlled assertions now inspect the same state through the
  shared owner.
- The slice added one 105-line file and reduced the two consumers, for a net
  +26 renderer Rust lines versus S1 (+20 versus the original baseline). This is
  within the per-slice budget but not the terminal net-deletion target.
- S2 fresh proof: workspace check; renderer lib 93 passed / 1 ignored; all
  offscreen parity and paging safety cases; local Surface paged non-zero test;
  GPU-required Metal conformance; strict clippy, fmt, and diff hygiene.

## S3 Accepted Result

- `GeometryPathRequest::default()` and every pre-existing Surface constructor
  retain exact Direct/default behavior. Packed and forced Paged remain explicit
  diagnostic controls rather than automatic product choices.
- New opt-in `from_window_auto`, `from_raw_handles_auto`, and
  `from_canvas_auto` constructors request a compatible adapter first, then use
  that adapter's limits with the existing Direct preflight. A fitting scene
  remains Direct; only `ActiveAtlasRequired` selects Paged.
- The automatic path is selected before scene GPU resources are allocated. If
  resource preparation fails after selection, renderer state rolls back to the
  previous path; structured preflight failures propagate without mutation.
- No C ABI function or existing Rust signature changed. Host and wasm32 builds,
  strict public rustdoc, FFI smoke, the 95-test renderer suite, and required
  Metal SortedAlpha conformance all passed freshly.
- S3 adds no file and is +279 renderer Rust lines relative to S2, remaining
  under the slice budget. This policy clarity has a real code cost; S4/S5 must
  still bring the final renderer total below the 10,796-line starting point.

## S4 Accepted Result

- `LocalScenePageSource` is an explicitly local, borrowed adapter over the full
  `SceneBuffers` plus `SpatialPageSet`. It owns extraction, shared scene-range
  encoding, SH scale reuse, and attribute-LOD reduction.
- `DecodedPagePayload` contains only page identity, source-index mapping, and
  packed page buffers. The new internal GPU upload accepts this payload and has
  no source-container parameter. Existing public `PagedAtlasGpu::new`,
  `upload_page`, and `upload_page_if_current` signatures remain intact as local
  compatibility wrappers.
- The active-set runtime decodes and uploads one page at a time; the payload is
  dropped after each upload and fixed GPU slot bounds remain enforced. A
  decoded cache was intentionally not added because the current source already
  retains the entire scene and no A/B evidence justifies duplicate residency.
- This is not metadata-first streaming: page metadata still embeds source
  indices, scheduling remains synchronous, and full source data remains
  resident for sorting and view-dependent color refresh. Those limitations are
  now on the local adapter side instead of hidden inside GPU upload.
- Fresh proof passed: payload equivalence, workspace check, 96 renderer tests
  plus one retained ignored oracle, all Direct/Packed/Paged count/image and
  paging safety gates, local Surface non-zero preparation, required Metal
  conformance, FFI smoke, wasm32 check, strict clippy, fmt, and diff hygiene.
- S4 is one new 166-line file and +201 renderer Rust lines from S3. Total is
  11,296 lines, 500 above the 10,796-line starting point; S5 needs at least 501
  lines of proven deletion for a terminal net decrease.
