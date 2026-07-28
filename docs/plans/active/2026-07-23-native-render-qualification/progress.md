# Qualification Progress

> Program plan: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

<!-- gsplat-program-task-states: begin -->
Q0 = Accepted
Q2 = Accepted
<!-- gsplat-program-task-states: end -->

## Scope

Package Q begins only after M8. Q0 freezes fair comparator identity, datasets,
camera traces, resolution receipts, timing semantics and endpoint schedule.
It keeps Exact, Balanced and Scalable claims separate and does not use a
browser, simulator, compile-only route or unmatched asset as proof of native
or competitor performance.

The active writer owns only this ledger and [Q0 contract](q0-contract.md). It
may not modify the renderer, benchmark producers, comparator harness, product
defaults, or the Balanced and Scalable packages.

## Candidate checkpoint

- Branch: `codex/q0-qualification-contract`
- Exact baseline: `6d5bd5442dee31cea24906744dfdd1d7492095ae`
- State: Q0 Accepted after independent review and root integration (`dd6af91`,
  `b195055`).
- Machine-state vocabulary: Q0--Q4 use only Accepted/Rejected/Deferred. If B6
  is Rejected, Q2 finishes Accepted after recording the finite decision; only
  its report may describe the Balanced comparison as `not_applicable`.
- Scope: comparator identity/launch, common workload admission, terminal timing,
  endpoint schedule, artifact receipts and finite Accepted/Rejected/Deferred
  outcomes are frozen in [q0-contract.md](q0-contract.md).
- Execution: no native, browser or device product benchmark was run by Q0.
- Next: Q1--Q3 start only from the accepted contract and their own declared
  prerequisites.

## Root-owned A065 formal functional artifact (2026-07-27)

One authorized physical-device run at root SHA
`95d2ccdc77adaf9bce9b0733a851939dc161fd9f` completed and was independently
validated with `validate-full-quality-experiment.py --verify-inputs`.

- Endpoint: Nothing A065 / Snapdragon SM8475 / Android 15 / Vulkan; thermal
  status before and after the run was `0`.
- Workload: complete SH3 Kitsune (`279,199` source, decoded and resident
  splats), Packed `SortedAlpha` with CPU ordering, `2412x1080`, both frozen
  trace views, 10 warmup and 20 measured frames.
- Evidence: repository-local formal suite
  `target/android-sort-benchmarks/verification-a065-95d2ccdc77ad/`, including
  the native-Surface PNG, receipt, logcat, per-run artifact and immutable
  input identities. The full-quality validator reported one expected and one
  rendered run, with no capacity rejection or missing artifact.
- Diagnostic only: the retained run reports `avg_call_ms=7.236`,
  `avg_frame_ms=7.859`, CPU preprocess `0.769 ms` and CPU sort `2.566 ms`.
  It is a 20-frame single-policy functional ledger, not a CPU/GPU comparison,
  competitor comparison, or release-performance claim.

This establishes A065 native functional evidence for its exact scope. It does
not accept Q1--Q3, qualify the Balanced candidate lanes, or substitute for the
separately frozen matched-comparator protocol.

## Root-owned M4 Balanced image-gate observations (2026-07-27)

The M4 Metal B1/B2/B3 suites at commit
`edbc656e04befd589b8e425f0874f98aebb7333d` passed their formal image gate for
complete SH3 Kitsune at `1920x1080`, including the frozen moving trace. This is
candidate image-integrity evidence, not a matched performance comparison and
does not advance Q1--Q3. Timing/performance claims remain unavailable until
the Q0 paired comparator protocol has retained matching native and comparator
artifacts.

The newly authorized Chrome/WebGPU attempt must also remain **Deferred**: it
exposed a ticket-namespace defect after the collector observed its initial
`ready` object. The repair at `a2437d4` has local verification only; no
post-repair browser endpoint evidence exists yet.

## Q2 finite dependency result (2026-07-27)

Balanced B6 is terminal **Rejected** after B1--B3 produced no Accepted
component, leaving no legal B4 combination or B5 product policy. Under the
predeclared Q0 dependency table, Q2 is therefore machine-state **Accepted**
with report result `product_comparison=not_applicable`. No gsplat-rs Balanced
product plan exists to compare with PlayCanvas, so launching or fabricating a
matched product-throughput run would be misleading. This outcome is not a
performance win; it is the finite truthful completion path for a rejected B6.

## Q1 M4 PlayCanvas prerequisite (2026-07-27)

At clean root commit `cab9c9e`, the pinned PlayCanvas `2.21.0-beta.14`
(`d5fe88878e338936fe763bbce1a58bc315e89cbe`) harness completed one fresh
Chrome/WebGPU prerequisite run over the full 2,541,226-splat Truck SH3 source.
The run used the canonical two-view moving trace, `1920x1080` backing/internal
resolution, 20 warmup and 80 measured frames, GPU sort on every forward frame,
and disabled LOD, sampling, dynamic resolution and upscaling. Source, decoded
and resident counts all equal `2,541,226`; both trace indices were measured.

The canonical benchmark validator accepted
`tests/competitive/playcanvas/target/benchmarks/qualification/`
`playcanvas-truck-1080p-cab9c9e-prereq-attempt2/`. Its stopped-rAF plus terminal
WebGPU queue-drain receipt reports frame-wall mean `17.6675 ms`, p95 `33.3 ms`,
48 of 80 frames over the 16.67 ms budget, terminal mean `18.7625 ms` and
`53.2978 FPS`. Browser `frameupdate`-to-`frameend` CPU time is separately
reported as `1.8 ms`; no unavailable GPU phase timing is invented.

This is a comparator prerequisite, not Q1 acceptance or a native-performance
loss. A desktop browser page cannot prove physical presentation dimensions;
the artifact truthfully leaves presented width/height unavailable and
`full_resolution=false` despite proving the full internal backing. Q1 still
needs the matched native Exact run and the scoped external-presentation/
quality decision before comparing terminal throughput.

The gsplat-rs Chrome/WebGPU prerequisite endpoint was subsequently accepted at
clean root commit `74b2647`. The one authorized replacement followed the
repository doctor -> command -> run launchbook without retry and retained
`target/qualification/q1-webgpu-truck-1080p-74b2647-attempt-2/` in its isolated
worktree. It used the same complete Truck SH3 source and two-view `1920x1080`
trace, with 20 warmup and 80 measured frames. Source, decoded, encoded,
resident and addressable counts all equal `2,541,226`; all measured frames used
`projected_quads_exact`, preserved `C <= V <= S` and `D = V`, and joined 80
unique renderer current-stats tickets to 80 Ready terminals. The canonical
benchmark and full-quality validators passed, and the final frame was inspected
as a non-black, complete Truck image.

This endpoint is accepted only for complete-scene, image, Exact-count and
ticket-integrity evidence. Its throughput observation is **Rejected**: the Web
frame loop still stopped submission whenever one renderer current-stats ticket
was pending, even while the artifact declared `sustained_window`. It therefore
serialized each presented frame with its asynchronous terminal readback, while
the PlayCanvas prerequisite submitted continuously and drained the queue only
after its final measured frame. The resulting gsplat-rs frame-wall mean
`66.875 ms` and 80/80 over-budget frames must not be compared with PlayCanvas
`17.6675 ms`. The same rejected artifact remains useful only as a diagnostic:
65 GPU-order frames had mean host call time about `0.308 ms`, while 15 CPU-order
probe frames had mean host call time about `29.573 ms`; Candidate drew mean
`1,779,155.5` visible splats although only mean `979,533.5` were exact
contributors. Those observations do not establish a bottleneck because the
per-frame observer could itself perturb plan learning and queue cadence.

A replacement sample is allowed only after the collector proves two separate
windows: an untimed, overlapped current-stats control ledger and a
same-configuration timed window with zero per-frame current-stats requests and
continuous measured submissions. The scoped repair uses one renderer-owned
current-stats ticket on the final warmup draw, stops drawing, and drains its
Result-bearing map terminal before accepting the first measured camera input.
It then uses a distinct ticket only on the final measured draw, after N-1
observer-free continuous measured submissions, and drains that ticket with no
new draw. The common terminal window starts before the first measured
`setCamera`/order/render and ends at the final Ready terminal; separate
post-render first/last submit timestamps expose rather than omit first-frame
CPU/order/encode time. This adds no queue submission, removes the rejected
per-fence device-loss handler/API, excludes any residual warmup queue tail, and
records the small final-frame readback overhead plus its conservative
difference from the PlayCanvas queue Promise. No replacement Chrome endpoint
was run by the candidate; its mechanism passed root-owned fixed-SHA review, but
Q1 throughput remains Deferred pending one authorized exact-SHA run. Q1 still
requires the native M4 Exact endpoint, common external-presentation scope and
counterbalanced outer pairing.

Root integrated that mechanism as `da4887d` and performed the single fresh
doctor -> command -> run attempt at
`target/qualification/q1-webgpu-truck-1080p-da4887d-attempt-1/`. The attempt is
terminal **Rejected** before any throughput sample: the launchbook profile still
started only its legacy per-frame `current_stats_evidence_window`, rather than
first producing the required untimed control artifact and then launching the
content-linked `terminal_queue_throughput_window`. Its first warmup frame issued
ticket 1; the next warmup frame correctly failed closed when that legacy mode
observed `NotRequested`. At that point the launch mismatch alone was sufficient
to reject the attempt; the later correctly orchestrated run proved that this
shape is a legal deferred request, not evidence that one ticket exhausted the
four-slot ring. No 80-frame timing or replacement comparison was published, and
the failed directory is retained without retry.

The launch-orchestration repair was independently accepted and root-integrated
as `3c4e8da`. Its canonical profile now runs build -> untimed control -> bound
terminal throughput, validates the control artifact before throughput, and
atomically claims each output stage once. Doctor and command preview on the
integrated SHA proved all prerequisites READY and printed the three frozen
commands in the required order.

The separately authorized `3c4e8da` endpoint at
`target/qualification/q1-webgpu-truck-1080p-3c4e8da-attempt-1/` nevertheless
ended terminal **Rejected** during control warmup member 1, again before any
throughput sample. This second failure proved a different, narrower defect:
the renderer deliberately permits one accepted current-stats request to remain
pending across queue-boundary and formal Adaptive presentations, but the Web
control harness incorrectly required every such presentation to carry the
ticket immediately. Ticket 1 had already reached Ready and its readback slot
had been recycled; the next request was accepted, then legally deferred as
`NotRequested`. Existing renderer tests prove the same boundary -> formal ->
observer sequence. No 80-frame throughput number exists for this attempt.

The focused Web repair now retains the same request, camera revision and trace
member across those auxiliary presentations, terminates any formal Adaptive
ticket without counting that presentation as a control sample, and advances
the logical 20+80 schedule only when the observer ticket is actually Issued.
Auxiliary presentations are recorded separately and a monotonic timeout remains
fail closed. The renderer, WASM bridge, public SDK, product policy and formal
throughput window are unchanged. A new endpoint remains unavailable until this
collector-only candidate receives fixed-SHA review; neither failed directory
may be retried or overwritten.

## Q1 M4 native sustained collector mechanism (2026-07-27)

Starting from clean exact parent
`9c81a05964ea8dd77d8f384f21cb4ee4bfa9d5f2`, the desktop private host and
`tests/perf/collect-q1-m4-native.py` now define the missing native control plus
terminal-throughput mechanism without changing renderer policy, a stable API,
PlanId, Adaptive, or product defaults. The host is feature-gated and admits
only Packed `ProjectedQuadsExact`, product Adaptive policies, every-frame
ordering, the frozen two-view Truck trace and 60 Hz cadence.

The first 20+80 stage remains sustained current-stats correctness/control
evidence. It drains warmup tickets with drawing stopped, restarts its measured
control at trace frame 0, continuously polls/recycles the bounded ring and
retains complete presentation/submission/terminal plus `V/C/D` ledgers. It is
explicitly `timing_eligible=false` and emits no throughput `N/FPS`.

