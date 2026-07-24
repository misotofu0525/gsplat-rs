# Native Render Core Migration Progress

> Program roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
>
> Accepted shadow-core package:
> [Package E final report](../../completed/2026-07-23-native-render-exact-core/final-report.md)

## Machine task-state registry

The architecture checker records only Active or terminal tasks in this block.
Unstarted tasks remain outside the machine registry until activated.

<!-- gsplat-program-task-states: begin -->
M0 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: M — atomic product migration and legacy deletion.
- Dependency: E13 Accepted at Exact implementation tip
  `d721ea6cd0c334e28d3ad5c28792383524e27935`.
- Active task: M0, a documentation/checklist task only.
- Product default: unchanged legacy renderer and `SurfaceRenderSession`.
- Unstarted tasks: M1, M2, M3, M4, M5, M6, M7 and M8. They are pending in
  roadmap order and are not active machine-state entries.
- No product consumer, public API, C ABI, wrapper or default may switch during
  M0.

## M0 activation contract

- Objective: freeze the exact cutover boundary before implementation by
  writing one reviewable cutover/rollback checklist and one exact artifact set.
- Scope: this migration plan bundle and existing factual handbook links only.
  M0 changes no Rust, WGSL, platform wrapper, build script, benchmark protocol
  or product route.
- Required cutover checklist:
  - identify the accepted Package E implementation and the exact rollback
    commit;
  - name the consumer and mutable-responsibility owner for M1--M7;
  - define the one-way integration order and the rollback point after each
    consumer migration;
  - preserve complete membership, SH0--SH3, stable full32 ordering, exact
    resolution, canonical `SortedAlpha` and same-Exact fallback;
  - forbid simultaneous legacy/new ownership of semantic generations,
    controller decisions, mandatory sampling or terminal results after each
    corresponding cutover;
  - keep public/API/ABI changes out of M1/M2 and isolate compatibility work to
    M3 and later consumer tasks;
  - distinguish real-window/platform evidence from compilation, simulator and
    injected-presentation evidence.
- Required artifact set:
  - fixed dataset, camera, resolution and image/count receipts inherited from
    Package E;
  - one native offscreen parity artifact for M1;
  - one real-window shared Surface transaction artifact for M2;
  - FFI/header/JNI/Swift compatibility evidence for M3;
  - browser, Android A065 and Apple consumer evidence only in M4--M6;
  - architecture and legacy-owner deletion proof for M7;
  - handbook/release alignment and final clean report for M8.
- Exit condition: the checklist names an exact baseline, rollback identity,
  task ownership, minimum verification and artifact acceptance for M1/M2,
  without implementing either cutover. M1 may activate only after M0 is
  Accepted.
- Performance boundary: M0 freezes correctness and rollback evidence. It runs
  no FPS, PlayCanvas or cross-device winner experiment.
