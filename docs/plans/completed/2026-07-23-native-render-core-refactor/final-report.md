# Package A Final Report

> Historical package report only. The aggregate branch was subsequently
> frozen as a research archive and is not a whole-branch integration
> candidate. See [research-branch-summary.md](research-branch-summary.md).

## Outcome

Package A is accepted at `ad0cc484a9764bcf0d2fd861af1bffc454050c24`
plus this documentation/policy closeout. It extracted strategy-free owners from
the legacy renderer without changing source membership, SH degree, resolution,
SortedAlpha math, CPU/GPU ordering policy, public API/ABI or platform behavior.

This package was an ownership refactor, not a performance claim. Existing
directional benchmark evidence remains scoped to its recorded binary and
protocol; no FPS or competitor percentage was used as a closeout gate.

## Accepted ownership

- `data/` owns GPU ABI layouts and immutable scene/order views.
- `api.rs` owns stable public execution identities and compatibility re-exports.
- `scene/` owns exact Resident CPU data, transactional encoding and resource
  preflight.
- `gsplat-sort` and renderer `cpu/` leaves own the existing stable CPU radix,
  SIMD helpers and visibility/depth/key primitives without changing them.
- `gpu/` owns strategy-free scan, radix, color and rank-projection mechanics.
- `raster/` owns the accepted pipeline construction and draw encoding.
- `evidence/` owns immutable receipts and bounded optional observer storage;
  policy still lives in the legacy session until migration.
- `surface/{lifecycle,configuration,capture}.rs` and
  `offscreen/{target,readback}.rs` own platform lifecycle leaves around the
  unchanged presenter/offscreen facades.

## Intentionally remaining legacy owners

Package A did not create a second renderer, plan controller or product route.
`surface_presenter.rs`, `surface_session.rs`, large GPU plan implementations
and platform consumers remain under their declared M/E tasks. Their legacy
ratchets are no-growth checkpoints, not arbitrary formatting budgets.

PLY and SPZ metadata/decode/import-policy ownership also remains open. The two
records receive one finite review renewal to M8 so an independent IO plan can
resolve responsibility without blocking the Exact core. No fixed line count
defines success, and the renewal may not be repeated.

## Verification

The final A8d production batch passed:

- source architecture checker and self-tests;
- `cargo fmt --all -- --check` and `git diff --check`;
- locked workspace check and test;
- workspace all-target Clippy with warnings denied;
- workspace Rustdoc with warnings denied;
- wasm32 Web check;
- required Metal SortedAlpha conformance;
- C FFI smoke.

The renderer library reported 306 passed tests and five pre-existing
research/device tests ignored. Fixed-SHA review found no P0, P1 or P2 issue.
The closeout itself changes only documentation, policy and ledger state and is
validated by the architecture fixtures, JSON parsing, Markdown-link checks and
clean Git scope.

## Next package

Package E starts with E0 only. E0 freezes the private Exact prepared-plan
contract and maps existing proofs to target owners. E1 then establishes the
transactional runtime skeleton. Independent implementation lanes begin only
after those shared seams exist: E2 and E8 can run in parallel after E1; the
platform-specific E4 and E5 SIMD leaves can run in parallel after E3.

Implementation work uses user-visible Codex tasks in isolated worktrees.
Collaboration subagents are limited to bounded read-only audits and fixed-SHA
reviews. Writer prompts never carry a fixed LOC quota.
