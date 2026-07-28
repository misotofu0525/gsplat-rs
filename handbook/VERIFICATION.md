# gsplat-rs Verification

## Purpose

- This file defines the canonical verification paths for the repository.
- Prefer these repo-local commands and scripts over ad-hoc command sequences.

## Verification Bootstrap

Run the repository doctor before an expensive platform build or device test:

```bash
python3 tests/verification_bootstrap.py doctor
```

The default report covers host-only Android build, macOS/Metal, Web/WebGPU,
Swift smoke, and XCFramework prerequisites. It is read-only: it may query tool
versions and installation prefixes, but it never builds, starts Chrome, calls
`adb`/`simctl`, installs a package, or changes shell configuration. Device
profiles are opt-in:

```bash
python3 tests/verification_bootstrap.py doctor --profile android-a065
python3 tests/verification_bootstrap.py doctor --profile android-a065-q3-simd
python3 tests/verification_bootstrap.py doctor --profile ios-simulator
```

Use `command` to print the exact existing repository entrypoint and derived
environment without running it. Use `run` only after the profile is READY:

```bash
python3 tests/verification_bootstrap.py command android-build
python3 tests/verification_bootstrap.py run android-build

python3 tests/verification_bootstrap.py command macos-metal
python3 tests/verification_bootstrap.py run macos-metal

python3 tests/verification_bootstrap.py command web-webgpu
python3 tests/verification_bootstrap.py run web-webgpu

python3 tests/verification_bootstrap.py command apple-host
python3 tests/verification_bootstrap.py run apple-host

python3 tests/verification_bootstrap.py command apple-xcframework
python3 tests/verification_bootstrap.py run apple-xcframework
```

`run` delegates to the scripts documented below; the bootstrap does not
duplicate their build, collection, or artifact-validation logic. It also does
not install missing prerequisites. Follow the reported remedy explicitly, then
rerun `doctor`.

### Discovery and overrides

- Android SDK: `ANDROID_SDK_ROOT`, then `ANDROID_HOME`, the standard macOS SDK
  directory, Homebrew's `share/android-commandlinetools` below the main
  `brew --prefix`, the `android-commandlinetools` formula prefix, then the SDK
  inferred from an `adb` executable under `platform-tools` on `PATH`.
- Java: `JAVA_HOME`, macOS `java_home -v 21`, Homebrew `openjdk@21`, then Java
  on `PATH`. Android profiles require major version 21 and JNI headers.
- Android components: NDK `29.0.14206865`, platform `android-35`, Build Tools
  `35.0.0`, platform-tools, and Rust target `aarch64-linux-android`.
- Web: Rust target `wasm32-unknown-unknown`, the exact `wasm-bindgen-cli`
  version in `Cargo.lock` (discovered on `PATH`, then in `$CARGO_HOME/bin`, or
  `~/.cargo/bin` when `CARGO_HOME` is unset), Node/npm, Chrome/Chromium, and the
  pinned `puppeteer-core` install under `tests/competitive/playcanvas`. Set
  `CHROME_PATH` when automatic Chrome discovery is not appropriate.
- Apple: macOS, Swift, the profile-specific Xcode tools, and the Rust targets
  needed by the XCFramework or simulator profile.

The Homebrew setup proven on Apple Silicon is discovered rather than embedded
as a required machine path: the SDK currently resolves to
`/opt/homebrew/share/android-commandlinetools` and JDK 21 to
`/opt/homebrew/opt/openjdk@21`. Intel Homebrew, a standard Android Studio SDK,
or explicit environment overrides follow the same profile contract.

### Explicit device runs

The short A065 profile reuses the Android collector with one full-quality
Kitsune Packed/CPU run, exact trace, native Surface PNG, and canonical artifact
validation. It is a functionality/ledger check, not a CPU/GPU comparison. The
serial and any non-default asset location are explicit:

```bash
GSPLAT_ANDROID_SERIAL=<adb-serial> \
GSPLAT_ANDROID_DATASET=/absolute/path/to/kitune1.ply \
python3 tests/verification_bootstrap.py command android-a065

GSPLAT_ANDROID_SERIAL=<adb-serial> \
GSPLAT_ANDROID_DATASET=/absolute/path/to/kitune1.ply \
python3 tests/verification_bootstrap.py run android-a065 --allow-device
```

The iOS simulator profile similarly requires an already selected simulator UUID
and the qualified Kitsune asset. Doctor/command do not boot or inspect it:

```bash
IOS_SIMULATOR_ID=<simulator-uuid> \
GSPLAT_IOS_DATASET=/absolute/path/to/kitune1.ply \
python3 tests/verification_bootstrap.py command ios-simulator

IOS_SIMULATOR_ID=<simulator-uuid> \
GSPLAT_IOS_DATASET=/absolute/path/to/kitune1.ply \
python3 tests/verification_bootstrap.py run ios-simulator --allow-device
```

The Q3 A065 SIMD entry is the single final qualification invocation. Doctor and
command are read-only: they verify the pinned SDK/NDK/Rust target, all canonical
Truck ladder inputs, the trace, a fresh output, and an explicit full commit SHA;
they never call `adb`. `run` requires `--allow-device` and delegates exactly once
to the Q3 collector (no automatic retry):

```bash
GSPLAT_ANDROID_SERIAL=<adb-serial> \
GSPLAT_Q3_A065_EXPECTED_COMMIT=<full-40-character-sha> \
GSPLAT_Q3_A065_OUTPUT=target/qualification/q3-a065-simd-<sha>-attempt-1 \
python3 tests/verification_bootstrap.py doctor --profile android-a065-q3-simd

GSPLAT_ANDROID_SERIAL=<adb-serial> \
GSPLAT_Q3_A065_EXPECTED_COMMIT=<full-40-character-sha> \
GSPLAT_Q3_A065_OUTPUT=target/qualification/q3-a065-simd-<sha>-attempt-1 \
python3 tests/verification_bootstrap.py command android-a065-q3-simd

GSPLAT_ANDROID_SERIAL=<adb-serial> \
GSPLAT_Q3_A065_EXPECTED_COMMIT=<full-40-character-sha> \
GSPLAT_Q3_A065_OUTPUT=target/qualification/q3-a065-simd-<sha>-attempt-1 \
python3 tests/verification_bootstrap.py run android-a065-q3-simd --allow-device
```

Immediately after `run --allow-device` enters the Q3 collector, and before it
creates a staging/output claim, builds, installs, or launches the Activity, one
bounded real-window readiness transaction reads the selected device's power,
display, and keyguard state. If necessary it sends at most one
`KEYCODE_WAKEUP` and one standard `wm dismiss-keyguard`, then observes state one
final time. It never enters credentials, swipes, bypasses a secure lock, polls,
or retries. If the display is still not awake/on or the keyguard is still
showing, the collector exits with a precise pre-launch
`EnvironmentPrerequisite`; manually unlock the device before another explicitly
authorized one-shot invocation. Because this failure precedes the staging
claim, the selected fresh output path remains unused. `doctor` and `command`
retain their zero-ADB contract.

The collector first requires a physical A065 AArch64 receipt proving Scalar and
Neon element parity for keys, source IDs, NaN bits, boundary bits, FMA-derived
keys, and stable ties, with both kernels executed. Only then may it run a short
Scalar/Neon image/count control plus one diagnostic pair per point-ladder tier.
Those cells cannot promote a plan. Complete Truck alone receives five matched,
counterbalanced terminal pairs. Timing runs do not repeat PNG capture. A
diagnostic performance rejection continues the later point ladder and does not
replace the complete-Truck terminal. Only an explicit capacity, admission, or
resource-range receipt from the renderer scene-admission producer, schema-bound
to the post-launch Activity phase, exact dataset ID/hash/splat count, and the
exact host-issued collector run identity, may scope out larger workloads. The
current Android product does not publish that receipt, so generic log text,
OOMs, PNG/artifact failures, and collector or ledger errors remain cell-local
integrity `Rejected` without range inference; the collector continues the
predeclared later ladder and complete-Truck cells, but the matrix remains
sticky `Rejected` even if that later terminal is otherwise Accepted. Only a
failure that prevents the matrix from maintaining its frozen repository,
device, installed-APK, or prepared-input identity terminates the matrix as
`Rejected`, while a proven pre-launch environment prerequisite is `Deferred`.
The trace is pushed and copied once for the matrix, each PLY is pushed and
copied once per workload, and the one-shot correctness/timing commands reuse
hash-bound prepared-input and installed-lane receipts without automatic retry.

