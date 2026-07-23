# Phase 2 production producer A/B checkpoint

> Status: implementation and same-binary A/B complete. The direct
> `S -> C -> stable full32 radix -> D` producer is transactionally reachable
> through the core Surface session and desktop, Web, C, JNI and Kotlin
> diagnostics. Exact A/B favors Preproject on all three tested GPU endpoints,
> while the qualified PostSort producer remains the universal default pending
> a composite-plan runtime controller.

## Outcome

The core renderer can run two complete Packed GPU order producers behind the
same CPU/GPU/Adaptive backend policy and the same exact hardware SortedAlpha
draw contract:

```text
PostSort (default):
  full source S -> order candidate V -> stable full32 radix V
                -> exact projection/compaction C -> indirect draw D = C

Preproject (diagnostic):
  full source S -> exact source-indexed projection/count C
                -> stable source-order compaction {full32 key, source ID}[0..C)
                -> stable full32 radix C -> indirect draw D = C
```

`SurfaceGpuOrderProducer::{PostSort, Preproject}` chooses only the producer
inside the GPU ordering lane. It does not remove or replace
`SurfaceOrderBackend::{Cpu, Gpu, Adaptive}`. The selector defaults to
`PostSort`, so constructing a session without using the new API has unchanged
behavior and allocation. Current Adaptive learns CPU versus GPU ordering while
using PostSort; it does not yet learn the producer axis.

The Preproject graph remains lazy. A Packed session pays its additional static
allocation only after an explicit prepare/select request.

## Admission and isolation

Preproject is admitted only when all of these are true:

- geometry is complete-resident `PackedAtlas`;
- raster execution is `ProjectedQuadsExact`;
- projected draw policy is explicitly forced to `Compact`;
- indirect execution is supported by the adapter.

The session rejects Candidate or Adaptive projected-draw policy while
Preproject is selected. It also rejects leaving the compatible geometry or
raster plan, and rejects enabling producer telemetry outside the same forced
Compact context. These checks are idempotent and happen before live state is
mutated.

This separation is intentional. Phase 1 projected Candidate/Compact learning
and its ticket ring cannot cross into the Phase 2 producer experiment. Old
projected/order tickets are generation-invalidated at a producer transition,
and projected learning consumes receipts only while `PostSort` is active.

## Transactional lifecycle

The public core sequence is:

```rust
session
    .prepare_gpu_order_producer(SurfaceGpuOrderProducer::Preproject)
    .await?;
session.set_gpu_order_producer(SurfaceGpuOrderProducer::Preproject)?;
```

On native targets, the synchronous setter may block while it performs the
same preparation if the graph is not already present. On wasm, a synchronous
setter fails with `GpuProducerPreparationRequired`; browser callers use the
async prepare or async select path instead.

Construction creates the complete projector, scan, compactor, external-prefix
radix graph, source-ID draw graph, and indirect arguments as one dormant
candidate. Validation, out-of-memory, and internal error scopes all complete
before publication. Failure discards the candidate and leaves the selected
producer and frame scheduler untouched. Publication itself is a small set of
infallible ownership moves.

Switching either direction then:

- invalidates both producer-specific order caches;
- invalidates old order, projected-draw, and producer telemetry generations;
- clears pending Adaptive/projection probe ownership;
- forces the next GPU-lane frame to refresh its complete order.

## Refresh and non-refresh semantics

The selected producer still obeys the session sort interval. The meaning of
each count is explicit:

| Frame kind | Projection work | `C` receipt source | IDs and `D` | Scope |
| --- | --- | --- | --- | --- |
| Refresh | project all `S`, scan, compact, stable full32 radix | same-frame radix control | same-frame sorted prefix; `D = C` | exact current contributors |
| Non-refresh | project all `S` and scan again | current full-`S` scan sentinel | previous valid sorted prefix and previous indirect `D` | stale-order candidates |

Therefore a non-refresh receipt may have either `C < D` or `C > D`. `C` is
always the current camera's full-source contributor count, while `D` is the
number of old-order source IDs actually issued. It is never mislabeled as an
exact current-contributor draw.

The non-refresh pass updates the source-indexed f32 projected cache for all
`S`; it does not overwrite the last valid sorted ID prefix or its indirect
arguments. The draw uses those retained IDs to index the newly projected
source geometry. This preserves the existing deliberate stale-order cadence
without reducing source residency, source membership, SH degree, key width,
or render resolution.

