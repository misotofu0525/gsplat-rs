# Progress Log: Render and Paging Architecture Convergence

## 2026-07-18 — Phase 0 Audit and Baseline

- **Status:** complete
- **Starting state:** clean `refactor/packed-atlas-d-reset` at `3150b7b`.
- Actions completed:
  - Created the thread goal before other task actions.
  - Read all canonical project docs required by `AGENTS.md`.
  - Read the active Phase D-F task/verification plan and relevant design,
    findings, and progress sections.
  - Recovered the original Direct-default/oversized-paged intent and the known
    bounded-prototype limitations.
  - Captured branch history, main diff shape, renderer module sizes, public
    item map, and crate dependency tree.
  - Ran current and detached-main Direct checks, conformance, benchmark, and
    exact PNG comparison; then removed the temporary worktree.
  - Froze S1-S5 with independent rollback and verification boundaries.
- Files created:
  - `docs/plans/active/2026-07-18-render-paging-architecture-convergence/task_plan.md`
  - `docs/plans/active/2026-07-18-render-paging-architecture-convergence/findings.md`
  - `docs/plans/active/2026-07-18-render-paging-architecture-convergence/progress.md`

## Test Results

| Test | Expected | Actual | Status |
|------|----------|--------|--------|
| Initial git state | clean reset branch at `3150b7b` | matched | pass |
| Current workspace check | `cargo check --workspace` | passed | pass |
| Current renderer lib suite | Direct/Packed/Paged and safety gates pass | 93 passed, 1 ignored | pass |
| Current SortedAlpha conformance | required GPU path passes | 1 passed on Metal | pass |
| Current minimal Direct benchmark | direct pipeline, non-zero draw, no budget miss | p95 GPU-complete 2.6283 ms; 0 misses | pass |
| Fresh main workspace check | `cargo check --workspace` | passed in detached worktree | pass |
| Fresh main SortedAlpha conformance | required GPU path passes | 1 passed on Metal | pass |
| Main/current minimal PNG | same Direct count and image | identical count and SHA-256 | pass |
| Main/current minimal benchmark | no obvious Direct regression | mean GPU-complete 1.9003/1.8189 ms | observation |
| S1 workspace check | Surface move compiles across consumers | passed | pass |
| S1 renderer lib suite | all path/safety/image gates remain green | 93 passed, 1 ignored | pass |
| S1 SortedAlpha conformance | required GPU path remains correct | 1 passed on Metal | pass |
| S1 FFI smoke | public C consumer renders | `drawn=2 visible=2` | pass |
| S1 hygiene | fmt, strict renderer clippy, diff check | passed | pass |
| S2 workspace check | shared owner compiles across consumers | passed | pass |
| S2 renderer lib suite | parity, safety, Surface local gate | 93 passed, 1 ignored | pass |
| S2 SortedAlpha conformance | global quality path unchanged | 1 passed on Metal | pass |
| S2 hygiene | fmt, strict renderer clippy, diff check | passed | pass |
| S3 renderer lib suite | policy plus all existing path/safety gates pass | 95 passed, 1 ignored | pass |
| S3 SortedAlpha conformance | global quality path unchanged | 1 passed on Metal | pass |
| S3 FFI smoke | stable C consumer remains Direct and non-zero | `drawn=2 visible=2` | pass |
| S3 Web check | additive canvas API compiles for wasm32 | passed; existing cfg-only warnings | pass |
| S3 public docs | new Rust API links and safety docs are valid | `-D warnings` passed | pass |
| S3 hygiene | workspace, fmt, strict renderer clippy, diff check | passed | pass |
| S4 payload equivalence | source output matches existing shared encoding and LOD rules | passed | pass |
| S4 renderer lib suite | decoded-payload path preserves all parity/safety gates | 96 passed, 1 ignored | pass |
| S4 SortedAlpha conformance | global quality path unchanged | 1 passed on Metal | pass |
| S4 Surface/paging safety | local Surface, stale/cancel/generation/nonresident gates | passed in full suite | pass |
| S4 FFI smoke | stable C consumer remains non-zero | `drawn=2 visible=2` | pass |
| S4 Web check | source/payload boundary compiles for wasm32 | passed; existing cfg-only warnings | pass |
| S4 hygiene | workspace, fmt, strict renderer clippy, diff check | passed | pass |
| S5 full workspace tests | all crates and doctests pass | passed; renderer 96 passed, 1 retained ignored | historical pass; final rerun pending |
| S5 required SortedAlpha | GPU-required conformance remains correct | 1 passed on Apple M4 Pro / Metal | pass |
| S5 native Direct PNG | final counts/image equal baseline | `visible=2`, `drawn=2`, identical SHA-256 | pass |
| S5 native Direct benchmark | Direct path, non-zero draw, no miss | mean 1.8581 ms; p95 3.1114 ms; 0 misses | observation |
| S5 Android real model | physical-device Direct/Paged runs completed before final commit | Direct 14.969 ms; Paged 36.587 ms | historical observation; provenance gap |
| S5 iOS available route | simulator Direct Surface run completed before final commit | 18.992 ms; 279199 visible/drawn | historical simulator observation; provenance gap |
| S5 Web | earlier wasm build/check/tests and browser Surface | 7 tests; `wasm_sorted_index_direct`, 3 visible/drawn | historical pass; ignored output predates final commit |
| S5 hygiene | fmt, workspace check, strict clippy, rustdoc, diff check | passed | pass |
| B red regression | synthetic adapter/requested storage mismatch fails before fix | exact test failed Direct vs Paged | expected red |
| B renderer lib | effective-limits fix preserves path/safety/image tests | 97 passed, 1 retained ignored | pass |
| B SortedAlpha | required native GPU conformance | 1 passed on Apple M4 Pro / Metal | pass |
| B workspace/hygiene | workspace check, strict renderer clippy, fmt, diff | passed | pass |
| C source/payload validation | missing, bounds, count, capacity, encoding, sidecar typed gates | 3 focused pure tests passed | pass |
| C invalid GPU payload | reject before GPU write/active publication | active entries remained empty | pass |
| C renderer/SortedAlpha | full renderer plus required Metal conformance | 100 passed, 1 ignored; conformance 1 passed | pass |
| C workspace/platform hygiene | workspace, strict clippy, wasm32, fmt, diff | passed; existing wasm cfg warnings only | pass |
| D1 renderer/SortedAlpha | shared draw and Surface ownership preserve every path | 100 passed, 1 ignored; conformance 1 passed on Metal | pass |
| D1 workspace/platform | workspace, strict clippy, wasm32, FFI, fmt, diff | passed; FFI `drawn=2 visible=2` | pass |
| D1 production size | remove duplication and retain tests | 7,978 -> 7,866 production; tests remain 3,129 | pass for slice; D gate open |
| D2 renderer/SortedAlpha | shared construction preserves Direct/Packed/Paged images | 100 passed, 1 ignored; conformance 1 passed on Metal | pass |
| D2 workspace/platform | workspace, strict clippy, wasm32, FFI, fmt, diff | passed; FFI `drawn=2 visible=2` | pass |
| D2 production size | remove construction/dead state and retain tests | 7,866 -> 7,769 production; tests remain 3,129 | pass for slice; D gate open |
| D3 renderer/SortedAlpha | compact color/validation paths preserve all renders | 100 passed, 1 ignored; conformance 1 passed on Metal | pass |
| D3 workspace/platform | workspace, strict clippy, wasm32, FFI, fmt, diff | passed; FFI `drawn=2 visible=2` | pass |
| D3 production size | retain only abstractions that delete code | 7,769 -> 7,732 production; tests remain 3,129 | pass for slice; D gate open |
| D4 renderer/SortedAlpha | direct errors and single geometry dispatch preserve behavior | 100 passed, 1 ignored; conformance 1 passed on Metal | pass |
| D4 workspace/platform | workspace, strict clippy, wasm32, FFI, fmt, diff | passed; FFI `drawn=2 visible=2` | pass |
| D final production size | finish below 7,621 and retain D tests | 7,607 production; tests remain 3,129 | pass |
| E consumer boundary | connect without changing Direct/API, or record blocker | every real SDK needs a new Auto option/ABI value | accepted API blocker; no code change |

