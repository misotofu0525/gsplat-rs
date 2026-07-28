# Camera Trace Contract v1

`gsplat-camera-trace/v1` is the canonical camera input shared by benchmark
engines. A consumer must use the explicit matrices for rendering and treat the
pose and intrinsics as independently checkable source metadata. It must not
recreate an orbit from engine-local yaw/pitch controls.

## Coordinate and matrix convention

- World and camera space are right-handed RUF: `+X` right, `+Y` up, `+Z`
  forward.
- `pose.rotation_xyzw` is a normalized camera-to-world quaternion in `x,y,z,w`
  order. `pose.position` is the camera origin in world coordinates.
- Matrices are arrays of 16 finite JSON numbers in row-major storage order.
- The mathematical convention is column vectors: `p_clip = P * V * p_world`.
- `view_matrix` maps world to camera space. Its rotation is the inverse of the
  camera-to-world quaternion and its translation is `-R * position`.
- `projection_matrix` is right-handed, `+Z`-forward perspective. NDC X/Y are
  `[-1, 1]`; NDC Z uses the WebGPU/Metal/Vulkan interval `[0, 1]`; clip `w =
  camera_z`. There is no hidden OpenGL depth conversion or Y flip.
- `view_projection_matrix` is exactly `projection_matrix * view_matrix`.
- `vertical_fov_radians`, `near_plane`, and `far_plane` define projection;
  aspect is `display.width / display.height`. Optional
  `focal_length_x_over_y` preserves a centered PINHOLE camera's independent
  horizontal focal length and defaults exactly to one for legacy traces.

Engines whose native convention is `-Z` forward, column-major storage, or
OpenGL `[-1,1]` depth must transpose/convert at the API boundary. The committed
matrix values remain the comparison oracle.

## File shape and hashing

The root object contains:

- `schema`, `trace_id`, `content_sha256`;
- frozen `coordinate_system`, `matrix_convention`, and `display` metadata;
- a non-empty `frames` array.

Each frame has a contiguous `frame_index`, strictly increasing `timestamp_ns`,
pose, intrinsics, and all three matrices. Timestamps are offsets from trace
start, not wall-clock timestamps.

`content_sha256` is SHA-256 over the UTF-8 encoding of the root object after
removing `content_sha256`, serialized with sorted keys and compact JSON
separators `(',', ':')`. No trailing newline participates in the hash.

Generate and validate the review fixture with only Python's standard library:

```bash
python3 tests/perf/trace/generate_trace_v1.py --output /tmp/camera-trace-v1.json
python3 tests/perf/trace/validate_trace_v1.py tests/perf/trace/fixtures/camera-trace-v1.json
bash tests/perf/trace/test-trace-v1.sh
```

For a native smoke/contract run, first read the real Surface or drawable
dimensions and generate the same deterministic pose/FOV at that exact aspect:

```bash
python3 tests/perf/trace/generate_trace_v1.py \
  --width 2622 --height 1206 \
  --output target/camera-trace-2622x1206-v1.json
```

Do not hard-code this example size for another endpoint. Android uses the
Activity's actual `SurfaceView` pixels and Apple uses
`CAMetalLayer.drawableSize`; the strict native trace entrypoint rejects a
mismatch. The explicit mobile opt-out is smoke-only native-aspect reprojection,
and its artifact is not formal quality evidence.

Runtime consumers share strict pose, intrinsics, convention, matrix, display,
and frame validation in `gsplat-core::camera_trace` (native) and
`camera-trace-v1.mjs` (Web). The Python command above remains the canonical
content-hash verifier: CPython and Rust/JavaScript can choose different shortest
decimal spellings for the same IEEE-754 value, so runtime consumers preserve
the declared lowercase hash receipt rather than reserializing floats into a
different byte stream.

Current fixed-frame entrypoints (kept for screenshots and steady-state image
parity) are:

- desktop: `--camera-trace PATH --camera-frame N`;
- Android benchmark: `gsplat_camera_trace_path` and
  `gsplat_camera_trace_frame` intent extras;
- iOS benchmark: `--gsplat_camera_trace PATH --gsplat_camera_trace_frame N`;
- Web: `gsplat_camera_trace_url` and `gsplat_camera_frame` query parameters.

All four require the render target to match `display` for formal evidence; they
do not rebuild an orbit or silently rescale the projection. Android/iOS expose
an explicit smoke-only display-match opt-out whose artifact records
`native_aspect_reprojection` and `quality_comparable=false`.

Each endpoint also has an explicit `trace_sequence` benchmark mode. It consumes
the selected `frame_indices` in order, wraps them during warmup/measurement,
and restarts at the first selected index for each measured loop. The default is
all trace frames exactly once, zero warmup, and one loop. Sequence runs require
`sort_interval=1` so every applied camera revision asks the selected CPU, GPU,
or adaptive ordering policy for fresh work. Per-frame receipts contain the
trace ID/hash, source frame index, `timestamp_ns`, phase, loop, and requested
backend; no endpoint may substitute a local yaw orbit.

- desktop: `--camera-sequence`, `--camera-frame-indices`,
  `--camera-warmup-frames`, `--camera-measured-frames`, `--camera-loops`;
- Android: `gsplat_camera_trace_sequence`, `gsplat_camera_frame_indices`,
  benchmark warmup/frames, and `gsplat_camera_trace_loops` extras;