After that control is fully drained, the same process runs a second 20+80
presentation stage under the same scene/trace/camera/config identity with zero
current-stats requests or polls. After warmup frame 20, drawing stops for one
shared-runtime queue completion outside the timed window; no current-stats or
capture work is admitted, and measured frame 0 freezes the window start only
after that Ready boundary. Drawing then stops after measured frame 80 for the
single measured-terminal no-draw
`SurfaceRenderSession::pump_receipts` owner. Only the common monotonic
first-measured-camera-input to measured-queue-completion window publishes
terminal `N/FPS`; it cannot reuse control `V/C/D`. The collector binds both
stages to the clean Git SHA and locked binary SHA.

Every timed member also records the actual Exact whole-plan Adaptive state,
executed PlanId and Candidate/Compact execution. Exact intentionally reports
the independent projected learner as `disabled` because `WholePlanController`
owns the whole-plan choice; the validator records that state without requiring
Preproject/Compact. The fairness gate is zero timed current-stats load plus the
terminal-only no-draw drain.

The host reads the live f32 camera back from `session.camera()`, derives Surface
aspect, recomputes canonical view/projection/view-projection matrices, and binds
the post-timing view 0/1 PNGs to camera revision, presentation sequence and
Ready control terminals. Busy, failure, timed observer load,
duplicate/missing/cross-joined ticket, failed queue completion, drain draw,
incomplete member or identity drift is fail closed; the collector claims one
fresh immutable root and never retries the host command.

No formal Truck endpoint was run by this implementation slice. Q1 remains
**Deferred** pending the separately authorized native run plus PlayCanvas
headful external presentation, the common reference-image gate and five outer
counterbalanced pairs; those boundaries are deliberately not implemented here.

Fixed-SHA review of `95e6b5a` rejected duplicate host/collector ownership and a
broad exception path that could misclassify protocol failures as Deferred. The
direct repair moves winit dispatch, current-stats ledger/join, live-camera
derivation, plan/count validation and ordinary capture I/O into the existing
private `surface_evidence` owner. Q1 now supplies the control/timed/capture state
transitions and frozen log contract. The Q1 collector reuses the existing
locked builder, finalizer and ticket ledger, the paired Truck workload loader,
and `trace_v1` camera math. Only an explicitly named preflight
`EnvironmentPrerequisiteError` can be Deferred; build/host/protocol/output-stage
exceptions are Rejected. Focused coverage includes unknown capture tickets and
a post-protocol injected `KeyError` through the real collection state path.

The second fixed-SHA review found four remaining fail-closed gaps. The direct
follow-up makes `surface_evidence::SurfaceEvidenceRuntime` the sole live session
owner; Q1 now issues request/present/poll/complete commands and consumes owned
receipts without calling session camera/render/current-stats/capture/drain APIs.
Collector joins now bind each ticket's phase/member/trace and full identity
across presentation, submission, terminal and derived frame, and count semantics
are a closed enum. Output publication now stages, cleans, fsyncs, chmods and
atomically renames; finalization failure is itself Rejected and cannot leave an
Accepted output root. Negative fixtures cover crossed ticket membership,
unknown count semantics, and cleanup/write/chmod failures.

Fixed-SHA review of `909b3bb` found that formal timed warmup still flowed into
measured frame 0 without first completing the warmup queue. The direct repair
adds a distinct no-draw `TimedWarmupDrain` through the shared runtime facade.
Its begin/end receipts require zero draw, current-stats and capture work, and
the collector admits the terminal window only when warmup completion is no
later than the first measured camera input. The later measured-terminal drain
remains the sole drain counted in `N/FPS`.

The root-integrated mechanism at `da4887d` was launched once through
doctor -> command -> run and failed closed before warmup frame 2, with no
performance artifact. Its retained failure is not retryable. The follow-up
launchbook repair at `3c4e8da` correctly made the profile explicitly two-stage,
but its separately authorized endpoint then exposed the control harness's
incorrect immediate-Issued assumption described above. Both attempts are
terminal Rejected and neither contains throughput evidence. Q1 throughput
therefore remains Deferred while the focused auxiliary-presentation repair is
reviewed.

Fixed-SHA review of the first repair, `60d6cd3`, rejected two evidence gaps
before any browser rerun. First, an Adaptive formal order ticket issued by a
deferred current-stats presentation was still written into the retired order
ledger, so the Exact artifact validator would reject its otherwise intentional
auxiliary work. Second, the request deadline was not checked while waiting for
that auxiliary ticket's terminal. Review also required the retained artifact
to prove every deferred/issued presentation attempt, rather than keeping only
aggregate counts.

The direct follow-up keeps auxiliary formal submissions and terminals in a
separate bounded ledger, joins each one to its exact deferred phase, logical
member, attempt, trace frame, camera revision and monotonic timestamps, and
checks the same finite request deadline during every empty terminal poll. The
current-stats schedule now retains every deferred and final issued presentation
and proves that each member has contiguous attempts ending in exactly one
issued ticket. The collector additionally requires the schedule's attempt
count to equal the manifest's actual presented count. Auxiliary frames remain
untimed control evidence and can never become one of the logical 20+80 samples.
Focused Web harness, Web SDK, benchmark-artifact, bootstrap and source-policy
checks pass locally. A new endpoint run remains forbidden until this follow-up
has its own fixed-SHA acceptance; the two prior failed output roots remain
immutable Rejected evidence.

The next two fixed-SHA reviews of `7a17b80` found four final admission gaps,
again before browser execution: auxiliary terminal joins omitted their
attempt/trace/deferred-time fields; deleting both auxiliary terminal arrays
could hide an actual formal ticket; the validator did not freeze the exact
20-warmup/80-measured phase boundary; and an isolated non-final current-stats
terminal depended on the outer collector timeout. The follow-up records the
auxiliary ticket/backend/submit time on its originating deferred attempt,
requires the full attempt -> submission -> terminal identity, validates each
logical index against the configured warmup boundary, and applies the same
finite per-ticket deadline in submitting and draining states. Focused negative
tests reproduce all four review counterexamples. No browser result is claimed
until this newer exact SHA is accepted.

Both independent fixed-SHA reviews accepted `f0e0629` with no P0/P1/P2, and
root executed its single authorized Web command into the immutable fresh root
`target/qualification/q1-webgpu-truck-1080p-f0e0629-attempt-1/`. The untimed
control stage is the first complete formal Web control artifact: all 100
logical members terminalized, 47 deferred attempts remained outside the
20+80 sample set, and all 147 actual presentations are retained and admitted.
It preserved complete Truck SH3 membership and full 1920x1080 rendering. Its
reported call/preprocess/sort timings remain control diagnostics with
`performance_evidence=false` and are not a competitor result.

The bound throughput stage rendered its 20 warmup and 80 measured frames, but
the host then failed before artifact admission with
`ReferenceError: Cannot access 'frames' before initialization`. The local
`parseArtifacts` function declared a later `const frames`, placing the earlier
configured frame-count reference in JavaScript's temporal dead zone. The
entire command is terminal **Rejected** and is not retried or reused; no
throughput number is retained. The direct follow-up renames the global
configuration to `requestedFrameCount` and the post-join collection to
`admittedFrames`, removing the collision without changing renderer behavior.

Two independent fixed-SHA reviews accepted that one-line collector repair at
`47f4ef5` with no P0/P1/P2. Root then executed the single authorized command
into the fresh immutable root
`target/qualification/q1-webgpu-truck-1080p-47f4ef5-attempt-1/`. Both the
untimed control artifact and the bound terminal-queue throughput artifact pass
the canonical benchmark and full-quality validators. The run preserves the
complete 2,541,226-splat Truck SH3 source, both trace views, 20 warmup plus 80
measured frames, and a 1920x1080 internal/presented backing with sampling, LOD,
dynamic resolution and upscaling disabled.

The accepted throughput candidate reports a 2,150 ms first-input-to-final-
terminal window, or `26.875 ms/frame` and `37.2093 FPS` by terminal N/time.
Headless RAF frame-wall mean is `26.51875 ms` (p95 `37.8 ms`) with 68/80 frames
over the 16.67 ms budget. Host call mean is `25.03375 ms`; its recorded CPU
preprocess and radix-sort means are `12.38375 ms` and `9.74875 ms`. Adaptive
ordering used CPU on 68 frames and GPU on 12, while projected execution stayed
Candidate. The bound control artifact proves mean `V=D=1,779,155.5` but mean
exact contributors `C=979,533.5`, so Candidate issues about 1.82 drawn splats
per actual contributor on this trace.

This is a valid gsplat-rs qualification candidate, not yet a public competitor
result. The existing PlayCanvas prerequisite uses the same asset, trace,
internal 1080p resolution and headless WebGPU browser, but it is unpaired,
records a different terminal anchor, lacks common pixel-quality evidence, and
ran the comparator's fixed GPU sort while gsplat-rs was still learning its
mixed Adaptive order lane. For internal diagnosis only, its terminal N/time is
`18.7625 ms/frame`; the current single-run ratio is therefore about `1.43x`,
not the rejected serialized sample's apparent `3.8x`. Formal comparison still
requires a frozen common terminal boundary, stable-plan and cold-Adaptive cells,
common image evidence, and the predeclared counterbalanced pairs.

The first deliberately small bottleneck slice is `d8cdafd`: on WASM, scalar
visibility/key preprocessing now writes the existing packed radix-pair ABI
directly. It removes the split depth-key buffer and the subsequent full-scene
split-to-packed pass without changing ExactFull32, inclusive near/far,
source membership, stable source-ID ties, PlanId, controller, GPU or raster
behavior. Two independent fixed-SHA reviews accepted the candidate, and host
tests plus WASM checks passed before the single fresh browser run at
`target/qualification/q1-webgpu-truck-1080p-d8cdafd-attempt-1/`.

Both stages and both canonical validators passed. With the same 68 CPU / 12
GPU measured-frame mix, terminal N/time improved from `26.875` to
`26.2525 ms/frame` (`37.2093` to `38.0916 FPS`, about 2.3%). CPU-lane mean call
time improved from `29.39265` to `28.14265 ms`; CPU preprocess from `14.56912`
to `14.17500 ms`; CPU radix from `11.46912` to `10.50441 ms`. This accepts the
memory-traffic slice as a small independent improvement but does not close Q1:
the current single-run PlayCanvas diagnostic ratio remains about `1.40x`, and
the Candidate `D/C` amplification remains unchanged. The next bounded cell is
therefore the existing fixed-GPU, exact contributor-compaction plan, not more
tuning of the WASM scalar pass.

## Q1 M4 fixed GPU preproject + Compact checkpoint (2026-07-27)

The next bounded cell freezes two independent choices instead of asking the
Adaptive controller to learn them inside an 80-frame sample: GPU ordering uses
the existing preproject producer, and projected drawing uses exact stable
contributor compaction. Its two-stage collector first proves the actual Whole
Plan and `V/C/D` in an untimed current-stats control, then runs the same frozen
configuration with 79 observer-free measured frames and one final
same-submission current-stats terminal. It does not add a new renderer plan or
change the product default.

The first formal command at `4038c9b`, retained under
`target/qualification/q1-webgpu-truck-fixed-gpu-preproject-compact-4038c9b-`
`attempt-1/`, is terminal **Rejected**. Its browser render and control stages
completed, but the canonical artifact validator still assumed that every
terminal-throughput ledger must contain an active Adaptive state and rejected
the disabled fixed cell. The staging data is preserved for diagnosis only; its
timing is not promoted after the fact.

The first validator repair at `b9c3f35` was also rejected before any browser
run. Two independent fixed-SHA reviews demonstrated that a CPU artifact could
be relabelled by changing only `execution_cell` and its self-reported ledger.
The accepted correction at `fe03ce7` binds the fixed cell to the renderer
request/actual tuple, every measured frame's GPU/Compact/preproject/no-fallback
identity, both warmup/final terminal plans and the final same-submission
current-stats plan. Label-only, post-sort, Candidate, active-Adaptive,
projected-state, fallback and final-plan counterexamples now fail closed. Two
independent reviews accepted that fixed SHA with no P0/P1/P2.

Root then executed exactly one fresh doctor -> command -> run into
`target/qualification/q1-webgpu-truck-fixed-gpu-preproject-compact-fe03ce7e2fd5/`.
Build, untimed control and bound throughput all exited successfully; canonical
benchmark and full-quality validation passed. The artifact preserves all
2,541,226 Truck SH3 splats at `1920x1080`, both moving trace views, 20 warmup
and 80 measured frames, with LOD, sampling, dynamic resolution and upscaling
disabled. Every measured frame reports actual GPU preproject, Compact and no
GPU fallback. The control mean is `V=1,579,811.5` and
`C=D=979,533.5`; Compact therefore removes about 600,278 guaranteed
non-contributor draws per frame for this plan and trace.

