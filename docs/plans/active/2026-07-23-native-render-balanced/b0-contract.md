# B0 Balanced Contract

> Program: [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)
>
> Evidence protocol: [Native Render Core Verification and Benchmark Protocol](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md)
>
> State: candidate contract; B0 remains Active until root review

## 1. Decision and non-goals

Balanced is an all-resident, full-membership research profile for measuring
explicit precision and representation trade-offs. It is not Scalable, LOD,
sampling, a draw budget, dynamic resolution, or a way to make an inadmissible
scene appear successful.

Until B5 qualifies and promotes a named endpoint:

- Exact is the product default and the fallback authority;
- Balanced is private/benchmark opt-in and cannot widen the stable API;
- a Balanced preparation or execution failure publishes no partial frame and
  retains or restores the matching Exact plan;
- no B1--B3 result changes product defaults, public C ABI, or release claims.

B0 freezes the experiment contract only. It is not implementation, runtime,
device, quality, or performance evidence.

## 2. Non-negotiable identity

Every retained Balanced frame proves all of the following before an image or
timing result is admitted:

| Field | Required receipt |
| --- | --- |
| membership | `source = decoded = encoded = resident = addressable`; no source sampling or point-count cap |
| SH | resident SH degree equals source SH degree; every coefficient required by that degree remains addressable, even when one declared B3 encoding quantizes its value |
| resolution | requested, Surface, internal-render and presented dimensions are equal; dynamic resolution and upscaling are disabled |
| render contract | `SortedAlpha`, the pinned covariance/blend/alpha-cutoff contract, and the current complete visibility semantics |
| order identity | one authoritative back-to-front order; source-ID ties remain deterministic within an equal candidate key |
| counts | `0 <= C <= V <= S`; Candidate/PostSort has `D = V`, exact contributor Compact/Preproject has `D = C` |
| lifecycle | scene, camera, viewport, contract, plan and presentation generations match; only a successful primitive presentation may publish evidence |

Camera visibility may make `V < S`; that is not source-membership reduction.
Unavailable `V/C/D`, image fields or timings remain unavailable and invalidate a
strict retained run. They are never replaced with zero, capacity, source count,
or a receipt from another ticket.

## 3. Frozen Balanced image gate

The machine-readable contract name is `gsplat-balanced-image-gate/v1`. B1--B3
must add or extend a repository validator for this exact schema before retaining
their first candidate result; a prose-only report cannot Accept an experiment.
The validator must use the existing Exact RGBA image convention and record its
own version/hash.

Reference and candidate images are RGBA8 outputs from the same complete source,
camera frame, backing dimensions and successful presentation. RGB values are
the stored sRGB bytes. For an image of `W * H` pixels:

- normalized RGB MAE is `sum(abs(candidate-reference)) / (255*3*W*H)`;
- RGB bad-pixel fraction counts a pixel when any RGB byte differs by more than
  `3`;
- normalized alpha MAE is `sum(abs(candidate-reference)) / (255*W*H)`;
- alpha bad-pixel fraction counts a pixel when alpha differs by more than `1`;
- temporal RGB residual MAE compares `(candidate[t]-candidate[t-1])` with
  `(Exact[t]-Exact[t-1])`, normalized by `255*3*W*H`.

Every captured frame and every captured transition must pass; averages cannot
hide an outlier:

| Metric | Balanced v1 gate |
| --- | ---: |
| 8x8-luma sRGB SSIM | `>= 0.99` per frame |
| normalized RGB MAE | `<= 0.005` per frame |
| RGB bad-pixel fraction (`> 3/255`) | `<= 0.02` per frame |
| normalized alpha MAE | `<= 0.001` per frame |
| alpha bad-pixel fraction (`> 1/255`) | `<= 0.005` per frame |
| temporal RGB residual MAE | `<= 0.005` for every adjacent captured pair |
| missing/invalid image or transition | zero allowed |

These are predeclared research-admission bounds, not a claim that Balanced is
pixel-exact or visually indistinguishable. They may be revised only by a new B0
contract revision made before evaluating the affected candidate. A failed
candidate never relaxes the gate that judged it.

