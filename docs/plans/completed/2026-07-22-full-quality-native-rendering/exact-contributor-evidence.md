# Exact Contributor Evidence Contract

## Why this exists

The historical `visible` counter means the near/far candidate set, not the
strict set of projected Gaussians that can contribute a fragment. Treating it
as both “visible” and “drawn” made an exact contributor-only draw look like
point loss. This contract separates residency, candidate selection,
conservative projection culling, and actual issued work.

## Count meanings

- `S`: complete source, resident, and GPU-addressable splat count.
- `V`: near/far candidate count; this retains the historical `visible` name in
  v1 artifacts.
- `C`: strict conservative post-projection contributor count. A splat may be
  removed only when its opacity-bounded projected quad provably cannot
  contribute to the viewport.
- `D`: count actually issued to the raster path.

Every formal receipt proves `0 <= C <= V <= S`. An explicit
`exact_contributor_compaction=true` permits `D=C`. Direct, downlevel, and
legacy execution require `D=V`. A budgeted or sampled subset cannot satisfy
either rule.

## Revision-safe terminal join

CPU and GPU sorting finish asynchronously. Frame stats may therefore still
describe a preceding revision when a terminal timing result arrives. Formal
evidence never guesses across those two streams.

Each issued order ticket has exactly one success or failure terminal. A
success is paired with a bounded, take-once count receipt containing:

- ticket;
- camera revision;
- V/C/D;
- exact-contributor execution flag.

Consumers join timing and counts by both ticket and camera revision. The C ABI
adds `GsplatSurfaceOrderCounts` and
`gsplat_surface_renderer_take_order_counts`; the frozen GPU and CPU measurement
struct sizes remain unchanged. Failure/generation invalidation removes the
matching count receipt. The shared count ledger is sized for the combined CPU
and GPU success queues.

## Endpoint wiring

- Desktop Rust consumes V/C/D directly from typed CPU/GPU terminal
  measurements and logs the declared count semantics.
- Android JNI takes the matching additive count receipt before returning a
  terminal Kotlin measurement. Typed CPU and GPU receipts expose the same
  fields and validate their compatibility flags.
- Apple `GsplatKit` takes the matching receipt inside `poll`/`drain`; the iOS
  qualification app does the same for its direct C-ABI collector.
- Web/WASM emits the same fields for CPU and GPU receipts. The SDK preserves
  them, and the collector joins provisional frames to terminal receipts by
  ticket plus revision before rebuilding the artifact summary.

## Artifact compatibility

New artifacts declare
`renderer.count_semantics="candidate_visible_contributor_issued_v1"` and emit
`contributor` plus `exact_contributor_compaction` on every frame. Legacy
artifacts omit all three additions and remain valid only under `D=V`. Mixed or
partially declared evidence fails validation.

Validator coverage includes:

- legal exact `C<V` with `D=C`;
- missing exact-compaction flag;
- exact mode with `D!=C`;
- legacy/downlevel `D<V`;
- CPU and GPU terminal ticket/revision joins.

## Verification record

Fresh passing checks during implementation:

- `npm --prefix packages/web test` (27 tests);
- Web artifact and ordering evidence tests (26 tests after the CPU join case);
- `python3 tests/perf/test_full_quality_experiment.py` (20 tests);
- `bash tests/perf/test-benchmark-artifacts.sh`;
- Android library and sample Kotlin compilation with the repository Android
  native build.

Apple, workspace Rust, and device/browser qualification are recorded only
after their canonical checks complete; this document does not promote a smoke
run into performance evidence.
