# Q1 Product Quality one-view smoke v1

This offline gate evaluates only formal Truck view `000001` at `979x546`. It
does not launch a browser, device, renderer, or benchmark and cannot authorize
performance.

## Inputs

The command requires four already materialized roots:

- the reviewed formal trace authority containing only `camera-trace.json` and
  `receipt.json`;
- the validated upstream Evaluation Images authority containing
  `source/gt/000001.png`;
- a gsplat-rs `gsplat-benchmark/v1` control with `manifest.json`,
  `frames.jsonl`, and `final-frame.png`;
- a PlayCanvas `gsplat-benchmark/v1` control with `manifest.json` and
  `final-frame.rgba8`.

The gsplat-rs adapter requires the existing same-present chain:
`q1_comparison.presentation_identity` selects formal trace index zero, binds
the canonical pose/intrinsics hash and terminal frame hash, and that terminal
contains the exact frozen `capture_depth_precision` receipt whose RGBA digest
matches the decoded PNG.

The PlayCanvas adapter requires the existing
`gsplat-playcanvas-webgpu-renderer-capture/v1` producer, terminal queue drain,
camera JSON digest and exact centered-pinhole custom-projection receipt. Its
`gsplat-playcanvas-renderer-capture-materialization/v1` receipt must bind the
raw RGBA file byte-for-byte. A canvas screenshot is not accepted.

Both controls must prove complete Truck membership, SH3, exact `979x546`
internal resolution, no LOD/sampling/upscaling/dynamic resolution, and the
formal trace content hash. Missing or malformed proof fails closed without a
publication.

## Decision

The gate calls the frozen `q1_pair_admission/product_quality.py` reducer. Each
endpoint independently receives `Accepted` or `Rejected` from:

- window-8 sRGB-luma SSIM `>= 0.90`;
- normalized RGB MAE `<= 0.05`;
- pixel fraction with any RGB error above 32 `<= 0.10`;
- alpha exactly 255 at every endpoint pixel.

A threshold miss is a finite `Rejected` result. Structural, provenance,
camera, alpha, resolution, producer, hash, or file failures produce no result.
The one-view aggregate is Accepted only when both endpoints accept, but formal
Product Quality remains `Deferred` because view `000009` is still required.
`performance_eligible` is always false.

The result deliberately contains no timing, frame-wall, FPS, throughput,
pairing, speed ratio, or winner fields. Publication is one fresh directory
containing only `result.json`, installed atomically without replacement.

```bash
python3 tests/perf/validate-q1-product-quality-smoke.py \
  --formal-trace-authority /retained/formal-trace \
  --evaluation-authority /retained/evaluation-authority \
  --gsplat-capture /retained/gsplat-control \
  --playcanvas-capture /retained/playcanvas-control \
  --output /fresh/q1-product-quality-view-000001
```
