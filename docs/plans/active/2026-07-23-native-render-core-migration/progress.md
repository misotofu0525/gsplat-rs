# Native Render Core Migration Progress

> Program roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
>
> Accepted shadow-core package:
> [Package E final report](../../completed/2026-07-23-native-render-exact-core/final-report.md)

## Machine task-state registry

The architecture checker records only Active or terminal tasks in this block.
Unstarted tasks remain outside the machine registry until activated.

<!-- gsplat-program-task-states: begin -->
M0 = Accepted
M1 = Accepted
M2 = Accepted
M3 = Accepted
M4 = Accepted
M5 = Accepted
M6 = Accepted
M7 = Accepted
M8 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: M — atomic product migration and legacy deletion.
- Dependency: E13 Accepted at Exact implementation tip
  `d721ea6cd0c334e28d3ad5c28792383524e27935`.
- M0 state: Accepted after fixed-SHA review of cutover tip
  `e26a1df39780e744112924eb378e098c29be7cd4`.
- M1 state: Accepted at integrated implementation/evidence tip
  `dc3e0de65073f819b714229b726ca55f47c6e6d6`.
- M2 state: Accepted. M2p1, M2p2, M2a, M2p3c, M2p4 and M2b are accepted.
- M3 state: Accepted. Its serial implementation/review slices ran as visible
  Codex tasks from accepted root tips.
- M4 state: Accepted for its defined Chrome/WebGPU/WASM consumer scope.
- M5 state: Accepted for its defined Android/JNI/AAR consumer and A065 strict
  evidence scope. See [M5 Android strict evidence closeout](m5-android-strict-evidence.md).
- M6 state: Accepted for the defined Apple/GsplatKit/XCFramework functional
  consumer scope. Physical-iPhone qualification remains explicit Deferred; see
  [M6 Apple functional evidence closeout](m6-apple-functional-evidence.md).
- M7 state: Accepted at source/evidence tip
  `2739f4899facc03e6a0fb35c23b42d9762ede9cc`. An independent fixed-SHA
  source-ownership audit accepted the `lib.rs`, `surface_session.rs` and
  `surface_presenter.rs` exit boundaries with no P0/P1/P2. The complete
  39-commit M7 range is tree-exactly rollbackable to accepted M6, the root
  static/Metal/FFI/WASM matrix passed, and the formal A065 exactness artifact
  passed canonical validation. See the
  [M7 final acceptance audit](m7-final-acceptance.md).
- M7 integrated closeout chain after that source/evidence tip is
  `0d291c5ea9b911fdfce8d064ff6617ee6f9955ed` (final acceptance and registry),
  `2e16fff072d6832d602d35c53e1c0582220ec9a2` (renderer-test total
  correction), then `1de3f79fa2fa22955f99c887bea421c918e31ee0`
  (retire the three satisfied M7 architecture grandfather records).
  `M7_ACCEPT_SHA == M8_BASE_SHA` is therefore `1de3f79`, not the earlier
  source/evidence tip.
- Product state at M2 activation: native Packed offscreen, desktop
  non-interactive and bench-runner use the Exact runtime; the interactive
  `SurfaceRenderSession` remains legacy until its complete M2 candidate is
  accepted.
- M8 state: Active from fixed base
  `1de3f79fa2fa22955f99c887bea421c918e31ee0`. The first slice is the
  documentation-only [M8 closeout inventory](m8-closeout-inventory.md).
  Package M remains Active; activation is not M8 acceptance or Package M
  completion.
- Stable v0.1 signatures/layouts, Web behavior and rendered semantics remain
  frozen during M2. Additive current-stats v1 receipts and pending-compatible
  Android/Apple translation are prepared before M2 activation. No root
  acceptance batch may switch the legacy getter until every in-tree live
  Surface consumer has stopped requiring its stale counts.

## Execution coordination

- **Goal execution constraint:** “isolated thread and worktree” in the active
  goal means a newly created, visible Codex task for every concrete code,
  review, benchmark, experiment, or repair slice. It explicitly excludes
  collaboration subagents for program work. The root task will not use a
  subagent as a substitute for a visible task/thread: it only coordinates,
  integrates, and performs critical verification.
- Every independently assigned implementation, review, benchmark or repair
  slice after M1 is dispatched as a new visible Codex task with its own
  worktree, fixed accepted base, narrow owned paths and one reviewable
  candidate SHA. Concrete program work is never delegated through the
  collaboration-subagent mechanism: it belongs to a visible task/thread.
- This applies without exception to follow-up repairs and small verification
  fixes: creating a task is not optional merely because the change is narrow.
  The root task may inspect, integrate and run final gates, but it does not
  hand a concrete code, test, review or experiment slice to a subagent.
- This visible-task rule is part of the active goal, not a temporary scheduling
  preference. Every later task brief must carry it forward; omitting it does
  not authorize concrete work in a subagent.
- **Enforcement:** a concrete implementation, review, benchmark, experiment or
  repair performed through a collaboration subagent is not an acceptable
  milestone deliverable. It must be discarded or independently repeated in a
  newly created visible Codex task with its own worktree before root acceptance.
  The root task records the replacement task and its fixed base in this bundle.
- The root task owns dependency order, fixed-SHA acceptance, integration and
  critical cross-platform verification. A candidate is never integrated merely
  because its implementation task reports completion.
- The root task performs its own critical review and verification; it may
  create visible read-only review tasks, but never substitutes a subagent for
  a visible Codex task. This keeps task history, goals, worktree state and
  final handoffs independently inspectable.
- Parallel Codex tasks are opened only for slices whose ownership and base make
  concurrent work safe. Dependent migrations remain serial rather than being
  forced into parallel execution.

## M0 closeout

- Final state: Accepted.
- Fixed parent/E13 closeout:
  `328e05c4cb55f4824cc9149c08629dd9d2ad5eed`.
- Cutover commits:
  - `4e3f9414b2fff936fb130e02628d09cc6924f3c1` added the complete migration
    contract;
  - `e26a1df39780e744112924eb378e098c29be7cd4` closed all fixed-SHA review
    findings and is `M0_CUTOVER_SHA`.
