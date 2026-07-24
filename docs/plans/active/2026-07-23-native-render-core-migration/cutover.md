# Native Render Core Migration Cutover Contract

## Status and authority

- Package: **M — atomic product migration and legacy deletion**.
- Task: **M0 — freeze cutover, rollback, ownership, and evidence**.
- M0 is documentation-only. It changes no Rust, WGSL, public API, C ABI,
  platform wrapper, build script, benchmark protocol, or product default.
- Accepted Exact implementation:
  `d721ea6cd0c334e28d3ad5c28792383524e27935` (`E_IMPL_SHA`).
- Accepted E13 closeout and fixed M0 parent:
  `328e05c4cb55f4824cc9149c08629dd9d2ad5eed` (`E13_CLOSEOUT_SHA`).
- `M0_CUTOVER_SHA` is the reviewed candidate/fix tip for this file and may be
  written into `progress.md` by root. `M0_ACCEPT_SHA == M1_BASE_SHA` is the
  later root-owned closeout/activation commit that marks M0 Accepted and M1
  Active. That commit cannot embed its own SHA: root reports it in the handoff,
  and the M1 closeout records it as M1's `BASE_SHA`. The accepted M0 tree, not
  `E_IMPL_SHA` directly, is the M1 baseline.
- Canonical design authority remains the
  [native render core roadmap](../../completed/2026-07-23-native-render-core-refactor/task_plan.md),
  [architecture](../../completed/2026-07-23-native-render-core-refactor/architecture.md),
  [benchmark protocol](../../completed/2026-07-23-native-render-core-refactor/benchmark_protocol.md),
  [accepted Exact contract](../../completed/2026-07-23-native-render-exact-core/contract.md),
  [oracle](../../completed/2026-07-23-native-render-exact-core/oracle.md), and
  [E13 final report](../../completed/2026-07-23-native-render-exact-core/final-report.md).

M0 freezes how the accepted private Exact core is migrated into products. It
does not claim that any product consumer has already switched. The current
legacy renderer and `SurfaceRenderSession` remain the product default until the
corresponding migration task is accepted.

## Non-negotiable Exact invariants

Every M1--M8 candidate must preserve all of the following. A failure is a
correctness failure, not a performance trade.

1. Source, decoded, encoded, resident, addressable, and declared source counts
   remain equal for the selected scene. No point sampling, draw budget, hidden
   subset, or partial publication is allowed.
2. Source SH degree 0--3 is retained. A consumer may not silently drop SH
   bands, clamp the degree, or substitute a lower-quality representation.
3. Ordering uses the accepted stable full-32-bit depth key with deterministic
   source-ID tie order. CPU, GPU, and Adaptive are execution choices only.
4. Requested, Surface, internal-render, and presented dimensions are equal.
   Dynamic resolution and upscaling remain disabled for retained Exact
   evidence.
5. Rendering uses the canonical `SortedAlpha` contract. A candidate may not
   change blend semantics, raster support, camera, clear color, or comparison
   oracle as part of migration.
6. `PlanId::CpuPostSort`, `PlanId::GpuPostSort`, and
   `PlanId::GpuPreproject` are complete prepared Exact plans. Adaptive compares
   whole plans from comparable terminal samples; it never selects by model
   point-count threshold.
7. Fallback is prepared and same-Exact. Failure may retain the prior published
   runtime or choose an eligible Exact plan; it may not enable LOD, sampling,
   Paged auto-selection, lower SH, lower resolution, or another approximation.
8. `Renderer` remains the sole owner of plan eligibility, controller decisions,
   semantic generations, mandatory sampling, terminal results, and published
   frame state. Hosts and wrappers own lifecycle and translation only.
9. Telemetry reports policy and results; it never becomes a second policy
   owner. Missing or stale evidence fails closed for a retained run.
10. Performance values are observations used to choose eligibility and
    defaults on proved scopes. FPS, a fixed speedup, or beating a competitor is
    not an acceptance gate for Package M.

## Integration and rollback protocol

