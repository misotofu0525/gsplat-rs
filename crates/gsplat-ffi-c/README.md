# gsplat-ffi-c

Small, stable C ABI over the `gsplat-rs` renderer and mobile Surface
presenters.

The public contract is the header at [`include/gsplat.h`](include/gsplat.h).
The crate builds as `staticlib`, `cdylib`, and `rlib`, and is the integration
boundary used by the Android JNI bridge and the iOS `GsplatKit` wrapper.

## Usage rules

- Use `gsplat_config_default()` and `gsplat_camera_default()` instead of
  hand-writing ABI defaults.
- Use `GSPLAT_RENDER_MODE_SORTED_ALPHA`; it is the only release-gated render
  mode in v0.1.
- Treat non-zero returns as `GsplatErrorCode` values and pass them to
  `gsplat_error_message()`; use `gsplat_last_error_message()` for the most
  recent operation detail.
- Exported entrypoints catch Rust unwinds before they cross the C boundary;
  error-code functions report `GSPLAT_ERROR_INTERNAL`. This does not make
  invalid pointers or other caller-side undefined behavior safe.
- Native Surface handles are single-owner: serialize all access through one
  thread or queue.
- Surface constructors that select `PACKED_ATLAS` before loading stream the
  path-backed PLY directly into the exact resident representation. Direct and
  Paged constructors retain the wide source representation required by those
  paths. A failed parse or resident build leaves no partially published scene.
- Default Android/UIKit Surface constructors select exact `PACKED_ATLAS` and
  adaptive ordering. Explicit paged construction remains CPU-only.
- Surface clients select CPU, GPU, or runtime-adaptive ordering through
  `gsplat_surface_renderer_set_order_backend()`. GPU receipts are asynchronous:
  call `gsplat_surface_renderer_poll_order_measurement()` until
  `out_available == 0` to drain timestamp-query or queue-completion timing,
  revision, and exact visible/drawn-count evidence without blocking.
- `SurfaceRenderSession` is the sole owner of order, projected-draw, and
  producer submissions, terminal queues, ticket-count state, and producer
  measurement enablement. The C entrypoints validate caller buffers, delegate
  directly to that session seam, and convert the returned value; the opaque C
  handle keeps no compatibility queue, ledger, submission cache, pump, or
  enabled-state mirror.
- `GsplatSurfaceOrderMeasurement` is additive. Its validity flags distinguish
  real GPU timestamp fields from completion-only receipts; the existing
  `GsplatSurfaceSortStats` layout is unchanged. A bounded-queue overflow drops
  the oldest receipt and marks the next retained receipt with
  `GSPLAT_SURFACE_ORDER_MEASUREMENT_DROPPED_PRIOR`, so benchmark collectors can
  reject incomplete evidence instead of silently treating it as complete.
- Every exact non-Paged CPU refresh also requests an even-numbered
  `GsplatSurfaceCpuOrderMeasurement` ticket. Its `frame_complete_ms` covers
  frame start through graphics-queue completion, making forced CPU/GPU and
  Adaptive comparisons use the same metric instead of CPU submit-wall time.
- Every issued CPU or GPU measurement ticket terminates in exactly one success or
  `GsplatSurfaceOrderMeasurementFailure` while the live renderer continues to
  be pumped. Drain both polling APIs; failure reasons distinguish readback-map
  errors from generation invalidation, and each bounded queue exposes a
  dropped-prior flag.
- After every successful frame, call
  `gsplat_surface_renderer_get_order_submission()` before draining terminal
  receipts. Its revision, ticket, measured backend, and explicit ring-busy or
  Surface-unavailable reason let a
  strict collector account for warmup, measured, and terminal-flush frames and
  reject any issued ticket that lacks exactly one success or failure.
- Current S/V/C/D evidence uses the additive, stateless `_v1` sequence:
  initialize each output's `struct_size`/`version`, call
  `gsplat_surface_renderer_request_current_stats_v1()`, render, read
  `gsplat_surface_renderer_get_current_stats_submission_v1()`, then repeatedly
  call `gsplat_surface_renderer_poll_current_stats_v1()`. Global C errors only
  report an illegal call; request admission is instead `REQUESTED`, `BUSY`,
  `GPU_UNAVAILABLE`, `RESOURCE_UNAVAILABLE`, or `TICKET_EXHAUSTED` in the
  current-stats output.
- A submission is either `NOT_REQUESTED` or `ISSUED`. Only `ISSUED` makes its
  non-zero ticket and complete join identity applicable. That identity retains
  scene, camera, viewport, contract and plan-set generations, the executed
  plan, order/raster generations, encode attempt, and presentation sequence.
  Consumers must join the terminal against every field, not ticket alone.
- The poll is a global atomic single-pop. `UNSAMPLED` is a pre-ticket request
  resolution; it has no ticket or identity. `READY` carries ticket, complete
  identity, S/V/C/D and count semantics in the same struct—there is no second
  take-counts call. `MAP_FAILURE`, `GENERATION_INVALIDATED`, `EXPIRED`, and
  `DROPPED` retain ticket/identity but have no usable counts. Payload zeros
  under other kinds are inapplicable fields, never real counts, generations,
  or a real ticket.
- Call `gsplat_surface_renderer_poll_current_stats_v2()` instead of the V1 poll
  when the same atomic terminal must also carry timing. V2 consumes the same
  Renderer-owned queue and never creates or joins a second ticket. `READY`
  always validates `frame_complete_ms`; `cpu_preprocess_ms` and `cpu_sort_ms`
  are applicable only under their explicit validity bits. Empty, unsampled,
  and failure payloads keep all timing bits and values zero. The V1 layout,
  reserved-zero contract, and symbols remain frozen.
