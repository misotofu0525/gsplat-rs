# Findings: Adaptive Direct Ordering Experiment

## Accepted conclusions

1. **Do not remove CPU ordering.** On the tested Adreno 730, CPU won all 18
   interval-1 and all 9 interval-2 paired ladder comparisons. The original
   roughly 200k CPU decision is supported on this device.
2. **Do not encode that result as a point threshold.** Full Kitsune and Flowers
   anchors differ materially from similar-count Truck tiers, and cadence changes
   the GPU deficit. Device, scene, candidate distribution, refresh cadence,
   draw pressure, driver, and load all matter.
3. **Retain the GPU implementation as a baseline, not as the default.** It is
   deterministic, portable, resident, and fallback-safe, but its 25 passes and
   serial global prefix are not production-competitive on this mobile GPU.
4. **Retain bounded runtime measurement.** Adaptive correctly probes and
   rejects GPU here, periodically reconsiders, and does not require a hard
   point-count rule. Its exploration cost is measurable and must remain bounded.
5. **Treat capacity as the next competitive blocker.** Direct/SH3 reaches a
   745,654-splat mobile binding limit. Million-point release support requires
   active-atlas/paging work before sorter tuning can help those scenes.
6. **Measure complete strategies.** The retained values answer which current
   product path to present; they do not isolate CPU radix versus GPU radix
   kernel duration.

## Path differences that constrain interpretation

- CPU: near/far candidate construction → stable CPU radix → compact source-ID
  upload → Direct candidate draw.
- GPU: device key generation → stable 8-pass radix → no ID upload/readback →
  Direct sort-all/draw-all with shader clipping.
- Both share resident scene attributes and Direct raster shaders.
- CPU candidate count happened to equal source count in the retained traces,
  but the semantics are still different and can diverge on another camera.
- GPU preprocess/sort/submit/complete phases are unavailable and serialized as
  `null`. `frame_wall_ms` is caller wall/queue-pressure evidence, not a GPU
  timestamp.

## Adaptive facts

- Separate 16-sample refresh/reuse p75 windows avoid long intervals hiding the
  refresh cost.
- Cadence score is `(refresh + (interval - 1) * reuse) / interval` when both
  sample types are available.
- Bootstrap/probe/hysteresis/backoff constants are fixed and tested.
- Sampling is attributed to the backend actually presented, including reuse
  frames after a policy state change.
- The state field describes the policy after observing the current frame; it
  may describe the next refresh while the current/reuse order came from the
  previous backend.
- Android warmup frames train the policy but are not stored in the artifact.
- There is no explicit point-count or thermal-status input. Performance drift
  is observed indirectly at bounded probe points.

## API findings

- v0.1 C ABI structure/layout and Android AAR public API remain unchanged.
- Sort-stat flags gain backend/fallback/state meanings; an Android-example-only
  selector is intentionally absent from the public header.
- Public Rust API is widened with backend/state enums, output fields, accessors,
  and errors. This is an experimental pre-1.0 expansion and must not be called
  "no public API change."

## Dataset research decisions

- Official INRIA pretrained archive:
  `https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/datasets/pretrained/models.zip`
- Official implementation:
  `https://github.com/graphdeco-inria/gaussian-splatting`
- Range-download reference used for archive discovery:
  `https://github.com/nerfstudio-project/gsplat/blob/main/examples/download_3dgs_paper_scenes.py`

Truck was selected because one official 2.54M-splat file can produce a dense,
deterministic scaling ladder while preserving complete SH3 records. The source
repository's research license does not establish pretrained model/upstream
dataset redistribution rights, so the files are local-only and marked
`NOASSERTION`. Kitsune (CC0) and Flowers (CC BY 4.0) remain independent anchors.

## Rejected or deferred claims

- Rejected: GPU is universally better above a particular point count.
- Rejected: this experiment measures a pure sort-kernel crossover.
- Rejected: zero-valued GPU phase fields mean zero GPU cost.
- Rejected: a shared implementation plus compilation proves runtime parity on
  Metal/WebGPU.
- Rejected: the branch already matches or beats competitor renderers.
- Deferred: iOS/desktop/browser tier runs, timestamp queries, explicit thermal
  policy input, million-point release paging, fast-motion image error, and
  side-by-side competitor evaluation.

## Evidence pointers

- Full report: `report.md`
- Machine-readable aggregates: `android-results.json`
- Local raw retained artifacts: `target/android-sort-benchmarks/retained-v2-*`
