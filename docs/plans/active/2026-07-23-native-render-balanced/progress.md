# Balanced Resident Research Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
B0 = Accepted
B1 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 3ecf0f2d0180c197faa132443066b3b9b98d36d4
B1 = diagnostic-surface-capture-receipt-bridge
<!-- gsplat-program-active-lanes: end -->

## B0 authoring slice

| Field | Frozen value |
| --- | --- |
| hypothesis | Resident precision/layout trade-offs can be evaluated independently without reducing source membership, source SH degree or rendered resolution. |
| integration baseline | `6d5bd5442dee31cea24906744dfdd1d7492095ae` |
| author branch | `codex/b0-balanced-contract` |
| owned files | this ledger and [B0 contract](b0-contract.md) only |
| forbidden scope | renderer/shader/test/API/policy/package/default/remote changes |
| completion owner | root integration review, completed with independent Accept review |

## Frozen package boundary

Balanced is an all-resident, full-membership research profile. Exact remains
the product default. Balanced is opt-in until B5 qualifies and explicitly
promotes a named endpoint; B1--B3 cannot change a public default or stable API.

The [B0 contract](b0-contract.md) now fixes:

- source/decoded/encoded/resident/addressable equality, unchanged source SH
  degree, full requested/Surface/internal/presented resolution, and the pinned
  `SortedAlpha` count/lifecycle receipts;
- a concrete `gsplat-balanced-image-gate/v1` with per-frame RGBA and
  moving-sequence temporal bounds that a repository validator must enforce
  before B1--B3 retain evidence;
- canonical authored and moving camera modes, real-scene minimums and Tier 1
  endpoint scopes;
- one-variable B1 depth-key, B2 projected-plane and B3 Resident attribute
  experiments, with combination deferred to B4;
- finite per-endpoint Accepted/Rejected/Deferred outcomes. Unclear performance
  Rejects instead of starting an unbounded tuning loop; unavailable external
  evidence Defers only its named scope.

## Candidate status

- Contract authoring: Accepted by independent review and root integration (`083870f`).
- Implementation evidence: none; B0 is a document contract.
- Product behavior/defaults: unchanged.
- Remote publication: forbidden for this slice.
- B0 is **Accepted**. B1 begins with the fail-closed image-gate validator;
  quantized depth-key execution and retained experiment evidence remain locked
  until that validator exists. B2--B3 remain not started.

## B1 validator slice

The first B1 implementation slice adds the repository validator for the frozen
`gsplat-balanced-image-gate/v1` contract. It validates artifacts only; B1
remains **Active** and no quantized depth-key implementation, renderer policy,
endpoint result, performance assertion, or product-default change is included.

This validator slice was independently accepted and root-integrated as
`9db1d14`, `5d4531e`, `a3fcdce`, and `1eb3b91`. That acceptance applies only
to the evidence admission seam below; B1 itself remains Active until the
separate 24-bit/20-bit depth-key experiment reaches its own terminal result.

The validator fails closed unless an artifact provides and proves:

- a hash-matched repository dataset manifest whose asset hash, point count and
  SH degree bind complete source/decoded/encoded/resident/addressable
  membership, `SortedAlpha`, and no sampling, LOD, or partial scene;
- equal requested, Surface, internal-render, and presented dimensions with
  dynamic resolution and upscaling disabled, bound to a repository camera
  trace whose ID, file/content hashes, display, poses and intrinsics pass the
  existing trace validator; formal traces must additionally bind their
  derivation source path, asset hash, point count and SH degree to that same
  dataset manifest, and a minimal contract fixture cannot claim formal quality;
- successful per-frame Exact/candidate presentation tickets whose scene,
  camera, viewport, contract, plan and presentation generations match, plus
  artifact-local, hash-matched, separately owned,
  non-interlaced RGBA8 PNGs for Exact and candidate output;
- recomputed per-frame SSIM, RGB and alpha metrics matching the artifact
  receipts and satisfying every frozen B0 threshold individually; and
- for moving quality evidence, exactly `0 -> 1 -> 0` captures plus both
  adjacent temporal-residual receipts, recomputed from retained RGBA bytes.

