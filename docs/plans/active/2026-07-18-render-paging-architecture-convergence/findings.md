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

- `GeometryPath::default()` and every pre-existing Surface constructor retain
  exact Direct/default behavior. Packed and forced Paged remain explicit
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

## S5 Accepted Result

The S5 module cleanup remains useful, but it is **not** accepted as terminal
architecture cleanup after the independent audit.

- Surface exact/automatic window, raw-handle, and canvas constructors now share
  selection-specific private helpers. Existing signatures and default behavior
  are unchanged; automatic selection is exposed only by the additive
  `*_auto` constructors and one focused selector function.
- Renderer preprocessing/sort timing, frame-stat recording, and path-specific
  raster submission now have one implementation each. `render_frame` retains
  its sorted-index allocation by moving the vector out and restoring it before
  propagating raster errors; it does not introduce a per-frame clone.
- The duplicate timers in `surface_session.rs` were removed in favor of the
  crate's existing native/wasm timer helpers. Page scheduler, residency, paged
  GPU, and renderer tests share fixtures without removing test names, scenes,
  image thresholds, safety assertions, or the retained research oracle.
- Canonical handbook docs now say exactly what the code implements: Direct is
  the stable/default path, automatic Paged is opt-in and oversized-only,
  Packed is diagnostic, and `LocalScenePageSource` is a full-source local
  prototype rather than streaming.
- S5 adds no file and, including canonical docs and the final proof record,
  adds 620 lines while deleting 995 (net -375). Final renderer Rust source is
  10,792 lines versus 10,796 at the start (net -4), while the responsibility
  hotspot `lib.rs` is 4,648 versus 5,792 (net -1,144).

## Independent Audit Reassessment — New Acceptance Facts

- Aggregate renderer lines hid the wrong movement. From `3150b7b` to
  `eb12e68`, total Rust lines changed 10,796 to 10,792, but excluding each
  file's terminal `#[cfg(test)] mod tests` changes production code from 7,621
  to 7,821 (+200) and tests/fixtures from 3,175 to 2,971 (-204). The required
  production cleanup has not passed; final acceptance requires production
  source below 7,621 without deleting controlled tests or moving bulk code.
- `PageSource` is still a private synchronous `Option` contract that borrows a
  complete `SceneBuffers` and `SpatialPageSet`. The page set retains source
  indices for every splat, and each frame still performs synchronous decode,
  packing, global sort, and SH refresh. This is an uploader seam, not bounded
  source/CPU architecture.
- `SurfacePresenter` is 1,011 lines and still combines adapter/device
  negotiation, path policy, Direct/Packed/Paged resources, SH refresh, paged
  scheduling, and draw submission. Moving code out of `lib.rs` did not finish
  the responsibility split.
- The additive automatic constructors and selector have no production
  consumer. Web, C, Android, and iOS continue to pass explicit paths, so a
  scene over Direct capacity does not automatically reach Paged end to end.
- Auto selection has a concrete limits defect: `from_surface_async` preflights
  adapter limits, while `from_surface_with_adapter_async` requests
  `downlevel_defaults` with only `max_texture_dimension_2d` raised. A device
  exposes requested limits, so an adapter with storage limits above 128 MiB can
  be classified Direct and then rejected by `DirectSceneResources` under the
  lower actual device limits. B must first reproduce this in pure logic, then
  select against the effective limits that will be requested.
- `DecodedPagePayload` validates count only. It does not prove source indices
  are within the scene or that encoding and atlas contracts match; a future
  source can therefore cause `refresh_paged_hot_colors` to index out of bounds.
  C requires structured typed boundary failure without a streaming claim.
- Previous platform evidence is not terminal evidence: ignored Web/WASM build
  outputs, Android APK, and iOS app were built before `eb12e68`, and raw logs
  were not retained. The Surface paged unit test prepares resources but does
  not present, and no fixed-camera over-slot Paged count/image comparison binds
  `3150b7b` to final HEAD.

## Corrected Acceptance Consequences

- S1-S5 commits remain preserved as verified module-level work; none should be
  reverted solely because the overall claim was too broad.
- Overall state is `in_progress`. B is the only current code blocker, followed
  strictly by C, production cleanup D, consumer boundary E, and final evidence
  F.
- Production line accounting, typed page-source validation, real consumer
  reachability, and commit-tagged final artifacts are hard gates. Total source
  size, prepare-only Surface tests, or binary timestamps cannot substitute.

