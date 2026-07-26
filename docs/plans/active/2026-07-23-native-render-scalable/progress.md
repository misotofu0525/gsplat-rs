# Scalable Runtime Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
> Design candidate: [S0 coverage and streaming contract](s0-contract.md)

<!-- gsplat-program-task-states: begin -->
S0 = Accepted
S1 = Active
<!-- gsplat-program-task-states: end -->

<!-- gsplat-program-active-lanes: begin -->
activation_commit = 3ecf0f2d0180c197faa132443066b3b9b98d36d4
S1 = proxy-image-gate-contract
<!-- gsplat-program-active-lanes: end -->

## Scope

Package S begins after M8 as a separate Streamed semantic contract. It is not a
promotion of the historical four-slot Paged diagnostic and cannot begin by
renaming or wrapping that runtime. S0 owns design and research only; it changes
no renderer, format API, product default or qualification policy.

The selected design is an offline-authored replacement hierarchy with a
bounded versioned manifest and independently addressable immutable pages.
Source-leaf coverage stays complete through a drawable bootstrap cut. A local
refinement keeps its parent published until every direct child is validated,
decoded, uploaded, globally ordered, drawn and successfully presented. Each
child subtree can then refine independently, so valid global cuts may mix
depths, such as `{A1,A2,B}`. Random source sampling, incomplete sibling
publication, ancestor/descendant double coverage and per-page alpha sorting are
excluded.

## Current S0 candidate

- Metadata-first admission is specified without constructing or borrowing a
  complete `SceneBuffers`.
- Compressed/source, decoded CPU and GPU bytes have independent reservation,
  in-flight, current and peak accounting.
- Page/error terminals are generation-bound, while coverage, quality, memory
  and presentation-latency receipts publish only after successful presentation.
- PlayCanvas SOG, Spark RAD, OGC 3D Tiles, Hierarchical 3DGS and SPZ research is
  recorded with accepted and rejected/deferred mechanisms.
- Historical Paged remains a labelled diagnostic because it retains full
  source ownership and lacks authored replacement, three bounded caches and
  metadata-first I/O.
- S1 must freeze its assets, cameras, resolutions, required proxy cuts, numeric
  image thresholds and aggregation before work, then pass them to be Accepted.
  A geometric-only result may remain research but cannot unlock S2--S5.

S0 is Accepted after independent review and root integration (`435d04d`,
`f7164ed`). S1 is Active for a bounded offline builder/fixture slice. It still
requires its own predeclared, image-gated task contract and cannot make a
product claim merely by producing a structurally valid hierarchy.

## Current S1 structural checkpoint

The first S1 implementation slice adds the private `gsplat-hierarchy` crate and
no renderer or product-path integration. Its deterministic offline builder:

This structural checkpoint was independently accepted and root-integrated as
`eef4143`. The acceptance is limited to authored data/coverage integrity; S1
itself remains Active until its separately frozen proxy-image gate has a
terminal result.

- partitions the canonical source sequence into gap-free highest-detail leaf
  ranges and preserves every source Gaussian, its SH degree and the complete
  SH3 rest plane bit-for-bit in the complete leaf cut;
- combines adjacent ranges into a recursive replacement hierarchy, with a
  finite independently drawable proxy, conservative bounds and monotone
  geometric error on every interior node;
- emits one canonical immutable page per node, addressed by the SHA-256 of its
  bytes, plus a stable versioned manifest encoding;
- validates root reachability, acyclicity, unique parent ownership, recursive
  child partition, page identity and the S0 recursive cut predicate before any
  public cut materialization; graph validation uses an explicit stack so a
  malformed cycle cannot consume the native call stack.

The deterministic fixture proves repeated builds are byte-identical,
`{A1,A2,B}` and `{A,B}` are valid, a missing sibling is rejected, an
ancestor/descendant pair is rejected, and the full leaf cut exactly restores
the authored non-zero SH3 source sequence. Tampered pages, cycles, duplicate
roots and overlapping leaf ranges fail closed, while canonical manifest bytes
are independent of root/page input order. These are structural results only.
No frozen camera/reference image, numeric proxy-quality threshold or image-gate
run is part of this slice, so **S1 remains Active** and S2--S5 remain locked.
The interior proxy authoring rule is not eligible for a quality or product
claim until that separately predeclared visual gate exists and passes.

## Current S1 proxy-image-gate contract checkpoint

The pre-implementation promotion contract is now frozen in
[S0 section 9.4](s0-contract.md#94-frozen-s1-proxy-image-gate). This checkpoint
changes documentation only. It does not validate the existing interior proxy,
add a renderer consumer, change a public API/default, or make S1 complete.

The contract fixes:

- the existing B1 canonical image/benchmark evidence pipeline as the required
  implementation base, including path safety, canonical run validation,
  successful-presentation joins, retained RGBA8 bytes and independently
  recomputed metrics; a proxy log or self-reported screenshot table is not an
  alternative;
- complete SH3 Kitsune as a small obtainable bring-up fixture and complete SH3
  Bonsai plus official training cameras `0/146` as the minimum formal asset,
  with source, camera-metadata, trace-file and trace-content hashes pinned;
- Apple M4 Metal at `1920x1080` and physical A065 Vulkan at its re-probed
  `2412x1080` Surface as the two required formal scopes. Simulator, reduced
  backing resolution and one endpoint cannot substitute;
- the complete leaf, complete bootstrap-root and deterministic two-replacement
  mixed-depth cuts, authored views `0/1`, moving `0 -> 1 -> 0`, and fixed-view
  parent/child replacement transitions;
- B0/B1's unchanged per-frame and temporal thresholds with logical-all
  aggregation, plus separate logical source coverage `S/R`, active proxy count
  `P` and actual `V/C/D` receipts so a proxy cut cannot impersonate full
  resident membership.

The two Bonsai traces still declare `candidate_requires_manual_image_review`,
and the source/camera files are external. Missing source, training-camera
metadata, manual authored-view review or physical-device access has an explicit
**Deferred** exit. A valid hierarchy or image run that misses any frozen gate is
**Rejected**. Only a fully joined two-endpoint pass is **Accepted**. Therefore
**S1 remains Active**, the current geometric proxy has no quality claim, and
S2--S5 remain locked pending a separate implementation/evidence slice.

## Ordered implementation ledger

| Task | State in this candidate | Independent result |
| --- | --- | --- |
| S0 | Accepted | coverage/budget/source/receipt contract and selected asset approach |
| S1 | Active: offline builder/fixture | predeclared image-gated, independently valid proxy hierarchy |
| S2 | Locked until S1 Accepted | metadata-first `PageSource` and bounded direct decode |
| S3 | Locked until S1 Accepted, then S2 | three byte-bounded caches and deterministic GPU page pool |
| S4 | Locked until S1 Accepted, then S3 | recursive mixed-depth cut selection and atomic local replacement |
| S5 | Locked until S1 Accepted, then S4 | one global active snapshot through shared CPU/GPU Adaptive plans |
| S6 | Not started | bounded platform memory/thermal/network feedback |
| S7 | Not started | endpoint-scoped quality-memory-latency qualification and closeout |

Each task has a finite Accept/Reject/Defer boundary in the S0 contract. A
failed performance hypothesis ends; it does not weaken coverage or trigger
unbounded tuning. Unavailable endpoint evidence is Deferred and limits the
claim rather than blocking unrelated implementation closure.

## Writer boundary

The S0 writer owns only this ledger and the linked contract. It may not change
resident renderer code, shaders, tests, public APIs, strategy/policy files,
product defaults, or the Balanced and qualification packages. Root review owns
integration, status transition and S1 activation.
