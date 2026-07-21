# Test and Benchmark Datasets

Large assets live under `tests/datasets/external/` and are ignored by Git. The
repository commits fetch/generation tools and identity metadata, not the model
files themselves.

## Point-count ladder

`ply_ladder.py` creates byte-stable performance tiers from any fixed-record
binary PLY. It keeps every header line and property declaration unchanged
except for `element vertex`, copies non-vertex elements unchanged, and selects
records in ascending source order with integer-only midpoint stratification:

```text
source_index = floor(((2 * output_index + 1) * source_count) /
                     (2 * output_count))
```

This samples the whole source record order instead of taking the first N
records. It is deterministic across Android, Apple, desktop, and Web because it
does not decode/re-encode floats and does not use a random-number generator.

For example, build the 50k/100k/200k tiers used around the original mobile
decision point:

```bash
python3 tests/datasets/ply_ladder.py \
  tests/datasets/external/wakufactory_kitune/kitune1.ply \
  --counts 50k 100k 200k \
  --source-manifest tests/perf/datasets/kitsune.json \
  --output-dir tests/datasets/external/ladder/kitsune
```

For a larger source, the same command can add `300k 500k 1m 1.5m 2m 3m 5m` as
long as no tier exceeds the source vertex count. Outputs include `ladder.json`
with source/output SHA-256 values, exact sizes, selected index endpoints, and
copied provenance.

Derived tiers isolate point-count scaling; they are not independent scenes and
must not replace full-scene image-quality checks. Use the exact same generated
PLY on every platform in a comparison. Each size is stratified independently,
so tiers are not guaranteed to be nested subsets; CPU/GPU pairs at one size are
content-identical, while cross-size curves should be interpreted as sampled
point-count scaling rather than an incrementally growing scene.

## Official INRIA 3DGS scene anchors

`fetch_inria_3dgs_scenes.py` extracts only requested
`iteration_30000/point_cloud.ply` entries from the official 14.66 GB Zip64
archive. It does not download the complete archive and does not hold a large
compressed or decompressed scene in memory. Each request is streamed to a
temporary file, checked against pinned archive CRC/size/count metadata and the
PLY SH layout, then atomically renamed. All four selected PLYs also have a
committed expected SHA-256; a `source.json` beside the PLY records the verified
identity and provenance.

Repeating a command re-hashes and verifies an existing scene locally instead of
downloading it again. Use `--overwrite` only to replace a failed or intentionally
refreshed local copy. Cancelling a transfer removes only its incomplete
temporary file and leaves any previously verified PLY untouched.

```bash
python3 tests/datasets/fetch_inria_3dgs_scenes.py --list
python3 tests/datasets/fetch_inria_3dgs_scenes.py \
  --scenes bonsai truck garden bicycle \
  --acknowledge-local-use-only
```

The bounded anchors form an approximate full-scene size ladder:

| Scene | Family | Splats | PLY bytes |
| --- | --- | ---: | ---: |
| Bonsai | Mip-NeRF 360 indoor | 1,244,819 | 308,716,644 |
| Truck | Tanks and Temples | 2,541,226 | 630,225,580 |
| Garden | Mip-NeRF 360 outdoor | 5,834,784 | 1,447,027,964 |
| Bicycle | Mip-NeRF 360 outdoor | 6,131,954 | 1,520,726,124 |

Asset rights are deliberately recorded as `NOASSERTION`. The source software
repository has a research/evaluation license, but that does not by itself prove
the pretrained model archive's asset license, and upstream dataset terms may
also apply. These files are therefore for local research/evaluation only and
must not be redistributed unless the relevant rights are clarified. The range
helper is adapted from nerfstudio-project/gsplat's Apache-2.0 downloader and
retains its SPDX notice in the source file.

## Verification

The focused tests use tiny generated fixtures and a local range-capable HTTP
server; they do not contact INRIA or download large assets:

```bash
python3 tests/datasets/test_dataset_tools.py
python3 tests/perf/validate-dataset-manifests.py
python3 tests/perf/validate-dataset-manifests.py --verify-available
```