Both device collectors require a fresh output directory. Override
`GSPLAT_ANDROID_OUTPUT` or `GSPLAT_IOS_OUTPUT` when retaining multiple runs;
otherwise the bootstrap derives a target-local directory from the current
commit. A missing dataset, simulator UUID, device authorization, thermal
admission, browser GPU capability, signing identity, or physical device remains
an external prerequisite rather than a reason to weaken validation.

### Reusable launchbook: Android, Web, and macOS

Use this sequence for every run: `doctor` establishes host prerequisites,
`command` prints the exact delegated repository command, and `run` executes it
once. `READY` means the static prerequisites and fresh destination are present;
it is not a promise that a device, driver, browser, or Surface will succeed at
runtime. `BLOCKED` is an admission result: fix the reported prerequisite and
rerun `doctor`. The bootstrap never installs software, deletes old evidence,
or retries a failed command.

| Target | Read-only admission | Execution | Evidence boundary |
| --- | --- | --- | --- |
| macOS / Metal | `python3 tests/verification_bootstrap.py doctor --profile macos-metal` | `python3 tests/verification_bootstrap.py run macos-metal` | Hardware-backed SortedAlpha Metal conformance; no benchmark artifact. |
| Chrome / WebGPU | `GSPLAT_ARTIFACT_DIR=<fresh-path> python3 tests/verification_bootstrap.py doctor --profile web-webgpu` | Repeat the same environment with `run web-webgpu` | Real Chrome WebGPU/WASM functional artifact; not a performance comparison. |
| Chrome / WebGPU Truck 1080p | `GSPLAT_WEB_TRUCK_OUTPUT=<fresh-root> python3 tests/verification_bootstrap.py doctor --profile web-webgpu-truck-1080p` | Repeat the same environment through `command`, then once through `run web-webgpu-truck-1080p` | Q1 two-stage Packed Exact control plus bound terminal-throughput prerequisite; not a PlayCanvas conclusion or Q1 acceptance. |
| Q1 Product Quality view 000001 | Set the exact commit and retained authorities, then run `python3 tests/verification_bootstrap.py doctor --profile q1-product-quality-view000001` | Repeat the unchanged environment through `command`, then once through `run q1-product-quality-view000001` | One native and one headful PlayCanvas quality-only capture followed by the offline one-view gate; no performance evidence. |
| Android A065 | `GSPLAT_ANDROID_SERIAL=<serial> GSPLAT_ANDROID_DATASET=<absolute-ply> GSPLAT_ANDROID_OUTPUT=<fresh-path> python3 tests/verification_bootstrap.py doctor --profile android-a065` | Repeat the same environment with `run android-a065 --allow-device` | One full-quality Packed/CPU functionality and strict-ledger artifact; not a CPU/GPU comparison. |
| Android A065 Q3 SIMD | `GSPLAT_ANDROID_SERIAL=<serial> GSPLAT_Q3_A065_EXPECTED_COMMIT=<full-sha> GSPLAT_Q3_A065_OUTPUT=<fresh-path> python3 tests/verification_bootstrap.py doctor --profile android-a065-q3-simd` | Repeat the same environment through `command`, then once through `run android-a065-q3-simd --allow-device` | Physical element parity, diagnostic ladder, and complete-Truck five-pair terminal. No device run is implied by READY. |

Before `run`, substitute `command` for the action to inspect the exact command
and derived environment without building, opening Chrome, querying `adb`, or
touching a device. Keep every environment assignment identical between
`doctor`, `command`, and `run`.

Known, supported overrides remove repeated host discovery without embedding one
maintainer's paths in scripts:

- Android: `ANDROID_SDK_ROOT` and `JAVA_HOME`; the profile exports the selected
  SDK as both `ANDROID_SDK_ROOT` and `ANDROID_HOME`. The Apple Silicon Homebrew
  locations documented above are known working discovery results, not portable
  defaults.
- Web: `CHROME_PATH` selects the exact Chrome/Chromium executable, and
  `GSPLAT_ARTIFACT_DIR` names the new artifact destination.
- A065: `GSPLAT_ANDROID_SERIAL`, `GSPLAT_ANDROID_DATASET`, and
  `GSPLAT_ANDROID_OUTPUT` make device, asset, and destination explicit. The
  serial and dataset are intentionally never guessed or committed.
- A065 Q3 SIMD: `GSPLAT_ANDROID_SERIAL`,
  `GSPLAT_Q3_A065_EXPECTED_COMMIT`, and `GSPLAT_Q3_A065_OUTPUT` freeze the
  device selector, exact integrated source, and immutable destination. Dataset
  paths come only from the committed full-quality matrix.

Use an absolute output path or a repository-relative path. Because bootstrap
executes collectors directly rather than through a shell, a literal `~` is not
expanded.

The Android collector and retained Web publication path refuse an existing
destination. The bootstrap applies the same immutable-output rule to the short
WebGPU smoke before execution. For a retry, keep the failed directory for
diagnosis and choose a new path, for example one with the current short SHA
plus a caller-chosen attempt label:

```bash
GSPLAT_ARTIFACT_DIR=target/benchmarks/webgpu-<sha>-attempt-2 \
python3 tests/verification_bootstrap.py doctor --profile web-webgpu

GSPLAT_ANDROID_SERIAL=<adb-serial> \
GSPLAT_ANDROID_DATASET=/absolute/path/to/kitune1.ply \
GSPLAT_ANDROID_OUTPUT=target/android-sort-benchmarks/a065-<sha>-attempt-2 \
python3 tests/verification_bootstrap.py doctor --profile android-a065
```

Do not remove a retained artifact merely to make a profile `READY`. A runtime
failure remains one finite failed attempt; inspect it, change only the proven
cause, choose a fresh destination, and invoke `run` explicitly again.

### Q1 Product Quality view 000001 one-shot transaction

This opt-in profile is deliberately absent from the default doctor. Its doctor
and command modes only inspect the clean exact commit, Darwin/Apple-Silicon
Metal host, Cargo/Node/Chrome and locked Web tools, complete Truck, formal
camera authority, official Evaluation Images authority, and a fresh output.
They do not build, open a window, or start Chrome.

```bash
GSPLAT_Q1_PRODUCT_QUALITY_EXPECTED_COMMIT=<full-40-character-sha> \
GSPLAT_Q1_FORMAL_TRACE_AUTHORITY=/absolute/q1-formal-trace-authority \
GSPLAT_Q1_EVALUATION_AUTHORITY=/absolute/q1-evaluation-authority \
GSPLAT_Q1_TRUCK_DATASET=/absolute/complete-truck.ply \
GSPLAT_Q1_PRODUCT_QUALITY_OUTPUT=/absolute/fresh/q1-view000001-attempt-1 \
python3 tests/verification_bootstrap.py doctor \
  --profile q1-product-quality-view000001

# Inspect the exact same environment with `command`, then invoke it once:
GSPLAT_Q1_PRODUCT_QUALITY_EXPECTED_COMMIT=<full-40-character-sha> \
GSPLAT_Q1_FORMAL_TRACE_AUTHORITY=/absolute/q1-formal-trace-authority \
GSPLAT_Q1_EVALUATION_AUTHORITY=/absolute/q1-evaluation-authority \
GSPLAT_Q1_TRUCK_DATASET=/absolute/complete-truck.ply \
GSPLAT_Q1_PRODUCT_QUALITY_OUTPUT=/absolute/fresh/q1-view000001-attempt-1 \
python3 tests/verification_bootstrap.py run q1-product-quality-view000001
```

