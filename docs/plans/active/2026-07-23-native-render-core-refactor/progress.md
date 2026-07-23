# Native Render Core Refactor Progress

> Program plan: [task_plan.md](task_plan.md)
> Architecture: [architecture.md](architecture.md)
> Verification: [benchmark_protocol.md](benchmark_protocol.md)

## Machine task-state registry

The architecture checker reads only the delimited records below, never prose
headings or the decision table. Future E/M/B/S/Q package ledgers use the static
active/completed paths declared in the source-architecture policy and contain
their own single block in the same format.

<!-- gsplat-program-task-states: begin -->
A0 = Accepted
A1 = Accept
A2 = Accepted
A3 = Accepted
A4 = Accepted
<!-- gsplat-program-task-states: end -->

## Program status

- Plan bundle: committed at `c478252246733f6dc209686091caf183e1ef7f06`.
- Implementation: Package A is in progress; A1 guardrails and the complete A2
  data/API, A3 scene/resource and A4 CPU order ownership extractions are
  root-accepted. A5a's strategy-free scan/radix leaves are also root-accepted;
  A5b's Direct/Resident stable-radix mechanics and A5c1's Projected scan reuse
  are now accepted behind those owners. A5c2's contributor compactor and A8a's
  offscreen lifecycle leaves are integrated and accepted. A7a/A7b now own
  immutable order receipts and bounded external evidence retention; A5d/A5e
  now own Direct Resident-visible and Preproject key/ID compaction. The parent
  A5, A7 and A8 packages remain open for later, separately activated slices.
- Current work package: A — responsibility extraction.
- Active package tasks: none while the accepted A7b/A5e lease is closed.
- Last completed tasks: A7b — bounded external evidence retention; A5e —
  Preproject key/ID compaction owner.
- Next eligible implementation: a separately governed A7c API-identity slice
  and A5f Resident-color GPU-owner slice may run concurrently only after a new
  exact activation commit records their disjoint allowlists.
- Integration branch: `codex/native-render-core-refactor`.
- Frozen source implementation closeout:
  `5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186`.
- Frozen plan/A0 parent:
  `c478252246733f6dc209686091caf183e1ef7f06`.
- Current checked-out branch when the plan was written:
  `codex/full-quality-native-rendering`.
- Current checked-out commit when the plan was written:
  `5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186`.
- `main` / `origin/main` observed at plan creation:
  `28f77d041d70fe3a11713590591ac57a122e1599`.
- Worktree was clean before this plan bundle was added.

The branch relationship is resolved in [a0-baseline.md](a0-baseline.md). A0
created a dedicated integration branch from the complete local full-quality
tip. No merge into `main`, rebase, cherry-pick, push or production change was
performed.

## Active-task and writer-lane rule

More than one row may be `Active` only when the architecture policy names the
exact task set and this file proves disjoint writer allowlists, no dependency on
unintegrated results, separate worktrees and an explicit root integration
order. Read-only audits need no writer lane. A production writer cannot start
until this file names:

- task ID;
- one hypothesis;
- dependency status;
- baseline commit;
- allowed files/scope;
- forbidden scope;
- hard gates;
- performance observations;
- required endpoints for the intended claim;
- maximum one performance/measurement correction after the first runnable
  implementation;
- known correctness issues (must be zero for Accept).

When the task ends, replace `Active` with exactly one of:

- `Accepted`;
- `Rejected`;
- `Deferred`.

The same closeout change must add exactly one normalized record to that
package ledger's machine task-state block. Narrative state text and the
decision table are human-readable mirrors, not checker inputs.

Then record the code commit (if any), evidence paths, commands, result and next
eligible task. Do not rewrite the architecture in this ledger.

## Cross-cutting source-size guardrail correction

- Corrected: 2026-07-23 after explicit owner review.
- No fixed physical line count is a task requirement or writer instruction.
  Existing checker numbers are diagnostic notices and legacy checkpoints only.
- A cohesive file may exceed a diagnostic threshold when the closeout explains
  its responsibility, dependency direction, test boundary and why another
  split would make ownership worse. No task may split or move tests merely to
  satisfy a count.
- A new multi-thousand-line owner triggers explicit responsibility review, but
  a finite documented exception remains a valid path for a cohesive owner; it
  cannot become an endless split loop.
- Dependency/ownership rules and the legacy giant-file growth ratchet remain
  hard. Historical A1 entries below record the then-current implementation;
  this section and the current policy are authoritative for later tasks.

## Current task

No production writer lease is active. Root is closing accepted evidence and
preparing the next exact disjoint activation; no parent task name by itself
grants write scope.

## Recently integrated tasks

### Parallel writer lanes — A7b and A5e

- State: Accepted.
- Activation baseline:
  `4f3a01faaa79132ce9f9f0402e48fb43ec3a6029`.
- A7b candidate `7969cce4cf87aa2dbf6c9e7bdb8e9be69e1c1a0e`
  integrated as `fe2400c`; its private `BoundedEvidenceRing<T>` now owns the
  five external observer queues while both lossless GPU-producer terminal
  queues remain unchanged. The final legacy session size is descriptive only:
  5,020 physical lines.
- A5e candidate initially received one fixed-SHA P1 because its new GPU leaf
  imported a legacy Resident constant. The same writer replaced that back-edge
  with a narrow caller-supplied value and amended the candidate to
  `ca28ec4f345b7afa7e6e511a065acb0cdd84382b`, integrated as `a6aecd0`.
  `PreprojectKeyIdCompactor` now owns only compact/finalize resources and
  encoding; projection, scan, radix, admission, stale draw reuse and pass order
  remain with the caller. The final legacy Preproject size is descriptive only:
  1,638 physical lines.
- Independent final fixed-SHA reviews reported P0/P1/P2 = 0 for both accepted
  candidates. Root reviewed both complete diffs and integrated in the declared
  order.
- Combined integrated verification passed architecture self-tests/checker,
  format and whitespace, locked workspace check/tests, all-target Clippy,
  Rustdoc, wasm32, required Apple Metal SortedAlpha conformance and C FFI smoke.
  Renderer results were 285 passed and 5 intentionally ignored; FFI reported
  `drawn=2`, `visible=2`.
- A5 and A7 remain open. Their next slices require a new exact activation and
  isolated user-visible writer tasks; collaboration subagents remain read-only.

### Parallel writer lanes — A5d and A7a

- State: Accepted

- Activation commit:
  `6b5a42cfdfc4d2f3c4916d8f83c6fe27490a1807`.
- Isolation: two user-visible Codex tasks, two worktrees, exact allowlists and
  no shared writable file. Collaboration subagents are read-only reviewers.
- Integration order: root fixed-SHA reviews and merges A7a first, verifies the
  A5d forbidden hashes, then merges A5d and reruns the combined gates.
- A5d owns only Direct Resident-visible compaction mechanics. A7a owns only the
  immutable order-receipt value cluster. Neither writer edits this ledger or
  the architecture policy.
- Source size is descriptive review context only. There is no fixed line-count
  target, split trigger or completion gate.

### A5d — Extract Direct Resident-visible compaction mechanics

- Parent task state: A5 remains open for later slices
- Subtask state: Accepted
- Activation baseline:
  `6b5a42cfdfc4d2f3c4916d8f83c6fe27490a1807`.
- Hypothesis: Direct's qualified Resident-visible compaction resources and
  three compute stages can move into a private two-phase GPU owner without
  moving target/capability policy, shared indirect arguments, radix/scan
  orchestration or timestamp/pass ordering.
- Exact writer allowlist:
  - `crates/gsplat-render-wgpu/src/direct_gpu_order.rs`;
  - `crates/gsplat-render-wgpu/src/gpu/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/gpu/visible_compact.rs`.
- Required ownership boundary:
  - a private seed owns the control buffer, group offsets and offset count and
    exposes only the references needed to construct the existing
    `StableFull32RadixProfile::ResidentVisible`;
  - after radix construction, binding the seed creates the private compactor
    owner for the three existing pipelines, bind group, dispatch, control and
    offsets;
  - the compactor encodes only its sentinel reset, keygen, compact and finalize
    mechanics;
  - `DirectGpuOrder` retains shared indirect args and reset/vertex setter,
    target allowlists, capability/error admission, key/radix owner, scan
    interleave, timestamps and the complete top-level pass order.
- Frozen source hashes at activation:
  - `direct_gpu_order.rs`: `cd2a087628f4ccac492e5fb65d24ddaa1116fde2569967c6b8937377ff5de584`;
  - `gpu/mod.rs`: `c2802f7a271d3a64f3a21a67764ca5d80fc227146d2d6a591d3681f81ab75e23`;
  - `gpu/radix.rs`: `51bf9c0ece413c9c84ed88fc8e7fac3c1fbc6cc7b673fab15f359572a5c54f7d`;
  - `gpu/scan.rs`: `08afa1daf9ec0fda0293a6cf467a14ffce2e9c24011d9b92bbf92704ce5eb6ab`;
  - `preproject_gpu.rs`: `5dfafabe06fa177b6c2d3ff3a32a5ed842fd05aac074774aba83fb48bf8d518f`;
  - `projected_quads_gpu.rs`: `84760e990707a77c971b6b830d19f112c1004995116bbbc0a4667fcd686d5d6c`.
- Frozen WGSL hashes:
  - Direct order: `4531b652f74f7ba9763256be9f151cef24225385d2f3a486d802f89d658f4f6a`;
  - prefix scan: `f58e3e47ef6175965f88c48e95f0df8eb1b380d847b7d5f65d45ad1a7537fc61`;
  - Resident radix8: `cb31ba454379eac6243bc77754095f566c11cbfe67303037dc17767aedfbebf7`;
  - Resident visible radix8:
    `3afe4e7c011172f973527f3130bd2ff8a89195938135a2eddb2da5cf92c583a5`;
  - Resident visible compaction:
    `e59efc1ae0cf3771d311bf97f36bafbe6a26dce5f35f330a76181cf10f71c526`.
- Forbidden: every WGSL file; `gpu/radix.rs`, `gpu/scan.rs`, Preproject,
  Projected, Scene/Resident/Surface/session/policy/telemetry/evidence/API/FFI,
  platform consumers, Cargo, benchmark and plan/policy files. No algorithm,
  precision, target allowlist, capability threshold, error text, resource
  size/usage/binding, label, entry point, clear range, dispatch, pass/timestamp
  order, V/C/D or performance change.
- Exact behavior sequence: zero count still returns before encoding; otherwise
  clear only the shared indirect instance count, clear the offset sentinel,
  visible keygen begins the keygen timestamp, existing visible scan runs,
  stable compact and finalize end the keygen timestamp, then existing radix
  consumes the same control buffer. The 32-byte control initialization,
  offset sizing, usages, bindings `0/1/2/3/4/5/6/9`, labels and entry points
  remain byte-for-byte equivalent.
- Hard gates: exact frozen hashes outside the allowlist; unchanged complete
  `gsplat-*` label multiset and compaction test inventory; format/diff,
  architecture, renderer/workspace tests, all-target Clippy, Rustdoc, wasm32
  and required Metal SortedAlpha conformance. No performance percentage or
  Android device run is required for this behavior-preserving ownership slice.
- Source size: reported only as review context; responsibility, dependency
  direction and preserved test seams determine acceptance.
- Candidate: `30f2cca8c7987358c3563e5ed04b3b7bcb213731`.
- Root integration: `8e7680b` after independent fixed-SHA review reported
  P0/P1/P2 = 0.
- Result: `ResidentVisibleCompactionSeed` and
  `ResidentVisibleCompaction` now own the existing control/offset resources,
  bindings and three compaction stages. Direct retains admission, indirect
  reset, scan/radix orchestration, timestamp boundaries and pass order.
- Verification: exact allowlist and frozen hashes, unchanged labels/entries/
  bindings/usages/test inventory, format/diff, architecture, renderer and
  workspace tests, all-target Clippy, Rustdoc, wasm32 and required Metal
  SortedAlpha conformance all passed in the writer and fixed-SHA review.
- Final source sizes are descriptive only: `direct_gpu_order.rs` 1,864 lines,
  `gpu/visible_compact.rs` 350 lines and `gpu/mod.rs` 23 lines.

### A7a — Extract the immutable order-receipt value cluster

- Parent task state: A7 remains open for later slices
- Subtask state: Accepted
- Activation baseline:
  `2e2e211` (`docs: accept parallel extraction slices`).
- Hypothesis: the closed order-receipt value cluster can move behind one
  private evidence leaf without moving transport, tickets, readback,
  invalidation, controller policy or the public crate-root API.
- Writer execution: one user-visible Codex task in an isolated worktree. A
  collaboration subagent may review the fixed candidate SHA but must not edit.
- Exact writer allowlist:
  - `crates/gsplat-render-wgpu/src/lib.rs`, only to add private
    `mod evidence;` while preserving the existing root export text and path;
  - new `crates/gsplat-render-wgpu/src/evidence/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/evidence/order.rs`;
  - `crates/gsplat-render-wgpu/src/gpu_telemetry.rs`, only to remove the five
    declarations below and re-export them from the private evidence facade.
- Exact moved declarations, with visibility, derives, docs, variant order,
  field names, field order and field types unchanged:
  - `SurfaceTimingSource`;
  - `SurfaceOrderMeasurement`;
  - `SurfaceOrderMeasurementFailureReason`;
  - `SurfaceOrderMeasurementFailure`;
  - `SurfaceCpuOrderMeasurement`.
- Frozen source hashes at activation:
  - `lib.rs`: `6f969ce3dc24186ba80ce555671b21ea5af7229fa52b129bb3b3a40affcd019e`;
  - `gpu_telemetry.rs`: `6ea2ff9ac75a31aee400f931d28ff93a6951212aae0f34b566a5f9c2afb98154`;
  - `projected_draw_telemetry.rs`: `3653a37f3132a0bab595185bbc46657ea6344c36572cdf788780c6eb9d33fc54`;
  - `gpu_producer_telemetry.rs`: `190fa41de9cbefd59c025c660456d8f89d931496f14fcfdc40b4b5ce5946fa54`;
  - `surface_presenter.rs`: `7b44a87f960dbf944b00d969477b8494afd9603d48229746866e3d6d0cc1e2aa`;
  - `surface_session.rs`: `7ed8b08dc13d5ccd97d4e6007de204211c7d208d00c894f5f9b6d363a99ef702`.
