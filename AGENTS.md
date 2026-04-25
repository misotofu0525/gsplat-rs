# gsplat-rs Agent Guide

## Purpose

- This file is the project entrypoint for agents and collaborators.
- Keep it thin: use it to route into the right project docs, not as a knowledge base.

## Canonical Docs

- Project context: `handbook/PROJECT_CONTEXT.md`
- Architecture map: `handbook/ARCHITECTURE.md`
- Verification entrypoint: `handbook/VERIFICATION.md`
- Current direction and release boundary: `handbook/ROADMAP.md`
- Project taste guide: `handbook/GOLDEN_PRINCIPLES.md`
- Task planning bundles: `docs/plans/active/<yyyy-mm-dd>-<task>/`

## Load Order

- Read `handbook/PROJECT_CONTEXT.md` first for any non-trivial task.
- Read `handbook/ARCHITECTURE.md` when the task affects crate boundaries, render flow, FFI, or repo structure.
- Read `handbook/VERIFICATION.md` before changing scripts/commands or claiming completion.
- Read `handbook/ROADMAP.md` when the task depends on current priorities, release scope, or non-goals.
- Read `handbook/GOLDEN_PRINCIPLES.md` when work touches architecture taste, error handling, public API shape, or repeated design choices.
- For Android or iOS work, also read the matching demo README under `apps/android-demo/` or `apps/ios-demo/`.
- For complex work, create or resume a plan bundle under `docs/plans/active/` and keep findings there.

## Project Hard Rules

- Never invent commands, file paths, SDK assumptions, or repo structure. Read the local scripts and READMEs first.
- Keep the repo shape small. Do not add placeholder top-level apps, docs, or experimental tracks without an explicit task.
- Treat `SortedAlpha` as the only release-gated render path until `handbook/ROADMAP.md` says otherwise.
- Keep the v0.1 C ABI small and stable. If you change it, update both `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`, then run the FFI smoke path.
- Prefer repo-local verification scripts from `handbook/VERIFICATION.md` over ad-hoc command sequences.

## MCP Research Tools

- Use MCP research tools as source-finding and API-reference helpers, not as substitutes for local code inspection or project verification.
- Prefer `exa` for broad external discovery: recent papers, comparable GitHub repositories, architecture references, and ecosystem surveys.
- Prefer `ref` or `context7` for API/library documentation, especially `wgpu`, Android NDK/JNI, Rust crates, shader language details, and platform behavior that may have changed.
- When MCP tools are needed but not currently exposed, discover them through the available tool-discovery route first; do not invent MCP tool names or schemas.
- For implementation decisions, reconcile MCP findings with repo-local files, current project docs, and true verification results. External examples are inspiration until the local benchmark/test path proves them.
- For complex research or performance work, summarize useful MCP findings in the active plan bundle under `docs/plans/active/<task>/`, including links or identifiers for papers/repos/docs and the reason each idea was accepted, rejected, or deferred.
- Do not count MCP research as completion evidence. Completion claims require local commands, tests, device runs, or other fresh verification from `handbook/VERIFICATION.md`.

## Verification

- Fast check: `cargo check --workspace`
- Full verification routes: `handbook/VERIFICATION.md`

## Notes

- Keep `README.md` human-facing.
- Keep project facts in `handbook/PROJECT_CONTEXT.md` and `handbook/ARCHITECTURE.md`.
- Keep planning and temporary research out of this file.