- Deliverable: [cutover.md](cutover.md) freezes Exact invariants, serial
  implementation order, read-only parallel audit boundaries, unique owners,
  frozen consumers, rollback identities, fixed Kitsune/trace inputs and honest
  artifact acceptance for M1--M8.
- Scope result: documentation only. No Rust, WGSL, API, ABI, wrapper, build
  script, benchmark protocol or product route changed.
- First independent review found five evidence/rollback P1 issues and two
  fixed-SHA/owned-path P2 issues. The follow-up commit closes all seven; the
  second fixed-tip review reported no remaining P0/P1/P2 findings.
- Verification passed:
  - architecture checker self-tests and real-tree policy;
  - all relative Markdown links;
  - canonical validation and raw/content identity for the three fixed Kitsune
    traces;
  - full-quality validator tests;
  - committed-range `git show --check` and `git diff --check`;
  - exact two-commit scope and clean worktree.
- Deferred facts are explicit rather than blockers: the uncommitted Kitsune
  PLY must be freshly fetched/hashed before retained scene evidence, and a
  physical iPhone is probed only in M6.
- No FPS, PlayCanvas or cross-device performance run was performed or claimed.
- The SHA of this root-owned closeout/activation commit is
  `M0_ACCEPT_SHA == M1_BASE_SHA`; it is reported in the handoff and recorded by
  M1 because a commit cannot contain its own object ID.

## M1 activation contract

- Objective: switch the native Packed offscreen path, desktop non-interactive
  consumer and bench-runner to one Exact runtime without changing public
  signatures or any interactive/platform consumer.
- Unique semantic owner: `Renderer` owns one `PreparedRuntimeSlot` for Packed
  offscreen scene, plans, controller, generations, mandatory sampler and frame
  result. The offscreen host owns only the shared device/queue, target and
  readback. It must not retain or clone a second Packed resident scene.
- Required implementation slices:
  1. renderer/offscreen seam with transactional complete GPU preparation,
     forced per-call CPU PostSort refresh and honest phase timings;
  2. desktop non-Surface host separation while the interactive viewer remains
     on the unchanged M2-owned Surface route;
  3. bench/offscreen collector support for the frozen Kitsune 1920x1080 trace,
     complete Exact receipts and final-frame identity.
- Public compatibility:
  - existing `Renderer` constructors, loading/render/readback/wait/resize and
    inspection signatures remain unchanged;
  - `FrameStats` layout and real preprocess/sort/raster meanings remain
    unchanged;
  - `GeometryPath` public default remains Direct; explicit Direct remains the
    wide-f32 image oracle and explicit Paged remains diagnostic only.
- Transaction boundary:
  - failed resident/GPU/plan/raster preparation retains the old scene, image,
    generations and stats;
  - resize publishes target/config only after successful allocation and the
    Exact viewport generation only after a successful rendered frame;
  - readback does not mutate renderer policy or timing state.
- Frozen paths/consumers: shared Surface/session/presenter, C ABI/header, FFI
  wrappers, Web/WASM, Android/JNI/Kotlin, Apple/Swift/iOS, WGSL/raster math and
  competitor protocol.
- Required acceptance evidence:
  - focused SH0--SH3, stable order, failure rollback, resize/readback and owner
    identity tests;
  - Packed Exact versus Direct oracle image/count parity;
  - one canonical Kitsune 1920x1080 offscreen artifact using the frozen trace,
    complete count/SH/resolution/plan receipts and final frame;
  - workspace, strict Clippy/Rustdoc, forced Metal conformance, architecture,
    benchmark artifact and FFI regression checks required by the cutover
    contract.
- Performance values are observations only. M1 has no fixed FPS, speedup or
  competitor-ratio acceptance threshold.

## M1 closeout

- Final state: Accepted.
- Fixed base: `49821f9df4784e60bf176f4c6762694eb406f710`.
- Integrated implementation/evidence tip:
  `dc3e0de65073f819b714229b726ca55f47c6e6d6`.
- Accepted implementation slices:
  - `3021ef9525490ddae0bd17daba059b72183dcae7` and
    `bf1da9d20568d7e8c3ab13366ca0775c38f07704` route native Packed
    offscreen through one transactional Exact runtime and prove ownership,
    rollback, stable ordering, SH0--SH3 and Direct-oracle behavior;
  - integrated commit `4bd4461` separates desktop CLI, trace, scene,
    offscreen, viewer and image-output host responsibilities without changing
    the interactive Surface route;
  - integrated commits `de8e8eb` and `dc3e0de` add the trace-aware Packed
    bench collector, atomic artifact publication, frozen M1 qualification
    contract and image-aware artifact validation.
- The first fixed-SHA bench review rejected three evidence defects: a guessed
  actual PlanId, an unbound `--full-quality` flag and an unvalidated final
  image. The replacement candidate removed the guessed value, records
  `renderer.exact_plan_actual` as unavailable, binds full-quality runs by
  dataset/trace content identity and validates the actual PNG file, size and
  SHA-256 before accepting the artifact.
- Formal retained local evidence:
  - dataset `kitsune`, SHA-256
    `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`,
    279,199 splats and source/resident SH3;
  - trace `candidate-kitsune-quality-2view-1920x1080-v1`, canonical SHA-256
    `8821c193506cdf7d67aa200248a45088c4750a3cd128ee29f6e2dc2d3a5bdb99`,
    sequence `[0, 1]`, 20 warmup and 80 measured frames;
  - Packed artifact
    `target/benchmarks/migration-m1-kitsune-formal` and completed one-cell
    suite `target/benchmarks/migration-m1-kitsune-suite.json` both pass their
    repository validators with all 279,199 points visible and drawn;
  - the Apple M4 Metal observation was 9.806 ms mean queue-complete frame time,
    12.883 ms p95 and zero missed 16.67 ms frames. These values describe this
    run only and are not an acceptance threshold or competitor claim.