## B Audit — Effective Surface Device Limits

- `direct_scene_preflight` selects from
  `min(max_storage_buffer_binding_size, max_buffer_size)`. The Surface device
  descriptor starts from `wgpu::Limits::downlevel_defaults()` and raises only
  `max_texture_dimension_2d`; therefore its effective storage ceiling remains
  the downlevel value even when the adapter advertises more.
- Automatic selection currently runs before `surface_resource_plan`, using
  `adapter.limits()` directly. The later resource plan is validated against the
  same adapter limits, but `DirectSceneResources::new` observes the lower
  limits of the requested device. This confirms the audit's exact mismatch.
- The minimal fix boundary is private Surface limit planning, not the public
  selector or constructors: derive the limits that the device descriptor will
  request, use those limits for Auto Direct preflight and resource-plan
  validation, and keep adapter limits only for checking that the request is
  supported.
- A pure logic regression can use a scene between the downlevel and an elevated
  adapter storage ceiling. It must demonstrate that adapter-limit preflight
  says Direct while effective-request-limit preflight says
  `ActiveAtlasRequired` before the implementation is changed.

## B Accepted Result

- Red proof ran one exact module-qualified test: a 3,000,000-splat degree-0
  scene on a synthetic 256 MiB adapter produced `SortedIndexDirect` when Paged
  was required by the later device request.
- `surface_effective_device_limits` now starts from downlevel defaults, retains
  only the adapter's requestable texture ceiling for resource planning, and
  rejects adapters that cannot satisfy the base request. Auto Direct preflight
  and `surface_resource_plan` use those effective storage/buffer limits; the
  final descriptor lowers texture capacity to the path's required dimension.
- Adapter info, advertised limits, and effective limits travel through one
  private `SurfaceAdapterContext`, keeping the constructor below the strict
  clippy argument threshold. No public selector/constructor, Rust baseline API,
  or C ABI changed.
- Green proof passed the strengthened regression, all 97 active renderer tests
  with one retained oracle ignored, required Metal SortedAlpha conformance,
  `cargo check --workspace`, strict renderer clippy, formatting, and diff
  hygiene.
- B adds no file and changes renderer accounting from 7,821 production / 2,971
  test-fixture lines to 7,864 / 3,003. The +43 production lines are the explicit
  correctness cost; D must now remove at least 244 production lines to finish
  below 7,621 rather than hiding this work in test deletion.

## C Audit — Typed Page Source and Payload Boundary

- `PageSource::decode_page` returns `Option`, so a missing page is flattened
  into `RendererError::InvalidScene`; it cannot distinguish source lookup from
  payload validation failure.
- `DecodedPagePayload::from_local_scene` calls `extract_page_scene` before any
  bounds check. Because `SpatialPage` and its source indices are public, an
  invalid index can panic before reaching GPU upload.
- `upload_decoded_page` checks only page/token identity and equality among
  packed count, hot count, and source-index count. It does not know the source
  scene length and cannot reject an out-of-range index before
  `refresh_paged_hot_colors` indexes `scene.positions[index]`.
- `PagedAtlasGpu` stores `PageEncoding` but the decoded payload does not. The
  packed bounds, log-scale range, and SH scales can therefore disagree with the
  atlas contract while still passing the current count check.
- `PagedGpuError` is public and re-exported. Adding a public payload-error enum
  variant would change exhaustive downstream matches, so C will keep that API
  stable: private `PageSourceError`/`PagePayloadError` provide typed internal
  failures, while existing public GPU methods map them to existing variants.
- Minimal validation state is the source splat count stored by the atlas, its
  fixed page capacity, and its encoding. The payload must carry its encoding
  and validate counts, non-empty/capacity, source-index bounds, packed encoding,
  and SH sidecar shape before any GPU write or active-entry publication.

## C Accepted Result

- `PageSource::decode_page` now returns private typed `PageSourceError` values
  for missing pages and invalid local source indices. Local extraction cannot
  index a malformed public `SpatialPage` before the bounds check.
- `DecodedPagePayload` carries its `PageEncoding` and validates non-empty/page
  capacity, packed/hot/source counts, every source index against the atlas's
  stored scene length, payload and packed encoding against the atlas, and SH
  sidecar shape.
