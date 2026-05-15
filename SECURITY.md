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

Use GitHub private vulnerability reporting for this repository if it is
available. If it is not available, contact the maintainers through a private
channel first and share only the minimum reproduction details needed to
establish impact.

Please include:

- A short description of the issue and affected code path
- Reproduction steps using public data when possible
- The expected impact
- Any known workarounds

## Public Disclosure

The project will coordinate disclosure after a fix or mitigation is available.
Public advisories should avoid unnecessary exploit detail and should point to
the fixed commit or release when available.