`GSPLAT_Q1_TRUCK_DATASET` must name the real Truck file, not a worktree
symlink. The quality producers intentionally reject symlinked immutable inputs;
the bootstrap doctor applies the same rule before any build or endpoint starts.

The coordinator first fully validates the immutable authorities and complete
Truck identity. It then invokes the native quality-only producer once, the
PlayCanvas `quality:truck-view000001` headful 979x546 command once, and the
offline one-view validator once, strictly in that order. A nonzero exit or
exception stops the transaction immediately; no step is repeated. Each
producer owns a separate child directory in one hidden staging root. Only all
three successes publish the requested root atomically. A failed attempt keeps
an immutable sibling named `<output>.failed-<session>` with `blocker.json`,
while the requested formal output remains absent. The result keeps Product
Quality `Deferred` until view 000009 qualifies and always records
`performance_authorized=false`. A child nonzero exit also retains its argv,
stdout, and stderr under `failed-command/<step>/`, without copying environment
variables, so the failure can be diagnosed without rerunning an endpoint.
If the native desktop host itself exits nonzero, the native producer also
publishes an immutable nested `native-view000001.failed-q1-native-*` tree with
its host command/stdout/stderr and build diagnostics. It removes the private
Cargo target and any unpublished candidate artifact before publication, so
failure evidence stays small and cannot masquerade as formal quality output.
Timeouts retain the subprocess partial streams; other subprocess failures
retain explicit empty streams when none exist. The build argv remains, but its
task-local environment values are removed before the failure tree is frozen.

### Q1 WebGPU Truck 1080p prerequisite

Before a Q1 image comparison, create the independent native Direct-f32 image
authority from a clean reviewed commit and a fresh output directory:

```bash
python3 tests/perf/collect-q1-truck-direct-reference.py \
  --expected-commit <full-40-character-sha> \
  --output target/qualification/q1-direct-f32-reference-<sha>
```

The producer performs no browser or device work and is not a performance run.
It renders only frozen Truck trace frames 0 and 1 through the existing native
offscreen Direct renderer, then atomically publishes two 1920x1080 RGBA8 PNGs
and `reference.json`. The sidecar freezes source, toolchain, release-binary,
dataset, trace pose/intrinsics, exactness, visible/drawn, PNG, and decoded-RGBA
identities. An existing output, dirty/different commit, identity drift, failed
render, or incomplete Direct receipt publishes only `blocker.json`; the
producer never retries automatically.

The named `web-webgpu-truck-1080p` profile reuses the standard Web collector
twice; it does not add a second wrapper. It admits only the canonical Truck PLY at
`tests/datasets/external/inria_3dgs/truck/point_cloud.ply` with SHA-256
`65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c`,
630225580 bytes, 2541226 splats, and source SH degree 3. The repository does not
fetch, copy, or install that 630 MB input. A missing file is a finite `BLOCKED`
doctor result.

Use one unchanged environment and a never-before-used output root for the full
launchbook sequence:

```bash
GSPLAT_WEB_TRUCK_OUTPUT=target/qualification/q1-webgpu-truck-1080p-<sha>-attempt-1 \
python3 tests/verification_bootstrap.py doctor --profile web-webgpu-truck-1080p

GSPLAT_WEB_TRUCK_OUTPUT=target/qualification/q1-webgpu-truck-1080p-<sha>-attempt-1 \
python3 tests/verification_bootstrap.py command web-webgpu-truck-1080p

GSPLAT_WEB_TRUCK_OUTPUT=target/qualification/q1-webgpu-truck-1080p-<sha>-attempt-1 \
python3 tests/verification_bootstrap.py run web-webgpu-truck-1080p
```

`command` prints one WASM build followed by two explicit standard-collector
commands in their execution order. The first uses
`GSPLAT_BENCHMARK_WINDOW_MODE=current_stats_evidence_window`, an untimed
`isolated_terminal` current-stats control, and writes
`<fresh-root>/control-current-stats/`. Only after that artifact passes the
canonical benchmark validator and its full-quality suite passes with verified
inputs does the second command use
`GSPLAT_BENCHMARK_WINDOW_MODE=terminal_queue_throughput_window`, continuous
`sustained_window` submission, and
`GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT=<fresh-root>/control-current-stats/manifest.json`
to write `<fresh-root>/throughput-terminal-queue/`. The control and throughput
share the exact build, dataset, trace, resolution, Packed/Adaptive policies,
sort interval, and 20/80 schedule; their observer/timing modes are deliberately
different and linked by the immutable workload-configuration digest.

The first collector atomically claims the fresh root before Chrome starts. A
validated control-completion marker then permits exactly one atomic throughput
stage claim. A control failure prevents the throughput command from starting;
a throughput failure is never retried. The root, stage claim, successful
control artifacts, and either failure log remain immutable for diagnosis. Both
collectors reject a dirty working tree before Chrome starts. Their
retained manifest and load receipt must prove
source=decoded=encoded=resident=addressable=2541226, source/resident SH3, all
source membership, and disabled sampling and LOD. The canonical full-quality
validator additionally rejects dynamic resolution, upscaling, a non-WebGPU
renderer path, or a display/image other than 1920x1080. Before any artifact,
PNG, or suite publication, every one of the 80 retained measured frames must
carry renderer-owned
`raster_execution_plan=projected_quads_exact`; missing, legacy, global, or mixed
raster plans fail closed.

On success, the control artifact and PNG publish first and
`<fresh-root>/suite.json` refers to that count-bearing control only after both
`validate-benchmark-artifacts.py` and
`validate-full-quality-experiment.py --verify-inputs` pass. The subsequent
throughput artifact independently passes the canonical benchmark validator and
binds the control run ID plus configuration digest; its first N-1 measured
frames have no current-stats observer and its final measured submission uses
the terminal receipt defined by the Q0 timing contract. A failed root is
retained and must not be rewritten; a new attempt requires separate explicit
authorization. This route is only the gsplat-rs Q1 prerequisite. It neither
runs the PlayCanvas comparator nor establishes a competitive performance
conclusion, so it cannot by itself make Q1 `Accepted`.

### Q1 same-Chrome five-pair series

Integrate the real PlayCanvas and gsplat-rs Q1 producers first, obtain a
fixed-SHA review of that exact clean commit, and create the Direct-f32
reference authority above from that same SHA. Then run the complete read-only
preflight and inspect the immutable five-pair command plan without starting
Chrome or creating output:

```bash
Q1_PREDECLARED_AT_UTC='<frozen-current-utc-timestamp>'
PYTHONDONTWRITEBYTECODE=1 python3 \
  tests/perf/collect-q1-truck-paired-series.py \
  --dry-run \
  --series-root /absolute/fresh/q1-series \
  --series-id <unique-series-id> \
  --collection-session-id <one-session-id> \
  --seed <declared-integer> \
  --chrome /absolute/path/to/Chrome \
  --gsplat-wasm-package /absolute/repo-local/quality-exact-package \
  --reference-authority /absolute/q1-direct-f32-reference-<sha> \
  --reviewed-sha <full-40-character-sha> \
  --predeclared-at-utc "$Q1_PREDECLARED_AT_UTC"
```

`--dry-run` performs the same complete read-only admission as `--execute`: it
validates the full blocker- and symlink-free authority tree, requires its
commit to equal `--reviewed-sha` and its generation time to precede the
predeclared series time, verifies clean exact `HEAD`, the Chrome executable and
process-table support, the quality-exact Wasm package and receipt, the locked
Puppeteer production closure, every formal input, and the fresh series-root
constraints. It does not create or copy files, build a package, or launch a
browser. A repository-local output must stay below ignored `target/`; an
output outside the repository is also valid. The parent directory must already
exist and the series root must not.

