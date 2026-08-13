# Task Plan: Resident-only renderer cleanup

## Goal

Rename the remaining full-resident render path, remove Packed and Paged product/runtime code across the workspace, synchronize current docs, run the repository verification gates, and commit the verified result on `main`.

## Current Phase

Complete

## Phases

### Phase 1: Discover the complete change surface

- [x] Re-read canonical project and verification docs
- [x] Inventory Direct/Packed/Paged symbols, modules, tests, scripts, and bindings
- [x] Separate the pre-existing ROADMAP edit from new changes
- **Status:** complete

### Phase 2: Remove Packed/Paged and rename the resident path

- [x] Remove runtime modules and branch points
- [x] Remove public C/Swift/Kotlin/Web selectors and obsolete compatibility APIs
- [x] Rename surviving internal identifiers to resident/full-resident terminology where a name remains necessary
- [x] Keep SortedAlpha behavior and the existing CPU production path intact
- **Status:** complete

### Phase 3: Synchronize tests, examples, scripts, and current docs

- [x] Remove obsolete Packed/Paged tests and fixtures
- [x] Update architecture and project-currentness documentation
- [x] Update ROADMAP to describe the resident-only baseline and future streaming as a distinct capability
- [x] Confirm AGENTS and verification routing remain accurate
- **Status:** complete

### Phase 4: Verify and repair

- [x] Run formatting and focused checks while iterating
- [x] Run the canonical workspace, FFI, Web, Android, and Apple gates applicable to this structural change
- [x] Record every result and any environment-limited gate
- **Status:** complete

### Phase 5: Review and commit on main

- [x] Review status/diff for scope and stale terminology
- [x] Confirm `main` and no unrelated files are staged
- [x] Prepare the verified refactor as one commit on `main`
- [x] Record the final verification boundary
- **Status:** complete

## Key Questions

1. Which `Direct` names describe full residency, and which independently mean direct draw/index access and should remain?
2. Are Packed/Paged symbols exposed through stable bindings, test tools, or docs beyond the renderer crate?
3. Which verification gates can be completed locally on this machine without claiming physical-device coverage?

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Use `ResidentScene` / `FullResident` only where the residency concept must be explicit | `Direct` is ambiguous; no public mode is needed when only one product path remains |
| Delete obsolete code instead of feature-gating or compatibility shims | Matches the user request and project hard rule against backward-compatibility layers |
| Keep the planning bundle under `docs/plans/active/` | Project-local planning convention overrides a root-level generic template |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| Large `GpuRasterizer` cleanup patch missed context after mechanical `direct_scene` rename | 1 | No partial write; inspect current block and apply smaller field/init/method patches |
| `cargo deny check` was unavailable as a direct command | 1 | Used the repository wrapper `bash tests/security/run-cargo-deny.sh`; all policy checks passed |

## Notes

- Initial state: `main...origin/main`; pre-existing modification: `handbook/ROADMAP.md`.
- Android AAR/sample builds and Apple XCFramework/simulator launch passed. No
  Android or iPhone physical-device runtime acceptance was performed in this
  change, so no physical-device performance claim is made.
