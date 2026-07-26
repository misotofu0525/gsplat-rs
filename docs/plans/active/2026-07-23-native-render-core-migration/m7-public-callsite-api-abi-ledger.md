# M7 Public Call-Site / API / ABI Ledger

## Status and fixed identity

- Fact baseline: `707cd8001abf84cd9fc990dd45246a0c02db75ef`
  (`refactor(render): remove tiled exact diagnostic`).
- Baseline parent: `a798b8accf454e96792df45d38e8564e7e27556e`.
- Program status: **M7 Active; M7 acceptance Deferred**. M8 is not activated and
  Package M remains Active.
- Deliverable status: **documentation evidence only**. This ledger changes no
  product source, Rust API, C ABI, JNI, Kotlin, Swift, WASM, JavaScript,
  TypeScript, test, script, generated artifact, runtime route, or rendered
  behavior.
- The rejected candidate `b0a4a3e14c4e15af2478327143540a02cdfba6c1`
  predates the TiledExact deletion and is not an evidence source for this
  ledger. Every fact below was re-derived from the fixed baseline, its direct
  parent, or a fresh command recorded below.

This document satisfies one inventory input to the M7 exit contract. It does
not prove final single ownership, the complete consumer/global matrix,
rollback, or fixed-SHA acceptance.

## Classification

| Mark | Meaning | M7 treatment |
| --- | --- | --- |
| **P** | Current in-tree product/application/package route | Preserve unless an accepted consumer migration removes it. |
| **C** | Current compatibility or externally callable public surface | An absent in-tree call site is not deletion authority. Preserve or change only through an explicit compatibility decision. |
| **D** | Diagnostic, smoke, benchmark, or qualification surface | Retain only with a named owner, caller, and verification. |
| **R** | Removed at or before the fixed baseline | Historical only; do not describe it as current. |
| **Q** | Decision or acceptance remains Deferred | Inventory is not acceptance evidence. |

`R` means removed. It does not mean merely legacy, compatibility-only, or
deprecated. In particular, the C/JNI/Kotlin Surface stats chain remains `C`.
Only the Swift Surface wrapper adds an explicit language-level deprecation.

## Current Surface owner conclusion

At the fixed baseline, all in-tree Packed Surface product constructors select
`SessionSurfaceOwner::ExactPacked(SurfacePresenterHost)`. The decision is
encoded by `SessionSurfaceConstruction::for_geometry` in
[`surface_session.rs`](../../../../crates/gsplat-render-wgpu/src/surface_session.rs)
lines 1696-1720: Packed selects the host-only Exact route; Direct and Paged
select the standalone `SurfacePresenter` route.

The product entrypoints are:

- desktop `SurfaceRenderSession::from_window` in
  [`viewer.rs`](../../../../examples/desktop/src/viewer.rs);
- C/Android/UIKit `SurfaceRenderSession::from_raw_handles` in
  [`gsplat-ffi-c/src/lib.rs`](../../../../crates/gsplat-ffi-c/src/lib.rs);
- browser `SurfaceRenderSession::from_canvas` in
  [`gsplat-web/src/wasm.rs`](../../../../crates/gsplat-web/src/wasm.rs).

The low-level public composition `SurfacePresenter::{from_window,
from_raw_handles,from_canvas}` plus `SurfaceRenderSession::new` still exists as
a Rust compatibility/diagnostic route. Its direct in-tree construction is the
desktop public-entry regression in
[`surface_geometry_entry.rs`](../../../../examples/desktop/tests/surface_geometry_entry.rs),
not a platform product constructor. After `707cd80`, that standalone presenter
can still own Direct, Paged, or legacy Packed Global/Projected resources, but it
cannot own TiledExact.

This route separation is not the final M7 single-owner proof. The retained
standalone Packed/Global compatibility graph still needs an explicit retain or
delete decision and the full M7 verification matrix.

## TiledExact deletion and caller impact

`707cd80` removes TiledExact as code and public behavior. Current source exposes
only `GlobalQuads` and `ProjectedQuadsExact` in
[`api.rs`](../../../../crates/gsplat-render-wgpu/src/api.rs) lines 41-46.