- `PagedAtlasGpu` validates before slot generation mutation, GPU writes, or
  active-entry publication. A GPU-backed regression confirms a future invalid
  payload maps through the existing public error surface and leaves the active
  draw set empty.
- Public `PagedGpuError`, public `PagedAtlasGpu` method signatures, Rust
  baseline API, C ABI, and `LocalScenePageSource` behavior remain unchanged.
  Typed private errors map to existing public variants at that boundary.
- Fresh gates passed 100 renderer tests with one retained ignored oracle,
  required Metal SortedAlpha conformance, workspace check, strict renderer
  clippy, wasm32 Web check, formatting, and diff hygiene. The wasm build retains
  only previously existing cfg-only warnings.
- C adds no file and moves accounting from 7,864 production / 3,003
  test-fixture lines to 7,978 / 3,129. D must delete at least 358 production
  lines to finish below the 7,621 baseline; none of that reduction may come
  from deleting these new safety tests.

## D Audit — Production Ownership and Proven Duplication

- `SurfacePresenter` is now 1,033 production lines after B/C. It represents
  exactly one active geometry path but stores three `Option` resources plus a
  separate `geometry_path`, forcing repeated path/resource checks and reset
  logic. A private enum can encode the invariant and remove duplicate state;
  this is structural deletion, not a file move.
- Surface Direct, Packed, and Paged each repeat the same acquire/view/encoder/
  render-pass/submit/present sequence. Offscreen Direct, Packed, and Paged
  repeat the same render-pass descriptor and draw sequence again. One shared
  command encoder helper can replace six copies while preserving clear color,
  pipeline, bind group, vertex count, and instance count as explicit inputs.
- Surface packed refresh stores three independent `Option` fields and clears
  them in multiple places. It should become one small owned state value only if
  doing so removes repeated transitions; moving the same lines to a new file is
  not sufficient.
- The first D slice will combine a shared render-pass encoder with the
  single-active Surface geometry enum. It may add at most two small files, must
  reduce total production lines materially, keep every test, and repeat full
  renderer/Metal/workspace/clippy verification before commit.
- Remaining production reduction after that slice must come from another
  proven duplicate owner, likely shared packed refresh or pipeline/resource
  construction. D acceptance remains numeric: 7,620 or lower, regardless of
  how much smaller `surface_presenter.rs` becomes.

## D1 Accepted Result — Shared Draw and Single Surface Geometry

- One 44-line private draw encoder now owns the sole `begin_render_pass` in the
  renderer source. Surface and offscreen Direct/Packed/Paged pass explicit
  labels, target, pipeline, bind group, clear color, vertex count, and instance
  count, preserving all six prior draw configurations without repeated pass
  descriptors.
- `SurfacePresenter` now owns exactly one `SurfaceGeometry` variant rather than
  a path plus three independently optional resources. The private Paged runtime
  alone is boxed to keep the enum compact; public constructors, methods, path
  values, Rust API, and C ABI are unchanged.
- The former Surface Packed fallback allocation was unreachable under both
  constructor and transactional path switching: every Packed presenter already
  owned Packed resources. Deleting that branch preserves first-frame refresh
  behavior and removes a second resource-construction path.
- Production accounting moved from 7,978 to 7,866 lines (-112), with all 3,129
  test/fixture lines retained. `SurfacePresenter` production code moved from
  1,033 to 918 lines. D still needs at least 246 more production-line deletions
  to finish at 7,620 or below.
- Fresh final D1 verification passed 100 renderer tests plus one retained
  ignored research oracle, required Metal SortedAlpha conformance, workspace
  check, strict renderer clippy, wasm32 Web check, FFI smoke, formatting, and
  diff hygiene. The wasm route retains only existing cfg-specific warnings.

## D2 Audit — Shared Pipeline/Layout Construction and Dead Packed State

- Direct and Packed pipeline factories repeat the same shader/module/layout,
  premultiplied-alpha target, vertex/fragment state, multisampling, and cache
  construction. Their only semantic differences are labels, shader source, and
  primitive topology. The shared draw module can own that invariant while the
  existing Direct/Packed wrappers preserve call sites and labels.
- Direct and Packed bind-group-layout factories repeat read-only vertex storage
  entries followed by one vertex uniform entry; only the number of storage
  bindings and label differ. One private builder can preserve binding numbers
  and visibility exactly.