- iOS: the same names as `--` launch arguments;
- Web: the same names as query parameters, with sequence warmup/measured aliases
  `gsplat_camera_trace_warmup_frames` and
  `gsplat_camera_trace_measured_frames`.

The fixture is deliberately three frames and uses a 90-degree vertical FOV so
that its matrices remain easy to audit. It is a contract fixture, not the final
qualification camera path.

## Candidate full-scene quality cameras

Unrelated scenes use separate, manually reviewed quality traces. Generate the
initial two-view candidates from complete fixed-record binary PLYs with:

```bash
python3 tests/perf/trace/generate_scene_quality_traces.py \
  --scene flowers=tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply \
  --scene truck=tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --output-dir target/full-quality-camera-candidates/traces
```

The generator converts positions from input RDF to runtime RUF, takes an exact
identity receipt for the complete source, and derives framing from a
deterministic midpoint-stratified position sample. Its default 20th-to-80th
per-axis quantile box prevents a few extreme Gaussians from shrinking the main
subject as a global AABB does. This changes only the camera composition: the
renderer must still load the complete source and preserve every SH band.

For the official INRIA anchors, prefer two manually selected training cameras
over an invented world-axis view. Fetch their pinned metadata with
`fetch_inria_3dgs_scenes.py --include-cameras`, then pass matching scene IDs:

```bash
python3 tests/perf/trace/generate_scene_quality_traces.py \
  --scene truck=tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --camera-metadata truck=tests/datasets/external/inria_3dgs/truck/cameras.json \
  --camera-indices truck=0,125 \
  --output-dir target/full-quality-camera-candidates/traces
```

The official `cameras.json` rotation is camera-to-world in source RDF. The
generator reflects world Y and local-camera Y to produce a proper RUF rotation,
uses the recorded vertical focal length, and retains robust position bounds for
the conservative far plane and audit receipt. Selected indices are candidates
until both Packed PNGs have been reviewed.

Generated files intentionally say
`candidate_requires_manual_image_review`. Render both frames with the desktop
Packed offscreen path and inspect the PNGs before promoting a trace into an
experiment plan. Do not treat the derivation algorithm alone as image-quality
evidence.

## Formal Truck Product Quality trace

The formal 979x546 two-view trace is derived only from the immutable
`formal-000001-000009` source-camera authority and the separately retained
official Evaluation Images authority:

```bash
PYTHONDONTWRITEBYTECODE=1 python3 \
  tests/perf/build-q1-formal-truck-trace.py \
  --source-camera-authority \
    target/qualification/q1-product-quality-source-camera-formal-v1 \
  --evaluation-authority \
    target/qualification/q1-product-quality-evaluation-authority-v1 \
  --output /fresh/q1-formal-truck-product-quality-trace-v1
```

The output path must not exist. The builder revalidates both authorities,
requires exact views `000001+000009`, 979x546 source and ground-truth images,
centered principal points, and complete byte identities before atomically
publishing `camera-trace.json` with `receipt.json`. It converts the COLMAP RDF
world-to-camera poses to RUF camera-to-world poses and carries the exact
`fx/fy` ratio plus the upstream 3DGS camera's `znear=0.01` and `zfar=100`
into every projection. Its trace content hash binds both complete
authority trees, including each authority receipt and retained source file.

The checked-in fixture is
`fixtures/quality/formal-truck-product-quality-979x546-v1/camera-trace.json`.
It is a formal camera input, not an endpoint image or Product Quality result;
its receipt keeps Product Quality `Deferred` and performance unauthorized.

The committed quality fixtures form an explicit resolution ladder:

- `979x546` only for the independently authored two-view Truck Product Quality
  image gate; it is not a throughput or cross-resolution comparison input;
- `640x360` for quick functional and telemetry diagnostics only;
- `1920x1080` for same-scene desktop/Web product-throughput experiments for
  Kitsune, Flowers, Bonsai, Truck, Garden, and Bicycle;
- `2412x1080` for the connected Nothing A065's observed Android `SurfaceView`;
- `2622x1206` for the booted iPhone 17 Pro simulator's observed Metal drawable;
- `3840x2160` for the Truck desktop pressure/runability experiment.

The 16:9 1080p variants are generated from the same scene source and selected
camera metadata as their `640x360` diagnostic counterpart. Native-aspect
variants are then derived from the 1080p trace with:

```bash
python3 tests/perf/trace/generate_scene_quality_traces.py \
  --derive-from tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json \
  --width 2412 --height 1080 \
  --output-dir target/native-quality-traces
```

Derivation preserves pose, vertical FOV, near/far planes, and the view matrix;
it recomputes only the projection and view-projection matrices for the target
aspect. The trace embeds its parent ID/hash and derivation policy. The matrix
adds a shared camera-family SHA-256 so a mobile trace with a shifted pose or FOV
cannot be paired with the desktop one.

A benchmark must still record and verify its actual Surface size through the
requested, Surface, internal-render, and presented receipts. The checked-in
2412x1080 and 2622x1206 files are tied to the observed A065 and simulator
drawables; re-probe those sizes immediately before a formal run and regenerate
the variants if either changes. A low-resolution result must not be reported as
1080p, native-mobile, or 4K performance, and `native_aspect_reprojection` is
smoke-only rather than formal evidence.