Missing fields, unavailable images, mismatched metric receipts, unsafe paths,
symlink/hardlink image aliasing, over-limit decompression streams, or
incomplete/misjoined transitions are rejected rather than defaulted. Formal
quality evidence below 1920x1080 is also rejected; smaller deterministic inputs
remain explicitly labeled contract fixtures and cannot qualify an endpoint. A
formal image must reference a hash-matched per-lane run artifact that first
passes the repository's canonical `gsplat-benchmark/v1` validator. The selected
terminal frame then binds pair/run/frame identity, capture and trace index,
dataset and trace authority, clean build/profile, full source count, successful
presentation lifecycle, and the same retained image bytes. External artifact
paths, forged hashes, receipt cross-joins, or generation cross-joins are
rejected. A real 1920x1080 positive fixture exercises the complete formal path;
it remains contract evidence rather than endpoint qualification. A
successful invocation emits the validator version and SHA-256 so later retained
evidence can identify the exact validator. Focused unit coverage exercises both
valid camera modes and the fail-closed boundaries; it is repository-local
contract evidence only and is not device, browser, image-quality, or B1
experiment evidence.

## B1 shared depth-key quantizer slice

The first executable B1 slice is a private contract foundation on baseline
`fb94f916c5e3416afc1012091696de6f1e54f23d`. Exact remains the only product
selection: every production CPU/GPU constructor continues to use stable full32
keys, and no public API, FFI, runtime policy, radix-pass count, or render plan
changes in this slice.

The candidate contract retains the high 24 bits of the canonical positive
IEEE-754 depth key and clears the low eight bits. Key zero remains reserved for
non-visible GPU sources, so the lowest positive candidate bin is represented by
one retained-bit unit. This exception preserves the existing visibility
sentinel without changing near/far tests, canonical FMA depth math, visible
membership, descending order, or stable source-ID ties.

Focused tests can force Exact full32 or Candidate stable-high24 independently.
They prove:

- Scalar and the current architecture leaf call the same Rust quantizer rather
  than defining architecture-local masks;
- Direct key generation compiles the existing WGSL with an explicit private
  override while production compilation still supplies full32;
- the Resident visible-compaction key generator uses the identical WGSL
  override contract in an isolated keygen test without changing its
  out-of-scope production owner; and
- CPU, Direct GPU, and Resident GPU agree on boundary visibility, every emitted
  candidate key, descending order, and source-ID order for depths that collapse
  to the same 24-bit key.

This is contract and focused GPU execution evidence only. It does not reduce
radix passes, measure a performance benefit, run the Balanced image gate, or
qualify Metal/WebGPU/Android endpoints. B1 therefore remains **Active**; 20-bit
keys remain locked until a later 24-bit candidate completes the frozen B0
identity and image gates.

Focused verification for this candidate:

- `cargo test -p gsplat-render-wgpu --lib cpu_order::tests`: 13 passed,
  2 finite benchmark observations ignored;
- `cargo test -p gsplat-render-wgpu --lib direct_gpu_order::tests`: 13 passed,
  4 external-asset/GPU-pressure observations ignored;
- required Metal SortedAlpha conformance: 1 passed;
- renderer all-target Clippy with warnings denied, Rust format, diff check, and
  source-architecture policy: passed.

## B1 surface-depth-precision carriage slice

The next B1 slice is deliberately limited to carrying the already-tested
private `ExactFull32` / `CandidateStable24` choice through the renderer-owned
Surface ordering path. It may change only private CPU/GPU plan construction and
its focused tests. It must leave the product selection at `ExactFull32`, keep
all public Rust/C/Swift/JavaScript APIs unchanged, and retain the same source
membership, near/far decisions, radix width, plan IDs and adaptive policy.

The purpose is diagnostic reachability: a later root-owned evidence harness
must be able to force a fully specified candidate plan without duplicating
depth-key rules or introducing a production preference. This slice does not
run an endpoint experiment, alter a default, make a performance claim, or
allow 20-bit quantization. If a required platform control would expand a
stable binding, it is out of scope and must be handed back to root rather than
silently exposed.

Implementation ownership is intentionally the real Exact plan construction:
`PlanSet`/`CpuPostSortPlan` for CPU order and transactional resident GPU
preparation for Direct GPU order. Surface façade files alone do not own this
state and are not a permitted shortcut.