### One-way order

Production implementation is serial:

```text
M0 -> M1 -> M2 -> M3 -> M4 -> M5 -> M6 -> M7 -> M8
```

Read-only audits may run ahead, but no later production candidate may be based
on an unaccepted predecessor. M3--M6 audits may begin after the M2 seam is
frozen; their implementation still integrates in the order above. M7 may keep
a deletion inventory current, and M8 may keep a stale-document inventory, but
neither edits production ownership before its turn.

### Per-task ledger

Root records these fields for every accepted migration task:

```text
BASE_SHA=<accepted predecessor tree>
CUTOVER_SHA=<accepted behavior/consumer cutover commit>
CLOSEOUT_SHA=<optional ledger-only closeout commit>
OWNED_PATHS=<exact path list>
VERIFY=<commands and exit statuses>
ARTIFACTS=<retained paths plus identity hashes>
DEFER=<explicit unavailable or out-of-scope items>
```

`OWNED_PATHS` is the exact union of paths changed by `CUTOVER_SHA` and the
optional `CLOSEOUT_SHA`; it is not merely the mutable implementation writer's
scope. The mutable writer owns the cutover candidate. Root alone owns the
ledger closeout/next-task activation commit. The cutover and ledger closeout
remain separate commits. A task candidate that is not integrated ends as
Reject or Defer and requires no repository revert.

### Revert procedure

Before downstream work begins, an accepted task can be rolled back without
rewriting history:

```bash
git revert --no-edit <CLOSEOUT_SHA> # omit when there is no closeout commit
git revert --no-edit <CUTOVER_SHA>
git diff --exit-code <BASE_SHA>..HEAD -- <OWNED_PATHS...>
```

If downstream tasks already exist, revert every integrated task in reverse
order, `M8 -> M7 -> M6 -> M5 -> M4 -> M3 -> M2 -> M1`, stopping at the target
dependency. Revert each task's closeout before its cutover. Do not use a
destructive reset. A
rollback is complete only when the owned-path diff is empty, repository checks
pass, and the previous product route is again the only state owner.

## Frozen qualification inputs

### Dataset contract

The shared retained scene is Kitsune, declared by
[`tests/perf/datasets/kitsune.json`](../../../../tests/perf/datasets/kitsune.json):

| Field | Committed manifest value |
| --- | --- |
| Dataset ID | `kitsune` |
| PLY path | `tests/datasets/external/wakufactory_kitune/kitune1.ply` |
| Manifest-declared PLY SHA-256 | `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2` |
| Bytes | `65,892,441` |
| Splats | `279,199` |
| SH degree | `3` |
| Fetch/validation entry | `tests/datasets/fetch-wakufactory-kitune.sh` |

The PLY is not committed and was not present for M0. The values above are the
committed manifest contract, not a fresh asset hash claim. Before retaining
M1, M2, M4, M5, or M6 scene evidence, that task must fetch or locate the asset
and revalidate it with the repository dataset tooling.

### Camera traces and hash semantics

Each trace carries a canonical JSON `content_sha256`. That value is distinct
from the raw bytes of the trace file. Both identities are frozen so a tool
cannot accidentally compare one hash domain with the other.

| Endpoint | Trace | Raw file SHA-256 | Canonical JSON `content_sha256` |
| --- | --- | --- | --- |
| Desktop/Web | [`candidate-kitsune-quality-1920x1080-v1.json`](../../../../tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json) | `c996e5fe757d9d6661cce9f1dc303edcfbe8e12059e08bf8ab9f8eb566e54657` | `8821c193506cdf7d67aa200248a45088c4750a3cd128ee29f6e2dc2d3a5bdb99` |
| Android A065 | [`candidate-kitsune-quality-2412x1080-v1.json`](../../../../tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2412x1080-v1.json) | `f9a8316369966928e63816141ef61f3b38af8758cf307f94673e3a234a7daf44` | `0775baeda60585a2668a415c63702b0c51c6efc643f1779b0ac5e3c82067f56a` |
| Apple simulator | [`candidate-kitsune-quality-2622x1206-v1.json`](../../../../tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2622x1206-v1.json) | `8b74bf8c4123e1d64907ab45cddb721c3a871ec0a31173a7ff781562b9c9e775` | `18bacc60328fb113ae3f9cc928428d300d98617de22f4c7b178595a187b64f2b` |