The accepted terminal window is `1,667.7 ms`, or `20.84625 ms/frame` and
`47.9703 FPS`. RAF frame-wall mean is `19.75375 ms`, p95 `21.3 ms`, with 75
of 80 frames above the configured 16.67 ms budget. Host call mean is only
`0.385 ms` and is not reported as GPU time. Against the earlier accepted
Adaptive candidate at `d8cdafd`, terminal N/time improves by `20.59%`
(`26.2525` to `20.84625 ms/frame`) while execution changes from a 68 CPU / 12
GPU Candidate mix to fixed GPU preproject + Compact; this is a cell comparison,
not attribution to one isolated shader instruction.

For internal diagnosis only, the single unpaired PlayCanvas prerequisite's
terminal mean is `18.7625 ms`, so the remaining observed terminal ratio is
about `1.111x`, rather than the rejected serialized sample's apparent `3.8x`
or the Adaptive candidate's `1.40x`. This is not yet a competitor result:
the two runs are unpaired, their terminal proof primitives differ, adapter and
driver identities remain unavailable, and no common same-camera pixel gate has
been admitted. Q1 remains Active pending the predeclared paired/fairness work
and native M4 endpoint; the fixed cell is nevertheless Accepted as the current
WebGPU mechanism candidate.

The following no-quality-change hypothesis was tested independently at
`c285a8b`. Candidate visibility `V` previously used a complete prefix scan even
though no downstream consumer read its per-group offsets; the candidate
replaces that graph with one exact atomic addition per source workgroup and a
single four-byte count buffer. Contributor scan/compaction, ExactFull32 keys,
stable ties, radix and draw behavior remain unchanged. Two fixed-SHA reviews
accepted the resource and correctness change with no P0/P1/P2; renderer tests,
WASM check and Clippy passed.

Root then ran exactly one fresh fixed-GPU Compact experiment at
`target/qualification/q1-webgpu-truck-fixed-gpu-preproject-compact-c285a8bb9db7/`.
Both canonical validators pass and the control counts are identical to the
previous cell: mean `V=1,579,811.5`, `C=D=979,533.5`. The endpoint hypothesis
did **not** show a performance improvement. Terminal N/time is
`21.0425 ms/frame` (`47.5229 FPS`) versus the prior single-run
`20.84625 ms/frame`; the observed delta is `+0.94%`, while both p95 values are
`21.3 ms`. No retry or tuning run is used to search for a favorable sample.
The simpler resource ownership remains a branch candidate, but it is not a
performance claim and requires native/A065 regression evidence before product
promotion. The next performance experiment, if admitted, must isolate the
explicit 20-bit depth-key quality/speed tradeoff rather than mix it with this
no-quality-change slice.

## Q1 K1 depth-key candidate execution checkpoint (2026-07-27)

The depth-key experiment is split into independent mechanism, evidence,
quality and performance slices. K1a parameterizes the existing external-prefix
radix graph without changing the product default. ExactFull32 remains shift 0
with eight 4-bit passes; CandidateStable24 uses shift 8 with six passes; and
CandidateStable20 uses shift 12 with five passes. Odd/even pass parity selects
the actual final B/A key and source-ID planes, so the shorter candidate graph
does not add a copy or readback. K1a was committed as `85f4f6e`; two fixed-SHA
reviews accepted it with no P0/P1/P2, and host GPU oracle, WASM check, Clippy
and full renderer tests passed.

The first K1b wiring commit, `b6b5e95`, was **Rejected** before any browser or
device run. CandidateStable20 was real, but a requested CandidateStable24 GPU
preproject graph still executed ExactFull32 while the successful-presentation
receipt reported the requested CandidateStable24 profile. That requested /
actual split violated the fail-closed evidence contract even though the image
path itself remained exact.

The bounded correction at `2a953f8` makes CandidateStable24 real: the shared
preproject key producer clears the low eight key bits, radix executes shifts
8/12/16/20/24/28, and the even-pass graph exposes its final A planes.
CandidateStable20 remains low-twelve-bit with five passes and final B;
ExactFull32 remains the unchanged eight-pass default. One graph-derived private
receipt now binds the realized precision, first shift and pass count through
byte planning, construction and admission. A separately tampered preparation
receipt fails before encode/present as `Preproject depth-key profile` rather
than publishing a false precision claim.

The 1,025-item Metal GPU oracle compares ExactFull32, CandidateStable24 and
CandidateStable20 key/ID output against the shared CPU precision function,
including cross-workgroup stable ties and exact `V=C=D` counts. Default,
Candidate24 and Candidate20 full library suites each report 483 passed and 8
ignored; all three WASM checks and all-targets Clippy runs pass. Two independent
fixed-SHA reviews accepted `2a953f8` with no P0/P1/P2. This accepts only the
private execution mechanism and truthful admission/presentation chain.
CandidateStable20 remains diagnostic and is not full-quality evidence. Web
diagnostic exposure, same-camera image qualification, browser performance and
Android/native endpoint behavior remain separate pending slices.

## CandidateStable20 Web presentation diagnostic (2026-07-27)

The hidden Web diagnostic is now isolated from the default Exact package. The
standard Web build remains Exact; a separate build requires both the private
`diagnostic-web-depth-key-candidate20` feature route and an explicit fresh
`GSPLAT_WEB_WASM_OUT_DIR`. It cannot replace, alias or delete the default
`examples/web/pkg` target, including when that path is a symlink.

At clean commit `fd989580a5626825d529de7b34e22e4ca99f56c1`, root performed one
real Chrome/WebGPU diagnostic run and retained
`target/diagnostics/candidate20-web-fd98958/artifact/`. Three consecutive
successful presents all reported actual Packed, GPU, Preproject, Compact and
`CandidateStable20` execution. Their scene, viewport, contract and PlanSet
generations remained identical; order generations and presentation sequences
advanced `1,2,3`. The collector rejects missing, duplicated, regressing,
cross-generation or mismatched receipts.

This artifact is intentionally classified `diagnostic`, with
`full_quality_eligible=false` and `validator=diagnostic_only`. It proves that
the hidden browser build reaches the intended present-fenced execution path;
it provides no image-quality, Truck-performance or product-default evidence.

## Q3 M4 SIMD microbenchmark checkpoint (2026-07-27)

At clean root commit `f025dff`, the fixed
`Q3.M4.PackedCpuExact.ScalarVsNeon` cell completed once and published the
fresh ignored artifact
`target/benchmarks/qualification/q3-m4-simd-f025dff.json`. Its fixed
200,003-item packed input and 11 interleaved sample pairs preserve exact key,
source-id, NaN-bit, boundary-bit, FMA-derived-key and stable-tie parity between
the forced Scalar and NEON radix-count/unpack leaves.

The observed medians were `849,833 ns` for Scalar and `819,417 ns` for NEON.
This is useful evidence that the native SIMD leaf is both correct and worth
carrying into the whole-plan experiment, but the cell remains **Deferred** by
construction: it is a microbenchmark only, does not include renderer-owned
preprocess, sort, upload, draw or terminal queue completion, and cannot select
the product default. Q3 next requires a private renderer qualification selector
and matched native terminal artifacts before making any whole-plan decision.

The renderer-private selector and finite matrix mechanism were subsequently
independently accepted and root-integrated as `6e67cb2`. Default dispatch and
all public APIs remain unchanged; qualification-only native features can force
the production Packed radix-count/unpack leaves to Scalar or NEON, reject an
ambiguous dual selection, and cannot be enabled for WASM. The collector freezes
five counterbalanced pairs, clean/fresh build and input identity, Exact
presentation/count rules, and one execution per command without automatic
retry.

Its first and only pre-integration Truck attempt is terminal **Deferred**, not
a performance result. The first NEON session presented all 100 planned frames
and proved complete SH3 membership, Exact CPU execution, preprocess/sort,
order upload and draw counts, but Packed Exact intentionally did not issue the
legacy order-measurement ticket. Queue completion belongs to the independent
renderer-owned current-stats namespace, which the desktop qualification host
did not yet expose. The retained experiment therefore contains no fabricated
pairs or runs. Q3 now has one bounded remaining owner slice: connect that
private current-stats terminal to the desktop qualification harness, then let
root execute one fresh five-pair matrix. A lack of consistent terminal benefit
will make the optimization Rejected rather than trigger tuning loops.

That owner slice was independently reviewed and integrated as `662221c` plus
the fail-closed nonzero-exit correction `5d54ee3`. At clean root commit
`74b2647`, root then executed the one authorized five-pair Truck whole-plan
matrix and retained
`target/benchmarks/qualification/q3-renderer-simd-74b2647-attempt-1/`.
Each of the ten runs used complete Truck SH3, Packed Exact CPU ordering,
`1920x1080`, 20 warmup plus 80 measured frames, and 101 renderer-owned
current-stats terminals. Correctness parity remained exact.

The M4 whole-plan cell is terminal **Rejected** under its predeclared rule.
NEON sort time was lower in all five pairs with a median paired difference of
`-0.421886 ms`, but queue-completion mean was lower in only three of five
pairs; its median paired difference was `-0.520333 ms`. Preprocess was lower in
only two pairs. This means the SIMD leaf is useful but does not establish a
stable end-to-end M4 product advantage, so Scalar/default dispatch remains
unchanged and no tuning retry is permitted. Q3 itself remains active until the
required A065 native cell reaches a finite terminal result; absent physical
x86_64 and Apple-mobile breadth will be reported Deferred rather than
substituted by cross-compilation or simulators.

## Q3 A065 SIMD collector checkpoint (2026-07-27)

The finite physical-A065 matrix mechanism was independently accepted and
root-integrated as `36736a8`. It freezes the Scalar/NEON APK and prepared-input
identities, stages each workload once, executes every matrix command once, and
binds any structured renderer range-admission receipt to the exact host-issued
collection identity. A stale or cross-run receipt cannot truncate the ladder.

Generic OOM text, PNG failure, malformed artifact or ledger failure remains a
cell-local integrity `Rejected`; collection continues through the later tiers
and complete Truck, while the aggregate result stays sticky `Rejected`. Only a
validated renderer scene-admission receipt may cut off larger workloads, and
only loss of the frozen repository/device/APK/input identity may terminate the
matrix early. The current Android product does not yet emit that range receipt.

This checkpoint contains collector, qualification-only SIMD leaves, tests and
launchbook support only. No A065 device run was performed during integration,
so Q3 remains **Active** and publishes no Android performance or capacity
result yet. The next root-owned action is one fresh doctor -> command -> run
matrix at an exact clean integrated SHA; a failed cell is retained rather than
retried or tuned in place.

## K1d Web same-present capture checkpoint (2026-07-27)

The Web depth-precision image gate now has a renderer-owned, take-once RGBA8
receipt joined to the same successfully presented Exact frame. Exact and
Candidate20 use separately built WASM packages, preserve the complete Kitsune
SH3 source at `1920x1080`, and fail closed on profile, plan, order generation,
presentation sequence, image hash or temporal-identity drift. The first fresh
endpoint attempt at `571b5f7` reached renderer construction but rejected an
invalid host transition order before capture. Root corrected the transition at
`452e462`: GPU plans are admitted, GPU ordering is selected, Compact is forced,
and only then is Preproject selected. Independent review found no partial state
or render/present opportunity between those transactional calls.

The second fresh endpoint attempt at `452e462` stopped before capture because
wgpu 28's Web backend exposes its Surface with `RENDER_ATTACHMENT` only. The
WebGPU canvas configuration API can represent other usages, but the current
wgpu Web Surface capability implementation does not expose `COPY_SRC`; this is
therefore a backend seam, not a Truck, Chrome or device-capacity result. Both
attempts retain structured failure artifacts and contain no image or
performance evidence.

