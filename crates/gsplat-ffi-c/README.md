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
  must be rejected.
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