- After an `ISSUED` submission, `EMPTY` means no globally oldest resolution is
  ready now; the consumer may describe that issued ticket locally as pending
  and continue rendering/polling. `EMPTY` alone does not identify a ticket or
  prove that one was requested. Evidence is bounded, so an issued receipt may
  later resolve as `EXPIRED` or `DROPPED`; strict evidence rejects that ticket.
  It must never fill a missing current receipt from legacy
  `gsplat_surface_renderer_get_stats()`.
- The legacy `gsplat_surface_renderer_get_stats()` remains successful whenever
  the last presented frame has synchronous current counts or a ticket- and
  generation-matched current-stats v1 `READY` receipt. Unrequested, pending,
  failed, expired, or mismatched asynchronous counts return
  `GSPLAT_ERROR_NOT_FOUND` without modifying the output. The C bridge owns no
  queue, cache, tombstone, ticket, generation, sampling policy, or result
  state.
- `GsplatSurfaceSortStats.flags` preserves its 48-byte layout and now reports
  an Adaptive GPU-unavailable reason (unsupported, initialization,
  out-of-memory, or validation) in bits 15-18. This makes an Adaptive CPU
  fallback observable even when GPU preparation failed before a ticket could
  be issued.
- Candidate/Compact selection is independent from CPU/GPU ordering. Use the
  additive `_v1` projected-draw API to force either exact execution or retain
  the default runtime-Adaptive policy. Initialize every versioned output with
  `struct_size = sizeof(struct)` and `version = 1`; undersized or unknown
  versions fail before the output is changed. V1 layouts are frozen; later
  extensions use new V2 types and symbols. Setter value zero aliases Adaptive,
  while a rendered submission reports the canonical Adaptive value `3`.
- Join `GsplatSurfaceProjectedSubmissionV1`, terminal measurement/failure, and
  `GsplatSurfaceProjectedCountsV1` by non-zero ticket plus camera revision.
  Candidate proves `D=V` while still measuring exact `C`; Compact proves
  `D=C<=V`. A formal ticket also records projection/probe generation and is
  rejected internally if it overlaps an order refresh or violates V/C/D.
  Projected tickets are issued by Adaptive's isolated formal probes; forcing
  Candidate or Compact controls execution but does not request a projected
  ticket by itself. After polling a success, take its counts immediately;
  `out_available == 0` means bounded evidence has expired and a strict run
  must be rejected. Pending, failed, expired, consumed, and invalid tickets
  remain distinct renderer-owned states even though the frozen v1 C call maps
  every unavailable state to `out_available == 0`; it never substitutes a
  stale, zero, capacity, or parity-inferred count.
- Packed GPU producer selection is a separate diagnostic lane. Keep the
  qualified default (`POST_SORT`) unless running an isolated A/B experiment;
  set `PREPROJECT` with
  `gsplat_surface_renderer_set_gpu_order_producer_v1()`, then explicitly opt
  into receipts with
  `gsplat_surface_renderer_set_gpu_producer_measurement_enabled_v1()`. Receipt
  admission requires Packed, forced Compact projected draw, and the GPU order
  lane. Join submission and terminal success/failure by ticket plus camera
  revision. An exact-current success carries source/contributor/drawn counts
  and must prove `D=C<=S`; ring-busy, Surface-unavailable, stale-order,
  dropped-prior, generation-invalidated, readback, or invariant evidence is a
  failed strict run. The additive V1 layouts are frozen and remain dormant
  until explicitly enabled. Disabling measurement stops new tickets but does
  not erase the last rendered submission or terminal outcomes for tickets
  already issued. Drain both terminal queues before treating a later enabled
  period as a fresh experiment. Each producer poll pops exactly one record
  from the renderer-owned lossless raw FIFO. Bounded compatibility-view
  overflow therefore cannot erase or duplicate raw C delivery.
- Call `gsplat_surface_renderer_get_exactness()` after construction to obtain
  source/decoded/encoded/resident/addressable counts, source/resident SH
  degree, full-quality policy bits, and the physical adapter limits used for
  admission. Direct and Packed handles are published only when all counts and
  SH degree match the source; diagnostic Paged handles do not claim that
  contract. A successful C-level geometry-path switch updates the receipt.
- Call `gsplat_surface_renderer_get_presentation()` after a successful frame
  to obtain requested, Surface, internal-render, and last-presented pixel
  dimensions plus the actual-presentation revision. `FULL_RESOLUTION` is set
  only when the last frame was presented and all four dimension pairs match;
  the native path explicitly reports dynamic resolution and upscaling disabled.
- For strict same-camera evidence, initialize and read
  `GsplatSurfaceCameraReceiptV1` immediately after the matching render call.
  The receipt exposes the session's actual f32 pose/intrinsics, current and
  presented camera revisions, Surface aspect, and canonical row-major
  view/projection/view-projection matrices. Both receipt flags must be set and
  the revisions must match. The matrices are derived from live session state;
  the API never reads or copies a benchmark trace payload.

## Keeping the ABI in sync

Any ABI change must update both `src/lib.rs` and `include/gsplat.h`, then pass
the FFI smoke path:

```bash
bash tests/ffi/run-ffi-smoke.sh
```

## License

MIT OR Apache-2.0, at your option.