## 2026-07-18 — S1 Surface Ownership Split

- **Status:** complete
- Moved Surface presentation and resource planning from `lib.rs` into one
  sibling module with the same crate-root `SurfacePresenter` export.
- Unified initial and switched geometry resource construction.
- Code-size result: one new file, root -875 lines, combined renderer Rust -6
  lines.
- Next: S2 shared paged active-set owner; no policy/source behavior changes yet.

## 2026-07-18 — S2 Shared Paged Active Set

- **Status:** complete
- Replaced Surface/offscreen atlas-residency setup with one internal
  `PagedActiveSet` owner while keeping public paging types intact.
- Fresh verification: workspace check, 93 renderer tests, required Metal
  conformance, strict clippy, fmt, and diff hygiene all passed.
- Code-size result: one 105-line file, net +26 renderer Rust lines from S1;
  terminal net deletion remains open.
- Next: S3 Direct-first automatic policy seam.

## 2026-07-18 — S3 Direct-First Automatic Policy

- **Status:** complete
- Added an explicit request policy whose default remains Direct; automatic
  selection is opt-in through new Surface constructors.
- Automatic construction waits for compatible-adapter limits, uses the
  existing structured Direct preflight, and selects Paged only for
  `ActiveAtlasRequired`.
- Failed automatic preparation restores the renderer's prior geometry path.
  Existing constructors, explicit diagnostic paths, and the C ABI are
  unchanged.
