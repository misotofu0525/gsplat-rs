# M7 S2 Session control extraction

Status: candidate implementation complete; M7 remains **Active**.

Base: `3ac7acd0f14a8d9a06fc2e6538e86601d712c08f`.

## Scope

This finite slice introduces the private `surface/session_control.rs` owner and
moves three control-plane responsibilities out of `surface_session.rs`:

- the exhaustive compatibility mapping from the public order, projected-draw,
  producer, raster, geometry and schedule controls to the closed Exact plan
  states `CpuPostSort`, `GpuPostSort`, `GpuPreproject` and `Adaptive`;
- validation and prepare-then-commit of forced Exact policy changes, including
  the rule that renderer rejection leaves every compatibility receipt intact;
- pure arbitration between the CPU/GPU order sampler and the independent
  Candidate/Compact projected-draw sampler, including cached-order admission,
  one-turn deferral and owner-boundary reset rules.

Every forced Exact state maps to one complete internal `PlanId`; Adaptive is a
renderer policy over the admitted set rather than an additional partial plan.
The mapping remains exhaustive, so adding a future `PlanId` cannot silently
fall through this compatibility layer.

## Preserved ownership

`SurfaceRenderSession` remains the public facade and keeps:

- construction and all public Rust entrypoints;
- concrete frame planning/execution and the one successful-present commit;
- telemetry polling, current-stats and compatibility-evidence publication;
- sort scheduling, native async worker state and pending frame choices;
- composition of the two Adaptive controllers and their runtime samples.

The pure rolling/pending policies remain in `surface/adaptive_order.rs` and
`surface/projected_adaptive.rs`. `SessionSurfaceOwner` remains in
`surface/session_owner.rs`. This slice changes no Renderer or GPU pass owner,
WGSL, resource layout, pass order, FFI/JNI/Swift/Web surface, public ABI, scene
membership, SH degree, resolution or pixel contract.

## Transaction and evidence invariants

- Exact switching first asks the renderer to accept the complete policy and
  updates the order/projected/plan compatibility receipts only after success.
- CPU-only Exact runtimes reject both GPU `PlanId`s without changing the
  current plan, prepared resources, published frame or compatibility fields.
- Projected formal evidence requires a cached authoritative order. A changed
  order may defer the projected choice but cannot manufacture a projected
  timing sample.
- Only one control axis owns a formal probe cohort at a time. Old terminal
  receipts remain diagnostic and cannot advance the other controller.
- Repeated setters remain idempotent; incompatible Preproject/Compact,
  geometry, raster and schedule combinations fail before mutation.

## Focused evidence

- `cargo test -p gsplat-render-wgpu surface::session_control::tests:: --lib`:
  PASS, closed policy/`PlanId` mapping and no-fake-sample arbitration.
- `cargo test -p gsplat-render-wgpu exact_surface --lib`: PASS, including
  CPU-only rejection snapshots and capable three-plan switching.
- `cargo test -p gsplat-render-wgpu projected_ --lib`: PASS, 24 focused
  projected controller, arbitration, telemetry and raster tests.

The final candidate additionally runs the crate/workspace, strict lint,
Rustdoc, WASM compile and architecture-policy checks recorded in the handoff.
No device, browser, package installation or performance experiment belongs to
S2. This document does not update aggregate progress, final M7 acceptance or
any grandfather record.
