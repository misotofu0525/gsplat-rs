#!/usr/bin/env python3
"""Generate candidate two-view quality traces and exact-display variants.

The framing deliberately does not use the global min/max AABB.  It takes a
deterministic midpoint-stratified sample of the complete fixed-record binary
PLY, converts input RDF positions to runtime RUF, and fits the requested
central per-axis quantile box.  Source identity and the derivation receipt are
embedded in every trace so a manually approved candidate remains auditable.

An exact-display variant keeps every pose, view matrix, vertical FOV, and clip
plane from a reviewed source trace.  It changes only the declared drawable
size and the aspect-dependent projection/view-projection matrices.  This is
the formal mobile path: it never stretches a 16:9 image or reprojects a frozen
matrix after loading.
"""

from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import math
import mmap
import pathlib
import re
import struct
from dataclasses import dataclass
from typing import Any

from trace_v1 import (
    SCHEMA,
    mat4_multiply,
    projection_matrix,
    view_matrix,
    with_content_hash,
)


ROOT = pathlib.Path(__file__).resolve().parents[3]
DEFAULT_SAMPLE_COUNT = 200_000
DEFAULT_LOWER_PERCENTILE = 20.0
DEFAULT_YAW_DEGREES = (0.0, 25.0)
COPY_CHUNK_BYTES = 8 * 1024 * 1024
HEADER_LIMIT_BYTES = 1024 * 1024

SCALAR_FORMATS = {
    "char": ("b", 1),
    "int8": ("b", 1),
    "uchar": ("B", 1),
    "uint8": ("B", 1),
    "short": ("h", 2),
    "int16": ("h", 2),
    "ushort": ("H", 2),
    "uint16": ("H", 2),
    "int": ("i", 4),
    "int32": ("i", 4),
    "uint": ("I", 4),
    "uint32": ("I", 4),
    "float": ("f", 4),
    "float32": ("f", 4),
    "double": ("d", 8),
    "float64": ("d", 8),
}


class TraceGenerationError(ValueError):
    pass


@dataclass(frozen=True)
class PlyPositionLayout:
    data_offset: int
    vertex_count: int
    record_bytes: int
    endian: str
    position_fields: tuple[tuple[int, str], tuple[int, str], tuple[int, str]]
    sh_degree: int