After inspecting a successful dry-run, invoke the same command with
`--execute` instead of `--dry-run`; do not change the reviewed SHA, authority,
package, Chrome, predeclared timestamp, schedule identity, or destination.
With identical arguments, dry-run and execute therefore bind byte-identical
plans and command receipts. `--execute` claims the
fresh root once, copies the complete authority tree to
`reference-authority/` without following symlinks, verifies each open source
file descriptor against the locked tree, then revalidates both source and
retained trees before any producer starts. The formal-input and execution-lock
receipts retain the authority source and claimed-tree identities before and
after the run.

Formal mode declares all 30 fresh producer commands before the first browser
launch, executes each once, stops on the first failure, binds the two actual
control manifest hashes before each throughput run, and calls only the
existing Q1 validator after the evidence schedule is complete. Retain a
failed root and its blocker; never delete it to simulate a retry.

The formal preflight fully admits the one authority receipt and complete tree,
decodes both bound reference PNGs, and freezes the reviewed commit, exact
Chrome binary, quality-exact Wasm package, producer/runtime
trees, the package-lock-derived installed Puppeteer production dependency
closure, Truck/trace inputs, validators and image tool. Producer subprocesses use
only the exact environment printed in `commands.json`; ambient Node, Python,
npm, Chrome, PlayCanvas, gsplat-rs and WebGPU variables are not inherited.
Each schedule reference binds the shared authority receipt path/SHA, PNG SHA,
decoded RGBA SHA and frozen pose/intrinsics SHA. Postprocessing receives that
same isolated host environment plus the exact
locked `CHROME_PATH`; each image receipt records the executable path and hash
reported by the image tool. Producer, canonical-validation, image-comparison,
final-validation and Git-helper commands have generous but finite predeclared
safety timeouts. A timeout is not a performance threshold: it terminates the
one attempt, writes a blocker once after the root is claimed, and never retries.
Every external command owns a fresh process group; timeout cleanup sends TERM,
waits for the declared grace period, then KILLs the entire remaining group and
reaps its leader. A bounded `ps` lineage tracker records PID, PPID, PGID and
start identity while the command runs, so Chrome/server children that call
`setsid` are also terminated and verified absent. A normal zero exit with a
remaining detached descendant fails closed instead of claiming a clean run.
Installed optional and peer runtime dependencies are included when reachable;
missing platform-only optional packages are allowed, while missing required or
installed-but-unlocked runtime dependencies fail preflight. Lockfile semantics
alone decide requiredness; installed manifests must match name, version and
runtime declarations, remain non-symlinked below `node_modules`, and cannot
reclassify a required dependency as optional.
After all producers and image comparisons, the same inputs plus clean HEAD are
rechecked before the schedule/result boundary. The final validator consumes
this lock and joins browser and Wasm hashes to endpoint artifacts. Integrate
all producer and orchestrator candidates first, then obtain a new fixed-SHA
review of that exact clean integrated commit before authorizing `--execute`.

## Fast Feedback

- Smallest useful check:

```bash
cargo check --workspace
```

- Typical use: most Rust changes that do not alter platform integration scripts or long-running perf behavior
- Expected runtime: short

## Core Rust Validation

```bash
cargo test --workspace
GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha
```

- Run this when changing shared types, parsing, render logic, or CLI behavior.
- The workspace test may skip pixel conformance when no native adapter exists.
  Set `GSPLAT_REQUIRE_GPU_CONFORMANCE=1` on a GPU-backed runner to make adapter
  absence a failure; CI and release run this requirement on macOS/Metal.
- Linux CI installs Mesa Vulkan/Lavapipe so GPU-required offscreen and C ABI
  paths have a deterministic software adapter; it is compatibility evidence,
  while the macOS jobs provide the required hardware-backed Metal evidence.

## Code Hygiene and Docs

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
node --check examples/web/src/main.js
npm --prefix packages/web run check
npm --prefix packages/web test
npm --prefix packages/web run pack:dry-run
```

- Run these before opening a pull request that changes Rust code, public docs,
  or the Web example.
- `node --check` is syntax validation only; browser behavior still requires
  the Web Example smoke path below.

## Day-to-Day Verification Set

These are the current day-to-day commands the repo relies on:

```bash
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
bash tests/security/run-cargo-deny.sh
node --check examples/web/src/main.js
npm --prefix packages/web run check
npm --prefix packages/web test
npm --prefix packages/web run pack:dry-run
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10 --max-avg-gpu-complete-ms 250
bash tests/perf/test-benchmark-artifacts.sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_desktop_surface_evidence.py
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_collect_q1_m4_native.py
python3 tests/perf/validate-dataset-manifests.py
python3 tests/datasets/test_dataset_tools.py
bash tests/perf/trace/test-trace-v1.sh
node --test examples/web/test/benchmark-artifact.test.mjs
bash bindings/android/scripts/test-android-benchmark-artifact-extraction.sh
python3 bindings/android/scripts/test_android_sort_benchmark_collector.py
bash bindings/apple/scripts/test-ios-benchmark-artifact-extraction.sh
PYTHONDONTWRITEBYTECODE=1 python3 bindings/apple/scripts/test_ios_sim_benchmark_collector.py
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest tests/test_verification_bootstrap.py
npm ci --ignore-scripts --prefix tests/competitive/playcanvas
npm test --prefix tests/competitive/playcanvas
bash tests/ffi/run-ffi-smoke.sh
bash bindings/android/scripts/run-jni-smoke.sh
bash bindings/apple/scripts/run-swift-smoke.sh
bash bindings/apple/scripts/build-xcframework.sh
```

## Desktop Smoke

```bash
cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png
cargo run -p desktop-example --features interactive-viewer -- tests/datasets/minimal_ascii.ply --auto-camera --interactive
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 30 --warmup-iterations 5
```

- Use the PNG path for deterministic local smoke output.
- Expect `offscreen_geometry_pipeline=sorted_index_direct` in the PNG and
  benchmark logs. GPU-required conformance compares this renderer against the
  CPU projection reference with an image tolerance.
- Use the interactive viewer when changing windowed presentation or camera
  interaction behavior. It uses the same `SurfaceRenderSession` and direct
  shader/resource layout as Web and mobile.

For a versioned raw-frame artifact, choose a fresh output directory; the runner
refuses to overwrite or mix an existing run:

```bash
cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 \
  --warmup-iterations 10 \
  --artifact-dir target/benchmarks/local/minimal-v1 \
  --series-id local \
  --run-id minimal-v1 \
  --frame-budget-ms 16.6666667 \
  --refresh-hz 60
python3 tests/perf/validate-benchmark-artifacts.py \
  target/benchmarks/local/minimal-v1
```

The artifact contract is `tests/perf/benchmark-artifact-v1.md`. A valid run
contains `manifest.json`, contiguous raw `frames.jsonl`, and recomputable
`summary.json`. The manifest distinguishes configured display values from
observed values and includes the direct resource preflight report.

For M2b real-window evidence, use the qualified Kitsune manifest and the
committed 1920x1080 trace. The output directory must be fresh and ignored. The
collector requires a clean Git tree and builds the release viewer itself with
the locked workspace into a newly empty collector-owned Cargo target inside the
staging transaction before it runs any evidence arm:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_desktop_surface_evidence.py
python3 tests/perf/collect-desktop-surface-evidence.py \
  --dataset-manifest tests/perf/datasets/kitsune.json \
  --trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json \
  --output target/benchmarks/m2b/surface-<candidate-sha>
```

