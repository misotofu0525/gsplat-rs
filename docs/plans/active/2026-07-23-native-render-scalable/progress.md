# Scalable Runtime Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
> Design candidate: [S0 coverage and streaming contract](s0-contract.md)

<!-- gsplat-program-task-states: begin -->
S0 = Accepted
S1 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 3ecf0f2d0180c197faa132443066b3b9b98d36d4
S1 = authored-proxy-builder
<!-- gsplat-program-active-lanes: end -->

## Scope

Package S begins after M8 as a separate Streamed semantic contract. It is not a
promotion of the historical four-slot Paged diagnostic and cannot begin by
renaming or wrapping that runtime. S0 owns design and research only; it changes
no renderer, format API, product default or qualification policy.

The selected design is an offline-authored replacement hierarchy with a
bounded versioned manifest and independently addressable immutable pages.
Source-leaf coverage stays complete through a drawable bootstrap cut. A local
refinement keeps its parent published until every direct child is validated,
decoded, uploaded, globally ordered, drawn and successfully presented. Each
child subtree can then refine independently, so valid global cuts may mix
depths, such as `{A1,A2,B}`. Random source sampling, incomplete sibling
publication, ancestor/descendant double coverage and per-page alpha sorting are
excluded.

## Current S0 candidate

- Metadata-first admission is specified without constructing or borrowing a
  complete `SceneBuffers`.
- Compressed/source, decoded CPU and GPU bytes have independent reservation,
  in-flight, current and peak accounting.
- Page/error terminals are generation-bound, while coverage, quality, memory
  and presentation-latency receipts publish only after successful presentation.
- PlayCanvas SOG, Spark RAD, OGC 3D Tiles, Hierarchical 3DGS and SPZ research is
  recorded with accepted and rejected/deferred mechanisms.
- Historical Paged remains a labelled diagnostic because it retains full
  source ownership and lacks authored replacement, three bounded caches and
  metadata-first I/O.
- S1 must freeze its assets, cameras, resolutions, required proxy cuts, numeric
  image thresholds and aggregation before work, then pass them to be Accepted.
  A geometric-only result may remain research but cannot unlock S2--S5.

S0 is Accepted after independent review and root integration (`435d04d`,
`f7164ed`). S1 is Active for a bounded offline builder/fixture slice. It still
requires its own predeclared, image-gated task contract and cannot make a
product claim merely by producing a structurally valid hierarchy.

## Ordered implementation ledger

| Task | State in this candidate | Independent result |
| --- | --- | --- |
| S0 | Accepted | coverage/budget/source/receipt contract and selected asset approach |
| S1 | Active: offline builder/fixture | predeclared image-gated, independently valid proxy hierarchy |
| S2 | Locked until S1 Accepted | metadata-first `PageSource` and bounded direct decode |
| S3 | Locked until S1 Accepted, then S2 | three byte-bounded caches and deterministic GPU page pool |
| S4 | Locked until S1 Accepted, then S3 | recursive mixed-depth cut selection and atomic local replacement |
| S5 | Locked until S1 Accepted, then S4 | one global active snapshot through shared CPU/GPU Adaptive plans |
| S6 | Not started | bounded platform memory/thermal/network feedback |
| S7 | Not started | endpoint-scoped quality-memory-latency qualification and closeout |

Each task has a finite Accept/Reject/Defer boundary in the S0 contract. A
failed performance hypothesis ends; it does not weaken coverage or trigger
unbounded tuning. Unavailable endpoint evidence is Deferred and limits the
claim rather than blocking unrelated implementation closure.

## Writer boundary

The S0 writer owns only this ledger and the linked contract. It may not change
resident renderer code, shaders, tests, public APIs, strategy/policy files,
product defaults, or the Balanced and qualification packages. Root review owns
integration, status transition and S1 activation.