The bounded correction stays in the Surface target adapter. Native/copy-capable
Surfaces retain direct readback. Only an explicitly requested diagnostic frame
on a non-copyable Surface allocates a same-size, same-format renderer-owned
presentation target. Exact splats raster once into that target; the same command
encoder copies it to the readback buffer and performs a sampler-free
`textureLoad` fullscreen blit to the acquired Surface. One submission and one
primitive present remain the only publication path. Ordinary frames still
raster directly to the Surface and allocate or encode no capture resources.

The GPU equivalence test compares the renderer-owned source with the blitted
destination byte for byte for RGBA/BGRA, linear and sRGB formats. Renderer
library tests, native strict Clippy, default/diagnostic WASM checks, Web unit
tests and the source architecture policy pass. This checkpoint is mechanism
evidence only until a fixed-SHA review and one new fresh Chrome run produce the
paired images; capture frames remain excluded from performance measurements.

The cumulative implementation at `78d9d4a` was independently accepted with no
P0/P1/P2. Review found and root closed one pre-endpoint defect: Direct/Paged
browser sessions do not raster into the intermediate target, so they now reject
the diagnostic request before resource allocation or GPU work. Exact Packed is
the sole browser owner of this diagnostic path; native copy-capable standalone
capture remains unchanged.

Root then executed exactly one fresh Chrome attempt at that clean SHA and
retained
`target/qualification/k1d-web-quality-78d9d4a-attempt-3/artifact/`.
All six Exact/Candidate20 captures joined the complete 279,199-splat Kitsune
SH3 scene, the `0 -> 1 -> 0` moving trace, `1920x1080`, `GpuPreproject`, current
order generation and successful presentation sequence. The image gate and all
six canonical benchmark-artifact validators pass.

| Trace frame | SSIM | normalized RGB MAE | RGB bad pixels over 3 |
| --- | ---: | ---: | ---: |
| 0 | `0.9998697584` | `0.0001101409` | `0.0018156829` |
| 1 | `0.9996730161` | `0.0002533777` | `0.0045987654` |
| 0 retry | `0.9998697584` | `0.0001101409` | `0.0018156829` |

Both transition residuals are `0.0003529746`; alpha MAE and alpha bad-pixel
fractions are zero in every frame. Returning to trace frame 0 reproduces the
same renderer RGBA hash in each lane. This accepts K1d's Web image-quality
prerequisite only. The capture/blit frames are not timing samples, complete
Truck performance remains unmeasured for Candidate20, and the separate Q1 pair
admission candidate is still rejected until real gsplat-rs and PlayCanvas
terminal image producers own their endpoint receipts.

### Q1 Web Candidate20/Exact Truck pair-orchestrator mechanism (2026-07-28)

An isolated gsplat-rs-only collector now defines the next bounded experiment;
this checkpoint contains no browser or endpoint run. It accepts only a retained
K1d balanced-image suite whose clean commit and Exact/Candidate JS, WASM, and
build-receipt hashes exactly match the timing packages. The canonical K1d
validator runs before the output directory is claimed.

The admitted workload is complete 2,541,226-splat Truck SH3, the committed
moving two-view trace, and true requested/Surface/internal/presented
`1920x1080`. Both lanes keep Packed Exact membership, GPU Preproject ordering,
Compact exact contributor drawing, and sort refresh interval one. The only
changed receipt is renderer-owned depth-key precision: `ExactFull32` versus
`CandidateStable20` as already admitted by K1d.

The collector creates at least three seeded, counterbalanced pairs. Every
scheduled lane has exactly one renderer execution with 20 warmup and 80
measured frames. Capture and blit APIs are absent from the timing path. The
final warmup draw and final measured draw each issue one renderer-owned
current-stats request; their Ready terminals must match the complete
scene/camera/viewport/contract/plan-set/order/raster/encode/presentation
identity and prove `source >= visible >= contributor == drawn`. The measured
window starts immediately before the first measured camera input and ends at
the final renderer terminal; per-frame `frame_wall_ms` is retained separately.

The machine result is deliberately finite: all paired deltas favoring one lane
produces `candidate` or `exact`; mixed/tied observations produce
`inconclusive`. Percent changes are observations and are not hard gates. Fresh
output, one execution, and no automatic retry are fail-closed. A real clean-SHA
K1d rerun followed by complete-Truck Chrome execution remains **Deferred** to
the root endpoint owner. This mechanism alone is not PlayCanvas comparison or
Q1 acceptance.

## Q1 Truck pair-admission mechanism candidate (2026-07-28)

The Q1 same-Chrome WebGPU isolation lane now has a pure offline admission and
finite-verdict mechanism candidate. It first invokes the canonical benchmark
artifact validator, then adds the frozen complete Truck SH3, two-view 1080p,
20+80, actual WebGPU/build/environment, control-versus-throughput, common
terminal-window, same-present image and five-pair AB/BA obligations documented
in `tests/perf/q1-truck-paired-comparison-v1.md`.

The mechanism leaves PlayCanvas `V/C/D` explicitly unavailable, requires exact
gsplat-rs control `C <= V <= S` and Compact `D=C`, and rejects copied counts in
the timed artifacts. Actual timestamps—not pair labels—must prove the
predeclared endpoint order. Image receipts bind fully decoded RGBA8 PNGs to a
structured frozen-trace and artifact-terminal presentation identity. Reference
identities participate in the predeclared schedule hash, and the validator
recomputes SSIM with the repository's locked algorithm and actual
comparator-tool content hash instead of trusting an IHDR header or self-reported
score. Required JS/WASM/package files and thermal pre/post receipts are likewise
admitted by content rather than by an arbitrary runtime label.

The follow-up receipt hardening is aligned to each real producer boundary.
gsplat-rs owns same-present RGBA8 through the terminal
`capture_depth_precision` receipt; it does not own the PNG hash. A host-only
admission join decodes the referenced PNG and binds its raw RGBA digest to that
renderer receipt without prescribing a PNG codec.

The first schema shape incorrectly asked one control manifest to prove both
trace-view captures. The corrected shape treats Q0's control as an evidence
set: two untimed native control artifacts, one per trace, plus one separately
timed throughput artifact. AB/BA timestamps compare throughput only. Each image
references its same-trace control manifest by path and SHA rather than copying a
synthetic renderer envelope.

PlayCanvas now has a real producer candidate at `6c3df43`: the validator accepts
only its native `presentation_capture.renderer_capture`, final presentation
frame copy join, terminal queue drain, exact raw camera JSON/hash, source and
resolution receipts, plus the separate native host materialization receipt.
The older retained Q1 inputs still lack those two per-trace native producer
artifacts and therefore remain candidate-only **Deferred** with no performance
claim; the producer is no longer permanently hard-coded unavailable.

The read-only source for that correction was
`target/qualification/k1d-web-quality-78d9d4a-attempt-3/artifact`: its first
Exact capture retains renderer RGBA
`b0efb0f89ffeb854bfcbe4b82025ebfdb8f14f70ca0d1e506cfd1835a09291cd`
inside `capture_depth_precision`, while the separately materialized PNG is
`643a4cc13a5cb1272b5df4b2323af4623237d4770dd4ebe9fed799c9943ed09f`.
Decoding the retained PNG reproduced the renderer-owned RGBA digest. This is
gsplat-rs producer evidence only; its PNG bytes remain host-owned and are not
used to define a cross-endpoint encoder contract.

This slice performs no browser or device run. The historical unpaired
PlayCanvas prerequisite and fixed-gsplat-rs diagnostic candidate remain
inadmissible because they do not supply five fresh pairs, one common terminal
primitive, common image receipts and complete frozen build/environment
identity. Q1 therefore remains **Active**. Root must integrate the real
PlayCanvas producer and collect one fresh predeclared two-control-per-endpoint
series; this validator can then reach a comparative verdict without another
schema revision. Missing provenance does not authorize automatic retries, and
no lead-percentage threshold can keep the task spinning.

## Q1 Web timing semantics and integrated admission checkpoint (2026-07-28)

Root subsequently integrated the PlayCanvas renderer-owned WebGPU capture
producer, the gsplat-rs browser timing collector and their publication
hardening through `beba235`. A fresh K1d run at that source identity retained
complete Kitsune SH3, the `0 -> 1 -> 0` trace and true internal/presented
`1920x1080`; its ExactFull32 versus CandidateStable20 captures passed with
SSIM `0.9998697584 / 0.9996730161 / 0.9998697584`, exact alpha and stable
return-to-frame-zero renderer RGBA hashes.

The separate complete-Truck gsplat-rs diagnostic executed three
counterbalanced pairs, each lane with 20 warmup plus 80 measured frames. Exact
terminal means were `19.1675 / 19.36375 / 19.1675 ms`; Candidate20 terminal
means were `19.3925 / 18.33625 / 18.33625 ms`. Pair directions disagreed, so
the finite machine result is `inconclusive`. This is not a PlayCanvas
comparison: the comparator prerequisite is an unpaired rAF/queue-drain sample,
the two endpoints do not yet share the Q1 two-control artifact shape, and
PlayCanvas does not expose post-projection `V/C/D`.

To prevent a repeat of the rejected apparent `66.875 ms` versus `17.6675 ms`
ratio, `71173fe` renamed the browser host-call fields to
`renderer_call_wall_ms` / `renderer_call_wall_mean_ms` and explicitly leaves
the cross-implementation metric unavailable. A renderer call interval is not
a browser rAF interval, queue terminal, GPU time or presented frame time.

The cumulative Q1 offline admission mechanism is integrated through `2c39306`.
It requires five fresh AB/BA pairs; two trace-specific untimed controls plus a
separate throughput artifact per endpoint and pair; full source, SH, backing,
build, environment and thermal identity; same-present images; and one common
terminal primitive. PlayCanvas camera receipts are checked against the
existing JavaScript trace authority for pose, intrinsics and all produced
matrices. Legacy or missing real capture evidence is `Deferred`, partial or
inconsistent evidence is `Rejected`, and admitted quality/performance produces
one finite result without an automatic retry or percentage success gate.

Integration advanced `trace-camera.js` with the separately accepted renderer
capture proof, so the generated camera-authority fixture was refreshed only to
bind that newer source SHA; its camera receipts did not change. The generator
`--check`, 35 focused admission tests, 15 PlayCanvas camera/capture tests, 21
canonical benchmark-artifact tests and the source architecture policy pass.

Q1 remains **Active**, but not because a benchmark percentage failed. The next
bounded implementation slices are the real PlayCanvas Q1 two-control/
throughput producer mode, then the equivalent gsplat-rs mode, then the finite
five-pair orchestrator. Launching thirty browser executions before those two
producer contracts exist would create another expensive but inadmissible
sample, so no endpoint series is authorized at this checkpoint.

## Q1 PlayCanvas two-control/throughput producer mechanism (2026-07-28)

The existing pinned PlayCanvas runner now has an explicit Q1 producer request
boundary. One invocation produces one fresh artifact only: a trace-0 or trace-1
control retains the already integrated native WebGPU same-present capture;
throughput binds both exact controls and disables the untimed presentation and
copy submission instead of measuring one shape and publishing another.

The request locks complete Truck SH3, the two-view sequence, 20+80 frames,
local Chrome/WebGPU and 1920x1080. The host records the actual browser binary
and normalized argv from the launched child process, redacting only its
ephemeral profile path and CDP port. WebGPU provides the renderer-selected
adapter/device limits, while the pre/post macOS OS build supplies the named
Apple Metal driver-stack identity; an adapter description is never mislabeled
as a driver. Power and thermal observations, pinned PlayCanvas identity, and
content-addressed runtime-tree/package-lock copies are retained. The build
inputs are rehashed after browser/server cleanup, and blocker or cleanup-blocker
artifacts are inadmissible. Artifact roots must be fresh children of the
declared series root; requests with duplicate/mixed control bindings or
different execution parameters fail before browser work.

The throughput loop performs zero per-frame presentation-observer reads and
does not issue the post-terminal renderer capture/copy. Controls retain the
presentation/capture observations outside the throughput claim.

This slice adds producer mechanism and focused tests only. It does not run a
browser, create a five-pair schedule, collect a formal Truck artifact, compare
an endpoint, or change gsplat-rs renderer/public API behavior. Q1 therefore
remains **Active** pending the equivalent gsplat-rs producer and the separately
authorized finite series orchestration/collection.

### Q1 actual adapter/device identity contract repair (2026-07-28)

