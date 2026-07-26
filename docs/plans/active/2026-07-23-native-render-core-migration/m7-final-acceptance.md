# M7 final acceptance audit

## Decision and fixed identities

- Final decision: **Active, not Accepted**. The obsolete Packed/Tiled deletion,
  compatibility work, available-platform evidence and aggregate rollback are
  complete, but M7's source-ownership exit is not.
- Rollback base / accepted M6 tree:
  `a798b8accf454e96792df45d38e8564e7e27556e`.
- Integrated M7 behavior sequence: `a798b8a..3f52558`, nine linear commits
  beginning at `707cd80`.
- Current M7 behavior tip:
  `3f525584c1f3c5909ecd2f87b6a953b8a984e9ec`.
- Canonical-document reconciliation tip:
  `9ebe7e3a1a115cbac18ef10e3adac2c14e984609`.
- Aggregate rollback evidence commit, retained only in an isolated detached
  worktree: `6fdb4460f07cf9b7e7ca44983b6c884ed37e5987`.
- M8 remains unstarted and outside the machine task-state registry. Package M
  remains Active; this audit does not accept or activate M8.

The first closeout candidate incorrectly removed the M7 grandfather records
after deleting the old Packed graph, even though `lib.rs` and
`surface_session.rs` still contain substantial concrete ownership and the
literal exit conditions were not established. This audit corrects that claim.

The architecture checker treats physical LOC only as non-blocking legacy
evidence. While M7 is Active, its grandfather records remain valid and growth
is reported as a notice. If M7 is terminal while an owned record remains, the
checker emits `grandfather.exit_due`; deleting the record makes that error
disappear but does not prove the semantic exit. Therefore the records are
restored and M7 stays Active. No fixed line count is an acceptance condition.

## Requirement-by-requirement result

| M7 exit requirement | Decision | Evidence and boundary |
| --- | --- | --- |
| One controller, generation owner, mandatory sampler, plan-cache owner and terminal-result owner | **Open / blocks M7** | Product Packed has one route through `SurfaceRenderSession -> SurfacePresenterHost -> Renderer::PreparedRuntimeSlot`, and the obsolete Packed graph/Tiled runtime are absent. However the existing `lib.rs`, `surface_session.rs` and `surface_presenter.rs` grandfather exit conditions have not all been proved. Their records remain owned by Active M7. |
| Call-site/API/ABI classification | **Satisfied evidence** | The [M7 public call-site/API/ABI ledger](m7-public-callsite-api-abi-ledger.md) classifies every removed or retained Rust, C, JNI/Kotlin, Swift and Web entry. Published C symbols remain M3-owned compatibility shims. |
| No legacy/new runtime toggle or dual product default | **Satisfied evidence** | Standalone Presenter supports Direct/Paged and rejects Packed before allocation. Product Packed has one host/renderer route. Paged remains an explicit CPU-only diagnostic and rejects async configuration before mutation. |
| Workspace and source policy after deletion | **Pending final ownership candidate** | Root passed format, workspace check/test, strict Clippy, Rustdoc and architecture policy while M7 was Active. Render tests reported 421 passed and 8 ignored. The final ownership candidate must rerun policy after legitimately satisfying/removing the three M7 entries and marking M7 Accepted. |
| C ABI/FFI compatibility | **Satisfied evidence** | No public C layout/signature removal is part of the completed deletion range. The real C header/client smoke passed. |
| WASM compatibility | **Compile evidence available** | `cargo check -p gsplat-web --target wasm32-unknown-unknown` passed. This is not a browser or WebGPU runtime result. |
| macOS/Metal product mechanics | **Satisfied evidence** | Forced SortedAlpha Metal conformance passed. The public hidden-window Surface test reported Apple M4, standalone Packed rejection and successful product Packed host presentation. |
| Android final shared-core function and strict terminal ledger | **Satisfied evidence** | A clean `3f52558` APK/AAR/native library rendered all 279,199 Kitsune SH3 splats at 2412x1080 for 20 measured CPU trace frames. Every frame retained full membership and `V=D=279199`; contributor count alternated between 226,450 and 236,792. The full-quality suite passed with verified inputs and retained a final native Surface PNG. |
| Chrome/WebGPU runtime after deletion | **Deferred** | The exact locked `wasm-bindgen-cli 0.2.121` prerequisite is unavailable in the final root environment. WASM compile evidence is not promoted to browser evidence, and the older accepted M4 browser run is not relabelled as a final-M7 run. |
| Apple wrapper/device endpoints | **Preserved M6 evidence; physical iPhone Deferred** | The completed M7 range changes no Swift/C wrapper API and the shared core has fresh Metal evidence. M6's accepted Simulator/XCFramework scope remains intact. No final-M7 physical-iPhone run exists. |
| Windows and Linux runtime after deletion | **Deferred** | No final-M7 Windows or Linux runtime was executed. macOS compilation and WASM checks do not imply those backends. |
| Aggregate rollback | **Satisfied evidence** | All nine completed behavior commits were reverse-reverted without conflict in an isolated worktree. The resulting tree ID exactly equals `a798b8a`; `cargo check --workspace` and the C FFI smoke passed on that restored tree. |
| Repository hygiene | **Satisfied evidence** | The root source worktree was clean. APK, AAR, native library, model, logs and PNG remain ignored machine-local artifacts under `target/`; none is committed as source. |

