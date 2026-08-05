# Native Render Research Branch Summary

## Disposition

`codex/native-render-core-refactor` is frozen as a research archive. It is not
a release branch, a replacement for `main`, or a whole-branch merge candidate.

This disposition separates two conclusions:

- the branch contains useful renderer mechanisms, bug fixes and negative
  experiment results;
- the aggregate execution did not deliver a sufficiently small, legible and
  product-focused architecture to justify integrating the complete history.

No test count, accepted subtask or evidence artifact overrides this aggregate
decision. Future work starts from a stable product baseline and carries over
only narrow slices after a new review of their ownership, dependencies and
user-visible value.

## Frozen snapshot

| Field | Value |
| --- | --- |
| research branch | `codex/native-render-core-refactor` |
| frozen tip | `1b13208fc6f790290e31c978b0420b0a44390c73` |
| plan baseline | `c478252246733f6dc209686091caf183e1ef7f06` |
| commits after plan baseline | 550 |
| changed files after plan baseline | 436 |
| added / deleted lines | 176,303 / 36,329 |
| relationship to `origin/main` at disposition | 557 commits ahead of `28f77d0` |

The additions after the plan baseline were approximately:

| Area | Added | Deleted | Net |
| --- | ---: | ---: | ---: |
| core crates and tools | 62,957 | 29,399 | 33,558 |
| tests and qualification harnesses | 66,179 | 753 | 65,426 |
| platform wrappers and examples | 31,565 | 5,638 | 25,927 |
| plans, handbook and other docs | 15,588 | 539 | 15,049 |

The evidence and qualification surface became larger than the product-core
change. The original maintainability goal also remains unmet: the frozen tree
still contains a 5,000-plus-line `surface_session.rs` and several renderer,
GPU and evidence owners between roughly 1,400 and 2,800 lines. File length is
not itself a rejection gate, but here it reflects unresolved responsibility
and state-machine coupling.

## Research results worth retaining

The following ideas or fixes are eligible for selective recovery. Eligibility
is not permission to cherry-pick their entire dependency chain.

1. **Two primary ownership concepts.** `SceneRuntime` owns scene data and
   resources; `Renderer` owns frame execution, camera/viewport generations and
   complete-plan selection.
2. **Transactional prepared runtime.** Scene, plan and raster resources are
   prepared as one candidate and published only after successful admission.
3. **Closed complete plans.** CPU PostSort, GPU PostSort and GPU Preproject are
   concrete plans feeding one canonical SortedAlpha raster, rather than a
   public pass graph or combinatorial strategy API.
4. **Reusable native CPU work.** Stable scalar ordering, reusable workspaces
   and isolated AArch64/x86 SIMD leaves are appropriate native Rust advantages.
5. **Portable GPU primitives.** Projection, compaction, scan and radix kernels
   are useful implementation material when a fresh benchmark proves that the
   complete plan benefits a named endpoint.
6. **Capacity and publication safety.** Checked allocation, full-membership
   admission and successful-present publication are sound correctness rules.
7. **Color-domain correction.** Selecting a non-sRGB unorm Surface format for
   already display-encoded SH colors removed a real native double-encoding
   defect and materially improved the retained Truck reference metrics.
8. **I/O responsibility splits.** PLY metadata, numeric decode and stream
   lifecycle are clearer independent owners.
9. **Scalable research assets.** The deterministic hierarchy builder and its
   coverage rules are useful offline research, but not yet a product streaming
   runtime.

## Results that must not be promoted

- Balanced B1 depth-key, B2 projected-cache and B3 Resident-SH candidates all
  reached finite Rejected results. Their diagnostic features, shaders,
  receipts and platform controls are not product architecture.
- Q1 did not produce a same-quality PlayCanvas/native performance result. The
  earlier unmatched `66.875 ms` versus `17.668 ms` observation is not a formal
  product ratio.
- Q3 produced no accepted A065 Scalar/NEON performance matrix at the frozen
  tip. Collector and ticket correctness do not prove a SIMD advantage.
- Scalable S1 did not qualify proxy image quality on both required real
  endpoints. S2--S7 runtime streaming, caches, replacement and product
  qualification were not delivered.
- Chrome/WebGPU, physical iPhone and Windows/Linux endpoint availability or
  compilation does not establish current frozen-tip product qualification.
- Package-level Accepted labels describe their frozen local contracts. They do
  not make the aggregate branch acceptable for integration.

## Primary cognitive debt

The central design intended evidence to be an optional observer. In the frozen
implementation, experiment evidence crossed the renderer, Surface lifecycle,
C ABI, JNI/Kotlin, Swift and JavaScript boundaries. Multiple submission,
terminal, count, capture and generation joins made platform wrappers understand
internal qualification state.

The largest debt categories are:

- overlapping renderer, Surface compatibility and publication state machines;
- `current-stats` V1/V2 plus order, projected, producer and capture lanes;
- rejected experiment features remaining compiled into the source topology;
- duplicated platform collectors and receipt DTOs;
- qualification infrastructure changes requiring new fixed-SHA endpoint runs;
- active-plan ledgers recording thousands of lines of transient execution
  history;
- a large branch that cannot be reviewed as one coherent product change.

The validation process was rigorous but applied release-grade evidence rules to
ordinary refactoring slices. Fixed-SHA review, immutable artifact publication,
process ownership and one-shot device execution should have been milestone or
claim gates, not the default loop for every local code correction.

## Selective recovery rules

Any successor branch must start from a named stable product baseline. It may
recover a research result only when all of the following are true:

1. the slice has one product responsibility and a narrow dependency boundary;
2. rejected experiment policy, qualification tickets and artifact machinery
   are not carried with it;
3. public Rust/C/Swift/Kotlin/JavaScript API growth has a direct product use;
4. focused compile/unit/image checks complete locally without a formal device
   campaign;
5. the slice demonstrates a user-visible correctness, maintainability or
   measured endpoint benefit;
6. a later milestone, not every commit, owns cross-platform and immutable
   evidence collection.

The preferred recovery sequence is:

1. recover the minimal `SceneRuntime` / `Renderer` / transactional runtime
   ownership skeleton;
2. recover the canonical Exact raster and the color-domain correction;
3. recover one CPU order implementation with reusable workspace, then add SIMD
   leaves only where a small paired benchmark supports them;
4. admit GPU plans individually based on complete-plan endpoint results;
5. keep hierarchy authoring outside the product renderer until a small runtime
   consumer and acceptable proxy images exist.

## Validation policy for successor work

Validation is divided into four costs:

| Scope | Normal evidence |
| --- | --- |
| commit | format, compile and focused unit tests |
| independently reviewable slice | local functional/image smoke |
| milestone | workspace and available cross-platform regression |
| public performance or quality claim | fixed binary, immutable artifacts, repeated endpoint runs and independent review |

Only the last scope requires the full qualification machinery. Performance
percentages, competitor wins and complete endpoint matrices remain observations
or scoped claims, never an unbounded completion gate.

## Branch policy

- Do not merge, rebase or squash the complete research branch into `main`.
- Do not add new product features, public APIs or endpoint campaigns here.
- Preserve retained artifacts and histories only as research provenance.
- Corrections to this summary may clarify provenance; they may not revive an
  experiment or change a terminal result.
- Product implementation resumes on a separately named recovery branch with a
  small plan and independently reviewable commits.