- `PackedAtlasResources::{atlas_height, declared_attribute_resource_bytes,
  measured_hot_storage_bytes}` have no reader anywhere in the private module's
  reachable crate graph, and `write_hot_colors` is hidden behind an explicit
  dead-code allowance while the range method only forwards to the already-used
  offset writer. Reference search plus strict clippy after removal is the
  deletion proof; public Rust API and C ABI cannot name this private module.
- D2 is limited to those exact construction and dead-state duplicates. It adds
  no file, preserves shader/primitive/label values, and must show a further
  production-line reduction before an independent commit.

## D2 Accepted Result — Shared GPU Construction

- The draw owner now builds the shared storage-plus-uniform bind-group-layout
  shape and common premultiplied-alpha pipeline state. Direct and Packed keep
  small named wrappers specifying the original label set, shader source, and
  TriangleList/TriangleStrip topology respectively.
- Removed three private Packed accounting/dimension fields with no reader and
  the dead full/range color forwarding APIs. Banded refresh now allocates only
  its compact range and calls the existing checked offset writer directly.
- Production accounting moved from 7,866 to 7,769 lines (-97), with all 3,129
  test/fixture lines retained and no new file. D has removed 209 production
  lines in total and still needs at least 149 more to finish at 7,620 or below.
- Fresh final D2 verification passed 100 renderer tests plus one retained
  ignored research oracle, required Metal SortedAlpha conformance, workspace
  check, strict renderer clippy, wasm32 Web check, FFI smoke, formatting, and
  diff hygiene. Existing wasm cfg-only warnings remain unchanged.

## D3 Audit — Shared Frame Preparation

- Direct and paged visibility preprocessing duplicate camera validation,
  scratch reset/reserve, view-depth-row construction, visibility testing, and
  depth-key emission. Only the mapping from draw index to source index differs;
  one iterator-based private helper can preserve both mappings.
- Native packed, wasm packed, and paged hot-color refresh repeat the identical
  position-to-camera direction, SH evaluation, clamp, and RGB10 pack sequence.
  A single source-index color helper removes those copies without changing
  parallelism, iteration order, or upload grouping.
- Direct and Packed resource `prepare` methods duplicate sorted-index capacity
  validation and optional queue upload. The draw owner can return the same
  instance count before each path writes its distinct uniform payload.
- The three target-specific selected constructors reject zero dimensions before
  calling `from_surface_async`, which performs the same check before adapter
  negotiation. Keeping the lower common check preserves the exact error while
  deleting unreachable duplicate validation.
- wgpu 28's `BlendState::PREMULTIPLIED_ALPHA_BLENDING` exactly equals the
  hand-written One/OneMinusSrcAlpha/Add color and alpha components used by the
  shared pipeline. Using the dependency's named constant is source-equivalent.

## D3 Accepted Result — Compact Color and Validation Paths

- Measurement rejected two initially prototyped abstractions before
  verification: the visibility mapper added one production line, and the
  sorted-order helper added roughly eight. Both were removed rather than
  retaining indirection that failed the cleanup objective.
- The accepted slice keeps one shared source-index SH-to-RGB10 evaluator for
  native packed, wasm packed, and paged refresh; uses wgpu's exact named
  premultiplied-alpha constant; and keeps zero-size validation only at the
  common pre-adapter boundary.
- Production accounting moved from 7,769 to 7,732 lines (-37), with all 3,129
  test/fixture lines retained and no new file. D has removed 246 production
  lines in total and still needs at least 112 more to finish at 7,620 or below.
- Fresh final D3 verification passed 100 renderer tests plus one retained
  ignored research oracle, required Metal SortedAlpha conformance, workspace
  check, strict renderer clippy, wasm32 Web check, FFI smoke, formatting, and
  diff hygiene. Existing wasm cfg-only warnings remain unchanged.

## D4 Audit — Remove Duplicate Render Orchestration State

- Private `GpuRasterError` is a complete one-for-one facade over existing
  `RendererError` variants, followed immediately by `From` translation at all
  caller boundaries. Returning `RendererError` directly preserves public error
  codes and payloads while deleting the duplicate enum and mapping owner.
- The offscreen creation path checks non-zero dimensions in the validated
  `RendererConfig`, `create_async`, `offscreen_device_limits`, output-target
  creation, and resize target setup. All private callers enter through config
  validation or `offscreen_device_limits`; retaining those boundary checks and
  the device-limit check makes the inner repeated zero checks unreachable.