The PlayCanvas producer and offline admission contract now distinguish three
facts that the earlier `adapter_limits_sha256` field conflated:

- `graphicsDevice.gpuAdapter` is the renderer's actual selected adapter and
  owns the complete adapter-supported limit receipt;
- `graphicsDevice.wgpu` is the actual requested GPUDevice and owns the complete
  endpoint-effective limit receipt; and
- only the 29 WebGPU limits mapped directly by wgpu 28 form the canonical
  supported-limit hash used across endpoints.

Endpoint-effective limits and full endpoint receipts remain frozen across a
producer's controls and throughput artifact, but do not need to equal the
other implementation's requested limits. Cross-endpoint admission uses the
actual-selection/backend class, canonical supported-limit hash, and the same
host/browser/OS/driver-stack fields. It explicitly records that a cross-stack
hardware name is unavailable because wgpu 28 BrowserWebGpu returns opaque
`AdapterInfo`; PlayCanvas' real `GPUAdapterInfo` remains endpoint-only evidence
instead of being compared with an invented wgpu name. Missing provenance or
limits is rejected, and no collector is permitted to call `requestAdapter()`
again for evidence.

This is a mechanism-only repair. No browser or Truck performance run was
performed, and Q1 remains **Active** pending the aligned gsplat-rs receipt and
finite orchestrated series.

## Q1 gsplat-rs Web producer mechanism candidate (2026-07-28)

The gsplat-rs endpoint now has a bounded Q1 producer mode layered on the
existing Truck collector. Each untimed control selects trace `0` or `1`, arms
the existing renderer-owned Surface capture before its frozen terminal frame,
and joins the returned ExactFull32 RGBA receipt to that same successful present
and current-stats terminal. The package wrapper forwards the diagnostic capture
without inventing or normalizing renderer identity. Host code materializes the
renderer-owned RGBA8 as PNG; no PNG hash is attributed to the renderer.

The independently timed throughput role retains the existing continuous
20-warmup/80-measured terminal window, removes per-frame V/C/D and capture
fields, and binds both control manifest identities. A caller-provided run
context carries the predeclared pair/configuration only; the hardened producer
below observes the physical environment and materializes immutable build
artifacts itself. Q1 also requires a fresh clean same-commit `quality-exact`
WASM build receipt before Chrome can start.

This slice ran no browser and publishes no performance or image result. The
formal five-pair orchestrator, fresh Chrome series, image joins and offline
verdict remain **Deferred** to their declared owners. Focused Q1/package tests,
all Web example tests, all package tests, the 35-test Q1 admission suite,
source architecture policy and the Web distribution build pass.

### Queue-terminal fairness repair

The Q1 throughput window no longer labels the next rAF observation of a
current-stats map as WebGPU queue completion. Its quality-exact-only diagnostic
seam registers `Queue::on_submitted_work_done` immediately after the warmup or
final measured render submission and captures `performance.now()` inside that
callback. Current-stats remains a separate same-submission identity/count
proof. Polling either result submits no command buffer, copy or capture, and
the Q1 decorator rejects the old rAF-observed timestamp source rather than
relabeling it.

The direct successor also closes the three producer-fairness P1s without
running a browser series. Q1 now requires a headful 1920x1080 DPR-1 Chrome
session and retains pre/post visible, focused, CSS, visual-viewport and backing
receipts. It hashes the actual Chrome executable and normalized process
`spawnargs`, observes stable browser/adapter/limits plus macOS build, power and
thermal state, and verifies the served JS/WASM/build receipt before and after
the run. The external context supplies only predeclared pairing/configuration;
the artifact's physical build/environment identity is generated by this
collector, with immutable build files copied into the fresh artifact.

The shared offline admission still owns blocker/cleanup-blocker rejection and
the finite five-pair series remains **Deferred**. Until the hardened producer
is independently reviewed and integrated, this mechanism candidate must not
support a comparative performance claim.

### Renderer-device and atomic-publication provenance repair

The gsplat-rs Q1 producer no longer performs a second browser
`requestAdapter()` for environment identity. A diagnostic-only receipt now
comes from the actual renderer-owned `SurfaceRenderSession` and retains the
selected adapter's supported limits separately from the effective limits of
the created device. Because pinned wgpu 28 does not expose browser
`AdapterInfo`, the receipt explicitly records
`unavailable_wgpu28_web_backend`; it does not invent a name, vendor, device or
driver. Cross-endpoint identity is the canonical 29-field supported-adapter
limit subset. Endpoint-specific effective device limits remain evidence but do
not create a false hardware mismatch.

Q1 output is now claimed before the HTTP server or Chrome starts. Every file
is written into one unique cell staging directory, browser/server cleanup is
awaited, and repository HEAD/status, served first-party module hashes and
runtime package hashes are compared pre/post before the staging directory can
be atomically renamed. Collection and cleanup failures publish explicit
blockers, never a complete-looking final artifact, and authorize no automatic
retry.

This mechanism slice ran no browser series and makes no quality or performance
claim. Focused renderer-device, transaction, cleanup and environment tests,
the full 143-test Web example suite, 55 Web package tests, the 37-test Q1
comparison validator, renderer library tests (488 passed, eight existing
research/asset tests ignored), wasm32 check, formatting and source architecture
policy pass. PlayCanvas must still expose the same actual selected-adapter
supported-limit receipt before root integration can authorize the finite paired
series; Q1 remains **Active** and endpoint evidence remains **Deferred**.
## Q1 finite five-pair orchestration mechanism (2026-07-28)

`tests/perf/collect-q1-truck-paired-series.py` now owns the previously missing
finite outer series without copying either endpoint producer. A seeded
declaration contains exactly five counterbalanced `playcanvas-first` /
`gsplat-rs-first` pairs. Every pair has exactly six predeclared invocations:
trace-0 control, trace-1 control and throughput for the first endpoint, then
the same three for the second endpoint. The resulting 30 argv, environments,
fresh artifact paths, pairing identities and dynamic control-binding sources
are written before any browser action.

Dry-run/print-only produces that complete plan with no filesystem or browser
side effect. Formal execution requires an explicit reviewed full SHA equal to
clean `HEAD`, atomically claims one absent series root, and never retries a
producer. The two native control manifests are canonically validated and
hashed before throughput; the exact hashes, run IDs and configuration digest
are then bound through the producer's existing contract. A first failure stops
the matrix and leaves a non-retryable blocker in the claimed root.

Only after all 30 commands succeed does the owner materialize the twenty
reference-image comparisons and final evidence schedule. It then calls the
existing offline Q1 validator, which remains the sole quality and performance
decision owner. This candidate runs focused unit/policy verification only: no
Chrome instance, Truck render, formal series artifact or comparative result is
created. Q1 remains **Active** pending fixed-SHA review, integration of the
gsplat-rs endpoint producer, and one separately authorized execution.

The first fixed-SHA review rejected `648b960` for three formal evidence gaps.
The repair freezes and post-run rechecks the browser, Wasm/build,
producer/runtime, validator/tool, dataset/trace/reference and clean reviewed
commit inputs; passes producers an exact allowlisted child environment rather
than ambient shell state; and performs complete canonical Q1 admission of both
controls immediately before throughput. This remains candidate-only: no
browser was started and no result was created. After integration, the new exact
SHA still needs its own fixed-SHA review; neither the rejected SHA nor a
component review authorizes collection.

The follow-up fixed-SHA review of `cc9b43e` found that postprocessing could
still rediscover a different Chrome, subprocesses lacked finite liveness
bounds, and only the Puppeteer top-level directory/lockfile—not its installed
production dependency closure—was frozen. The next repair binds the image
tool's actual Chrome path/hash to the formal receipt, gives every child role a
wide predeclared non-performance timeout with one-shot blocker semantics, and
hashes the package-lock-derived Puppeteer production module graph before and
after the run. This is still candidate-only and ran no Chrome workload. Its
new integrated SHA must pass another fixed-SHA review before collection.

The next review rejected `26bec8f`: Python's simple timeout killed only the
Node leader and could leave Chrome/server descendants alive, while the npm
closure did not yet model installed optional/peer runtime dependencies. The
repair now runs every external command in a fresh process group and records a
bounded TERM/grace/KILL/reap receipt on timeout. Its package-lock and installed
package manifests jointly close the reachable production graph: installed
optional/peer modules are hashed, absent platform-only optional modules are
allowed, and missing required or installed-but-unlocked modules fail closed.
The focused process test uses a real leader/grandchild pair that ignores TERM;
no Chrome workload or Q1 result was produced. The resulting SHA remains
candidate-only until another fixed-SHA review.

The fixed-SHA review of `7ccf904` rejected two remaining proof gaps. Puppeteer
may launch Chrome in a detached process group, so the owner now polls the
macOS/Linux process table during execution and retains PID/PPID/PGID/start
lineage across reparenting. Both timeout and normal-exit cleanup rescan known
descendants, signal only identity-verified child-owned groups/PIDs, and reject
any surviving or unproven tree. The npm lock is now the sole authority for
required/optional/peer classification; installed manifests must match locked
name, version and runtime declarations, stay below `node_modules`, and contain
no symlinked package directory, manifest or file. Real detached-child tests
cover timeout and zero-exit counterexamples. No Chrome workload or formal
artifact was run; the new SHA still requires fixed-SHA review.

### Q1 Direct-f32 reference-authority admission candidate (2026-07-28)

The offline Q1 gate now treats the independent native Direct-f32 producer as
the sole image authority instead of accepting any decodable, self-consistent
1920x1080 PNG. Both predeclared views bind one blocker-free
`gsplat-q1-direct-f32-reference/v1` receipt by path and SHA. Admission verifies
the clean full commit, Cargo/Rust/producer identities, the retained release
binary inside the authority root, exact Truck SH3 and frozen trace/view hashes,
Direct wide-f32 CPU ExactFull32 stable SortedAlpha GlobalQuads execution,
complete five-stage membership, disabled quality reductions and exact per-view
PNG/decoded-RGBA plus `0<D=V<=S` receipts. The authority must predate the
predeclared schedule, its commit must equal every endpoint artifact commit, and
its identity is retained in Deferred and finite terminal results.

This is an offline mechanism candidate only. It performs no browser/device run
and does not authorize a comparison by itself. The separate five-pair
orchestrator must emit the new shared authority fields after this candidate is
independently reviewed and integrated; no formal series should start before
that schedule-emitter sync and a fresh authority artifact exist. The 64 focused
Q1 admission tests, five producer-transaction tests, source architecture policy
and diff hygiene pass on this isolated candidate.

The follow-up fixed-SHA repair rejects lexical symlink aliases for the authority
directory/receipt, retained binary and reference PNGs by checking every path
component before resolution. This contract detects retained mutation and join
drift; it is not a cryptographic attestation against an actor who can rewrite
the receipt and all hashes together, and this slice does not add signing.

The second fixed-SHA repair extends the same fail-closed rule to series-root and
endpoint `blocker.json` / `cleanup-blocker.json`: a dangling symlink is still a
retained blocker and cannot disappear merely because `Path.exists()` follows
it to a missing target.

### Q1 reference-authority schedule wiring candidate (2026-07-28)

The five-pair owner now accepts one required `--reference-authority` instead of
two independently supplied PNG paths. Before either dry-run output or root
claim, the shared admission owner validates the complete blocker- and
symlink-free `gsplat-q1-direct-f32-reference/v1` tree, requires its clean commit
to equal the reviewed SHA and its timestamp to precede the schedule, and emits
the authority receipt, PNG, decoded RGBA and pose/intrinsics identities for
both frozen views. Dry-run performs the same complete read-only Git, Chrome,
process-table, Wasm, Puppeteer and formal-input preflight as execute while
retaining zero filesystem, build or browser side effects.

Execute copies the entire locked authority tree into the fresh series root
without following symlinks, verifies each open source file descriptor, and
rechecks both source and retained identities before the first producer and at
the post-run boundary. Formal-input and execution-lock receipts preserve these
pre/post joins. Process cleanup tests also cover a descendant first discovered
after tracker shutdown and a persistently unavailable process-table snapshot.
This candidate performs no reference generation, Chrome/device run or formal
comparison. Q1 remains **Active** pending fixed-SHA review, integration, a new
same-SHA authority artifact and one separately authorized finite execution.