- Fresh verification: workspace check; 95 renderer tests plus one retained
  ignored oracle; required Metal conformance; FFI smoke; wasm32 check; strict
  renderer clippy; rustdoc with warnings denied; formatting and diff hygiene.
- Next: S4 local page-source/payload boundary.

## 2026-07-18 — S4 Local Page-Source Boundary

- **Status:** complete
- Added one private `page_source.rs` module. `LocalScenePageSource` explicitly
  borrows the full in-memory scene and page metadata, performs page extraction,
  shared-range packing, and attribute-LOD reduction, and returns a decoded
  payload with stable source indices.
- `PagedAtlasGpu` now has an internal decoded-payload upload path. Existing
  public scene/page upload methods remain unchanged compatibility wrappers.
- The shared active set schedules against source metadata and sends one
  transient payload at a time to fixed GPU slots. No source/decoded cache was
  added; the current adapter remains unbounded at source residency and is not
  described as streaming.
- Fresh verification: workspace check; payload equivalence; 96 renderer tests
  plus one retained ignored oracle; required Metal conformance; local Surface,
  parity and safety gates; FFI smoke; wasm32 check; strict clippy; fmt/diff.
- Code-size result: one 166-line file and +201 renderer Rust lines from S3.
  Final source is currently 11,296 lines versus the 10,796-line start, so S5
  must recover at least 501 lines to satisfy terminal net deletion.
- Next: S5 proven cleanup and final regression.

## 2026-07-18 — S5 Cleanup, Documentation, and Platform Regression

- **Status:** module slice complete; overall acceptance invalidated by later audit
- Unified renderer timing/stat/raster logic, Surface constructor setup, and
  repeated test fixtures. The four touched test modules retain identical test
  counts before/after, and all original image/safety thresholds remain.
- Reconciled project context, architecture, roadmap, and golden principles with
  the implemented Direct/default, opt-in oversized/Paged, and explicit
  diagnostic Packed boundary.
- The slice produced workspace, Metal, PNG, FFI, Web, Android, and simulator
  observations, but Web/mobile binaries and raw logs were not reliably bound to
  final `eb12e68`; they are historical evidence only and must be repeated in F.
- Code-size result: aggregate source is 10,792 versus 10,796, but the corrected
  production-only measure is 7,821 versus 7,621 (+200), while tests/fixtures
  fell from 3,175 to 2,971 (-204). Production cleanup remains open.