- Forbidden: Projected/producer receipt declarations; ticket namespaces and
  counters; slots/rings/poll/reservation; count sources; readback/query/map,
  callback, arm/cancel/fail/invalidate/poll/timestamp code; submissions,
  adaptive/controller state; `surface_presenter.rs`, `surface_session.rs`, API,
  FFI/header/bindings/examples/Cargo, GPU/raster/offscreen/shaders and this
  plan/policy. No observer ring or new public module/path.
- Hard gates: mechanical declaration equivalence; existing root imports,
  construction and pattern matching compile unchanged; private evidence path
  is not public; frozen forbidden hashes; format/diff, architecture,
  renderer/workspace tests, all-target Clippy, Rustdoc, wasm32 and C FFI smoke.
  No device run or performance percentage is required because behavior and
  execution are unchanged.
- Source size: reported only as review context; no fixed line count is a task
  requirement, split criterion or completion gate.
- Candidate: `07eed6f23de65e1e4d248ef5828b5da96eb48078`.
- Root integration: `ff7eace` after independent fixed-SHA review reported
  P0/P1/P2 = 0.
- Result: five immutable order-receipt value types now live behind the private
  `evidence` facade while their crate-root public paths remain unchanged.
  Tickets, rings, readback, invalidation, controller policy, Projected and FFI
  behavior did not move.
- Verification: exact declaration/path equivalence, frozen forbidden hashes,
  format/diff, architecture, renderer and workspace tests, all-target Clippy,
  Rustdoc, wasm32, C FFI smoke and required Metal SortedAlpha conformance all
  passed in the writer and fixed-SHA review.

## Last integrated tasks

### Parallel writer lanes — A5c2 and A8a

- State: Accepted
- Activated from:
  `962c5c2544c4d087a3a4203e14e41d3fea12f7d7`.
- Root governance correction:
  `14ef3a6d6b4ccb74c24e085e39720cc7d297ee24`.
- Integrated in the declared order:
  - A5c2 candidate `bee15d0a0de08e46158efc0ce1d22c6c5439fafc`
    via merge `57fa8e5`;
  - A8a candidate `fa4f01bf0d4a56301039c6273e485197faa7389f`
    via merge `cd4a618`.
- Both writers ran as user-visible Codex tasks in isolated worktrees. The
  collaboration subagent performed read-only fixed-SHA review only.
- Fixed-SHA review: both candidates reported P0/P1/P2 = 0, exact allowlists,
  preserved resource/pass/submit/map ordering, unchanged shader and public ABI
  boundaries.
- Combined gates: architecture checker/self-tests, format/diff, renderer and
  locked workspace tests, all-target Clippy, Rustdoc, wasm32, C FFI smoke and
  required Apple Metal SortedAlpha conformance all pass.
- Parallel execution: exact lanes `A5c2` and `A8a`; policy members, normalized
  write allowlists, activation commit, integration order and machine lane
  records are bound together. A parent task name alone grants no parallelism.
- Isolation: separate user-visible Codex tasks and separate worktrees from this
  exact activation commit; neither writer edits this ledger or architecture
  policy.
- Integration order: root reviews and merges A5c2 first. It then merges A8a
  only if A8a's frozen forbidden-file hashes still match; root reruns shared
  architecture and cross-platform gates on the combined SHA.
- File intersection: empty. A5c2 owns only Projected/GPU compaction files;
  A8a owns only `lib.rs` wiring and new `offscreen/` leaves.
- A7 remains a read-only audit while these writers run. Its implementation is
  sequential after A8a because both may require `lib.rs`.
- Source size: report final LOC for review, but no fixed number is a gate or a
  writer instruction.

### A5c2 — Extract the Projected strategy-free compute compactor

- Parent task state: A5 remains open for later slices
- Subtask state: Accepted
- Started: 2026-07-24
- Ended: 2026-07-24
- Candidate: `bee15d0a0de08e46158efc0ce1d22c6c5439fafc`.
- Integrated: `57fa8e5`.
- Result: `StableContributorCompactor` now exclusively owns contributor ranks,
  compact/finalize resources and encoding. Projected retains admission,
  preparation/publication, strategy choice and raster orchestration. The final
  owner files are 2,055 lines for `projected_quads_gpu.rs` and 252 lines for
  `gpu/compact.rs`; these counts are descriptive, not gates.
- Production baseline: `32b277e1181a76ac8442e263e5a0be39162edb39`;
  activation parent is the current ledger commit.
- Hypothesis: Projected's stable rank compaction and indirect-argument compute
  mechanics can move into one private GPU leaf without moving Candidate/Compact
  policy, optional graph admission/publication or either raster pipeline.
- Writer allowlist:
  - `crates/gsplat-render-wgpu/src/projected_quads_gpu.rs`;
  - `crates/gsplat-render-wgpu/src/gpu/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/gpu/compact.rs`.
- Frozen source hashes before activation:
  - `projected_quads_gpu.rs`: `9d332038f9e7b42aa36778f619b7621adac593cce85676aa84081951d5692b7d`;
  - `gpu/scan.rs`: `08afa1daf9ec0fda0293a6cf467a14ffce2e9c24011d9b92bbf92704ce5eb6ab`;
  - `gpu/mod.rs`: `e7adefec48555340b23fb429f838575db0831ee9061e1de6ceebca0770a4f8d1`.
- Required ownership boundary:
  - the new leaf owns contributor-rank storage, the 16-byte indirect args,
    compact/finalize compute pipelines and bind group, reset/encode mechanics
    and read-only buffer accessors;
  - Projected retains capacity/admission, prepared/publish transaction,
    Candidate/Compact resolution, projection/scan/pass orchestration and both
    raster pipeline/bind-group owners;
  - Direct and Preproject compaction graphs remain separate.
- Frozen behavior: contributor ranks are `capacity * 4` bytes with a four-byte
  minimum and `STORAGE | COPY_SRC`; indirect args remain four `u32` initialized
  to vertex count four and zero instance/base fields with identical usage;
  only instance-count bytes are cleared before Compact; compact dispatch is
  skipped for zero items and finalize is always one workgroup; bindings,
  labels, pass order and `D=C<=V` remain exact.
- Frozen WGSL hashes:
  - compact: `e8348a6535c39da1ebcd0a18d59fa3b84d36f865505b7a8f15e6d6f8eef01822`;
  - compacted draw: `d9d648dedd06c9af441691ec9a0adb441e2c2b4f45495e6788712076dc4b8024`;
  - project: `7c224f1f1e9380d8427da714fd17134203049bf96334b5ef7119efcba0eadd3d`;
  - Candidate draw: `ed48658f0008728368206b5c59637e42be580e861e17c7eef6f1f89a5ee71529`.
- Forbidden: every WGSL file; `gpu/scan.rs`, radix, Direct, Preproject,
  Resident/Scene/Surface/session/policy/telemetry/API/FFI/platform/Cargo,
  benchmark and plan/policy files; algorithm, resource, usage, binding,
  dispatch, labels, pass order, errors, target selection or performance change.
- Hard gates: frozen hashes/labels/resources; exact 13 Projected tests and zero
  ignored; A5a/A5b inventories unchanged; format/diff, architecture, locked
  renderer/workspace tests, all-target Clippy, Rustdoc, wasm32 and forced Metal
  conformance. No performance percentage is a completion gate.

### A8a — Extract the offscreen target/readback leaf

- Parent task state: A8 remains open for later Surface slices
- Subtask state: Accepted
- Started: 2026-07-24
- Ended: 2026-07-24
- Candidate: `fa4f01bf0d4a56301039c6273e485197faa7389f`.
- Integrated: `cd4a618`.
- Result: private `OffscreenTarget` owns texture/view/size and transactional
  reuse; the readback leaf owns row alignment, copy/map/poll and padding
  removal. `GpuRasterizer` still owns device/queue/pipelines, command ordering
  and submit. `lib.rs` is now 4,716 lines; this count is descriptive, not a
  completion condition.
- Baseline: current activation commit; worktree must start clean.
- Hypothesis: offscreen target allocation/reuse and synchronous RGBA8 readback
  can become private lifecycle leaves without changing Renderer ownership,
  submission timing, public API, error mapping or output bytes.
- Writer allowlist:
  - `crates/gsplat-render-wgpu/src/lib.rs` for private wiring and mechanical
    delegation only;
  - new `crates/gsplat-render-wgpu/src/offscreen/mod.rs`;
  - new `crates/gsplat-render-wgpu/src/offscreen/target.rs`;
  - new `crates/gsplat-render-wgpu/src/offscreen/readback.rs`.
- Frozen source hashes before activation:
  - `lib.rs`: `22a1e823cf3416b315734c0d7726dd5a07ea1d17d259318906d30659818fe2cf`;
  - `surface_presenter.rs`: `7b44a87f960dbf944b00d969477b8494afd9603d48229746866e3d6d0cc1e2aa`;
  - `surface_session.rs`: `7ed8b08dc13d5ccd97d4e6007de204211c7d208d00c894f5f9b6d363a99ef702`.
- Required ownership boundary:
  - `OffscreenTarget` owns output texture/view/size and same-size reuse;
  - readback leaf owns row alignment, copy buffer, copy/map/poll and padding
    removal, while Renderer supplies device/queue and keeps the public
    `readback_rgba8` signature;
  - adapter/device/queue, scene resources, render-path selection, ordering,
    submits, stats and `offscreen_device_limits` remain in the legacy owner.
- Forbidden: `surface_presenter.rs`, `surface_session.rs`, telemetry, GPU
  primitive files, shaders, API/FFI/JNI/Swift/Web/examples, Cargo, benchmark
  schemas and plan/policy files; no completion token, new public type, submit
  reordering, async readback, error/string/format/usage or behavior change.
- Hard gates: frozen forbidden hashes; offscreen count/image/conformance and
  unsupported-dimension/4K tests; public Rust and C ABI unchanged; format/diff,
  architecture, locked renderer/workspace tests, all-target Clippy, Rustdoc,
  wasm32, FFI smoke and forced Metal conformance. Device/long performance runs
  are not required for this behavior-preserving ownership slice.

### A5c1 — Reuse the accepted scan owner for Projected contributor counts

- Parent task state: A5 Active
- Subtask state: Accepted
- Started: 2026-07-24
- Ended: 2026-07-24
- Baseline commit:
  `e687614fbd9b973d060904978d2b5aebde898959` (`docs: accept A5b
  stable radix extraction`).
- Exact source baseline before production edits:
  - `projected_quads_gpu.rs`: 2,360 physical LOC;
  - `gpu/scan.rs`: 416 physical LOC;
  - `gpu/mod.rs`: 15 physical LOC;
  - architecture grandfather baseline for `projected_quads_gpu.rs`: 2,358.
- Frozen source identities:
  - `projected_quads_gpu.rs`:
    `1dc7b2f7b9d5e0b99e6d968f8c952a66c303308359ff8c5d96030f9f49d4c217`;
  - `gpu/scan.rs`:
    `f70a0ec9acd056e49a181cd94678eca05512b9f73dd2d52a05560ad7f5c0b4fb`;
  - `gpu/mod.rs`:
    `ff673dfe17496ea287e210b7e7b821dab42075f55b4254ef4cff61f5ef6e1c6a`.
- Frozen shader identities:
  - `gpu_prefix_scan.wgsl`:
    `f58e3e47ef6175965f88c48e95f0df8eb1b380d847b7d5f65d45ad1a7537fc61`;
  - `projected_quads_project.wgsl`:
    `7c224f1f1e9380d8427da714fd17134203049bf96334b5ef7119efcba0eadd3d`;
  - `projected_quads_compact.wgsl`:
    `e8348a6535c39da1ebcd0a18d59fa3b84d36f865505b7a8f15e6d6f8eef01822`;
  - `projected_quads_draw.wgsl`:
    `ed48658f0008728368206b5c59637e42be580e861e17c7eef6f1f89a5ee71529`;
  - `projected_quads_draw_compacted.wgsl`:
    `d9d648dedd06c9af441691ec9a0adb441e2c2b4f45495e6788712076dc4b8024`.
- Hypothesis: the Projected contributor counter's duplicate hierarchical
  prefix-scan construction and forward/reverse encode loops can delegate to
  the accepted `gpu::scan` owner while preserving every label, allocation,
  usage, pass and exact V/C/D result. Projected remains the sole owner of
  projection, Candidate/Compact admission, transactional publication,
  contributor rank compaction and both raster pipelines.
- Read-only scope conclusion:
  - Direct Resident-visible, Projected contributor and Preproject key/ID
    compaction are not one interchangeable ABI or resource graph;
  - Direct is an S-to-V producer coupled to radix control, target allowlists
    and timestamp boundaries; Projected is a sorted V-rank-to-C-rank producer
    whose Candidate path keeps D=V; Preproject has separate S/V/C identity and
    must preserve stale D on non-refresh frames;
  - therefore A5c is split. A5c1 removes only the proven duplicate scan. Later
    compact owners require separate plan-only activation per consumer rather
    than one generic graph.
- Allowed production scope:
  - `crates/gsplat-render-wgpu/src/projected_quads_gpu.rs`;
  - `crates/gsplat-render-wgpu/src/gpu/scan.rs` for a strategy-free profiled
    constructor, separate forward/reverse encoding and exact-count access;
  - `crates/gsplat-render-wgpu/src/gpu/mod.rs` for the minimum crate-private
    re-export;
  - existing Projected tests remain in place; no test is moved for size.
- Required responsibility boundary:
  - the existing `GpuPrefixScan::new` and `encode` behavior used by A5a and
    Preproject remains source-compatible and resource-identical;
  - a profiled scan may accept static labels and sums usage, expose forward and
    reverse phases, and expose the final count buffer/offset without importing
    Projected, Scene, Surface, policy or telemetry types;
  - `ProjectedQuadsGpu` retains scan capability admission, downlevel sentinel
    fallback, Candidate/Compact choice, prepared/publish lifecycle, dispatch
    ordering, compaction resources and raster ownership.
- Forbidden scope:
  - every WGSL file; `direct_gpu_order.rs`, `preproject_gpu.rs`,
    `gpu/radix.rs`, `lib.rs`, Resident/Scene, Surface/session/presenter,
    telemetry/policy, raster helpers, API/FFI/platform/example/Cargo/benchmark
    files and architecture policy/ledger files in the writer worktree;
  - creating `gpu/compact.rs`, moving any compact/finalize/draw pipeline,
    changing Candidate/Compact behavior, V/C/D semantics, transactional
    optional-resource publication, limits, errors, target admission or shader
    math;
  - changing scan workgroup size, hierarchy, scratch sizes/usages, uniform
    stride, bind groups, dynamic offsets, pass labels/order, dispatch shape or
    exact-count location; line-count-only splitting or test relocation.
