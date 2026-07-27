# Scalable Runtime Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
> Design candidate: [S0 coverage and streaming contract](s0-contract.md)

<!-- gsplat-program-task-states: begin -->
S0 = Accepted
S1 = Active
<!-- gsplat-program-task-states: end -->

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

## S1 Bonsai authority-manifest admission slice

The next S1 slice admits only the independently reviewable
`gsplat-dataset/v1` identity required by the frozen formal gate. It may add the
committed Bonsai manifest, manifest-validation coverage, and the accompanying
contract/ledger record. The manifest must bind exactly the predeclared
source-path, hash, byte count, splat count, SH degree, provenance and local
research-use restrictions; any bounds field must come from a retained local
analysis receipt, never an inferred camera or benchmark result.

This is an authority prerequisite, not proxy quality evidence. It does not add
a renderer consumer, page loader, hierarchy rule, image result, device claim
or product default. Missing local source data or absent independently derived
bounds is a finite Deferred handoff to root, not permission to guess values or
weaken the manifest contract.

Candidate `bonsai.local-candidate.json` now binds the frozen official archive
entry, source SHA-256, byte count, 1,244,819 SH3 splats and local-only rights
status. Bounds were produced with `bench-runner --analyze-spatial` at exact
baseline `ffaf8b591f07035afc60331a1a2ae8c1e43a0c91` after independently
checking the 308,716,644-byte source SHA. This candidate can satisfy local S1
dataset-identity admission after review and `--verify-available`; it cannot
support redistribution or a public qualification claim. Manual authored-view
review, proxy images, Metal/A065 runs and the aggregate S1 result were not run
in this slice; the missing review remains an explicit Deferred prerequisite.

## Current S1 proxy-image-evidence validator slice

The next S1 slice is deliberately limited to a fail-closed validator and its
focused tests. It consumes retained image-evidence artifacts; it does not add a
renderer consumer, create a proxy bundle, select a camera, run a browser or
device, or change any product default. The validator must independently bind
each endpoint to the frozen source, camera, resolution, proxy cut, page/cut
hashes, and exact `visible/contributor/drawn` relation before it may reuse the
canonical image gate. A missing field, unavailable count, incomplete endpoint,
or failed image threshold is Reject/Deferred evidence, never a substitute
quality claim. Real Bonsai proxy generation and Metal/A065 qualification remain
separate root-owned slices after this validator exists.

The candidate now adds the independent
`tests/perf/validate-scalable-proxy-image-gate.py` validator for
`gsplat-scalable-proxy-image-gate/v1`. It imports the pinned B1 image-gate
reader and canonical benchmark validator rather than copying their path
confinement, trace validation, artifact-tree hashing, PNG decoding, or image
and temporal metric implementations. On top of those validated inputs it
checks the S1-only contract: Bonsai/formal authority identities, the exact
three frozen cuts and their canonical coverage/page/node hashes, complete
`S/R/P` coverage, terminal per-frame `V/C/D`, the complete two-endpoint and
forced-CPU/forced-GPU capture matrix, and logical-all per-image and transition
thresholds. Contract fixtures report `ValidatedFixture`; only complete formal
evidence can report `Accepted`. Missing required external authority or endpoint
access reports `Deferred`, while malformed, incomplete, hash-invalid,
unavailable-count, misjoined or threshold-failing evidence reports `Rejected`.

Focused tests retain one complete synthetic 72-comparison/32-transition
contract fixture and separately reject/defer missing or misjoined canonical
benchmark lanes, invalid/unavailable `V/C/D`, camera/resolution drift,
coverage/page hash drift, missing endpoint/cut/order/capture/transition scope,
producer metric drift, per-image threshold failure and temporal threshold
failure. These tests use tiny local RGBA8 and canonical benchmark artifacts;
they are validator behavior evidence only. No real proxy bundle, Bonsai image
result, Metal/A065 run, quality pass, runtime integration or device
qualification exists in this slice, so **S1 remains Active** and S2--S5 remain
locked.

## S1 shared-validator pin maintenance

The B1 validator repair that admits the valid initial
`viewport_generation = 0` changed the hash of the shared Balanced validator.
S1 correctly failed closed rather than silently using the new dependency; the
S0 contract and S1 validator pin now name the repaired hash
`6c1e61edf97096ecb8dd1555cc9553353d6a12dd77373c643f5a65a4138c0dfa`.
The repair and the later Balanced-only depth-profile registry leave the shared
image metric, artifact/path confinement, resolution and Exactness rules
unchanged. A focused regression verifies that the S0 contract text, S1 pin and
actual shared-validator bytes agree, so later drift fails at dependency
identity rather than masking the remaining S1 assertions.

This is dependency identity maintenance only; it does not create proxy image
evidence, revise the frozen S1 thresholds or unlock S2--S5. **S1 remains
Active**.

## S1 Bonsai proxy-quality closure checkpoint

The offline builder now has one formal S1 entrypoint layered over the retained
generic structural builder. It rejects any source Gaussian below complete SH3,
derives `complete_leaf_exact`, `bootstrap_roots`, and
`mixed_depth_two_replacements` before images are available, applies the frozen
smallest-range replacement rule exactly twice, validates recursive coverage
after each replacement, and rejects a hierarchy that cannot produce at least
two selected depths. The complete leaf path continues to validate every page
and reproduce every source Gaussian bit-for-bit. This does not add a renderer
consumer, runtime hierarchy, S2 source, product default, FFI, or public product
API.

The S1 image validator also provides a read-only formal collection preflight.
It binds the current shared-validator hashes, committed Bonsai authority
manifest, complete-SH3 PLY identity when available, official camera metadata
when available, and both frozen trace file/content hashes. It emits a
machine-readable `Deferred` receipt with exact missing prerequisites and keeps
`s2_s5_unlocked=false`; malformed local authority or hash drift is `Rejected`.
It never synthesizes pages, images, counts, review approval, or endpoint runs.

At root integration, the committed Bonsai dataset manifest, both frozen trace
identities, exact Bonsai PLY (`a16af6d8...0d273`, `1,244,819` complete-SH3
splats) and official 292-entry `cameras.json` (`41e62374...05af3`) all passed
the formal preflight from ignored, read-only worktree asset links. The assets
are not committed or redistributed. The remaining prerequisites are the
approved authored-camera review plus Apple M4 and physical A065 formal
proxy-image artifacts. The formal collection preflight is therefore
**Deferred** only at those three named boundaries. Builder and validator
behavior can close locally, but no Bonsai proxy images or threshold result
exist, aggregate S1 remains Active, and S2--S5 remain locked.

## Ordered implementation ledger

| Task | State in this candidate | Independent result |
| --- | --- | --- |
| S0 | Accepted | coverage/budget/source/receipt contract and selected asset approach |
| S1 | Active: builder and gate frozen | predeclared image-gated, independently valid proxy hierarchy |
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