## Final root verification ledger

The root task ran the following against clean behavior tip `3f52558` before the
docs-only reconciliation and this audit:

| Command | Result | Evidence class |
| --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | Rust formatting. |
| `cargo check --workspace` | PASS | Host workspace compile. |
| `cargo test --workspace` | PASS | Workspace behavior; render crate 421 passed, 8 ignored. |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | Strict all-target lint. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS | Public Rust documentation. |
| `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py` | PASS | Current ownership/source policy. |
| `bash tests/ffi/run-ffi-smoke.sh` | PASS, `ffi smoke ok` | Real C header/client ABI smoke. |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | PASS | WASM compilation only. |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha` | PASS | Hardware-backed Apple M4 Metal SortedAlpha conformance. |
| `cargo test -p desktop-example --features interactive-viewer --test surface_geometry_entry` | PASS | Public Metal Surface: Direct/Paged retained, standalone Packed rejected, product Packed host presented. |
| `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest tests/test_verification_bootstrap.py` | PASS: 8 | Read-only discovery, command generation and explicit device authorization. |

The canonical docs reconciliation at `9ebe7e3` changed documentation only.
Its edited relative links and diff hygiene passed. This acceptance audit
does not reinterpret those checks as runtime evidence.

## Adaptive-order ownership progress

This M7 slice satisfies one finite part of the legacy Session ownership exit:
the complete private CPU/GPU Adaptive order controller now lives in
`surface/adaptive_order.rs`. That owner contains the rolling estimator, CPU
bootstrap, repeated ABBA probes, pending-ticket matching, hysteresis,
GPU-failure cooldown/reprobe state, transitions and focused unit tests.

`surface_session.rs` now composes that controller. It still owns the actual
CPU/GPU sorting calls, telemetry submission and polling, current-stats and
compatibility-evidence publication, and arbitration between order probes and
the independent Projected Candidate/Compact controller. Public Rust re-export
paths and enum discriminants remain unchanged through the Session facade.

This is not M7 acceptance. The Projected control extraction described below is
now complete, but residual Session composition/publication, `lib.rs`
implementation ownership and the Presenter responsibility audit remain open;
all three M7 grandfather records therefore remain in force.

## Projected-draw Adaptive ownership progress

The next isolated M7 slice moves the complete private Candidate/Compact
Adaptive control state from `surface_session.rs` into
`surface/projected_adaptive.rs`. That owner now contains the public state and
pending-sample receipts, private CPU/GPU-order policy lanes, phase and sample
types, rolling p75 estimates, ABBA sequence, promotion hysteresis, cooldown /
reprobe transitions, and their focused unit tests.

`surface_session.rs` composes those lanes but deliberately retains the actual
Candidate/Compact plan execution, projected telemetry reservation and polling,
ticket/current-stats/evidence publication, and arbitration against the
independent `adaptive_order` controller. Public Rust re-export paths and enum
variant order remain unchanged through the Session facade. No ABI wrapper,
shader, GPU allocation, pass order, or pixel contract moves in this slice.

This remains an Active M7 ownership step, not acceptance. Concrete `lib.rs`
ownership, residual Session composition/publication ownership, and the
Presenter responsibility audit are still open; the three grandfather records
remain in force.

## Direct wide-f32 GPU scene ownership progress

The next isolated M7 slice moves the Direct compatibility path's complete GPU
scene resource closure from `lib.rs` into `direct_scene_gpu.rs`: source and SH
buffers, CPU/GPU order bindings, Direct GPU-order ownership, parameter upload,
and the matching bind-group layout and render pipeline construction.

Native offscreen and standalone Direct Surface rendering consume that same
private owner through narrow methods. They no longer reach through
`DirectSceneResources` or `DirectGpuSceneOrder` to access capacity, buffers,
bind groups, sorter state or indirect arguments. `lib.rs` retains public
renderer/facade wiring and the shared `make_surface_render_params`; the latter
also serves Resident/Packed consumers and is intentionally outside this
Direct-only slice. No crate-root re-export is introduced.

This is a behavior-preserving ownership move. It changes no public Rust/C/Web
or mobile surface, resource layout, shader, pass order, error scope,
publication boundary or pixel contract. M7 remains Active: other concrete
`lib.rs` responsibilities, residual Session composition/publication, and the
Presenter audit still require separate review.

## Android retained artifact

Machine-local suite:

```text
target/android-sort-benchmarks/m7-final-3f52558-a065/suite.json
```

Root revalidation command:

```bash
python3 tests/perf/validate-full-quality-experiment.py \
  target/android-sort-benchmarks/m7-final-3f52558-a065/suite.json \
  --verify-inputs