| Former entry or behavior | Fixed-baseline state | Caller impact / classification |
| --- | --- | --- |
| `SurfaceRasterExecutionPlan::TiledExact` | Variant removed; the enum moved to `api.rs` with two variants | **R.** Rust callers naming `TiledExact` no longer compile. Current callers may use only Global or Projected. |
| `ResidentTiledError` and the `RendererError::ResidentTiled` / `SurfacePresenterError::ResidentTiled` variants | Removed from crate-root exports and error enums | **R.** External pattern matches or conversions naming these entries must be removed. |
| `tiled_resident_gpu.rs` and five tiled WGSL shaders | Deleted by `707cd80` | **R.** There is no retained tiled runtime, lazy allocation, count/readback, or render path. |
| `SurfacePresenter::tiled_entry_count` / `tiled_entry_capacity` | Public accessors removed | **R.** The desktop regression stopped snapshotting these values. |
| `SurfaceFrameOutput::tiled_preparation_pending` | Public compatibility field removed | **R.** Desktop, Web example, JS normalization, TypeScript, and tests now use only `gpu_order_preparation_pending` / `gpuOrderPreparationPending`. |
| raw WASM `rasterExecutionPlan="tiled_exact"` and package TypeScript `"tiled_exact"` union member | Removed | **R.** Web callers must not expect the label. There is no raw or package Tiled selector. |
| former `SurfaceRenderSession::set_raster_execution_plan(TiledExact)` path | Tiled argument deleted; the method itself remains public for Global/Projected compatibility | **R** for Tiled, **C/D** for the remaining method. Exact Packed accepts only Projected; standalone Packed may switch Global/Projected; Direct/Paged accept only Global. No C/JNI/Kotlin/Swift/WASM/JS raster setter exists. |
| desktop `--surface-raster-plan` selector | Removed earlier at `24157b933d15662a29b9e2347bb945155b08ecbf` | **R.** Current desktop callers explicitly reapply Projected; the retired CLI is rejected. |

The current in-tree calls to `SurfaceRenderSession::set_raster_execution_plan`
are limited to desktop viewer/evidence/test code and all pass
`ProjectedQuadsExact`. The public method remains a two-plan Rust compatibility
entrypoint; TiledExact does not remain current through it.

The handbook and renderer/desktop README files still contain historical Tiled
prose at this baseline. Those stale documents are M8 correction inputs, not
evidence that the deleted route survives.

## Rust public API and call-site ledger

### Crate-root export groups

The complete export groups are defined in
[`crates/gsplat-render-wgpu/src/lib.rs`](../../../../crates/gsplat-render-wgpu/src/lib.rs)
lines 41-126.

| Public group | Class | Current disposition |
| --- | --- | --- |
| `GeometryPath`, `PreprocessOutput`, `SurfaceRasterExecutionPlan` | P/C/D | Geometry is used by desktop, bench, C, WASM, and tests. Raster now has only Global/Projected. Retain. |
| `GpuInstance`; `RESIDENT_*`; `ResidentChunkMeta`, `ResidentColorAux`, covariance/position/SH layout records | P/C | Public GPU/resident layout and compatibility data used by renderer construction and tests. Retain. |
| all `SurfaceCompatibility*` submission/terminal/count/order/projected/producer DTOs | C/D | Renderer-owned compatibility evidence translated by the C layer. They are not a second current-stats owner. Retain while ABI consumers remain. |
| `ResidentGpuError` | P/C | Public preparation error. Retain. `ResidentTiledError` is removed. |
| `SurfaceGpuOrderProducer`, producer draw scope and producer success/failure DTOs | D/Q | Producer selection participates in the complete Exact PlanSet. Independent compatibility telemetry remains separate from Exact current-stats qualification. |
| CPU/GPU order measurement, failure, reason, and timing-source DTOs | C/D | Published diagnostics consumed by FFI/platform qualification. Retain while consumers remain. |
| packed-atlas constants, CPU records, encode/decode helpers, sizing helpers, and `pack_scene*` functions | P/C | Packed scene construction/accounting contract. Retain. |
| projected execution/measurement/failure DTOs | C/D | Compatibility and qualification surface. Retain. |
| Direct/Packed/Resident scene builders, paths, preflight reports/errors, byte plans, source splats, and preflight helpers | P/C/D | Product scene construction plus Direct/Paged diagnostics. Retain. |
| all `SurfaceCurrentStats*` request/submission/poll/receipt/failure/count DTOs | P/D | Authoritative Surface `S/V/C/D` observation contract. Pending/unavailable remains count-free. Retain. |
| `SurfaceFrameCapture`, `SurfacePresenter` | C/D/Q | Capture is diagnostic. The low-level presenter remains public and is the standalone compatibility composition; its final M7 disposition is Deferred. |
| `SurfaceAdaptive*`, frame output/timings, order/projected/producer submission and unsampled DTOs, policy enums, `SurfaceRenderSession`, `SurfaceSortSchedule` | P/C/D | Shared Surface product facade and compatibility/evidence data. Retain. `SurfaceFrameOutput::tiled_preparation_pending` is removed. |