- Hard gates:
  - all five frozen shader hashes and forbidden consumer files remain
    byte-identical;
  - the complete `gsplat-*` resource/pass label set for Projected plus scan is
    identical, and sums/params/binding/usage/dispatch receipts match the
    baseline;
  - Candidate encodes project plus forward count only and still draws D=V;
    Compact adds reverse offsets, stable rank compact and finalize in the same
    order and still proves D=C<=V;
  - adapters below the current scan floor retain the sentinel exact-count path
    and never lose Candidate; optional Compact construction/publication remains
    transactional;
  - the exact 13 Projected tests and zero ignored tests remain, with no weakened
    assertion; A5a/A5b scan/radix inventories also remain unchanged;
  - fixed-SHA review and root integration run format, whitespace,
    architecture checks, locked renderer/workspace tests, all-target Clippy,
    Rustdoc, wasm32 and forced Apple M4 Metal conformance. Root alone adjusts a
    grandfather baseline after semantic acceptance.
- Flexible LOC rule: no 800-line gate applies. This task succeeds only if it
  removes duplicate scan ownership without inventing a generic compaction
  framework; final cohesive owner sizes are review evidence, not pass/fail
  numbers.
- Performance observations: none. A5c1 is behavior-preserving ownership work
  and makes no speed claim.
- Required endpoints for claim: Apple M4 executes existing Projected image and
  count oracles plus forced SortedAlpha conformance; wasm32 compiles. A065 stays
  at the A5 package boundary because shader bytes and product routing cannot
  change here.
- Performance correction used: no
- Known correctness issues: none
- Closeout requirement: one isolated writer produces a fixed SHA; root reviews
  the exact graph and integrated gates before Accept/Reject/Defer. No later
  compaction slice activates automatically.
- Writer commit:
  `74206fde135e33470dd0d22cc66261cbfb952c86` (`refactor: reuse prefix scan
  for projected contributors`), with exact parent
  `23569d9f627deaa370ec7404f71fc93f62816efe`.
- Integration merge commit:
  `32b277e1181a76ac8442e263e5a0be39162edb39` (`merge: integrate A5c1 shared
  projected scan`).
- Fixed-SHA review:
  - independent review concluded P0/P1/P2 = 0/0/0;
  - only the three allowed files changed; all five frozen WGSL hashes and all
    forbidden files are byte-identical;
  - the complete 52-entry `gsplat-*` label/resource multiset is unchanged;
  - sums bytes/usage, 16-byte params ABI, uniform stride, bindings, dynamic
    offsets, two-dimensional dispatch, pass order and final count location are
    exact;
  - Candidate remains projection plus forward scan with D=V; Compact adds
    reverse offsets, stable rank compaction and finalize with D=C<=V;
    downlevel sentinel and prepare/publish transactionality are unchanged.
- Final ownership and physical LOC:
  - `projected_quads_gpu.rs`: 2,194 LOC, retaining Projected admission,
    Candidate/Compact selection, compaction, publication and raster ownership;
  - `gpu/scan.rs`: 495 LOC, one profiled hierarchical prefix-scan owner with
    forward/reverse phases and exact-count access;
  - `gpu/mod.rs`: 15 LOC, private facade.
- Root integrated gates:
  - PASS architecture checker/self-tests after lowering only the
    `projected_quads_gpu.rs` current ratchet from 2,358 to 2,194;
  - PASS format, whitespace, locked workspace check/tests; renderer result is
    281 passed / 5 ignored and Projected remains 13 passed / 0 ignored;
  - PASS all-target Clippy and Rustdoc with warnings denied;
  - PASS wasm32 `gsplat-web`, forced Apple M4 Metal SortedAlpha conformance and
    C ABI smoke (`drawn=2`, `visible=2`).
- Required endpoint decision: A065 stays at the A5 package boundary because
  this ownership-only slice changes no shader, product routing, algorithm or
  rendered set. No performance claim is made.
- Decision: Accept A5c1. A5 remains Active; A5c2 is eligible but inactive.

### A5b — Extract Direct/Resident stable-radix mechanics

- Parent task state: A5 Active
- Subtask state: Accepted
- Started: 2026-07-23
- Ended: 2026-07-24
- Production baseline commit:
  `3b726b0` (`docs: accept A5a GPU primitive extraction`).
- Exact source baseline before production edits:
  - `crates/gsplat-render-wgpu/src/direct_gpu_order.rs`: 2,855 physical LOC;
  - `crates/gsplat-render-wgpu/src/gpu/scan.rs`: 325 physical LOC;
  - `crates/gsplat-render-wgpu/src/gpu/radix.rs`: 810 physical LOC;
  - architecture grandfather baseline for `direct_gpu_order.rs`: 2,855.
- Frozen source/shader identities:
  - `direct_gpu_order.rs`:
    `e18e233a18bcfc68ff9f79dc8a649e8c40ae6ff4a1c427525ff55404625d104f`;
  - `direct_gpu_order.wgsl`:
    `4531b652f74f7ba9763256be9f151cef24225385d2f3a486d802f89d658f4f6a`;
  - `gpu_prefix_scan.wgsl`:
    `f58e3e47ef6175965f88c48e95f0df8eb1b380d847b7d5f65d45ad1a7537fc61`;
  - `resident_gpu_order_radix8.wgsl`:
    `cb31ba454379eac6243bc77754095f566c11cbfe67303037dc17767aedfbebf7`;
  - `resident_gpu_order_visible_radix8.wgsl`:
    `3afe4e7c011172f973527f3130bd2ff8a89195938135a2eddb2da5cf92c583a5`;
  - `resident_gpu_order_compact.wgsl`:
    `e59efc1ae0cf3771d311bf97f36bafbe6a26dce5f35f330a76181cf10f71c526`.
- Hypothesis: Direct's eight-pass stable 4-bit path and the qualified
  Resident four-pass stable 8-bit path can move mechanically behind the
  strategy-free `gpu::radix` owner and accepted `gpu::scan` primitive, while
  `DirectGpuOrder` remains the sole consumer/orchestrator and every target,
  resource, pass-order, count and error result remains unchanged.
- Dependencies: A2 Accepted and A5a Accepted at `3b726b0`. A5c, A5d, A6 and
  later tasks are inactive.
- Required responsibility boundary:
  - `direct_gpu_order.rs` retains `DirectGpuOrder`, all existing crate-private
    consumer signatures, `GpuOrderTimestampRange`, key generation/visibility,
    indirect-count reset, top-level timestamp/pass ordering and exact
    `DirectSceneError` mapping;
  - it also retains the two current target allowlists and
    `ResidentVisibleCompaction`, including its control/indirect handoff. Those
    are policy and A5c responsibilities, not radix mechanics;
  - `gpu/scan.rs` remains the one portable hierarchical prefix-scan owner;
  - `gpu/radix.rs` owns stable full32 radix mechanics: pass parameters,
    key/source-ID ping-pong, prefix/pass buffers, pipeline/bind-group
    construction, stable LSD passes, final-A contract and the optional legacy
    pair pack;
  - private children under `gpu/radix/` are allowed only when they express a
    real portable-vs-Resident algorithm/resource responsibility. They may not
    be created to satisfy a physical line number.
- Allowed production scope:
  - `crates/gsplat-render-wgpu/src/direct_gpu_order.rs`;
  - `crates/gsplat-render-wgpu/src/gpu/mod.rs`;
  - `crates/gsplat-render-wgpu/src/gpu/radix.rs` and responsibility-justified
    private children under `src/gpu/radix/`;
  - `crates/gsplat-render-wgpu/src/gpu/scan.rs` only for the minimum explicit
    label/usage/profile parameterization required to reproduce both accepted
    graphs byte-for-byte in resources and pass order. Its A5a API, graph and
    tests otherwise remain frozen;
  - existing tests may move only with the exact radix/scan mechanic they
    directly verify.
- Forbidden scope:
  - every WGSL file; `lib.rs`, `resident_gpu.rs`, `surface_presenter.rs`,
    `surface_session.rs`, `preproject_gpu.rs`, `projected_quads_gpu.rs`, Scene,
    data/API, FFI/JNI/Swift/Web/examples, Cargo, benchmark and architecture
    policy/ledger files in the writer worktree;
  - moving or changing `ResidentVisibleCompaction`, contributor/project
    compaction, V/C/D semantics, Resident SH/color resolve or canonical raster;
  - changing shader math, workgroup size, radix width/pass count, binding,
    buffer size/usage, dynamic offset, dispatch, ping-pong parity, final-A,
    compatibility allocation/omission, submit/map/readback, target allowlist,
    error text, timestamp interval, CPU/GPU/Adaptive policy or performance;
  - sharing merely similar code when exact resource/pass equivalence cannot be
    proved; line-count-only splitting, compression or test relocation.
- Hard gates:
  - all five frozen shader hashes remain identical and the A5a
    `ExternalPrefixRadix` API, byte plan, graph and test inventory do not
    change;
  - Direct and the four-binding fallback still execute eight stable 4-bit LSD
    passes; qualified macOS Resident still executes four stable 8-bit passes;
    every other target allowlist and fallback remains identical;
  - full32 ordering, equal-depth/source-ID stability, zero/one/non-power/tail
    handling, final IDs in A, legacy pair allocation/omission, SoA storage
    limit errors, exact indirect count and timestamp begin/end remain exact;
  - `lib.rs`, `resident_gpu.rs` and `surface_presenter.rs` are byte-identical to
    the baseline, proving existing consumers and crate-private paths did not
    migrate in this slice;
  - the exact baseline inventory of 16 `direct_gpu_order` tests and four
    ignored external/pressure oracles is preserved with no weakened assertion;
  - fixed-SHA review proves production mechanics are equivalent after only
    path, visibility and explicit leaf-input normalization;
  - format, whitespace, architecture checks, locked renderer/workspace tests,
    all-target Clippy and Rustdoc with warnings denied, wasm32 check and forced
    Apple M4 Metal conformance pass. Root alone updates the exact grandfather
    baseline after semantic review.
- Flexible LOC rule: there is no 800-line completion gate. A cohesive radix
  owner may exceed any review target; responsibility, dependency direction and
  executable tests decide the boundary. Only a new multi-thousand-line mixed
  owner requires a finite exception or redesign.
- Performance observations: none. A5b changes ownership only; FPS, elapsed
  milliseconds and competitor ratios are not acceptance gates or claims.
- Required endpoints for claim: Apple M4 executes the existing native GPU
  oracles and forced SortedAlpha conformance; wasm32 compiles. A065 is retained
  for the A5 package boundary because this slice cannot change product behavior.
- Performance correction used: no
- Known correctness issues: none
- Closeout requirement: one isolated writer produces a fixed SHA; root checks
  the exact inventory, hashes, mechanics and integrated gates before
  Accept/Reject/Defer. Acceptance activates no later A5 slice automatically.
- Writer commit:
  `835b01e6238e7c81b701ca540dffa7f9bd990cae` (`refactor: extract
  stable full32 GPU radix owner`).
- Integration merge commit:
  `0fd16a84cd8ad24d576afcd338a9e4a9eea024ad` (`merge: integrate
  A5b stable radix owners`).
- Fixed-SHA review:
  - the first candidate had one P2 documentation defect: the radix module
    overview described only ExternalPrefix after the owner had gained the
    Direct and Resident profiles;
  - the writer amended only that overview, and independent re-review of the
    fixed commit concluded P0/P1/P2 = 0/0/0;
  - the parent is exactly `c9df391`, only the four allowed files changed, all
    five shader hashes match, and forbidden consumers are byte-identical.
- Final ownership and physical LOC:
  - `direct_gpu_order.rs`: 2,010 LOC, retaining key generation, visibility,
    indirect reset, Resident visible compaction and top-level orchestration;
  - `gpu/scan.rs`: 416 LOC, one portable hierarchical scan kernel/graph owner;
  - `gpu/radix.rs`: 1,724 LOC, one stable full32 radix owner covering the
    external-prefix, Direct/fallback and qualified Resident profiles;
  - `gpu/mod.rs`: 15 LOC, private facade.
- Flexible size decision: the 1,724-LOC radix owner is accepted intact. Its
  buffer graph, profiles, final-A contract and executable GPU oracles form one
  responsibility; splitting only to satisfy the advisory 800 target would add
  coupling without creating a new owner. The architecture notice remains
  advisory, while dependency and legacy-growth rules remain hard.
- Root integrated gates:
  - PASS exact 101-label set comparison and five frozen WGSL hashes;
  - PASS unchanged Direct 16-test/four-ignored and A5a five-test inventories;
  - PASS architecture checker (`59 production Rust, 25 WGSL, 19
    grandfathered`) plus its three fixture tests after lowering only the
    `direct_gpu_order.rs` ratchet baseline from 2,855 to 2,010;
  - PASS format, `git diff --check`, locked workspace check and tests; renderer
    result is 281 passed / 5 ignored;
  - PASS all-target Clippy and Rustdoc with warnings denied;
  - PASS wasm32 `gsplat-web` check, forced Apple M4 Metal SortedAlpha
    conformance and C ABI smoke (`drawn=2`, `visible=2`).
- Required endpoint decision: A065 remains at the A5 package boundary because
  this ownership-only slice cannot alter product behavior, shader bytes,
  target selection or the rendered point set.
- Decision: Accept A5b. A5 remains Active; A5c is merely eligible and is not
  activated by this closeout.

### A5a — Extract external-prefix scan and stable radix owners

- Parent task state: A5 Active
- Subtask state: Accepted
- Started: 2026-07-23
- Ended: 2026-07-23
- Production baseline commit:
  `41c3b31` (`docs: accept A4 CPU order ownership extraction`)
- Exact source baseline before production edits:
  - `crates/gsplat-render-wgpu/src/external_prefix_radix.rs`: 1,123 physical LOC
  - architecture grandfather baseline: 1,123 physical LOC
- Hypothesis: the existing hierarchical prefix scan and portable eight-pass
  stable 4-bit external-prefix radix graph can move unchanged behind private
  strategy-free GPU leaves, while the Preproject caller retains Resident
  admission policy and every key, source-ID, byte-plan, dispatch and error
  result remains unchanged.