The carriage candidate is implemented on baseline
`ffaf8b591f07035afc60331a1a2ae8c1e43a0c91`. Root approved the audited true
owner syncs recorded at `300d73a`, `95fc878` and `fa0bc50`; the candidate did
not merge those bookkeeping commits. One immutable private precision value now
travels from `PreparedRuntimeSlot` through `PlanSet` into both the native packed
CPU/Wasm preprocess entry and the Resident GPU order constructor. Existing
constructors still select `ExactFull32`, and scene replacement preserves the
already prepared value. Candidate reachability remains crate-private.

Focused execution proves that two depths differing only in the cleared low
eight bits keep the previous Exact descending order while Candidate produces a
stable source-ID tie. A real renderer-owned Surface GPU candidate also reports
that `CandidateStable24` reached its Resident sorter. The packed native path
matches the scalar shared quantizer; Wasm compiles through the same precision
entrypoint. No shader mask, public API, FFI, plan ID, radix-pass count,
visibility predicate, near/far rule, Adaptive behavior or product default was
changed.

Verification for this carriage candidate:

- renderer library tests: 461 passed, 8 existing research/external-resource
  observations ignored;
- packed preprocess: 4 passed; CPU ordering: 13 passed and 2 ignored; Direct /
  Resident GPU ordering: 13 passed and 4 ignored;
- focused renderer-owned CPU and Surface GPU carriage: 2 passed;
- required Metal SortedAlpha conformance: 1 passed;
- native and `wasm32-unknown-unknown` checks, renderer all-target Clippy with
  warnings denied, Rust format and source-architecture policy: passed.

This remains construction and contract evidence only. Android, browser and
device performance/image qualification were not run, no endpoint is promoted,
and B1 remains **Active** pending the root-owned evidence harness.

## B1 surface-depth-precision presentation-receipt slice

The next B1 slice owns only a private diagnostic profile selection and the
receipt that proves which depth-key precision reached a successfully presented
Packed Exact Surface frame. The profile is selected once before session
preparation, defaults to `ExactFull32`, survives resource replacement, and
does not expand Rust, C, Swift or JavaScript stable APIs. A failed or
unavailable presentation publishes neither a receipt nor a candidate claim.

This slice deliberately does not change a platform collector, write a benchmark
artifact, run a browser/device experiment, or judge image quality. Those later
consumers must join this receipt with their already-presented RGBA capture and
the canonical Balanced validator. The only purpose here is to establish a
truthful renderer-owned identity for that future join.

Implementation on baseline `bd1cf8da1b80ac218e0a9e22841358b770c0b205`
keeps ordinary builds on `ExactFull32` and adds the default-disabled
`diagnostic-surface-depth-key-candidate24` feature plus a crate-private profile
construction seam. The renderer consumes that profile before its unpublished
Packed Surface candidate transaction; the resulting immutable `PlanSet` is the
source of truth for CPU/GPU admission and replacement.

`SessionPublication` now retains a private
`PresentedDepthPrecisionReceipt` only through its existing successful-present
DTO commit. The receipt is derived from the admitted renderer runtime and binds
the profile to the complete frame identity, actual plan, order generation and
presentation sequence. Unavailable or failed attempts never enter that commit
and therefore cannot create or overwrite the receipt.

Focused and broad verification for this candidate:

- the default Exact CPU path, explicit Candidate renderer/PlanSet/Resident-GPU
  path, replacement GPU re-admission and present-fenced receipt tests passed;
- `cargo test -p gsplat-render-wgpu --lib`: 462 passed, 8 existing
  research/external-resource observations ignored;
- host and `wasm32-unknown-unknown` renderer checks passed for the ordinary
  build; the explicitly enabled diagnostic feature also passed host and Wasm
  checks;
- renderer all-target Clippy with warnings denied passed for ordinary and
  diagnostic-feature builds; Rust format, diff check and the source
  architecture checker passed.

No browser, Android, platform collector or artifact writer was changed or run.
No image, performance or endpoint qualification is claimed, and B1 remains
**Active** pending root-owned capture/validator integration and formal endpoint
evidence.

## Current B1 atomic capture-to-receipt join slice

The next B1 slice fixes a discovered evidence-integrity gap before any endpoint
run: a native `SurfaceFrameCapture` freezes pixels after one successful
presentation, while the session's latest precision receipt can be overwritten
by a later successful frame. A delayed consumer could therefore pair the old
RGBA8 bytes with the wrong depth profile.

