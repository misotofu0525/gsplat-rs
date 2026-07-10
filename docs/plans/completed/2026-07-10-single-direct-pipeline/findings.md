# Findings: Single Direct Runtime Pipeline

## Initial State

- The current branch already centralizes Web, desktop interactive, Android, and iOS Surface scheduling in `SurfaceRenderSession`.
- Runtime geometry is still selectable among CPU instances, direct sorted indices, and GPU preprojection.
- Native offscreen rendering supports CPU instances and GPU preprojection, but not direct sorted indices yet.
- The C ABI and Web API expose experimental path toggles that must remain source-compatible during the collapse.
- `bindings/android/build/` is unrelated untracked workspace content and must not be staged.

## Target Boundary

- Production: CPU depth sort -> sorted `u32` source IDs -> direct vertex/fragment rendering.
- Reference: CPU projected instances only in test/conformance helpers.
- Delete: compute-preproject production resources, async CPU geometry worker, runtime geometry pipeline selection, double-buffer/preproject knobs, and misleading A/B CLI flags.

## Runtime Inventory

- `SurfaceGeometryPipeline` currently selects among CPU-expanded instances, direct sorted-index rendering, and GPU preprojection. `OffscreenGeometryPipeline` selects between CPU-expanded instances and GPU preprojection.
- `SurfaceRenderSession` owns the surface runtime branching, async CPU geometry worker, geometry cache invalidation, and preprojection double-buffer controls. It is the main collapse point shared by Web, desktop, Android, and iOS.
- `SurfacePresenter` contains both direct resident-scene resources and legacy instance/preprojection resources. The direct path only needs scene attribute buffers, the sorted-index buffer, camera/viewport uniforms, and its render pipeline.
- `Renderer`/`GpuRasterizer` still branch for offscreen rendering. Direct offscreen rendering must exist before the last production use of CPU-expanded instances and compute preprojection can be deleted.
- The C ABI, JNI, Swift, Kotlin, and Web wrappers expose path-selection knobs. Stable entry points will remain but always resolve to direct rendering; internal path state and branches will be removed.
- Desktop and benchmark A/B command-line flags are not stable API and can be removed with their tests and documentation.
- CPU projection helpers and `GpuInstance` remain useful as a test/conformance correctness oracle, but will no longer be a selectable production renderer.

## Deletion Contract

- Delete runtime geometry enums and aliases, surface CPU-instance construction, the async surface geometry worker, preprojection pipelines/shaders/resources, and offscreen pipeline selection.
- Retain CPU depth sorting and async sorting. Upload sorted IDs only when order changes; update camera/viewport uniforms for every draw.
- Retain exported v0.1 C functions and wrapper option fields that callers may compile against, but implement them as compatibility shims.
- Do not stage the unrelated untracked `bindings/android/build/` directory.

## Publication Scope

- Current branch: `refactor/unified-render-pipeline`.
- Remote: `origin` -> `misotofu0525/gsplat-rs`.
- Publish as a draft PR against the remote default branch after all checks pass.

## Implemented Result

- Production Surface and offscreen rendering now share one WGSL shader, one bind-group layout, and the `DirectSceneResources` ownership model.
- PLY-derived positions, covariance terms, opacity, DC color, and SH coefficients are uploaded once per loaded scene. Frames update camera uniforms and upload sorted `u32` IDs only when CPU order changes.
- CPU projection remains behind `GpuInstance` builder APIs for tests/conformance. It is not selectable by any runtime renderer.
- The GPU compute-preproject code, CPU Surface-instance code, instance-buffer rings, async geometry worker, path enums/selectors, A/B CLI flags, and four obsolete shaders were removed.
- Android and Apple high-level options/examples no longer expose removed path knobs. Native async CPU sorting remains available; mobile defaults still use sort interval 2.
- The v0.1 C ABI retains the removed setters as successful, documented no-ops. The Web option/setter is also retained as a direct-path compatibility shim.
- Local packaging and simulator validation proves the direct path compiles and runs across Web, desktop, Android artifacts, and iOS Simulator. Physical Android/iOS device performance remains a separate device-only validation gap and is not claimed by this task.
