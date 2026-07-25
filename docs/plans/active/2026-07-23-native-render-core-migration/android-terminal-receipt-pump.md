# Android strict Exact receipt closure

## Candidate boundary

- Parent: `8c81a102ff863d034be1a7bbb2d592be8d036f7e`.
- Scope: restore the strict same-frame order ticket/terminal projection for
  explicitly sampled Exact Surface refreshes and make Android terminal-pump
  outcomes observable. Collector schema and strict ledgers are unchanged.
- Physical A065 execution remains a root-owned verification step.

## A065 mechanism finding

The complete third CPU run did reach artifact preparation. It failed before
summary emission at `SurfaceBenchmark.requireOrderMeasurements` with
`refreshed measured frame 0 lacks its own order ticket`. The terminal drain
short-circuited because Exact `SurfaceFrameOutput` published every refreshed
frame with `order_measurement_submission=NotRequested`; an empty order ticket
set therefore looked terminal-complete. The strict result builder correctly
rejected that contradiction. This was not evidence that Vulkan callback
progress had timed out.

The Exact renderer already issued one current-stats ticket for every measured
frame. That ticket has the complete frame/plan/generation/presentation identity
and resolves to one atomic S/V/C/D terminal, but the session never projected it
into the compatibility order ledger used by Android. Metal pump success could
not repair a ticket that was never registered in that ledger.

## Repair contract

For a presented Exact frame that both refreshed order and issued the explicitly
requested current-stats receipt, `SurfaceFrameOutput` now registers that same
ticket as its order measurement submission. It creates no second request,
ticket, command buffer, queue submission, or render. The current-stats command
buffer attaches a queue-completion callback before its existing submit and
retains CPU preprocess/sort host timings when the executed plan is CPU.

Polling the atomic current-stats terminal publishes exactly one matching
compatibility terminal:

- CPU -> CPU preprocess/sort plus frame-to-queue-completion and the same V/C/D.
- GPU -> CompletionOnly timing plus the same V/C/D; unavailable phase
  timestamps remain null.
- map failure -> readback-map failure; invalidation/expiry/drop -> generation
  invalidation failure.

No current-stats request means no compatibility ticket. A presented frame that
did not refresh order also does not register an order ticket. Thus ordinary
rendering stays observer-free and the strict per-refresh requirement is met
only with genuine same-frame issued evidence.

The additive C/JNI `pump_receipts_v1` returns QueueComplete versus Timeout
without changing either into an error. Android logs the pump status, calling
thread id/name, and order/current-stats issued/terminal/pending ledgers. The
final classification distinguishes native pump error, repeated timeout, and
queue completion with an unconsumed callback or ledger mismatch.

## Candidate-local evidence

- Renderer GPU test: an existing current-stats ticket reaches Ready through
  the no-submit receipt pump, includes finite completion/CPU timings, and the
  next explicit sample is exactly `T+1`.
- Pure ledger test: an Exact CPU current-stats Ready terminal resolves the
  already-registered same ticket in the order compatibility queue.
- FFI boundary test: invalid v1 pump input fails without mutating output.
- `cargo test -p gsplat-render-wgpu`: 474 passed, 8 ignored; the ignored tests
  are pre-existing external-dataset/finite benchmark gates.
- `cargo test -p gsplat-ffi-c`: 35 passed.
- `cargo check --workspace` and changed-crate Clippy with `-D warnings`: pass.
- C FFI smoke and JNI host smoke: pass.
- Android arm64 release native build, sample unit tests, AAR assembly, and
  sample APK assembly: pass with API 24; the APK used its documented runtime
  minimal fallback because no local showcase dataset was present.
- Strict collector: 54 tests pass. Artifact extractor fixture route passes,
  including its expected rejection cases.
- Source architecture checker remains red on the unchanged stale M3/M4
  `grandfather.exit_due` entries (`gsplat-ffi-c/src/lib.rs` and
  `gsplat-web/src/wasm.rs`); this candidate does not rewrite that root-owned
  policy history or treat its size notices as a completion gate.
- Physical Android/Vulkan execution is deliberately not run in this worktree.

## Root A065 verification

Build/install the exact candidate and rerun the canonical `2412x1080` Kitsune
20-warmup/80-measured CPU/GPU pair. Require:

- each refreshed measured frame has a unique same-frame order ticket;
- each issued order/current-stats ticket has exactly one matching terminal;
- `BENCHMARK_TERMINAL_DRAIN` reports `terminal_ledger_complete` (or, on
  rejection, its finite diagnostic classification and ledger counts);
- summary/chunks and strict extractor acceptance are complete;
- no terminal-flush render, queue submit, or additional ticket occurs after
  measured frame 79.

Any pump error, finite drain exhaustion, timeout-only completion claim,
missing/duplicate ticket, identity drift, weakened counts, or absent summary
rejects the candidate.
