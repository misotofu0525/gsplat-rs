# Findings & Decisions

Status: completed on 2026-08-13.

## Requirements

- Rename the misleading Direct path terminology.
- Remove Packed and Paged related implementation and public selection surfaces.
- Verify the resulting workspace according to repository guidance.
- Commit the verified result directly on `main`.
- Preserve unrelated user changes and avoid compatibility layers.

## Research Findings

- Historical evidence says Direct is the stable/default path.
- Historical Paged implementation retained full `SceneBuffers`, was not bounded end-to-end streaming, and was slower on previously measured Android and iPhone devices.
- `Packed` is a storage-layout dimension while `Paged` is a residency mechanism; they should not remain mutually exclusive geometry modes.
- Current docs still treat `GeometryPath::{Direct,Packed,Paged}` as a cross-platform selector in renderer construction, Surface scheduling, C ABI, JNI/Swift, and Web WASM setup.
- `PROJECT_CONTEXT.md`, `ARCHITECTURE.md`, `ROADMAP.md`, and `GOLDEN_PRINCIPLES.md` all contain durable Packed/Paged runtime claims that will become stale after deletion.
- The pre-existing ROADMAP edit is a substantial authorized rewrite from the prior roadmap task, not an incidental unrelated line; it must be reconciled with the resident-only result rather than discarded.
- Repository structure will change because multiple renderer modules are expected to disappear, so current-doc sync must at least cover project context and architecture; verification needs inspection for obsolete selectors and commands.
- Tracked Packed/Paged renderer modules are `packed_atlas.rs`, `packed_gpu.rs`, `page_atlas.rs`, `page_scheduler.rs`, `page_source.rs`, `paged_active_set.rs`, `paged_gpu.rs`, `residency.rs`, and `spatial_pages.rs`, plus `splat_surface_packed.wgsl`.
- `GeometryPath` is embedded in `Renderer`, `SurfacePresenter`, and `SurfaceRenderSession`; resource planning, device-limit negotiation, frame stats, and runtime switching all branch on it.
- Geometry selection is exposed in the desktop CLI, bench runner, C ABI/header, Android JNI/Kotlin, Apple Swift wrapper/example, Rust/WASM API, and the local Web ESM package.
- Generated/ignored Android build outputs, wasm package output, Web dist files, and the built XCFramework mirror stale strings but are not source-of-truth edits; canonical build scripts should regenerate them during verification.
- Generic uses of the word `packed` in SPZ bit decoding and unrelated test fixtures are not part of this removal.
- `Renderer` currently stores `geometry_path` and `spatial_pages`; switching paths rebuilds CPU data and clears GPU resources. The single resident path can directly precompute covariance/alpha on load and validate indices against scene length.
- `RendererError`, `DirectSceneError`, and `SurfacePresenterError` contain Packed/Paged-only error variants that can be deleted with those paths.
- The failed `origin/codex/native-render-core-refactor` branch is not a safe source for this task: its resident work is intertwined with roughly 58k lines of broader experimental changes across 120 relevant files. Do not cherry-pick it.
- Most production Packed/Paged code in `lib.rs` is isolated in path-specific preprocessing/color refresh, packed preflight, and `GpuRasterizer` fields/methods; those blocks can be deleted without changing canonical SortedAlpha shaders or CPU sorting.
- The current resident capacity report is still named `DirectScenePreflight` and recommends an active atlas. It should become `ResidentScenePreflight` with an explicit `ReduceScene` remediation now that no automatic/non-streaming atlas exists.
- `SurfacePresenter` currently owns two pipelines, two bind-group layouts, a three-variant `SurfaceGeometry`, packed color refresh state, and a paged runtime. The resident-only presenter can own a single `ResidentSceneResources` value.
- `SurfaceRenderSession` geometry switching is a separable transaction helper; all remaining GPU/Adaptive and AsyncLatest scheduling can stay after deleting only geometry guards and paged frame-count branches.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Product runtime becomes a single full-resident SortedAlpha path | It is the only currently release-gated path and avoids a strategy cross-product |
| Future compact storage and streaming/LOD remain roadmap capabilities, not dormant runtime branches | Keeps current code honest and small without abandoning long-term SDK differentiation |
| Existing `handbook/ROADMAP.md` edit will be reviewed before inclusion | It predates this task and must not be blindly overwritten or committed as unrelated work |
| Remove `GeometryPath` instead of replacing it with a one-value public enum | A selector with one valid product path has no semantic value and would preserve obsolete API complexity |
| Preserve internal names such as direct sorted-index draw only when they describe access/command behavior, not residency | `direct` can still be technically accurate outside the removed geometry-mode taxonomy |
| Implement the cleanup directly on current `main` rather than importing the failed refactor branch | The branch mixes the desired vocabulary with large unverified renderer/telemetry/tiled rewrites |

## Issues Encountered

| Issue | Resolution |
|-------|------------|

## Resources

- `AGENTS.md`
- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`
- `handbook/ROADMAP.md`
- `handbook/GOLDEN_PRINCIPLES.md`
