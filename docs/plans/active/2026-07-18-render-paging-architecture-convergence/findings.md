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
- Android Paged is materially slower than Direct in this fresh run. No
  performance threshold is claimed, and no telemetry/sidecar/network
  validator machinery was restored to disguise that result.
- Android evidence is physical-device evidence; iOS evidence is simulator-only.
  A physical iOS rerun remains useful before a release-level mobile claim.
- Production code is currently 200 lines above the accepted baseline, automatic
  selection can use incompatible limits, decoded page payloads are
  under-validated, and no production consumer exercises the automatic path.