The canonical trace validator must pass before a trace is used in retained
evidence. A trace identity includes its endpoint dimensions, frame sequence,
camera matrices/intrinsics, raw file hash, and canonical content hash.

## Retained artifact contract

Use the existing `gsplat-benchmark/v1` artifact contract and repository
validator for render evidence. Do not introduce an M-only evidence schema.
Every retained run records, where applicable:

- exact commit and dirty state;
- executable, native library, app, APK, AAR, XCFramework, package, or WASM
  content hash;
- freshly validated dataset and trace identities;
- adapter, backend, device, OS, browser/driver, and capability limits;
- requested and actual Exact profile, actual complete `PlanId`, actual order
  lane/kernel, and any same-Exact fallback;
- source, decoded, encoded, resident, and addressable counts, plus
  source/visible/contributor/drawn (`S/V/C/D`) counts; every unavailable field
  has an explicit reason;
- requested/Surface/internal/presented resolution receipt;
- terminal success or failure for each issued ticket, joined by ticket and
  semantic generation;
- raw frame records, summary, final image where supported, and explicit
  unavailable fields where capture is not exposed;
- exact commands, exit statuses, exclusions, rollback identities, and the
  artifact validator result.

Compilation proves compilation only. A simulator proves simulator function
only. Injected presentation proves the injected transaction only. None of
these substitutes for a real OS-window or physical-device claim. A missing
physical iPhone narrows M6 evidence and remains explicit; it does not turn
simulator timing into device performance.

## Package checklist

### M0 — Freeze cutover and rollback

**Mutable writer:** M0 cutover-document owner; documentation only.

**Root ledger owner:** migration root, which alone updates `progress.md` and
activates M1 after independent review.

**Owned paths across both M0 commits:** this `cutover.md` in the cutover commit
plus root-owned `progress.md` in the closeout/activation commit. The two writers
and commits remain separate.

**Exit:**

- fixed E implementation and E13 closeout identities are named;
- Exact invariants, serial task order, owner boundaries, rollback records, and
  M1--M8 artifacts are complete;
- dataset and trace hash domains are unambiguous;
- M0 document links, architecture policy, and diff hygiene pass;
- root records `M0_CUTOVER_SHA` in `progress.md`; the resulting
  `M0_ACCEPT_SHA == M1_BASE_SHA` is reported in the handoff and later recorded
  by M1 as its base identity.

M0 does not run or claim a renderer benchmark.

### M1 — Native offscreen, desktop, and bench-runner

**Unique owner:** native non-Surface host cutover. `Renderer` owns all render
semantics; hosts own CLI/windowless lifecycle, input/output, and artifact I/O.

**Owned paths:**

- `crates/gsplat-render-wgpu/src/renderer/**` and `offscreen/**` as required;
- `crates/gsplat-render-wgpu/src/lib.rs` only as a compatibility facade;
- `examples/desktop/src/**` for non-interactive/offscreen entrypoints;
- `tools/bench-runner/src/**`;
- focused tests owned by those consumers.

**Frozen boundary:** the interactive shared Surface remains legacy until M2.
M1 changes no public API, C ABI, Web, Android, Apple wrapper, or product-wide
default. Desktop CLI, viewer, and benchmark responsibilities must remain
separate; no host-side controller is introduced. The current bench-runner has
no camera-trace CLI and its default dimensions are not the frozen 1920x1080
Kitsune workload. Its existing minimal artifact is smoke only. M1 must extend
bench-runner or add a dedicated offscreen collector that consumes the frozen
trace, proves the Exact receipts, writes a final frame, and publishes the
canonical artifact before M1 can exit.

**Required exit artifacts:**