### Constructors and mutable entries

| Entrypoint | Class | Actual in-tree scope |
| --- | --- | --- |
| `Renderer::{new,with_config}` | P/C | GPU offscreen construction used by desktop, bench-runner, C context, and tests. |
| `Renderer::{new_for_surface,with_config_for_surface}` and pre-session `set_geometry_path` | P/C/D | Shared Surface scene construction. Geometry selection here precedes session publication. |
| `SurfacePresenter::{from_window,from_raw_handles,from_canvas}` | C/D/Q | Low-level public compatibility constructors. Product `from_*` session constructors bypass the standalone presenter for Packed. |
| native/WASM `SurfaceRenderSession::new` | C/D/Q | Accepts a caller-created presenter. Direct in-tree construction is test-only; it is not the product platform root. |
| `SurfaceRenderSession::{from_window,from_raw_handles,from_canvas}` | P | Desktop, C/mobile, and WASM product roots respectively; Packed selects host-only Exact. |
| `SurfaceRenderSession::{set_geometry_path,set_geometry_path_async}` | C/D | Native Direct/Paged remains transactional; transitions entering/leaving Packed reject; Web changed-path requests reject. Same-path calls are idempotent. |
| `SurfaceRenderSession::set_raster_execution_plan` | C/D | Current two-plan compatibility entry. Exact Packed is Projected-only; no Tiled variant exists. Desktop callers only reapply Projected. |
| order/projected/producer policy setters and receipt drains | P/C/D | Compatibility inputs translate to complete Exact plans or fail before mutation; renderer/session owns policy and terminals. |
| current-stats request/submission/poll and capture methods | P/D | Explicit non-blocking observation and native capture. They do not authorize fallback counts or platform-owned ticket state. |

## C header/source ABI ledger

The public declarations are in
[`gsplat.h`](../../../../crates/gsplat-ffi-c/include/gsplat.h); implementations
are in [`lib.rs`](../../../../crates/gsplat-ffi-c/src/lib.rs). The baseline has
58 header functions and 62 Rust `extern "C"` definitions. There is no
header-only symbol. The four intentional source-only example/compatibility
symbols are listed below.

| Exact symbol group | Class | Disposition |
| --- | --- | --- |
| `gsplat_{version_major,version_minor,error_message,last_error_message,config_default,camera_default}` | P/C | Common ABI/version/default utilities. Retain. |
| `gsplat_context_{create,destroy,set_camera,set_auto_camera,load_scene_path,render_frame,get_stats}` | P/C | Stable offscreen context ABI, independent of Surface owner deletion. Retain. |
| Android/UIKit default and `_with_geometry_path` Surface constructors | P/C | Default C constructors pass Packed ID `1`; typed Kotlin/Swift wrappers use the explicit-path forms. All route through `SurfaceRenderSession::from_raw_handles`. |
| Surface destroy/resize/frame-latency/camera/render lifecycle | P | Product lifecycle ABI. Retain. |
| sort interval, order backend, async sort, projected policy, geometry-path, and GPU-order-producer setters | P/C/D | Current compatibility/product controls. They map to shared session decisions; no C raster/Tiled setter exists. |
| `set_gpu_preproject`, `set_gpu_preproject_double_buffer`, `set_async_geometry`, `set_instance_buffer_count` | C | Published v0.1 compatibility no-ops. They remain ABI symbols and are not classified as removed. |
| current-stats request/submission/poll plus receipt pump and status-reporting pump | P/D | Authoritative native current-count and bounded callback-progress ABI. Retain. |
| `gsplat_surface_renderer_get_stats` | C | Current fail-closed compatibility getter. Synchronous current counts can succeed; unavailable asynchronous counts return `NOT_FOUND` without output mutation. It is not removed or C-deprecated. |
| exactness, presentation, camera receipt, sort status, and order submission getters | P/D | Product status and qualification receipts. Retain. |
| projected submission, success poll/count take, and failure poll | C/D | Compatibility/qualification lane. Retain while consumers remain. |
| producer selection plus independent producer measurement enable/submission/success/failure | C/D/Q | Published ABI. Exact producer selection and independent measurement are distinct controls; current platform qualification limits remain explicit. |
| CPU/GPU order success polls, count take, and failure poll | C/D | Current compatibility/qualification ABI. Retain while consumers remain. |

