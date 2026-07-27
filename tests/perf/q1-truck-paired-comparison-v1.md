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
  and exactly five unique pairs; both references bind the same independently
  produced Direct-f32 authority receipt by relative path and SHA-256, and the
  references are inside the canonical schedule hash rather than mutable side
  input;
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

The common reference is not an arbitrary pair of hash-consistent PNGs. Before
any endpoint image or timing evidence is admitted, both schedule entries must
bind one accepted `gsplat-q1-direct-f32-reference/v1` receipt inside the series
root. The authority must be blocker-free and prove:

- one clean full repository commit, equal to the frozen endpoint commit, and a
  generation timestamp no later than schedule predeclaration;
- the locked Cargo, Rust 1.93 toolchain and complete producer-source identities;
- an immutable release desktop binary identity plus the matching retained
  binary copy inside the authority directory (the original build path need not
  continue to exist);
- the exact complete Truck bytes/count/SH3 source and the frozen trace file,
  semantic hash, poses and intrinsics for exactly views 0 and 1;
- Direct wide-f32, CPU ExactFull32 stable ordering, SortedAlpha and wgpu
  Direct GlobalQuads, with sampling, LOD, partial publication, dynamic
  resolution and upscaling disabled;
- `source = decoded = encoded = resident = addressable`, unchanged SH3, and
  per-view `0 < drawn = visible <= source`; and
- 1920x1080 RGBA8 top-left readback whose retained PNG and decoded-RGBA hashes
  exactly match each schedule view.

The validator reads the retained binary and images from the authority root; it
never relies on the producer's now-disposable absolute Cargo target path. A
self-consistent replacement PNG without that receipt and predeclared binding
is rejected. Accepted, Rejected, and Deferred series results retain the common
authority receipt, commit and binary identities so the image oracle cannot be
detached from the endpoint result.

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

## One-shot series orchestration

`collect-q1-truck-paired-series.py` is the sole finite collection owner once
both endpoint producers have been integrated and its **exact integrated SHA**
has passed fixed-SHA review. It does not duplicate either renderer collector.
The default-safe route is print-only:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 \
  tests/perf/collect-q1-truck-paired-series.py \
  --dry-run \
  --series-root /absolute/fresh/series-root \
  --series-id <unique-series-id> \
  --collection-session-id <one-session-id> \
  --seed <declared-integer> \
  --chrome /absolute/path/to/Chrome \
  --gsplat-wasm-package /absolute/repo-local/quality-exact-package \
  --reference-trace-0 /absolute/reference/view-0.png \
  --reference-trace-1 /absolute/reference/view-1.png
```

`--dry-run` (also spelled `--print-only`) reads the two reference identities
and prints the immutable schedule plus all 30 commands. It creates no series
directory, request, artifact or browser process.

Formal execution replaces `--dry-run` with `--execute` and adds the full
40-character `--reviewed-sha`. Before the first browser action, the collector
atomically claims the absent series root and writes:

- `schedule-declaration.json`, containing the two reference identities and
  exactly five seeded counterbalanced pair declarations;
- `commands.json`, containing all 30 producer argv/environment/output records;
- every control request, every gsplat-rs pairing context and every throughput
  control-binding source path.

Formal execution also writes `formal-execution-lock.json`. It binds the full
reviewed SHA and clean-tree state; the actual Chrome executable; the
quality-exact Wasm JS, Wasm binary and build receipt; Truck and trace inputs;
both fully decoded 1920x1080 non-interlaced RGBA8 references; endpoint
producer/runtime module trees; the canonical artifact validator; this Q1
validator; the locked image tool; and the installed production dependency
closure rooted at `puppeteer-core`, resolved from `package-lock.json` without
cache, test or temporary files. `commands.json` contains the complete
child environment rather than overrides, and its file digest is part of the
formal lock and final schedule. Undeclared Node, Python, npm, Chrome,
PlayCanvas, gsplat-rs and WebGPU host controls are not inherited.
The postprocess environment adds only the exact locked `CHROME_PATH`; image
tool output and the retained comparison receipt bind its executable path and
SHA-256. All child roles have generous finite safety timeouts predeclared in
`commands.json` and the formal lock. They are liveness bounds, never a frame
time or performance gate; expiry stops the one-shot series and cannot trigger
an automatic retry. Each child runs as a new process-group leader; timeout
cleanup records stdout/stderr and the TERM/grace/KILL/reap result in logs and
the root blocker. PID/PPID/PGID/start snapshots retain detached child identity;
normal exit with a surviving child is also a terminal process-tree failure.
The production dependency closure includes installed
optional and peer runtime dependencies, permits absent platform-only optional
packages, and rejects missing required or installed-but-unlocked modules.
Requiredness comes only from package-lock; installed package names, versions,
dependency declarations, directory containment and non-symlink identity must
match without overriding it.

Each pair then runs its declared first endpoint followed by its second. Within
an endpoint, trace-0 control, trace-1 control and throughput run exactly once.
The orchestrator validates each control before hashing its native
`manifest.json`; only then does it materialize the PlayCanvas throughput
request or expose the same two gsplat-rs control directories to its producer.
The resolution receipt retains both manifest hashes and configuration digests.
Before either throughput invocation, both controls are re-admitted through the
canonical benchmark and Q1 endpoint contract. This checks blocker absence,
run identity, endpoint/role/trace, configuration, protocol, schedule,
pair/order/position, collection session and manifest digest. Merely observing
that two controls share a configuration digest is insufficient.

After all 30 commands complete, the orchestrator runs the locked image tool,
writes the evidence-bearing `schedule.json`, and delegates the only
quality/performance decision to
`validate-q1-truck-paired-comparison.py`. It does not compute a winner. Any
producer, canonical artifact, image-materialization or final-admission command
failure stops the attempt immediately and writes root `blocker.json`; it never
retries or reuses the claimed root. A later operator-authorized attempt must
use a different path and is not authorized by this script.

After the twentieth image comparison but before `schedule.json` is published,
the owner re-hashes every frozen input and rechecks clean HEAD. The final
validator reads and joins that post-run receipt, command digest, reviewed
commit, browser digest and exact Wasm hashes to endpoint evidence. Any
integrated SHA that changes a producer, orchestrator or contract requires a
new fixed-SHA review before `--execute`; an earlier component review does not
authorize the integrated commit.
