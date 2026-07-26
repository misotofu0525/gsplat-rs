# Benchmark Dataset Manifests

These committed manifests identify local benchmark inputs without committing the
large assets themselves. A qualification run must match the exact SHA-256,
byte count, splat count, SH degree, and bounds before it is accepted.

`qualified` means the source and local benchmark use are documented. It does
not automatically permit redistributing the asset or screenshots. A
`local_candidate` remains excluded from public claims until its source and
license fields are resolved.

The Bonsai local candidate binds the official pretrained archive identity for
local S1 research, but its archive has no asset-specific model license. It may
be used only for local evaluation and must not be redistributed or used for a
public qualification claim until those rights are clarified. Its bounds
receipt names the exact repository commit, source SHA-256 and splat count used
by the canonical spatial-analysis command.

Bounds are derived by the repository's spatial-analysis command:

```bash
cargo run --release -p bench-runner -- <scene.ply> --analyze-spatial
```

`minimal_binary` is a deterministic binary-little-endian equivalent used by
engines that do not accept ASCII PLY. Regenerate it with:

```bash
python3 tests/datasets/generate-minimal-binary.py
```

Point-count scaling tiers are generated from a qualified full scene rather than
committed as independent quality datasets. See `tests/datasets/README.md` and
`tests/datasets/ply_ladder.py`; each generated ladder has its own source/output
SHA-256 provenance document.

The cross-platform small-to-large coverage plan and its strict full-residency
receipts live in `tests/perf/full-quality-matrix-plan-v1.json` and
`tests/perf/full-quality-experiment-v1.md`. Scaling tiers remain performance
inputs only; full-scene anchors provide image-quality evidence.
