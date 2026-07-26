# M7 final acceptance audit

## Decision and fixed identities

- Final decision: **Accepted** for M7's defined legacy-owner deletion,
  compatibility, source-ownership and available-endpoint evidence scope.
- Accepted source and evidence tip:
  `2739f4899facc03e6a0fb35c23b42d9762ede9cc` (tree
  `be32416d24dc069f25bcd44d8156c89527fd9927`).
- Rollback base / accepted M6 tree:
  `a798b8accf454e96792df45d38e8564e7e27556e` (tree
  `fd13949e2237c96181b570671f23ecbfe408b1a1`).
- The complete linear M7 range is `a798b8a..2739f48`: 39 commits, beginning
  with `707cd8001abf84cd9fc990dd45246a0c02db75ef` and ending with the accepted
  tip. It supersedes the obsolete nine-commit `3f52558` closeout range.
- M8 remains unstarted and outside the machine task-state registry. Package M
  remains Active until M8 is separately activated and accepted; this closeout
  neither performs nor claims any M8 handbook, release or archival work.

M7 acceptance is responsibility- and evidence-based. It does not depend on a
fixed source-line count and does not widen the v0.1 API, ABI, shader, render or
release boundary.

## Requirement-by-requirement result

| M7 exit requirement | Decision | Evidence and boundary |
| --- | --- | --- |
| Renderer, Session and Presenter source ownership | **Accepted** | An independent read-only audit of clean `2739f48` accepted all three exits with P0/P1/P2 each zero. `lib.rs` is wiring/public facade/re-export/compatibility; `Renderer` implementation lives in `renderer/facade.rs` and private owners. `surface_session.rs` is the public frame-transaction composer; the single publication ledger is `evidence/session_publication.rs::SessionPublication`. `surface_presenter.rs` is the adapter/presentation host around the private standalone Direct/Paged runtime owners. |
| One Exact semantic owner | **Accepted** | Product Packed has one route through `SurfaceRenderSession -> SurfacePresenterHost -> Renderer::PreparedRuntimeSlot`. `PreparedRuntimeSlot` remains the sole Exact generation, `PlanSet`, mandatory sampler, controller and terminal-result owner. The obsolete standalone Packed graph and TiledExact are absent. |
| Call-site/API/ABI classification | **Accepted** | The [M7 public call-site/API/ABI ledger](m7-public-callsite-api-abi-ledger.md) classifies every removed or retained Rust, C, JNI/Kotlin, Swift and Web entry. The accepted range changes no stable C layout or signature; compatibility facades remain where the ledger requires them. |
| No legacy/new toggle or dual product default | **Accepted** | Standalone Presenter supports Direct and diagnostic Paged and rejects Packed before allocation. Product consumers select Packed through the shared Session/Host/Exact route. Paged remains explicit, CPU-only and rejects async configuration before mutation. |
| Workspace, renderer and source policy | **Accepted evidence** | The root matrix at clean `2739f48` passed workspace check, renderer library tests, strict Clippy, Rustdoc and the real-tree architecture policy. Architecture self-tests also passed. The later read-only ownership audit inspected the current call graph and accepted the three M7 exits; it did not use file length as proof. |
| C ABI/FFI compatibility | **Accepted evidence** | The canonical real C header/client smoke passed at `2739f48`. No C header, export, wrapper or ABI layout changed in the final ownership/publication subrange. |
| WASM compatibility | **Compile evidence** | Both the renderer and `gsplat-web` compiled for `wasm32-unknown-unknown`. This is compile evidence only and is not promoted to browser/WebGPU runtime evidence. |
| macOS/Metal product mechanics | **Accepted evidence** | Forced SortedAlpha conformance passed on Apple M4 Metal. The hidden-window public Surface test also passed, covering retained Direct/Paged entrypoints, pre-allocation standalone Packed rejection and one successfully presented product Packed host frame. This is not broad cross-platform pixel proof. |
| Android A065 exact function and strict terminal ledger | **Accepted evidence** | The clean `2739f48` formal run rendered the complete 279,199-splat Kitsune SH3 scene at 2412x1080 through Packed/CPU PostSort. All 20 measured frames joined issued order and current-stats terminals, with `V=D=279199` and `C` in `{226450,236792}`. The canonical artifact and full-quality suite validators passed with verified inputs and the retained native Surface PNG. |
| Chrome/WebGPU runtime at the accepted SHA | **Deferred** | The exact locked `wasm-bindgen-cli 0.2.121` was unavailable. WASM compilation and older browser runs are not substituted for a `2739f48` Chrome/WebGPU run. |
| Apple physical device | **Deferred** | Host Metal conformance and hidden-Surface evidence do not establish a final-M7 physical-iPhone run. Existing M6 wrapper/Simulator qualification remains historical M6 evidence. |
| Windows and Linux runtime | **Deferred** | No `2739f48` Windows or Linux runtime was executed. macOS and WASM compilation do not imply either backend. |
| Aggregate rollback | **Accepted evidence** | All 39 reachable M7 commits were reverse-applied newest-to-oldest without conflict in an isolated detached worktree. The restored index and worktree exactly matched the accepted M6 tree, then passed `cargo check --workspace` and the C FFI smoke. |
| Repository hygiene | **Accepted evidence** | The accepted source worktree and both independent audit worktrees finished clean. Android binaries, model input, logs and PNG remain ignored machine-local evidence under `target/`; none is committed as source. |

