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
`refreshed measured frame 0 lacks its own order ticket`. The retained 122-line
log proceeds directly from measured frame 79 to that formal artifact rejection
and renderer destruction; it contains no terminal-drain, receipt-pump, or
`benchmark_measurement_flush_error` record. It therefore cannot establish
whether callback progress, pumping, or terminal association would have
completed. What it does establish is that Exact `SurfaceFrameOutput` published
refreshed frames with `order_measurement_submission=NotRequested`, so the
strict artifact builder correctly rejected the missing same-frame order ticket
before evidence publication.

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

## A065 follow-up: historical terminal projection

### Fixed boundary

- Parent: `45e09bbb35e95b314bf58c2602ebf3a321fb422e`.
- Scope: preserve an already-consumed current-stats terminal across the Android
  adapter/sample-ledger boundary. Renderer, C ABI, JNI, collector schema and
  strict acceptance rules remain unchanged.
- Physical Android execution remains root-owned.

### Device evidence and mechanism

The canonical CPU run issued and armed all 80 current-stats tickets. During the
no-render drain, native queue completion advanced the final two atomic receipts,
and the compatibility order ledger observed tickets 79 and 80. The strict
current-stats ledger observed only one of those terminals and retained measured
frame 78 pending without a rejection.

This was not a stuck Vulkan map callback or a bounded readback-slot leak. The
Android adapter had already single-popped ticket 79, validated it, removed it
from its bounded pending map and recorded its terminal tombstone. Because ticket
79 belonged to the preceding presentation while ticket 80 was still current,
the adapter deliberately normalized its UI state to `Pending(ticket=80)` to
avoid resurrecting stale counts. The sample's separate strict ledger consumed
only that normalized state, so it never learned which raw terminal had just
been destructively removed. Repeated queue-complete pumps could not replay the
lost single-pop value.

### Repair contract

The additive Kotlin `pollResult()` returns the one raw native poll together with
the adapter's existing presentation-safe state. It performs exactly one JNI
poll. Ordinary typed wrapper polling still returns only the normalized state;
the strict sample consumer uses the raw ticket/full identity to terminalize its
ledger, then uses the normalized state for live display. Thus an older Ready
closes only its matching issued record while a newer Pending remains Pending.
No count, terminal, ticket, render, submit or queue completion is synthesized.

Focused tests cover the exact two-ticket case: raw Ready for the older ticket
survives normalization to the newer Pending state, closes the corresponding
strict sample record, and leaves the newer ticket pending until its own Ready.

### Candidate-local verification

- Forced focused Gradle tests: Android binding current-stats 31 passed; sample
  current-stats consumer 19 passed.
- Full Android unit suites: binding 42 passed; sample app 52 passed.
- JNI host smoke: passed.
- Android arm64 release AAR assembly: passed with API 24.
- Android sample debug APK assembly: passed with the documented runtime minimal
  fallback because no showcase dataset was supplied to this build.
- `cargo check --workspace`, `cargo fmt --all --check` and `git diff --check`:
  passed.
- No emulator, physical Android device or Vulkan benchmark was run. The root
  task retains the sole A065 fixed-SHA rerun and formal artifact decision.

## M7 strict terminal-ledger repair v2

### Boundary

- Parent: `707cd8001abf84cd9fc990dd45246a0c02db75ef`.
- This repair closes two evidence-layer gaps only. It does not change the
  Renderer/session owner, native terminal poll, C/JNI ABI, render graph,
  submission cadence, ticket allocation, `SortedAlpha`, Exact plan selection,
  source membership, SH degree or resolution.
- Android continues to translate immutable Renderer receipts. The bounded
  sample ledger consumes each destructive raw terminal by ticket before the
  presentation-normalized state is used for UI display. No unbounded UI
  history or native replay queue is introduced.
- Strict artifact production and both collector/extractor validation paths
  require a refreshed frame's order submission, successful order terminal and
  current-stats Ready terminal to use one ticket. An unrefreshed frame must
  carry the explicit null order-ticket state. Independently valid terminals on
  different tickets are rejected.

### Qualification separation

This candidate is host-side repair evidence, not A065 qualification. Focused
Kotlin drain tests reproduce historical Ready A while newer ticket B remains
Pending, then prove both raw terminals close the bounded strict ledger without
another render or submit. Collector and standalone extractor fixtures retain a
valid order terminal on ticket A and a valid current-stats terminal on ticket B
and require fail-closed rejection of that cross-ledger drift.

The root task alone may decide whether to build/install the reviewed fixed SHA
and repeat the canonical A065 `2412x1080` Kitsune run. Until that separate
device step succeeds, this note makes no Android/Vulkan completion,
performance, thermal, power or image-quality claim and does not replace the
earlier M5 evidence record.