- Dependencies: A2 Accepted and A4 Accepted at `41c3b31`. A5b, A5c, A5d, A6
  and later work are inactive.
- Required responsibility boundary:
  - private `gpu/scan.rs` owns the current scan parameters, hierarchy, scratch,
    2D dispatch and encode sequence;
  - private `gpu/radix.rs` owns the current external control ABI, radix
    constants, exact byte plan, ping-pong buffers, eight stable LSD passes and
    final A-buffer contract;
  - a small `gpu/dispatch.rs` is allowed only if it contains the exact shared
    checked 2D workgroup calculation; otherwise that helper stays with scan;
  - `preproject_gpu.rs` continues to own the Preproject plan, its two producer
    scans, Resident capability admission, resource aggregation, pipelines,
    command order and diagnostic publication;
  - `gpu/radix.rs` validates only the shader's actual kernel limits. The
    Preproject caller retains the product's current eight-storage-binding
    Resident/Packed floor. A generic kernel may not import a
    Scene/Resident/Surface policy constant, and this extraction may not broaden
    or narrow product admission.
- Allowed production scope:
  - `crates/gsplat-render-wgpu/src/lib.rs` only for private module wiring;
  - `crates/gsplat-render-wgpu/src/external_prefix_radix.rs`, which may become
    a compatibility facade or be removed after all current private paths are
    migrated;
  - new private `crates/gsplat-render-wgpu/src/gpu/{mod,scan,radix}.rs` and an
    optional `gpu/dispatch.rs` under the responsibility rule above;
  - `crates/gsplat-render-wgpu/src/preproject_gpu.rs` only for mechanical
    imports plus the explicit unchanged Resident binding-floor handoff;
  - existing tests may move only with the primitive they directly verify.
- Forbidden scope:
  - `direct_gpu_order.rs`, `projected_quads_gpu.rs`, `resident_gpu.rs`,
    `surface_presenter.rs`, `surface_session.rs`, Scene/data/API modules,
    shaders, FFI/JNI/Swift/Web/examples, Cargo manifests/lockfile, benchmark
    protocols or architecture policy/ledger in the writer worktree;
  - editing `gpu_prefix_scan.wgsl` or `external_prefix_radix.wgsl`, changing
    workgroup sizes, 2D dispatch shape, scan hierarchy, pass count, radix width,
    stable tie order, buffer usage, ping-pong parity, dynamic offsets, binding
    layouts, allocation sizes, submit/map/readback/device ownership or errors;
  - adopting a different Direct/Projected scan, deduplicating merely similar
    algorithms, touching CPU/GPU/Adaptive policy, adding a new algorithm or
    claiming performance gains;
  - splitting or moving code to satisfy 800 or any other fixed line count.
    Cohesion, dependency direction and independently executable tests decide
    the module boundary.
- Hard gates:
  - fixed-SHA review proves shader bytes unchanged and production/test bodies
    mechanically preserved after path, visibility and explicit-policy-argument
    normalization;
  - `gpu/**` imports no Scene, Surface, Presenter, Session, Preproject plan or
    Resident admission constant; it owns no queue submission, polling, map or
    adapter/device creation;
  - the Preproject caller still requires the same eight storage bindings; a
    focused seven-binding limits test returns the unchanged
    `StorageBindingCountUnsupported(7)` result;
  - exact byte plans and execution cover zero/one, scan boundaries, non-powers
    of two, non-workgroup tails, duplicate/all-equal/zero/MAX keys, stable
    source-ID ties, poisoned capacity tails, high-water reuse and 2D dispatch;
  - all existing external-prefix and Preproject count/image tests remain, with
    the same ignored inventory and no weakened assertion;
  - format, whitespace, architecture checks, locked renderer/workspace tests,
    all-target Clippy and Rustdoc with warnings denied, wasm32 check and forced
    Metal GPU conformance pass. The root task alone may lower/remove the exact
    architecture grandfather entry after semantic review.
- Performance observations: none. A5a is an ownership extraction and neither
  FPS nor a competitor percentage is a completion gate.
- Required endpoints for claim: Apple M4 Metal executes the existing scan/radix
  GPU oracles; wasm32 compiles. A065 and broader cross-endpoint evidence are
  package-boundary responsibilities, not an excuse to expand this writer task.
- Performance correction used: no
- Known correctness issues: none
- Writer commit:
  `53eb004626fb12b70f4dfe2da14ea91a43c870f9` (`refactor: extract external
  prefix GPU owners`).
- Integration merge commit: `53d7d57`.
- Fixed-SHA review: P0/P1/P2 = 0; the reviewer confirmed the exact allowlist,
  unchanged WGSL blobs, mechanically identical scan/radix graph, unchanged
  stable eight-pass 4-bit LSD order, unchanged byte plan and explicit
  six-binding leaf/eight-binding Resident admission split.
- Final ownership and physical LOC:
  - `gpu/mod.rs`: 7 LOC, private facade;
  - `gpu/scan.rs`: 325 LOC, hierarchical scan scratch and dispatch owner;
  - `gpu/radix.rs`: 810 LOC, cohesive stable external-prefix radix owner;
  - the old 1,123-LOC `external_prefix_radix.rs` owner is gone.
- Flexible size decision: `gpu/radix.rs` remains one 810-LOC owner because its
  ABI, byte plan, ping-pong graph, eight passes and direct tests form one
  responsibility. The architecture checker emits a non-blocking notice; no
  split was made merely to satisfy the advisory 800 target or another numeric
  target.
- Root integrated gates:
  - PASS architecture checker/self-tests; the obsolete 1,123-LOC grandfather
    entry is removed after its semantic exit condition was met;
  - PASS format and `git diff --check`;
  - PASS locked workspace check and tests; renderer 281 passed / 5 ignored;
  - PASS all-target Clippy and Rustdoc with warnings denied;
  - PASS wasm32 `gsplat-web` check;
  - PASS forced Apple M4 Metal SortedAlpha conformance;
  - PASS C ABI smoke.
- Decision: Accept A5a. This closes only the primitive-owner slice; A5 remains
  Active and the next consumer slice requires a new explicit activation.

### A4b — Extract renderer CPU visibility/depth/key primitives

- Parent task state: A4 Accepted
- Subtask state: Accepted
- Started: 2026-07-23
- Ended: 2026-07-23
- Production baseline commit:
  `7650c59449736b560e051ceefecf67d5dcd6db13` (`docs: accept A4a CPU sort
  ownership extraction`)
- Exact source baseline before production edits:
  - `crates/gsplat-render-wgpu/src/lib.rs`: 5,004 physical LOC
  - architecture grandfather baseline for `lib.rs`: 5,004 physical LOC
- Hypothesis: the existing exact CPU visibility, depth-key and deterministic
  native chunk primitives can move unchanged into one private renderer leaf,
  while `Renderer` retains all orchestration/workspace ownership and every
  existing internal crate-root path remains available by private re-export.
- Dependencies: A4a Accepted at `7650c59`; the extracted `gsplat-sort` API is
  frozen. A5 and every later task are inactive.
- Required responsibility boundary:
  - the new private `cpu_order.rs` owns exactly
    `PARALLEL_PREPROCESS_THRESHOLD`, `MAX_PARALLEL_PREPROCESS_CHUNKS`,
    `PreprocessChunkScratch`, `is_visible`, `depth_to_key`,
    `world_to_camera_depth_with_view_row`,
    `preprocess_positions_visible_into`,
    `preprocess_positions_visible_into_parallel` and
    `preprocess_paged_visible_into`;
  - the current threshold, thread-count fallback, four-chunk cap, camera
    validation and `RendererError::InvalidCamera` mapping move inside those
    byte-identical function bodies; they are not lifted into configuration or
    a new error abstraction;
  - `lib.rs` continues to own `Renderer`, its vectors, timed orchestration,
    geometry-path/call timing, sort invocation and sort/error orchestration;
  - shared quaternion/matrix/canonical-dot math remains in its current owner;
    A4b may call it but may not relocate or rewrite it;
  - the async loop in `surface_session.rs` remains deliberately duplicated for
    E3, where one `CpuOrderEngine` will replace all sync/async/offscreen owners.
- Allowed production scope:
  - exact file allowlist: `crates/gsplat-render-wgpu/src/lib.rs` and new private
    `crates/gsplat-render-wgpu/src/cpu_order.rs`, including relocation of the
    one directly owned test into that module when useful;
  - private module declaration, imports and `pub(crate)` re-exports required to
    preserve existing internal paths used by `surface_presenter`,
    `surface_session`, `direct_gpu_order` and `preproject_gpu` without editing
    those consumers;
  - only `parallel_visibility_preprocess_matches_sequential_source_order` may
    move with its owner. Sorted-alpha ordering, missing-scene, invalid-camera
    and every consumer test remain in their current files;
  - mechanical visibility changes only; moved production/test bodies must be
    byte-identical after path and indentation normalization.
- Forbidden scope:
  - `crates/gsplat-sort/**`, `surface_presenter.rs`, `surface_session.rs`,
    `direct_gpu_order.rs`, `preproject_gpu.rs`, any other renderer file,
    architecture policy/ledger, Cargo manifests/lockfile, shaders, FFI/JNI,
    Swift, Web, examples or benchmark protocol;
  - changing near/far inclusivity, `depth.max(0.0).to_bits()`, quaternion or dot
    arithmetic order, source iteration/concatenation order, capacity reuse,
    Rayon threshold `256 * 1024`, four-chunk cap, fallback conditions or error
    mapping;
  - merging sync and async loops, constructing a new engine/workspace, moving
    `Renderer` fields, new SIMD, calibration, CPU/GPU/Adaptive policy, timing,
    allocation, public API, optimization claims, merge/rebase/push or edits in
    another worktree;
  - splitting by a numeric line target. The new module may take the cohesive
    size implied by these primitives; only mixed ownership is a failure.
- Hard gates:
  - fixed-SHA review proves all moved production and test bodies, constants and
    cfg gates unchanged, with one definition of every primitive;
  - `Renderer::preprocess_visible_scratch`, `sort_preprocessed_scratch`, every
    Renderer vector/backend field, `canonical_dot3_f32`, quaternion/matrix
    helpers and `surface_session::sort_positions_for_camera` remain in place;
  - every pre-task internal path continues to compile; no consumer source file
    changes and no public Rust/C ABI surface changes;
  - the complete renderer test inventory remains, including invalid camera,
    missing scene, sorted-alpha order, scalar/native parallel parity, duplicate
    depths, non-lane tails, empty/singleton and source-order stability where
    currently present;
  - `cargo fmt --all -- --check`, `git diff --check`, locked renderer/workspace
    tests, all-target Clippy with warnings denied, Rustdoc with warnings denied,
    wasm32 check and architecture checks pass after the root task lowers the
    current `lib.rs` ratchet baseline separately;
  - Apple M4 native exercises the Rayon/scalar oracle. Root owns a fresh A065
    exact-count/full-quality receipt at the A4 package boundary after accepting
    the writer SHA; no FPS or speed percentage is a gate.
- Performance observations: none. This task relocates the current scalar/Rayon
  implementation and makes no SIMD/preprocess speed claim.
- Required endpoints for claim: native Apple M4 tests plus wasm32 compile in
  the writer task; A065 Vulkan exactness is owned by root closeout.
- Performance correction used: no
- Known correctness issues: none
- Writer commit:
  `a835e5536f059aabb436715ba525d1c26a95af12` (`refactor: extract renderer
  CPU order primitives`). The first candidate was not accepted because it
  narrowed existing private root aliases/cfg availability; the fixed commit
  restores those contracts without weakening the gate.
- Integration commit: `0cf228ffb32e19d3e84af599b7f896e1790b1dd6`.
- Architecture-ratchet commit: `b47d08c4a0def082971127f4e6a06fa815d73b82`.
  Only the current `lib.rs` baseline changed from 5,004 to 4,828; its immutable
  A0 LOC, M7 owner and semantic exit condition remain unchanged.
- Final responsibilities and physical LOC:
  - `cpu_order.rs`: 192 LOC; the cohesive visibility/depth/key/chunk primitive
    leaf named by this task
  - `lib.rs`: 4,828 LOC; Renderer workspace, orchestration, timing, sorting,
    camera math and all consumers remain in their previous owner
- Independent fixed-SHA review: P0/P1/P2 = 0 after correction. It confirmed one
  definition of each primitive, byte-identical moved bodies after path and
  indentation normalization, unchanged constants/cfg gates and no numeric-LOC
  split.
- Root verification:
  - PASS format, whitespace, architecture checker and all three checker
    self-tests; `packed_atlas.rs` at 916 LOC remains a non-blocking review
    notice because it is one cohesive quantization/compatibility owner;
  - PASS locked workspace check/tests, all-target Clippy and Rustdoc with
    warnings denied, locked wasm32 check, C ABI smoke and forced Metal
    SortedAlpha conformance;
  - PASS renderer inventory: 280 passed and the same 5 external/research
    oracles ignored; `gsplat-sort`: 16 passed and the same manual microbenchmark
    ignored;
  - PASS clean-build A065 artifact at
    `target/benchmarks/native-render-core-refactor/a4-a065-truck-exactness/`:
    complete 630,225,580-byte Truck, 2,541,226 source/decoded/encoded/resident/
    addressable splats, SH3, sampling/LOD/upscaling disabled, two trace views,
    and requested/Surface/internal/presented dimensions all 2412x1080.
- Performance observation: the four retained full-Candidate CPU frames averaged
  about 166 ms on A065. This short exactness run is not a performance claim and
  no FPS or competitor percentage is an A4 gate.
- Decision: Accept A4b and close A4. Existing CPU/SIMD/Rayon behavior now has
  explicit ownership without changing algorithm, allocation, ordering,
  public API, shader, platform lifecycle or product policy. A5 remains inactive
  until a separate plan-only activation commit.

### A4a — Extract the existing `gsplat-sort` CPU owner

- Parent task state: A4 Active
- Subtask state: Accepted
- Started: 2026-07-23
- Ended: 2026-07-23
- Production baseline commit:
  `3128b5e` (`docs: accept A3 scene resource extraction`)
- Exact source baseline before production edits:
  - `crates/gsplat-sort/src/lib.rs`: 870 physical LOC
  - `crates/gsplat-sort/src/radix.rs`: 455 physical LOC
