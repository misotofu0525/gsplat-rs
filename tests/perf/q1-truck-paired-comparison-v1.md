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
- actual WebGPU backend plus immutable browser, adapter, driver-stack, limits, build
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

Every endpoint has a fresh control evidence set containing exactly two untimed
native artifacts (one per trace view), one fresh throughput artifact and two
image paths. A control entry freezes its trace index, artifact path and native
manifest SHA-256. The image for that trace may reference that artifact only; it
does not copy a renderer receipt into the schedule. Paths must be relative to
the schedule, remain inside its root, and may not be reused. Run IDs must also
be unique.

The two control artifacts are excluded from pair timing. Actual throughput
start/end timestamps must prove the declared AB/BA endpoint order and
non-overlapping pair order; labels alone do not prove randomization.

## Control and throughput separation

Both endpoint artifacts use the canonical `manifest.json`, `frames.jsonl` and
`summary.json`. The incremental manifest object is `q1_comparison`.

Each control artifact has:

- `artifact_role = control` and `performance_evidence = false`;
- exactly one trace-selected live-camera and successful-present capture;
- PlayCanvas count scope `full_membership_v_c_d_unavailable`; or
- gsplat-rs count scope `exact_v_c_d_control_only`, with exact `C <= V <= S`
  and Compact `D = C` on every frame.

The throughput artifact has:

- `artifact_role = throughput` and `performance_evidence = true`;
- a content binding to the control run ID, manifest hash and configuration;
- no copied per-frame `V/C/D` values;
- the common terminal-window receipt below.

Both controls and throughput must have the same clean repository commit, required
build artifact key set, actual JS/WASM/package file content hashes, environment
identity and configuration. Thermal `pre` and `post` receipts remain explicit
for every artifact; severe/critical states reject admission. All five pairs
must retain the same non-thermal identities. A diagnostic readback from control is never silently charged
to or copied into the timed comparator.

WebGPU does not expose a portable driver version, so the macOS endpoint never
relabels `GPUAdapterInfo.description` as one. Apple ships the Metal driver stack
with the OS; the producer records the pre/post-stable `sw_vers -buildVersion`
as its explicit `apple_metal_os_build` driver-stack identity. The browser binary
hash separately binds Chrome's Dawn implementation. The browser process
argument receipt is derived from actual child-process argv; only the ephemeral
profile path and debugging-port value are normalized, with their indices and
kinds retained for admission to verify before comparing the cross-run digest.

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

Each control image names its trace-specific native control artifact by path and
manifest hash. A host-owned `host_admission_join` binds that artifact identity,
the schedule PNG path/hash, decoded raw RGBA8 hash and dimensions. It is not a
renderer receipt and does not prescribe one common PNG encoder.

For gsplat-rs, the validator reads the actual selected
`frames.jsonl[].capture_depth_precision`. That terminal frame must retain the
real renderer-owned
`capture_depth_precision` receipt already emitted by the Web Surface capture
path: scene/camera/viewport/contract/plan-set/order generations, actual plan,
presentation sequence, dimensions, depth profile and `rgba8_sha256`. The
renderer owns RGBA bytes and their same-present identity; it does **not** own a
PNG hash. Admission verifies its frozen trace, camera, presentation and plan
identity, decodes the schedule PNG, and requires the raw RGBA digest to equal
the renderer receipt. There is no invented gsplat-rs PNG materialization
schema.

For PlayCanvas, the validator reads the producer only from its native manifest
locations: `presentation_capture.renderer_capture`, the last presentation
frame's `renderer_capture_copy`, `presentation_capture.queue_drain`, and the
terminal camera receipt. The frozen producer is
`gsplat-playcanvas-webgpu-renderer-capture/v1` /
`playcanvas_webgpu_copy_texture_to_buffer`; its raw camera JSON is hashed before
parsing so Python never guesses JavaScript serialization. The separate real
`gsplat-playcanvas-renderer-capture-materialization/v1` receipt must bind its
RGBA file, PNG file, byte lengths and hashes. The top-level duplicate capture
object is checked for equality but is not treated as a second producer.

If either trace-specific PlayCanvas control lacks that real native producer,
the candidate must say so explicitly. Host pixels alone then remain diagnostic,
and the whole series is candidate-only `Deferred` with `performance=null`.

The PNG must fully decode as non-interlaced RGBA8 at 1920x1080; an IHDR-shaped
header is not an image. Its comparison
receipt has schema `gsplat-q1-reference-image-comparison/v1` and binds:

- the raw reference and candidate SHA-256 values;
- `tests/perf/compare-image-ssim.mjs` and its content SHA-256;
- the trace view, dimensions, metric and predeclared SSIM threshold;
- a finite score in `[0, 1]`, which is checked against SSIM recomputed from the
  decoded reference and candidate bytes with the repository's locked algorithm.

Until both real producers exist, image scores are diagnostic inputs only. They
do not become an admitted quality pass/miss or unlock a performance result.

## Result semantics

The result schema is `gsplat-q1-truck-paired-result/v1`.

- malformed, missing or mismatched evidence: `Rejected`,
  `evidence_admitted=false`, `performance=null`, `retry_authorized=false`;
- structurally valid candidate with either real renderer producer unavailable:
  `Deferred`, `evidence_admitted=false`, `candidate_evidence_valid=true`,
  `performance=null`, `retry_authorized=false`.

When all ten trace-specific PlayCanvas controls and all ten gsplat-rs controls
carry their respective real producers and pass the same host joins, this
revision may admit the predeclared series and emit a finite Accepted/Rejected
comparative verdict. It cannot report a winner or quality pass from host-only
PlayCanvas pixels.

- admitted image miss: finite `Rejected`, no performance comparison, no retry;
- admitted image pass with slower gsplat-rs paired median terminal mean: finite
  `Rejected`, measured values retained, no retry;
- admitted image pass with gsplat-rs no slower on the paired median terminal
  mean: `Accepted` for the named near-contract only.

There is deliberately no required lead percentage or “must beat PlayCanvas”
completion gate. Deferred is not a tuning or automatic-rerun trigger.

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

A structurally valid Deferred candidate exits zero without a performance claim.
Malformed evidence exits two after writing a fail-closed Rejected result when
the output is fresh. Existing output is immutable and is never overwritten.
