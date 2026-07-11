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
  aspect is `display.width / display.height`.

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

The fixture is deliberately three frames and uses a 90-degree vertical FOV so
that its matrices remain easy to audit. It is a contract fixture, not the final
qualification camera path.