The fixed-SHA review of `33539c8` rejected three remaining authority/schedule
proof gaps. Its direct-successor repair makes the predeclared UTC timestamp an
explicit non-future input reused unchanged by dry-run and execute, so identical
arguments produce byte-identical plans and command receipts. Offline admission
now joins the formal input, command receipt, execution lock and post-run
authority identities to every scheduled PNG/RGBA/pose receipt and to a fresh
semantic re-admission of the complete retained tree. Authority copying now
anchors root directory descriptors and opens every relative directory/file
with no-follow `dir_fd` operations, rejecting an intermediate-directory
symlink swap. These are mechanism tests only; no producer or endpoint ran.

### Q1 formal attempt 1 terminal and capture phase repair (2026-07-28)

The reviewed root SHA `16855499a73707dba0ab447dfdf76633589bb378`
produced a same-SHA Direct-f32 reference authority and quality-exact Wasm
package, then passed the complete dry-run twice. The separately declared
formal series stopped fail-closed on invocation 1 of 30, the gsplat-rs trace-0
control. No PlayCanvas endpoint ran and the failed series therefore supports
no quality or performance comparison. Its immutable attempt-1 root retains the
claim, browser ownership handshake, process cleanup receipt and non-retryable
blocker; Chrome and the HTTP server left no surviving process.

The failure was a producer state-machine contract drift. The canonical camera
trace labels measured steps `measure`, while both Q1 capture consumers in the
Web example tested the unreachable string `measured`. The renderer capture was
armed, but JavaScript could neither take it nor attach its same-present receipt,
so the final guard correctly rejected publication. A shared
`isQ1CaptureTraceStep` predicate now owns this phase/index/trace match for both
consumers. Focused tests use the real 20-warmup/80-measured alternating trace:
logical frames 98 and 99 select measured indices 78/79 and trace frames 0/1;
warmup, non-target and wrong-trace steps reject.

This repair does not change renderer execution, camera input, scene membership,
resolution, timing policy or either competitor endpoint. It authorizes no
automatic retry. A new attempt requires a new reviewed SHA, regenerated
same-SHA authority and Wasm artifacts, a fresh immutable series root and a new
successful dry-run/go-no-go review.

### Q1 formal attempt 2 terminal and optional contributor repair (2026-07-28)

The separately declared attempt 2 at reviewed root SHA
`acfb7bf6d94b5e8316f5a6b81d698b096ac5caf8` stopped fail-closed on invocation
1 of 30, the PlayCanvas trace-0 control. No gsplat-rs endpoint or later
PlayCanvas invocation ran, so this immutable attempt supports no quality or
performance comparison. Its top-level blocker records exit status 2 and keeps
automatic retry disabled.

The renderer capture itself completed, but PlayCanvas artifact materialization
emitted `contributor: null` without `exact_contributor_compaction`. The shared
artifact contract makes contributor evidence optional; presence means the
producer claims the explicit contributor contract, so the canonical validator
correctly rejected the unmatched field. PlayCanvas does not expose this count.
The producer now omits both optional contributor fields while retaining
`frames[*].contributor` in `unavailable_fields`; it continues to report
`visible` and `drawn` as unavailable rather than inventing counts.

Focused tests run the canonical validator against both sides of that boundary:
null visible/drawn with omitted contributor evidence passes, while a lone null
contributor remains rejected. This is a producer artifact repair only. It does
not modify the validator, competitor engine, rendering, timing, camera, or
quality policy; it ran no browser or formal experiment and authorizes no
automatic retry.

### Q1 formal attempt 3 terminal and cross-language identity repair (2026-07-28)

The separately declared attempt 3 at reviewed root SHA
`7693a07c1ee471f62df0df4e7ebef240bef2a5eb` completed both gsplat-rs control
views and stopped before its first throughput invocation. PlayCanvas did not
run, no timed endpoint pair exists, and the immutable attempt supports no
quality or performance comparison. The retained blocker records that offline
admission expected the schedule/source-manifest dataset id while the real
gsplat-rs Web producer emitted its canonical file/logical identity.

The repair makes the artifact admission namespace explicit. The schedule and
PlayCanvas remain bound to `inria-3dgs-truck-iteration-30000`; gsplat-rs must
instead emit exactly `truck.ply`, logical id `truck`, and the frozen Web source
path. Both forms still require identical source hash, bytes, splat count and SH
degree. The Direct-f32 reference authority keeps its separate `truck-full`
suite id. Drift in any gsplat-rs identity component fails closed.

Replaying the two real attempt-3 controls after that fix exposed one additional
cross-language mismatch before a fourth browser attempt: JavaScript used
locale-sensitive key ordering for the selected adapter/device limit hashes,
while Python admission used code-point ordering. The producer now uses explicit
code-point ordering. Node tests bind those hashes, and Python invokes the real
JavaScript dataset/environment owners before exercising offline admission.
With only the two known old locale-sensitive hashes normalized in memory for
diagnosis, both retained attempt-3 controls cross the complete artifact
admission path; no other field drift remains. The retained files themselves
were not changed. A new formal attempt still requires a new reviewed SHA,
regenerated same-SHA authority and Wasm artifacts, a fresh series root, two
identical dry runs and a separate go/no-go decision. Automatic retry remains
disabled.

### Q1 formal attempt 4 terminal and admitted-environment join repair (2026-07-28)

The separately declared attempt 4 at reviewed root SHA
`a9a85e48291ffcf191b1c94959f4a4e493608ba9` completed the two PlayCanvas
control views and stopped before PlayCanvas throughput. No gsplat-rs endpoint
or timed endpoint pair ran, so this immutable attempt supports no quality or
performance comparison. Its blocker keeps automatic retry disabled.

Both controls contained the exact declared `collection_session_id`; the
orchestrator read the wrong level of its own admitted DTO. Artifact admission
returns environment evidence as `{identity, cross_identity, thermal}`, but the
throughput resolver looked for `collection_session_id` beside those owners
instead of inside `identity`. The repair reads the already-validated endpoint
identity without changing either producer, the environment contract, or any
render/timing behavior. Focused orchestration fixtures now use the real nested
admission shape so this owner boundary cannot be hidden by a flat mock.

A later formal attempt still requires a new reviewed SHA, regenerated same-SHA
authority and Wasm assets, a fresh root, identical dry runs and a separate
go/no-go. Attempt 4 is retained unchanged and is never reused.

### Q1 formal attempt 5 terminal and macOS Chrome ownership repair (2026-07-28)

The separately declared attempt 5 at reviewed root SHA
`649d40b4d64746b9900c6ac51d09611c028ae25d` stopped after its first
PlayCanvas control. No gsplat-rs endpoint or timed endpoint pair ran, so this
immutable attempt supports no quality or performance comparison. Its blocker
keeps automatic retry disabled, and cleanup proved that no marked browser
process survived.

The control producer completed successfully, but the ownership validator
rejected its browser handshake because the observed macOS Chrome leader had
changed its visible command to `(Google Chrome)`. The producer's immutable
spawn receipt contained the exact unique `--user-data-dir` argument, and
observed Chrome helpers in the same browser process group retained that exact
argument. Attempt 4 happened to capture the leader before this argv change, so
the old check was timing-dependent.

The repair remains fail closed: it requires a coherent spawnfile/spawnargs
receipt containing the exact marker once, the exact browser PID to have been
observed, and either that PID or a direct-child/same-process-group identity to
carry the exact marker. A marker in an unrelated process group and a forged or
incomplete spawn receipt are rejected. A later formal attempt still requires
a new reviewed SHA, regenerated same-SHA authority and Wasm assets, a fresh
root, identical dry runs and a separate go/no-go. Attempt 5 is retained
unchanged and is never reused.

### Q1 formal attempt 6 terminal and gsplat-rs throughput receipt repair (2026-07-28)

The separately declared attempt 6 at reviewed root SHA
`f8ad83cb571345f65ddef58f6b4235a395174c53` completed both gsplat-rs control
views and stopped while materializing its first throughput artifact. PlayCanvas
did not run, no complete timed endpoint pair exists, and this immutable attempt
supports no quality or performance comparison. Its blocker keeps automatic
retry disabled; the retained browser ownership receipt is verified and cleanup
reports no surviving marked process or process group.

The 80-frame renderer run completed, but the Q1 throughput decorator replaced
the deliberately unavailable contributor count with `contributor: null` while
leaving `exact_contributor_compaction` absent. The canonical artifact contract
treats those keys as one optional evidence claim and correctly rejected the
half-present pair. Throughput obtains exact V/C/D only from its two same-SHA
control bindings, so the repair omits both optional contributor keys while
continuing to publish visible/drawn as unavailable and retaining all three
paths in `unavailable_fields`.

A read-only replay of the retained 80-frame staging artifact after that repair
then found a second stale owner before another browser run: the canonical
Python validator still admitted only the older current-stats-poll terminal,
while both the Web producer and Q1 fairness gate require the newer direct
`queue.onSubmittedWorkDone` completion callback. The validator now admits that
second form only with its exact timestamp source, V2 overhead kind, competitor
Promise primitive, callback flag and same-primitive fairness receipt; the
legacy fixture remains admitted under its original, field-absent contract.
Mutation tests reject a wrong kind, missing fairness receipt and mismatched
competitor primitive.

This is an artifact-schema repair only. It does not change renderer execution,
scene membership, camera input, resolution, timing, either competitor endpoint
or the admission validator. A later formal attempt still requires a new
reviewed SHA, regenerated same-SHA authority and Wasm assets, a fresh root,
identical dry runs and a separate go/no-go. Attempt 6 is retained unchanged and
is never reused.

### Q1 formal attempt 7 terminal and symmetric host-start repair (2026-07-28)

The separately declared attempt 7 at reviewed root SHA
`7aa61a1cdcfc66411af4084bc7543d2f45a45f9f` completed all six invocations in
pair 1 and then stopped at pair 2's first gsplat-rs trace-0 control. The page
reported that its pre-measurement browser runtime was not a visible, focused
1920x1080 DPR-1 surface. The immutable root and producer blockers disable
automatic retry, and the retained process receipt proves the marked Chrome
process group was reaped with no survivors. Pair 1 is not a five-pair series,
so attempt 7 supports no quality or performance comparison.

The failure was a start-ownership race rather than a render or timing result.
Both pages previously began warmup immediately when their 2,541,226-splat
scene became ready. The gsplat-rs collector called `bringToFront()` only before
navigation and the PlayCanvas collector did not own an equivalent post-load
foreground boundary, so a later focus check could reject after useful work had
already started. Pair 1 happened to retain focus; that did not make the
mechanism deterministic.

The repair is symmetric and does not change either renderer, workload or
terminal timing primitive. Both pages now finish scene/device/surface setup,
publish an armed host-start state with zero warmup frames consumed, and wait.
Their collectors bring the page to the foreground, wait a bounded interval for
live focus, visibility, DPR 1 and exact 1920x1080 window/visual-viewport/CSS/
backing dimensions, then call a one-shot start function. The page atomically
revalidates that receipt before starting the existing warmup. Timeout or drift
fails before warmup; no sleep, `window.focus()`, relaxed focus condition or
automatic retry is introduced. A later formal attempt still requires a new
reviewed SHA, regenerated same-SHA authority and Wasm assets, a fresh root,
identical dry runs and a separate go/no-go. Attempt 7 is retained unchanged and
is never reused.

### Q1 formal attempt 8 terminal and raw-PNG metric repair (2026-07-28)

The separately declared attempt 8 at reviewed root SHA
`048faeabbfb2377332b4f0c8db7dd6e2a788977d` completed all 30 predeclared
browser invocations across five counterbalanced pairs. The symmetric host-start
gate therefore fixed attempt 7's focus race, and neither endpoint produced a
runtime or queue-terminal blocker. The final offline validator nevertheless
rejected the series before quality or performance admission because every
recorded image score differed from the score recomputed from the retained PNG
sample bytes. The immutable result remains `Rejected`,
`evidence_admitted=false`, `performance=null`, `retry_authorized=false`; the
30 completed runs cannot be recovered into a comparison.

