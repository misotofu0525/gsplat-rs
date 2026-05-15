# Task Plan: Open Source Readiness Review

## Goal

Review `gsplat-rs` from the perspective of a strong open-source project and
close the highest-value gaps that block external contributors from building,
testing, understanding, and contributing safely.

## Findings

- The codebase already has a clear handbook, small release boundary, CI smoke
  paths, and platform validation demos.
- The root README was accurate but too thin as a public entrypoint.
- The manifest declared `MIT OR Apache-2.0`, but the repository did not include
  the corresponding license files.
- The repository lacked contributor guidance, security reporting policy,
  issue templates, and a pull request template.
- CI covered build/test/smoke paths, but not rustfmt, clippy, rustdoc warning
  checks, or Web demo JavaScript syntax.
- `cargo clippy --workspace --all-targets -- -D warnings` exposed only small
  mechanical issues, so it is practical to make clippy part of the quality bar.

## Plan

1. [done] Read project docs, current workflows, manifests, and root files.
2. [done] Fix low-risk clippy issues that block a clean CI lint gate.
3. [done] Add open-source maintenance files and GitHub templates.
4. [done] Strengthen README and package metadata for external consumers.
5. [done] Sync handbook verification docs and CI with the new quality
   bar.
6. [done] Run the updated local verification set and record evidence.

## Acceptance Criteria

- README explains status, quick start, verification, contribution, security,
  license, and release boundary without overstating SDK maturity.
- License files match the workspace license expression.
- Contributors have issue/PR templates and a contribution guide.
- CI runs formatting, clippy, docs, Web demo syntax, core Rust tests, and the
  existing smoke paths.
- Fresh local verification evidence exists for the updated checks.

## Evidence

- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo check --workspace` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` passed.
- `node --check apps/web-demo/src/main.js` passed.
- `cargo test --workspace` passed.
- `cargo run -p bench-runner -- tests/datasets/minimal_ascii.ply 120` passed
  with `avg_visible_count=2.00` and `avg_drawn_count=2.00`.
- `bash tests/ffi/run-ffi-smoke.sh` passed with `drawn=2 visible=2`.
- `bash apps/android-demo/run-jni-smoke.sh` passed.
- `bash apps/ios-demo/run-swift-smoke.sh` passed with `drawn=2 visible=2`.