Admission pins the qualified Kitsune identity, asset SHA/bytes/count/SH3, the
exact two-view trace content and file SHA, and the 20-warmup/80-measured
schedule. It also runs the repository camera-trace validator. Alternate inputs,
schedule overrides, a dirty source tree, or executable drift fail closed before
canonical publication. This is a four-arm transaction: `CpuPostSort`,
`GpuPostSort`, `GpuPreproject`, and `Adaptive` must each produce terminal joined
S/V/C/D receipts plus a final presented PNG. Every staged
`gsplat-benchmark/v1` artifact must pass the repository validator before the
suite directory is atomically published. Missing Metal capability, missing
terminal receipt, failed presentation/capture, plan drift, incomplete
membership, SH downgrade, resolution drift, or validator rejection leaves the
canonical output absent.
Each raw capture receipt records artifact-relative `final-frame.png`, so the
published logs remain revalidatable after the staging directory is renamed.

For the B1 desktop Balanced artifact bridge, first run the synthetic collector
tests. A separately authorized macOS/Metal endpoint owner may then choose a
fresh ignored destination and run the collector exactly once:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_collect_balanced_desktop_b1.py
python3 tests/perf/collect-balanced-desktop-b1.py \
  --dataset-manifest tests/perf/datasets/kitsune.json \
  --trace tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json \
  --output target/benchmarks/b1/desktop-balanced-<candidate-sha>
```

The collector builds private ExactFull32 and CandidateStable24 desktop hosts,
then invokes each host once with `--surface-diagnostic-multi-capture`. Each
lane must produce capture indexes and trace frames `0 -> 1 -> 2` / `0 -> 1 ->
0` inside that single process and Surface session; three independent host
processes may never be joined into a formal lane. Only after both complete
terminal streams validate does the collector stage six `gsplat-benchmark/v1`
run directories and one `gsplat-balanced-image-gate/v1` suite manifest. Every
run is checked by `validate-benchmark-artifacts.py`, and the outer suite is
checked by `validate-balanced-image-gate.py`, before a fresh destination is
atomically published. A malformed, missing, duplicate, out-of-order, mismatched
or failed receipt retains only failure diagnostics and leaves the requested
output absent. The collector is not a browser, mobile, performance or
cross-device qualification, and a failed endpoint attempt must not be retried
without a new explicit authorization.

For the separate B0 timing decision, do not reuse the short Kitsune image gate
as a throughput result. `collect-balanced-paired-timing.py` first validates a
same-commit `gsplat-balanced-image-gate/v1` suite, then builds the two private
diagnostic binaries and collects at least three counterbalanced Exact/candidate
pairs on the complete Truck and its committed moving 1920x1080 trace. It keeps
each run's terminal receipts, final capture and ordinary v1 artifact. It
returns only `candidate`, `exact`, or `inconclusive`; the last is a finite
result, not permission to tune indefinitely.

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_collect_balanced_paired_timing.py
python3 tests/perf/collect-balanced-paired-timing.py \
  --experiment b1-20 \
  --quality-suite <same-commit-validated-suite.json> \
  --output target/benchmarks/balanced/b1-20-truck-<candidate-sha>
```

The output must be fresh and ignored. This command opens a native Metal
Surface, so it is an endpoint action: inspect it through the launchbook first
and run it only with the relevant one-shot authorization. Missing Truck data or
a quality suite from another commit is rejected before either diagnostic binary
is built.

For the Q1 M4 native control plus terminal-throughput prerequisite, first run
the pure collector/ledger tests. A separately authorized endpoint owner may
then select one fresh ignored root and invoke the collector once:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/test_collect_q1_m4_native.py
python3 tests/perf/collect-q1-m4-native.py \
  --output target/qualification/q1-m4-native-<candidate-sha>-attempt-1
```

This collector admits only a clean Apple M4/Metal source identity, the complete
canonical Truck SH3 asset, the frozen two-view `1920x1080` trace, locked release
private host, Packed `ProjectedQuadsExact`, product Adaptive policies,
every-frame ordering and 60 Hz host cadence. It atomically claims the fresh
root and invokes the host once with no automatic retry. The first 20+80 stage
is an explicitly untimed sustained current-stats correctness control: warmup
tickets are fully drained with drawing stopped, the measured control restarts
at trace frame 0, and all issued tickets resolve before timing begins. Its
`V/C/D` and terminals cannot populate a throughput field.

The same process then runs a second 20+80 presentation stage with identical
scene, trace, camera and config identity and zero current-stats requests or
polls. After warmup frame 20 the host stops drawing and completes the warmup
queue once outside the measurement window, with no current-stats or capture
work. Measured frame 0 begins only after that drain is Ready. After measured
frame 80 the host stops drawing and uses
the existing no-draw queue-completion owner; only the monotonic
first-measured-camera-input to queue-completion
boundary derives terminal `N/FPS`. The two fixed-view PNG captures occur only
afterward and bind recomputed live-session camera matrices, camera revision,
presentation sequence and their own Ready control terminals. A phase-binding
receipt also joins both stages to the clean Git SHA and locked binary SHA.
Each timed presentation retains the actual Exact whole-plan Adaptive state,
executed PlanId and Candidate/Compact execution. Exact's independent projected
Adaptive state is intentionally `disabled` under `WholePlanController`; this is
an admitted receipt, not a rejection, and no Compact/Preproject occurrence is
required.

Ring busy, request/terminal failure, missing/duplicate/cross-joined control
tickets, timed current-stats load, incomplete counts or membership, failed
queue completion, draw during a drain, and dataset, trace, camera, matrix,
resolution, adapter, binary or Git drift reject the native artifact. This route
does not run or qualify PlayCanvas headful external presentation, the common
reference-image gate, or the five outer counterbalanced pairs, so even a valid
native prerequisite leaves Q1 itself Deferred.

The ordinary desktop Surface host and Q1 phase policy share one private runtime
owner that holds `SurfaceRenderSession` and exposes only command/immutable-
receipt events for current-stats, camera, render, capture and queue completion.
Their collectors likewise share the locked release builder, transactional
immutable-output finalizer and ticket ledger; Q1
reuses the committed paired-workload loader and camera-trace math. Only a named
`EnvironmentPrerequisiteError` raised during preflight may produce Deferred.
Once build or host execution starts—or any runtime output exists—every parsing,
ledger, `KeyError` or other unexpected exception is Rejected.

Every control ticket joins phase, member, trace frame and full renderer identity
across presentation, submission and terminal records. Count semantics are a
closed enum; unknown strings reject rather than defaulting to `D=V`. Collection
is written under a private sibling staging root, private build state is removed,
files and directories are fsynced and made immutable, and only then is the root
atomically renamed. Cleanup failure may publish only a bounded immutable
Rejected result; write/chmod/publication failure leaves no Accepted root and
reports an explicit finalization blocker.

Committed dataset identities and the shared camera oracle have separate checks:

```bash
python3 tests/datasets/test_dataset_tools.py
python3 tests/perf/validate-dataset-manifests.py
python3 tests/perf/validate-dataset-manifests.py --verify-available
bash tests/perf/trace/test-trace-v1.sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/validate-scalable-proxy-image-gate.py \
  --preflight-formal
python3 tests/perf/test_full_quality_experiment.py
python3 tests/perf/validate-full-quality-experiment.py \
  tests/perf/full-quality-matrix-plan-v1.json --allow-incomplete