- No platform-specific fix was required, no public baseline API/C ABI changed,
  and no push was performed.

## 2026-07-18 — Independent Audit Reopens Overall Acceptance

- **Status:** corrective A complete in this docs-only boundary; overall
  architecture convergence `in_progress`; B is the sole current blocker.
- Confirmed a clean `refactor/packed-atlas-d-reset@eb12e68`; preserved all five
  verified S1-S5 commits and did not switch to the old expanded branch.
- Recorded the corrected production/test split, synchronous full-source page
  contract, 1,011-line Surface responsibility hotspot, absent automatic
  production consumer, effective-device-limits defect, missing payload bounds,
  and final-evidence provenance gaps.
- Froze A-F in strict sequence. No code, API, C ABI, generated artifact, or
  platform binary changes belong to A.
- Next: B starts with a failing pure logic regression for adapter limits above
  the requested downlevel storage limit, followed by the smallest effective
  device-limits selection fix and focused verification.

## 2026-07-18 — B Auto Effective-Device-Limits Audit

- **Status:** complete; independently verified code/docs commit boundary.
- Confirmed the mismatch in code: Auto preflight and resource planning consume
  full adapter limits, while `request_device` starts from downlevel defaults
  and raises only the required 2D texture dimension.
- Chosen minimal boundary: one private effective-limits helper shared by Auto
  selection, resource planning, and device request. Public selectors,
  constructors, path defaults, and C ABI remain unchanged.
- Red proof: the module-qualified pure logic regression ran exactly one test
  and failed with `left: SortedIndexDirect`, `right: PagedActiveAtlas` for a
  3,000,000-splat degree-0 scene on a synthetic 256 MiB adapter, reproducing the
  adapter/requested-storage mismatch before the fix.
- Green focused proof: the same test now confirms adapter preflight is Direct,
  effective requested-device preflight is `ActiveAtlasRequired`, and Auto
  selects Paged. Full B verification is still pending.
- First full run reached 97 passed / 1 ignored renderer tests, required Metal
  conformance pass, and workspace check pass. Strict clippy then rejected the
  newly eight-argument private constructor; no later chained hygiene command is
  counted yet. The fix is a private adapter context, not a lint suppression.
- Final full rerun passed renderer 97/1, required Metal conformance, workspace
  check, strict clippy, formatting, and diff hygiene. No public API/C ABI or
  stable Direct default changed.
- Production accounting after B is 7,864 lines with 3,003 test/fixture lines;
  no file was added. C is now the only current blocker.

## 2026-07-18 — C Page Boundary Audit

- **Status:** complete; independently verified code/docs commit boundary.
- Confirmed missing pages are untyped `Option`, malformed local page indices can
  reach extraction before a bounds check, the atlas lacks source scene length,
  and decoded payloads carry no encoding identity.
- Public `PagedGpuError`, `PagedAtlasGpu` methods, and C ABI will remain stable.
  C uses private typed errors and maps them only at the existing public error
  boundary.
- Focused proof passed three source/payload tests plus a GPU-backed rejection
  test. Missing pages and malformed local indices are typed, count/capacity/
  source bounds/payload encoding/packed encoding/sidecars are validated, and an
  invalid payload leaves the active draw set empty.
- Full rerun passed renderer 100/1, required Metal conformance, workspace
  check, strict clippy, wasm32 Web check, formatting, and diff hygiene.
- Production accounting is now 7,978 lines and test/fixture accounting 3,129;
  no file or public API/C ABI surface was added. D is the sole current blocker
  and must remove at least 358 production lines.

## 2026-07-18 — D Production Cleanup

- **Status:** complete after D4 verification; E automatic-consumer boundary is
  the sole next blocker after the D4 commit.
- Baseline for D is 7,978 production / 3,129 test-fixture lines. Acceptance is
  7,620 production lines or fewer with all tests retained.
- Six duplicated render-pass bodies and the three-Option-plus-path Surface
  resource invariant are the first proven deletion targets. No code move alone
  will count toward acceptance.