Source-only Rust exports, absent from the public header by design:

| Symbol | Class | Scope |
| --- | --- | --- |
| `gsplat_android_benchmark_set_order_backend` | C | Old Android benchmark compatibility alias. |
| `gsplat_benchmark_set_surface_order_backend` | C | Old mobile qualification alias. |
| `gsplat_benchmark_set_surface_camera_trace_frame` | D | Example benchmark hook. |
| `gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy` | D | Android/iOS example trace hook, not SDK API. |

### C/JNI/Kotlin producer route versus Web

`gsplat_surface_renderer_set_gpu_order_producer_v1` is the C ABI entry. JNI
`Java_com_gsplat_android_NativeBridge_setSurfaceGpuOrderProducerV1` calls it;
Kotlin `NativeBridge.setSurfaceGpuOrderProducerV1` is consumed by
`GsplatSurfaceRenderer.configure` / `setGpuProducerDiagnostics`. That is one
C -> JNI -> Kotlin compatibility/diagnostic route.

Web is independent. Raw WASM `setGpuOrderProducerAsync` calls
`SurfaceRenderSession::set_gpu_order_producer_async` directly, and the package
method calls that raw WASM method. It does not call or translate
`gsplat_surface_renderer_set_gpu_order_producer_v1`, JNI, or Kotlin. Exact Web
producer evidence uses renderer-owned current-stats terminals; the legacy
producer-specific learner/terminal stream stays disabled.

## Android JNI and Kotlin ledger

Declarations are in
[`NativeBridge.kt`](../../../../bindings/android/gsplat-android/src/main/kotlin/com/gsplat/android/NativeBridge.kt),
JNI implementations in
[`gsplat_jni.c`](../../../../bindings/android/jni/gsplat_jni.c), and the typed
wrapper in
[`GsplatSurfaceRenderer.kt`](../../../../bindings/android/gsplat-android/src/main/kotlin/com/gsplat/android/GsplatSurfaceRenderer.kt).

| Declaration group | Class | Actual scope |
| --- | --- | --- |
| version/error helpers and `runFfiSmoke` | P/D | Library utilities plus host smoke. |
| default / explicit-geometry create JNI methods | C / P | Typed wrapper uses explicit geometry; Packed is the default option. |
| resize, camera, render, destroy, sort/order/projected/async/frame-latency controls | P/D | Typed product lifecycle and diagnostics. |
| low-level `setSurfaceGeometryPath` | C | JNI compatibility method; no typed live geometry setter. |
| current-stats request/submission/poll and receipt pumps | P/D | Typed strict current-stats adapter and finite terminal progress. |
| `getSurfaceStats` and Kotlin `stats()` | C | Current compatibility call chain, not deprecated in Kotlin and not valid strict evidence. It must not be labeled `R`. |
| exactness/presentation/status/submission and projected/order terminal methods | P/D | Status and qualification receipts. |
| GPU producer selection and producer measurement methods/DTOs | C/D/Q | C/JNI/Kotlin route described above. Retained schema/compatibility is not a Web route and does not itself prove device qualification. |

There is no JNI/Kotlin raster-plan or Tiled selector, and there is no typed
Kotlin runtime geometry setter. Android strict examples use current-stats,
not `stats()`, for live `S/V/C/D`.

## Swift ledger