- The same scene, trace and resolution were rendered through the Direct oracle.
  Packed RGB mean absolute error was `0.000004691`, no pixel exceeded `3/255`,
  and the maximum channel difference was `1/255`. Three of 2,073,600 alpha
  pixels differed by one byte step; a repeated Packed run was byte-identical,
  so this deterministic path-rounding boundary is recorded rather than hidden.
  The repository's existing focused Kitsune Direct/Packed image gate also
  passes.
- Final verification passed: formatting, workspace check/tests, strict
  workspace Clippy, Rustdoc, forced Metal SortedAlpha conformance, source
  architecture policy, benchmark and full-quality validators, desktop default
  and interactive-feature tests, plus C ABI smoke. Dependency-policy status is
  recorded separately if its external advisory refresh is unavailable.
- The dependency-policy wrapper made no progress while refreshing remote data
  and was stopped; an offline retry could not resolve uncached conditional
  Windows/Khronos crates. This is recorded as environment-unavailable rather
  than a dependency-policy pass or a source failure. M1 introduced no new
  runtime rendering dependency beyond the bench artifact tooling captured in
  `Cargo.lock`.
- No public signature, C ABI, interactive Surface, Web, Android or Apple
  consumer changed in M1. Direct remains the explicit wide-f32 oracle and
  Paged remains diagnostic only.
- The SHA of this root-owned closeout/activation commit is
  `M1_ACCEPT_SHA == M2_BASE_SHA`; it is reported in the handoff and recorded by
  M2 because a commit cannot contain its own object ID.

## M2 activation contract

- Objective: replace the shared real-window Surface's legacy semantic owner
  with the accepted Exact runtime while preserving stable ABI/layouts, pixels,
  constructors and platform behavior, and while making asynchronous counts
  explicit instead of returning stale values as current.
