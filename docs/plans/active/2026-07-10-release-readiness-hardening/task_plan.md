# Task Plan: Release Readiness Hardening

## Goal

Turn the current source-only `0.1.x` workspace into a safe, verifiable, and
adoptable prerelease whose Rust and C APIs fail explicitly, whose renderer is
proved to produce pixels, and whose release path matches the documented bar.

## Current Phase

Phase 6

## Phases

### Phase 1: Baseline and Contracts

- [x] Re-read the project context, architecture, verification, roadmap, and principles.
- [x] Audit the current worktree and live GitHub state.
- [x] Define the error/resource contracts for PLY loading, GPU creation, and FFI.
- [x] Record the exact regression tests required before implementation.
- **Status:** complete

### Phase 2: Input and FFI Safety

- [x] Add explicit PLY resource limits and checked allocation arithmetic.
- [x] Validate binary body size before scene allocation.
- [x] Expose structured resource/allocation errors without changing existing successful loads.
- [x] Add a uniform panic barrier around exported C ABI entrypoints.
- [x] Add malicious/oversized/truncated input regression tests.
- **Status:** complete

### Phase 3: Renderer Outcome Semantics

- [x] Validate requested dimensions against the selected adapter/device limits.
- [x] Stop swallowing offscreen GPU initialization and runtime render failures.
- [x] Separate CPU preprocessing success from GPU render success in the public contract.
- [x] Add explicit no-adapter, device-limit, and runtime-failure tests where practical.
- **Status:** complete

### Phase 4: Executable Conformance and Performance Semantics

- [x] Add an offscreen readback assertion that proves non-empty pixel output.
- [x] Add a stable golden/tolerance comparison for the release-gated `SortedAlpha` path.
- [x] Make benchmark output distinguish CPU preparation, submit, and actual GPU completion.
- [x] Run benchmarks in release mode with adapter/backend metadata and thresholds where stable.
- **Status:** complete

### Phase 5: Release, Security, and Adoption

- [x] Align the release workflow with `handbook/VERIFICATION.md` and the 1800-second release bar.
- [x] Add dependency advisory/license checks and triage current RustSec findings.
- [x] Harden Actions permissions and pin third-party actions by full commit SHA.
- [x] Close the public installation/API documentation gap for the first prerelease.
- [x] Repair CHANGELOG, third-party asset provenance, and stale plan state.
- [x] Prepare GitHub settings changes; apply external settings only with explicit authorization.
- **Status:** complete

### Phase 6: Full Verification and Delivery

- [x] Run all relevant local Rust, FFI, Web, packaging, and platform smoke paths.
- [x] Run the complete 1800-second GPU-complete stability bar locally.
- [ ] Verify the fresh CI/release evidence covers every documented requirement.
- [x] Confirm the worktree diff contains no unrelated changes.
- [ ] Archive this plan bundle only after the full objective is complete.
- **Status:** in_progress

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Fix crash/false-success paths before documentation polish | A public-looking API must first be safe and truthful. |
| Preserve `SortedAlpha` as the only release-gated renderer | This matches `handbook/ROADMAP.md` and avoids widening scope. |
| Keep platform SDK publication out of the first hardening phase | Android, iOS, and Web distribution are already documented as later boundaries. |
| Treat GitHub settings as a separate authorized mutation | Local implementation work is authorized; external repository policy changes deserve an explicit checkpoint. |

## Errors Encountered

| Error | Resolution |
|-------|------------|
| Focused Clippy rejected an identity multiplication in scene-byte accounting | Removed the redundant multiplication and reran Clippy with warnings denied. |
| The first `v0.1.0` tag run assumed `rg` existed on the stock Ubuntu runner | Preserved the immutable failed tag, replaced the release check with portable `grep`, and prepared patch release `v0.1.1`. |