- D1's first verification reached 100 renderer tests plus one retained ignored
  oracle, required Metal conformance, and workspace check. Strict clippy then
  rejected the large size gap in the single-active geometry enum; those earlier
  passes are not the final D1 evidence. The private Paged runtime alone is now
  boxed, and every D1 gate must rerun.
- Final D1 rerun passed renderer 100/1, required Metal conformance, workspace
  check, strict clippy, wasm32, FFI smoke, formatting, and diff hygiene.
  Production code is 7,866 lines and tests remain 3,129; Surface production is
  918 lines. D still needs at least 246 production-line deletions.
- D2 final verification passed the same renderer/Metal/workspace/clippy/wasm/
  FFI/hygiene gates. Shared pipeline/layout construction plus dead private
  Packed state moved production to 7,769 while tests remain 3,129. D3 audit is
  now the sole next action; at least 149 production lines remain.
- D3 discarded two abstractions that did not reduce code, then passed the full
  renderer/Metal/workspace/clippy/wasm/FFI/hygiene sequence with the remaining
  compact color and validation changes. Production is 7,732, tests are 3,129,
  and D4 must remove at least 112 more production lines.
- D4 removed the duplicate offscreen error facade and double Surface geometry
  dispatch, then passed the full renderer/Metal/workspace/clippy/wasm/FFI/
  hygiene sequence. Final D accounting is 7,607 production / 3,129 tests:
  production is 14 lines below `3150b7b`, and D retained its complete test
  corpus. Overall architecture convergence remains in progress at E.

## 2026-07-18 — E Automatic Consumer Boundary

- **Status:** complete as the explicitly permitted API-boundary blocker; F
  final HEAD-bound proof is the sole next blocker.
- Read Web/WASM, C ABI, Android, and Apple construction code and SDK READMEs.
  Each scene is available before Surface construction, but every public option
  names Direct/Packed/Paged only and documents Direct as the default.
- Reusing the Rust Auto constructors would require a new JS/WASM option/export
  or C enum/function propagated into Kotlin and Swift. No existing private
  opt-in consumer can request Auto without changing product semantics.
- No source, header, generated binding, C ABI, default path, or binary changed.
  The review boundary is recorded for a future product/API decision.

## 2026-07-18 — F Final-Evidence Harness

- **Status:** locally executable candidate proof complete; Android device
  Direct/Paged is the remaining hardware blocker. Exact final-HEAD rerun follows
  the no-code evidence-record commit.
- Added an example-only `--geometry-path direct|packed|paged` switch to the
  desktop PNG runner. The default remains Direct, the runner reports the actual
  offscreen pipeline, and focused CLI tests cover both the default and explicit
  Paged selection. No renderer API, SDK option, C ABI, or product default
  changed.
- Fresh harness verification passed six desktop CLI tests, workspace check,
  strict desktop clippy, formatting, and diff hygiene before commit
  `bf756e3`.
- A current Paged probe loaded the 279,199-splat Kitsune scene through
  `paged_active_atlas` at 640x360 with the fixed auto camera and zero yaw. It
  emitted a PNG and reported `visible_count=drawn_count=225784`, proving the
  chosen fixture exceeds the four-slot 262,144 source capacity while retaining
  a nonzero active set.
- Baseline proof will use a disposable `3150b7b` worktree with only `bf756e3`
  applied and an empty renderer diff. Final proof will rerun the identical
  command, compare PNG SHA-256 plus SSIM, and retain commit-tagged raw text.
- `adb devices -l` currently exposes no device. Android can still be rebuilt
  and host-smoked from the final HEAD, but Direct/Paged device acceptance will
  remain partial unless a target appears. A booted iPhone 17 Pro simulator is
  available for real Surface Direct/Paged presentation evidence.
- Candidate-head core proof passed workspace tests, 100/1 renderer tests,
  required Metal conformance, workspace check, strict clippy, rustdoc, format,
  and diff hygiene. Focused parity, stale/cancel/generation/nonresident,
  continuity, and payload-boundary suites all passed; FFI smoke drew the same
  two visible splats.
