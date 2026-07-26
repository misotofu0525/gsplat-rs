# Scalable Runtime Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
S0 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 63678fcdd5d26973ec21427071ea9cea20bc5b24
S0 = scalable-contract
<!-- gsplat-program-active-lanes: end -->

## Scope

Package S begins only after M8 and is a separate Streamed semantic contract,
not a promotion of the historical four-slot Paged diagnostic. S0 freezes
coverage, quality, byte-budget and source-format evidence before any runtime
work. Parent coverage must remain drawable until ready children replace it
atomically; random source-point sampling is not a Scalable solution.

The active writer owns only this ledger and [S0 contract](s0-contract.md). It
may not change resident renderer code, product defaults, or the Balanced and
qualification packages.