- Unique owner, frozen paths, rollback identity and required evidence remain
  exactly those defined by [cutover.md](cutover.md#m2--shared-real-window-surface).
- M2 begins only from this Accepted M1 tree. Its implementation is dispatched
  as a separate Codex task/worktree; the root task retains fixed-SHA acceptance,
  integration and real-Surface validation.
- The first M2a candidate `bd1d12a4418d76e68b6e0a408dc8b976730e3a05`
  remains Rejected after fixed-SHA review. It correctly established a single
  Exact Surface owner and present-after-submit publication, but it allowed a
  stale prepared scene to replace a newer scene, did not make projected policy
  constrain the actual complete plan, removed one experimental live switch
  without an explicit contract decision, and exposed unresolved GPU counts as
  successful stale `FrameStats`. The repair candidate
  `166b457942e55947c68fbc8b9906de8e37043f26` is also Rejected after independent
  review: it correctly closes scene currentness and candidate-added wasm
  diagnostics, but still accepts forced CPU/GPU plus projected Adaptive while
  executing Candidate with adaptive state disabled. Neither candidate is
  integrated; their valid changes are reference material for the later M2a
  rebuild on the accepted preparation tree.
- Root approves one narrowly scoped experimental compatibility decision:
  after Native Surface scene publication, any runtime mutation entering or
  leaving Packed is unsupported and fails before mutation; same-path calls are
  idempotent and constructor-time Direct/Packed selection remains supported.
  This does not change the stable `context_*` ABI, Direct-to-Paged, or Web.
- M2 is delivered as serial, independently reviewable slices:
  1. **M2p1 — Renderer current-stats receipts:** add bounded, non-blocking,
     generation-safe request/submission/terminal V/C/D receipts using the sole
     Renderer sampler and actual plan-owned count sources. Ordinary frames do
     no count readback; optional evidence pressure cannot change policy.
  2. **M2p2 — additive C v1 bridge:** add versioned request/submission/receipt/
     failure symbols and structs without changing existing layouts or making
     the C layer a state owner. The legacy getter is not switched yet.
  3. **M2p3 — pending-compatible platform adapters:** Android and Apple are
     separate visible tasks from the same accepted M2p2 base. They consume the
     v1 status/receipts, treat Pending/Busy as non-fatal, and never display or
     retain counts without matching ticket and generation.
  4. **M2a — Surface semantic cutover:** replace the legacy Surface semantic
     writer with the accepted Exact runtime, preserve lifecycle ownership and
     prove forced CPU PostSort, GPU PostSort, GPU Preproject, Adaptive,
     acquire/configure/present ordering and retry rollback through focused and
     injected-presentation tests. It also enforces the approved construction-
     time-only Packed choice and a single canonical complete-plan state. Every
     successful legacy control setter atomically maps all facade fields to
     CpuPostSort/Candidate, GpuPostSort/Candidate, GpuPreproject/Compact, or
     whole-plan Adaptive; unrepresentable combinations reject before mutation.
     It does not build the benchmark collector.
  5. **M2p3c — live-consumer compatibility completion:** from the fixed M2a
     candidate, Android and Apple run as separate visible tasks. They move
     the real example/UI/benchmark loops off the legacy Surface getter and
     consume only ticket- and generation-matched v1 receipts. Pending or
     unavailable counts remain non-fatal outside a strict retained benchmark;
     a strict benchmark rejects incomplete evidence instead of inventing it.
     M2a and M2p3c are accepted as one migration batch because the legacy
     `FrameStats` layout cannot represent unavailable GPU counts truthfully;
     M2a is not published or declared accepted while a live consumer still
     reads that layout.
  6. **M2p4 — legacy getter fail-closed switch:** only after both M2p3c
     candidates are accepted, pending/unrequested/expired or mismatched counts
     return `NOT_FOUND` without modifying the output. No prior value, zero,
     capacity, or sentinel is substituted.
  7. **M2b — real-window evidence seam:** on the accepted
     M2a/M2p3c/M2p4 tree, add or extend the native Surface collector/capture
     path so the four policies emit canonical artifacts and final frames. It
     does not redesign render semantics.
- **M2p1 closeout: Accepted.** The independently reviewed candidate range was
  `3874b00bebb2b3c8c7496bb04d3e20c8e1bd2ff7..a7799fee850f88220d65f38b3781c6d678574927`;
  its three patches are integrated as `cf5b74b`, `842cabe` and `702aca3`.
  Renderer/PlanSampler remain the sole request, ticket, generation, bounded
  queue and terminal owners. A poll now consumes at most one pre-ticket
  resolution or one atomic terminal; additional ready terminals stay in the
  Renderer queue and are returned exactly once in ticket order.
- Each terminal carries one inseparable ticket, complete frame/plan/generation
  join identity, S/V/C/D values and count semantics. Surface exposes only
  immutable value DTOs plus request/submission/poll delegates. The still-legacy
  Surface returns `GpuUnavailable` / `NotRequested` / `Empty`, including when a
  Packed renderer happens to own the unrelated M1 offscreen Exact runtime; it
  never targets that runtime or creates a second state owner.
- Three fixed-SHA reviews were required. The first rejected unscoped observer
  resource creation and queue contamination; the second confirmed those fixes
  but rejected the batch-drain and unreachable Surface seam; the final review
  found no P0/P1/P2 findings. Root integration re-ran the 13 focused receipt
  tests, the real Packed legacy-isolation regression, forced Metal SortedAlpha
  conformance, architecture checks and FFI smoke successfully.
- M2p1 changed no C/header, Android, Apple, Web, WGSL or product route and did
  not alter an existing Surface output layout. M2p2 may therefore remain a
  direct value translation over the accepted Surface seam and must not add a C
  queue, cache, tombstone, generation or policy owner. The SHA of this
  root-owned closeout commit is the M2p2 base and is reported in its handoff.
- **M2p2 closeout: Accepted.** Candidate
  `61c347ed56399ce83b082784ef021100522bd1b4` was the single direct child of
  `1d00f2538ad66340f8b28b1e97138a4703181ef9` and is integrated as
  `113bfe1`. It adds only three stateless `_v1` calls: request admission,
  presentation-committed submission identity and one atomic global poll.
  `Ready` carries ticket, complete join identity, S/V/C/D and count semantics
  in one value; terminal failures retain ticket/identity without usable
  counts. The C layer adds no current-stats field to `GsplatSurfaceRenderer`
  and the legacy stats getter remains byte-for-byte unchanged.
- Fixed-SHA review task `019f9417-4768-72b0-b2c3-de5765817467` found no
  P0/P1/P2 findings. Candidate and root verification covered 31 FFI tests, 13
  Renderer current-stats tests, C FFI smoke, workspace check, architecture
  checks, C/C++ headers and the new ABI layouts on 64-bit plus targeted i386
  and ARM32 compilation. The existing full i386 legacy smoke has an unrelated
  pre-existing 44/48-byte layout assertion and is not treated as evidence for
  or against the new structs.
- **M2p3 closeout: Accepted.** Android and Apple were implemented from the
  same accepted `95972ef` base in separate visible tasks and integrated only
  after independent fixed-SHA review. Apple candidates `5928ee15` and
  `9d9f2a48` are integrated as `8bc1b9e` and `d2f057f`; Android candidates
  `0353a6d9` and `0a00ffa8` are integrated as `682f08a` and `012e049`.
- Both initial reviews rejected a terminal-snapshot replay defect. Android's
  review also rejected a failed-presentation request that could later issue an
  unobserved ticket on an ordinary frame. The repair chains keep pre-ticket
  intent, issued pending entries and terminal replay state distinct; terminal
  identity drift fails closed, already-popped resolutions are still accounted,
  and no count is published without matching ticket plus complete identity.
  Apple retains one last-snapshot lifecycle slot; Android relies on Renderer
  admission for live-pending bounds and keeps only a fixed terminal/rejection
  replay window. Neither platform adds a sampler, policy owner or legacy-count
  fallback.
- Final review tasks `019f9444-db2e-7cf0-8c46-72bdec2e1572` (Apple) and
  `019f9454-0c82-7302-baa1-276334c63361` (Android) reported no P0/P1/P2.
  Root integration re-ran 14 Apple XCTest cases, 24 Android current-stats
  cases plus sample tests, XCFramework, Swift/JNI/C smoke, AAR/APK, 13
  Renderer receipt tests, workspace check, architecture self/real-tree policy
  and forced Metal SortedAlpha conformance. Physical signed iPhone and Android
  Ready-receipt qualification remain deferred until the later Surface/device
  slices; packaging success is not reported as device or performance evidence.
- M2p3 Android and Apple implementation may run in parallel because their paths
  are disjoint; all dependent slices remain serial. Root accepts and integrates
  every fixed candidate separately, then owns the Apple M4 real-window run and
  final M2 closeout. M2b cannot begin on an unaccepted M2a candidate.
- **M2a provisional review: Rejected.** Candidate
  `a6c839cb319b847b763c62d358a75f68fce00ae2` is a clean direct child of
  `bf93ad2ca070a2a07ad49051b0890db0a07dc0e1` and passed focused, workspace,
  Clippy, Rustdoc, Metal SortedAlpha, architecture, WASM, cargo-deny, FFI and
  Swift checks. Fixed-SHA task `019f949d-1133-7ea1-8466-32a40025363a` still
  rejected it for two P1 findings and one P2: legacy live consumers see false
  zero V/D on GPU plans; a frame-latency change retains incomparable Exact
  controller evidence; and one GPU-vs-GPU probe is labelled `CpuProbe`.
- **M2a core repair: accepted for root integration, not package acceptance.**
  Visible repair task `019f94a5-bedf-7e20-b2f5-0aec185df089` produced fixed
  candidate `e98c5a21a0e6f410235e574862b1783e6d7efe1c`. Independent read-only
  task `019f94b5-6299-7840-b253-ce2c9159d945` reported no P0/P1/P2: every
  frame-latency setter call restarts only latency-bound whole-plan learning,
  retires the old formal callback behind one bounded queue barrier, preserves
  current-stats tickets and semantic identity, and labels GPU-to-GPU probes
  from the recorded execution lane. Root integrated the provisional runtime
  and repair as `a2bca9d` and `7e0e7c9`, then re-ran 12 controller/sampler
  tests, 15 current-stats tests, workspace check and architecture policy.
- The false-zero finding is not "fixed" with a sentinel, source capacity,
  stale value or synchronous readback. Visible Android task
  `019f94ab-9147-7ff1-b6c3-67c60bd12af4` and Apple task
  `019f94ab-9148-79e0-b921-313b059c21e7` instead remove every in-tree live
  dependency on the legacy getter using matching current-stats tickets.
- **M2a/M2p3c integration closeout: Accepted.** Root integrated the
  accepted Apple and Android current-receipt migration chains as
  `5e6ceab..d1a9e28`. The final Android repair makes the explicit
  `--gpu-producer` collector mode Deferred until M2b rather than publishing an
  unproved producer result; it rejects non-boolean flags, terminal identity
  drift and incomplete ticket sets. Its independent fixed-SHA review task
  `019f952f-92c3-79f2-aa2d-f3c64a91da06` found no P0/P1/P2.
- Root combined verification passed workspace tests/check, strict
  Clippy/Rustdoc, forced Metal SortedAlpha conformance, FFI/JNI smoke, Android
  collector/extractor and legacy-live-stat guards, Android AAR/APK, Swift
  smoke, and the iOS Simulator app. A real A065 Surface smoke at 2412x1080
  created a Packed SH3 Surface with source/decoded/encoded/resident/addressable
  counts all equal to 2,541,226 and a current `cpu_post_sort` receipt. Its
  roughly 198 ms observed call time is a directional functional observation,
  not a performance qualification.
- The same closeout also passed release XCFramework assembly and a fresh
  `wasm32-unknown-unknown` renderer check. The fixed accepted root tip is
  recorded by this closeout commit; M2p4 may use only that tree as its base.
  M2b remains blocked until M2p4 is independently accepted and integrated.
- **M2p4 closeout: Accepted.** Candidate
  `3d70301b2e5d321cfb5e6e1b5da40b3493ef9c5a`, a direct child of
  `502b751cf1795143102e9628eb6381962786ee2f`, is integrated as `c4db3cf`.
  It changes the legacy C getter only through a Surface compatibility
  availability projection: synchronous current counts retain legacy success;
  asynchronous counts are unavailable until a READY terminal exactly matches
  the last presented ticket and complete join identity. Unrequested, pending,
  expired, failed and mismatched states return `NOT_FOUND` without writing the
  caller buffer. Renderer/PlanSampler remain the sole ticket, generation,
  queue and terminal owners.
- Independent fixed-SHA review task `019f9723-ebb8-7f80-b355-67cbfdc33485`
  accepted the candidate with no P0/P1/P2. It independently passed 20
  current-stats tests, 33 C ABI tests, C smoke, workspace check, formatter,
  architecture policy and the Android live-getter guard. Root re-ran the same
  focused checks after integration. ABI layouts, render policy, pixels,
  producer evidence, M2b capture and platform consumers did not change.
- The fixed M2p4 integration tip is recorded by this closeout commit. M2b may
  begin only from that tip and must remain a separate visible task.
- **M2b provisional review: Rejected.** Candidate
  `60b5e929edbdfe3086306dbc25335d9d56ca42bb` correctly keeps renderer policy,
  ticket allocation, count readback and terminal publication in the existing
  Surface runtime. Its retained four-arm Apple M4 Metal suite is internally
  consistent, but it is not accepted as canonical evidence: the collector can
  label a caller-supplied stale executable as the clean source SHA, admits
  arbitrary SH3 workloads/traces/schedules, and emits capture paths that die
  after staging is atomically renamed. A separate visible repair task must
  bind the executable and canonical workload, make published raw logs
  self-revalidating, and add fail-closed mutation coverage before root repeats
  fixed-SHA review or considers integration.
- **M2b closeout: Accepted.** The evidence seam was repaired through visible,
  isolated candidate and read-only-review tasks. Root integrated the accepted
  publication/attestation chain and the terminal-event repair as
  `d4f4246..ce15c90`, then `742bd5a` (`fix: emit surface evidence summary
  once`). The final repair makes terminal success one-way: it commits before
  emitting its summary, and queued post-exit redraws cannot render, capture,
  log another summary or overwrite completion. Its independent fixed-SHA
  review reported no P0/P1/P2 findings.
- Root repeated the complete collector on Apple M4 / Metal at code SHA
  `742bd5a928123557a9016e50655724aa82ef9bd5`, retaining the ignored suite at
  `target/benchmarks/m2b/surface-742bd5a`. It built the locked release viewer
  in a collector-owned target, then retained four validated 1920x1080 Kitsune
  runs: forced CPU PostSort, forced GPU PostSort, forced GPU Preproject and
  Adaptive. Every arm has exactly one begin/capture/summary record, all
  `279,199` source/decoded/resident/addressable SH3 splats, full-resolution
  presentation with sampling/LOD/upscaling disabled, and the same final PNG
  SHA-256 `f4e95120066270d0351bd6fab445f1beac26725dc94ced120583239c85667574`.
  Actual plans were CPU PostSort, GPU PostSort, GPU Preproject and CPU PostSort
  for Adaptive. Root independently revalidated all four raw logs and confirmed
  no executable artifact or retained collector Cargo target. This is
  correctness/evidence acceptance, not a CPU-versus-GPU performance claim.
- M2 final focused gates passed after integration: formatter, three desktop
  terminalization tests, 26 strict collector tests, focused desktop Clippy,
  artifact validators and raw-log validators. No M2b change altered raster
  semantics, ABI, source membership, SH degree or platform-consumer behavior.
- **M3a start slice: Accepted and integrated.** A visible read-only boundary
  audit selected only the already stateless current-stats v1 ABI DTOs and pure
  conversion as the first safe extraction. Candidate
  `d7eb6000f15e8469fe459c10138bbf36e6fcfe31` was independently reviewed with
  no P0/P1/P2 and integrated by root as `6be40a8`. The new
  `current_stats_v1` module owns constants, C layouts and value conversion;
  `lib.rs` keeps crate-root re-exports and all three exported entrypoints,
  preserving validation-before-poll, one session delegation and one output
  write. Header, symbols, unavailable/no-write behavior and platform consumers
  did not change. Root verification passed formatter, 34 FFI unit tests, C
  smoke and architecture checks. JNI host smoke is Environment Unavailable
  because this Mac currently has no JDK; it is neither a code pass nor an M5
  qualification result. M3 remains Active because order/projected/producer
  compatibility queues and count ledgers remain in the FFI handle; merely
  moving them would not remove duplicate ownership.
- **M3b renderer-owned terminal store: Accepted and integrated.** Visible
  implementation candidate `621ff8c967f04d29c2d19ee101d2df41c82b84fa` was
  independently reviewed with no P0/P1/P2 and integrated by root as
  `0452fe6`. `SurfaceRenderSession` now owns compatibility submission,
  terminal and ticket-count state for order, projected and producer evidence.
  Success publishes its terminal and exact count record atomically; failures
  expose no usable counts; counts are ticket-addressed, take-once and explicit
  about pending, expiry, consumption or invalid tickets. Order retention is
  64 terminal records per lane with 128 combined count records, projected
  counts retain 64, and producer keeps its raw terminal FIFO lossless while
  bounding only its compatibility view. Root repeated formatter, 461 renderer
  library tests (8 existing research ignores), workspace check, strict
  renderer Clippy, architecture policy and WASM renderer check. This is still
  not M3 completion: M3c must delete FFI-side duplicate queues and translate
  the unchanged C ABI directly from this renderer-owned seam.
- **M3b field-completeness repair: Accepted and integrated.** The first M3c
  task correctly stopped before editing when it found that its frozen C order
  terminal layouts need GPU visible/drawn and CPU contributor counts without
  consuming the independent ticket-count receipt. A separate visible renderer
  repair candidate `c4855664bb758879e058c0a78573b01b9c9bbf38` was independently
  accepted with no P0/P1/P2 and integrated as `d696cb3`. It copies exactly
  those immutable payload fields into the two order-success DTOs and tests
  both poll-before-take and take-before-poll. No FFI mirror state was retained
  or reintroduced. Root reran formatter, the 13 compatibility tests, renderer
  check and strict Clippy, architecture self-tests/policy and diff checks.
  M3c must restart from this repaired base.
- **M3b producer-seam repair: Accepted and integrated.** The restarted M3c
  task then correctly stopped before editing because the frozen C producer poll
  is one-record-at-a-time while renderer raw evidence exposed only a full
  drain. A separate visible candidate
  `2ace73223dd98f4df501c9b716c8c1930e23352e` was independently accepted with
  no P0/P1/P2 and integrated as `f0f8c0b`. Compatibility polling and raw
  producer FIFO now consume separate visibility views: bounded compatibility
  delivery cannot erase raw records, and renderer-owned single-pop success and
  failure APIs retain exact FIFO beyond the 64-record compatibility window.
  The session also exposes its committed producer-measurement state while
  retaining all transition admission and rollback logic. Root reran formatter,
  456 renderer tests (8 existing research ignores), 34 FFI tests, workspace
  check, strict renderer Clippy, WASM Web check and architecture policy. M3c
  must restart from this second repaired base; FFI must not recreate queues or
  mirror the enabled state.
- **M3c FFI ownership deletion: Accepted and integrated.** Visible candidate
  `bd221592e387b920118ff8d60b8bd8d1a49cc909` was independently accepted with
  no P0/P1/P2 and integrated as `2618ae9`. The C layer now directly translates
  renderer-owned compatibility submissions, terminals, ticket counts and raw
  producer single-pop FIFO. `GsplatSurfaceRenderer` no longer owns
  compatibility queues, ledgers, ticket contexts, pumps, submission caches or
  a producer-enabled mirror. Header content and the Rust exported C-function
  set are unchanged; validation still occurs before session polling or caller
  output mutation. Root M3 gates passed formatter, 35 FFI unit tests, 14
  renderer compatibility tests, C smoke, workspace check, strict FFI Clippy,
  WASM renderer compilation, architecture self-tests/policy and diff checks.
  M3 is therefore accepted for its defined C ABI cutover scope. This does not
  claim a fresh real-window or physical Android/iOS qualification: those remain
  explicit later M5/M6 evidence obligations.
- **M4 browser WebGPU/WASM consumer: Accepted.**
  Root integrated the independently reviewed Web/WASM Exact-session candidate
  `bfeefa8153e52806e281bca118e9b048a3b1ba0a` as `54ed237`, then the separately
  reviewed current-stats repair
  `3685300d8f298e7f0ea461a7d72f40f6a0e2bfe5` as `54baa69`. The first review
  rejected the initial candidate because the retained Web collector still
  required a retired legacy order ticket and exposed pending indirect V/D as
  numeric zero. The repair moves strict artifact admission to renderer-owned
  current-stats submissions and matching generation-, camera-, encode- and
  presentation-bound terminals; pending V/D now remains unavailable through
  Surface, WASM, JS and artifact records. Root and the independent reviewer
  each reproduced adaptive and forced-GPU Chrome/WebGPU minimal artifacts:
  terminals matched their submissions, legacy ledgers remained empty, and
  forced-GPU pending V/D was `null` before its terminal arrived. Root's fresh
  collector artifact at `target/benchmarks/root-m4-current-stats-v2` passed
  the repository validator. These are functional regression checks only.
- Root then integrated the focused moving-trace receipt repairs as `4200199`
  and `31d8aff`, followed by the formal artifact telemetry repair as
  `68651f659d9b97e3ef2fe2149feabbd1073b8b31`. The latter serializes only
  complete per-frame receipt facts: the retained Adaptive run has 80 measured
  frames, seven CPU selections, 73 GPU selections, and zero GPU-sort
  fallbacks. The values sum to the sample count; they are not inferred from a
  requested policy.
- At `68651f6`, root rebuilt the Web/WASM package, reran the frozen Kitsune
  SH3 Packed Exact collector (1920x1080, trace `[0,1]`, 20 warmup and 80
  measured frames), and validated both the `gsplat-benchmark/v1` artifact and
  a task-local `gsplat-full-quality-experiment/v1` suite with
  `--verify-inputs`. The suite retained one rendered cell, all 279,199 source
  SH3 splats, full declared/presented resolution, final-image hash
  `7ac2611619ef39482f7e77a5492b1844945006282e555872023c4ac6fc256247`, and
  `sort_refreshed=true` for all 80 ticketed moving-trace frames.
- This accepts M4 only as one Chrome/WebGPU browser-consumer migration and
  formal functional/quality receipt. It is not a PlayCanvas comparison,
  multi-browser result, cross-device claim, or general performance claim.

## M5 closeout

- Final state: Accepted for the defined Android/JNI/AAR consumer and A065
  strict-evidence scope. At this M5 closeout point, M6--M8 were still pending.
- The detailed retained paths, identities, repair audit trail, validator
  results and residual boundaries are recorded in
  [M5 Android strict evidence closeout](m5-android-strict-evidence.md).
- At `5cc7c97`, forced CPU and forced GPU each completed one strict 80-frame
  Kitsune run under the same 2412x1080 Packed/SH3/no-sampling/no-LOD conditions.
  Each run is directional evidence only and is not a performance comparison.
- The first `5cc7c97` Adaptive attempt failed strict current-stats pre-ticket
  admission. Fixed-SHA candidate `d902c04` retained the same binding and camera
  across retries until a real `Issued` submission; root reviewed it and
  integrated it as `46dce2e`. Pending counts remain unavailable, with no
  synthetic ticket, synchronous readback or CPU fallback.
- A fresh `46dce2e` APK/AAR/device run passed the generic artifact, strict
  current-stats and 80-frame camera-receipt validators with all 279,199 SH3
  splats at 2412x1080 and sampling/LOD disabled. Its actual 79 CPU / 1 GPU
  selection is a functional observation only.
- The initial collector stopped after that successful run because the PLY came
  from another checkout. With no device rerun, a byte-identical, ignored,
  non-symlink canonical dataset copy enabled atomic publication of a complete
  one-cell Adaptive suite. Suite SHA-256 is
  `d9029ac53946b45d19d0c14343416d8c7427076e3b907070f96125f2095eea53`;
  root independently revalidated the full-quality suite, generic artifact and
  all 80 Android camera receipts.
- Resource, power, sustained-thermal, CPU/GPU winner and PlayCanvas claims are
  unverified. All retained `target/` paths are temporary machine-local evidence;
  Git contains no model, PNG, APK, AAR or native library.

## M6 closeout

- Final state: Accepted for the defined Apple/GsplatKit/XCFramework functional
  consumer scope at `8022841957a196824eb9669c477f11ce91d2aab1`.
- CPU and Adaptive are retained Simulator functional/capacity runs at the
  canonical 2622x1206 Kitsune SH3 configuration. Forced GPU is explicitly
  Deferred because the Simulator rejects the backend as unsupported; it was
  neither substituted nor published as a valid artifact.
- Root independently passed the packaged XCFramework/Swift tests, lifecycle and
  collector tests, two artifact validator triplets, and the two-cell
  full-quality suite with freshly verified inputs. No physical iPhone is
  attached, so physical-device qualification remains Deferred rather than being
  represented by Simulator timing.
- The complete identity, receipt, validator and boundary record is
  [M6 Apple functional evidence closeout](m6-apple-functional-evidence.md).
- At this M6 closeout point, M7 and M8 had not been accepted. The current M7
  partial-integration state is recorded below.

## M7 integration and acceptance status

- State: **Accepted** at fixed source/evidence tip
  `2739f4899facc03e6a0fb35c23b42d9762ede9cc`, with integrated M7 acceptance
  tip `1de3f79fa2fa22955f99c887bea421c918e31ee0`. M8 is now Active and Package M
  remains Active.
- The completed 39-commit range `a798b8a..2739f48` removes TiledExact and the
  unreachable standalone Packed graph, preserves the classified Rust/C/Web/
  mobile compatibility boundary, and leaves one product Packed route through
  `SurfaceRenderSession -> SurfacePresenterHost -> PreparedRuntimeSlot`.
- An independent read-only audit accepted the final `lib.rs`, Session and
  Presenter responsibility boundaries with P0/P1/P2 each zero. Renderer
  semantics live in the private renderer owners; Session is the public frame-
  transaction composer with a single private publication ledger; Presenter is
  the mechanical Surface adapter/host around private standalone Direct/Paged
  runtime owners.
- Root verification at clean `2739f48` passed workspace check, 454 renderer
  library tests, with 8 manual research tests ignored (462 total), strict Clippy, Rustdoc,
  architecture self-tests and real-tree policy, the real C FFI smoke, renderer
  and Web WASM compilation, forced Apple M4 Metal SortedAlpha conformance, and
  the hidden-window Apple M4 Surface test.
- The retained formal Nothing A065 run at
  `target/android-sort-benchmarks/verification-a065-2739f4899fac` used the
  complete 279,199-splat Kitsune SH3 scene at 2412x1080 with Packed/CPU
  PostSort. Its 20/20 issued terminals retained `V=D=279199`, contributor count
  in `{226450,236792}`, full membership and a native Surface PNG. Both the
  benchmark artifact validator and the full-quality suite validator with
  verified inputs passed. Timing remains observational, not a performance
  claim.
- Aggregate rollback is accepted for all 39 commits, not merely the old
  nine-commit `3f52558` subset. A conflict-free newest-to-oldest reverse in an
  isolated worktree produced tree
  `fd13949e2237c96181b570671f23ecbfe408b1a1`, exactly equal to accepted M6
  `a798b8a`, then passed workspace check and C FFI smoke.
- Chrome/WebGPU runtime is Deferred because locked `wasm-bindgen-cli 0.2.121`
  is unavailable. Physical-iPhone and Windows/Linux runtime also remain
  Deferred. WASM compile, macOS/Metal or historical platform evidence is not
  substituted for those endpoints, and M7 makes no broad pixel/performance,
  power, thermal or competitor claim.
- Exact identities, commands, validators, rollback proof and finite evidence
  boundaries are recorded in the
  [M7 final acceptance audit](m7-final-acceptance.md).
- Integrated closeout is complete. Commit `0d291c5` records final acceptance
  and `M7 = Accepted`, `2e16fff` corrects the renderer-test total, and
  `1de3f79` removes exactly the three satisfied M7 grandfather records. The
  real-tree architecture policy passes at that final integrated tree.
- `M7_ACCEPT_SHA == M8_BASE_SHA` is
  `1de3f79fa2fa22955f99c887bea421c918e31ee0`.

## M8 activation and closeout boundary

- State: **Active** from fixed accepted M7 tree
  `1de3f79fa2fa22955f99c887bea421c918e31ee0`.
- Activation slice: documentation-only fact inventory and closeout workplan;
  see [M8 closeout inventory](m8-closeout-inventory.md).
- This activation changes no handbook, README, release file, source, ABI,
  shader, script, test or product route. It performs no browser, device or
  cross-platform runtime.
- D11 active-path normalization was independently reviewed and integrated as
  `76188933a87a37f50175fa80720a429d50608ee7`. M8 acceptance still requires D12
  root review/integration, D15 fixed-candidate clean-tree verification/review,
  and the subsequent D13 archival of this bundle. Completing D11 or D14 does
  not complete M8 or Package M.
- The independent IO-SPZ ownership review at fixed baseline `41e18f8` accepted
  the existing loader as one cohesive `SPZ v4 -> validated SceneBuffers`
  transaction with no P0/P1/P2 ownership finding. Its M8 grandfather and
  external-review allowlist entry are therefore removed without changing
  `crates/gsplat-io-spz`. External-format interoperability and any future
  product-selected non-default resource budget remain explicit **Deferred**.
- The initial independent IO-PLY review at `41e18f8` rejected the former mixed
  owner and required cohesive owners plus terminal-safe incremental failure.
  The accepted integrated sequence is A1 metadata `b99adc5`, A2 terminal
  safety `fdd37a9`, A3 per-vertex decode `0c9ba23`, and A4 stream/facade
  `cee2a97`. The final independent A4 review task
  `019f9ed8-25e0-7393-b961-a9af77aef2a2` reported no P0/P1/P2 and accepted
  `metadata.rs` for header/attributes/SH layout, `decode.rs` for per-vertex
  numeric and coordinate normalization, `stream.rs` for file/bytes/reader
  traversal plus incremental lifecycle, and `lib.rs` for the public facade,
  errors, budgets, allocation/publication and rotation import-policy choice.
  The IO-PLY grandfather and external-review allowlist are therefore removed.
  The existing file-backed Packed summary/stream two-open snapshot boundary
  remains **Deferred** as a separate behavior-hardening issue, not an ownership
  blocker.
- Chrome/WebGPU at the accepted M7 SHA, physical iPhone, and Windows/Linux
  runtime remain **Deferred**. Compile, macOS/Metal, Simulator or historical
  evidence is not substituted.
- Package M remains **Active**. Only a later independently reviewed root
  closeout may set `M8 = Accepted`, move this bundle to `docs/plans/completed/`,
  and declare Package M complete.
- The independent read-only M8e release/distribution audit rejected D08, D09
  and D16 for two factual contradictions and two maintenance drifts: stale
  `0.1.2` release commands, inconsistent Apple artifact wording, an unlocked
  `wasm-bindgen-cli` install diagnostic, and obsolete M2b ownership text for
  the still-rejected Android GPU-producer option. The bounded M8f candidate
  `6bbd3c1ca82a21c04613b17e722d001c23561f8a` repaired only those facts and
  preserved workflow, manifests, collector behavior, CI policy and all
  Deferred endpoint/release operations. Its independent review found no
  P0/P1/P2, and root integrated it as
  `93f9dc662a6f1bac008def8ba4b541fb9a66ab4e`. D08, D09 and documented D16
  facts are therefore Accepted; no release or endpoint qualification follows.
- The historical M8e/M8f remote audit on 2026-07-26 recorded
  `v0.1.3 -> a47542fbcae092e07eb427f64e0a81ac2123b4c7` plus the GitHub direct
  prerelease assets `gsplat-android-release.aar`,
  `GsplatFFI.xcframework.zip`, `gsplat-rs-web-*.tgz` and `SHA256SUMS`. The
  wildcard is the local release-workflow name. This later status reconciliation
  did not re-query the network; current-source package verification, future
  release operations and registry publication remain Deferred.
- The M8g evidence audit rejected `93f9dc6` as a D15/final-closeout point. It
  preserved historical platform artifacts under their recorded SHAs but did
  not promote them to current-candidate qualification. D11 and D14 are now
  Accepted; the only `active/` path retained by D11 is an explicitly historical,
  non-rerunnable pre-archive command transcript. D12 has a populated candidate
  awaiting root review/integration, and D15 remains pending until the fixed
  pre-archive candidate matrix and independent review run.
- To avoid a circular gate, D15 now runs on one fixed pre-archive candidate
  after all source, documentation, policy, D11 and final-report edits. An
  independent review accepts or rejects that exact SHA. Only then may root make
  the pure archive/state commit; that move receives link, architecture-policy,
  stale-path, diff-summary and clean-tree checks rather than another expensive
  global matrix. M8 and Package M remain **Active** until that root closeout is
  accepted.
- **M8-D12 candidate ready; root acceptance pending.** The populated
  [M8 final closeout report](m8-final-closeout.md) records the final
  architecture/deprecation boundary, exact existing M0--M8 Git identities,
  milestone-local versus historical endpoint evidence, explicit limitations
  and the non-circular D15-before-D13 sequence. Its fixed input tree is
  `76188933a87a37f50175fa80720a429d50608ee7`. This documentation candidate
  does not accept M8 or Package M, does not qualify a new endpoint, and cannot
  record its own integrated SHA; root supplies that identity only after
  independent review and integration.
