# Progress Log

## Session: 2026-07-10

### Phase 1: Audit the Current Demo

- **Status:** complete
- **Started:** 2026-07-10
- Actions taken:
  - Loaded the project's context, roadmap, and verification entrypoint.
  - Confirmed the clean `main` worktree before beginning.
  - Created a task-scoped planning bundle.
  - Audited the Web runtime topology, existing controls, renderer fallback, datasets, and repository constraints.
  - Inspected the current desktop first view in the in-app browser and recorded the visual hierarchy problems.
  - Researched open Gaussian Splat sources and verified CC BY 4.0 or CC0 terms for three promising collections.
  - Rendered Loop CE `basket` locally, inspected official previews, and rejected the original `Nandi` file after a fresh renderer-limit failure.
- Files created/modified:
  - `docs/plans/active/2026-07-10-viral-showcase-demo/task_plan.md`
  - `docs/plans/active/2026-07-10-viral-showcase-demo/findings.md`
  - `docs/plans/active/2026-07-10-viral-showcase-demo/progress.md`

### Phase 2: Creative Direction and Model Research

- **Status:** complete
- Actions taken:
  - Compared multiple CC BY and CC0 model sources by license, subject, file size, splat count, and local renderer behavior.
  - Selected the CC0 Wakufactory `kitune` trimmed model as the leading showcase candidate.
  - Diagnosed the candidate's import failure to Scaniverse infinite opacity logits only.
  - Added and tested strict opacity-logit normalization, then rendered both trimmed candidates with the project renderer.
  - Selected `kitune` after visual comparison with `sakura`.
- Files created/modified:
  - Planning bundle updated with model research and decisions.
  - `crates/gsplat-io-ply/src/lib.rs` updated with compatibility normalization and tests.

### Phase 3: Implementation

- **Status:** complete
- Actions taken:
  - Replaced the engineering-first sidebar with a full-viewport responsive showcase shell.
  - Added streamed loading progress, Kitsune-first fallback loading, scene switching, theme switching, a local PLY action, and a collapsible Studio panel.
  - Added a checksum-pinned CC0 model fetch script without committing the 65.9 MB PLY.
  - Rebuilt the generated WASM package so the browser uses the new import normalization.
  - Updated the README, Web README, architecture map, and project media.
- Files created/modified:
  - `examples/web/index.html`
  - `examples/web/styles.css`
  - `examples/web/src/main.js`
  - `tests/datasets/fetch-wakufactory-kitune.sh`
  - `docs/media/kitune.jpg`
  - `README.md`
  - `examples/web/README.md`
  - `handbook/ARCHITECTURE.md`

### Phase 4: Verification and Visual Review

- **Status:** complete
- Actions taken:
  - Passed JavaScript syntax, Rust formatting, diff whitespace, fetch-script syntax/checksum, targeted parser tests, and the Web WASM build.
  - Visually inspected dark desktop, light desktop, and mobile layouts in the in-app browser.
  - Verified Studio, theme, scene switching, active-state, loading, and WASM rendering behavior.
  - Removed the final wasm-bindgen deprecation warning and confirmed no fresh browser warnings or errors.
- Files created/modified:
  - Planning bundle updated with fresh evidence.

### Phase 5: Delivery

- **Status:** complete
- Actions taken:
  - Updated user-facing setup, model source/license notes, the repository hero image, and the architecture map.
  - Recorded all verification evidence and remaining hosting boundary.
- Files created/modified:
  - `README.md`
  - `examples/web/README.md`
  - `handbook/ARCHITECTURE.md`
  - Planning bundle finalized for completed history.

## Test Results

| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| Initial repository state | `git status --short --branch` | Clean worktree on the current branch | `## main...origin/main` | Pass |
| Parser unit tests | `cargo test -p gsplat-io-ply` | Infinite opacity normalized; other non-finite fields rejected | 17 passed | Pass |
| Workspace build | `cargo check --workspace` | All crates check | Passed | Pass |
| Workspace tests | `cargo test --workspace` | All unit, conformance, and doc tests pass | Passed | Pass |
| Rust formatting | `cargo fmt --check` | No formatting diff | Passed | Pass |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` | No warnings | Passed | Pass |
| Rust docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | Documentation builds without warnings | Passed | Pass |
| Web syntax | `node --check examples/web/src/main.js` | Valid JavaScript | Passed | Pass |
| Web package check | `npm --prefix packages/web run check` | Source and test syntax valid | Passed | Pass |
| Web package tests | `npm --prefix packages/web test` | Wrapper behavior passes | 6 passed | Pass |
| Web WASM build | `bash packages/web/scripts/build-wasm.sh` | Generated package includes parser fix | Passed | Pass |
| Web SDK build | `bash packages/web/scripts/build.sh` | Dist package rebuilt | Passed | Pass |
| Web pack dry-run | `npm --prefix packages/web run pack:dry-run` | Package contents valid | 8 files | Pass |
| Model fetch integrity | `tests/datasets/fetch-wakufactory-kitune.sh` | Existing/downloaded model matches pinned SHA-256 | Passed | Pass |
| Offscreen Kitsune render | `cargo run -p desktop-example -- .../kitune1.ply --auto-camera --png ...` | 279,199 splats render successfully | Passed | Pass |
| Browser smoke | Local server plus in-app browser | WASM scene loads, renders, and controls respond | Passed | Pass |
| Responsive visual check | 1440x900 and 390x844, dark and light | Hero and controls remain usable | Passed | Pass |
| Browser console | Fresh final reload | No new warnings or errors | `[]` | Pass |
| Diff hygiene | `git diff --check` | No whitespace errors | Passed | Pass |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-07-10 | `planning-with-files` not found under `~/.codex/skills` | 1 | Loaded it from `~/.agents/skills/planning-with-files`. |
| 2026-07-10 | Port 4173 already in use | 1 | Reused the already-running local repository server. |
| 2026-07-10 | `Nandi` exceeded `wgpu`'s 128 MB binding limit | 1 | Recorded the constraint and rejected the original 3.45M-splat file for direct use. |
| 2026-07-10 | Wakufactory models failed strict numeric parsing | 1 | Confirmed only opacity contains `+inf`; scoped a compatibility normalization. |
| 2026-07-10 | `cargo fmt --check` reported one formatting change | 1 | Ran `cargo fmt` before continuing. |
| 2026-07-10 | Browser selector waits hit a short dispatch deadline | 1 | Used fresh snapshots and authoritative state checks instead. |
| 2026-07-10 | Browser cached the old stylesheet and generated WASM | 1 | Added versioned asset URLs and rebuilt WASM. |
| 2026-07-10 | wasm-bindgen emitted a deprecated initialization warning | 1 | Updated wrapper initialization, tests, and rebuilt dist; final fresh console was clean. |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Complete. |
| Where am I going? | Hand off the implemented and verified showcase. |
| What's the goal? | Build a distinctive, shareable `gsplat-rs` demo with a suitable licensed model. |
| What have I learned? | The new WASM showcase runs at interactive speed and holds its hierarchy across dark, light, desktop, and mobile layouts. |
| What have I done? | Built, documented, and fully verified the showcase, model path, parser compatibility, media, and Web wrapper cleanup. |
