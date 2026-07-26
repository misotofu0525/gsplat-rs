# S0 Scalable Coverage and Streaming Contract

> Status: S0 design candidate; no product runtime is implemented by this file.
> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
> Architecture source: [Scalable ownership](../../completed/2026-07-23-native-render-core-refactor/architecture.md#15-scalable-ownership)
> Evidence source: [Scalable track](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md#33-scalable-track)

## 1. Decision

Scalable uses an **offline-authored replacement hierarchy**. A small,
versioned manifest describes independently drawable proxy nodes and immutable,
content-addressed page objects. Each page locator can name a whole object or a
byte range, so the same `PageSource` contract can serve a local bundle, HTTP
objects, or an HTTP range-capable container without changing selection or
rendering policy.

The first reference asset is a directory/index bundle produced offline:

- the manifest contains topology, bounds, monotone geometric error, payload
  locators, contiguous source-leaf ranges, encoded/decoded sizes, hashes and
  codec identifiers;
- a node owns an independently valid Gaussian proxy for all source leaves below
  it; leaves retain the authored highest-detail splats;
- a page is only a transfer/cache unit and may contain several nodes; pages do
  not define visibility, alpha order or replacement semantics;
- payloads decode directly into the compact scene representation required by
  the shared renderer, never through a complete wide `SceneBuffers` owner;
- the bootstrap page contains a complete coarse cut and is admitted before any
  optional refinement request.

S0 selects this asset *model*, not a public file extension or frozen payload
codec. S1 and S2 may revise the private binary layout while proving proxy and
bounded-I/O behavior. A future SOG, RAD or SPZ adapter is eligible only if it
can produce this same internal manifest/page contract without weaker coverage,
budget or receipt semantics.

Plain PLY/SPZ files without authored hierarchy remain inputs to the existing
all-resident path when they fit. If they do not fit, admission returns an
explicit preprocessing requirement; the runtime does not read the complete
file merely to synthesize a hierarchy in memory.

## 2. Product promise and non-claims

Scalable promises that a scene larger than the all-resident budget can become
drawable from bounded metadata and a bounded bootstrap, then improve as pages
arrive while retaining valid whole-scene coverage. It does **not** promise that
every original source splat is resident or drawn each frame, and it must never
emit an Exact/Balanced full-source receipt.

The hard product boundary is:

- every authored source leaf is represented by exactly one node on the active
  hierarchy cut;
- the active cut changes only by complete parent/child replacement;
- every allocation is admitted under an explicit byte owner before work starts;
- every presented Scalable frame reports what was represented, at what error,
  with which bytes and which latency evidence;
- all active node payloads enter one global `SortedAlpha` plan. CPU/GPU ordering
  remains a measured renderer decision and is not chosen by page, platform
  wrapper or asset format.

“No holes” means no source-leaf lineage disappears during load, failure,
eviction or cancellation. It is a coverage invariant, not a claim that a coarse
proxy is pixel-identical to its leaves. Proxy quality has its own image/error
gates and receipts.

## 3. Terms and coverage model

| Term | Contract meaning |
| --- | --- |
| source leaf | One highest-detail authored Gaussian assigned to one hierarchy lineage. |
| proxy node | An independently drawable Gaussian set representing every source leaf in its subtree. It is not an arbitrary sample. |
| replacement group | All direct children of one parent. A strict subset can be resident but can never replace the parent. |
| page | Immutable transfer/cache payload containing one or more node records. It is not a render layer or sort domain. |
| cut | An antichain of nodes whose descendant leaf sets are disjoint and whose union is the complete authored leaf set. |
| prepared cut | A validated cut whose required payloads and GPU resources are ready but are not yet public. |
| published cut | The cut bound to the last successfully presented semantic generation. |
| coverage generation | Monotone identity advanced only when a newly prepared cut is successfully presented. |

For every interior node `p` with children `children(p)`:

```text
leaves(p) = disjoint_union(leaves(c) for c in children(p))

active(p) XOR all(active(c) for c in children(p))
```

The second rule applies to the rendered cut. Parent and child payloads may
briefly coexist in caches during a transaction, but they are never drawn
together in the release-gated Scalable path and are never both counted as
coverage. This avoids both missing opacity and double opacity.

## 4. Runtime ownership and flow

```mermaid
flowchart LR
    Source["Manifest locator"] --> Admit["Metadata-first admission"]
    Admit --> Select["Renderer SceneRequest + cut selection"]
    Select --> Fetch["PageSource"]
    Fetch --> Compressed["Compressed cache"]
    Compressed --> Decode["Bounded decode"]
    Decode --> Decoded["Decoded cache"]
    Decoded --> Upload["GPU page pool"]
    Upload --> Prepared["Prepared global cut"]
    Prepared --> Plans["Shared CPU/GPU Adaptive PlanSet"]
    Plans --> Raster["Global SortedAlpha raster"]
    Raster --> Present["Successful presentation"]
    Present --> Published["Published cut + receipts"]
    Published -->|"camera/error/budget feedback"| Select
```

Ownership remains aligned with the refactored renderer:

- `StreamedScene` owns validated manifest state, page lifecycles, the three
  byte-budget owners and immutable prepared snapshots;
- `Renderer` supplies `SceneRequest`, drains completed work, chooses the one
  global plan, encodes uploads/draws and publishes a coverage generation only
  after successful presentation;
- `PageSource` performs bounded asynchronous I/O and has no camera, Surface,
  cache-eviction, ordering or policy authority;
- Surface/offscreen/platform hosts adapt targets and handles only. They do not
  select pages, mutate budgets or run another controller.

One submission may contain upload, global order and draw work, but I/O/decode
completion never mutates the current cut directly. Resize, scene replacement,
camera revision and budget revision invalidate stale prepared work through
explicit generations.

## 5. Metadata-first admission

Opening a Scalable scene first reads only a bounded manifest/header. Before any
page request it validates:

1. schema/version, root set and topology (acyclic, reachable, bounded depth and
   counts);
2. child lineage partition and monotone parent/child error metadata;
3. finite bounds and supported SH/attribute/coordinate conventions;
4. page locator normalization, ranges, declared byte sizes and hashes;
5. checked totals and per-page maxima without summing into unchecked integers;
6. bootstrap decoded bytes, GPU bytes and worst allowed replacement overlap
   against the selected budgets and adapter limits.

Manifest bytes, node count, page count, string length and nesting depth have
separate admission caps. They cannot grow in proportion to an untrusted
payload before validation. Redirected/base URLs and decoded relative paths are
normalized before use; no page may escape the manifest's allowed source root.

Admission must not construct, retain or borrow a complete `SceneBuffers`, a
full source-index table, all decoded pages or one contiguous buffer sized by
the logical scene. A synthetic manifest representing a scene far larger than
RAM must still be inspectable with memory proportional to bounded metadata and
the selected bootstrap.

Contiguous leaf ranges make the partition proof metadata-only: a parent's
declared range must equal the ordered, gap-free union of its children's ranges.
The offline builder binds the range order to the highest-detail payload hash;
the runtime does not allocate one manifest entry or source index per splat.

## 6. Parent/child publication transaction

Refinement of parent `p` is a transaction:

1. select the complete replacement group and reserve all compressed, decoded,
   GPU and transition bytes;
2. fetch, hash and decode every required page under the captured scene/request
   generations;
3. upload every child payload and validate bindings/counts;
4. build one candidate global cut by replacing `p` with all its children;
5. prepare the global order/plan resources for that cut;
6. render and present the candidate;
7. only then publish its coverage generation and permit retired resources to
   become evictable.

Any failure or stale generation before step 7 discards the candidate and keeps
the prior published cut and its receipt. Coarsening is the reverse transaction:
the parent must be ready and successfully presented before its children become
evictable. At least one complete bootstrap cut remains protected while the
scene is live.

The GPU budget reserves bootstrap bytes, renderer plan/scratch bytes and enough
transition headroom for the largest admitted replacement group. If an authored
group cannot fit that bound, the asset is inadmissible for that budget; the
runtime may not replace children one at a time and create a transient hole.

## 7. Three independent byte budgets

All scalable allocations are charged exactly once to one of these owners.
Point counts may be reported, but never substitute for bytes.

| Budget | Charged bytes | Required behavior |
| --- | --- | --- |
| compressed/source | cached encoded pages, active response/range bodies, retry-retained bodies and decoder input ownership | reserve declared bytes before request; reject or evict before exceeding the limit |
| decoded CPU | decoded compact pages, decoder scratch, validation tables and CPU upload staging | decoder advertises peak before start; a page is not decoded if peak cannot be reserved |
| GPU | bootstrap, page pool, replacement overlap, upload destination/staging, global snapshot/order/project/raster scratch attributable to Scalable | intersect configured budget with adapter limits; allocate transactionally under validation/OOM scopes |

Current, reserved, in-flight and observed peak bytes are separate fields.
Sharing or aliasing may reduce physical bytes only when ownership can prove the
same allocation is not charged twice. “The page cache is 64 MiB” is not enough
to imply the decoded or GPU working set is bounded.

Deterministic eviction protects, in order: the published cut, the bootstrap
cut, an in-progress replacement transaction, then optional prefetched pages.
Unprotected ties use declared benefit/cost priority followed by stable page ID.
A tighter budget triggers a prepared coarser cut before child eviction. If even
the bootstrap plus required renderer resources does not fit, the budget change
fails explicitly and does not publish a partial scene or another profile.

## 8. Selection, quality and ordering

Each node has a conservative object-space geometric error and an offline proxy
quality receipt. Runtime projects geometric error to screen-space error (SSE)
using the actual camera and backing dimensions. It seeks the lowest-error cut
that fits all three budgets and the bounded request/upload work allowance.

Selection is benefit-per-byte scheduling, not a fixed splat-count switch:

- benefit is the estimated reduction in projected error for a complete
  replacement group;
- cost includes compressed, decoded, GPU transition and predicted active-plan
  work;
- hysteresis, minimum residency and cancellation generations prevent camera
  jitter from repeatedly fetching the same group;
- unmet requested SSE remains a valid coarse Scalable frame only when the
  receipt says `quality_satisfied=false` and gives the budget/network/availability
  reason. It cannot enter qualification as a target-quality frame.

Every published cut is flattened into one immutable active snapshot. Its
splats share the renderer's visibility, depth/tie contract, measured CPU/GPU
Adaptive selection and canonical `SortedAlpha` raster. Pages are never sorted
or blended independently. Page order, download order and cache address cannot
affect final alpha order.

## 9. Receipts

Receipt types are internal and immutable during S1--S6; S0 does not widen the
public API. Every unavailable measurement is `unavailable` with a reason, not
zero or an inferred substitute.

The retained proxy-quality record uses the complete leaf cut at the same frozen
camera and backing dimensions as reference. It records image hashes, SSIM,
normalized RGB MAE, alpha MAE, RGB outlier fraction and temporal delta error
for moving traces. The manifest names the metric/schema version and authored
validation set; runtime receipts report that identity plus achieved SSE. S1
freezes any candidate promotion threshold before viewing its final results,
while S7 retains the complete quality-memory-latency curve. There is no
universal FPS threshold and no claim that geometric SSE alone proves appearance
quality.

### 9.1 Admission receipt

- manifest URI/identity, schema and generator version;
- manifest and source-content hashes where available;
- logical source-leaf count, node/page counts and hierarchy depth;
- attribute/SH/coordinate contract and payload codec IDs;
- declared bootstrap and largest-replacement bytes;
- requested/effective budgets and adapter-limit intersection;
- admitted, preprocess-required or rejected result with structured reason.

### 9.2 Page terminal receipt

- scene/request/page IDs and captured generations;
- locator/range, declared and observed encoded/decoded bytes, hash result;
- request queue, transfer, decode, upload and ready latency when observable;
- cache hit/source, retry count and terminal ready/cancelled/failed state;
- structured failure stage, retryability and reason.

### 9.3 Presented-cut receipt

- presentation ticket, camera/viewport/scene/coverage/budget generations;
- active node IDs or a content hash of their canonical list;
- complete-cut proof, represented source-leaf count and active splat count;
- requested and achieved maximum SSE, offline proxy-quality identity and
  `quality_satisfied` plus degradation reason;
- current/reserved/in-flight/peak bytes for each of the three budgets;
- page states, cache hits/evictions and replacement transactions completed;
- manifest-to-bootstrap, request-to-ready, ready-to-present and
  camera-revision-to-refinement latencies where the clock/domain permits;
- actual global plan/order backend, active V/C/D counts, render resolution and
  terminal success/failure identity.

Only the successful presentation boundary publishes a new cut receipt.
Preparation, queue submission, Surface unavailability and failed presentation
cannot advance public coverage, policy learning or latency success.

## 10. Failure and recovery contract

| Failure | Required result |
| --- | --- |
| invalid/cyclic/oversized manifest | reject before page allocation; structured admission failure |
| missing/hash-invalid/invalid page | fail that request; keep current parent coverage; never publish partial children |
| transient network failure | bounded retry/backoff under the same generation and byte reservation; terminal failure after declared attempts |
| cancellation or stale generation | release its reservations; stale completion cannot populate cache or cut |
| decoded/GPU budget pressure | evict only unprotected data or prepare a complete coarser cut first |
| adapter limit or GPU OOM/validation error | fail candidate transaction; preserve prior published cut and profile |
| Surface unavailable/present failure | do not publish the prepared cut or success receipt |
| bootstrap cannot fit or is corrupt | fail scene admission; do not draw an arbitrary subset |

Retry is finite and observable. Repeated failures do not lower SH degree,
sample source points, change resolution, switch to Exact/Paged, or spin until a
performance target happens to pass.

## 11. Why historical Paged is not Scalable

The retained Paged diagnostic is useful evidence about slots and residency,
but it violates the Scalable ownership contract:

- `LocalScenePageSource` borrows a complete `SceneBuffers`;
- page metadata retains source indices into that full owner;
- spatial analysis and global source sorting require the full scene;
- scheduling/extraction/packing/color work are synchronous in the frame path;
- four GPU slots are a fixed diagnostic configuration, not three independent
  byte-budget owners;
- its coarse cover is sampled from source splats, not an independently valid
  authored proxy hierarchy with measurable error;
- it has no metadata-first remote source, complete-child replacement
  transaction, bounded compressed/decoded caches or Scalable receipts.

No S task may rename, wrap or automatically select that implementation as its
starting runtime. Reusable low-level slot-generation or checked-payload ideas
may be extracted later only after their ownership no longer depends on full
`SceneBuffers`; such reuse is not evidence that Paged already streams.

## 12. Research decision record

External sources guide the design but are not local completion evidence.

| Source | Accepted | Rejected or deferred |
| --- | --- | --- |
| [PlayCanvas Streamed SOG format](https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/streamed-sog/) and [LOD streaming guide](https://developer.playcanvas.com/user-manual/gaussian-splatting/building/lod-streaming/) | spatial hierarchy, independently addressable chunks, one LOD per region and coarse-first loading | do not bind the core to WebP/SOG; distance thresholds and a global splat-count budget alone do not prove byte bounds or proxy quality |
| [Spark 2.0](https://github.com/sparkjsdev/spark/blob/main/docs/docs/new-features-2.0.md) and [Spark renderer](https://github.com/sparkjsdev/spark/blob/main/docs/docs/spark-renderer.md) | root-to-leaf frontier, fixed shared GPU page pool, range-capable `.RAD` assets and viewpoint-prioritized fetch | do not copy platform point-count defaults, browser worker ownership or assume its file/runtime representation is portable to native `wgpu` |
| [OGC 3D Tiles 1.1](https://docs.ogc.org/cs/22-025r4/22-025r4.html) | geometric error projected to SSE and explicit `REPLACE` refinement semantics | do not adopt the generic 3D Tiles/glTF ecosystem or `ADD` refinement for transparent Gaussian coverage |
| [Hierarchical 3D Gaussians](https://repo-sam.inria.fr/fungraph/hierarchical-3d-gaussians/) | interior Gaussians must represent descendants, hierarchy cuts, screen granularity and authored quality validation | training-time joint optimization and smooth parent/child interpolation are Deferred until a basic atomic replacement path passes image gates; interpolation cannot be used to hide incomplete children |
| [Niantic SPZ](https://github.com/nianticlabs/spz) | compact, versioned page-payload candidate and bounded/streaming decode inspiration | compression alone supplies no spatial hierarchy, replacement rule, page cache or active-cut quality contract; a whole SPZ is not a Scalable scene |

## 13. S1--S7 execution order

Each task owns one narrow result and ends **Accept**, **Reject** or **Defer**.
There is no fixed FPS or competitor-win completion gate.

| Task | Independently verifiable result | Hard gate | Reject / Deferred boundary |
| --- | --- | --- | --- |
| S1 authored proxies | deterministic offline hierarchy builder plus versioned manifest/page fixture | leaf lineage partitions exactly; every node is finite, independently drawable and has monotone error; complete leaf cut reproduces source; proxy cuts pass declared multi-view coverage/image schema | Reject a proxy/merge method that cannot pass; training-optimized proxies may be Deferred without blocking a correct geometric baseline |
| S2 metadata-first source | local and HTTP/range `PageSource` plus bounded decoder into compact pages | huge synthetic logical scene opens without full body or `SceneBuffers`; ranges/hashes/path rules/check arithmetic and cancellation pass | Reject any API requiring full input ownership; real remote endpoints may be Deferred while deterministic local HTTP evidence closes core behavior |
| S3 three caches | independent compressed, decoded and GPU budget owners with reservation and deterministic eviction | adversarial request/decode/upload schedules never exceed any declared peak; stale work releases reservations; bootstrap/transition headroom is protected | Reject oversubscription or count-only budgeting; platform memory-pressure callbacks wait for S6 |
| S4 selection/replacement | SSE selector and transactional refinement/coarsening over simulated delayed, failed and reordered pages | every published cut proves complete lineage; parent remains until all children are ready and presented; no holes/double coverage under failure/eviction | Reject non-atomic selection; transition interpolation remains Deferred unless separately image-qualified |
| S5 global render snapshot | one immutable active cut consumed by the existing shared plan boundary | materialized-cut oracle matches page-built cut; all pages share one global CPU/GPU order and canonical raster; successful present is publication boundary | Reject per-page sorting/render graphs or a second renderer scheduler; unavailable portability endpoint is Deferred, not emulated |
| S6 platform feedback | bounded memory/thermal/network signals adjust budgets inside the same Scalable contract | hysteresis/cooldown/generation tests; no hidden profile/SH/resolution change; current cut remains valid during budget changes | Each unavailable platform signal is Deferred and reported; it does not block deterministic manual budgets on other endpoints |
| S7 qualification/close | retained quality-memory-latency curves on real oversized scenes and available native/Web/mobile endpoints | receipt identity, coverage, budgets, same camera/resolution, raw images and terminal timings validate; claims are endpoint-scoped | Reject product promotion if quality/complexity is unjustified; unavailable iPhone/browser/device evidence is Deferred explicitly, never inferred |

The tasks are sequential at their semantic seam: S2 consumes an accepted S1
asset, S3 owns resources used by S4, and S5 is the first product renderer
integration. Read-only research or fixture preparation may run in parallel only
with disjoint write ownership. S1 cannot start until root review accepts this
S0 contract and updates the program ledger.

## 14. S0 acceptance boundary

S0 is ready for root review when:

- the selected asset model, cut invariant, metadata admission and page roles are
  unambiguous;
- the three byte budgets account for in-flight, transition and peak memory;
- publication, errors, receipts and historical Paged rejection are explicit;
- S1--S7 have finite independent evidence and Reject/Deferred exits;
- every local/external link resolves and the active ledger still names only S0.

Acceptance of this document proves design closure only. It proves no authored
proxy quality, bounded decoder, cache, streaming runtime, endpoint behavior or
performance. Until S1--S5 pass their gates, the repository has no Scalable
product path.
