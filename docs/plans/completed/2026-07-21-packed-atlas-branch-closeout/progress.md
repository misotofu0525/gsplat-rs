# Progress: Packed Atlas Branch Closeout

## 2026-07-21

- Created a goal without a token, time, FPS, or competitor-win hard budget.
- Configured Rust 1.93.0, rustfmt, clippy, and the locked local workspace dependencies.
- Confirmed the original branch HEAD had successful Linux, macOS/Swift, and dependency-policy checks.
- Completed local and external competitive audits.
- Removed the unread Packed SH GPU texture and corrected preflight accounting.
- Removed the remaining fictitious hot-texture dimension gate and avoided
  building unbound full-scene SH sidecars on the GPU hot-record path.
- Added complete first-frame Packed SH color and frozen-camera band refresh state.
- Removed unused Auto Surface constructors while retaining explicit diagnostics.
- Kept Paged implementation types crate-private while retaining its explicit
  geometry selector and diagnostic tests.
- Archived all three predecessor plans and aligned canonical docs plus CI.
- Final Rust result: workspace tests passed; renderer discovered 103 tests,
  with 102 passed, 0 failed, and 1 retained research oracle ignored.
- `cargo fmt`, Clippy with warnings denied, rustdoc with warnings denied,
  dependency policy, Web/WASM checks, benchmark artifact/dataset/trace
  contracts, collector tests, and pinned PlayCanvas preflight passed.
- Required Metal conformance passed on Apple M4. The minimal 120-frame release
  smoke completed with zero missed 16.67 ms frames; this is regression evidence,
  not a competitor or broad performance claim.
- C ABI, JNI, Swift, XCFramework, and GsplatKit simulator builds passed.
- Local Android AAR/APK/JVM tests could not run because this machine has no
  Android SDK. Java 21 is installed; the exact branch SHA must pass the required
  Linux job's provisioned SDK/NDK AAR, APK, and JVM tests before `main` moves.

## Publication Handoff

- Push this repository tree as one closeout commit on
  `refactor/packed-atlas-d-reset`.
- Wait for the required Linux, macOS/Swift, and dependency-policy checks on that
  exact SHA.
- Fast-forward `main` to the same SHA; do not add a merge commit or post-check
  documentation commit.