This slice adds only a private, take-once join at the `SurfaceRenderSession`
composition boundary. The join must bind the capture to the same successful
presentation sequence and `PresentedDepthPrecisionReceipt`, reject a mismatch,
and be unavailable after a failed/unpresented frame or after consumption.
`SurfaceCapture` remains a mechanical readback owner; stable Rust/C/Swift/JS
APIs and platform collectors remain unchanged. The canonical Balanced validator
will require the retained Exact/Candidate depth-profile receipt instead of
trusting a caller-provided lane label.

The acceptance tests are host-only: a capture followed by another successful
frame must retain its original receipt; failed presentation cannot create a
join; mismatched sequence, duplicate take, absent receipt, and malformed
artifact precision fields must fail closed. This does not run a browser or
device, produce a formal artifact, or make an image/performance claim. Web and
Android adapters are later, separately scoped consumers of the proven join.

Implementation on baseline `4f761eb7ef63bf2e960294797daa264e1dd909cb`
keeps `SurfaceCapture` as the mechanical native readback owner and adds the
join only to the private `SurfaceRenderSession` / `SessionPublication`
composition boundary. A successful capture request arms a take-once ledger.
Only an independently observed Surface lifecycle presentation sequence equal
to the renderer-owned `PresentedDepthPrecisionReceipt` sequence can seal the
join. Missing or mismatched receipts become unavailable, failed/unpresented
attempts cannot seal it, a later successful frame cannot overwrite it, and
take/cancel consumes or clears the private state.

The session exposes the completed pair only through a restricted crate-private
`take_surface_capture_evidence` path, which returns the captured RGBA8 bytes and
their matched renderer receipt as one typed value. The stable public
`take_surface_capture` remains pixels-only, projects from that same atomic take,
and consumes the receipt so a later evidence producer cannot reuse or rejoin
it. No latest-receipt getter participates in either path.

The canonical Balanced validator now requires each retained image receipt to
bind `ExactFull32` to the Exact lane and `CandidateStable24` to the Candidate
lane, with an explicit `presentation_sequence` equal to that lane's successful
presentation generation. Formal quality additionally requires the identical
depth-precision receipt in both the hash-covered canonical benchmark terminal
frame and its outer artifact receipt. Wrong or missing profiles, wrong
sequences, missing benchmark receipts, and lane/profile swaps fail closed.

Host-only focused verification for this candidate:

- `cargo check -p gsplat-render-wgpu --lib`: passed;
- `cargo test -p gsplat-render-wgpu --lib evidence::session_publication::tests`:
  7 passed;
- `cargo test -p gsplat-render-wgpu --lib surface_session::exact_control_tests`:
  9 passed;