## Source-ownership exit audit

The independent fixed-SHA audit gave a finite **Accept** with no P0, P1 or P2:

- `crates/gsplat-render-wgpu/src/lib.rs` contains module wiring, public types
  and re-exports, compatibility/error mapping and the public facade's private
  owner fields. It contains no `impl Renderer`; concrete renderer semantics
  live under `renderer/` and its private scene/offscreen/attempt owners.
- `crates/gsplat-render-wgpu/src/surface_session.rs` has one defensible
  responsibility: compose the public Surface frame transaction across the
  renderer, host, camera/schedule state and private controllers. The sole
  evidence/statistics/terminal publication owner is
  `evidence/session_publication.rs`, with one successful-present publication
  entry and terminal-safe transition handling.
- `crates/gsplat-render-wgpu/src/surface_presenter.rs` owns Surface/device/
  queue hosting, configuration, lifecycle, capture, capabilities and the
  mechanical acquire/encode/submit/present transaction. Direct/Paged scene,
  order, draw and telemetry semantics live in
  `surface/standalone_session_runtime.rs` and its private Direct/Paged leaves.

The final publication commits `88fd7b5`, `ca9bf45`, `48eed96` and `2739f48`
were reviewed as one integrated state. They provide the private publication
owner, atomic complete-DTO publication, architecture guardrails and transition
terminal-lifecycle regression coverage. Intermediate rejected candidates are
not acceptance evidence.

The audit changed no files and ran no device or browser endpoint. Its platform
and pixel-runtime cells therefore remain Deferred and are supplied only by the
separate root evidence explicitly listed below.

## Final root verification ledger

The root task recorded these results against clean source tip `2739f48`:

| Command | Result | Evidence class |
| --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | Rust formatting. |
| `cargo check --workspace` | PASS | Host workspace compile. |
| `cargo test -p gsplat-render-wgpu --lib` | PASS: 462 passed, 8 ignored | Renderer behavior, including publication and transition regressions; ignored manual research tests are not acceptance evidence. |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | Strict all-target lint. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS | Public Rust documentation. |
| `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/architecture -p 'test_source_architecture.py'` | PASS: 3 | Architecture checker self-tests. |
| `PYTHONDONTWRITEBYTECODE=1 python3 tests/architecture/check_source_architecture.py` | PASS | Real-tree source/dependency policy at the fixed source tip. |
| `bash tests/ffi/run-ffi-smoke.sh` | PASS, `ffi smoke ok` | Real C header/client ABI smoke. |
| `cargo check -p gsplat-render-wgpu --target wasm32-unknown-unknown` | PASS | Renderer WASM compilation only. |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | PASS | Web binding compilation only. |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha` | PASS | Hardware-backed Apple M4 Metal SortedAlpha conformance. |
| `cargo test -p desktop-example --features interactive-viewer --test surface_geometry_entry` | PASS | Real hidden Apple M4 Metal Surface mechanics and one product Packed presentation. |

These results do not claim Chrome/WebGPU, physical-iPhone, Windows or Linux
runtime, nor image parity beyond the explicit conformance/Surface checks.

## Closeout integration boundary

This visible candidate owns only this audit and `progress.md`. The fixed source
tip still contains three M7 grandfather records in
`tests/architecture/source_architecture_policy.json`. They were valid when the
root matrix ran with M7 Active, but the `M7 = Accepted` registry transition
makes them expire with `grandfather.exit_due` for `lib.rs`,
`surface_presenter.rs` and `surface_session.rs`.

The root integration must co-land removal of exactly those now-satisfied M7
records and rerun the real-tree architecture policy before treating the
integrated closeout as green. This documentation task does not edit that test
policy because its ownership brief forbids code/test changes. The independent
fixed-SHA audit is the responsibility evidence for the removal; deleting the
records alone is not used as proof of the source exit.

## Android A065 retained artifact

Machine-local suite in the root evidence worktree:

```text
target/android-sort-benchmarks/verification-a065-2739f4899fac/suite.json
```

Fresh closeout revalidation from that evidence worktree passed:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 tests/perf/validate-full-quality-experiment.py \
  target/android-sort-benchmarks/verification-a065-2739f4899fac/suite.json \
  --verify-inputs

python3 tests/perf/validate-benchmark-artifacts.py \
  target/android-sort-benchmarks/verification-a065-2739f4899fac/\
run-001-pair-001-pos-01-cpu/artifact
```