```

Result:

```text
full-quality experiment valid: expected=1 rendered=1 capacity_rejected=0 missing=0
```

Identity and exactness facts:

- repository commit `3f525584c1f3c5909ecd2f87b6a953b8a984e9ec`, clean;
- Nothing A065, Android 15/API 35, Snapdragon SM8475/Adreno Vulkan;
- dataset SHA-256
  `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`;
- 279,199 source/decoded/encoded/resident/addressable splats, source/resident
  SH3, no partial publication;
- requested/Surface/internal/presented `2412x1080`, dynamic resolution and
  upscaling disabled;
- 10 warmup plus 20 measured CPU/PostSort frames, strict same-ticket terminal
  joins, final-frame PNG SHA-256
  `0c4ef40b1318b903e62a992a6c20611fd6b8a00c42a7c8a3eabf8eabc2faba27`;
- APK SHA-256
  `ef39bb79b776fa6beea83aea5a85b14290d2a80a2ac9cf0d132f07f605a641ac`;
- APK/AAR native library SHA-256
  `6ef4a41315eb57d8e705a532c86558914cd4c427d6b604feebc1816868e9dc79`.

This short, single-policy run is functional, exactness, presentation and
terminal-ledger evidence. Its timing values are observations only. It cannot
support a CPU/GPU winner, PlayCanvas comparison, sustained thermal, power, or
general performance claim.

## Aggregate rollback proof

The isolated rollback task started at `3f52558` and applied one no-commit
reverse operation over this newest-to-oldest sequence:

```text
3f52558 fc90662 38bd608 a454719 432c430
048344f 6befc3b 08bd5bf 707cd80
```

It then committed the evidence tree as `6fdb446`, verified
`git diff --exit-code a798b8a HEAD`, and compared tree objects:

```text
rollback tree = fd13949e2237c96181b570671f23ecbfe408b1a1
baseline tree = fd13949e2237c96181b570671f23ecbfe408b1a1
```

`cargo check --workspace` and `bash tests/ffi/run-ffi-smoke.sh` both passed on
the restored tree. The evidence branch reference was removed and the root
worktree remained on the clean M7 tip. This proves supported aggregate rollback
to M6; it does not claim that arbitrary individual M7 commits can be reverted
out of order.

## Finite next M7 ownership slice

One independently reviewed M7 candidate must:

1. continue removing remaining concrete renderer ownership from `lib.rs` so it
   is crate wiring, public facade/re-export and compatibility entrypoints rather
   than a second implementation owner; the shared Direct GPU scene resource
   extraction described above is already satisfied;
2. review and resolve the residual `surface_session.rs` composition,
   publication and host-coordination ownership. The CPU/GPU order and
   Candidate/Compact Projected Adaptive controller extractions described above
   are already satisfied; actual plan execution, telemetry polling, evidence
   publication and cross-controller arbitration intentionally remain Session
   responsibilities in those isolated slices;
3. prove `surface_presenter.rs` now contains only adapter/presentation-host
   responsibilities, or extract any residual semantic owner;
4. preserve public Rust/C/Web/mobile compatibility and exact render behavior;
5. only after those responsibility conditions pass review, remove the three
   grandfather entries, mark M7 Accepted, and run architecture self-tests,
   real-tree policy, focused compatibility tests and the applicable global
   matrix.

This is a responsibility-bound task, not a request to reach a numeric file
length. Failure to prove any one item leaves M7 Active; it does not trigger
repeated tuning or platform experiments.

## Deferred and non-claims while M7 remains Active

- Chrome/WebGPU runtime at the final M7 SHA awaits the pinned wasm-bindgen CLI.
- Physical-iPhone function/performance remains Deferred; Simulator evidence is
  not substituted.
- Windows and Linux runtime remain Deferred; no cross-compilation result is
  presented as hardware proof.
- M7 makes no new FPS, fixed-speedup, battery, thermal, memory-leadership or
  competitor-parity claim.
- M8 remains unstarted. It owns final handbook/release alignment,
  link/command audit, accepted-SHA ledger and migration-bundle archival only
  after M7's ownership exit is genuinely accepted.