- Hypothesis: the existing `CpuSortBackend`, reusable scratch, packed-pair
  scalar/NEON/AVX2 helpers and stable serial/Rayon radix implementation can be
  moved behind focused private modules while keeping the crate-root API and
  every sorted bit unchanged. The historical `GpuOddEvenSortBackend` may move
  mechanically into one compatibility module so `lib.rs` becomes a real
  facade; it does not become a product GPU candidate.
- Dependencies: A3 Accepted at `3128b5e`; A2's strategy-free data/API leaves
  remain unchanged. No A4b/A5 or later task is active.
- Required dependency direction:
  - crate root owns the public facade and re-exports only;
  - one private CPU backend/workspace owner composes packed-pair and radix
    leaves;
  - packed-pair helpers own only the current scalar and target-gated NEON/AVX2
    bit packing/unpacking;
  - radix owns only the current stable serial/Rayon histogram, prefix and
    scatter mechanics;
  - the odd-even WGPU tool remains isolated compatibility/conformance code and
    cannot be imported by CPU modules.
- Allowed production scope:
  - exact file allowlist: `crates/gsplat-sort/src/lib.rs`, new private
    `src/cpu.rs`, existing `src/radix.rs`, new private
    `src/gpu_odd_even.rs`, and tests colocated in those same modules;
  - `crates/gsplat-sort/README.md` only when needed to describe the unchanged
    ownership;
  - mechanical visibility/import/re-export changes that preserve every public
    crate-root name, signature, trait implementation, error and behavior;
  - the A4 architecture grandfather entry may be lowered or removed only when
    its semantic exit condition is actually satisfied. Physical LOC is review
    information, not the decision.
- Forbidden scope:
  - renderer files, Surface/offscreen lifecycle, FFI/JNI/Swift/Web consumers,
    examples, `crates/gsplat-sort/Cargo.toml`, `Cargo.lock`,
    `shaders/odd_even_sort.wgsl`, Cargo dependencies/features or benchmark
    protocol;
  - depth/key preprocess, camera math or Rayon chunking currently owned by the
    renderer; those belong to a later A4b;
  - any radix bit width, bucket count, parallel threshold/chunk cap, tie rule,
    packed key/value representation, target-feature detection, unsafe
    instruction sequence or allocation/reuse behavior change;
  - any change to the compatibility GPU sorter's O(N^2) algorithm, 4,096-item
    ceiling, lazy initialization, device limits, submit/readback lifecycle or
    `BackendUnavailable`/`BackendFailure` behavior;
  - new SIMD kernels, calibration, performance selection, CPU/GPU/Adaptive
    policy, optimization claim, merge/rebase/push or edits in another worktree.
- Hard gates:
  - pre/post public API and trait compile probes match, including
    `SortBackend`, `SortError`, `CpuSortBackend` and
    `GpuOddEvenSortBackend`;
  - pre/post test names and assertions migrate without loss, including empty,
    singleton, non-lane multiples, mismatch errors, every radix digit, large
    parallel inputs, immutable keys, nonsequential values, duplicate-key
    stability and full-64-bit pair ordering;
  - deterministic scalar reference parity holds for scalar/SIMD/Rayon serial
    and parallel paths; the WASM path remains serial;
  - existing AArch64 NEON execution is exercised on the Apple M4 and the crate
    still checks for wasm32; x86 AVX2 remains compile-gated unless an x86
    endpoint is actually available;
  - `cargo test -p gsplat-sort --lib`, the unchanged
    `gsplat-render-wgpu --lib` consumer tests, format, architecture policy,
    locked workspace tests, all-target Clippy, Rustdoc and WASM pass;
  - source size follows responsibility: advisory notices are allowed and only
    the shared 2,500-line mixed-owner circuit breaker needs a finite exception.
    Do not split or move tests to hit a numeric threshold.
- Performance observations: none. A4a is a behavior-preserving ownership task;
  its manual microbench remains ignored and no speed percentage is a gate.
- Required endpoints for claim: Apple M4 native unit path plus wasm32 compile;
  root may use A065 later at the A4 package boundary, not in the writer task.
- Performance correction used: no
- Known correctness issues: none
- Closeout requirement: root reviews one fixed writer SHA, proves symbol/test
  inventory and public API parity, records final responsibilities, then either
  Accepts/Rejects/Defers A4a. A4b is activated only in a later plan-only commit.
- Fixed writer commit:
  `8ba7845fce002c26e65280a60de81259096beda1` (parent
  `1e4a45c18b0e8b260c52b0693fd6e9454461a701`).
- Integration commit:
  `2ead582fdeb723c1292755ccd6e44807cc16ad90` (`merge: integrate A4a CPU sort
  ownership extraction`).
- Final responsibilities and physical LOC:
  - `lib.rs`: 26 LOC, crate-root facade, public trait/error and re-exports only
  - `cpu.rs`: 548 LOC, CPU backend, reusable scratch, packed pairs and existing
    target-gated NEON/AVX2 helpers
  - `radix.rs`: 455 LOC, unchanged stable serial/Rayon radix mechanics
  - `gpu_odd_even.rs`: 309 LOC, mechanically moved compatibility/conformance
    backend with its original O(N^2) and 4,096-item limits
- Preservation evidence:
  - independent fixed-SHA review found P0/P1/P2 = 0 and proved CPU production,
    GPU odd-even production, public trait/error, test bodies and assertion
    inventories byte-identical after accounting for module ownership
  - repository-external probes passed before and after for all four public root
    names, both `Default` implementations, backend names, sorting behavior and
    exact error Display strings
  - all 17 test leaves remain; 16 pass and the same manual size-ladder
    microbenchmark remains ignored
- Verification:
  - PASS `cargo fmt --all -- --check`, `git diff --check`, locked `gsplat-sort`
    and `gsplat-render-wgpu` library tests, wasm32 check, all-target Clippy with
    warnings denied and Rustdoc with warnings denied
  - PASS Apple M4 GPU conformance for the unchanged odd-even compatibility
    backend
  - PASS architecture checker and all three self-tests after removing the
    completed 870-LOC `gsplat-sort/src/lib.rs` grandfather entry; the unrelated
    cohesive 916-LOC `packed_atlas.rs` remains an advisory notice only
- Performance observation: none. This is a byte-preserving responsibility
  extraction and makes no speed claim.
- Decision: Accept A4a. The crate root is now a real facade, CPU and legacy GPU
  compatibility responsibilities have one owner each, and no algorithm,
  policy, public API or consumer behavior changed. The grandfather entry was
  removed because its semantic exit condition is satisfied, not because a
  numeric quota was reached.

### A3b — Extract GPU resource planning and Direct/Packed preflight

- Parent task state: A3 Accepted
- Subtask state: Accepted
- Started: 2026-07-23 21:34 CST
- Ended: 2026-07-23
- Production baseline commit:
  `0562cfe` (`docs: accept A3a resident ownership extraction`)
- Exact source baseline before production edits:
  - `crates/gsplat-render-wgpu/src/lib.rs`: 5,589 physical LOC
  - `crates/gsplat-render-wgpu/src/resident_gpu.rs`: 1,182 physical LOC
  - `crates/gsplat-render-wgpu/src/projected_quads_gpu.rs`: 2,358 physical LOC
  - `crates/gsplat-render-wgpu/src/scene/budget.rs`: 73 physical LOC
- Hypothesis: the existing Resident GPU byte plan, Direct/Packed capacity
  receipts and their pure arithmetic can move into private leaf modules without
  changing one public root name, formula, minimum descriptor, error, selected
  geometry path, requested device limit, allocation or GPU command.
- Dependencies: A3a Accepted at `0562cfe`; no later package is active.
- Required dependency direction:
  - extend private `scene/budget.rs` from its existing CPU payload ledger into
    the single owner of `ResidentGpuBytePlan`, projected-contributor byte
    arithmetic and the Resident/projected descriptor constants; it remains
    pure resource math and must not own GPU buffers, commands or policy;
  - add private `scene/preflight.rs` for the Direct/Packed public capacity
    reports, paths, remediation/failure types, `DirectSceneError` and three
    preflight functions;
  - add a private top-level `gpu_error.rs` leaf for the existing
    `ResidentGpuError`, moved mechanically with all variants, derives and
    Display text unchanged;
  - make `scene/budget.rs` the single source for projected cache/scan planning
    constants and `RESIDENT_COLOR_STORAGE_BINDINGS`; GPU consumers may import
    or privately re-export them, but must not duplicate numeric definitions;
  - preserve every current crate-root public re-export and method signature.
- Allowed production scope:
  - extend private `crates/gsplat-render-wgpu/src/scene/budget.rs`, add private
    `scene/preflight.rs`, and add private top-level `gpu_error.rs`;
  - private `scene/mod.rs` re-exports needed to preserve root API;
  - mechanical module declarations, root re-exports and removal of the moved
    declarations/implementations from `lib.rs` and `resident_gpu.rs`;
  - only constant/import/re-export changes required in
    `projected_quads_gpu.rs` and `surface_presenter.rs`;
  - move the existing pure byte-plan/limit and Direct/Packed preflight tests to
    focused `scene/tests/budget.rs` and `scene/tests/preflight.rs` modules
    without weakening assertions;
  - exact A1 architecture-policy grandfather entries for legacy files that
    physically shrink; normal modules use the advisory size profile.
- Must remain with existing owners:
  - `SurfaceResourcePlan`, `surface_resource_plan`, device-limit requests,
    selected-path validation and fallback policy remain in
    `surface_presenter.rs`;
  - GPU buffer creation/descriptors/upload/bind groups and
    `ResidentGpuResources` remain in `resident_gpu.rs`;
  - Renderer preflight accessors, offscreen device-limit behavior and
    `ErrorCode` mapping remain in `lib.rs`;
  - allocation/order/dispatch tests remain beside their runtime owner.
- Forbidden scope:
  - any resource formula, field order/type, derive, error variant/message,
    public signature or numeric constant change;
  - shaders/WGSL, projection math, sort, visibility, raster, point membership,
    SH degree/precision, camera, resolution or benchmark behavior;
  - buffer creation/upload, bind groups, queue submission, Surface policy,
    adapter/device selection, requested limits or automatic paging;
  - `direct_gpu_order.rs`, `external_prefix_radix.rs`, `preproject_gpu.rs`,
    `surface_session.rs`, Packed/Paged implementation modules, FFI/JNI/Swift,
    Web API, examples, tools, Cargo dependencies/features, merge/rebase/push or
    edits in another worktree.
- Hard gates:
  - all moved symbols have one production definition and all old/new pure-test
    inventories match;
  - a repository-external temporary crate imports every current public root
    preflight/plan/error name and checks signatures, fields, traits, error text
    and boundary receipts;
  - exact boundary/overflow cases remain unchanged, including empty minimum
    descriptors, `min(max_storage_buffer_binding_size, max_buffer_size)`, eight
    bindings, 128 MiB boundaries, `u32` draw count and `usize::MAX` overflow;
  - architecture checker/self-tests, format, locked workspace check/tests,
    all-target Clippy, Rustdoc, WASM and FFI smoke pass;
  - LOC is advisory below the multi-thousand circuit breaker. Cohesive modules
    above 800 or 1,200 lines are acceptable after root review; only a new file
    above 2,500 needs a finite documented exception. Do not split at 799 lines
    or game physical LOC.
- Performance observations: none; A3b is a behavior-only responsibility
  extraction.
- Required endpoints for claim: none in the writer task. Root acceptance owns
  forced Metal conformance and an A065 full-count exactness run at the A3
  package boundary. No fixed FPS or competitor percentage is a completion gate.
- Performance correction used: no
- Known correctness issues: none
- Fixed writer commit:
  `ba8ed0e19999d90123403d5c3df6c034cdad9261` (`refactor: extract scene resource planning`)
- Integration merge commit: `3fe4c3e`.
- Root policy correction commit: `278e393`; numeric source-size targets are
  review notices, not A3 completion gates.
- Final physical LOC:
  - `lib.rs`: 5,004
  - `projected_quads_gpu.rs`: 2,360
  - `resident_gpu.rs`: 677
  - `scene/budget.rs`: 407
  - `scene/preflight.rs`: 372
  - `gpu_error.rs`: 35
  - `scene/mod.rs`: 29
  - focused budget/preflight test modules: 139 / 235 LOC
- Root verification:
  - PASS architecture checker self-tests and real-tree policy check;
  - PASS `cargo fmt --all -- --check`, locked workspace check and tests
    (renderer 280 passed, 5 ignored), all-target Clippy and Rustdoc with
    warnings denied;
  - PASS locked `wasm32-unknown-unknown` check, C ABI smoke and forced Metal
    SortedAlpha conformance;
  - PASS repository-external public-API probe with unchanged Direct maximum
    `745654`, Resident/Packed largest binding `134217728` and eight storage
    bindings;
  - PASS A065 full-Truck exactness artifact at
    `target/benchmarks/native-render-core-refactor/a3-a065-truck-exactness-retry2/`:
    complete 630,225,580-byte PLY, 2,541,226 source/decoded/encoded/resident/
    addressable splats, SH3 preserved, sampling/LOD/upscaling disabled, and
    requested/Surface/internal/presented dimensions all 2412x1080. Four
    retained frames passed the benchmark and camera-receipt validators.
- Performance observation: current full Candidate draw averaged about 165 ms
  per retained frame on A065. This is not an A3 failure because A3 changes
  ownership only; it is explicit input to later exact-work/raster tasks and is
  not converted into a fixed FPS gate.
- Responsibility review: `packed_atlas.rs` remains a cohesive 916-line
  quantization/legacy-compatibility owner. Product Resident ownership is now in
  `scene/{resident,builder,codec}.rs`, so its A3 grandfather entry was removed;
  the normal 800-line notice remains visible, but no mechanical split is
  required.
- Decision: Accept A3b and close A3. Resource arithmetic has one pure owner,
  preflight/error ownership is explicit, root public contracts are unchanged,
  and the exact-count native product path remains intact on Metal, Web target,
  C ABI and A065 Vulkan.

### A3a — Extract Resident CPU ownership, ABI layout and encoder

- Parent task state: A3 Active
- Subtask state: Accepted
- Started: 2026-07-23 21:02 CST
- Ended: 2026-07-23 21:30 CST
- Production baseline commit:
  `e03a26901fc92e7b449cbf07ca38c80eb0a06963`
