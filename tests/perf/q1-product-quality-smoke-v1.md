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
the canonical pose/intrinsics hash, the renderer-returned eleven-value camera
receipt, its camera revision and terminal frame hash. That terminal contains
the exact frozen `capture_depth_precision` receipt whose RGBA digest matches
the decoded PNG.

The PlayCanvas adapter requires the existing
`gsplat-playcanvas-webgpu-renderer-capture/v1` producer, the complete
`presentation_capture` submit chain, same-frame copy, both terminal queue
drains, camera JSON digest and exact centered-pinhole custom-projection receipt.
The offline gate independently recomputes pose, view, OpenGL/WebGPU projection
and both view-projection matrices from the formal trace; a self-consistent but
incorrect producer declaration is rejected. Its
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
containing only `result.json`, installed atomically without replacement. The
output must be disjoint from every immutable authority and capture input tree.

```bash
python3 tests/perf/validate-q1-product-quality-smoke.py \
  --formal-trace-authority /retained/formal-trace \
  --evaluation-authority /retained/evaluation-authority \
  --gsplat-capture /retained/gsplat-control \
  --playcanvas-capture /retained/playcanvas-control \
  --output /fresh/q1-product-quality-view-000001
```

## One-shot view 000001 transaction

The repository also provides a one-shot coordinator for producing and
evaluating view `000001`. It requires a clean exact full commit SHA, validates
both authorities and complete Truck before either endpoint starts, then runs
the native quality-only producer once, the PlayCanvas headful quality-only
producer once, and this validator once. The requested output appears atomically
only after all three succeed. Any failure stops the sequence without retry and
is retained in an immutable sibling failure directory; it cannot masquerade as
a one-view result. A child nonzero exit retains its exact argv, stdout, and
stderr under `failed-command/<step>/` without serializing environment
variables, so diagnosis does not require another endpoint run.
The native producer applies the same rule one level deeper: a desktop-host
failure retains an immutable `native-view000001.failed-q1-native-*` child with
the host command/stdout/stderr and build diagnostics, while deleting its
rebuildable private Cargo target and unpublished candidate artifact. That tree
is diagnostic only and never substitutes for the requested native capture.

```bash
python3 tests/perf/collect-q1-product-quality-view000001.py \
  --expected-commit <full-40-character-sha> \
  --formal-trace-authority /retained/formal-trace \
  --evaluation-authority /retained/evaluation-authority \
  --dataset /retained/complete-truck.ply \
  --output /fresh/q1-product-quality-view-000001-transaction
```