- `SurfacePresenter::render_sorted_indices` dispatches by the path derived from
  `SurfaceGeometry`, then each private render method matches the same enum again
  and can only return `SceneNotLoaded` on an impossible mismatch. One enum
  dispatch can prepare the active resource, set the count, and present once.
- Auto selection currently computes effective device limits before building
  `SurfaceAdapterContext`, then recomputes them in a second private adapter
  helper. Selecting from the already-derived effective limits preserves B's
  regression and deletes the duplicate calculation.
- Four `SurfaceResourcePlan` fields cache local inputs used only by one test;
  the tested resident capacity is already observable through exact packed
  preflight byte assertions. Removing the cached copies keeps the gate while
  shrinking runtime state. `PackedAtlasResources::splat_count` likewise has no
  reader after D2 and is private to an unexported module.

## D4 Accepted Result — Production Cleanup Gate Passed

- Offscreen internals now return the existing public `RendererError` directly;
  the private mirror enum and complete translation impl are gone. Error codes
  and dimension payloads are unchanged, and the existing limit regression now
  asserts the public error variant.
- Surface preparation now matches the single active geometry once, updates the
  shared instance count, and presents once. Impossible second path/resource
  mismatch checks and three private render-method shells are gone.
- Auto selection consumes the already-derived effective device limits; the B
  regression still proves the 256 MiB adapter / downlevel requested-limit case
  chooses Paged. Resource-plan cached test inputs and the unread private Packed
  splat count were removed while explicit slot/capacity assertions were retained.
- Production accounting moved from 7,732 to 7,607 lines (-125), with all 3,129
  D-baseline test/fixture lines retained and no new file. Across D, production
  moved 7,978 -> 7,607 (-371); against `3150b7b`, production is 7,621 -> 7,607
  (-14), so the corrected cleanup gate now passes without test deletion.
- `SurfacePresenter` production code is 843 lines, down from the independent
  audit's 1,011 and D's 1,033-line starting point. Adapter negotiation remains
  in the module, but draw ownership, resource construction, and path-specific
  frame preparation no longer exist as six repeated bodies or parallel options.
- Fresh final D4 verification passed 100 renderer tests plus one retained
  ignored research oracle, required Metal SortedAlpha conformance, workspace
  check, strict renderer clippy, wasm32 Web check, FFI smoke, formatting, and
  diff hygiene. Existing wasm cfg-only warnings remain unchanged.

## E API Boundary Audit — Automatic Consumer Blocked by Product Surface

- All real SDKs load the scene before presenter construction, so the internal
  Rust Auto constructors are technically usable at the correct point. The
  blocker is not scene timing or adapter availability; it is the public choice
  required to request Auto without changing stable Direct semantics.
- Web's `createRenderer` and wrapper `geometryPath` default explicitly to
  Direct, while the optional constructor accepts only IDs 0/1/2 and the public
  type accepts only `direct | packed | paged`. Calling `from_canvas_auto`
  requires a new wasm-bindgen export or an `auto` option, both additive JS/WASM
  product API changes. Reinterpreting `direct` would violate its documented
  explicit/default meaning.
- The C ABI defines `GsplatGeometryPath` as Direct/Packed/Paged, default mobile
  constructors pass Direct ID 0, and `geometry_path_from_ffi` rejects every
  other value. Calling `from_raw_handles_auto` requires a new enum value or C
  constructor. Android and Apple mirror that enum in Kotlin/Swift and default
  `GsplatSurfaceOptions.geometryPath` to Direct, so the ABI change necessarily
  widens both SDK product surfaces and regenerated binary/header artifacts.
- Runtime `setGeometryPath` is also explicitly 0/1/2 and cannot represent the
  constructor-time adapter-limit policy without adding the same public value.
  Silently treating an existing value as Auto would make Direct non-explicit
  and break existing A/B behavior.
- E therefore records an accepted API-boundary blocker and makes no code,
  C-header, generated artifact, or default-behavior change. A future reviewed
  product decision may add one named Auto option consistently across Rust,
  Web, C, Kotlin, and Swift; this thread does not invent it.

## F Evidence Harness and Over-Slot Fixture

