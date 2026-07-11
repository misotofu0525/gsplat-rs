# Benchmark Dataset Manifests

These committed manifests identify local benchmark inputs without committing the
large assets themselves. A qualification run must match the exact SHA-256,
byte count, splat count, SH degree, and bounds before it is accepted.

`qualified` means the source and local benchmark use are documented. It does
not automatically permit redistributing the asset or screenshots. A
`local_candidate` remains excluded from public claims until its source and
license fields are resolved.

Bounds are derived by the repository's spatial-analysis command:

```bash
cargo run --release -p bench-runner -- <scene.ply> --analyze-spatial
```

`minimal_binary` is a deterministic binary-little-endian equivalent used by
engines that do not accept ASCII PLY. Regenerate it with:

```bash
python3 tests/datasets/generate-minimal-binary.py
```