- `cargo test -p gsplat-render-wgpu --lib surface::capture::tests`: 6 passed;
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest tests/perf/test_validate_balanced_image_gate.py`:
  36 passed;
- renderer all-target Clippy with warnings denied passed for ordinary and
  `diagnostic-surface-depth-key-candidate24` builds;
- Rust format, source-architecture tests (3 passed), and diff check passed.

No browser, Android, iOS, `adb`, `simctl`, platform collector, shader, binding,
PlanId, Adaptive policy, or product default was changed or run. No formal
artifact was created, no image-quality or performance conclusion is claimed,
and all device/browser/endpoint qualification remains **Deferred**. B1 remains
**Active** pending separately scoped collector integration and formal endpoint
evidence.

## B1 diagnostic Surface capture bridge slice

The proven private capture-to-receipt join is not callable by the existing
desktop real-window evidence host: the ordinary public capture method correctly
consumes the receipt and returns pixels only. This slice therefore introduces a
separate, default-disabled Rust diagnostic feature for exactly one purpose:
allow an explicitly opted-in native diagnostic host to take the already joined
capture and immutable presentation receipt as one value.

The bridge must be take-once and presentation-bound. It must not add a
"latest" receipt query, alter the ordinary pixels-only capture method, choose a
Candidate profile at runtime, change rendering defaults, or expose a stable C,
Swift, Kotlin, or JavaScript API. The public diagnostic value contains only the
fields needed to serialize the pre-existing immutable identity: profile,
frame/plan/order generations and presentation sequence, together with the
captured RGBA8 bytes. It is unavailable for Web and unavailable whenever no
Exact capture/receipt join exists.

This is a bridge for the subsequent desktop/Metal artifact consumer, not an
endpoint result. Its acceptance is focused host-only API and lifecycle tests in
both normal diagnostic-receipt and Candidate24 feature builds. No browser,
Android, iOS, image-quality, performance, or product-default conclusion is
authorized by this slice.

## Next B1 desktop diagnostic receipt-host slice

The next consumer is deliberately narrower than a formal artifact collector.
It lets the existing native desktop real-window evidence host opt into the
diagnostic receipt feature and consume its atomic pair immediately after the
capture readback. The host must record the sealed identity in a distinct,
explicit diagnostic log record; the existing M2b collector and its ordinary
pixels-only route remain unchanged.

The desktop package forwards the diagnostic renderer features only under an
explicit package feature. A separate fail-closed CLI selection is required, so
a normal surface-evidence invocation neither builds the diagnostic bridge nor
changes its logs. The candidate build may combine that feature with the existing
Candidate24 diagnostic feature; Exact uses the same receipt-host feature
without Candidate24. The host must take the pair exactly once and write profile,
frame identity, plan, order generation and presentation sequence from the
returned value rather than recomputing or querying a later renderer state.

This slice is limited to desktop host/CLI feature wiring and host-only parsing
tests. It does not create a formal Balanced artifact, alter the accepted M2b
collector, run Metal, compare images, or change C/Swift/Kotlin/JS. A later,
separate collector slice will turn its diagnostic log into a hash-covered
artifact and the root thread will perform the one-shot Metal endpoint run.

Implementation on baseline `51ffe09c8720eba99305ddb1aed71cbb7f4a5a7f`
adds the independent, default-disabled
`diagnostic-surface-capture-receipt` feature. Native diagnostic hosts that opt
in can call `take_diagnostic_surface_capture_receipt` to consume the existing
`take_surface_capture_evidence` composition and receive one immutable value
containing the RGBA8 capture plus stable diagnostic profile/plan strings,
complete frame generations, order generation and presentation sequence. The
ordinary `take_surface_capture` remains pixels-only and consumes the same
private join; no latest receipt getter or caller-labelled profile path exists.

The bridge and its DTO are both excluded from `wasm32`, and the new feature
does not enable `diagnostic-surface-depth-key-candidate24`. Focused native
lifecycle tests passed for `ExactFull32` with only the receipt bridge enabled,
`CandidateStable24` with both diagnostic features enabled, ordinary pixels-only
consumption, absent/unpresented receipts, sequence mismatch and duplicate
takes. The three requested locked library checks (default, receipt-only and
Candidate24 plus receipt), Rust formatting/diff checks and the source
architecture test passed.

No browser, Android, iOS, real-device or evidence-host run was performed; no
collector, binding, ABI, shader, plan, Adaptive policy or product default was
changed. This bridge makes no image-quality, performance or endpoint claim.
B1 remains **Active** pending separately scoped desktop/Metal host integration,
formal artifact validation and endpoint evidence.

## B1 desktop diagnostic receipt-host implementation

Implementation on baseline `2adf2a38ff2a9272f85ff267f7ae81174b37fcfe`
adds two default-disabled desktop package features. The receipt-host feature
depends on `interactive-viewer` and forwards the renderer's native diagnostic
capture-receipt bridge; the separate Candidate24 feature depends on that host
feature and forwards the renderer Candidate24 diagnostic. Both remain outside
ordinary/default builds, and a diagnostic-feature desktop build is rejected
for `wasm32`.

The native-only `--surface-diagnostic-capture-receipt` flag is accepted only
when the receipt-host feature is compiled and only with the existing strict
`--surface-evidence-plan` contract. A Candidate24 desktop binary rejects every
non-help execution without that flag, so it cannot silently run without
receipt output. Exact diagnostic builds use the same flag without enabling the
Candidate24 feature; ordinary M2b invocations retain the pixels-only take and
existing log behavior.

After the final capture presentation, diagnostic mode immediately consumes
`take_diagnostic_surface_capture_receipt()` and stores that returned DTO as the
pending capture value. It never falls back to `take_surface_capture()` on an
absent, failed, or repeated diagnostic take. Once the existing current-stats
terminal also validates, the host writes the unchanged PNG and M2b capture
record, then emits exactly one `SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT` line. Its
profile, five frame identity values, plan ID, order generation, presentation
sequence, dimensions, and SHA-256 over the returned RGBA8 bytes all derive
only from the immutable DTO; no current/latest session state substitutes for
those fields.

Focused verification passed:

- `cargo test -p desktop-example --locked --bin desktop-example`: 18 passed;
- the same unit-test command with
  `--features diagnostic-surface-capture-receipt`: 28 passed;
- the same unit-test command with
  `--features diagnostic-surface-depth-key-candidate24`: 19 passed;
- locked desktop checks passed for `interactive-viewer`, the receipt-host
  feature, and the Candidate24 feature combination;
- `cargo fmt --check` and `git diff --check` passed;
- `PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_desktop_surface_evidence.py`:
  26 passed;
- `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/test_source_architecture.py`:
  3 passed.

One initial package-wide diagnostic-feature test command unintentionally
included the existing `surface_geometry_entry` harness, which selected the
local Apple M4 Metal adapter and printed its smoke PASS. That out-of-scope run
is excluded from this slice's acceptance evidence and establishes no formal
Metal, image-quality, performance, or collector result. No collector,
artifact writer, browser, Android, iOS, binding, ABI, shader, renderer policy,
or product default was changed. Formal desktop Metal collection and all other
endpoint/device qualification remain **Deferred** to separately authorized
tasks; B1 remains **Active**.

Fixed-SHA review then rejected a test-only parser bypass because it obscured
the Candidate24 runtime gate. The repair removes that bypass completely:
every production and test call now uses the same `Args::parse`, and every
successful Candidate24 parse requires
`--surface-diagnostic-capture-receipt`. Since that flag in turn requires the
strict `--surface-evidence-plan` contract, ordinary `--interactive` and other
receipt-less Candidate24 executions fail closed. Candidate-incompatible
ordinary success cases are excluded under that feature rather than parsed
through alternate semantics; direct tests cover empty arguments, ordinary
`--interactive`, strict evidence without the flag, and the one valid strict
receipt-host combination.

A subsequent fixed-SHA review rejected independent publication of the
validated current-stats terminal and diagnostic capture DTO. The repaired host
now builds both join identities and compares them before updating terminal
state, writing the PNG, or printing either capture record. The gate requires
equal scene, camera, viewport, contract and plan-set identity; the renderer
plan maps to the exact DTO plan name; order generation and presentation
sequence match; and capture dimensions equal the requested evidence
resolution. Any mismatch returns an error and publishes nothing. Pure tests
cover a complete match plus every individual identity, plan, order,
presentation and dimension mismatch. No renderer, FFI or public API changed,
and no endpoint was run for this repair.

## B1 desktop formal-artifact collector candidate

Implementation on exact parent
`e5c5e4848dc0dd40338880a294d13d647cd156d2`, branch
`codex/b1-balanced-desktop-artifact-collector-dd5b`, adds only the downstream
collector and its synthetic tests. The accepted M2b collector and both
canonical validators remain unchanged. Shared dataset/trace authority,
artifact primitives, PNG decoding, image metrics and temporal metrics are
called from the existing M2b and Balanced modules rather than reimplemented.

The collector builds two private release binaries and invokes the desktop host
exactly twice: one ExactFull32 process and one CandidateStable24 process. Each
process receives `--surface-diagnostic-multi-capture` and must yield exactly
three ordered terminal receipts for capture indexes `0/1/2` and trace frames
`0/1/0` from that one continuous Surface session. Lane profile, full
resolution, current-stats/capture identity, present fence, S/V/C/D semantics,
monotonic receipt identity and finite timings fail closed before any standard
run directory exists.

After both lane sessions validate, the collector stages the six canonical
`gsplat-benchmark/v1` run directories, recomputes the Balanced frame/temporal
receipts through the existing validator implementation, and writes the outer
`gsplat-balanced-image-gate/v1` manifest. It runs the standard benchmark
validator on every run and the Balanced validator on the complete suite before
one fresh-destination rename. Partial host failure, invalid evidence or
validator rejection cannot publish the requested output.

Candidate-local verification covers malformed records, missing and duplicate
captures, lane/receipt mismatches, invalid timing, the exact two-invocation
multi-capture command seam, partial second-host failure, incomplete staging and
no output publication on collection failure. No desktop host, Metal adapter,
real window, browser or device endpoint was run for this candidate. Formal
desktop collection, image-quality admission, performance evidence and every
other endpoint qualification remain **Deferred** to the root-owned one-shot;
B1 remains **Active**.