- Web candidate proof passed wasm32 check, seven package tests, dry-run pack,
  release WASM/SDK build, syntax checks, and output hashing. Android host JNI
  smoke and a Kitsune sample APK build passed, but the empty device list means
  neither Direct nor Paged Surface executed on Android.
- The iPhone 17 Pro simulator presented 120 measured Kitsune frames on both
  paths. Direct reported `sorted_index_direct` and 279,199 visible/drawn;
  Paged reported `paged_active_atlas`, 279,199 visible, and 225,784 drawn.
  Paged averaged 98.752 ms/frame versus Direct 18.186 ms/frame; this is an
  honest observation, not a performance pass.
- The disposable `3150b7b` worktree had no renderer diff after applying only
  the CLI harness. Baseline and candidate both reported 225,784 visible/drawn,
  emitted the identical PNG SHA-256
  `eb41d85b6e93f8b5682fd4f7d6c7791e1a65fb482a1ee512cc196bd14c7bbffb`,
  zero absolute-error pixels, and repository SSIM 1.0 over 3,600 windows.
- Production accounting reran from Git objects: `3150b7b` is 7,621 production
  plus 3,175 test/fixture lines; the candidate is 7,607 plus 3,129. The cleanup
  gate therefore remains a 14-line production reduction without test deletion
  during D.

## 2026-07-18 — F Device Refresh After Exact-HEAD Proof

- **Status:** partial. Android emulator evidence expanded, but the physical
  device gate remains open and overall architecture convergence is still
  `in_progress`.
- Exact-HEAD proof at `4a8863a` already passed workspace/renderer/required
  Metal/clippy/rustdoc/FFI/Web, iOS simulator Direct/Paged, production recount,
  and baseline/final over-slot PNG parity. This refresh does not replace those
  logs or turn ignored binaries into provenance.
- A cold-started API 36 arm64 AVD on gfxstream/SwiftShader used a fresh
  temporary data image. A final-code minimal fixture completed 30 measured
  frames on Direct (`avg_visible=avg_drawn=3`) and Paged
  (`avg_loaded_source=avg_drawn=3`).
- The same final code and AVD completed Kitsune Paged for 30 measured frames:
  `geometry_pipeline=paged_active_atlas`, `avg_loaded_source=279199`, and
  `avg_drawn=225784`. The emulator averaged 3,364 ms/frame; this is a slow
  compatibility observation, not a performance pass.
- Kitsune Direct reproducibly SIGSEGVs during `DirectSceneResources::new`.
  Symbolization and disassembly place the failing call at the 50,255,820-byte
  degree-3 SH-rest `create_buffer_init`, after the smaller params/source calls.
  A disposable `3150b7b` APK failed on the same AVD with the same signal and
  fault address, so the emulator failure is not introduced by this refactor.
  It still cannot be reported as a final Direct device pass.
- A paired physical iPhone was discovered and the current app built, signed,
  and installed. The first CoreDevice attempt disconnected; the retry acquired
  a tunnel but SpringBoard rejected launch because the device was locked. No
  physical-iOS Direct/Paged renderer result exists.
- The temporary AVD data image and detached baseline worktree were removed.
  The primary worktree remained clean at `4a8863a` before this docs update.

## Error Log

