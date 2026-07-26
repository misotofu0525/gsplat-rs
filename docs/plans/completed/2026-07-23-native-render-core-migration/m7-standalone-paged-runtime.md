# M7 standalone Paged runtime ownership slice

Status: candidate complete; M7 remains **Active**.

Base: `1548893`.

## Scope

This slice extracts the explicit diagnostic Paged compatibility lane from
`surface_presenter.rs` into the private
`surface/standalone_paged_runtime.rs` owner. The new owner contains:

- the Paged bind-group layout and render pipeline;
- the active four-slot `PagedActiveSet` and Paged-only instance count;
- synchronous page selection/upload and view-dependent hot-color refresh;
- CPU visibility preprocessing, stable sorting and its reusable scratch;
- Paged draw encoding against the active atlas.

`SurfacePresenter` remains the public facade and coordinates Surface acquire,
capture, command completion, queue submission, presentation, shared CPU
completion telemetry and Direct/Paged transactions. Standalone Direct remains
in `standalone_direct_runtime.rs`; Product Packed continues through
`SurfaceRenderSession`, `SurfacePresenterHost` and the renderer-owned Exact
runtime.

## Preserved contracts

- Paged remains explicit, CPU-only, synchronous and non-full-quality. This
  ownership move does not promote it into a product path or optimize it.
- Direct/Paged switches still construct a complete target candidate before
  commit. A successful switch publishes one owner and clears the inactive
  owner's scene; failure retains the old path and resources.
- Constructor-time Validation, OutOfMemory and Internal scopes still enclose
  both private runtimes and the selected scene candidate before publication.
- Surface acquire, capture, finish/arm, submit and present order is unchanged.
- The wasm constructor-only geometry boundary is unchanged.
- No public Rust/C/JNI/Swift/Web API, ABI layout, WGSL, GPU resource layout,
  pass label/order or pixel contract changes in this slice.

## Focused evidence

- The existing local Paged runtime test passes with the new owner: four fixed
  slots remain addressable and occupied, and three nearby camera positions
  produce non-zero draws.
- Presenter source-policy coverage proves that Paged active-set, CPU sort,
  preprocessing/color refresh, pipeline/layout creation and Paged draw-pass
  ownership no longer live in the facade.
- The private owner has a focused source-contract test covering that complete
  execution closure.

The full candidate verification ledger is recorded in the handoff for this
commit. Device/browser experiments are intentionally outside this mechanical
ownership slice. This is not M7 acceptance and removes no grandfather record.
