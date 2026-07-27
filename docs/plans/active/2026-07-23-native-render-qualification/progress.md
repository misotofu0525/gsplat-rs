# Qualification Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
Q0 = Accepted
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