- native offscreen Direct-oracle image parity at fixed scene/camera/resolution;
- Exact count/SH/order/resolution receipts and focused CPU PostSort, GPU
  PostSort, and GPU Preproject eligibility/fallback tests;
- one validated formal `gsplat-benchmark/v1` offscreen artifact at the frozen
  Kitsune 1920x1080 trace, including final-frame identity; M1 owns this
  offscreen artifact and M2 owns all real-Surface artifacts;
- clean ownership proof that native non-Surface consumers no longer write
  renderer generations, controller decisions, mandatory samples, or results.

**Rollback base:** accepted M0 tree.

### M2 — Shared real-window Surface

**Unique owner:** the shared Surface transaction. Lifecycle remains the sole
owner of acquire/configure/present; `Renderer` owns Exact prepare/encode/
publish semantics. The old Surface owner must stop writing semantic state when
M2 is accepted.

**Owned paths:**

- `crates/gsplat-render-wgpu/src/surface/**`;
- renderer Surface transaction seam;
- `surface_presenter.rs`, `surface_session.rs`, and the `lib.rs` facade;
- focused Surface integration tests.

**Frozen boundary:** no C ABI, Web, Android, or Apple consumer cutover. Capture
becomes complete only after a successful real presentation. Surface loss or
failed presentation must leave the transaction retryable and must not publish
false terminal evidence.

**Required exit artifacts:**

- real OS-window Apple M4/Metal Surface runs for forced CPU PostSort, forced
  GPU PostSort, forced GPU Preproject, and Adaptive;
- exact Kitsune trace, full resolution, all points/SH3, canonical SortedAlpha,
  terminal tickets, counts, and final-frame identity;
- acquire/configure/submit/present ordering and surface-loss retry tests;
- proof that the accepted E12 injected-presentation test still passes, while
  being labelled insufficient by itself for the M2 real-window exit.

The existing desktop real-window command is a smoke/log entrypoint, not a
canonical artifact collector. The specialized
`tests/perf/collect-desktop-producer-ab.py` covers only the forced GPU producer
A/B. M2 must extend it or add a general shared-Surface collector/capture seam
for forced CPU PostSort, forced GPU PostSort, forced GPU Preproject, and
Adaptive. M2 cannot exit until that seam emits validated canonical artifacts
and a final frame for every required arm.

**Rollback base:** accepted M1 tree.

### M3 — Stable C ABI compatibility cutover

**Unique owner:** thin C ABI translation and compatibility. The C layer may
translate stable v0.1 inputs/outputs but may not own policy, generations,
sampling, plan caches, or terminal result state.

**Owned paths:**

- `crates/gsplat-ffi-c/src/**`;
- `crates/gsplat-ffi-c/include/gsplat.h` only when compatibility requires it;
- `crates/gsplat-ffi-c/README.md` and `tests/ffi/**`.

**Frozen boundary:** keep the published v0.1 ABI small and stable. Existing
symbols remain thin shims unless a separately reviewed compatibility decision
allows removal. Platform consumer edits remain M5/M6.

**Required exit artifacts:**

- Rust/header layout, size, alignment, enum/flag, symbol, and version agreement;
- C smoke plus JNI and Swift compatibility smoke against the same native core;
- invalid pointer/version/size and unavailable-evidence behavior fail closed;
- no duplicate ABI-side renderer controller or state cache.

**Rollback base:** accepted M2 tree.

### M4 — Browser WebGPU/WASM consumer

**Unique owner:** Web input, browser lifecycle, JS/WASM translation, package,
and artifact collection. It does not own a Web-specific render controller.

**Owned paths:**

- `crates/gsplat-web/**`;
- `examples/web/**`;
- `packages/web/**`.

**Frozen boundary:** WebGPU uses the shared Exact renderer. Any WebGL preview
remains explicitly outside retained Exact evidence. No silent fallback, point
budget, SH drop, CSS/backing-resolution mismatch, or host policy is accepted.

**Required exit artifacts:**

