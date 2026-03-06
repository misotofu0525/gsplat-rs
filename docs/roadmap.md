# Roadmap

This document describes the current maintainable direction of the repository.

## Current baseline

- Keep the `SortedAlpha` renderer path, C ABI, pack tooling, and demo apps healthy.
- Prefer small, release-gated improvements over adding more placeholder surfaces.
- Treat desktop/mobile demos as integration harnesses for the core crates, not separate product lines.

## Current priorities

1. Keep `cargo check --workspace`, `cargo test --workspace`, smoke scripts, and release checklist passing.
2. Expand conformance/perf coverage with real datasets before widening API surface.
3. Improve mobile integration depth only when the shared C ABI remains simple and stable.
4. Document repo structure changes immediately when directories or responsibilities move.

## Deferred until runnable code exists

- A Web demo app
- Additional experimental blending/rendering backends
- New top-level apps or docs-only placeholders

## Repo hygiene rules

- Stable project docs belong in `README.md`, `docs/architecture.md`, `docs/api.md`, and `docs/releases/`.
- Historical planning and execution material belongs in `docs/archive/`.
- Agent/process notes belong in `docs/agent-notes/`, not the repository root.