## 4. Frozen cameras, data and endpoint scope

The canonical camera source is the checked-in
[camera-trace contract](../../../../tests/perf/trace/README.md). Auto camera,
scene AABB framing, endpoint-local orbit recreation, CSS size, and simulator
dimensions cannot substitute for a trace receipt.

Each B1--B3 candidate uses both modes:

1. **Authored views:** all frames in the scene's checked-in two-view quality
   trace are captured against Exact.
2. **Moving sequence:** frame indices `0,1` repeat with `sort_interval=1`.
   Performance uses 20 warmup and 80 measured frames. Quality captures one
   complete post-warmup `0 -> 1 -> 0` sequence, so both transition directions
   enter the temporal gate.

The common minimum evidence matrix is:

| Layer | Data and camera | Purpose |
| --- | --- | --- |
| contract | deterministic minimal fixtures, boundary/tie/invalid inputs | byte/order/layout semantics; never a real-scene quality claim |
| native quality | complete SH3 Kitsune, Bonsai and Truck, authored and moving, 1920x1080 | small/mid, dense indoor and 2.54M primary scene |
| Web quality | complete SH3 Kitsune and Truck, authored and moving, 1920x1080 | portability behavior on Chrome/WebGPU |
| Android quality | complete SH3 Kitsune and Truck, authored and moving, actual A065 2412x1080 drawable | mobile Vulkan behavior without rescaling |
| large extension | complete Garden and Bicycle authored views where the endpoint admits them | range/capacity sensitivity; short runs are not sustained-performance evidence |

Every external asset must match its existing manifest identity and hash. A
missing asset is Deferred for that scene; a deterministic prefix may locate a
performance crossover but cannot satisfy this quality matrix.

Tier 1 endpoint scopes are Apple M4 native Metal, Chrome/WebGPU on that Mac, and
Nothing A065/Adreno 730/Vulkan. Results are terminal per endpoint. Missing
hardware, authorization, locked tooling, or a qualified asset produces a named
Deferred scope; it does not keep a task running or manufacture evidence. iOS
Simulator is functional evidence only. Physical Apple mobile, x86_64 AVX2/FMA,
discrete Vulkan/DX12 and additional browsers remain Tier 2 promotion evidence.

## 5. One-variable experiment definitions

For every candidate, the task record names one changed field, its Exact
baseline encoding, candidate encoding, expected byte/traffic effect, affected
plans, and forced diagnostic selector before implementation. Everything not
named stays Exact. CPU/GPU ordering is forced separately while qualifying the
representation; Adaptive is tested only after both forced lanes pass.

### B1 — stable depth-key precision

The sole variable is the number of retained depth-key bits.

1. Compare stable 24-bit keys directly with Exact stable full32 keys.
2. Attempt stable 20-bit keys only if 24-bit passes the identity and image
   gates on that scope, even if 24-bit's performance result is Rejected.
3. Both CPU and GPU implementations must share the same quantizer, descending
   key order and source-ID tie rule.
4. Visibility, source data, projected planes, raster, color work and all other
   plan fields remain Exact.

The coarsest quality-admissible key that also supports the declared benefit may
be Accepted for its named endpoint scope. If 24-bit fails an identity or image
gate, 20-bit is not used as a corrective attempt and B1 is Rejected for that
scope. B1 may not change near/far tests, depth math, visibility, radix stability
or source membership.

### B2 — projected/cache numeric precision

The sole variable is one projected numeric plane family. B2 runs separate
sub-experiments:

- `B2-axes16`: projected axis numeric channels use binary16; center, alpha,
  source ID and every Resident plane remain Exact;
- `B2-center16`: projected screen-center numeric channels use binary16; axes,
  alpha, source ID and every Resident plane remain Exact.

Source IDs are never represented as binary16. The two B2 variants are judged
independently and are not combined inside B2; B4 may combine only individually
Accepted variants. Projection math, ordering, contributor semantics, raster
equations, SH evaluation and update cadence stay Exact.