- wasm32 compile, reproducible WASM/package build, JS checks/tests, and package
  dry run;
- real Chrome WebGPU/WASM run with complete Kitsune membership/SH3, exact
  backing resolution, trace identity, actual plan/order receipts, terminal
  evidence, final frame, and package/WASM hashes;
- validated `gsplat-benchmark/v1` artifact or explicit unsupported fields;
- browser capability failure is explicit and never misreported as a passed
  WebGL or compilation result.

The generic minimal collector invocation is smoke only. Formal M4 exit also
requires the existing full-quality suite validator to prove the frozen
resolution and Exact receipt; passing only the generic benchmark-v1 validator
is insufficient.

**Rollback base:** accepted M3 tree.

### M5 — Android/JNI/AAR consumer

**Unique owner:** Android lifecycle, JNI translation, app UI, packaging, and
device artifact collection. Kotlin/Java must not implement render policy.

**Owned paths:**

- `bindings/android/**`;
- `examples/android/**`.

**Frozen boundary:** one serial device run at a time. CPU/GPU/Adaptive are
actual complete-plan choices from the shared renderer. No point-count rule,
sampling, reduced SH, reduced resolution, or Paged fallback is permitted.

**Required exit artifacts:**

- JNI smoke, AAR, sample APK, and Android unit tests;
- Nothing A065 serial `033ed212` physical-device evidence using the 2412×1080
  Kitsune trace for forced CPU, forced GPU, and Adaptive;
- exact app/APK/AAR/native `.so`, dataset, trace, device, OS, Vulkan adapter,
  resolution, plan, terminal-ticket, source/decoded/encoded/resident/addressable,
  S/V/C/D, and SH receipts;
- thermal state and unavailable timing fields are reported honestly; a device
  disconnect is Defer, not simulated evidence.

**Rollback base:** accepted M4 tree.

### M6 — Apple/GsplatKit/XCFramework consumer

**Unique owner:** Swift translation, Apple lifecycle, package/XCFramework,
sample UI, and Apple artifact collection. Swift owns no render policy.

**Owned paths:**

- `bindings/apple/**`;
- `examples/ios/**`.

**Frozen boundary:** the shared renderer owns plan selection and evidence.
Simulator evidence is functional only. Physical iPhone qualification is used
when a device/signing environment is available and is otherwise an explicit
Defer, not a blocker for the functional consumer migration.

**Required exit artifacts:**

- Swift smoke, XCFramework build, Swift package description, generic iOS
  Simulator build, and simulator app/smoke;
- exact struct/symbol/translation compatibility through the M3 ABI;
- simulator scene/lifecycle/count/SH/resolution evidence labelled functional;
- when available, physical iPhone full-resolution CPU/GPU/Adaptive evidence.
  M6 first probes the actual drawable; if it differs from 2622×1206, M6 derives
  and validates a same-camera-family trace for that drawable rather than
  reusing the simulator dimensions. The artifact retains app/native/framework,
  device, raw-trace, and canonical-trace identities.

The existing simulator scripts build and launch but do not by themselves
retain the complete console artifact. M6 must add or extend an Apple-owned
simulator wrapper that waits for benchmark completion, captures the log, and
passes it to the existing `extract-ios-benchmark-artifacts.py`. If that seam is
not present, a successful app launch remains smoke and M6 cannot exit.

**Rollback base:** accepted M5 tree.

### M7 — Delete legacy owners and obsolete experiments

**Unique owner:** removal of superseded renderer/session/presenter and
experiment ownership after every consumer is on the shared core.

**Owned paths:**

- legacy renderer/session/presenter and `lib.rs` compatibility owners;
- obsolete tiled/diagnostic/experiment owners identified by the accepted M7
  inventory;
- architecture ownership policy and focused deletion tests.

**Frozen boundary:** M7 deletes duplicate semantic ownership, not published ABI
compatibility. Any still-published C symbol remains an M3-owned thin shim.
Diagnostic code survives only when it has a named owner, use, and verification;
otherwise it is removed rather than becoming a dormant second architecture.