Results:

```text
full-quality experiment valid: expected=1 rendered=1 capacity_rejected=0 missing=0
benchmark artifact valid
```

Identity and exactness facts:

- repository commit `2739f4899facc03e6a0fb35c23b42d9762ede9cc`,
  clean;
- Nothing A065, Android 15/API 35, Vulkan renderer backend; the artifact's
  adapter and driver strings remain unavailable rather than inferred;
- dataset SHA-256
  `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`;
- 279,199 source/decoded/encoded/resident/addressable splats, source/resident
  SH3, full membership, no sampling, LOD or partial publication;
- requested/Surface/internal/presented `2412x1080`, dynamic resolution and
  upscaling disabled;
- 10 warmup plus 20 measured CPU/PostSort frames, 20 successful order
  terminals and 20 matching Ready current-stats terminals;
- final device/native Surface PNG SHA-256
  `0c4ef40b1318b903e62a992a6c20611fd6b8a00c42a7c8a3eabf8eabc2faba27`;
- APK SHA-256
  `a33dfc46d516846464ffb81becc0d2d12b4cd24c2ed84f05b7502636a37f66ae`;
- APK/AAR native library SHA-256
  `7969ab6b1b99032559b20ea406c338ae3d865909820dd8d963db491758d09ce2`.

This one-policy formal run proves device function, complete membership,
presentation and strict terminal-ledger exactness for its recorded
configuration. Timing fields are observations only. It does not establish a
CPU/GPU winner, competitor comparison, sustained thermal, power, broad pixel
parity or general performance claim.

## Aggregate rollback proof

The isolated rollback audit first proved that `a798b8a` is an ancestor of
`2739f48`, the range is linear, and `git rev-list a798b8a..2739f48` contains
exactly 39 commits and no merge commits. It then reverse-applied that exact
newest-to-oldest list with `git revert --no-commit`.

The reconstructed state was exact:

```text
reverse count = 39
newest = 2739f4899facc03e6a0fb35c23b42d9762ede9cc
oldest = 707cd8001abf84cd9fc990dd45246a0c02db75ef
restored tree = fd13949e2237c96181b570671f23ecbfe408b1a1
baseline tree = fd13949e2237c96181b570671f23ecbfe408b1a1
index vs baseline = identical
worktree vs baseline = identical
```

`cargo check --workspace` and `bash tests/ffi/run-ffi-smoke.sh` passed on the
restored tree; the latter reported `ffi smoke ok`, `drawn=2 visible=2`. The
audit then restored detached HEAD, index and worktree to the clean accepted
tip and cleared the temporary sequencer state.

This proves aggregate rollback of the complete integrated M7 range to accepted
M6. It does not claim that arbitrary individual M7 commits are independently
revertible or that the restored tree received device/browser qualification.

## Deferred and non-claims after M7 acceptance

- Chrome/WebGPU runtime at `2739f48` remains Deferred until the exact locked
  `wasm-bindgen-cli 0.2.121` prerequisite is available.
- Physical-iPhone and Windows/Linux runtime remain Deferred; host/simulator,
  macOS or cross-compilation results are not substituted.
- M7 makes no new FPS, fixed-speedup, battery, thermal, memory-leadership,
  competitor-parity or broad cross-platform pixel claim.
- M8 remains unstarted. Its factual handbook/release alignment, final link and
  command audit, accepted-SHA ledger and migration-bundle archival are not
  performed or activated by this M7 closeout.
