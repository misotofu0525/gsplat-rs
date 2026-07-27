# Q1 Truck Paired Comparison v1

This schema is the offline admission and finite-decision boundary for the Q1
same-Chrome WebGPU isolation comparison. It replaces neither the canonical
benchmark artifact schema nor the renderer collectors. Every referenced
artifact must first pass `validate-benchmark-artifacts.py`; this validator then
checks only the additional Q0 pairing, common-terminal and image obligations.

It does not run Chrome, render a frame, retry a failed attempt, or claim a
winner from incomplete evidence.

## Frozen workload

- complete INRIA Truck source: 2,541,226 splats, source and resident SH3;
- exact committed source bytes and SHA-256;
- the two-view Truck trace, applied as a sequence before update/order/project;
- requested, canvas, internal-render and presented size all `1920x1080`, DPR 1;
- 20 warmup frames followed by 80 measured frames;
- LOD, sampling, partial publication, dynamic resolution and upscaling off;
- pinned PlayCanvas `2.21.0-beta.14`, full upstream revision, runtime revision
  and package integrity;
- actual WebGPU backend plus immutable browser, adapter, driver, limits, build
  artifact hashes and one collection-session identity.

The gsplat-rs cell is actual GPU Preproject ordering plus Compact exact
contributor drawing. This is a near-contract comparison: PlayCanvas may leave
post-projection `V/C/D` unavailable. The validator never converts its complete
source membership into inferred draw counts.

## Predeclared series

The schedule is one JSON object with schema
`gsplat-q1-truck-paired-comparison/v1`. It contains:

- a unique `series_id`;
- the frozen `protocol` object;
- `schedule.seed`, `schedule.predeclared_at_utc`, two reference-image receipts,
  and exactly five unique pairs; the references are inside the canonical
  schedule hash rather than mutable side input;
- a counterbalanced `playcanvas-first` / `gsplat-rs-first` order whose counts
  differ by no more than one;
- two immutable 1920x1080 reference PNG identities;
- five evidence pairs matching the predeclared IDs and order.

Every endpoint has a fresh control artifact, a fresh throughput artifact and
two terminal-present-bound images. Every endpoint image and comparison path and
content hash is unique across all five pairs. Paths must be relative to the
schedule, remain inside its root, and may not be reused. Run IDs must also be
unique.
Actual start/end timestamps must prove control before throughput, the declared
AB/BA endpoint order, and non-overlapping pair order; labels alone do not prove
randomization.

## Control and throughput separation

Both endpoint artifacts use the canonical `manifest.json`, `frames.jsonl` and
`summary.json`. The incremental manifest object is `q1_comparison`.

The control artifact has:

- `artifact_role = control` and `performance_evidence = false`;
- two live-camera and successful-present receipts, one per trace view;
- PlayCanvas count scope `full_membership_v_c_d_unavailable`; or
- gsplat-rs count scope `exact_v_c_d_control_only`, with exact `C <= V <= S`
  and Compact `D = C` on every frame.

The throughput artifact has:

- `artifact_role = throughput` and `performance_evidence = true`;
- a content binding to the control run ID, manifest hash and configuration;
- no copied per-frame `V/C/D` values;
- the common terminal-window receipt below.

Control and throughput must have the same clean repository commit, required
build artifact key set, actual JS/WASM/package file content hashes, environment
identity and configuration. Thermal `pre` and `post` receipts remain explicit
for every artifact; severe/critical states reject admission. All five pairs
must retain the same non-thermal identities. A diagnostic readback from control is never silently charged
to or copied into the timed comparator.

## Common terminal window

`q1_comparison.terminal_window` has schema
`gsplat-q1-webgpu-terminal-window/v1`. Both endpoints must prove:

```text
warmup queue drained
-> first measured camera input accepted
-> 80 continuous camera/update/order/project/render submissions
-> drawing stopped
-> final measured GPUQueue.onSubmittedWorkDone completed
```

The monotonic start/end, duration, before/after submission counters, stable
terminal drain and zero extra submissions are required. Per-frame observer
reads and dropped measured frames are forbidden. `summary.sustained_throughput`
must derive exactly from the terminal duration and `N=80`; frame-wall or host
call time cannot substitute for this boundary.

## Images

Each control image is bound to the same live-camera and presented-frame receipt
as its trace view. The camera receipt binds the frozen trace ID, trace content,
trace-frame pose/intrinsics and camera revision. The presentation receipt binds
the control run, canonical terminal-frame hash, camera revision and presentation
sequence. Opaque arbitrary digests are not accepted.

The PNG must fully decode as non-interlaced RGBA8 at 1920x1080; an IHDR-shaped
header is not an image. Its comparison
receipt has schema `gsplat-q1-reference-image-comparison/v1` and binds:

- the raw reference and candidate SHA-256 values;
- `tests/perf/compare-image-ssim.mjs` and its content SHA-256;
- the trace view, dimensions, metric and predeclared SSIM threshold;
- a finite score in `[0, 1]`, which is checked against SSIM recomputed from the
  decoded reference and candidate bytes with the repository's locked algorithm.

A quality miss is admitted as a finite quality observation but is Rejected
from a performance claim. The performance result is then `null`, and pair
receipts omit terminal means, deltas and ratios; it is not a tuning or rerun
trigger.

## Result semantics

The result schema is `gsplat-q1-truck-paired-result/v1`.

- malformed, missing or mismatched evidence: `Rejected`,
  `evidence_admitted=false`, `performance=null`, `retry_authorized=false`;
- admitted image miss: finite `Rejected`, no performance comparison, no retry;
- admitted image pass with slower gsplat-rs paired median terminal mean: finite
  `Rejected`, measured values retained, no retry;
- admitted image pass with gsplat-rs no slower on the paired median terminal
  mean: `Accepted` for the named near-contract only.

There is deliberately no required lead percentage or “must beat PlayCanvas”
completion gate. Accepted and Rejected are both terminal experiment outcomes.

The current historical unpaired PlayCanvas prerequisite and fixed-gsplat-rs
candidate are not input-compatible: they lack the five fresh pair identities,
the same terminal primitive, common image bindings and complete frozen
build/environment receipts. They must be rejected rather than retroactively
combined. The older 640x480, 120+3600 threshold comparator is a different
diagnostic contract and is not consumed here.

## Offline command

```bash
PYTHONDONTWRITEBYTECODE=1 python3 \
  tests/perf/validate-q1-truck-paired-comparison.py \
  /fresh/series/schedule.json \
  --output /fresh/series/result.json
```

An admitted finite Accepted or Rejected decision exits zero. Evidence admission
failure exits two after writing a fail-closed Rejected result when the output is
fresh. Existing output is immutable and is never overwritten.