**Required exit artifacts:**

- source-policy proof that there is one controller, generation owner,
  mandatory sampler, plan cache owner, and terminal-result owner;
- call-site/API/ABI classification for every deleted or retained entrypoint;
- no legacy/new runtime toggle or dual product default;
- workspace, platform, architecture, FFI, WASM, and forced Metal conformance
  verification after deletion;
- rollback by reverting M7 restores the last accepted compatibility tree.

**Rollback base:** accepted M6 tree.

### M8 — Documentation, release alignment, and closeout

**Unique owner:** factual repository documentation and the final migration
ledger. M8 adds no renderer behavior.

**Owned paths:**

- `handbook/PROJECT_CONTEXT.md`, `handbook/ARCHITECTURE.md`,
  `handbook/VERIFICATION.md`, `handbook/ROADMAP.md`, and
  `handbook/GOLDEN_PRINCIPLES.md` where facts changed;
- `RELEASING.md` only when release facts changed;
- this active migration bundle, moved to completed on acceptance;
- final architecture/deprecation ledger and closeout report.

**Frozen boundary:** docs describe only implemented behavior and retained
evidence. M8 does not automatically activate Balanced, Streamed, or competitor
qualification programs. The existing `IO-PLY-1` and `IO-SPZ-1` records receive
an explicit owner/follow-up decision rather than being silently declared
migrated.

**Required exit artifacts:**

- handbook, roadmap, verification, release, public API, and examples agree;
- active/completed links and commands resolve;
- final clean-worktree verification matrix and exact accepted SHAs are recorded;
- open limitations and deferred physical endpoints are explicit;
- the migration package is archived only after M7 deletion and M8 factual
  review are accepted.

**Rollback base:** accepted M7 tree.

## Verification command catalog

Commands below already exist in the repository. Each task first runs its
focused commands, then the applicable global matrix before acceptance.

### M0

```bash
PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py
PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py
git show --check "$M0_CUTOVER_SHA"
git diff --check "$M0_BASE_SHA..$M0_CUTOVER_SHA"
```

M0 also runs a read-only local Markdown relative-link checker over this file.
After root creates the closeout/activation commit, it runs
`git diff --check "$M0_BASE_SHA..$M0_ACCEPT_SHA"`. An unscoped
`git diff --check` on a clean tree is not candidate evidence because it ignores
already committed changes.

### Global migration matrix

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha
PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py
PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py
bash tests/security/run-cargo-deny.sh
git show --check "$CUTOVER_SHA"
git diff --check "$BASE_SHA..$CUTOVER_SHA"
# When CLOSEOUT_SHA exists:
git diff --check "$BASE_SHA..$CLOSEOUT_SHA"
```

### M1 focused

```bash
cargo test -p gsplat-render-wgpu --lib
cargo test -p desktop-example
cargo test -p bench-runner
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 \
  --warmup-iterations 10 \
  --artifact-dir target/benchmarks/migration-m1-minimal \
  --series-id migration-m1 --run-id migration-m1 \
  --frame-budget-ms 16.6666667 --refresh-hz 60
python3 tests/perf/validate-benchmark-artifacts.py \
  target/benchmarks/migration-m1-minimal
```

Those commands are smoke only. The current offscreen executable does accept the
frozen workload and final-frame output:

```bash
mkdir -p target/benchmarks/migration-m1-kitsune-offscreen
cargo run --release -p desktop-example -- \
  tests/datasets/external/wakufactory_kitune/kitune1.ply \
  --geometry-path packed \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json \
  --camera-sequence --camera-frame-indices 0,1 \
  --camera-warmup-frames 20 --camera-measured-frames 80 \
  --camera-loops 1 \
  --png target/benchmarks/migration-m1-kitsune-offscreen/final-frame.png
