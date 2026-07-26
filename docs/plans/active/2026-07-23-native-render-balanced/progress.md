# Balanced Resident Research Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
B0 = Accepted
B1 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 3ecf0f2d0180c197faa132443066b3b9b98d36d4
B1 = surface-depth-precision-carriage
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
