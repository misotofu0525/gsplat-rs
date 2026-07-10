# Release Process

`gsplat-rs` uses signed-off, tag-triggered GitHub prereleases for the `0.1.x`
line. Package registries are not part of the first release boundary.

## Preconditions

Before creating a tag:

1. Confirm `main` is protected by required reviews and required CI checks.
2. Confirm GitHub private vulnerability reporting is enabled and the link in
   `SECURITY.md` accepts a new report.
3. Confirm the release commit is on `main`, the worktree is clean, and all
   workspace/package versions match the intended tag. Verify locally with
   `RELEASE_VERSION=0.1.1 bash tests/release/check-version.sh`.
4. Move completed plan bundles out of `docs/plans/active/` and update
   `CHANGELOG.md`, `README.md`, and affected handbook pages.
5. Run the full local verification matrix in `handbook/VERIFICATION.md`,
   including `bash tests/security/run-cargo-deny.sh`, platform packaging
   smokes, pixel conformance,
   the release-mode benchmark, and the 1800-second stability bar.
6. Record any physical-device or browser gaps explicitly; a successful build
   is not device-runtime evidence.

The first two items are GitHub repository settings. They are intentional
manual gates and are not mutated by repository scripts.

## Tag and Artifacts

Create an annotated semantic-version tag from the verified release commit:

```bash
git tag -s v0.1.1 -m "gsplat-rs v0.1.1"
git push origin v0.1.1
```

The tag workflow re-runs core checks, dependency policy, C ABI smoke, the
1800-second stability bar, and platform packaging before creating a GitHub
Release. It attaches:

- `gsplat-android-release.aar`
- `GsplatFFI.xcframework.zip`
- `gsplat-rs-web-*.tgz`

Verify the GitHub Release contains all three artifacts and generated notes.
Verify each artifact against the attached `SHA256SUMS` file.
Do not describe these files as Maven, SwiftPM, npm, or crates.io publication;
they remain direct prerelease artifacts.

## Rollback

If the tag workflow fails, fix the underlying issue on a new commit and create
a new patch tag. Do not move or overwrite an already-published tag or replace
release assets in place.
