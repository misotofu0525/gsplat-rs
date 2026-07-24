# Exact Core Migration Oracle

> Status: frozen E0 oracle
> Baseline: `7848912803f9601fb598dcca07b532e9d9893407`
> Contract: [contract.md](contract.md)
> Parent verification protocol:
> [benchmark_protocol.md](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md)

## 1. How to use this oracle

This oracle maps inherited proof to the target owner and the E task that must
preserve or re-establish it. “Existing source” means evidence available at the
fixed baseline; it does not mean the future owner already exists. E tasks reuse
accepted proof when ownership-only movement leaves the tested algorithm and
artifact identity unchanged, and add focused shadow parity where the new
boundary itself needs proof.

Hard correctness can reject a candidate. Performance observations can decide
whether a performance hypothesis is Accepted or Rejected and which plan is
eligible on a tested endpoint, but cannot weaken correctness or keep a task
open until a target number is reached.

## 2. Hard-correctness migration matrix

| Invariant | Target owner | Existing evidence source at the baseline | E1--E13 migration gate |
| --- | --- | --- | --- |
| Complete point set: `source = decoded = encoded = resident = addressable` | `SceneRuntime` owns membership; `PreparedRuntime` binds it to every admitted plan; `Renderer` publishes the count receipt | [full-quality design](../../completed/2026-07-22-full-quality-native-rendering/design.md), [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md), [`scene/tests/encoding.rs`](../../../../crates/gsplat-render-wgpu/src/scene/tests/encoding.rs), [artifact contract](../../../../tests/perf/benchmark-artifact-v1.md) | E1 carries immutable scene/count identity through CPU PostSort; E2 shadow-compares complete counts; E8/E9 prove their complete plans use the same scene; E12 joins target receipts; E13 rejects any missing parity. |
| Complete source SH0--SH3 and coherent view-dependent color | `SceneRuntime` owns encoded degree/planes; shared GPU color leaf executes; plans may not select or lower degree | [full-quality design](../../completed/2026-07-22-full-quality-native-rendering/design.md), [`scene/tests/encoding.rs`](../../../../crates/gsplat-render-wgpu/src/scene/tests/encoding.rs), [`gpu/color.rs`](../../../../crates/gsplat-render-wgpu/src/gpu/color.rs), [Direct/Resident final evidence](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) | E1 freezes degree in the runtime contract; E2 covers SH0--SH3 shadow images; E8/E9 reuse the same color owner; E12 proves Surface parity; E13 records no downgrade path. |
| Stable full32 depth order, inclusive visibility and deterministic source-ID ties | Each concrete plan owns its order mechanics; `ProjectedWork` carries authoritative owner/generation; CPU/GPU leaves own only kernels | [`gsplat-sort`](../../../../crates/gsplat-sort/src/lib.rs), [`cpu_order.rs`](../../../../crates/gsplat-render-wgpu/src/cpu_order.rs), [`gpu/radix.rs`](../../../../crates/gsplat-render-wgpu/src/gpu/radix.rs), [`direct_gpu_order.rs`](../../../../crates/gsplat-render-wgpu/src/direct_gpu_order.rs), [full-quality design](../../completed/2026-07-22-full-quality-native-rendering/design.md) | E2 defines common equal-depth/adversarial/tail contract tests; E3 proves one CPU engine/workspace; E4/E5 match Scalar element-for-element; E8 proves GPU PostSort; E9 proves Preproject contributor prefix; E11 compares only plans that passed the same contract. |
| `S/V/C/D`: `0 <= C <= V <= S`; Candidate/PostSort `D=V`; exact Compact/Preproject `D=C` | Concrete plan creates counts; `ProjectedWork` transports them; `Renderer` alone finalizes `FrameResult` and mandatory receipt values | [exact contributor evidence](../../completed/2026-07-22-full-quality-native-rendering/exact-contributor-evidence.md), [Preproject architecture](../../completed/2026-07-22-full-quality-native-rendering/phase2-preproject-c-architecture.md), [artifact vocabulary](../../../../tests/perf/benchmark-artifact-v1.md), [`gpu_telemetry.rs`](../../../../crates/gsplat-render-wgpu/src/gpu_telemetry.rs) | E1 defines explicit availability/count source; E2 compares legacy and shadow receipts; E8 retains `D=V`/exact indirect identity; E9 retains `D=C<=V`; E11 rejects incomparable samples; E12 binds counts to terminal Surface frames. |
| Camera identity, complete camera guard and exact backing resolution | `FrameState` owns camera/viewport revisions; plan-local cache stores the guard; target host owns acquire/configure/present primitives | [full-quality experiment contract](../../../../tests/perf/full-quality-experiment-v1.md), [matrix plan](../../../../tests/perf/full-quality-matrix-plan-v1.json), [`surface/configuration.rs`](../../../../crates/gsplat-render-wgpu/src/surface/configuration.rs), [`surface_presenter.rs`](../../../../crates/gsplat-render-wgpu/src/surface_presenter.rs) | E1 creates authoritative generations; E2 exercises camera/viewport invalidation offscreen; E8/E9 bind caches to owner/generation/count; E11 tags samples; E12 proves requested/Surface/internal/presented identity and no generation advance on failed acquire; E13 records remaining endpoint gaps honestly. |
| Per-plan cache reuse is exact and fail-closed | Each prepared concrete plan owns its cache; `FrameState` owns semantic generations; `Renderer` owns invalidation and plan-set replacement | [`surface_presenter.rs`](../../../../crates/gsplat-render-wgpu/src/surface_presenter.rs) projected-cache and preparation tests, [`resident_gpu.rs`](../../../../crates/gsplat-render-wgpu/src/resident_gpu.rs) lazy-publication tests, [parent architecture](../../completed/2026-07-23-native-render-core-refactor/architecture.md) | E1 tests complete guard and transactional CPU-plan publication; E2 tests stale camera/viewport/order/count rejection; E8/E9 add plan-local guards; E11 proves switching plans preserves valid competitor caches without cross-generation sampling; E12 covers resize/acquire failure. |
| Transactional prepare/publish and same-Exact fallback | `Renderer` prepares/publishes the whole `PreparedRuntime`; `PlanSet` proves non-empty eligibility and present fallback | [`resident_gpu.rs`](../../../../crates/gsplat-render-wgpu/src/resident_gpu.rs), [`surface_presenter.rs`](../../../../crates/gsplat-render-wgpu/src/surface_presenter.rs), [`surface/configuration.rs`](../../../../crates/gsplat-render-wgpu/src/surface/configuration.rs), [Package A report](../../completed/2026-07-23-native-render-core-refactor/final-report.md) | E1 is the primary transaction/fallback gate; E2 injects failed preparation and confirms the old runtime remains usable; E8/E9 fail closed on optional GPU graph creation; E11 cools down failed challengers without quality change; E12 covers Surface transaction boundaries. |
| ProjectedQuadsExact canonical work and raster math | Plans return one `ProjectedWork`; `CanonicalRaster` owns the accepted projected-quad pipeline/encode; only `Renderer` invokes it | [`projected_quads_gpu.rs`](../../../../crates/gsplat-render-wgpu/src/projected_quads_gpu.rs), [`raster/`](../../../../crates/gsplat-render-wgpu/src/raster), [full-quality findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md), [SortedAlpha conformance](../../../../crates/gsplat-render-wgpu/tests/conformance_sorted_alpha.rs) | E1 establishes the canonical handoff through CPU PostSort; E2 compares shadow images; E8/E9 emit the same handoff; E10 migrates the already accepted four-vertex topology before testing its one isolated hypothesis; E13 requires no known image issue. |
| Direct f32 and GlobalQuads remain reference oracles, not adaptive product plans | Existing legacy oracle path/test harness; new core consumes its comparisons but does not admit it into `PlanSet` | [full-quality design](../../completed/2026-07-22-full-quality-native-rendering/design.md), [full-quality final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md), [`conformance_sorted_alpha.rs`](../../../../crates/gsplat-render-wgpu/tests/conformance_sorted_alpha.rs), [parent roadmap](../../completed/2026-07-23-native-render-core-refactor/task_plan.md) | E2 owns the shadow offscreen Direct/Global image oracle; E8/E9 must pass it; E10 preserves raster math; E13 records parity without adding oracle IDs to the controller. TiledExact stays diagnostic unless a later complete-plan task separately qualifies it. |
| Surface and offscreen execute the same renderer core | Surface/offscreen hosts own lifecycle leaves; `Renderer` owns execution, plan selection, generations, result and mandatory samples | [`offscreen/`](../../../../crates/gsplat-render-wgpu/src/offscreen), [`surface/`](../../../../crates/gsplat-render-wgpu/src/surface), [Package A report](../../completed/2026-07-23-native-render-core-refactor/final-report.md), [verification handbook](../../../../handbook/VERIFICATION.md) | E2 establishes shadow offscreen parity; E12 establishes explicit shadow Surface CPU/GPU/Adaptive parity without cutover; E13 checks no second scheduler/controller. Product migration remains M1/M2. |
| `FrameResult`, terminal completion and mandatory plan samples have one producer and generation identity | `Renderer` joins plan outcome, completion and primitive target outcome; `evidence/` owns immutable types; `PlanSampler` is mandatory; `EvidenceRing` is optional | [`evidence/`](../../../../crates/gsplat-render-wgpu/src/evidence), [`gpu_telemetry.rs`](../../../../crates/gsplat-render-wgpu/src/gpu_telemetry.rs), [`projected_draw_telemetry.rs`](../../../../crates/gsplat-render-wgpu/src/projected_draw_telemetry.rs), [artifact contract](../../../../tests/perf/benchmark-artifact-v1.md) | E1 defines the small result/count identity; E2 proves one result per shadow frame; E8/E9 preserve ticket/count source; E11 installs mandatory sampler and sole controller, including stale/expired samples; E12 binds actual presentation; E13 audits no duplicate receipt ring or controller. |
| Existing CPU/GPU Adaptive behavior remains measured, never point-threshold selected | Final `WholePlanController` under `Renderer`; mandatory sampler compares complete `PlanId`s; CPU calibration is bounded plan-internal initialization | [`surface_session.rs`](../../../../crates/gsplat-render-wgpu/src/surface_session.rs), [full-quality final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md), [parent benchmark protocol](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md) | E3 preserves CPU execution semantics; E7 freezes accepted CPU kernel/chunk choice; E8/E9 supply complete GPU competitors; E11 proves learning/hysteresis/cooldown/re-probe on comparable terminal intervals; E12 runs the shadow Surface route; E13 closes only with no second controller. |
| Public C ABI, wasm boundary and platform hosts do not gain policy or regress | Existing stable C/header and platform wrappers remain owners until Package M; E shadow route stays private/test-only | [`gsplat.h`](../../../../crates/gsplat-ffi-c/include/gsplat.h), [`gsplat-ffi-c/src/lib.rs`](../../../../crates/gsplat-ffi-c/src/lib.rs), [`gsplat-web`](../../../../crates/gsplat-web), [FFI smoke](../../../../tests/ffi/run-ffi-smoke.sh), [verification handbook](../../../../handbook/VERIFICATION.md), [Package A final matrix](../../completed/2026-07-23-native-render-core-refactor/final-report.md) | E1--E11 make no public/host cutover; E12 uses explicit shadow routing and relevant native/Web compile or smoke evidence; E13 reports the exact platform evidence available. C/Web/Android/Apple product cutovers remain M3--M6 and cannot be claimed by E. |