def read_header(path: pathlib.Path) -> bytes:
    data = bytearray()
    terminator = re.compile(rb"(?:^|\n)end_header\r?\n")
    with path.open("rb") as handle:
        while len(data) <= HEADER_LIMIT_BYTES:
            chunk = handle.read(min(4096, HEADER_LIMIT_BYTES + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
            match = terminator.search(data)
            if match is not None:
                return bytes(data[: match.end()])
    raise TraceGenerationError(f"{path}: missing bounded PLY header")


def inspect_position_layout(path: pathlib.Path) -> PlyPositionLayout:
    header = read_header(path)
    try:
        lines = header.decode("ascii").splitlines()
    except UnicodeDecodeError as error:
        raise TraceGenerationError(f"{path}: PLY header must be ASCII") from error
    if not lines or lines[0] != "ply":
        raise TraceGenerationError(f"{path}: missing PLY magic")

    endian: str | None = None
    vertex_count: int | None = None
    in_vertex = False
    record_bytes = 0
    fields: dict[str, tuple[int, str]] = {}
    rest_count = 0
    for line in lines[1:]:
        parts = line.split()
        if not parts or parts[0] in {"comment", "obj_info"}:
            continue
        if parts[0] == "format":
            if len(parts) != 3 or parts[2] != "1.0":
                raise TraceGenerationError(f"{path}: unsupported format declaration")
            endian = {"binary_little_endian": "<", "binary_big_endian": ">"}.get(parts[1])
            if endian is None:
                raise TraceGenerationError(f"{path}: candidate framing requires binary PLY")
        elif parts[0] == "element":
            if len(parts) != 3:
                raise TraceGenerationError(f"{path}: malformed element declaration")
            in_vertex = parts[1] == "vertex"
            if in_vertex:
                if vertex_count is not None:
                    raise TraceGenerationError(f"{path}: duplicate vertex element")
                vertex_count = int(parts[2])
        elif parts[0] == "property" and in_vertex:
            if len(parts) >= 2 and parts[1] == "list":
                raise TraceGenerationError(f"{path}: variable-width vertex properties unsupported")
            if len(parts) != 3 or parts[1] not in SCALAR_FORMATS:
                raise TraceGenerationError(f"{path}: unsupported vertex property {line!r}")
            scalar_format, scalar_bytes = SCALAR_FORMATS[parts[1]]
            fields[parts[2]] = (record_bytes, scalar_format)
            record_bytes += scalar_bytes
            if parts[2].startswith("f_rest_"):
                rest_count += 1

    if endian is None or vertex_count is None or vertex_count <= 0 or record_bytes <= 0:
        raise TraceGenerationError(f"{path}: incomplete vertex layout")
    try:
        position_fields = (fields["x"], fields["y"], fields["z"])
    except KeyError as error:
        raise TraceGenerationError(f"{path}: vertex positions require x, y, z") from error
    try:
        sh_degree = {0: 0, 9: 1, 24: 2, 45: 3}[rest_count]
    except KeyError as error:
        raise TraceGenerationError(
            f"{path}: unsupported f_rest property count {rest_count}"
        ) from error
    expected_bytes = len(header) + vertex_count * record_bytes
    if path.stat().st_size != expected_bytes:
        raise TraceGenerationError(
            f"{path}: expected fixed-record size {expected_bytes}, got {path.stat().st_size}"
        )
    return PlyPositionLayout(
        data_offset=len(header),
        vertex_count=vertex_count,
        record_bytes=record_bytes,
        endian=endian,
        position_fields=position_fields,
        sh_degree=sh_degree,
    )


def midpoint_index(output_index: int, output_count: int, source_count: int) -> int:
    return ((2 * output_index + 1) * source_count) // (2 * output_count)


def sample_positions_ruf(
    path: pathlib.Path, layout: PlyPositionLayout, requested_count: int
) -> list[tuple[float, float, float]]:
    sample_count = min(requested_count, layout.vertex_count)
    unpackers = tuple(
        struct.Struct(layout.endian + scalar_format)
        for _, scalar_format in layout.position_fields
    )
    positions: list[tuple[float, float, float]] = []
    with path.open("rb") as handle:
        with mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as mapped:
            for output_index in range(sample_count):
                source_index = midpoint_index(output_index, sample_count, layout.vertex_count)
                base = layout.data_offset + source_index * layout.record_bytes
                values = tuple(
                    float(unpacker.unpack_from(mapped, base + field[0])[0])
                    for unpacker, field in zip(
                        unpackers, layout.position_fields, strict=True
                    )
                )
                # Common 3DGS PLY input is RDF; the renderer flips Y once to RUF.
                position = (values[0], -values[1], values[2])
                if not all(math.isfinite(value) for value in position):
                    raise TraceGenerationError(
                        f"{path}: source vertex {source_index} has non-finite position"
                    )
                positions.append(position)
    return positions


def percentile(sorted_values: list[float], percent: float) -> float:
    if not sorted_values:
        raise TraceGenerationError("cannot derive a percentile from no positions")
    rank = (len(sorted_values) - 1) * percent / 100.0
    lower = math.floor(rank)
    upper = math.ceil(rank)
    if lower == upper:
        return sorted_values[lower]
    fraction = rank - lower
    return sorted_values[lower] * (1.0 - fraction) + sorted_values[upper] * fraction


def robust_bounds(
    positions: list[tuple[float, float, float]], lower_percentile: float
) -> tuple[list[float], list[float]]:
    axes = [[position[axis] for position in positions] for axis in range(3)]
    for axis in axes:
        axis.sort()
    upper_percentile = 100.0 - lower_percentile
    lower = [percentile(axis, lower_percentile) for axis in axes]
    upper = [percentile(axis, upper_percentile) for axis in axes]
    if any(high <= low for low, high in zip(lower, upper, strict=True)):
        raise TraceGenerationError("robust PLY position bounds are degenerate")
    return lower, upper


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(COPY_CHUNK_BYTES):
            digest.update(chunk)
    return digest.hexdigest()


def camera_for_yaw(
    center: list[float],
    lower: list[float],
    upper: list[float],
    yaw_degrees: float,
    vertical_fov: float,
    aspect: float,
) -> tuple[list[float], list[float], float, float]:
    yaw = math.radians(yaw_degrees)
    forward = [math.sin(yaw), 0.0, math.cos(yaw)]
    right = [math.cos(yaw), 0.0, -math.sin(yaw)]
    up = [0.0, 1.0, 0.0]
    corners = list(itertools.product(*zip(lower, upper, strict=True)))
    tan_y = math.tan(vertical_fov * 0.5)
    tan_x = tan_y * aspect
    required_distance = 0.0
    robust_radius = 0.0
    for corner in corners:
        relative = [corner[axis] - center[axis] for axis in range(3)]
        x = sum(relative[axis] * right[axis] for axis in range(3))
        y = sum(relative[axis] * up[axis] for axis in range(3))
        z = sum(relative[axis] * forward[axis] for axis in range(3))
        required_distance = max(
            required_distance,
            abs(x) / tan_x - z,
            abs(y) / tan_y - z,
        )
        robust_radius = max(robust_radius, math.sqrt(sum(value * value for value in relative)))
    distance = max(required_distance * 1.20, robust_radius * 1.05, 0.1)
    position = [center[axis] - forward[axis] * distance for axis in range(3)]
    rotation = [0.0, math.sin(yaw * 0.5), 0.0, math.cos(yaw * 0.5)]
    far = max(100.0, distance + robust_radius * 50.0)
    return position, rotation, distance, far


def matrix_to_quaternion_xyzw(matrix: list[list[float]]) -> list[float]:
    if len(matrix) != 3 or any(len(row) != 3 for row in matrix):
        raise TraceGenerationError("camera rotation must be a 3x3 matrix")
    m00, m01, m02 = matrix[0]
    m10, m11, m12 = matrix[1]
    m20, m21, m22 = matrix[2]
    trace = m00 + m11 + m22
    if trace > 0.0:
        scale = math.sqrt(trace + 1.0) * 2.0
        quaternion = [(m21 - m12) / scale, (m02 - m20) / scale, (m10 - m01) / scale, 0.25 * scale]
    elif m00 > m11 and m00 > m22:
        scale = math.sqrt(1.0 + m00 - m11 - m22) * 2.0
        quaternion = [0.25 * scale, (m01 + m10) / scale, (m02 + m20) / scale, (m21 - m12) / scale]
    elif m11 > m22:
        scale = math.sqrt(1.0 + m11 - m00 - m22) * 2.0
        quaternion = [(m01 + m10) / scale, 0.25 * scale, (m12 + m21) / scale, (m02 - m20) / scale]
    else:
        scale = math.sqrt(1.0 + m22 - m00 - m11) * 2.0
        quaternion = [(m02 + m20) / scale, (m12 + m21) / scale, 0.25 * scale, (m10 - m01) / scale]
    length = math.sqrt(sum(value * value for value in quaternion))
    if not math.isfinite(length) or length <= 0.0:
        raise TraceGenerationError("camera rotation produced an invalid quaternion")
    return [value / length for value in quaternion]


def transformed_official_camera(
    camera: dict[str, Any],
) -> tuple[list[float], list[float], float, dict[str, Any]]:
    try:
        raw_position = [float(value) for value in camera["position"]]
        raw_rotation = [[float(value) for value in row] for row in camera["rotation"]]
        source_width = int(camera["width"])
        source_height = int(camera["height"])
        fx = float(camera["fx"])
        fy = float(camera["fy"])
    except (KeyError, TypeError, ValueError) as error:
        raise TraceGenerationError("malformed official cameras.json entry") from error
    if len(raw_position) != 3 or len(raw_rotation) != 3 or any(
        len(row) != 3 for row in raw_rotation
    ):
        raise TraceGenerationError("malformed official camera pose dimensions")
    values = [*raw_position, *[value for row in raw_rotation for value in row], fx, fy]
    if not all(math.isfinite(value) for value in values):
        raise TraceGenerationError("official camera entry contains non-finite values")
    if source_width <= 0 or source_height <= 0 or fx <= 0.0 or fy <= 0.0:
        raise TraceGenerationError("official camera entry has invalid intrinsics")

    # cameras.json stores camera-to-world in the source's RDF world and RDF
    # camera basis.  Reflecting both world Y and local camera Y preserves a
    # proper rotation while matching the renderer's RUF world and camera basis:
    # R_ruf = diag(1,-1,1) * R_rdf * diag(1,-1,1).
    signs = (1.0, -1.0, 1.0)
    position = [raw_position[axis] * signs[axis] for axis in range(3)]
    rotation_matrix = [
        [raw_rotation[row][column] * signs[row] * signs[column] for column in range(3)]
        for row in range(3)
    ]
    quaternion = matrix_to_quaternion_xyzw(rotation_matrix)
    vertical_fov = 2.0 * math.atan(source_height / (2.0 * fy))
    receipt = {
        "source_camera_id": camera.get("id"),
        "source_image_name": camera.get("img_name"),
        "source_width": source_width,
        "source_height": source_height,
        "source_fx": fx,
        "source_fy": fy,
        "source_horizontal_fov_radians": 2.0 * math.atan(source_width / (2.0 * fx)),
        "source_vertical_fov_radians": vertical_fov,
    }
    return position, quaternion, vertical_fov, receipt


def portable_path(path: pathlib.Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def generate_trace(
    scene_id: str,
    path: pathlib.Path,
    *,
    sample_count: int,
    lower_percentile: float,
    width: int,
    height: int,
    yaw_degrees: tuple[float, ...],
    camera_metadata_path: pathlib.Path | None = None,
    camera_indices: tuple[int, ...] | None = None,
) -> dict[str, Any]:
    layout = inspect_position_layout(path)
    positions = sample_positions_ruf(path, layout, sample_count)
    lower, upper = robust_bounds(positions, lower_percentile)
    center = [(low + high) * 0.5 for low, high in zip(lower, upper, strict=True)]
    aspect = width / height
    near = 0.01
    frames = []
    view_receipts = []
    camera_metadata_receipt: dict[str, Any] | None = None
    if camera_metadata_path is None:
        views: list[tuple[list[float], list[float], float, float, dict[str, Any]]] = []
        vertical_fov = math.radians(55.0)
        for frame_index, yaw in enumerate(yaw_degrees):
            position, rotation, distance, far = camera_for_yaw(
                center, lower, upper, yaw, vertical_fov, aspect
            )
            views.append(
                (
                    position,
                    rotation,
                    vertical_fov,
                    far,
                    {"frame_index": frame_index, "yaw_degrees": yaw, "distance": distance},
                )
            )
        framing_method = "robust_quantile_box_fit"
        fit_margin: float | None = 1.20
    else:
        if not camera_indices:
            raise TraceGenerationError("official camera metadata requires camera indices")
        try:
            camera_metadata = json.loads(camera_metadata_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise TraceGenerationError(
                f"cannot load official camera metadata {camera_metadata_path}: {error}"
            ) from error
        if not isinstance(camera_metadata, list) or not camera_metadata:
            raise TraceGenerationError("official cameras.json must be a non-empty array")
        robust_radius = math.sqrt(
            sum(((upper[axis] - lower[axis]) * 0.5) ** 2 for axis in range(3))
        )
        views = []
        for frame_index, camera_index in enumerate(camera_indices):
            if not 0 <= camera_index < len(camera_metadata):
                raise TraceGenerationError(
                    f"official camera index {camera_index} is outside 0..{len(camera_metadata) - 1}"
                )
            position, rotation, vertical_fov, receipt = transformed_official_camera(
                camera_metadata[camera_index]
            )
            distance = math.sqrt(
                sum((position[axis] - center[axis]) ** 2 for axis in range(3))
            )
            far = max(100.0, distance + robust_radius * 50.0)
            receipt.update(
                {
                    "frame_index": frame_index,
                    "camera_metadata_index": camera_index,
                    "distance_to_robust_center": distance,
                }
            )
            views.append((position, rotation, vertical_fov, far, receipt))
        camera_metadata_receipt = {
            "local_path": portable_path(camera_metadata_path),
            "sha256": sha256(camera_metadata_path),
            "bytes": camera_metadata_path.stat().st_size,
            "entry_count": len(camera_metadata),
            "coordinate_conversion": (
                "R_ruf=diag(1,-1,1)*R_rdf*diag(1,-1,1); position_ruf=(x,-y,z)"
            ),
        }
        framing_method = "official_training_camera_pose_with_robust_far_plane"
        fit_margin = None

    for frame_index, (position, rotation, vertical_fov, far, receipt) in enumerate(views):
        intrinsics = {
            "vertical_fov_radians": vertical_fov,
            "near_plane": near,
            "far_plane": far,
        }
        view = view_matrix(position, rotation)
        projection = projection_matrix(vertical_fov, near, far, aspect)
        frames.append(
            {
                "frame_index": frame_index,
                "timestamp_ns": frame_index * 16_666_667,
                "pose": {"position": position, "rotation_xyzw": rotation},
                "intrinsics": intrinsics,
                "view_matrix": view,
                "projection_matrix": projection,
                "view_projection_matrix": mat4_multiply(projection, view),
            }
        )
        view_receipts.append(receipt)

    source_digest = sha256(path)
    trace = {
        "schema": SCHEMA,
        "trace_id": f"candidate-{scene_id}-quality-{len(frames)}view-{width}x{height}-v1",
        "coordinate_system": {
            "handedness": "right",
            "axes": "RUF",
            "camera_forward": "+Z",
        },
        "matrix_convention": {
            "storage_order": "row-major",
            "vector_convention": "column",
            "composition": "projection * view * world_position",
            "ndc_xy": "[-1,1]",
            "ndc_z": "[0,1]",
            "clip_w": "camera_z",
        },
        "display": {"width": width, "height": height},
        "derivation": {
            "status": "candidate_requires_manual_image_review",
            "source_path": portable_path(path),
            "source_sha256": source_digest,
            "source_bytes": path.stat().st_size,
            "source_splat_count": layout.vertex_count,
            "source_sh_degree": layout.sh_degree,
            "source_coordinate_system": "RDF",
            "runtime_coordinate_system": "RUF",
            "position_sampling": "stratified-midpoint-integer/v1",
            "position_sample_count": len(positions),
            "lower_percentile": lower_percentile,
            "upper_percentile": 100.0 - lower_percentile,
            "robust_bounds_min_ruf": lower,
            "robust_bounds_max_ruf": upper,
            "robust_center_ruf": center,
            "framing_method": framing_method,
            "fit_margin": fit_margin,
            "views": view_receipts,
            "note": (
                "Global extrema do not affect framing distance. The far plane uses "
                "a conservative multiple of robust radius; every source Gaussian "
                "remains loaded even when outside the selected view frustum."
            ),
        },
        "frames": frames,
    }
    if camera_metadata_receipt is not None:
        trace["derivation"]["official_camera_metadata"] = camera_metadata_receipt
    return with_content_hash(trace)


def derive_display_variant(
    source: dict[str, Any], *, width: int, height: int
) -> dict[str, Any]:
    """Return a same-pose/FOV trace projected for one exact target display."""

    if source.get("schema") != SCHEMA:
        raise TraceGenerationError(f"source trace schema must equal {SCHEMA}")
    source_id = source.get("trace_id")
    source_hash = source.get("content_sha256")
    source_display = source.get("display")
    frames = source.get("frames")
    if not isinstance(source_id, str) or not source_id:
        raise TraceGenerationError("source trace requires a non-empty trace_id")
    if not isinstance(source_hash, str) or re.fullmatch(r"[0-9a-f]{64}", source_hash) is None:
        raise TraceGenerationError("source trace requires a lowercase content_sha256")
    if not isinstance(source_display, dict):
        raise TraceGenerationError("source trace requires display metadata")
    if not isinstance(frames, list) or not frames:
        raise TraceGenerationError("source trace requires at least one frame")
    if width <= 0 or height <= 0:
        raise TraceGenerationError("target display dimensions must be positive")

    match = re.fullmatch(r"(.+)-([0-9]+)x([0-9]+)-(v[0-9]+)", source_id)
    if match is None:
        raise TraceGenerationError(
            "source trace_id must end in -<width>x<height>-v<version>"
        )

    # A JSON round trip provides an explicit deep copy while preserving every
    # declared numeric value used by the source trace's camera receipt.
    variant = json.loads(json.dumps(source))
    variant.pop("content_sha256", None)
    variant["trace_id"] = f"{match.group(1)}-{width}x{height}-{match.group(4)}"
    variant["display"] = {"width": width, "height": height}
    aspect = width / height

    for index, frame in enumerate(variant["frames"]):
        if not isinstance(frame, dict):
            raise TraceGenerationError(f"source frame {index} must be an object")
        intrinsics = frame.get("intrinsics")
        view = frame.get("view_matrix")
        if not isinstance(intrinsics, dict) or not isinstance(view, list):
            raise TraceGenerationError(
                f"source frame {index} requires intrinsics and view_matrix"
            )
        try:
            projection = projection_matrix(
                float(intrinsics["vertical_fov_radians"]),
                float(intrinsics["near_plane"]),
                float(intrinsics["far_plane"]),
                aspect,
            )
        except (KeyError, TypeError, ValueError) as error:
            raise TraceGenerationError(
                f"source frame {index} has invalid projection intrinsics"
            ) from error
        frame["projection_matrix"] = projection
        frame["view_projection_matrix"] = mat4_multiply(projection, view)

    derivation = variant.get("derivation")
    if not isinstance(derivation, dict):
        derivation = {}
        variant["derivation"] = derivation
    derivation["display_variant"] = {
        "source_trace_id": source_id,
        "source_trace_sha256": source_hash,
        "source_display": source_display,
        "policy": "same_pose_fov_clip_reprojected_for_exact_target_aspect",
    }
    return with_content_hash(variant)


def parse_scene(value: str) -> tuple[str, pathlib.Path]:
    scene_id, separator, raw_path = value.partition("=")
    if not separator or not scene_id or not raw_path:
        raise argparse.ArgumentTypeError("scene must be ID=PATH")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", scene_id):
        raise argparse.ArgumentTypeError("scene ID must use lowercase letters, digits, and dashes")
    return scene_id, pathlib.Path(raw_path)


def parse_indices(value: str) -> tuple[str, tuple[int, ...]]:
    scene_id, separator, raw_indices = value.partition("=")
    if not separator or not scene_id or not raw_indices:
        raise argparse.ArgumentTypeError("camera indices must be ID=N,N,...")
    try:
        indices = tuple(int(item) for item in raw_indices.split(","))
    except ValueError as error:
        raise argparse.ArgumentTypeError("camera indices must be integers") from error
    if len(indices) < 2 or any(index < 0 for index in indices):
        raise argparse.ArgumentTypeError("camera indices require at least two non-negative values")
    return scene_id, indices


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    source_group = parser.add_mutually_exclusive_group(required=True)
    source_group.add_argument("--scene", action="append", type=parse_scene)
    source_group.add_argument(
        "--derive-from",
        action="append",
        type=pathlib.Path,
        help=(
            "derive a same-pose/FOV exact-display variant from an existing "
            "scene-quality trace; may be repeated"
        ),
    )
    parser.add_argument(
        "--camera-metadata",
        action="append",
        type=parse_scene,
        default=[],
        help="optional ID=official-cameras.json source for training-view candidates",
    )
    parser.add_argument(
        "--camera-indices",
        action="append",
        type=parse_indices,
        default=[],
        help="ID=N,N,... entries selected from the matching cameras.json",
    )
    parser.add_argument("--output-dir", required=True, type=pathlib.Path)
    parser.add_argument("--sample-count", type=int, default=DEFAULT_SAMPLE_COUNT)
    parser.add_argument(
        "--lower-percentile", type=float, default=DEFAULT_LOWER_PERCENTILE
    )
    parser.add_argument("--width", type=int, default=640)
    parser.add_argument("--height", type=int, default=360)
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()
    if args.sample_count <= 0:
        parser.error("--sample-count must be positive")
    if not 0.0 < args.lower_percentile < 50.0:
        parser.error("--lower-percentile must be in (0, 50)")
    if args.width <= 0 or args.height <= 0:
        parser.error("--width and --height must be positive")

    if args.derive_from:
        if args.camera_metadata or args.camera_indices:
            parser.error("--derive-from cannot be combined with camera metadata")
        args.output_dir.mkdir(parents=True, exist_ok=True)
        for source_path in args.derive_from:
            try:
                source = json.loads(source_path.read_text(encoding="utf-8"))
                if not isinstance(source, dict):
                    raise TraceGenerationError("source trace must contain a JSON object")
                trace = derive_display_variant(
                    source, width=args.width, height=args.height
                )
            except (OSError, json.JSONDecodeError, TraceGenerationError) as error:
                parser.error(f"cannot derive {source_path}: {error}")
            match = re.fullmatch(
                r"(.+)-[0-9]+x[0-9]+-(v[0-9]+\.json)", source_path.name
            )
            if match is None:
                parser.error(
                    f"source trace filename must end in -<width>x<height>-vN.json: {source_path}"
                )
            output = (
                args.output_dir
                / f"{match.group(1)}-{args.width}x{args.height}-{match.group(2)}"
            )
            if output.exists() and not args.overwrite:
                parser.error(f"refusing to overwrite {output}; pass --overwrite")
            output.write_text(
                json.dumps(trace, indent=2, ensure_ascii=False) + "\n",
                encoding="utf-8",
            )
            print(
                f"trace={output} source_trace={source_path} "
                f"source_trace_sha256={source['content_sha256']}"
            )
        return 0

    scenes = args.scene or []
    metadata_by_scene = dict(args.camera_metadata)
    indices_by_scene = dict(args.camera_indices)
    if len(metadata_by_scene) != len(args.camera_metadata):
        parser.error("duplicate --camera-metadata scene ID")
    if len(indices_by_scene) != len(args.camera_indices):
        parser.error("duplicate --camera-indices scene ID")
    unknown_metadata = set(metadata_by_scene) - {scene_id for scene_id, _ in scenes}
    unknown_indices = set(indices_by_scene) - {scene_id for scene_id, _ in scenes}
    if unknown_metadata or unknown_indices:
        parser.error("camera metadata/indices contain an unknown scene ID")
    if set(metadata_by_scene) != set(indices_by_scene):
        parser.error("every --camera-metadata requires matching --camera-indices")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    for scene_id, path in scenes:
        path = path.resolve()
        output = (
            args.output_dir
            / f"candidate-{scene_id}-quality-{args.width}x{args.height}-v1.json"
        )
        if output.exists() and not args.overwrite:
            parser.error(f"refusing to overwrite {output}; pass --overwrite")
        trace = generate_trace(
            scene_id,
            path,
            sample_count=args.sample_count,
            lower_percentile=args.lower_percentile,
            width=args.width,
            height=args.height,
            yaw_degrees=DEFAULT_YAW_DEGREES,
            camera_metadata_path=metadata_by_scene.get(scene_id),
            camera_indices=indices_by_scene.get(scene_id),
        )
        output.write_text(
            json.dumps(trace, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        print(
            f"trace={output} source_splats={trace['derivation']['source_splat_count']} "
            f"source_sha256={trace['derivation']['source_sha256']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
