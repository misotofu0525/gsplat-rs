# gsplat-rs Golden Principles

## Purpose

- This file captures stable engineering taste for this project.
- Keep it short and practical.
- Promote repeated rules into tests, scripts, or CI checks when possible.

## Golden Principles

- Preserve the small release surface until `ROADMAP.md` explicitly widens it.
- Reuse the existing crate boundaries before adding new crates, apps, or top-level directories.
- Validate data at repository boundaries: PLY import, FFI, JNI, Swift, and CLI arguments.
- Keep public native integration boring and stable. C header changes must move with the Rust FFI implementation and smoke coverage.
- Prefer explicit render-mode contracts over partially supported branches. `SortedAlpha` is the release-gated path today.
- Make verification executable. Use repo-local scripts and commands instead of prose-only confidence.

## Smells To Resist

- New placeholder apps, docs tracks, or experimental backends without a release-boundary reason.
- Internal asset/cache formats without a measured runtime need and a real consumer.
- Divergence between `crates/gsplat-ffi-c/include/gsplat.h` and `crates/gsplat-ffi-c/src/lib.rs`.
- Mobile demo changes that hide shared-library or ABI issues behind platform-specific workaround code.
- Ad-hoc command sequences that bypass `VERIFICATION.md`.
- Runtime assumptions that skip the documented PLY quaternion and coordinate-space normalization rules.

## Mechanical Follow-Through

- If repo structure changes, update `PROJECT_CONTEXT.md`, `ARCHITECTURE.md`, and `AGENTS.md` in the same change.
- If verification changes, update `VERIFICATION.md` before claiming the new path is canonical.
- If release scope changes, update `ROADMAP.md` before treating the change as in-contract.
- If contributor workflow changes, update `README.md`, `CONTRIBUTING.md`, and GitHub templates together.
