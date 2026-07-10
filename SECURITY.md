# Security Policy

## Supported Scope

`gsplat-rs` is on the `0.1.x` line. Security fixes are handled on the main
development line unless a maintained release branch is explicitly announced.

Security-sensitive areas include:

- Unsafe Rust, C ABI, JNI, and Swift bridge boundaries
- PLY parsing and input validation
- Platform file loading paths
- Build scripts and CI configuration

## Reporting a Vulnerability

Do not open a public issue with exploit details, private datasets, tokens,
credentials, or other sensitive material.

Report vulnerabilities privately through GitHub Security Advisories:
<https://github.com/misotofu0525/gsplat-rs/security/advisories/new>.
This is the only supported reporting channel. Share only the minimum
reproduction details needed to establish impact.

Maintainers must verify that private vulnerability reporting is enabled before
publishing a release tag. If the link above does not offer a private report
form, the repository is not release-ready and exploit details must not be
posted to a public issue.

You should receive an initial response within 7 days. If you do not, follow up
on the advisory thread rather than opening a public issue.

Please include:

- A short description of the issue and affected code path
- Reproduction steps using public data when possible
- The expected impact
- Any known workarounds

## Public Disclosure

The project will coordinate disclosure after a fix or mitigation is available.
Public advisories should avoid unnecessary exploit detail and should point to
the fixed commit or release when available.

## Dependency Policy

Dependency advisories, licenses, duplicate versions, and source registries are
checked with:

```bash
bash tests/security/run-cargo-deny.sh
```

The script downloads cargo-deny 0.20.2 from its official release and verifies
the platform artifact against a pinned SHA-256 before running the policy in
`deny.toml`. Advisory exceptions must include a scoped
reachability or upstream-blocker reason and remain visible in audit output;
new vulnerabilities fail the check. An exception is not evidence that an
affected dependency is safe, and should be removed as soon as a compatible
upstream fix is available.