```

This freezes the formal offscreen workload but does not yet publish the
required canonical artifact. M1 first supplies the missing bench-runner or
offscreen-collector trace/1920x1080/Exact-receipt seam, commits its real
invocation in the owned tests or documentation, and retains that exact command
with the artifact. The minimal artifact cannot substitute for it.

### M2 focused

The documented real-Surface smoke/log entrypoint is:

```bash
cargo run --release -p desktop-example --features interactive-viewer -- \
  tests/datasets/external/wakufactory_kitune/kitune1.ply \
  --geometry-path packed --interactive \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json \
  --camera-sequence --camera-warmup-frames 20 \
  --camera-measured-frames 80 --camera-loops 1 \
  --surface-benchmark-mode throughput \
  --order-backend adaptive
```

M2 repeats the documented entrypoint with `--order-backend cpu` and
`--order-backend gpu`, and uses the documented GPU producer controls for
PostSort and Preproject. This command alone is not an artifact. M2 first
supplies the general shared-Surface collector/capture seam described above,
then retains its exact forced CPU PostSort, GPU PostSort, GPU Preproject, and
Adaptive invocations, canonical artifacts, and final frames. M1's offscreen
artifact cannot satisfy M2.

### M3 focused

```bash
bash tests/ffi/run-ffi-smoke.sh
bash bindings/android/scripts/run-jni-smoke.sh
bash bindings/apple/scripts/run-swift-smoke.sh
```

### M4 focused

```bash
cargo check -p gsplat-web --target wasm32-unknown-unknown
bash packages/web/scripts/build-wasm.sh
bash packages/web/scripts/build.sh
node --check packages/web/dist/index.js
npm --prefix packages/web run check
npm --prefix packages/web test
npm --prefix packages/web run pack:dry-run
```

The current generic command below is smoke only:

```bash
GSPLAT_DATASET=kitsune \
GSPLAT_ARTIFACT_DIR=target/benchmarks/migration-m4-web-kitsune-smoke \
  node examples/web/scripts/collect-web-benchmark-artifact.mjs
python3 tests/perf/validate-benchmark-artifacts.py \
  target/benchmarks/migration-m4-web-kitsune-smoke
```

The formal current collector invocation is:

```bash
GSPLAT_PHASE_E_QUALIFICATION=kitsune-static-v1 \
GSPLAT_DATASET=kitsune \
GSPLAT_GEOMETRY_PATH=packed \
GSPLAT_ORDER_BACKEND=adaptive \
GSPLAT_PROJECTED_POLICY=adaptive \
GSPLAT_SORT_INTERVAL=1 \
GSPLAT_ORDER_COMPLETION_PROTOCOL=sustained_window \
GSPLAT_CAMERA_TRACE_URL=/tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json \
GSPLAT_CAMERA_TRACE_SEQUENCE=1 \
GSPLAT_CAMERA_FRAME_INDICES=0,1 \
GSPLAT_CAMERA_TRACE_LOOPS=1 \
GSPLAT_BENCHMARK_WARMUP_FRAMES=20 \
GSPLAT_BENCHMARK_FRAMES=80 \
GSPLAT_ARTIFACT_DIR=target/benchmarks/migration-m4-web-kitsune/run-adaptive \
  node examples/web/scripts/collect-web-benchmark-artifact.mjs
python3 tests/perf/validate-benchmark-artifacts.py \
  target/benchmarks/migration-m4-web-kitsune/run-adaptive
```

M4 writes a task-local `suite.json` using the existing
`gsplat-full-quality-experiment/v1` contract and references that run, then
requires:

```bash
python3 tests/perf/validate-full-quality-experiment.py \
  target/benchmarks/migration-m4-web-kitsune/suite.json --verify-inputs