## 3. E-task proof routing

| Task | New proof owned by the task | Inherited proof it must not rerun or overclaim |
| --- | --- | --- |
| E1 | Whole-runtime transaction, non-empty fail-closed `PlanSet`, CPU PostSort fallback, canonical `ProjectedWork`, complete generation/result identity | Does not re-qualify platform performance or change the product default. |
| E2 | Common plan-contract tests and shadow offscreen membership/SH/order/count/image/state parity | Reuses accepted Direct/Global/Projected raster gates; does not reopen their shader math. |
| E3 | One sync/async/offscreen CPU engine and reusable workspace, plus narrow architecture-leaf interface | Preserves existing scalar/Rayon/SIMD semantics; makes no new SIMD win claim. |
| E4 | AArch64 NEON element parity and scoped end-to-end admission on available ARM endpoints | A missing endpoint narrows or defers the claim; no global CPU conclusion. |
| E5 | x86_64 AVX2/FMA element parity and scoped admission when an x86 endpoint exists | Endpoint absence is Deferred, not emulated performance evidence and not a blocker for ARM/Web. |
| E6 | One isolated fused preprocess-to-radix hypothesis, ending Accepted or Rejected | Does not modify correctness or trigger repeated tuning when the result loses. |
| E7 | Bounded initialization calibration among already accepted CPU candidates | Does not become an ongoing controller or fixed point-count rule. |
| E8 | Existing exact GPU PostSort mechanics as one prepared complete plan | Does not rerun accepted full32/raster experiments merely because ownership moved. |
| E9 | Existing exact GPU Preproject/Compact mechanics as one prepared complete plan with no separate producer policy | Historical directionality is not final controller evidence or a default-change decision. |
| E10 | Accepted raster topology migration, then one separately declared submission/batching hypothesis | Four-vertex acceptance is inherited; only the new hypothesis receives performance iteration. |
| E11 | Mandatory comparable terminal sampler and sole whole-plan controller across CPU PostSort, GPU PostSort and GPU Preproject | Does not compare producer/sort/raster axes independently and encodes no point-count threshold. |
| E12 | Explicit shadow Surface parity for complete plans, generations, lifecycle and terminal receipts | Does not switch product consumers or move host policy into platform wrappers. |
| E13 | Fixed-SHA package audit, no known correctness issue, parity report and honest evidence gaps | Does not manufacture a full device matrix or performance conclusion from historical runs. |