Public declarations are in
[`GsplatKit.swift`](../../../../bindings/apple/GsplatKit/Sources/GsplatKit/GsplatKit.swift)
and
[`CurrentStats.swift`](../../../../bindings/apple/GsplatKit/Sources/GsplatKit/CurrentStats.swift).

| Declaration group | Class | Actual scope |
| --- | --- | --- |
| `GsplatContextRenderer` construction/camera/load/render/stats | P/C | Public offscreen context wrapper; its `stats()` is not deprecated. |
| `GsplatUIKitSurfaceRenderer.init(... options:)` and `GsplatSurfaceOptions.geometryPath` | P/D | Calls the explicit-path UIKit C constructor; default is Packed. No public live geometry setter. |
| close/resize/camera/render | P | Surface lifecycle. |
| Surface `stats()` | C, Swift-deprecated | Line 1107 has `@available(*, deprecated, ...)`. It still calls the current C compatibility getter and is not removed. |
| current-stats request/submission/poll and consumer DTOs | P/D | Authoritative live/strict Surface counts. |
| exactness/presentation/order/projected status, setters, and drains | P/D | Product status plus qualification diagnostics. |
| example `_silgen_name` trace hook | D | Example-only qualification hook, not GsplatKit product API. |

There is no Swift raster/Tiled selector, live geometry setter, or GPU-producer
selection/measurement wrapper at this baseline.

## Raw WASM and package JavaScript/TypeScript ledger

Raw exports are defined in
[`crates/gsplat-web/src/wasm.rs`](../../../../crates/gsplat-web/src/wasm.rs).
The package exports and declarations are in
[`packages/web/src/index.js`](../../../../packages/web/src/index.js) and
[`index.d.ts`](../../../../packages/web/src/index.d.ts).

| Layer / exact name | Class | Actual contract |
| --- | --- | --- |
| raw `api_version_major`, `api_version_minor` | P/C | wasm-bindgen version functions. Package `getGsplatApiVersion` calls these snake-case names. There are no raw `apiVersionMajor` / `apiVersionMinor` exports. |
| package `initGsplatWeb` | P | Loads the wasm-bindgen module. There is no package `initGsplat`. |
| raw `createRenderer`, `createRendererWithGeometryPath` | P/C | In-memory construction. Raw compatibility `createRenderer` defaults to Packed; package construction uses the explicit-path raw export. |
| package `createGsplatRenderer`, `createGsplatRendererFromUrl`, `createGsplatRendererFromStream` | P | Bytes, URL, and readable-stream package constructors. There are no package `createRendererFromBytes` / `createRendererFromUrl` exports. |
| raw `createPackedPlyStream` / `PackedPlyStream` | P | Allocation-bounded Packed transport builder consumed by the package stream/URL path. |
| raw synchronous `resize` | C | Compatibility entry. Same-size is allowed; changed size fails closed and requires the async transaction. |
| raw `resizeAsync` | P | Transactional WebGPU Surface resize. |
| package `GsplatWebRenderer.resize(width,height): Promise<void>` | P | Package Promise API; it checks size and calls raw `resizeAsync`. The package does not expose a `resizeAsync` method. |
| raw/package geometry setters | C | Same-path compatibility only; changed paths fail closed. |
| raw/package order/projected controls and frame/current-stats receipt methods | P/C/D | Translation over shared session ownership; JS does not own plan generations or terminal state. Pending indirect V/D remains `null`. |
| raw/package `setGpuOrderProducerAsync` | D | Independent Web route directly into the Rust session, not the C/JNI/Kotlin symbol chain. |
| raw `rasterPath`, frame `rasterExecutionPlan`, package read-only translation | P/D | Observation only. Current serialized values are `global_quads` or `projected_quads_exact`; no Tiled value or setter remains. |

`npm --prefix packages/web test` runs Node's test runner against constructed
mock native-renderer objects in `packages/web/test/index.test.js`. It is useful
wrapper contract evidence, but it is **not** a real browser, wasm-bindgen,
WebGPU, GPU, Surface, or pixel test. Likewise,
`cargo check -p gsplat-web --target wasm32-unknown-unknown` is compile evidence,
not browser/WebGPU runtime evidence.

## Remaining M7 decisions and residual boundaries

1. Decide whether the public low-level standalone presenter/session
   composition and legacy Packed Global/Projected graph are retained as a named
   compatibility diagnostic or removed/restricted through a separate API
   decision.