- Exact source baseline before production edits:
  - `crates/gsplat-render-wgpu/src/resident_scene.rs`: 1,881 physical LOC
  - architecture grandfather baseline: 1,881 physical LOC
- Hypothesis: the exact Resident CPU owner, fixed GPU ABI layout, transactional
  encoder and their tests can move into small private `scene/` and existing
  `data/layout.rs` modules without changing one encoded bit, source order,
  upload-staging lifecycle, public crate-root path, allocation behavior,
  render/preflight policy or any GPU command.
- Dependencies: A2 Accepted at
  `d041a4a3cbefaa70f8647f65ff575727acbb3589`; root closeout commit
  `e03a26901fc92e7b449cbf07ca38c80eb0a06963` is the task production baseline.
- Allowed scope:
  - private focused modules under `crates/gsplat-render-wgpu/src/scene/` for
    Resident CPU ownership, builder/codec, the existing CPU byte-accounting
    value, and focused tests; split by responsibility rather than an arbitrary
    line quota
  - move the existing Resident ABI constants and `repr(C)` storage structs into
    `data/layout.rs`, with compile-time size/alignment/offset assertions
  - delete or reduce `resident_scene.rs` to a temporary compatibility facade;
    no duplicated owner or implementation may remain
  - private module declarations/re-exports and compilation-required mechanical
    imports in `lib.rs`, `resident_gpu.rs` and existing Resident tests
  - this ledger and the exact A1 architecture-policy ratchet entry for
    `resident_scene.rs`; remove the entry only if the legacy file is removed or
    is below the declared target
- Forbidden scope:
  - Direct/Packed selection, resource limits, requested wgpu limits, preflight
    formulas or error policy; these belong to A3b
  - GPU buffers, bind groups, queue submission, shaders, projection, raster,
    sort, visibility, point membership, SH degree/precision, camera, resolution
    or platform lifecycle
  - new runtime/renderer/plan owner types, public `scene`/`data` namespaces,
    public signature/semantic changes, C ABI/header, JNI/Kotlin, Swift, Web API,
    examples, Cargo dependencies/features, benchmarks or device experiments
  - moving unrelated `packed_atlas` tests, merging, rebasing, pushing or editing
    another worktree
- Required preservation:
  - every existing public crate-root Resident name, derive, field, error
    variant/message and numeric layout remains source-compatible
  - SH0/1/2/3 encoded bytes, chunk metadata, covariance, source ordering,
    reports, overflow/allocation errors and upload-staging release behavior are
    bit-for-bit unchanged
  - `ResidentCpuByteAccounting` may move but its formula and result remain
    unchanged; A3a does not expand it into the A3b preflight model
- Hard gates:
  - architecture checker/self-tests pass; the 1,881-line grandfather entry is
    removed or ratcheted exactly; 800 physical LOC is a review target, while
    the 2,500-line circuit breaker and finite exception mechanism prevent a new
    multi-thousand-line owner without forcing artificial splits
  - format, locked workspace check/tests/clippy/rustdoc, render-wgpu tests, wasm
    check and FFI smoke pass
  - a repository-external temporary crate imports the existing public Resident
    root names and validates ABI sizes/offsets
  - `git diff --check`, exact scope and single-definition audits pass
- Performance observations: none; A3a is a behavior-only ownership extraction
- Required endpoints for claim: none in the writer task. Root acceptance owns a
  forced real-GPU conformance run; A065 full-count validation is deferred to
  the A3 package boundary after A3b.
- Performance correction used: no
- Known correctness issues: none
- Closeout requirement: record the fixed implementation SHA, exact file sizes,
  verification and result here, but keep `A3 = Active`; only root acceptance of
  A3b may close the parent package.
- Fixed writer commit:
  `7db9cadf27e3c51e7d27c59e5faa3a21db063c1f` (parent
  `21f0fa0b877b1e78bbe919117ec5d27d56acb5f9`).
- Integration merge commit:
  `43ec7e759ee52d0f0199bfa14d602c26d8c791f1`.
- Result:
  - deleted the 1,881-line `resident_scene.rs` owner without a compatibility
    facade or duplicate implementation;
  - production responsibilities now live in `scene/budget.rs` (73 LOC),
    `scene/builder.rs` (409), `scene/codec.rs` (296), `scene/mod.rs` (16) and
    `scene/resident.rs` (303), with focused tests under `scene/tests/`;
  - fixed Resident GPU ABI layouts moved to `data/layout.rs` with compile-time
    size, alignment and offset assertions; every crate-root public name and
    signature remains available;
  - the obsolete architecture grandfather entry was removed. The only legacy
    file growth was one mechanical import line in each of `lib.rs` and
    `resident_gpu.rs`, accepted by the responsibility-aware +32-line tolerance.
- Root acceptance evidence:
  - architecture self-tests and the real-tree source checker pass;
  - `cargo fmt --check`, locked workspace check/tests, all-target Clippy with
    warnings denied, Rustdoc with warnings denied, render-wgpu WASM check and C
    ABI smoke pass on the merged tree;
  - renderer tests report 280 passed and 5 existing research/stress cases
    ignored; forced Metal SortedAlpha conformance passes;
  - a repository-external temporary crate imported the preserved Resident root
    API and checked traits, builder flow, errors, sizes and offsets;
  - old and new test inventories are both 20 tests; normalized production
    function bodies are unchanged and SH0--SH3 encoding remains byte-identical.
- Non-blocking follow-up: the pre-existing `legacy_encode` test oracle shares
  production codec helpers, so it is an independent traversal/builder oracle,
  not a fully independent codec implementation. A future frozen encoded-byte
  digest fixture may strengthen this without blocking a pure ownership move.
- Decision: Accept. A3 remains Active and A3b is the only next eligible writer
  task.

### A2b — Extract the stable API leaf

- Parent task state: A2 Accepted after fixed-SHA root-task acceptance.
- Subtask state: Complete and root-accepted
- Started: 2026-07-23 20:46 CST
- Ended: 2026-07-23 20:54 CST
- Baseline commit: `4e110e4e02291dc0a8d77046e8f6b9d123d0ff69`
- Worktree before task: clean detached checkout at the exact baseline; branch
  `codex/native-render-a2b-api` was created and checked out before the first edit
- Exact source baseline before production edits:
  - `crates/gsplat-render-wgpu/src/lib.rs`: 5,601 physical LOC
  - architecture grandfather baseline for `lib.rs`: 5,601 physical LOC
- Hypothesis: `GeometryPath` and `PreprocessOutput` are strategy-independent,
  stable API leaves that can move byte-for-byte to one small private `api.rs`
  while crate-root re-exports preserve every existing public path and all
  render, preflight, FFI, shader, resource and platform-lifecycle behavior.
- Dependencies: A1 Accepted; A2a complete and root-accepted at the stated baseline
- Allowed scope:
  - new private `crates/gsplat-render-wgpu/src/api.rs`
  - move the existing `GeometryPath` and `PreprocessOutput` declarations from
    `lib.rs` without changing derives, variants, default, docs, fields or types
  - private `mod api;`, root `pub use api::{GeometryPath, PreprocessOutput};`,
    and only compilation-required mechanical imports
  - this A2b ledger record
  - exact formatted `lib.rs` physical-LOC ratchet reduction in
    `tests/architecture/source_architecture_policy.json`; immutable A0 LOC,
    owner task and exit condition remain unchanged
- Forbidden scope:
  - `RendererError`, `SurfacePresenterError`, Direct/Packed preflight or error
    clusters; these remain deferred to A3, A6 and A8 ownership
  - `SurfaceFrameOutput`, `SurfaceFrameTimings`, session/controller/adaptive,
    evidence or receipt types; these remain deferred to A7/A8 ownership
  - `Renderer` ownership, preflight functions, resource calculations,
    `surface_session.rs`, `surface_presenter.rs`, Resident/Paged/Tiled/GPU order,
    project/raster modules, `data/`, shaders or tests
  - C FFI/header, JNI/Kotlin, Swift, Web/Wasm, examples, tools, Cargo
    dependencies/features, public signatures, FFI numeric mappings, defaults,
    camera, point count, SH, resolution or ordering behavior
  - public `api` module exposure, a future placeholder/renderer owner, test
    relocation, merge, rebase, push or another worktree
- Required preservation:
  - `GeometryPath` retains `Debug, Clone, Copy, PartialEq, Eq, Default`, exact
    variant order, `SortedIndexDirect` default, variant documentation and
    semantics; no `repr` or explicit discriminants
  - `PreprocessOutput` retains `Debug, Clone, PartialEq, Eq`, both public fields
    and their exact types
  - `gsplat_render_wgpu::GeometryPath` and
    `gsplat_render_wgpu::PreprocessOutput` remain valid; no
    `gsplat_render_wgpu::api::*` path is introduced and consumers need no edits
- Hard gates:
  - architecture checker/self-tests pass; shrink never requires code padding
    or a same-task numeric checkpoint update
  - format, locked workspace check/tests/clippy/rustdoc, render-wgpu lib tests,
    wasm check and FFI smoke all pass
  - a repository-external temporary crate imports both existing root names and
    exhaustively matches all three `GeometryPath` variants
  - `git diff --check`, exact physical LOC, declaration/path visibility and
    declared-scope audits pass
- Performance observations: none planned; A2b is a behavior-only extraction
- Required endpoints for claim: none; GPU/API/FFI critical acceptance remains
  the root task's responsibility, and device/browser/performance runs are out
  of scope
- Performance correction used: no
- Deferred ownership:
  - A3: Resident ownership/layout/preflight and related error responsibilities
  - A6: canonical raster ownership
  - A7: immutable evidence/receipt types and observer storage
  - A8: Surface/offscreen lifecycle plus legacy session/presenter boundary
- Known correctness issues: none
- Result:
  - private `api.rs` owns only the byte-for-byte moved `GeometryPath` and
    `PreprocessOutput` declarations; it contains no placeholder or renderer owner
  - `lib.rs` declares `mod api;` and publicly re-exports both names from the
    crate root, so existing consumers compile unchanged and no public
    `gsplat_render_wgpu::api::*` namespace exists
  - `GeometryPath` derive list, variant order, `SortedIndexDirect` default,
    documentation and semantics are unchanged; no representation or
    discriminant was added
  - `PreprocessOutput` derive list, public fields and field types are unchanged
- Source-size ratchet:
  - `lib.rs`: 5,601 -> 5,588 formatted physical LOC
  - new `api.rs`: 14 formatted physical LOC
  - only the `lib.rs` `baseline_physical_loc` changed from 5,601 to 5,588;
    immutable A0 LOC 5,680, owner task M7 and exit condition are unchanged
- Verification:
  - PASS `PYTHONDONTWRITEBYTECODE=1 python3
    tests/architecture/test_source_architecture.py` (3 tests)
  - PASS `PYTHONDONTWRITEBYTECODE=1 python3
    tests/architecture/check_source_architecture.py` (42 production Rust, 25
    WGSL, 24 grandfathered)
  - PASS `cargo fmt --all -- --check`
  - PASS `cargo check --workspace --locked`
  - PASS `cargo test -p gsplat-render-wgpu --lib --locked` (280 passed, 5 ignored)
  - PASS `cargo test --workspace --locked`, including one SortedAlpha
    conformance test and all workspace/doc tests
  - PASS `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - PASS `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`
  - PASS `cargo check -p gsplat-web --target wasm32-unknown-unknown --locked`
  - PASS `bash tests/ffi/run-ffi-smoke.sh` (`drawn=2`, `visible=2`)
  - PASS repository-external temporary crate compile probe importing
    `gsplat_render_wgpu::{GeometryPath, PreprocessOutput}` and exhaustively
    matching all three geometry variants; the initial networked invocation was
    safely interrupted at exit 130 while downloading, then the repository lock
    was copied into the temporary directory and `cargo check --offline` passed
    using the normal Cargo cache without modifying user or repository config
  - PASS exact declaration comparison against baseline, private-module rustdoc
    audit, `git diff --check`, exact LOC/policy audit and declared-scope audit
- Not run by the A2b writer: device runs, browser runtime smoke and performance
  benchmarks; the root task separately ran the required GPU/API/FFI acceptance
- Scope audit: the final repository diff contains only `api.rs`, `lib.rs`, this
  ledger and the exact `lib.rs` architecture baseline; no consumer, error,
  preflight, resource, lifecycle, shader, data, FFI, binding, example, tool or
  Cargo file changed
- Root acceptance:
  - independent fixed-SHA review found no P0, P1 or P2 issue
  - PASS `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p
    gsplat-render-wgpu --test conformance_sorted_alpha --locked -- --nocapture`
    with one Metal test executed and no skip
  - PASS root architecture checker/self-tests, exact four-file scope, private
    module/public root-path audit, LOC/policy audit and `git diff --check`
  - PASS root wasm target check and FFI smoke (`drawn=2`, `visible=2`)
  - the writer's repository-external consumer probe passed offline against the
    exact committed tree; the root review independently verified its source,
    exhaustive variants and private-module rejection
- Decision: accept A2. The data/layout/view leaves and the stable API leaf now
  satisfy A2's behavior-preserving extraction contract. Owner-coupled errors,
  preflight implementation and the legacy frame result remain explicitly with
  A3/A7/A8 rather than creating a new API dependency cycle.
- Result identity: `d041a4a3cbefaa70f8647f65ff575727acbb3589`,
  accepted and fast-forwarded unchanged into
  `codex/native-render-core-refactor`

### A2a — Extract strategy-free data/layout/view leaves

- Parent task state: A2 Active; A2a completion does not close A2.
- Subtask state: Complete; A2 remains Active
- Started: 2026-07-23 20:18 CST
- Ended: 2026-07-23 20:33 CST
- Baseline commit: `304ab0ee115e7cc1bb939922ac531376b90d3796`
- Worktree before task: clean detached checkout at the exact baseline; branch
  `codex/native-render-a2a-data` was created before the first edit
- Hypothesis: the legacy owner can shed its strategy-free kernel ABI and narrow
  data views without changing render behavior, public paths, asynchronous copy
  timing, shader math, FFI or lifecycle ownership.
- Dependencies: A1 Accepted and integrated at the stated baseline
- Allowed scope:
  - new `crates/gsplat-render-wgpu/src/data/{mod.rs,layout.rs,view.rs}`
  - mechanical `mod`, import and root re-export changes in render-wgpu
  - `lib.rs`-resident `GpuInstance`, `GpuSurfaceSourceElem` and
    `GpuSurfaceRenderParams`
  - the single-definition strategy-free `GpuSortPair` ABI if its move remains
    confined to `direct_gpu_order.rs`
  - private `CameraCovarianceTerms`, `ShColorLayout<'a>`, a real Direct upload
    `SplatSetView<'a>` parameter bundle and the existing native async worker's
    `OwnedCpuOrderInput`
  - this ledger and only the A1 grandfather baselines for legacy files that
    physically shrink in this extraction