- The desktop example already owns deterministic auto-camera and PNG output,
  so an explicit geometry-path CLI switch is the smallest same-camera evidence
  seam. It is example-only and leaves the renderer, SDKs, C ABI, and default
  Direct behavior unchanged.
- The local Kitsune fixture declares 279,199 source splats, exceeding the
  current four slots times 65,536 splats (262,144). A fixed 640x360, one-frame,
  zero-yaw Paged probe selected `paged_active_atlas`, wrote a 91 KiB PNG, and
  reported 225,784 visible and drawn active splats.
- `3150b7b` predates the CLI switch but the desktop example was otherwise
  unchanged through the pre-harness final state. Applying only the harness
  commit in a disposable worktree and requiring no renderer diff isolates the
  renderer comparison from evidence plumbing.
- An ignored PNG or platform binary cannot prove its source revision. The
  accepted provenance is a clean exact HEAD captured in a text manifest plus
  raw command logs in a HEAD-named evidence directory.
- The local Android device list is empty. A final APK/JNI build can prove
  toolchain compatibility but cannot replace Direct/Paged Surface execution;
  this keeps overall completion unavailable unless hardware appears.
- Candidate core evidence passed all workspace tests, required Metal
  SortedAlpha conformance, strict workspace clippy, rustdoc, FFI smoke, and
  focused parity/safety/continuity/payload suites. The production-only recount
  exactly reproduces `7,621 -> 7,607`; renderer tests remained 3,129 lines
  throughout D even though the earlier audited start contained 3,175.
- Candidate Web output was rebuilt and hashed after seven package tests. The
  Android JNI host smoke and Kitsune APK build passed, but there is no device
  run. On the available iPhone 17 Pro simulator, real Direct and Paged Surface
  runs both created successfully and completed 120 measured frames.
- The iOS count split is path-correct: Direct drew all 279,199 visible splats;
  Paged saw the 279,199 source-visible splats and drew its 225,784 active
  residents. Paged was about 5.4x slower in this simulator observation, so no
  performance improvement or threshold is claimed.
- The same over-slot Paged camera on `3150b7b` and the candidate produced
  225,784 visible/drawn splats, byte-identical 640x360 PNGs, zero absolute
  pixel error, and repository SSIM 1.0. The renderer comparison is isolated
  because the temporary baseline worktree applied only the desktop CLI harness
  and had an empty `crates/gsplat-render-wgpu` diff.

## F Device Refresh — Android A/B and Physical-iOS Boundary

- The API 36 arm64 AVD is useful only after a cold boot. Its saved quickboot
  state exposed a launcher in package metadata but rejected the same explicit
  activity; cold boot restored normal package-manager resolution. A separate
  temporary user-data image avoided modifying the user's stored AVD and
  provided enough installation space for the 90 MiB APK.
- Minimal Direct and Paged both complete on the same gfxstream/SwiftShader
  backend, so the backend is not generally incapable of running the renderer.
  Kitsune Paged also completes with the expected 279,199 source / 225,784
  resident-drawn split, proving the final fixed-slot Surface path executes on
  Android even though it is extremely slow in software rendering.
- Kitsune Direct crashes before presentation in wgpu buffer creation. The
  caller return addresses distinguish the three `create_buffer_init` calls:
  params returns at `0x32a0d4`, source at `0x32a150`, and the failing SH-rest
  call at `0x32a19c`. Degree-3 Kitsune SH-rest is 50,255,820 bytes. The same
  final and `3150b7b` APKs fault at `0x7b02fe98d0`, so this AVD behavior is a
  baseline-equivalent large mapped-upload limitation, not evidence of a
  refactor regression.
- The A/B result narrows the claim but does not convert failure into success:
  Android final evidence now contains minimal Direct/Paged and over-slot Paged,
  while large-scene Direct required a physical-device run rather than a
  SwiftShader-specific upload workaround. The later A065 run supplies it.
- Physical iOS initially failed while locked, then later exposed a connected
  tunnel and unlocked state. Exact-`ca27053` Kitsune Direct and Paged runs both
  completed 30 measured frames with the expected 279,199 source-visible count;
  Direct drew 279,199 and Paged drew 225,784 active residents.
