# Scalable Runtime Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
> Design candidate: [S0 coverage and streaming contract](s0-contract.md)

<!-- gsplat-program-task-states: begin -->
S0 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 63678fcdd5d26973ec21427071ea9cea20bc5b24
S0 = scalable-contract
<!-- gsplat-program-active-lanes: end -->

## Scope

Package S begins after M8 as a separate Streamed semantic contract. It is not a
promotion of the historical four-slot Paged diagnostic and cannot begin by
renaming or wrapping that runtime. S0 owns design and research only; it changes
no renderer, format API, product default or qualification policy.

The selected design is an offline-authored replacement hierarchy with a
bounded versioned manifest and independently addressable immutable pages.
Source-leaf coverage stays complete through a drawable bootstrap cut. A parent
remains published until its complete child group is validated, decoded,
uploaded, globally ordered, drawn and successfully presented. Random source
sampling, partial child publication and per-page alpha sorting are excluded.

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

This branch is only a review candidate. The machine state remains `S0 = Active`
until the root task accepts and integrates it; this writer does not activate
S1.

## Ordered implementation ledger

| Task | State in this candidate | Independent result |
| --- | --- | --- |
| S0 | Active, ready for root review | coverage/budget/source/receipt contract and selected asset approach |
| S1 | Not started | authored independently valid proxy hierarchy |
| S2 | Not started | metadata-first `PageSource` and bounded direct decode |
| S3 | Not started | three byte-bounded caches and deterministic GPU page pool |
| S4 | Not started | screen-error selection and atomic parent/child replacement |
| S5 | Not started | one global active snapshot through shared CPU/GPU Adaptive plans |
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
