# gsplat-rs Agent Guide

## Purpose

- This file is the project entrypoint for agents and collaborators.
- Keep it thin: use it to route into the right project docs, not as a knowledge base.

## Canonical Docs

- Project context: `docs/PROJECT_CONTEXT.md`
- Architecture map: `docs/ARCHITECTURE.md`
- Verification entrypoint: `docs/VERIFICATION.md`
- Current direction and release boundary: `docs/roadmap.md`
- Task planning bundles: `docs/plans/active/<yyyy-mm-dd>-<task>/`

## Load Order

- Read `docs/PROJECT_CONTEXT.md` first for any non-trivial task.
- Read `docs/ARCHITECTURE.md` when the task affects crate boundaries, render flow, FFI, or repo structure.
- Read `docs/VERIFICATION.md` before changing scripts/commands or claiming completion.
- Read `docs/roadmap.md` when the task depends on current priorities, release scope, or non-goals.
- For Android or iOS work, also read the matching demo README under `apps/android-demo/` or `apps/ios-demo/`.
- For complex work, create or resume a plan bundle under `docs/plans/active/` and keep findings there.

## Project Hard Rules

- Never invent commands, file paths, SDK assumptions, or repo structure. Read the local scripts and READMEs first.
- Keep the repo shape small. Do not add placeholder top-level apps, docs, or experimental tracks without an explicit task.
- Treat `SortedAlpha` as the only release-gated render path until `docs/roadmap.md` says otherwise.
- Keep the v0.1 C ABI small and stable. If you change it, update both `crates/gsplat-ffi-c/src/lib.rs` and `crates/gsplat-ffi-c/include/gsplat.h`, then run the FFI smoke path.
- Prefer repo-local verification scripts from `docs/VERIFICATION.md` over ad-hoc command sequences.

## Verification

- Fast check: `cargo check --workspace`
- Full verification routes: `docs/VERIFICATION.md`

## Notes

- Keep `README.md` human-facing.
- Keep project facts in `docs/PROJECT_CONTEXT.md` and `docs/ARCHITECTURE.md`.
- Keep planning and temporary research out of this file.