| Error | Attempt | Resolution |
|-------|---------|------------|
| S1 first compile: unresolved root import for moved transaction helper | 1 | Removed the stale import and reran the focused check. |
| S1 first format check: import order drift | 1 | Applied the exact rustfmt ordering. |
| S1 post-dedup format check: one call wrapping difference | 1 | Applied rustfmt's requested compaction and reran hygiene. |
| S1 completion-record patch context mismatch | 1 | Re-read the active files and applied smaller exact patches. |
| S2 first test compile: old paged field names in fixtures | 1 | Routed assertions through the new shared owner and reran test compilation. |
| S2 first format check: standard ordering/wrapping drift | 1 | Applied rustfmt's requested layout before verification. |
| S2 completion-record patch context mismatch | 1 | Re-read the active files and applied smaller exact patches. |
| S3 first format check: one auto-selection call wrap | 1 | Applied the standard layout and continued with policy proof. |
| S3 focused-test command rejected a second test filter | 1 | Ran the two tests as separate Cargo commands; both passed. |
| S4 audit search included a guessed `paged_atlas_gpu.rs` file | 1 | Inspected the actual `paged_gpu.rs` found by repository search. |
| S4 first format check found import/wrapping drift | 1 | Applied rustfmt before the focused compile and tests. |
| S4 FFI discovery search included a missing `scripts/` root | 1 | Followed the canonical handbook command under `tests/ffi/`. |
| S5 duplicate-search used a backreference unsupported by default `rg` regex | 1 | Re-ran with `rg --pcre2` and used the successful result only. |
| First iOS simulator run did not preserve app stdout | 1 | Used the rebuilt installed app with `simctl --console` to capture the benchmark. |
| Browser documentation exceeded one response | 1 | Read the full 40,171-character contract in seven bounded chunks before navigation. |
| Aggregate `10,796 -> 10,792` was used as an overall cleanup gate | 1 | Independent audit split production from terminal test modules and found production `7,621 -> 7,821`; overall completion was withdrawn. |
| Platform observations were called final without commit-tagged raw provenance | 1 | Reclassified them as historical and added final-HEAD manifests/logs plus baseline/final over-slot comparison to F. |
| First B focused-test command matched zero tests because `--exact` received an unqualified name | 1 | Rerun with `surface_presenter::tests::automatic_selection_uses_effective_requested_storage_limits`; zero-test output is not evidence. |
| First B fix patch used stale shortened context around the texture-limit assignment | 1 | No code hunk applied; split the fix into exact smaller patches. |
| B format check found one non-canonical wrapped test call | 1 | Applied standard rustfmt before full verification. |
| First full B verification failed strict clippy on `too_many_arguments` after earlier gates passed | 1 | Replace three adapter-related parameters with one private context, then rerun every B gate. |
| First D1 full verification failed strict clippy on `large_enum_variant` | 1 | Box only the private Paged runtime variant, then rerun every D1 gate from renderer tests. |
| First D2 per-revision line-count shell used zsh's reserved `path` parameter as a loop variable | 1 | The temporary shell lost command lookup only; use task-specific `source_file` and rerun before making deletion decisions. |
| First F provenance manifest passed `head=HEAD` as a revision | 1 | Git rejected it without changing state; regenerated the log with the full HEAD SHA and retained only the corrected manifest. |
| First final-evidence record patch used a stale error-table context | 1 | No file changed in that failed patch; re-read exact sections and applied smaller hunks. |
| Guessed `cmdline-tools/latest/bin/avdmanager` was absent | 1 | Located the installed emulator and existing AVD from the SDK/AVD files instead of inventing a tool path. |
| Existing AVD quickboot had stale package-manager launcher state | 1 | A non-destructive cold boot restored launcher resolution. |
| Existing AVD data partition could not stage the APK | 1 | Used a disposable fresh data image with 5 GiB free; the original AVD data was not wiped. |
| First baseline Android build set `CARGO_TARGET_DIR`, but the repo script requires its root-local archive path | 1 | Re-ran unmodified in the disposable worktree and retained the failed attempt in the raw build log. |
| First physical-iOS wrapper used zsh's read-only `status` variable after the run | 1 | Inspected the preserved raw log, then used a task-specific `rc` variable on retry. |
| Physical-iOS benchmark could not launch while the paired phone was locked | 2 | Retain build/install and CoreDevice errors as partial evidence; require an unlocked rerun before any device claim. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Exact-HEAD local proof is complete; refreshed Android is partial and physical device acceptance remains open |
| Where am I going? | Rerun physical Android Direct/Paged and unlocked physical iOS, then freeze the true final evidence HEAD |
| What's the goal? | Restore a clear Direct/default vs oversized/Paged architecture while reducing proven waste |
| What have I learned? | See `findings.md` |
| What have I done? | Preserved S1-S5, completed corrective A-E, proved local F routes, isolated an AVD Direct failure as baseline-equivalent, and kept overall acceptance open for physical devices |
