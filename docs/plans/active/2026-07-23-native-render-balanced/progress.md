# Balanced Resident Research Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
B0 = Accepted
B1 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 3ecf0f2d0180c197faa132443066b3b9b98d36d4
B1 = balanced-image-gate
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

The validator fails closed unless an artifact provides and proves:

- a hash-matched repository dataset manifest whose asset hash, point count and
  SH degree bind complete source/decoded/encoded/resident/addressable
  membership, `SortedAlpha`, and no sampling, LOD, or partial scene;
- equal requested, Surface, internal-render, and presented dimensions with
  dynamic resolution and upscaling disabled, bound to a repository camera
  trace whose ID, file/content hashes, display, poses and intrinsics pass the
  existing trace validator;
- successful per-frame presentation tickets and complete positive lifecycle
  generations, plus artifact-local, hash-matched, separately owned,
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
successful invocation emits the validator version and SHA-256 so later retained
evidence can identify the exact validator. Focused unit coverage exercises both
valid camera modes and the fail-closed boundaries; it is repository-local
contract evidence only and is not device, browser, image-quality, or B1
experiment evidence.
