# M7 standalone Packed public-entry closeout

## Disposition

- Candidate-local disposition: Accepted for this isolated entry-admission
  slice. This note does not accept M7 or activate M8.
- Fixed parent: `707cd8001abf84cd9fc990dd45246a0c02db75ef`.
- Candidate identity: the single commit containing this note.
- Rollback: revert that candidate commit to restore the prior experimental
  standalone Packed construction behavior.

## Source and call-site proof

- The low-level public Rust construction family is
  `SurfacePresenter::from_window`, `SurfacePresenter::from_raw_handles`, and
  wasm-only `SurfacePresenter::from_canvas`.
- Before this candidate, each entry delegated into `SurfacePresenterHost` and
  then `SurfacePresenter::from_host_async`, which built the complete legacy
  presenter graph for an explicitly selected Packed renderer.
- `SurfaceRenderSession::from_window`, `from_raw_handles`, and `from_canvas`
  already dispatch on geometry before presenter construction. Packed selects
  the crate-private `SurfacePresenterHost` plus the renderer-owned Exact graph;
  Direct and Paged select the standalone presenter.
- In-tree Rust call sites of the public low-level family are the Direct/Paged
  session branch and the macOS public-entry regression. Product desktop, C,
  Web, Android, and Apple Packed construction enters through
  `SurfaceRenderSession::from_*`; no product Packed caller requires the
  standalone presenter.
- `SurfaceRenderSession::new` is unchanged. It does not convert an already
  built presenter, discard presenter state, or allocate a replacement host.

## Implemented boundary

- Every public low-level `SurfacePresenter::from_*` entry now checks geometry
  before delegating to any Surface host or legacy graph construction.
- Direct and diagnostic Paged pass admission unchanged.
- Explicit standalone Packed returns
  `SurfacePresenterError::StandalonePackedPresenterUnsupported`, repeatably
  mapped to `ErrorCode::Unsupported`. It does not inspect scene contents,
  create a Surface, request an adapter/device, or build legacy pipelines and
  geometry first.
- Product Packed remains constructor-time selected and host-owned through
  `SurfaceRenderSession::from_*`. The Apple M4 hidden-window regression renders
  a successful three-splat Packed frame on that route.
- No renderer math, source membership, SH data, resolution, ordering, WGSL,
  pass order, benchmark protocol, platform constructor, or product default is
  changed.

## Public Rust compatibility classification

- This is an intentional behavior incompatibility for callers of the
  experimental low-level Rust API that explicitly passed Packed to
  `SurfacePresenter::from_*`: construction previously succeeded and now
  returns structured Unsupported before allocation.
- Adding `StandalonePackedPresenterUnsupported` to the public, exhaustively
  matchable `SurfacePresenterError` enum is a Rust source-compatibility impact
  for downstream exhaustive matches. The repository's roadmap classifies the
  geometry selectors as experimental, but that does not erase this impact.
- There is no deprecation attribute or prior deprecation claim. None is
  invented here.
- Low-level Direct/Paged construction, `SurfaceRenderSession::new`, product
  Packed `SurfaceRenderSession::from_*`, exported C symbols, C layout/header,
  JNI/Kotlin, Swift, and Web declarations remain unchanged.
- Handbook-wide wording alignment belongs to the root M7/M8 factual closeout;
  this candidate records the implemented boundary locally without claiming
  that the active migration package is complete.

## Changed paths

- `crates/gsplat-render-wgpu/src/lib.rs`
- `crates/gsplat-render-wgpu/src/surface_presenter.rs`
- `examples/desktop/tests/surface_geometry_entry.rs`
- `docs/plans/completed/2026-07-23-native-render-core-migration/m7-standalone-packed-entry-closeout.md`

## Fresh local verification

All commands ran from the repository root on Apple M4 unless stated otherwise.

| Command | Result | Evidence boundary |
| --- | --- | --- |
| `cargo fmt --all` followed by `cargo fmt --check` | Pass | Repository Rust formatting. |
| `cargo test -p gsplat-render-wgpu --lib` | Pass: 461 passed, 8 ignored | Includes three-entry guard inventory, Direct/Paged admission, repeatable structured Packed rejection, and the existing session host-selection tests. |
| `cargo test -p desktop-example` | Pass: 17 passed | Desktop argument/loader regressions. |
| `cargo test -p desktop-example --features interactive-viewer --test surface_geometry_entry` | Pass: `M7D_SURFACE_GEOMETRY_ENTRY=PASS backend=Metal adapter="Apple M4" public_presenter=true public_session=true standalone_packed_rejected=true product_packed_host=true` | Real hidden macOS window/Metal check. It covers low-level Direct/Paged, pre-allocation standalone Packed rejection, and one successfully presented product Packed Exact frame. |
| `CARGO_INCREMENTAL=0 cargo test --workspace -j 2` | Pass | Full host workspace tests and doc-tests after reclaiming only this worktree's rebuildable Cargo artifacts. |
| `cargo check --workspace` | Pass | Host workspace compile. |
| `cargo check -p gsplat-web --target wasm32-unknown-unknown` | Pass | WASM constructor and public error compile only; no browser runtime claim. |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1 cargo test -p gsplat-render-wgpu --test conformance_sorted_alpha` | Pass: 1 passed | Required native GPU SortedAlpha image/count conformance; adapter absence was not allowed. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass | Host workspace/all-target lint. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | Pass | Public constructor links and docs resolve without warnings. |
| `PYTHONDONTWRITEBYTECODE=1 tests/architecture/test_source_architecture.py` | Pass: 3 tests | Architecture checker self-tests. |
| `PYTHONDONTWRITEBYTECODE=1 tests/architecture/check_source_architecture.py` | Pass: 123 production Rust, 17 WGSL, 5 grandfathered | Existing size notices only; policy passed. |
| `bash tests/ffi/run-ffi-smoke.sh` | Pass: `ffi smoke ok`, drawn 2, visible 2 | Host C ABI smoke; no header or ABI change. |
| `git diff --check` | Pass | Working-tree whitespace check before candidate commit. |

The first full renderer-library attempt had one test-only failure: the new
source guard inventory counted its own unsplit string literal and observed
four matches instead of three. The assertion was changed to the repository's
split-literal pattern; the complete retained rerun above passed 461 tests with
no failures. The implementation and hidden-window Metal route had already
passed during that first attempt.

An initial additional `cargo test --workspace` attempt failed during compilation
with `No space left on device`. Read-only inspection found only 124 MiB free
and 3.3 GiB under this worktree's `target/`. `cargo clean` removed only those
rebuildable Cargo artifacts (reported 4.0 GiB reclaimed); the retained
workspace rerun used `CARGO_INCREMENTAL=0` and two jobs and passed. A final
`cargo clean` removed the 1.0 GiB rebuild after verification because concurrent
machine activity had again reduced free space to 117 MiB. No source, dataset,
project, or configuration data was removed.

## Unavailable or unclaimed evidence

- No Android APK/AAR or physical-device run was performed. Android runtime,
  Vulkan, packaging, and device behavior are not claimed by this candidate.
- No iOS Simulator app, XCFramework, or physical-iPhone run was performed. The
  macOS hidden-window result is Metal Surface evidence only, not UIKit,
  Simulator, packaging, signing, or device evidence.
- No browser runtime was performed. The wasm32 result is compilation evidence,
  not Chrome/WebGPU behavior.
- No formal Kitsune performance run was performed because this slice changes
  admission before allocation rather than rendering execution. Existing
  quality/performance evidence is neither replaced nor reclassified.
