# Qualification Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
Q0 = Accepted
Q2 = Accepted
<!-- gsplat-program-task-states: end -->

## Scope

Package Q begins only after M8. Q0 freezes fair comparator identity, datasets,
camera traces, resolution receipts, timing semantics and endpoint schedule.
It keeps Exact, Balanced and Scalable claims separate and does not use a
browser, simulator, compile-only route or unmatched asset as proof of native
or competitor performance.

The active writer owns only this ledger and [Q0 contract](q0-contract.md). It
may not modify the renderer, benchmark producers, comparator harness, product
defaults, or the Balanced and Scalable packages.

## Candidate checkpoint

- Branch: `codex/q0-qualification-contract`
- Exact baseline: `6d5bd5442dee31cea24906744dfdd1d7492095ae`
- State: Q0 Accepted after independent review and root integration (`dd6af91`,
  `b195055`).
- Machine-state vocabulary: Q0--Q4 use only Accepted/Rejected/Deferred. If B6
  is Rejected, Q2 finishes Accepted after recording the finite decision; only
  its report may describe the Balanced comparison as `not_applicable`.
- Scope: comparator identity/launch, common workload admission, terminal timing,
  endpoint schedule, artifact receipts and finite Accepted/Rejected/Deferred
  outcomes are frozen in [q0-contract.md](q0-contract.md).
- Execution: no native, browser or device product benchmark was run by Q0.
- Next: Q1--Q3 start only from the accepted contract and their own declared
  prerequisites.

## Root-owned A065 formal functional artifact (2026-07-27)

One authorized physical-device run at root SHA
`95d2ccdc77adaf9bce9b0733a851939dc161fd9f` completed and was independently
validated with `validate-full-quality-experiment.py --verify-inputs`.

- Endpoint: Nothing A065 / Snapdragon SM8475 / Android 15 / Vulkan; thermal
  status before and after the run was `0`.
- Workload: complete SH3 Kitsune (`279,199` source, decoded and resident
  splats), Packed `SortedAlpha` with CPU ordering, `2412x1080`, both frozen
  trace views, 10 warmup and 20 measured frames.
- Evidence: repository-local formal suite
  `target/android-sort-benchmarks/verification-a065-95d2ccdc77ad/`, including
  the native-Surface PNG, receipt, logcat, per-run artifact and immutable
  input identities. The full-quality validator reported one expected and one
  rendered run, with no capacity rejection or missing artifact.
- Diagnostic only: the retained run reports `avg_call_ms=7.236`,
  `avg_frame_ms=7.859`, CPU preprocess `0.769 ms` and CPU sort `2.566 ms`.
  It is a 20-frame single-policy functional ledger, not a CPU/GPU comparison,
  competitor comparison, or release-performance claim.

This establishes A065 native functional evidence for its exact scope. It does
not accept Q1--Q3, qualify the Balanced candidate lanes, or substitute for the
separately frozen matched-comparator protocol.

## Root-owned M4 Balanced image-gate observations (2026-07-27)

The M4 Metal B1/B2/B3 suites at commit
`edbc656e04befd589b8e425f0874f98aebb7333d` passed their formal image gate for
complete SH3 Kitsune at `1920x1080`, including the frozen moving trace. This is
candidate image-integrity evidence, not a matched performance comparison and
does not advance Q1--Q3. Timing/performance claims remain unavailable until
the Q0 paired comparator protocol has retained matching native and comparator
artifacts.

The newly authorized Chrome/WebGPU attempt must also remain **Deferred**: it
exposed a ticket-namespace defect after the collector observed its initial
`ready` object. The repair at `a2437d4` has local verification only; no
post-repair browser endpoint evidence exists yet.

## Q2 finite dependency result (2026-07-27)

Balanced B6 is terminal **Rejected** after B1--B3 produced no Accepted
component, leaving no legal B4 combination or B5 product policy. Under the
predeclared Q0 dependency table, Q2 is therefore machine-state **Accepted**
with report result `product_comparison=not_applicable`. No gsplat-rs Balanced
product plan exists to compare with PlayCanvas, so launching or fabricating a
matched product-throughput run would be misleading. This outcome is not a
performance win; it is the finite truthful completion path for a rejected B6.

## Q1 M4 PlayCanvas prerequisite (2026-07-27)

At clean root commit `cab9c9e`, the pinned PlayCanvas `2.21.0-beta.14`
(`d5fe88878e338936fe763bbce1a58bc315e89cbe`) harness completed one fresh
Chrome/WebGPU prerequisite run over the full 2,541,226-splat Truck SH3 source.
The run used the canonical two-view moving trace, `1920x1080` backing/internal
resolution, 20 warmup and 80 measured frames, GPU sort on every forward frame,
and disabled LOD, sampling, dynamic resolution and upscaling. Source, decoded
and resident counts all equal `2,541,226`; both trace indices were measured.

The canonical benchmark validator accepted
`tests/competitive/playcanvas/target/benchmarks/qualification/`
`playcanvas-truck-1080p-cab9c9e-prereq-attempt2/`. Its stopped-rAF plus terminal
WebGPU queue-drain receipt reports frame-wall mean `17.6675 ms`, p95 `33.3 ms`,
48 of 80 frames over the 16.67 ms budget, terminal mean `18.7625 ms` and
`53.2978 FPS`. Browser `frameupdate`-to-`frameend` CPU time is separately
reported as `1.8 ms`; no unavailable GPU phase timing is invented.

This is a comparator prerequisite, not Q1 acceptance or a native-performance
loss. A desktop browser page cannot prove physical presentation dimensions;
the artifact truthfully leaves presented width/height unavailable and
`full_resolution=false` despite proving the full internal backing. Q1 still
needs the matched native Exact run and the scoped external-presentation/
quality decision before comparing terminal throughput.

## Q3 M4 SIMD microbenchmark checkpoint (2026-07-27)

At clean root commit `f025dff`, the fixed
`Q3.M4.PackedCpuExact.ScalarVsNeon` cell completed once and published the
fresh ignored artifact
`target/benchmarks/qualification/q3-m4-simd-f025dff.json`. Its fixed
200,003-item packed input and 11 interleaved sample pairs preserve exact key,
source-id, NaN-bit, boundary-bit, FMA-derived-key and stable-tie parity between
the forced Scalar and NEON radix-count/unpack leaves.

The observed medians were `849,833 ns` for Scalar and `819,417 ns` for NEON.
This is useful evidence that the native SIMD leaf is both correct and worth
carrying into the whole-plan experiment, but the cell remains **Deferred** by
construction: it is a microbenchmark only, does not include renderer-owned
preprocess, sort, upload, draw or terminal queue completion, and cannot select
the product default. Q3 next requires a private renderer qualification selector
and matched native terminal artifacts before making any whole-plan decision.