The mismatch was deterministic evidence plumbing, not a renderer-performance
result or a wrong image join. The locked Node comparator decoded each PNG via
`Image -> Canvas2D -> getImageData`, while the independent Python validator
decoded the raw non-interlaced RGBA8 samples. The Direct-f32 references contain
partial alpha, so Canvas premultiplication and unpremultiplication changed many
RGB samples by one code value; the opaque endpoint images were unchanged. All
20 comparisons therefore failed in the same direction by approximately
`6.6e-5` to `8.0e-5`, far outside the intentionally strict `1e-9` receipt
tolerance.

The repair does not loosen that tolerance or alter either renderer. The Node
tool now strictly parses CRC-checked non-interlaced RGBA8 PNGs, rejects color
management outside the raw-byte contract, unfilters the original samples and
runs the existing metric directly on those bytes. Chrome launch and ownership
remain part of the formal execution identity, but Canvas is no longer a pixel
decoder. The imported metric module has its own path and SHA receipt and is a
formal locked input. A partial-alpha cross-language fixture requires Node and
Python to agree within `1e-9`; all PNG filter types and fail-closed chunk/CRC
boundaries have focused coverage.

A later formal attempt still requires a new reviewed SHA, regenerated same-SHA
authority and Wasm assets, a fresh root, two identical dry runs and a separate
go/no-go decision. Attempt 8 is retained unchanged and is never reused.

### Q1 forward-only absolute workload observation contract (2026-07-28)

The Q1 result schema advances to
`gsplat-q1-truck-paired-result/v2`. A fully admitted series that misses the
predeclared image gate still terminates `Rejected` with `performance=null` and
`claim_scope=null`; it may now retain only the two endpoints' absolute
terminal-mean medians and five pairs of absolute terminal means plus their
predeclared order under the versioned `workload_timing_observation`. The
receipt marks both same-quality performance
and aggregate eligibility false and publishes no delta, ratio, FPS, direction
or winner. Deferred, structurally invalid, producer-incomplete, thermally
inadmissible or identity-invalid evidence retains a null observation. An image
pass keeps the existing performance semantics and has no workload-only
observation.

This is a forward-only evidence-contract slice. It ran no browser or device and
does not rewrite attempt 8's immutable v1 rejection or derive observations from
its retained files. A later formal attempt still needs a new reviewed SHA and
the existing one-shot execution gates.

### Q1 formal attempt 9 terminal and runtime-tree ordering repair (2026-07-28)

Attempt 9 at reviewed SHA
`6f11ffa4191812d5306a6ac6cc6df2e6d0378a3a` passed two byte-identical dry
runs, completed all 30 predeclared browser invocations without a producer or
cleanup blocker, then stopped at final admission. Its immutable v2 result is
`Rejected`, `evidence_admitted=false`, `performance=null` and
`workload_timing_observation=null`; no endpoint timing is recovered from it.

The final mismatch was a set-equality bug in the formal PlayCanvas runtime
tree join. The Node producer and Python lock contained the same 2,389 unique
paths with identical byte counts and file hashes. Node sorted complete portable
path strings, while Python's repository lock sorted `Path` components; these
orders first differ when a directory such as `glb/` has a sibling such as
`glb-animation`. Hashing the unsafely order-sensitive lists therefore reported
false content drift.

Admission now validates unique portable relative paths and normalizes the
producer list to the formal component-wise path order before hashing. It still
rejects duplicate, absolute, parent-traversing or non-portable paths and any
real file-set, byte-count or content-hash difference. A later formal attempt
requires a new reviewed SHA, regenerated same-SHA assets, fresh root and the
existing one-shot gates; attempt 9 is never reused.

### Q1 formal attempt 10 admitted workload and image-gate rejection (2026-07-28)

Attempt 10 at fixed reviewed SHA
`630f9d3f173b89c068274ddb931608cd5fdfb15f` used regenerated same-SHA
quality-exact Wasm and Direct-f32 authority assets, two byte-identical dry
runs, and an independent go/no-go review. The one authorized execution
completed all 30 predeclared browser invocations across five counterbalanced
pairs. Every command returned successfully with 80/80 retained frames; runtime
tree, authority, browser, cleanup and thermal admission passed, and no blocker
or survivor was published.

The immutable v2 result is nevertheless `Rejected` because the predeclared
common-reference image gate failed. Raw RGBA8 recomputation over all 20 images
matched the retained scores exactly. gsplat-rs scored approximately
`0.999970` to `0.999971` against the independent Direct-f32 authority, while
PlayCanvas scored approximately `0.938169` to `0.948069`; the minimum observed
score `0.9381693241147921` is below the shared `0.99` threshold. The result is
therefore `evidence_admitted=true` but `quality_passed=false`, with
`performance=null`, `claim_scope=null` and `retry_authorized=false`.

The forward-only workload receipt retains only non-comparative absolute
terminal means. Across the five pairs, the median gsplat-rs observation was
`20.74125000089407 ms` and the median PlayCanvas observation was
`17.93500000089407 ms`. These values prove that both endpoints completed the
named matched Truck/Chrome/WebGPU/1080p workload; they do not support a ratio,
relative slowdown, FPS advantage, winner or broader product-performance
claim. An independent result audit accepted the root with P0/P1/P2 `0/0/0`.
The earlier `66.875 ms` versus `17.668 ms` diagnostic is not a valid formal
comparison and is superseded as a decision input by this admitted workload
receipt.

Q1 remains active. The next bounded slice is image-contract attribution:
identify the smallest renderer-semantic differences responsible for the
PlayCanvas reference gap before changing either performance path or quality
threshold. Attempt 10 remains immutable and must not be revalidated into a
same-quality comparison.

### Q1 Attempt 10 footprint-attribution diagnostic (2026-07-28)

The reviewed diagnostic at commit
`4a5b9dded64c692b00b8744599dfee885ca70d12` changed only the pinned
PlayCanvas forward-color Gaussian footprint to the gsplat-rs Direct-f32
contract while retaining the complete Truck source, SH3, camera, 1920x1080
backing resolution, PlayCanvas representation precision and sorting. It is
explicitly classified `mechanism_attribution_only`, contains no pairing, and
its terminal status is `valid_mechanism_diagnostic`; its frame timings are not
performance evidence.

Against Attempt 10's immutable Direct-f32 authority, the two diagnostic images
scored:

| Trace | Pair-01 pinned PlayCanvas SSIM | Footprint diagnostic SSIM | Diagnostic RGB MAE (8-bit) |
| --- | ---: | ---: | ---: |
| 0 | `0.9381725005` | `0.9543464688` | `5.5943` |
| 1 | `0.9480650642` | `0.9618119574` | `4.8727` |

The diagnostic image remained close to, but observably different from, the
original pinned PlayCanvas output: SSIM `0.9893232813 / 0.9908994297` and RGB
MAE `2.7139 / 2.1366`. This proves that footprint/support semantics account
for a material part of the common-reference gap, but they do not explain all
of it and do not reach the existing `0.99` Native Exact threshold. Remaining
high-probability mechanisms are PlayCanvas's half/quantized resident and
projected representation plus its lower-precision unstable depth ordering.

No production renderer policy changes from this diagnostic. In particular,
the project will not lower the existing threshold, adopt the diagnostic shader
as a performance path, or rerun the five-pair series merely to obtain a
relative number. The next bounded decision is to separate the current
Direct-f32 self-conformance authority from an independent product-quality
authority. The former stays strict for gsplat-rs regression evidence; a fair
cross-product lane must use independently authored source images and their
camera calibration before any same-quality performance comparison can be
admitted.

An independent raw-RGBA8 recomputation accepted this attribution with
P0/P1/P2 `0/0/0`. It also confirmed the diagnostic's intentionally narrower
presentation boundary: requested, Surface and internal-render dimensions are
1920x1080, while external presented-screen dimensions are unavailable. That is
valid for mechanism attribution but cannot be promoted into a formal endpoint
qualification receipt.

### Q1 upstream Truck product-quality authority (2026-07-28)

The first independent Product Quality input now has a fail-closed offline
builder. It pins the official Inria `tandt_db.zip` archive at 682,628,995 bytes
and SHA-256
`816e62f22a161abbfe841d2a6b10cdf036e297c9fa289b3bfeee9c6ec526d7e1`,
then binds the complete `cameras.bin` and `images.bin` records for
`000001.jpg` and `000108.jpg`. The builder accepts only the upstream authority
class, exact Truck scene identity and COLMAP PINHOLE model; hashes, dimensions,
duplicate image names, incomplete binary records, symlinks, extra retained
files and output replacement all fail closed.

The official JPEGs are 979x546 while their COLMAP calibration declares
1957x1091. The authority therefore records exact independent axis ratios
`979/1957` and `546/1091` and derives `fx/fy/cx/cy` from those ratios. It does
not resize, crop, blur or post-align source pixels. This corrects the earlier
assumption that the existing 1920x1080 performance trace could itself act as a
source-image quality camera.

A real local build produced authority class
`upstream_source_camera_images` with both named views. It launched no browser,
retained no endpoint timing and remains outside Git because upstream asset
redistribution rights are unresolved. The next slice is the pure two-lane
state reducer, followed by a one-view 979x546 quality-only smoke; neither step
authorizes another five-pair performance series.

### Q1 dual-lane reducer and official evaluation correction (2026-07-28)

The pure Native Exact/Product Quality reducer is independently accepted at
`15c63510930f198e9ebaab116c47075021786edc` with P0/P1/P2 `0/0/0`. Native
Exact consumes only gsplat-rs scores and retains the exact `0.99` Direct-f32
threshold. Product Quality accepts only the upstream authority class and both
named endpoint states. Only `Accepted + Accepted` unlocks same-quality
performance. Rejected or Deferred output recursively removes and rejects
relative fields even through arbitrarily nested list/tuple structures, while
retaining non-comparative absolute terminal means.

The official 7,064,286,140-byte Evaluation Images archive was then inspected
by HTTP range without downloading the complete archive. It corrects two
assumptions before any endpoint run:

1. the official held-out Truck views are `000001`, `000009`, `000017`, and so
   on; `000108` was a legacy performance-trace choice, not the second formal
   product-quality view;
2. the archive already contains 979x546 RGB8 ground-truth PNGs and published
   `ours_30000` renders, so Q1 does not need a browser-dependent JPEG decoder.

The published `ours_30000` Truck aggregate is SSIM `0.8787403703`, PSNR
`25.1867847443`, LPIPS `0.1477580667`. Its per-view results are SSIM
`0.9103236794 / 0.9118889570`, PSNR `26.25909805298 / 26.6379737854` and
LPIPS `0.1162385568 / 0.1099530607` for `000001 / 000009`. These upstream
numbers are threshold-calibration evidence, not either tested endpoint.

A separate camera audit found that the current 979x546 trace is still not
eligible: exact scaling yields `fx=581.9245675736333` and
`fy=578.6701201866216`, while the vertical-FOV-only runtime assumes `fx=fy`.
The approximately `-0.5593%` horizontal focal error must not be charged to
renderer quality. The next bounded implementation slices are therefore raw
RGB/RGBA PNG authority support and exact-pinhole `fx/fy/cx/cy` carriage. No
Chrome endpoint, performance pair, ratio, FPS or winner was produced here.

The raw PNG authority slice then added strict non-interlaced RGB8 decoding to
the existing CRC/chunk/zlib-bounded parser. RGB samples expand to opaque RGBA
without Canvas or color management; all five PNG filters are covered and the
existing partial-alpha RGBA8 contract is unchanged. The official ground-truth
and `ours_30000` images produced the following locked-metric baselines:

| View | SSIM | normalized RGB MAE | severe RGB tail over 32 |
| --- | ---: | ---: | ---: |
| `000001` | `0.9169842309` | `0.0267901128` | `0.0318763633` |
| `000009` | `0.9196254631` | `0.0258173395` | `0.0287334388` |

Before either endpoint image exists, Product Quality v1 is frozen per view and
per endpoint at SSIM `>=0.90`, normalized RGB MAE `<=0.05`, severe-tail
fraction `<=0.10`, and exact opaque alpha. No averaging can hide a failed view.
The old over-3/255 tail is intentionally not reused: even the official render
has roughly 69-70% of pixels above that conformance-oriented threshold.

### Q1 official Evaluation Images provenance authority (2026-07-28)

