# M7 final acceptance closeout

## Decision and fixed identities

- Final state: **Accepted** for M7's legacy-owner deletion and compatibility
  boundary.
- Rollback base / accepted M6 tree:
  `a798b8accf454e96792df45d38e8564e7e27556e`.
- Integrated M7 behavior sequence: `a798b8a..3f52558`, nine linear commits
  beginning at `707cd80`.
- Final M7 behavior tip:
  `3f525584c1f3c5909ecd2f87b6a953b8a984e9ec`.
- Canonical-document reconciliation tip:
  `9ebe7e3a1a115cbac18ef10e3adac2c14e984609`.
- Aggregate rollback evidence commit, retained only in an isolated detached
  worktree: `6fdb4460f07cf9b7e7ca44983b6c884ed37e5987`.
- The closeout commit that adds this record cannot contain its own object ID.
  Its exact SHA is reported by the isolated task and recorded by the next
  root-owned integration/M8 ledger step.
- M8 remains unstarted and outside the machine task-state registry. Package M
  therefore remains Active; accepting M7 does not accept or activate M8.

M7 acceptance is finite. It proves the deletion, ownership, compatibility,
available-platform and rollback requirements in the
[M7 cutover contract](cutover.md#m7--delete-legacy-owners-and-obsolete-experiments).
It does not convert unavailable endpoints into successes and does not make a
performance or competitor-leadership claim.

## Requirement-by-requirement result

| M7 exit requirement | Decision | Evidence and boundary |
| --- | --- | --- |
| One controller, generation owner, mandatory sampler, plan-cache owner and terminal-result owner | **Accepted** | Product Packed is `SurfaceRenderSession -> SurfacePresenterHost -> Renderer::PreparedRuntimeSlot`. The obsolete standalone Packed semantic graph and TiledExact runtime are absent. The three temporary M7 grandfather records for `lib.rs`, `surface_presenter.rs` and `surface_session.rs` are removed at closeout; architecture policy and focused ownership tests pass. This is a responsibility/ownership result, not a fixed line-count claim. |
| Call-site/API/ABI classification | **Accepted** | The [M7 public call-site/API/ABI ledger](m7-public-callsite-api-abi-ledger.md) classifies every removed or retained Rust, C, JNI/Kotlin, Swift and Web entry. Published C symbols remain M3-owned compatibility shims. |
| No legacy/new runtime toggle or dual product default | **Accepted** | Standalone Presenter supports Direct/Paged and rejects Packed before allocation. Product Packed has one host/renderer route. Paged remains an explicit CPU-only diagnostic and rejects async configuration before mutation. |
| Workspace and source policy after deletion | **Accepted** | Root passed format, workspace check/test, strict Clippy, Rustdoc and the real-tree architecture policy on the final behavior tip. Render tests reported 421 passed and 8 explicitly ignored research/external-asset cases. |
| C ABI/FFI compatibility | **Accepted** | No public C layout/signature removal is part of M7. The real C header/client smoke passed after deletion. |
| WASM compatibility | **Accepted as compile evidence** | `cargo check -p gsplat-web --target wasm32-unknown-unknown` passed. This is not a browser or WebGPU runtime result. |
| macOS/Metal product mechanics | **Accepted** | Forced SortedAlpha Metal conformance passed. The public hidden-window Surface test reported Apple M4, standalone Packed rejection and successful product Packed host presentation. |
| Android final shared-core function and strict terminal ledger | **Accepted** | A clean `3f52558` APK/AAR/native library rendered all 279,199 Kitsune SH3 splats at 2412x1080 for 20 measured CPU trace frames. Every frame retained full membership and `V=D=279199`; contributor count alternated between 226,450 and 236,792. The full-quality suite passed with verified inputs and retained a final native Surface PNG. |
| Chrome/WebGPU runtime after deletion | **Deferred** | The exact locked `wasm-bindgen-cli 0.2.121` prerequisite is unavailable in the final root environment. WASM compile evidence is not promoted to browser evidence, and the older accepted M4 browser run is not relabelled as a final-M7 run. |
| Apple wrapper/device endpoints | **Accepted for preserved M6 source/functional scope; physical iPhone Deferred** | M7 changes no Swift/C wrapper API and the final shared core has fresh Metal evidence. M6's accepted Simulator/XCFramework scope remains intact. No final-M7 physical-iPhone run exists. |
| Windows and Linux runtime after deletion | **Deferred** | No final-M7 Windows or Linux runtime was executed. macOS compilation and WASM checks do not imply those backends. |
| Aggregate rollback | **Accepted** | All nine M7 commits were reverse-reverted without conflict in an isolated worktree. The resulting tree ID exactly equals `a798b8a`; `cargo check --workspace` and the C FFI smoke passed on that restored tree. |
| Repository hygiene | **Accepted** | The root source worktree was clean. APK, AAR, native library, model, logs and PNG remain ignored machine-local artifacts under `target/`; none is committed as source. |

## Final root verification ledger

The root task ran the following against clean behavior tip `3f52558` before the
docs-only reconciliation and acceptance record:

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
Its edited relative links and diff hygiene passed. This acceptance closeout
does not reinterpret those checks as runtime evidence.

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

## Deferred and non-claims carried into M8

- Chrome/WebGPU runtime at the final M7 SHA awaits the pinned wasm-bindgen CLI.
- Physical-iPhone function/performance remains Deferred; Simulator evidence is
  not substituted.
- Windows and Linux runtime remain Deferred; no cross-compilation result is
  presented as hardware proof.
- M7 makes no new FPS, fixed-speedup, battery, thermal, memory-leadership or
  competitor-parity claim.
- M8 owns final handbook/release alignment, link/command audit, accepted-SHA
  ledger and migration-bundle archival. Accepting M7 neither performs nor
  pre-accepts that work.
