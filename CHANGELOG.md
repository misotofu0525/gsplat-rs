# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Open-source maintenance docs: `CONTRIBUTING.md`, `SECURITY.md`, issue
  templates, and a pull request template.
- Dual-license files for the existing `MIT OR Apache-2.0` package metadata.
- CI hygiene coverage for rustfmt, clippy, rustdoc warnings, and Web example
  JavaScript syntax.
- `CODE_OF_CONDUCT.md` (Contributor Covenant v2.1), `.github/CODEOWNERS`, and
  Dependabot configuration for cargo, GitHub Actions, npm, and Gradle.
- Tag-triggered release workflow that runs core checks and publishes a GitHub
  Release with AAR, XCFramework, and npm package artifacts.
- Crate-level READMEs and crates.io metadata (`readme`, `keywords`,
  `categories`, `homepage`) for all publishable crates.
- A rendered hero image in the README, produced by the desktop example from
  the public NVIDIA `flowers_1` dataset.

### Changed

- README now describes project status, quick start, verification, integration
  boundaries, contribution flow, security policy, and licensing in one
  external-facing entrypoint, with CI/license/MSRV badges and a platform
  support matrix.
- `SECURITY.md` now points to GitHub Security Advisories as the only
  supported private reporting channel.
- Blank GitHub issues are disabled; reports are routed through the issue
  templates.