```

The same frozen trace and counts are used for forced plan/lane checks; M4
records each exact environment rather than inferring it from the Adaptive run.

### M5 focused

```bash
bash bindings/android/scripts/run-jni-smoke.sh
bash bindings/android/scripts/build-aar.sh
bash bindings/android/scripts/build-sample-apk.sh
GRADLE_BIN="$(bindings/android/scripts/ensure-gradle.sh)"
"$GRADLE_BIN" -p bindings/android :sample-app:testDebugUnitTest
python3 bindings/android/scripts/collect-android-sort-benchmarks.py \
  --serial 033ed212 \
  --ply tests/datasets/external/wakufactory_kitune/kitune1.ply \
  --backend cpu --backend gpu --backend adaptive \
  --repetitions 1 --sort-interval 1 \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2412x1080-v1.json \
  --camera-frame-indices 0,1 --geometry-path packed \
  --output target/android-sort-benchmarks/migration-m5-a065-kitsune
```

### M6 focused

```bash
bash bindings/apple/scripts/run-swift-smoke.sh
bash bindings/apple/scripts/build-xcframework.sh
(cd bindings/apple/GsplatKit && swift package describe --type json)
(cd bindings/apple/GsplatKit && \
  xcodebuild -scheme GsplatKit \
    -destination 'generic/platform=iOS Simulator' build)
bash bindings/apple/scripts/run-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-smoke.sh
```

All commands above are build/launch smoke. The frozen formal simulator launch
payload uses the actual documented arguments and bundled 2622x1206 trace:

```bash
GSPLAT_CAMERA_TRACE_PATH=tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2622x1206-v1.json \
  bash bindings/apple/scripts/run-ios-sim-app.sh \
    tests/datasets/external/wakufactory_kitune/kitune1.ply -- \
    --gsplat_benchmark true \
    --gsplat_benchmark_frames 80 \
    --gsplat_benchmark_warmup_frames 20 \
    --gsplat_camera_trace camera_trace.json \
    --gsplat_camera_trace_sequence true \
    --gsplat_camera_frame_indices 0,1 \
    --gsplat_camera_trace_loops 1 \
    --gsplat_surface_sort_interval 1 \
    --gsplat_surface_order_backend adaptive \
    --gsplat_surface_projected_policy adaptive \
    --gsplat_geometry_path packed
```

Because `run-ios-sim-app.sh` returns after launch, that invocation remains
smoke until the M6-owned wrapper captures a complete simulator console log.
The wrapper must then invoke the existing extractor and full-quality validator:

```bash
python3 bindings/apple/scripts/extract-ios-benchmark-artifacts.py \
  "$SIMULATOR_LOG" target/benchmarks/migration-m6-ios-sim-kitsune/run-adaptive \
  --validator tests/perf/validate-benchmark-artifacts.py
python3 tests/perf/validate-full-quality-experiment.py \
  target/benchmarks/migration-m6-ios-sim-kitsune/suite.json --verify-inputs
```

M6 repeats the retained wrapper for the required CPU/GPU/Adaptive coverage,
records the exact simulator UUID/drawable, and rejects an unsupported forced
GPU arm explicitly rather than substituting another lane.

Physical-device execution uses the existing
`bindings/apple/scripts/benchmark-ios-device-app.sh` support after M6 inspects
and freezes the available device/signing environment. M0 does not invent a
device identifier or claim device availability.

### M7 and M8 focused

M7 runs the global matrix plus every M1--M6 consumer smoke/build path affected
by a deletion. M8 reruns link/command validation, the global matrix, and the
final platform evidence validators needed by the factual claims it publishes.

## Activation decisions

- **Accept M0** only when this file is the sole M0 change, its links resolve,
  architecture policy/self-tests and diff hygiene pass, independent review
  finds no missing owner or artifact, and root records `M0_CUTOVER_SHA` in the
  separate closeout/activation commit.
- **Reject M0** when it changes product behavior, weakens Exactness, invents a
  command/schema, or leaves ambiguous ownership/rollback.
- **Defer M0** only for a concrete missing fact that blocks a safe M1 cutover;
  performance results and unavailable later physical endpoints do not block
  this documentation freeze.

Root's closeout/activation commit simultaneously makes M0 Accepted and M1
Active; its resulting SHA is `M0_ACCEPT_SHA == M1_BASE_SHA`, reported in the
handoff because it cannot name itself in its tree. No other production
migration task is active until its predecessor closes.