### B3 — Resident/SH/color encoding

The sole variable is the encoding of exactly one semantic Resident attribute
family. Each proposed family is a separate sub-experiment, declared before
code:

- position components, with opacity unchanged;
- world covariance components;
- all coefficients of the source SH degree, with no omitted band/coefficient;
- resolved color representation, with SH evaluation inputs and cadence
  unchanged.

An implementation may choose not to attempt every family. It may not submit a
single "quantized Resident" candidate that changes several families at once,
and it may not combine a B3 result with B1/B2 before B4. The receipt records the
exact format, scale/range metadata, bytes per source, saturation count and
non-finite handling. Saturation, dropped coefficients, lowered SH degree, or a
hidden color-update threshold is a hard failure rather than an allowed quality
trade-off.

## 6. Evidence and performance decision

Every retained candidate artifact contains the identity fields required by the
[benchmark protocol](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md):
clean commit and binary hash, dataset and trace hashes, endpoint/adapter/limits,
full profile and precision contract, actual CPU/GPU/plan identity, exact
membership/SH/resolution/count receipts, raw terminal frames, image/transition
metrics, screenshots and explicit unavailable fields.

The task declares one primary benefit before implementation:

- a timing hypothesis uses at least three interleaved Exact/candidate pairs on
  the complete Truck moving sequence; all retained paired deltas must support
  the same winner, otherwise the result is unclear and Rejected;
- a storage/traffic hypothesis must show the exact declared byte reduction and
  the same paired run must show no consistent terminal regression; a mixed or
  consistently slower result is Rejected unless a new, separately reviewed
  task predeclares a different product trade-off before seeing its output.

FPS, a fixed percentage lead over PlayCanvas, and a complete Tier 2 device
matrix are observations, not correctness or completion gates. One initial
runnable implementation, at most one in-scope corrective measurement/performance
iteration, and one final evidence run are allowed. Ordinary correctness fixes
do not authorize a scope expansion; an unresolved correctness failure Rejects
the candidate.

## 7. Finite terminal states

Each B1--B3 sub-experiment records one terminal state for every claimed
endpoint and one aggregate state with an explicit scope:

- **Accepted:** the single-variable boundary is proven, every required identity
  and Balanced v1 image/temporal gate passes for the named scope, artifacts are
  complete, and the predeclared benefit is supported. The result is only
  eligible input to B4; it is not a default change.
- **Rejected:** any invariant or quality gate fails; evidence is invalid; the
  benefit is unclear after the fixed run; the implementation consistently
  loses its primary metric; or its complexity is not justified. Production
  code is reverted and only the useful experiment record remains.
- **Deferred:** a specific external endpoint, asset, toolchain or upstream
  capability is unavailable before a valid run. No half-enabled code or
  fallback claim remains. Other endpoint scopes still close independently.

There is no fourth "keep tuning" state. B4 receives only Accepted components
and must measure their interactions. B5 alone may decide endpoint opt-in or
promotion, and any default promotion requires reliable Exact fallback plus an
explicit Roadmap/release-boundary change.

## 8. Source decisions

No new external research is required to freeze B0. The accepted inputs are
repository-local and implementation-neutral:

| Source | Accepted use | Explicit rejection |
| --- | --- | --- |
| [program plan](../../completed/2026-07-23-native-render-core-refactor/task_plan.md) | Exact/Balanced/Scalable separation and B1--B3 task boundaries | one combined approximation task or default promotion |
| [benchmark protocol](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md) | evidence classes, endpoint tiers, pairing, identities and Exact oracle | unmatched native/Web timing as an architecture gate |
| [camera trace contract](../../../../tests/perf/trace/README.md) | identical explicit cameras and dimensions across endpoints | auto-framing or endpoint-local camera recreation |
| [completed full-quality findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md) | inherited Exact oracle and historical competitor trade-off context | treating competitor defaults, LOD, sampled points, lower SH or lower resolution as Balanced evidence |

External competitor documentation may motivate a later candidate, but its
numbers never satisfy a local quality, endpoint or implementation gate.