2. Preserve all still-published C ABI symbols as M3-owned thin shims unless a
   separately reviewed compatibility decision authorizes removal. Compatibility
   no-ops and fail-closed stats are current symbols, not removed entries.
3. Treat TiledExact as already deleted. Do not revive it through the retained
   two-plan raster setter or stale README/handbook prose.
4. Keep the C/JNI/Kotlin producer route distinct from Web's raw WASM -> Rust
   session route; neither route may be used as evidence for the other.
5. Run the complete workspace, platform, architecture, FFI, WASM, forced-Metal,
   rollback, and fixed-SHA review matrix on the final M7 deletion candidate.
   The focused checks below qualify only this documentation ledger.

## Fresh verification at `707cd80`

All commands below are literal repository-root shell commands; no angle-bracket
placeholder is required.

| Command | Result | Evidence scope |
| --- | --- | --- |
| `git status --short --branch && git rev-parse HEAD HEAD^` | PASS; detached clean baseline `707cd80...`, parent `a798b8a...` before the ledger edit | Source identity only. |
| `git grep -n -E 'TiledExact\|tiledPreparationPending\|ResidentTiledError\|tiled_entry_(count\|capacity)' 707cd80 -- crates bindings examples packages tools tests ':!crates/gsplat-render-wgpu/README.md' ':!examples/desktop/README.md'; test $? -eq 1` | PASS; no product/binding/test source identifier remains | Static removal check. Historical docs are deliberately excluded and classified above. |
| `cargo test -p gsplat-render-wgpu exact_packed_session_construction_skips_the_legacy_presenter_graph` | PASS: 1 passed, 467 filtered | Focused owner-route test only. |
| `cargo test -p gsplat-render-wgpu execution_identities_remain_available_at_the_crate_root` | PASS: 1 passed, 467 filtered | Current crate-root execution/raster identity compilation. |
| `cargo test -p desktop-example args_parse_rejects_retired_surface_raster_plan` | PASS: 1 passed, 16 filtered | Retired CLI rejection. |
| `npm --prefix packages/web run check` | PASS | JS/test syntax only. |
| `npm --prefix packages/web test` | PASS: 49 passed, 0 failed | Node mock-wrapper tests only; not browser/WebGPU evidence. |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | PASS | WASM-target compile only; not browser/WebGPU evidence. |
| `bash tests/ffi/run-ffi-smoke.sh` | PASS after the authorized task-local Cargo clean: `ffi smoke ok`; `drawn=2 visible=2` | C header/source ABI functional smoke. The first attempt stopped at linker `errno=28` before the clean and produced no smoke result. |
| `bash bindings/android/scripts/run-jni-smoke.sh` | **ENVIRONMENT UNAVAILABLE:** macOS reported `Unable to locate a Java Runtime`; no JNI smoke executed | JDK availability only; not Android bridge/device evidence. |
| `bash bindings/apple/scripts/run-swift-smoke.sh` | PASS after the authorized task-local Cargo clean: `swift smoke ok`; `drawn=2 visible=2` | Swift/C host smoke only. The first attempt stopped before the smoke because the device had no free space. |
| `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/test_source_architecture.py` | PASS: 3 tests | Architecture checker self-tests. |
| `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/check_source_architecture.py` | PASS: 123 production Rust, 17 WGSL, 5 grandfathered | Real-tree ownership policy. Two grandfather-growth notices are notices, not failures or completion evidence. |
| task-local Python Web-name audit shown below | PASS: `web API name audit: PASS` | Exact raw/package names and resize layering. |
| task-local Python C-symbol audit shown below | PASS: header 58, Rust 62, no header-only entry, exactly four classified source-only entries | Static ABI declaration/definition reconciliation. |
| task-local relative Markdown-link check shown below | PASS: `relative Markdown links: PASS` | This ledger's local file links only. |
| `git diff --no-index --check /dev/null docs/plans/active/2026-07-23-native-render-core-migration/m7-public-callsite-api-abi-ledger.md; test $? -eq 1` and `git diff --check` | PASS | New-file and tracked-patch hygiene. The committed-object check is reported in the handoff because a commit cannot contain its own result. |