The formal Evaluation Images input now has a narrow offline authority builder.
It pins the exact official ZIP entry path and local extracted relative path,
bytes and SHA-256 for `truck/results.json`, `truck/per_view.json`, and the
`000001`/`000009` ground-truth plus `ours_30000` render PNGs. Publication is a
fresh no-replace directory transaction: all six inputs, duplicate-free JSON,
and strict 979x546 non-interlaced RGB8 PNG headers and CRCs are validated in
the staging tree before it becomes visible. Missing, changed, aliased,
escaping or symlink inputs fail closed.

A real local build retained six entries at
`target/qualification/q1-product-quality-evaluation-authority-v1` with class
`upstream_evaluation_images`. The receipt deliberately reports only
`pinned_extracted_entries_only`: the official 7,064,286,140-byte archive itself
was not retained, so `archive_sha256_verified` is false rather than invented.
Nine focused tests passed. This closes the local extracted-entry provenance
mechanism only; Product Quality remains `Deferred`, performance stays
unauthorized, and no browser, device, endpoint image, ratio, FPS or winner was
produced.

### Q1 centered-pinhole core carriage (2026-07-28)

The first exact-camera slice adds a centered calibrated `fx/fy` ratio to the
canonical Rust camera while preserving an exact default of one. CPU reference,
GPU Exact parameter construction and the FFI projection receipt all consume a
single derived `viewport_aspect / (fx/fy)` value; no WGSL uniform or stable C
camera ABI changed. The accepted ratio range `2^-16..=2^16` is a
representability guard: for any positive `u32` viewport the derived aspect stays
finite and normal, and out-of-range input fails rather than being clamped.

The Truck 979x546 focused oracle reconstructs `fx=581.92456` and
`fy=578.6701`, with CPU and GPU projection parameters bit-identical. Core,
renderer-lib, FFI-lib, workspace/Wasm checks, affected Clippy/Rustdoc, FFI
smoke, architecture, format and diff checks passed. This is not endpoint
qualification: the legacy trace adapter still selects ratio one, and the
trace, Wasm/JS and PlayCanvas receipt carriers remain separate reviewed slices.
The public Rust struct gains one field, so downstream literal construction is
a source-compatibility change; the C ABI remains unchanged. The integration
decision is explicit: this belongs to the `0.2` Rust source API boundary and
must not ship as a source-compatible `0.1.3` patch. Retaining the v0.1 Rust
surface would instead require an additive calibrated-camera carrier before
merge.

### Q1 formal Truck camera view-set authority (2026-07-28)

The source-camera authority now names two immutable view sets instead of
silently changing the existing builder default. The retained legacy
`000001+000108` input remains valid as `legacy-000001-000108`; formal Product
Quality uses `formal-000001-000009`, with `000009.jpg` pinned at 469,920 bytes
and SHA-256
`3da0fedd20eb8df970ff7ff4596526c10f9f99c4766cfe776f6bb907c6751fbd`.
Both reuse the same pinned archive, complete cameras/images metadata and Truck
scene identity.

A real fresh build at
`target/qualification/q1-product-quality-source-camera-formal-v1` validates
both named records. Each scales to `fx=581.9245675736333`,
`fy=578.6701201866216`, `cx=489.5`, `cy=273.0` at 979x546; the two principal
points are therefore exactly centered for the supported core model.

Command-line construction now requires `--view-set`; the Python compatibility
alias remains explicitly legacy so existing local authorities can still be
revalidated. Cross-validating one view set as the other fails closed. This
slice launches no endpoint and does not authorize performance; it supplies the
formal `000001+000009` camera records needed by the next trace-carriage slice.

### Q1 formal Truck Product Quality trace input (2026-07-28)

The formal camera input now joins the two previously independent immutable
authorities without launching either endpoint. A fresh no-replace builder
revalidates every retained source-camera and Evaluation Images file, requires
exact `000001+000009` membership and 979x546 dimensions, and rejects any
off-center principal point before publication. Its trace and adjacent receipt
bind both authority schemas, classes, authority-receipt hashes, and every
retained input file identity.

The generated trace converts the upstream COLMAP RDF world-to-camera poses to
runtime RUF camera-to-world poses, derives the exact vertical FOV from
`fy=578.6701201866216`, carries
`focal_length_x_over_y=1.005624011459175`, and recomputes view, projection and
view-projection matrices for 979x546 with the upstream 3DGS camera's
`znear=0.01` and `zfar=100`. The retained content hash is
`46819f71d5025bb61f6583392448d977051a0b4c67a0c05db860232033c0676c`.
Eleven focused tests cover the checked-in fixture plus synthetic construction,
complete input binding, no-replace publication, input-root output rejection,
wrong view set, off-center intrinsics, missing/input drift, trace mutation and
unreceipted output files. The real builder and the canonical trace validator
both pass against
`target/qualification/q1-formal-truck-product-quality-trace-v1`.

This is an immutable formal camera input only. Product Quality remains
`Deferred`, performance stays unauthorized, and no renderer, browser, device,
endpoint image, timing ratio, FPS or winner was produced. The next slices are
the independent Wasm/JS and PlayCanvas projection/receipt carriers.

### Q1 view 000001 offline Product Quality admission gate (2026-07-28)

The one-view slice now has a pure offline gate over the existing renderer-owned
capture shapes. The gsplat-rs adapter requires the benchmark manifest's
same-present identity, terminal-frame hash, diagnostic Surface capture receipt,
and decoded PNG RGBA digest to join. The PlayCanvas adapter requires the pinned
WebGPU copy-to-buffer producer, calibrated custom-projection camera JSON hash,
terminal queue drain, materialization receipt, and raw RGBA file. Neither
adapter changes an endpoint producer or artifact schema.

The gate revalidates the retained formal trace and Evaluation Images authority,
locks exact view `000001`, 979x546, complete Truck SH3 membership and the
calibrated focal ratio, then invokes the already frozen Product Quality reducer.
Well-formed threshold misses terminate as endpoint `Rejected`; missing,
malformed, nonopaque, stale-camera, wrong-producer, or mismatched byte evidence
fails closed before publication. Successful structural evaluation publishes a
fresh atomic `result.json` with separate endpoint decisions and no timing,
throughput, pairing, speed ratio, FPS or winner fields.

The first fixed-SHA review rejected the gate with five P1 evidence gaps. An
additive repair now decodes the official RGB8 truth, accepts the producer's
dimension-only native presentation receipt, binds the renderer-returned native
camera, joins PlayCanvas capture to its stable presentation/copy/drain chain,
and independently recomputes all PlayCanvas camera matrices from the formal
trace. Publication also rejects every overlap with an immutable input tree.

Sixteen focused synthetic tests now pass, including RGB8 Sub and Paeth filter
rows with the correct three-byte pixel stride. The real retained RGB8 ground truth
decodes to 1,603,602 RGB bytes with SHA-256
`c0e93747c7d07c30b2e2c740084bd08b1651bcc61a53424800f3e23194146d39`;
the Python camera oracle matches the reviewed JavaScript oracle across 94
values with zero observed difference. The repair still requires a fresh
fixed-SHA review. No endpoint was launched and no one-view image result exists;
Product Quality remains `Deferred` and `performance_eligible=false` until both
formal views independently qualify.

### Q1 view 000001 one-shot coordination (2026-07-28)

The two reviewed quality-only endpoint interfaces are now joined by one narrow
root-owned transaction rather than an ad-hoc launch sequence. Before any build
or browser action, it requires a clean exact full SHA, validates the retained
formal trace and Evaluation Images trees, decodes the official ground truth,
hashes the complete 630,225,580-byte Truck PLY, and proves the fresh output is
disjoint from every immutable input.

The execution order is frozen as Native quality-only once, PlayCanvas headful
quality-only once, then the offline one-view gate once. Every child writes to a
separate new directory below a hidden staging root. Any nonzero exit or
exception terminalizes that invocation immediately with no automatic retry;
the requested result stays absent and an immutable sibling failure tree keeps
the blocker. Only three successes atomically publish the top-level receipt and
children. The bootstrap profile `q1-product-quality-view000001` is opt-in and
does not touch a device; doctor and command remain read-only.

Focused tests cover exact ordering, one call per step, native/PlayCanvas/gate
failure cutoffs, exception cutoff, fresh/disjoint outputs, headful request
identity, commit/platform/tool/authority probes, and the single coordinator
command. This slice did not launch Chrome, a native window, a device, or either
endpoint and produced no formal artifact. Product Quality remains `Deferred`;
performance stays unauthorized.

The first two authorized one-shot executions stopped in the Native child
before PlayCanvas or the offline gate ran. The first exposed that the default
worktree Truck path was a symlink; commit `7442c6b` moved that real-file check
into the read-only doctor. The next run at `3aa1a6e` proved the outer
coordinator's `failed-command/native_quality_only` retention, but its stderr
contained only `native quality host exited with 1`: the Native producer had
unconditionally deleted its own private stage, including the desktop host's
stdout/stderr. Neither attempt published a formal output or authorized any
performance statement, and neither was automatically retried.

The current diagnostic repair retains a Native-host failure as an immutable
nested `native-view000001.failed-q1-native-*` tree. It preserves the exact host
command, stdout/stderr and build diagnostics, removes the rebuildable private
Cargo target and any unpublished candidate artifact, and keeps the requested
native output absent. Focused tests exercise the real `collect` failure path
with exactly one host invocation. A new endpoint execution remains deferred
until this repair is committed and independently reviewed at a fixed SHA.

Fixed-SHA review rejected the first diagnostic repair at `3abd05e` with three
P1 gaps: timeout partial streams were not written, the shared build receipt
still serialized its task-local Cargo environment, and strict staging cleanup
could turn an already published success into a command failure. The follow-up
repair records timeout/other subprocess exceptions after exactly one call,
redacts the retained build command to argv only, and makes post-publication
cleanup unable to reverse success. It adds fault-injection coverage for both
exception classes, build failure before host launch, environment redaction and
post-publication cleanup failure. No endpoint is authorized until the new SHA
passes another independent review.

The next authorized run at reviewed SHA `6426c38` stopped before its first
render with the retained host error `surface evidence resize 980x546 violates
979x546`. Native had emitted only the begin receipt; PlayCanvas and the offline
gate did not start. This isolates the failure to AppKit/winit top-level-window
backing conversion: a 979-pixel width at the default 2x backing scale becomes a
half logical point and AppKit reports 980. It is not Truck admission, Metal
capacity, sorting or raster failure.

The narrow correction separates the OS window container from the render
resolution instead of disabling high DPI. The diagnostic event loop freezes
the actual container size reported immediately after creation and rejects any
later resize. Independently, wgpu Metal configures `CAMetalLayer.drawableSize`
from the exact Surface extent, and renderer receipts still require Surface,
presented drawable and capture to be 979x546. No endpoint image is cropped,
resized or resampled, and ordinary interactive rendering is unchanged. A new
endpoint execution remains deferred until this platform correction passes
compile/tests and fixed-SHA review.

The preceding `18131f6` attempt to use winit's macOS
`with_disallow_hidpi(true)` was rejected before another endpoint execution.
Inspection of locked winit 0.30.12 showed that the option only changes the
NSView OpenGL-surface preference after NSWindow creation; it does not control
the `backingScaleFactor` conversion that produced the 980-pixel container.
That commit is therefore superseded by the container/drawable separation above
and is not endpoint evidence.

Fixed-SHA review accepted the replacement `270ba02` with no P0/P1/P2. Its
single authorized endpoint execution passed the odd-width container boundary,
then stopped before presentation with
`live camera projection_matrix[0] mismatch: actual=1.1821657419204712
expected=1.188814234062581`. PlayCanvas and the offline gate did not start and
no formal output was published. The retained failure proves a diagnostic-oracle
drift: the renderer and formal trace both apply Truck's calibrated
`focal_length_x_over_y=1.005624...`, while the desktop live-camera receipt
recomputed projection X with the historical square-pixel `focal/aspect`
formula. The correction uses `CameraIntrinsics::effective_projection_aspect`
in that receipt only and adds the exact 979x546 calibrated projection as a
regression test. Renderer projection, authority inputs and endpoint pixels are
unchanged. Another endpoint remains deferred until this new candidate passes
focused verification and fixed-SHA review.