- Forbidden scope:
  - `api.rs`, `SurfaceFrameOutput`, public signature changes or new public API
  - Resident types/constants, `resident_scene.rs`, `resident_gpu.rs`, their
    ownership/preflight, or either Resident grandfather baseline
  - shaders, render/sort/projection/raster math, precision, membership, camera,
    resolution, C ABI, JNI, Swift, Web API or platform lifecycle
  - `Renderer`, `SceneRuntime`, `PlanSet`, controller, evidence owner,
    `FrameSnapshot`, `InstanceBuildParams` or `PreparedRendererGeometryPath`
  - activating `plans/` or `renderer/mod.rs`, changing product defaults, GPU
    conformance/device collection, merging the integration branch or pushing
- Symbol and dependency inventory before production edits:
  - `GpuInstance` is a 48-byte public `repr(C)`/`Pod` instance record defined
    in `lib.rs`; all in-tree consumers are the legacy CPU reference path. Its
    existing crate-root public path must remain a root re-export.
  - `GpuSurfaceSourceElem` is the 64-byte Direct source upload record defined in
    `lib.rs`; `make_surface_source_elems` packs it and Direct GPU-order tests
    construct it through the parent module. Moving it to a sibling leaf
    requires crate-only field visibility, not public exposure.
  - `GpuSurfaceRenderParams` is the 112-byte shared Direct/Resident/projected
    uniform defined in `lib.rs`; `lib.rs`, `direct_gpu_order.rs`,
    `resident_gpu.rs`, `preproject_gpu.rs` and `projected_quads_gpu.rs` consume
    the existing crate-root path. A crate-root re-export preserves those uses.
  - `GpuSortPair` is one 8-byte `repr(C)`/`Pod` key-ID record defined only in
    `direct_gpu_order.rs`; moving this definition and importing it back is a
    single-file mechanical A5-owner edit with no shader or algorithm change.
  - five private `ScanParams` definitions and three differently named indirect
    draw records span `direct_gpu_order.rs`, `external_prefix_radix.rs`,
    `projected_quads_gpu.rs`, `preproject_gpu.rs`, `tiled_gpu.rs` and
    `tiled_resident_gpu.rs`. Even where byte layouts match, unifying them crosses
    several A5-owned modules, so A2a defers that work to A5.
  - `CameraCovarianceTerms` is a six-f32 private value used by the legacy owner
    and a Preproject CPU oracle through the crate-root path; `ShColorLayout` is
    a borrow-only SH slice/stride view used only during current color packing.
  - current Direct source packing passes `SceneBuffers`, covariance terms and
    alpha slices separately. `SplatSetView` will replace that real packing
    bundle for the duration of the call only; it will not be stored.
  - native async ordering currently clones the Resident position `Arc` or makes
    the Direct position copy exactly once in `SurfaceAsyncSorter::new`, then
    copies `Camera` per request. `OwnedCpuOrderInput` must wrap that worker
    boundary without moving either copy point and without cloning an `Arc` or
    scene per frame.
- Hard gates:
  - architecture checker and its self-test pass; ownership movement is judged
    by symbols, dependencies and tests rather than a numeric shrink checkpoint
  - GPU ABI retains `repr(C)`, `Pod`/`Zeroable`, exact size/alignment and field
    offset assertions; `Vec3f` is not made an ABI/Pod type
  - `GpuInstance` remains available at the existing public crate-root path and
    all other visibility remains no wider than before
  - async Direct copy occurs once at sorter construction; Resident uses the
    existing `Arc::clone`; no retained borrowed view or per-frame scene clone
  - format/check/workspace tests/clippy/rustdoc and affected wasm/FFI entrypoints
    from `handbook/VERIFICATION.md` pass
  - final diff contains only the declared extraction/ledger/policy scope
- Performance observations: none planned; A2a is behavior-only extraction
- Required endpoints for claim: none; GPU conformance and real-device evidence
  remain root-task responsibilities and will be reported as not run
- Performance correction used: no
- Known correctness issues: none
- Result:
  - `data/layout.rs` now owns `GpuInstance`, `GpuSurfaceSourceElem`,
    `GpuSurfaceRenderParams` and the single-definition `GpuSortPair`; every ABI
    retains `repr(C)` plus `Pod`/`Zeroable` and compile-time size, alignment and
    per-field offset assertions
  - `data/view.rs` now owns the existing covariance/SH views, one borrow-only
    `SplatSetView` consumed immediately by Direct source packing, and
    `OwnedCpuOrderInput` at the native async sort worker boundary
  - `GpuInstance` remains documented at `gsplat_render_wgpu::GpuInstance`; all
    other moved symbols remain crate-only
  - Resident ordering still performs `Arc::clone(&scene.positions)` and Direct
    still copies positions once in `SurfaceAsyncSorter::new`; the worker moves
    that same Arc into and back out of each owned input without an Arc or scene
    clone per request
  - duplicate `ScanParams` and indirect draw records remain in place and are
    explicitly deferred to A5 rather than unified across A5-owned modules
- Source-size ratchet:
  - `lib.rs`: 5,680 -> 5,601 physical LOC
  - `direct_gpu_order.rs`: 2,862 -> 2,855 physical LOC
  - `surface_session.rs`: 5,044 -> 5,042 physical LOC
  - new `data/mod.rs`, `layout.rs`, and `view.rs`: 10, 98, and 130 physical LOC
  - all three changed grandfather baselines exactly match those final counts;
    Resident baselines are unchanged