```

- The dataset-tool test proves deterministic fixed-record PLY tier selection,
  header/non-vertex preservation, provenance checks, and streaming HTTP range
  extraction against tiny local fixtures; it never downloads a large model.
- The manifest command without `--verify-available` validates committed metadata
  and is safe in CI.
- `--verify-available` additionally hashes and inspects every external asset
  present in the local checkout.
- The trace test regenerates the `gsplat-camera-trace/v1` fixture and rejects
  hash or matrix-convention drift. A competitive harness must consume the
  explicit matrices or prove its API reconstruction matches them.
- The S1 formal preflight verifies the pinned validator dependencies, Bonsai
  authority manifest, any locally available full SH3 source/camera files, and
  both frozen trace identities. Exit `2` with a machine-readable `Deferred`
  receipt is the expected finite result while the source, authored-camera
  review, or either required endpoint artifact is unavailable; it never
  converts a missing prerequisite into proxy-quality success.
- The full-quality plan rejects 640x360/640x480 as formal evidence, pins
  desktop/Web to 1920x1080, the connected A065 to its observed 2412x1080
  Surface, and the iOS simulator to its observed 2622x1206 drawable. It also
  proves that endpoint-specific traces retain one pose/FOV camera family and
  require exact requested, Surface, internal-render, and presented dimensions.

### Formal full-quality acceptance contract

- The only accepted dimensions are desktop/Web `1920x1080`, Nothing A065
  `2412x1080`, and iPhone 17 Pro simulator `2622x1206`. Evidence from another
  size is smoke or exploratory evidence until the formal matrix is explicitly
  revised.
- Every accepted frame must prove
  `requested = Surface = internal render = presented` pixels and an actually
  presented terminal frame. Dynamic resolution and upscaling must both be
  disabled.
- `source = decoded = encoded = resident = addressable` membership and the
  complete source SH degree must be retained. Sampling, point reduction, LOD,
  silent SH downgrade, and incomplete residency are rejection conditions, not
  performance strategies.
- Direct and full-resident Packed may qualify. Paged and the sampled WebGL
  preview are smoke/diagnostic paths only and cannot produce formal quality or
  competitor evidence. A device that cannot admit the exact workload must
  return an explicit capacity failure.
- Count evidence is `S/V/C/D`: complete source/resident/addressable `S`,
  near/far candidate `V` (historical `visible`), conservative post-projection
  contributor `C`, and issued draw `D`. New artifacts declare
  `candidate_visible_contributor_issued_v1` and prove `0 <= C <= V <= S`.
  Only an explicit exact-contributor flag permits `D=C`; Direct/downlevel and
  legacy receipts still require `D=V`. This prevents a point budget from being
  mislabeled as projection culling.
- Adaptive CPU/GPU selection compares the same `FrameCompletion` interval for
  both backends: frame start through queue completion, including sorting,
  projection, rasterization, submission, and queueing. Order-stage timestamps
  are diagnostic only. Switching raster execution plans resets the learned
  Adaptive state before new samples are compared.

Android, Apple, and Web collectors can also emit the same v1 artifact contract:

```bash
bash bindings/android/scripts/test-android-benchmark-artifact-extraction.sh
bash bindings/apple/scripts/test-ios-benchmark-artifact-extraction.sh
PYTHONDONTWRITEBYTECODE=1 python3 bindings/apple/scripts/test_ios_sim_benchmark_collector.py
node --test examples/web/test/benchmark-artifact.test.mjs
# After a device benchmark, extract from logcat:
# python3 bindings/android/scripts/extract-android-benchmark-artifacts.py \
#   target/benchmarks/phase-a/android-kitsune-logcat.txt \
#   target/benchmarks/phase-a/android-kitsune-pong-a065 \
#   --validator tests/perf/validate-benchmark-artifacts.py

# Reproducible paired CPU/GPU device collection (output must be fresh):
# Add --prepare-apk only to the first tier after app/native code changes.
# python3 bindings/android/scripts/collect-android-sort-benchmarks.py \
#   --serial <adb-serial> --ply <dataset.ply> \
#   --backend cpu --backend gpu --repetitions 5 \
#   --randomize-order --seed 20260722 --sort-interval 1 \
#   --cooldown-seconds 10 --max-thermal-status 0 \
#   --output target/android-sort-benchmarks/<series-id>

# Desktop Web (requires Chrome + PlayCanvas harness puppeteer-core):
# GSPLAT_ARTIFACT_DIR=target/benchmarks/phase-a/web-kitsune-desktop \
#   node examples/web/scripts/collect-web-benchmark-artifact.mjs
```

- The Android collector defaults to reusing an already installed debuggable
  sample APK only after its `base.apk` SHA-256 and byte count exactly match the
  local APK; `--prepare-apk` performs the one-time build/install with the tiny
  `minimal_ascii.ply` bootstrap asset when required, never with the measured
  tier.
  For each dataset experiment it pushes one hash-addressed PLY under the exact
  `/data/local/tmp/gsplat-benchmark-<sha256>.ply` path, clears only
  `com.gsplat.example` before each run, copies the fixture to
  `files/imported_scene.ply` with `run-as`, and verifies that internal file
  before launch. It cleans only its exact staged path, records a seeded paired
  schedule, full tagged logcat, thermal observations, and validated v1
  artifacts, and refuses to overwrite an existing output root. Use `--dry-run`
  to inspect every command without changing the device.

## Competitive Harness and Historical Phase E Pairing

The PlayCanvas harness freezes dependency identity and has a separate
fail-closed browser path smoke plus a validated timed collector:

```bash
npm ci --ignore-scripts --prefix tests/competitive/playcanvas
npm test --prefix tests/competitive/playcanvas
npm run smoke --prefix tests/competitive/playcanvas
npm run benchmark:kitsune-static --prefix tests/competitive/playcanvas
```

- Use only the committed exact dependency and lockfile.
- A passing preflight proves package version, tarball integrity, MIT license,
  frozen full revision mapping, and runtime revision prefix.
- The smoke additionally requires Chrome/Chromium WebGPU and proves the
  selected backend, resolved and active GPU-sort renderer, source format,
  canvas size, and a nonzero loaded splat count. Its result and pre-timing
  screenshot are written below `target/benchmarks/playcanvas-path-smoke/`.
- The original Phase E procedure used five predeclared, sequential
  randomized-order pairs, 120 warmups and 3,600 measured frames per run, the
  shared Kitsune trace, and raw 640×480 images. It remains reproducible
  historical smoke/diagnostic evidence, but its 640×480 resolution violates
  the formal contract above and therefore cannot establish a current quality,
  performance-parity, or competitor claim.
- Store `pairing.pair_id`, `pairing.run_order`, and `pairing.position` in both
  manifests with `PHASE_E_PAIR_ID`, `PHASE_E_PAIR_ORDER`, and
  `PHASE_E_PAIR_POSITION`. Set `PLAYCANVAS_ARTIFACT_DIR` and
  `GSPLAT_ARTIFACT_DIR` to fresh per-run directories.
- Generate each pair's `image-diff.json` and the series result with:

```bash
node tests/perf/compare-image-ssim.mjs \
  <pair>/playcanvas/final-frame.png \
  <pair>/gsplat-rs/final-frame.png \
  --threshold 0.99 --output <pair>/image-diff.json
python3 tests/perf/compare-paired-benchmarks.py \
  <five-pair-series> --output <five-pair-series>/comparison.json
```

- The paired comparator rejects mismatched dataset, trace, display, backend,
  order, sample, warmup, or quality fields and reports deterministic bootstrap
  confidence intervals. For formal desktop/Web competitor evidence, rerun the
  same controlled pairing at `1920x1080` with the full-quality contract above;
  the historical 640×480 series proves only harness behavior.

## Web Example Smoke

```bash
node --check examples/web/src/main.js
node --test examples/web/test/renderer-policy.test.mjs
python3 -m http.server 4173 --bind 127.0.0.1 --directory .
```

- Open `http://127.0.0.1:4173/examples/web/` in a browser.
- Do not use `file:///.../examples/web/index.html`; the example depends on HTTP
  serving from the repository root so wasm imports and `/tests/...` dataset
  fetches resolve correctly.
- The default/product route requires the generated Rust/WASM package and a
  usable browser WebGPU Surface. Complete the Web WASM Build below first.
  Missing package, WebGPU construction failure, and Exact scene-admission
  failure all fail closed; they do not select WebGL2 automatically.