There is no presentable stale prefix after initial construction, scene
replacement, resize, a CPU-to-GPU/backend transition, or a producer switch.
Those events force a refresh. The presenter also independently rejects a
non-refresh Preproject draw while its prefix is invalid, so a scheduling bug
cannot publish an uninitialized/invalid indirect draw. Ordinary camera motion
between scheduled sort frames is the intentional non-refresh case above.

## Independent producer receipts

`set_gpu_producer_measurement_enabled(true)` enables a separate eight-slot,
non-blocking receipt ring. It does not reuse or reinterpret the projected-draw
telemetry ring.

Each successful queue-terminal receipt reports:

- ticket and camera revision;
- actual producer used by the presented Packed GPU frame;
- order generation and projection generation;
- complete source count `S`;
- current-camera contributor count `C`;
- issued draw count `D`;
- whether order was refreshed;
- exact-current or stale-order scope;
- frame-start-to-queue-complete milliseconds.

The producer ticket namespace is `[2^51, 2^52)`, disjoint from the existing
order and projected ticket namespaces and exactly representable by JavaScript
numbers. A busy ring is explicitly reported as unsampled; a missing Surface
does not issue a ticket. Readback, generation invalidation, or invariant
failure yields one terminal failure receipt rather than silently dropping an
issued identity.

The ring validates `C <= S` and `D <= S`. Exact-current receipts additionally
require `order_refreshed && D == C`; stale-order receipts require
`!order_refreshed` and deliberately impose no false `C == D` invariant.
For Preproject, `C` is always scanned from all `S`. On a retained-order
PostSort frame, `C` keeps that graph's existing meaning: current projection of
its retained candidate prefix. The producer and stale-order fields prevent
those two scopes from being conflated in analysis.

## Resource and quality contract

The Preproject graph owns two 16-byte-per-source f32 projection planes,
complete-source count/scan storage, a stable external-prefix radix graph with
full capacity for the worst case `C = S`, and one indirect draw record. Its
checked byte plan and per-binding validation run before allocation.

The implementation preserves the Phase 2 architectural gates:

- projection covers every source `S` and uses the existing canonical f32 math;
- compaction input order is ascending source ID;
- the radix consumes all 32 depth-key bits and stays stable for equal keys;
- refresh draws use exactly the complete contributor prefix, `D = C`;
- source IDs index the same complete Resident attributes and resolved SH color;
- no sampling, top-K, LOD, SH reduction, point dropping, dynamic resolution,
  upscaling, or early blend exit is introduced.

## Final A/B evidence on 2026-07-23

All retained runs use complete 2,541,226-point SH3 Truck, Packed,
ProjectedQuadsExact, forced Compact, stable full-32-bit order, fixed formal
resolution, and the same binary within each endpoint cohort. Every formal
ticket closes with exact `S/C/D` evidence and no failure.

| Endpoint | Cohort | Preproject / PostSort mean completion | Exact image gate |
| --- | --- | ---: | --- |
| M4 Metal | 6 balanced AB/BA pairs, 20 warmup + 80 measured | median `0.83405`, 95% bootstrap CI `0.81787--0.86388` | all 12 PNGs byte-identical, SHA `0291965b...` |
| Chrome/WebGPU on M4 | 4 balanced AB/BA pairs, 20 + 80 | pair ratios `0.87552/0.84455/0.88351/0.88042`, median `0.87797` | all 8 PNGs byte-identical, SHA `9f602220...` |
| A065/Adreno 730 | 2 interleaved descriptive AB/BA pairs, 20 + 80 | queue completion `213.648/401.465 ms`, ratio `0.53217` | rendered Truck/background region unchanged; whole screenshot differs only in Android UI timing |

The valid clean evidence identity is `7cabb6e`. The earlier
`desktop-m4-truck-producer-ab-006e37e` attempt is excluded. Android's per-run
`pairing` metadata is null, so its cohort is a controlled descriptive A/B, not
a self-contained formal paired statistic. All Android runs use identical
source, APK/native hash, camera and resolution and report thermal status 0.

## Retain decision

Preproject is retained: it is production-reachable, transactional,
count-observable, image/order exact, and materially faster in all currently
tested GPU cohorts. It is not promoted globally in this branch because its
admission is intentionally narrower, its graph adds lazy resources, it has
only complete-Truck A/B, and there is no producer-level Adaptive/fallback
controller.

The next policy unit must therefore be a whole execution plan, not an
independent producer toggle:

1. `PostSort + Candidate`;
2. `PostSort + Compact`;
3. `Preproject + Compact`.

After selecting the best supported GPU plan with measured completion,
hysteresis, cooldown, periodic reprobe and transactional fallback, the outer
CPU/GPU Adaptive layer can compare that winner against CPU. No point-count
threshold or quality fallback follows from the current evidence.