- Verification:
  - PASS `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`
  - PASS `PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py`
  - PASS `cargo fmt --all -- --check`
  - PASS `cargo check --workspace --locked`
  - PASS `cargo test -p gsplat-render-wgpu --lib --locked` (280 passed, 5 ignored)
  - PASS `cargo test --workspace --locked`, including the available non-required
    SortedAlpha conformance invocation and all workspace/doc tests
  - PASS `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - PASS `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`
  - PASS `cargo check -p gsplat-web --target wasm32-unknown-unknown --locked`
  - PASS `bash tests/ffi/run-ffi-smoke.sh` (`drawn=2`, `visible=2`)
  - PASS policy JSON parse, public rustdoc-path inspection, exact LOC audit,
    async Arc/copy-site audit and `git diff --check`
- Environment note: the first workspace check failed with `No space left on
  device (os error 28)`. Only rebuildable Cargo caches were cleared; after the
  disk-space issue was resolved, the exact workspace check and all later gates
  passed.
- Not run by A2a: `GSPLAT_REQUIRE_GPU_CONFORMANCE=1`, formal GPU conformance,
  browser runtime smoke, Android/iOS physical-device runs and performance
  collection; the root task owns those endpoints.
- Root acceptance:
  - independent fixed-SHA review found no P0, P1 or P2 issue
  - PASS `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p
    gsplat-render-wgpu --test conformance_sorted_alpha --locked -- --nocapture`
    with one test executed and no skip
  - PASS exact Direct GPU key/radix complete-element and Resident position
    stride parity tests
  - PASS external crate compile probe for
    `gsplat_render_wgpu::GpuInstance`
  - PASS root reruns of architecture policy/self-tests, declared-scope diff,
    shader/Resident/FFI/binding zero-diff checks and FFI smoke
- Scope audit: no `api.rs`, `SurfaceFrameOutput`, Resident source/baseline,
  shader, FFI source/header, platform wrapper, renderer owner, policy owner or
  product-default change is present.
- Decision: complete the independently verifiable A2a result and leave the
  machine registry at `A2 = Active`; do not claim overall A2 acceptance.
- Result identity: `3758fd614bc09c5f330210cf87a120dd9fd0ccdd`, accepted and
  fast-forwarded unchanged into `codex/native-render-core-refactor`.

### A0 — Freeze integration baseline and evidence inventory

- State: Accepted
- Started: 2026-07-23 18:38 CST
- Ended: 2026-07-23 18:43 CST
- Baseline commit: `c478252246733f6dc209686091caf183e1ef7f06`
- Worktree before task: clean detached checkout at the baseline commit
- Hypothesis: because full-quality is a strict linear descendant of the
  unchanged `main`, a dedicated integration branch can preserve evidence
  identities and isolate Package A without a merge or history rewrite.
- Dependencies: none
- Allowed scope:
  - this progress ledger
  - `a0-baseline.md`
  - read-only Git, evidence and document inspection
- Forbidden scope:
  - production renderer or platform changes
  - other plan contracts, handbook, CI or benchmark-schema changes
  - new performance optimization or device performance collection
- Hard gates:
  - PASS local and live-remote ref identities are explicit
  - PASS ancestry and all seven post-main commit identities are unambiguous
  - PASS all observed worktrees were clean before branching
  - PASS inherited accepted/rejected/deferred evidence is indexed
  - PASS canonical evidence authorities exist at the frozen commit
  - PASS final diff contains only the two A0-owned documentation files
- Observations:
  - `main`, cached `origin/main` and live remote `main` all equal `28f77d0`
  - full-quality is seven commits ahead and zero behind `main`
  - the full-quality ref has no live remote branch
  - no device or performance observation was collected
- Required endpoints for claim:
  - none; A0 makes a repository-topology and document-identity claim
- Performance correction used: no
- Known correctness issues: none
- Commands:
  - `git status --porcelain=v2 --branch --untracked-files=all`
  - `git worktree list --porcelain`
  - `git ls-remote --heads origin main codex/full-quality-native-rendering`
  - `git merge-base` and `git rev-list --left-right --count` for the three refs
  - `git log --reverse --format=... main..codex/full-quality-native-rendering`
  - `git cat-file -t` for the seven named commits
  - `shasum -a 256` for canonical evidence authorities
  - active-plan local Markdown link validation
  - `python3 -m json.tool tests/perf/full-quality-matrix-plan-v1.json`
  - `python3 tests/perf/validate-full-quality-experiment.py
    tests/perf/full-quality-matrix-plan-v1.json --allow-incomplete`
  - `git diff --check`
- Evidence:
  - [A0 baseline and evidence inventory](a0-baseline.md)
- Decision: use `codex/native-render-core-refactor`, created from `c478252`;
  do not merge to `main`, rebase or cherry-pick before A1
- Decision reason: the history is already linear; the dedicated branch keeps
  `main` unchanged, preserves artifact-linked commit identities and provides a
  reversible package boundary.
- Commit: `2aef9f0e209c36fc24c706c284e02a181d275b8c`
- Next eligible task: A1 — add source-size and dependency ratchet

### A0 evidence-boundary follow-up

- Outcome: Accepted
- Started: 2026-07-23 18:52 CST
- Ended: 2026-07-23 18:57 CST
- Baseline: `2aef9f0e209c36fc24c706c284e02a181d275b8c`
- Scope: documentation-only correction to the A0 evidence classification and
  artifact identity ledger
- Hard gates:
  - PASS semantic/correctness, directional performance and capacity-only are
    distinct inherited classes
  - PASS dirty A065/PlayCanvas, clean descriptive Preproject and capacity-only
    Garden/Bicycle boundaries are explicit
  - PASS the full-quality matrix is recorded as schema-valid but 0/339 complete
  - PASS ignored artifact locators are workspace-relative and not claimed as
    worktree contents
  - PASS A1 cannot combine cross-commit performance; final qualification belongs
    to Package Q
- Device/performance work: none
- Decision: A0 remains Accepted with narrower evidence claims; the integration
  baseline and A1 eligibility are unchanged
- Commit: `9df0d6cb2e7e7df7ddc95e85d16c416e4e91d4b0`
- Next eligible task: A1 — add source-size and dependency ratchet

### A1 — Add source-size and dependency ratchet

- Final state: Accept
- Started: 2026-07-23 18:59 CST
- Ended: 2026-07-23 19:14 CST
- Baseline commit: `9df0d6cb2e7e7df7ddc95e85d16c416e4e91d4b0`
- Worktree before task: clean detached checkout at the baseline commit; branch
  `codex/native-render-a1-ratchet` was created before the first edit
- Hypothesis: a deterministic standard-library-only checker can freeze A0 physical LOC,
  prevent new giant source files and enforce future render-core dependency
  directions without changing production behavior or scanning legacy owners as
  if the target module tree already existed.
- Dependencies: A0 Accepted
- Allowed scope:
  - one executable checker, policy, fixture matrix and self-test under
    `tests/architecture/`
  - this A1 ledger entry
- Forbidden scope:
  - renderer, shader, API, FFI or platform-consumer changes
  - benchmark schema, CI or handbook changes
  - responsibility movement, device/browser runs or performance collection
- Hard gates:
  - PASS physical LOC counts comments, blank lines and embedded tests
  - PASS all current target breaches are checked exactly: 21 production Rust
    files plus 3 WGSL files, each with A0 LOC, owner task and exit condition
  - PASS every grandfather entry retains immutable A0 LOC plus a checked
    no-growth baseline; any amount of shrink is accepted without forcing code
    padding or metadata churn, and owner closeout while over the normal profile
    remains detectable
  - PASS production Rust `<800` target / `1,200` hard ceiling, concrete plan
    `<600` target / `1,000` hard ceiling, render `lib.rs <200`, future renderer
    orchestrator `<800`, and WGSL `<350` are encoded
  - PASS the top-level orchestration `<150` rule is explicitly disabled because
    A0 has no reliable single-function boundary; creation of
    `renderer.rs` / `renderer/mod.rs` or E1 closeout forces exact function
    activation
  - PASS exceptions require baseline LOC, maximum temporary delta, reason and
    removal task; terminal removal-task closeout expires them
  - PASS future `gpu.rs` / `gpu/` imports of
    renderer/plans/policy/evidence/platform hosts, plan submit/present/poll/map,
    host plan/cache/adaptive ownership, runtime `Vec<Box<dyn ...Pass>>` and
    public `RenderPlan` are covered by positive and negative fixtures
  - PASS future GPU modules reject aliases that hide `crate/self/super` module
    roots; ordinary item aliases remain legal and grouped imports resolve before
    direction checks
  - PASS any plan file with a per-frame function forbids environment reads and
    pipeline creation across the entire file; only an exclusively
    `preparation_only_files` entry may contain them, and an unclassified plan
    file fails closed
  - PASS function boundaries accept array-return semicolons plus
    generic/lifetime signatures instead of using a first-semicolon heuristic
  - PASS `surface.rs` / `surface/` and `offscreen.rs` / `offscreen/` cover named
    and tuple ownership of PlanId/Controller/FrameState plus explicit renderer
    generation fields/types without banning presentation generations
  - PASS the static A/E/M/B/S/Q task catalog aggregates one active/completed
    ledger per opened package; A is always required, E follows A9, M follows
    E13, and B/S/Q each follow M8; duplicate package ledgers, undeclared
    multiple-Active sets, duplicate/conflicting tasks, unknown task/state and
    wrong-package records fail closed
  - PASS every policy task reference is cataloged, including both activation
    tasks; E1 terminal state requires a real per-frame plan boundary
  - PASS task state is read only from one explicit machine block per ledger;
    `Accept/Reject/Defer` and `Accepted/Rejected/Deferred` normalize to terminal
    states without scanning narrative prose
  - PASS source discovery enumerates configured source-root globs directly and
    does not walk repository-wide build output; binding-root build products use
    explicit path exclusions without hiding legitimate production modules such
    as `crates/*/src/build/`
  - PASS the checker itself is 1,192 physical lines and self-ratcheted below
    1,200 after deleting the incomplete helper call graph
  - PASS checker self-tests and checker against the real A0 tree
  - PASS `cargo check --workspace --locked`
  - PASS final whitespace and scope checks
- Observations:
  - real-tree checker reports `38 production Rust, 25 WGSL, 24 grandfathered`
  - warm real-tree timing after the directed-glob/lexer fix was `0.17 s` and
    `0.17 s` in consecutive `/usr/bin/time -p` correction runs
  - generic Rust and plan targets are reported notices until their hard
    ceilings; specialized render `lib.rs`, renderer and WGSL targets are errors
  - no renderer timing, FPS, device, browser or competitor observation was
    collected
- Required endpoints for claim: none; A1 makes a deterministic repository
  policy and fixture-test claim
- Performance correction used: no; no performance hypothesis exists in A1
- Known correctness issues: none
- Commands:
  - `PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py`
  - `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py`
  - `/usr/bin/time -p env PYTHONDONTWRITEBYTECODE=1
    tests/architecture/check_source_architecture.py`
  - `python3 -m json.tool` for the policy and fixture JSON
  - `cargo check --workspace --locked`
  - `git diff --check`
- Evidence:
  - `tests/architecture/source_architecture_policy.json`
  - `tests/architecture/fixtures/cases.json`
  - `tests/architecture/test_source_architecture.py`
- External owner tracking:
  - `IO-PLY-1` and `IO-SPZ-1` are deliberately outside the current native-core
    work-package map; each entry has `review_task: A9`
  - once A9 reaches any terminal closeout, an unchanged entry fails with
    `grandfather.review_due`; A9 must therefore add that IO task and a static
    active/completed ledger pair to `program_task_state`, reassign an already
    cataloged owner, or remove the entry after meeting its exit condition
- Claim boundary:
  - dependency checks are deterministic lexical architecture checks, not a
    complete Rust type resolver; comments and literals are removed before
    matching, target directories and exact frame bodies are configured, and
    missing future boundaries fail closed
  - A1 moves no production responsibility and changes no render behavior
- Decision: Accept
- Decision reason: every declared A1 guardrail is executable on fixtures and
  the real A0 tree, all required local gates pass, and the diff remains inside
  the checker/test/config/ledger boundary.
- Result identity: `9df0d6c..codex/native-render-a1-ratchet`; resolve the
  branch tip SHA when integrating the correction
- Next eligible task: A2 — extract `api.rs` and strategy-free
  `data/{layout,view}.rs` types
- A2 input:
  - start from accepted A1 commit
    `f6180844bbaf910b74ff5ecfe81c9b9588c88561` and keep the checker passing
    before and after the extraction
  - use responsibility cohesion, dependency direction, navigation and test
    seams for module boundaries; numeric size notices are diagnostic only
  - A2 does not need to activate future `plans/` or `renderer/mod.rs`; if it
    introduces either path, it must configure the exact boundary rather than
    suppressing the activation failure
  - moving embedded tests alone is not evidence that an A2 responsibility moved

### Reviewer correction after A1

- Outcome: Accepted
- Baseline: `b87c17fd901012ea5ca583e25af125902d100036`
- Scope: checker, static policy, adversarial fixtures/self-tests and this ledger
  only; no production, shader, API, FFI, platform, benchmark-schema, CI or
  handbook change
- Result identity: `9df0d6c..codex/native-render-a1-ratchet`; the integration
  handoff must record the resolved branch-tip SHA because a commit cannot
  truthfully contain its own object ID
- Verification:
  - PASS 39-case fixture matrix
  - PASS real-tree checker: `38 production Rust, 25 WGSL, 24 grandfathered`
  - PASS two real-tree timing observations at `0.17 s`
  - PASS `cargo check --workspace --locked`
  - PASS JSON, Python syntax and whitespace checks
- Decision: keep A1 Accepted after the reviewer-blocking ratchet, dependency,
  function-boundary and cross-package expiry cases pass locally
- Next eligible task: A2, from the resolved correction tip

### Final acceptance correction after A1

- Outcome: Accepted
- Baseline: `d85312fcac16576131f716eb78c1e5dc3e9017b4`
- Scope: the same five A1-owned checker/policy/fixture/self-test/ledger files;
  no production, shader, API, FFI, platform, benchmark-schema, CI or handbook
  change
- Simplification result:
  - deleted the incomplete plan helper call graph and made per-frame plan files
    whole-file pipeline/environment-free
  - replaced alias expansion with a conservative ban on GPU root-module aliases
  - retained only bounded signature parsing for exact configured function LOC
    and existence checks
  - reduced the checker from 1,318 to 1,194 physical lines and added a `<1,200`
    self-ratchet
- Registry result: static package lifecycle, one-Active enforcement, complete
  policy task-reference validation and consumed E1 plan activation are covered
  by negative fixtures
- Verification:
  - PASS 52-case fixture matrix
  - PASS real-tree checker: `38 production Rust, 25 WGSL, 24 grandfathered`
  - PASS `cargo check --workspace --locked`
  - PASS JSON, Python execution/syntax, diff/show whitespace and bytecode checks
- Result identity: `d85312f..codex/native-render-a1-ratchet`; resolve the final
  branch-tip SHA during integration
- Decision: keep A1 Accepted; the final rules are simpler, fail closed at
  package/file boundaries and have no remaining reviewer-blocking parser path
- Next eligible task: A2, from the resolved final correction tip

### Root acceptance correction after A1

- Outcome: Accepted
- Baseline: `958587121370e89a077f32f8dafcb5b7f05faf87`
- Scope: the same five A1-owned checker/policy/fixture/self-test/ledger files;
  the root coordinator took over final acceptance and changed no production,
  shader, API, FFI, platform or benchmark behavior
- Closed review gaps:
  - binding-root build products are excluded by exact paths without hiding a
    legitimate production `src/build/` module
  - platform hosts reject domain-prefixed cache generations while continuing
    to allow presentation identity generations
  - valid outer-group and `super::super::{self as root}` aliases cannot hide a
    forbidden GPU dependency
  - a completed package ledger cannot retain an Active task
- Verification:
  - PASS 56-case fixture matrix and all checker self-tests
  - PASS real-tree checker: `38 production Rust, 25 WGSL, 24 grandfathered`
  - PASS checker self-ratchet at 1,192 physical lines
  - PASS `cargo check --workspace --locked`
  - PASS JSON, in-memory Python compilation and whitespace checks
- Result identity: `f6180844bbaf910b74ff5ecfe81c9b9588c88561`
- Integration: fast-forwarded without conflict into
  `codex/native-render-core-refactor`
- Decision: A1 is eligible for fast-forward integration only with this root
  acceptance correction included
- Next eligible task: A2a, the strategy-free data/layout/view extraction

## Decision ledger

| Task | State | Commit | Evidence/report | Decision summary |
| --- | --- | --- | --- | --- |
| Plan bundle | Accepted | `c478252` | this directory | complete route, architecture, protocol and finite-task ledger written |
| A0 | Accepted | `2aef9f0` | [a0-baseline.md](a0-baseline.md) | dedicated integration branch from `c478252`; no merge/rebase/cherry-pick |
| A0 evidence audit | Accepted | `9df0d6c` | [a0-baseline.md](a0-baseline.md) | evidence classes and artifact identity limits tightened; integration decision unchanged |
| A1 | Accepted | `f6180844bbaf910b74ff5ecfe81c9b9588c88561` | `tests/architecture/` and this ledger | root-accepted physical-LOC/dependency ratchet passes 56 fixtures and the A0 tree; A2a is eligible |
| A2a | Complete | `3758fd614bc09c5f330210cf87a120dd9fd0ccdd` | `data/{layout,view}.rs` and this ledger | root-accepted strategy-free ABI and real data views; API/Resident/shader/lifecycle unchanged |
| A2b | Complete | `d041a4a3cbefaa70f8647f65ff575727acbb3589` | `api.rs` and this ledger | root-accepted stable API leaves; public root paths preserved and `lib.rs` ratchet lowered exactly |
| A2 | Accepted | `d041a4a3cbefaa70f8647f65ff575727acbb3589` | A2a/A2b records above | data and API leaf extraction complete without owner, policy, shader, FFI or lifecycle changes; A3 is eligible |
| A3 | Accepted | `3fe4c3e` | A3a/A3b records above | Resident CPU ownership, GPU resource arithmetic, Direct/Packed preflight and errors have one explicit owner; exact-count cross-target evidence retained |
| A4 | Accepted | `0cf228f` + `b47d08c` | A4a/A4b records above and A065 exactness artifact | Existing CPU/SIMD/Rayon sort and renderer visibility/depth/key primitives have focused owners; behavior and full-count native rendering remain unchanged |
| A5a | Accepted | `53eb004` + `53d7d57` | A5a record above, fixed-SHA review and integrated gates | External-prefix scan and stable radix now have strategy-free GPU owners; product admission and behavior remain unchanged; A5 stays Active |
| A5b | Accepted | `835b01e` + `0fd16a8` | A5b record above, fixed-SHA review and integrated gates | Direct/fallback and qualified Resident stable full32 radix mechanics now share the private radix/scan owners; labels, resources, shaders, policy and behavior remain unchanged; A5 stays Active |

## Baseline evidence inherited, not rerun by default

A0 should validate identities and links, then reuse these completed records:

The validation and terminal inventory are now recorded in
[a0-baseline.md](a0-baseline.md). The links below remain the detailed evidence
sources.

- [completed task plan](../../completed/2026-07-22-full-quality-native-rendering/task_plan.md)
- [completed final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md)
- [completed design](../../completed/2026-07-22-full-quality-native-rendering/design.md)
- [completed findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md)
- [CPU parallel radix](../../completed/2026-07-22-full-quality-native-rendering/cpu-parallel-radix.md)
- [Preproject architecture](../../completed/2026-07-22-full-quality-native-rendering/phase2-preproject-c-architecture.md)
- [Android evidence](../../completed/2026-07-22-full-quality-native-rendering/android-surface-evidence.md)

Key inherited facts:

- Exact Resident and full-count SH3 Truck/Garden/Bicycle rendering are already
  established.
- Stable CPU full32 ordering, portable GPU ordering, Exact ProjectedQuads and
  S/V/C/D evidence already exist.
- Existing NEON/AVX2/Rayon code must be consolidated, not marketed as greenfield.
- Preproject/Compact already exists and is image-exact; it needs clean ownership
  as a complete plan.
- The A065 nine-run CPU/GPU/Adaptive result and A065 PlayCanvas result are
  dirty directional observations, not clean final-binary qualification.
- A065 Preproject is clean descriptive evidence at `7cabb6e`, but its pairing
  metadata is null; Garden/Bicycle results at `76a9267` are capacity-only.
- The full-quality matrix is a schema-valid plan with 0 rendered of 339
  expected cells, not a completed unified-binary qualification.
- A1 may inherit semantic or correctness proofs, but performance numbers from
  distinct commits/binaries cannot be stitched together; Package Q owns clean,
  same-protocol, final-binary qualification.
- Fixed-slot Paged is not streaming and is not the Scalable foundation.

## Rejected experiments that require new evidence before reopening

The following are not silently rescheduled under a new name:

- Adreno radix8 without complete order and image qualification;
- the slower portable radix8 local-rank variant;
- early-fragment support variants that regressed full-plan performance;
- random point subsampling as LOD;
- four-slot Paged productization;
- treating 640x360 as product qualification;
- treating iOS simulator timings as device performance;
- using one fixed point-count threshold for CPU/GPU selection.

A future task may reopen one only if it states what material condition changed
and why the prior evidence no longer answers the question.

## Task closeout template

```markdown
### Xnn closeout

- Final state: Accepted / Rejected / Deferred
- Ended: YYYY-MM-DD HH:MM TZ
- Baseline: `<sha>`
- Result commit: `<sha>` / none
- Performance correction used: yes / no
- Known correctness issues at closeout: none
- Hard gates:
  - PASS ...
  - PASS ...
- Observations:
  - metric, endpoint and scope
- Evidence:
  - `path/to/artifact`
- Removed/reverted code:
  - ...
- Claim boundary:
  - ...
- Next eligible task:
  - Xnn
```

## Program closeout checklist

The program roadmap has no single monolithic implementation goal. Each package
opens its own active plan/progress ledger and closes independently. This master
bundle may move to `docs/plans/completed/` after Package A closes and the future
package entrypoints have been created or linked; later packages continue to
reference it.

Any package may close only when:

- no task in that package is Active;
- every started task in that package has Accepted/Rejected/Deferred state;
- no rejected experiment remains enabled in production;
- the worktree is clean;
- source-size/dependency ratchets pass;
- handbook architecture and verification docs match the implemented tree;
- retained benchmark artifacts validate;
- public/FFI/API changes have their required smoke evidence;
- final report distinguishes Exact, Balanced and Scalable claims;
- remote branch/PR/merge status is recorded without implying checks that did
  not run.