The first C and Swift attempts were capacity-blocked. Under explicit root
authorization, the exact command
`cargo clean --target-dir /Users/misotofu/.codex/worktrees/c18c/gsplat-rs/target`
removed exactly 3,978 task-local rebuildable Cargo artifact files (Cargo
reported 1.3 GiB; `du` reported the directory as 1.1 GiB before removal). No
source, dataset, other worktree, or external cache was removed. Only the C and
Swift checks that had been capacity-blocked were rerun; the JDK-unavailable JNI
check was not repeated.

The reproducible Web-name audit is:

```bash
python3 - <<'PY'
from pathlib import Path
import re

raw = Path('crates/gsplat-web/src/wasm.rs').read_text()
js = Path('packages/web/src/index.js').read_text()
dts = Path('packages/web/src/index.d.ts').read_text()
required_raw = [
    r'pub fn api_version_major\(',
    r'pub fn api_version_minor\(',
    r'#\[wasm_bindgen\(js_name = createRenderer\)\]',
    r'#\[wasm_bindgen\(js_name = createRendererWithGeometryPath\)\]',
    r'pub fn resize\(',
    r'#\[wasm_bindgen\(js_name = resizeAsync\)\]',
]
required_package = [
    r'^export async function initGsplatWeb\(',
    r'^export function getGsplatApiVersion\(',
    r'^export async function createGsplatRenderer\(',
    r'^export async function createGsplatRendererFromUrl\(',
    r'^export async function createGsplatRendererFromStream\(',
    r'^  resize\(width, height\) \{',
    r'await nativeRenderer\.resizeAsync\(width, height\)',
]
for pattern in required_raw:
    assert re.search(pattern, raw, re.M), pattern
for pattern in required_package:
    assert re.search(pattern, js, re.M), pattern
for name in ['apiVersionMajor', 'apiVersionMinor']:
    assert name not in raw, name
for name in ['initGsplat', 'createRendererFromBytes', 'createRendererFromUrl']:
    assert not re.search(rf'^export (?:async )?function {name}\(', js, re.M), name
assert not re.search(r'^  resizeAsync\(', js, re.M)
assert not re.search(r'^  resizeAsync\(', dts, re.M)
print('web API name audit: PASS')
PY
```

The reproducible C-symbol audit is:

```bash
python3 - <<'PY'
from pathlib import Path
import re

header = Path('crates/gsplat-ffi-c/include/gsplat.h').read_text()
rust = Path('crates/gsplat-ffi-c/src/lib.rs').read_text()
header_symbols = set(re.findall(
    r'(?m)^[A-Za-z_][^;{}]*?\b(gsplat_[a-z0-9_]+)\s*\(',
    header,
))
rust_symbols = set(re.findall(
    r'(?m)^pub (?:unsafe )?extern "C" fn (gsplat_[a-z0-9_]+)\s*\(',
    rust,
))
expected_source_only = {
    'gsplat_android_benchmark_set_order_backend',
    'gsplat_benchmark_set_surface_camera_trace_frame',
    'gsplat_benchmark_set_surface_camera_trace_frame_with_display_policy',
    'gsplat_benchmark_set_surface_order_backend',
}
assert not header_symbols - rust_symbols
assert rust_symbols - header_symbols == expected_source_only
assert len(header_symbols) == 58
assert len(rust_symbols) == 62
print('C symbol audit: PASS header=58 rust=62 source_only=4')
PY
```

The relative-link check used after the final edit is:

```bash
python3 - <<'PY'
from pathlib import Path
import re

doc = Path('docs/plans/active/2026-07-23-native-render-core-migration/m7-public-callsite-api-abi-ledger.md')
missing = []
for target in re.findall(r'\[[^]]+\]\(([^)]+)\)', doc.read_text()):
    path = target.split('#', 1)[0]
    if not path or path.startswith(('http://', 'https://')):
        continue
    resolved = (doc.parent / path).resolve()
    if not resolved.exists():
        missing.append((target, str(resolved)))
assert not missing, missing
print('relative Markdown links: PASS')
PY
```

No focused result above is a browser run, WebGPU execution, Android device
run, iOS simulator/device run, pixel comparison, performance result, or full
M7 acceptance matrix. Those remain explicit residual boundaries.