- Sakura narrows the Android AVD issue beyond one dataset: its 236,178 splats
  require 42,512,040 SH-rest bytes and fail in the same Direct resource stage.
  Replacing the single mapped buffer initialization with an unmapped buffer and
  4 MiB queue-write chunks moved the SIGSEGV into wgpu queue-write `memcpy`.
  Therefore simple chunking is not a valid fix for this SwiftShader mapped-
  memory behavior; the experiment was reverted instead of adding workaround
  complexity to the Direct default.

## F Physical Android Final Evidence

- The reconnected Nothing A065 is a real API 35 / Adreno Vulkan target. A
  clean install of the exact-`64f704d` Kitsune APK completed both constructor-
  selected Surface paths with 120 measured frames and canonical v1 artifacts.
- Direct retained the intended small/default implementation and drew all
  279,199 visible splats at an observed 11.330 ms/frame. Paged loaded the full
  279,199 local source while drawing the fixed active set of 225,784 at 23.626
  ms/frame. Counts match desktop, simulator, and physical-iOS evidence.
- Paged is about 2.1x slower in this observation despite drawing fewer splats.
  That result reinforces the honest boundary: fixed GPU slots and validated
  residency are complete, but this is not yet metadata-first bounded streaming
  or a performance win.
- The AVD large-buffer crash is not a platform regression claim: physical
  Direct succeeds, baseline/current fail identically only on SwiftShader, and
  the ineffective queue-chunk experiment remains reverted.

## Prior Evidence — Not Terminal HEAD-Bound Proof

- The earlier `cargo test --workspace` run passed. The renderer reported 96
  passed and one explicitly retained research oracle ignored; the required Metal
  `SortedAlpha` conformance test passed again with
  `GSPLAT_REQUIRE_GPU_CONFORMANCE=1`.
- Final canonical 1280x720 Direct PNG reported `visible=2`, `drawn=2`, and the
  same SHA-256 as fresh main/current baselines:
  `c17f90b23a73b466348150266b610bbd54b5438408d5ab8975a261cfcb9c3c53`.
- Final 120-frame native Direct observation used
  `sorted_index_direct`, mean GPU-complete 1.8581 ms, p95 3.1114 ms, p99
  3.1361 ms, with zero misses. This is a correctness/regression observation,
  not a performance guarantee.
- Android device `A065` ran the real 279,199-splat Kitsune model during the
  earlier slice, but the APK/logs are not reliably bound to `eb12e68`.
  Direct completed 120 frames at 14.969 ms average frame time; the four-slot
  Paged prototype completed at 36.587 ms and drew 225,784 active splats on
  average. The slower Paged result is retained as evidence that fixed local
  residency is not yet a performance win or true streaming.
- An iPhone 17 Pro simulator app completed a 120-frame Kitsune
  Direct Surface run with `avg_frame_ms=18.992`, `avg_visible=279199`, and
  `avg_drawn=279199`. This is simulator evidence, not a physical-iOS-device
  claim.
- Web WASM build, package check, and seven package tests passed earlier. The
  ignored build output predates `eb12e68`, so it must be rebuilt for F. The local
  showcase then reported `renderer=wasm_sorted_index_direct`, `avg_visible=3`,
  and `avg_drawn=3` from the real Rust/WASM Surface path.

## Remaining Risks and Explicit Non-Claims

- `LocalScenePageSource` still retains complete `SceneBuffers` and page index
  vectors; scheduler state changes are synchronous. Metadata-first source I/O,
  bounded compressed/decoded caches, and asynchronous cancellation remain
  future work.
- Paged uses four fixed GPU slots and a global SortedAlpha order over the active
  set, but it is not an end-to-end bounded CPU/source/GPU streaming pipeline.
- Android Kitsune Paged averaged 3,364 ms/frame on the software-rendered AVD;
  large-scene Direct crashed before rendering, so no same-scene performance
  comparison is valid. No threshold is claimed, and no telemetry/sidecar/
  network validator machinery was restored to disguise that result.
- Earlier Android evidence used a physical device but is not terminal-HEAD
  proof. Fresh A065 evidence now covers both 120-frame Kitsune paths on
  `64f704d`, and physical iOS covers both paths as well. The final status-only
  commit still requires its exact-HEAD rerun before completion is declared.
- Production cleanup now passes at 7,607 lines versus the 7,621-line baseline.
  Auto selection now uses effective requested-device limits, and decoded page
  payloads have typed lookup and structural bounds validation. The remaining
  product gap is the deliberately unexpanded public Auto option, not those
  corrected implementation defects.