- A successful default/product startup shows `surface=wasm-wgpu realtime`,
  `state=rendering`, the selected installed dataset and path, and non-zero
  terminal `Visible` and `Drawn` counts. Pending Exact counts remain
  unavailable until their matching renderer current-stats terminal.
- Use the file picker or the `Flowers` button for larger local `.ply` smoke
  checks. Use `?dataset=flowers` for repeatable automation against
  `tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply`.
- The non-equivalent sampled WebGL2 diagnostic is available only after the
  explicit `?gsplat_allow_sampled_webgl=true` opt-in and only outside formal
  qualification. The flag permits the diagnostic after an Exact startup
  failure; it does not replace a working Exact route or make sampled output
  product/fallback evidence. For example:

```text
http://127.0.0.1:4173/examples/web/?dataset=minimal&gsplat_allow_sampled_webgl=true
```

- For Exact Packed benchmark smoke, open:

```text
http://127.0.0.1:4173/examples/web/?gsplat_benchmark=true&gsplat_benchmark_sync=true&gsplat_benchmark_frames=5&gsplat_benchmark_warmup_frames=1&gsplat_surface_sort_interval=2
```

- Expected benchmark output includes `BENCHMARK_RESULT` and
  `renderer=wasm_packed_atlas`. The latter is the current Packed raster
  identity returned as `wasm_${rasterPath()}`; the retired Direct product label
  is not an acceptable expectation.
- Optional Flowers scene smoke:

```text
http://127.0.0.1:4173/examples/web/?dataset=flowers&gsplat_benchmark=true&gsplat_benchmark_sync=true&gsplat_benchmark_frames=2&gsplat_benchmark_warmup_frames=0&gsplat_surface_sort_interval=2
```

- Expected benchmark output includes `BENCHMARK_RESULT dataset=flowers_1.ply`.
- After the benchmark/camera motion stops, leave the page visible for at least
  three animation frames. The canvas must remain non-black with non-zero
  terminal `Visible` / `Drawn` counts; this is the stationary Packed Exact
  regression check and proves cached exact order is still presented.

## Web WASM Build

Use this when changing `crates/gsplat-web/` or the browser canvas Surface entry
in `crates/gsplat-render-wgpu/`:

```bash
cargo check -p gsplat-web --target wasm32-unknown-unknown
bash packages/web/scripts/build-wasm.sh
bash packages/web/scripts/build.sh
node --check packages/web/dist/index.js
npm --prefix packages/web run pack:dry-run
```

- `cargo check --workspace` still checks the host-side workspace and the
  non-wasm stub for `gsplat-web`.
- The wasm target must be installed separately with
  `rustup target add wasm32-unknown-unknown`.
- `packages/web/scripts/build-wasm.sh` also requires the `wasm-bindgen` CLI and writes
  generated files to ignored `examples/web/pkg/`.
- `packages/web/scripts/build.sh` writes the local ESM wrapper distribution to
  ignored `packages/web/dist/`.
- This is the proof path for the shared Rust `wgpu` renderer and local Web SDK
  wrapper running in the browser. After the package exists, reload
  `http://127.0.0.1:4173/examples/web/?dataset=flowers`; expected status should
  report `surface=wasm-wgpu`, `renderer=wasm_packed_atlas` in benchmark output,
  and non-zero `Visible` / `Drawn` counts for `flowers_1.ply`.

The commands in this documentation edit are build, syntax, and unit-policy
evidence only. They do not claim a browser run. Real Chrome/WebGPU execution at
the accepted M7 SHA `1de3f79fa2fa22955f99c887bea421c918e31ee0` remains
**Deferred**; WASM compilation, Node tests, sampled WebGL2, or a browser result
from another SHA cannot substitute for that fixed-SHA endpoint evidence.

For a local external-consumer check, build the distribution, pack from the
package directory, install the resulting tarball into a fresh directory, and
import its public ESM entrypoint. This proves tarball shape and local
consumption only; it does not publish to npm or make the Web API stable.

```bash
bash packages/web/scripts/build.sh
mkdir -p target/qualification/web-pack target/qualification/web-consumer
(cd packages/web && npm pack --pack-destination ../../target/qualification/web-pack)
npm install --prefix target/qualification/web-consumer \
  "$PWD/target/qualification/web-pack/gsplat-rs-web-0.1.3.tgz"
node --input-type=module -e \
  "import('./target/qualification/web-consumer/node_modules/@gsplat-rs/web/dist/index.js').then(m => console.log(m.GSPLAT_WEB_SDK_VERSION))"
```

## Mobile Builds and Simulator Smoke

```bash
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-app.sh
bash bindings/apple/scripts/build-ios-device-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/run-ios-device-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/benchmark-ios-device-app.sh
bash bindings/apple/scripts/build-ios-sim.sh
bash bindings/apple/scripts/build-xcframework.sh
bash bindings/apple/scripts/run-ios-sim-smoke.sh
bash bindings/android/scripts/build-aar.sh
bash bindings/android/scripts/build-sample-apk.sh
```

- Run these when changing mobile packaging, simulator run scripts, or build scripts.
- Check `bindings/android/README.md` or `bindings/apple/README.md` for platform
  prerequisites before assuming SDK/NDK/Xcode state.
- iOS device runs require a development provisioning profile whose device list
  includes the target phone. `bindings/apple/scripts/build-ios-device-app.sh`
  can auto-select a matching local development profile and `Apple Development:`
  identity, or you can set `IOS_PROVISIONING_PROFILE`,
  `IOS_CODE_SIGN_IDENTITY`, and `IOS_BUNDLE_ID` explicitly.
- iOS run and benchmark scripts require `IOS_DEVICE_ID=<coredevice-id-or-udid>`.
  Run `xcrun devicectl list devices` to inspect paired device identifiers.
- `bindings/apple/scripts/build-ios-device-app.sh` builds Rust with `release` and Swift
  with `-O` by default so the iPhone path can be compared with Android's
  release-native APK. Use `IOS_RUST_PROFILE=dev` and
  `IOS_SWIFT_OPT_LEVEL=-Onone` only for debugging.
- `bindings/apple/scripts/build-xcframework.sh` builds the local
  `bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework` used by the
  `GsplatKit` Swift package wrapper. It builds both
  `aarch64-apple-ios-sim` and `x86_64-apple-ios` simulator slices by default;
  set `IOS_XCFRAMEWORK_SIM_TARGETS` for a custom simulator slice.
- `bindings/android/scripts/build-sample-apk.sh` builds a debug APK container, but compiles the Rust native library with the Rust `release` profile by default. Set `ANDROID_RUST_PROFILE=dev` only for native debugging.
- `bindings/android/scripts/build-aar.sh` builds the local `gsplat-android` AAR at
  `bindings/android/gsplat-android/build/outputs/aar/gsplat-android-release.aar`.
  It accepts `ANDROID_SDK_ROOT` or `ANDROID_HOME`, packages `arm64-v8a` only in
  this slice, uses Android native API level `24` by default, and is not a Maven
  publishing path.

## Android Surface Smoke

Use this when changing Android Surface rendering, JNI surface glue, or `SurfacePresenter` behavior:

```bash
bash bindings/android/scripts/build-sample-apk.sh
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ADB="$ANDROID_SDK_ROOT/platform-tools/adb"
"$ADB" install -r examples/android/app/build/outputs/apk/debug/sample-app-debug.apk
"$ADB" shell am start -n com.gsplat.example/.MainActivity
# Experimental local paged path:
"$ADB" shell am start -n com.gsplat.example/.MainActivity \
  --es gsplat_geometry_path paged
```

- Expected first frame includes `Kitsune shrine`, `LIVE`, a non-zero splat count,
  and frame time. Open `Studio` and confirm `surface=wgpu realtime` and
  `state=rendering`. Direct/packed receipts use
  `drawn=<surface_instances>/<visible_instances>`; paged receipts use
  `drawn=<active_resident>/<loaded_source>`.