## 4. Performance observations and prohibited synthesis

The following records may guide hypotheses but are not hard E acceptance gates:

- historical A065 CPU/PostSort, GPU/Preproject and PlayCanvas timings in the
  [parent task plan](../../completed/2026-07-23-native-render-core-refactor/task_plan.md);
- same-binary PostSort/Preproject directional cohorts in the
  [full-quality final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md);
- short Garden/Bicycle runs, which are capacity evidence only;
- 640x360/640x480 results, which are diagnostic only;
- iOS simulator runs, which are functional/conformance evidence only;
- the checked-in [matrix plan](../../../../tests/perf/full-quality-matrix-plan-v1.json),
  which is a schema-valid plan rather than a completed final-binary matrix.

Do not combine timings from different commits, binaries, resolutions, cameras,
timing intervals or evidence classes into a final conclusion. In particular,
E8/E9 ownership consolidation cannot inherit a winner, E11 must compare fresh
same-contract terminal samples for its actual prepared plans, and Package Q
owns later competitor/native-advantage qualification.

No fixed FPS, fixed percentage lead, 800/890-line rule or any other physical
LOC value is part of this oracle. A mixed owner receives a bounded
responsibility review only when cohesion, dependency direction, change
locality, navigation or test seams show a maintenance problem; line count alone
cannot trigger it. Correctness, ownership, compatibility and those semantic
boundaries decide acceptance.

## 5. E13 closeout checklist

E13 can close the shadow-core package only if the integrated result proves:

- all three supported complete plans satisfy one Exact contract;
- unsupported plans are absent from eligibility and fallback remains Exact;
- membership, SH0--SH3, stable full32/ties, `S/V/C/D`, camera and resolution
  receipts remain bound to the terminal frame;
- Direct/Global and canonical ProjectedQuads image oracles pass in their tested
  scope;
- Surface and offscreen share one renderer owner;
- there is one controller, one semantic generation owner, one result producer
  and one mandatory sampling path;
- optional evidence pressure cannot alter execution;
- public defaults and platform consumer policy remain unchanged;
- every missing endpoint or performance claim is labelled unavailable,
  directional, capacity-only or deferred rather than inferred.
