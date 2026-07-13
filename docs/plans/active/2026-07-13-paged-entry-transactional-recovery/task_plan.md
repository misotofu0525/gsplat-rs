# Task Plan: Paged Entry and Transactional Recovery

## Goal

Make the experimental Native/Web paged path selectable before scene-derived
Direct resources or a presenter are created, preserve the old session if a
runtime path switch fails, and retain compact commit-auditable proof that a
scene beyond the Direct resource gate can enter a fixed-slot paged Surface.

## Current Phase

Phase 4 — compact evidence and terminal regression.

## Hard Boundaries

- Direct `SortedAlpha` remains the default and only stable release-gated path.
- Reuse `Renderer`, `SurfaceRenderSession`, `SurfacePresenter`, paging modules,
  and existing platform wrappers; no new runtime crate or streaming stack.
- No HTTP, SOG/Streamed SOG, 10M qualification, telemetry sidecar, adversarial
  validator, registry publication, push, or force operation.
- One blocker at a time; target less than 800 net lines and at most two new
  files per implementation slice.
- Evidence must prove constructor behavior, not infer it from a post-create
  selector or from steady-state slot counts.

## Phases

### Phase 0: Freeze the recovery contract

- [x] Record the audit findings and ordered acceptance gates.
- [x] Confirm current branch/worktree and retained stable API boundary.
- **Status:** complete

### Phase 1: Select paged before scene derivation and presenter creation

- [x] Add an experimental constructor-time Web geometry selector while
      preserving the existing direct constructor.
- [x] Add the equivalent additive native Surface selector.
- [x] Apply the Web path before `Renderer::load_scene` path-specific derivation
      and before `SurfacePresenter` resource/limit selection.
- [x] Apply the native path at the same constructor boundary.
- [x] Prove direct remains the default and no full Direct CPU cache is created
      for a paged constructor.
- [x] Remove the full-scene packed staging allocation still performed while a
      paged atlas initializes its fixed-slot placeholder.
- **Status:** complete

### Phase 2: Make runtime path switching transactional

- [x] Build prospective renderer/presenter state without clearing the current
      working path.
- [x] Commit both sides only after all fallible allocations succeed.
- [x] Add a deterministic allocation/failure test proving the old path remains
      usable after a failed switch.
- **Status:** complete

### Phase 3: Prove an over-Direct-limit paged Surface entry

- [x] Create a deterministic synthetic scene/resource preflight whose full
      Direct representation exceeds the declared gate while paged slots fit.
- [x] Exercise the constructor-time Surface/Web path without allocating the
      whole Direct GPU representation.
- [x] Verify page count exceeds slots, fixed slot bounds, and non-zero draw or
      equivalent presenter preparation without weakening quality checks.
- **Status:** complete

### Phase 4: Persist compact evidence and close the audit

- [ ] Commit a small machine-readable evidence index containing commands,
      commit, hashes/receipts, and measured outcomes; keep bulky raw output
      ignored.
- [ ] Run the canonical renderer/Web/FFI/platform regression matrix.
- [ ] Reconcile achieved and still-deferred claims, then leave a clean worktree.
- **Status:** in_progress

## Acceptance

The goal is complete only when Native/Web can request paged before full Direct
startup work, a failed runtime switch cannot poison a usable session, the
over-limit proof is executable and committed, direct remains default, and the
evidence index survives a fresh clone.

## Decisions

| Decision | Rationale |
|---|---|
| Preserve existing direct constructors and add only an explicitly experimental preselected path | Avoids changing stable default behavior or silently widening v0.1 semantics. |
| Keep full CPU `SceneBuffers` out-of-core work deferred | This goal fixes the proven startup Direct-resource bug; production source streaming remains a separate architecture phase. |
| Persist a compact evidence index, not raw 36,000-frame artifacts | Makes claims auditable from a commit without reintroducing an artifact army. |
| Treat full-scene packed staging inside paged startup as a blocker | A fixed-slot atlas is not a bounded working set if initialization transiently packs every splat. |

## Errors Encountered

| Error | Attempt | Resolution |
|---|---:|---|
| Workspace clippy rejected the private UIKit create helper's eight arguments | 1 | Grouped the view/controller pointers into one internal target tuple; public ABI unchanged. |