- For the paged Kitsune smoke, expect dense fixed-slot residency rather than a
  fragmented low-thousands subset. The 2026-07-13 A065 initial-view smoke
  observed `drawn=225784/279199`: one global cover plus three balanced
  refinements. A different camera may select the 53,415-entry final refinement
  and report one fewer active splat. The denominator is loaded source splats,
  not a claim that every source splat is simultaneously resident.
- For repeatable perf checks, add the benchmark extras documented in `bindings/android/README.md` and read the `BENCHMARK_RESULT` logcat line.
- The APK packages the selected scene as `assets/showcase.ply`. If `adb install -r`
  reports insufficient storage, uninstall `com.gsplat.example` and reinstall.

## iOS Surface Smoke

Use this when changing iOS realtime rendering, UIKit surface glue, touch
controls, mobile packaging, signing, or `SurfacePresenter` behavior:

```bash
bash bindings/apple/scripts/run-ios-sim-app.sh
IOS_DEVICE_ID=<coredevice-id-or-udid> bash bindings/apple/scripts/run-ios-device-app.sh
```

- Expected first frame includes `Kitsune shrine`, `LIVE`, `279199 SPLATS`, and
  frame time. Open `Studio` and confirm `state=rendering`, `camera=<mode>`,
  `dataset=kitune1.ply`, and `drawn=<surface_instances>/<visible_instances>`.
- The simulator app bundle lives at `target/ios-sim-app/GsplatIOSExample.app`; the
  device app bundle lives at `target/ios-device-app/GsplatIOSExample.app`. Both
  package the selected build-time dataset as `showcase.ply`, preferring Kitsune
  and falling back to Flowers.
- The app uses `Documents/imported_scene.ply` when present, otherwise the
  bundled showcase dataset, otherwise a generated minimal ASCII PLY fallback.
- Touch smoke should include at least one one-finger swipe/orbit check in the
  simulator. Pinch zoom and two-finger pan use the same C ABI camera-control
  functions.
- For repeatable perf checks, add the benchmark args documented in
  `bindings/apple/README.md` after `--`. Use
  `bindings/apple/scripts/benchmark-ios-device-app.sh` on a physical iPhone to print the
  `BENCHMARK_RESULT` line and keep the raw log under
  `target/ios-device-benchmarks/`.

## Release Bar

Before cutting a release, also run:

```bash
RELEASE_VERSION=<major.minor.patch> bash tests/release/check-version.sh
STABILITY_SECONDS=1800 bash tests/perf/run-long-stability.sh
```

## Current Manual Validation Gaps

- Android true-device launch and benchmark are not implied by
  `bash bindings/android/scripts/build-sample-apk.sh`; run the Android Surface
  Smoke path on a physical device before claiming Android device validation.
- iOS physical-device launch and benchmark are not implied by
  `bash bindings/apple/scripts/build-ios-device-app.sh`; set
  `IOS_DEVICE_ID=<coredevice-id-or-udid>` and run the iOS Surface Smoke or
  benchmark path before claiming iPhone runtime validation.
- Maven, remote binary SwiftPM/XCFramework, and npm publishing are release
  distribution tasks. Local AAR/XCFramework/npm-pack checks prove packaging
  shape, not public distribution readiness.

## Targeted Checks

- If you touch `crates/gsplat-ffi-c/`, run `bash tests/ffi/run-ffi-smoke.sh`.
- If you touch `bindings/android/`, `examples/android/`, or JNI glue, run
  `bash bindings/android/scripts/run-jni-smoke.sh`. If you touch Android packaging or
  `bindings/android/gsplat-android/`, also run
  `bash bindings/android/scripts/build-aar.sh`,
  `bash bindings/android/scripts/build-sample-apk.sh`, and
  `GRADLE_BIN="$(bindings/android/scripts/ensure-gradle.sh)"; "$GRADLE_BIN" -p bindings/android :sample-app:testDebugUnitTest`;
  for Surface changes, also run the Android Surface smoke above.
- If you touch `bindings/apple/`, `examples/ios/`, or Swift/FFI integration, run
  `bash bindings/apple/scripts/run-swift-smoke.sh`; for `GsplatKit` or iOS packaging
  changes, also run `bash bindings/apple/scripts/build-xcframework.sh` and
  `cd bindings/apple/GsplatKit && swift package describe --type json` plus
  `cd bindings/apple/GsplatKit && xcodebuild -scheme GsplatKit -destination 'generic/platform=iOS Simulator' build`;
  for realtime Surface or touch changes, also run
  `bash bindings/apple/scripts/run-ios-sim-app.sh`; for offscreen simulator smoke
  changes, run `bash bindings/apple/scripts/run-ios-sim-smoke.sh`.
- If you touch PLY import or scene normalization, run `cargo test --workspace` and `cargo run -p desktop-example -- tests/datasets/minimal_ascii.ply --png target/out.png`.
- If you touch SPZ import (`crates/gsplat-io-spz/`), run
  `cargo test -p gsplat-io-spz` and
  `cargo test -p gsplat-render-wgpu ply_vs_spz_offscreen_image_parity_gate_on_minimal_fixture`.
  Load metrics land under `target/benchmarks/phase-c/`
  (`minimal-spz-vs-ply-load-metrics.json`, `minimal-spz-vs-ply-ttff.json`).
- If you touch Phase D residency / spatial pages / page scheduling in
  `crates/gsplat-render-wgpu/`, run
  `cargo test -p gsplat-render-wgpu --lib` and confirm the
  `spatial_pages`, `residency`, and `page_scheduler` unit tests pass.
- If you touch renderer, sorting, or perf-sensitive code, run `cargo run --release -p bench-runner -- tests/datasets/minimal_ascii.ply 120 --warmup-iterations 10 --max-avg-gpu-complete-ms 250` and consider the long-stability script. The runner reports CPU preprocessing, CPU sort, encode/submit CPU wall, GPU wait, GPU-complete, nearest-rank frame distributions, missed-frame counts, and structured direct-resource preflight together with adapter/backend/driver metadata. Use the artifact route above when the result will be retained or compared. Surface/WASM output additionally reports render/submit and frame-wall phases; compatibility CPU-geometry fields stay zero on the sole direct path.
- If you touch `examples/web/`, run `node --check examples/web/src/main.js`
  and the Web Example smoke above. If you touch
  `packages/web/`, also run
  `npm --prefix packages/web run check`,
  `npm --prefix packages/web test`,
  `bash packages/web/scripts/build.sh`, and
  `node --check packages/web/dist/index.js`.
- If you touch `crates/gsplat-web/` or browser Surface creation in `crates/gsplat-render-wgpu/`, run `cargo check --workspace` and the Web WASM Build path above.
- For spatial/tile/chunk feasibility checks on a loaded PLY, use:

```bash
cargo run -p bench-runner -- <scene.ply> --analyze-spatial
```

## Structural Checks

- CI entrypoints live in `.github/workflows/ci.yml`, `.github/workflows/perf-smoke.yml`, and `.github/workflows/long-stability.yml`.
- Contributor issue and pull request templates live under `.github/`.
- The lint and docs entrypoints are `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`.
- Dependency advisory, license, duplicate-version, and source policy is
  configured in `deny.toml` and checked with
  `bash tests/security/run-cargo-deny.sh`.
- The tag release contract and manual GitHub settings gates live in
  `RELEASING.md`.

## Failure Triage

- First inspect the failing script itself. The scripts in `tests/`, `bindings/`,
  and `packages/` are the canonical source for environment assumptions.
- Common failure modes are missing platform toolchains, missing Android SDK/NDK state, Kotlin/JVM toolchain resolution, dynamic library path issues, and dataset path mistakes.
- If a platform-specific path fails, rerun the exact repo-local script directly from the repo root and inspect the first failing command before widening the investigation.
