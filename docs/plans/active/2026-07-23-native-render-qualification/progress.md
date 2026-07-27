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
