"""Build the immutable two-view Truck Product Quality camera trace.

The builder consumes only the already validated source-camera and evaluation
authorities.  It never opens a renderer, browser, device, or performance
collector.  The trace and its receipt bind every retained input file by byte
identity so a later endpoint cannot silently substitute a camera or reference
image.
"""

from __future__ import annotations

import json
import math
import os
import pathlib
import shutil
import sys
import tempfile
from typing import Any


PERF_DIR = pathlib.Path(__file__).resolve().parent
TRACE_DIR = PERF_DIR / "trace"

for module_dir in (PERF_DIR, TRACE_DIR):
    module_text = os.fspath(module_dir)
    if module_text not in sys.path:
        sys.path.insert(0, module_text)

import q1_product_quality_authority as CAMERA_AUTHORITY
import q1_product_quality_evaluation_authority as EVALUATION_AUTHORITY
from trace_v1 import SCHEMA as TRACE_SCHEMA
from trace_v1 import mat4_multiply, projection_matrix, view_matrix, with_content_hash
from validate_trace_v1 import ValidationError as TraceValidationError
from validate_trace_v1 import validate as validate_trace_v1


SCHEMA = "gsplat-q1-formal-truck-trace-authority/v1"
AUTHORITY_CLASS = "upstream_derived_camera_trace"
TRACE_ID = "formal-truck-product-quality-000001-000009-979x546-v1"
VIEW_IDS = ("000001", "000009")
WIDTH = 979
HEIGHT = 546
NEAR_PLANE = 0.01
FAR_PLANE = 100.0
FRAME_INTERVAL_NS = 16_666_667
MAX_JSON_BYTES = 256 * 1024


class FormalTraceError(ValueError):
    pass


def fail(message: str) -> None:
    raise FormalTraceError(message)


def canonical_json(value: Any) -> str:
    try:
        return CAMERA_AUTHORITY.canonical_json(value)
    except CAMERA_AUTHORITY.AuthorityError as error:
        fail(str(error))


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"JSON repeats key {key!r}")
        result[key] = value
    return result


def load_json(path: pathlib.Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        fail(f"missing regular JSON file: {path.name}")
    if path.stat().st_size > MAX_JSON_BYTES:
        fail(f"{path.name} exceeds the bounded JSON size")
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=unique_object,
            parse_constant=lambda value: fail(f"non-finite JSON number: {value}"),
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"cannot read {path.name}: {error}")
    if not isinstance(value, dict):
        fail(f"{path.name} must contain a JSON object")
    return value


def tree_identity(root: pathlib.Path, receipt: dict[str, Any]) -> dict[str, Any]:
    files: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            fail(f"authority contains a symlink: {path.relative_to(root)}")
        if path.is_file():
            files.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "bytes": path.stat().st_size,
                    "sha256": CAMERA_AUTHORITY.sha256(path),
                }
            )
    if not files or files[0]["path"] != "authority.json":
        fail("authority tree identity is incomplete")
    return {
        "schema": receipt.get("schema"),
        "authority_class": receipt.get("authority_class"),
        "files": files,
    }


def _world_to_camera_rotation(qvec_wxyz: list[float]) -> list[list[float]]:
    if len(qvec_wxyz) != 4:
        fail("COLMAP qvec must contain four values")
    w, x, y, z = qvec_wxyz
    norm = math.sqrt(sum(value * value for value in qvec_wxyz))
    if not math.isfinite(norm) or abs(norm - 1.0) > 1.0e-9:
        fail("COLMAP qvec must be normalized")
    return [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]


def runtime_pose(colmap: dict[str, Any]) -> tuple[list[float], list[float]]:
    """Convert COLMAP RDF world-to-camera pose into runtime RUF camera pose."""

    try:
        qvec = [float(value) for value in colmap["qvec_wxyz"]]
        tvec = [float(value) for value in colmap["tvec"]]
    except (KeyError, TypeError, ValueError) as error:
        fail(f"malformed COLMAP pose: {error}")
    if len(tvec) != 3 or not all(math.isfinite(value) for value in (*qvec, *tvec)):
        fail("COLMAP pose must contain finite values")
    rotation = _world_to_camera_rotation(qvec)
    position_rdf = [
        -sum(rotation[row][column] * tvec[row] for row in range(3))
        for column in range(3)
    ]
    position_ruf = [position_rdf[0], -position_rdf[1], position_rdf[2]]

    # qvec is RDF world-to-camera.  Conjugation gives RDF camera-to-world;
    # reflecting both world Y and camera Y maps its vector part to
    # (qx,-qy,qz) while preserving a proper rotation and scalar component.
    w, x, y, z = qvec
    rotation_xyzw = [x, -y, z, w]
    return position_ruf, rotation_xyzw


def _entry_by_path(receipt: dict[str, Any], path: str) -> dict[str, Any]:
    entries = receipt.get("entries")
    matches = (
        [
            entry
            for entry in entries
            if isinstance(entry, dict)
            and entry.get("source_relative_path") == path
        ]
        if isinstance(entries, list)
        else []
    )
    if len(matches) != 1:
        fail(f"evaluation authority requires exactly one {path}")
    return matches[0]


def validate_inputs(
    source_camera_root: pathlib.Path,
    evaluation_root: pathlib.Path,
    *,
    source_spec: CAMERA_AUTHORITY.AuthoritySpec = (
        CAMERA_AUTHORITY.FORMAL_OFFICIAL_SPEC
    ),
    evaluation_entries: tuple[EVALUATION_AUTHORITY.EntrySpec, ...] = (
        EVALUATION_AUTHORITY.OFFICIAL_ENTRIES
    ),
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    try:
        source_receipt = CAMERA_AUTHORITY.validate_authority(
            source_camera_root, spec=source_spec
        )
        evaluation_receipt = EVALUATION_AUTHORITY.validate_authority(
            evaluation_root, evaluation_entries
        )
    except (
        CAMERA_AUTHORITY.AuthorityError,
        EVALUATION_AUTHORITY.EvaluationAuthorityError,
    ) as error:
        fail(f"input authority rejected: {error}")

    scene = source_receipt.get("scene")
    if (
        not isinstance(scene, dict)
        or scene.get("id") != "inria-3dgs-truck-iteration-30000"
        or evaluation_receipt.get("scene") != "truck"
    ):
        fail("input authorities do not describe the formal Truck scene")
    views = source_receipt.get("views")
    if not isinstance(views, list) or [view.get("name") for view in views] != [
        f"{view_id}.jpg" for view_id in VIEW_IDS
    ]:
        fail("source-camera authority does not contain formal views 000001+000009")
    for view_id, view in zip(VIEW_IDS, views, strict=True):
        jpeg = view.get("jpeg")
        scale = view.get("intrinsic_scale")
        if not isinstance(jpeg, dict) or not isinstance(scale, dict):
            fail(f"{view_id}: source-camera receipt is incomplete")
        if (jpeg.get("width"), jpeg.get("height")) != (WIDTH, HEIGHT):
            fail(f"{view_id}: source-camera dimensions must be 979x546")
        gt = _entry_by_path(evaluation_receipt, f"gt/{view_id}.png")
        png = gt.get("png")
        if not isinstance(png, dict) or (
            png.get("width"),
            png.get("height"),
        ) != (WIDTH, HEIGHT):
            fail(f"{view_id}: evaluation ground truth dimensions must be 979x546")
        try:
            fx = float(scale["fx"])
            fy = float(scale["fy"])
            cx = float(scale["cx"])
            cy = float(scale["cy"])
        except (KeyError, TypeError, ValueError) as error:
            fail(f"{view_id}: scaled PINHOLE intrinsics are malformed: {error}")
        if (
            not all(math.isfinite(value) for value in (fx, fy, cx, cy))
            or fx <= 0.0
            or fy <= 0.0
            or cx != WIDTH / 2.0
            or cy != HEIGHT / 2.0
        ):
            fail(f"{view_id}: Product Quality requires centered finite PINHOLE intrinsics")

    bindings = {
        "source_camera": tree_identity(source_camera_root, source_receipt),
        "evaluation_images": tree_identity(evaluation_root, evaluation_receipt),
    }
    return source_receipt, evaluation_receipt, bindings


def generate_trace(
    source_camera_root: pathlib.Path,
    evaluation_root: pathlib.Path,
    *,
    source_spec: CAMERA_AUTHORITY.AuthoritySpec = (
        CAMERA_AUTHORITY.FORMAL_OFFICIAL_SPEC
    ),
    evaluation_entries: tuple[EVALUATION_AUTHORITY.EntrySpec, ...] = (
        EVALUATION_AUTHORITY.OFFICIAL_ENTRIES
    ),
) -> dict[str, Any]:
    source, evaluation, bindings = validate_inputs(
        source_camera_root,
        evaluation_root,
        source_spec=source_spec,
        evaluation_entries=evaluation_entries,
    )
    frames: list[dict[str, Any]] = []
    view_bindings: list[dict[str, Any]] = []
    for frame_index, (view_id, source_view) in enumerate(
        zip(VIEW_IDS, source["views"], strict=True)
    ):
        scale = source_view["intrinsic_scale"]
        fx = float(scale["fx"])
        fy = float(scale["fy"])
        focal_ratio = fx / fy
        vertical_fov = 2.0 * math.atan(HEIGHT / (2.0 * fy))
        position, rotation = runtime_pose(source_view["colmap"])
        view = view_matrix(position, rotation)
        projection = projection_matrix(
            vertical_fov,
            NEAR_PLANE,
            FAR_PLANE,
            WIDTH / HEIGHT,
            focal_ratio,
        )
        frames.append(
            {
                "frame_index": frame_index,
                "timestamp_ns": frame_index * FRAME_INTERVAL_NS,
                "pose": {"position": position, "rotation_xyzw": rotation},
                "intrinsics": {
                    "vertical_fov_radians": vertical_fov,
                    "near_plane": NEAR_PLANE,
                    "far_plane": FAR_PLANE,
                    "focal_length_x_over_y": focal_ratio,
                },
                "view_matrix": view,
                "projection_matrix": projection,
                "view_projection_matrix": mat4_multiply(projection, view),
            }
        )
        gt = _entry_by_path(evaluation, f"gt/{view_id}.png")
        view_bindings.append(
            {
                "frame_index": frame_index,
                "view_id": view_id,
                "source_image": {
                    key: source_view["jpeg"][key] for key in ("path", "bytes", "sha256")
                },
                "ground_truth": {
                    "path": gt["retained_relative_path"],
                    "bytes": gt["bytes"],
                    "sha256": gt["sha256"],
                },
                "colmap_image_id": source_view["colmap"]["image_id"],
                "centered_principal_point": [
                    float(scale["cx"]),
                    float(scale["cy"]),
                ],
                "scaled_focal_lengths": [fx, fy],
            }
        )

    trace = {
        "schema": TRACE_SCHEMA,
        "trace_id": TRACE_ID,
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
        "display": {"width": WIDTH, "height": HEIGHT},
        "derivation": {
            "status": "formal_product_quality_camera_input",
            "scene": source["scene"],
            "view_ids": list(VIEW_IDS),
            "view_bindings": view_bindings,
            "input_authorities": bindings,
            "intrinsics_policy": "centered_colmap_pinhole_fx_fy_preserved",
            "coordinate_conversion": (
                "COLMAP_RDF_world_to_camera_to_RUF_camera_to_world"
            ),
            "clip_policy": {
                "near_plane": NEAR_PLANE,
                "far_plane": FAR_PLANE,
                "source": "upstream_3dgs_camera_znear_zfar",
            },
            "endpoint_output": False,
            "performance_authorized": False,
        },
        "frames": frames,
    }
    trace = with_content_hash(trace)
    try:
        validate_trace_v1(trace)
    except TraceValidationError as error:
        fail(f"generated camera trace failed v1 validation: {error}")
    return trace


def receipt_for_trace(trace_path: pathlib.Path, trace: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "authority_class": AUTHORITY_CLASS,
        "trace": {
            "path": "camera-trace.json",
            "bytes": trace_path.stat().st_size,
            "sha256": CAMERA_AUTHORITY.sha256(trace_path),
            "schema": trace["schema"],
            "trace_id": trace["trace_id"],
            "content_sha256": trace["content_sha256"],
            "display": trace["display"],
            "view_ids": list(VIEW_IDS),
        },
        "input_authorities": trace["derivation"]["input_authorities"],
        "qualification": {
            "product_quality": "Deferred",
            "performance_authorized": False,
            "endpoint_output": False,
        },
    }


def validate_output(
    output: pathlib.Path,
    source_camera_root: pathlib.Path,
    evaluation_root: pathlib.Path,
    *,
    source_spec: CAMERA_AUTHORITY.AuthoritySpec = (
        CAMERA_AUTHORITY.FORMAL_OFFICIAL_SPEC
    ),
    evaluation_entries: tuple[EVALUATION_AUTHORITY.EntrySpec, ...] = (
        EVALUATION_AUTHORITY.OFFICIAL_ENTRIES
    ),
) -> tuple[dict[str, Any], dict[str, Any]]:
    if output.is_symlink() or not output.is_dir():
        fail("formal trace output must be a real directory")
    if {path.name for path in output.iterdir()} != {
        "camera-trace.json",
        "receipt.json",
    }:
        fail("formal trace output file set mismatch")
    trace_path = output / "camera-trace.json"
    receipt_path = output / "receipt.json"
    trace = load_json(trace_path)
    receipt = load_json(receipt_path)
    expected_trace = generate_trace(
        source_camera_root,
        evaluation_root,
        source_spec=source_spec,
        evaluation_entries=evaluation_entries,
    )
    if canonical_json(trace) != canonical_json(expected_trace):
        fail("camera trace does not match the bound authority inputs")
    expected_receipt = receipt_for_trace(trace_path, trace)
    if canonical_json(receipt) != canonical_json(expected_receipt):
        fail("formal trace receipt does not match the immutable output")
    return trace, receipt


def build_output(
    source_camera_root: pathlib.Path,
    evaluation_root: pathlib.Path,
    output: pathlib.Path,
    *,
    source_spec: CAMERA_AUTHORITY.AuthoritySpec = (
        CAMERA_AUTHORITY.FORMAL_OFFICIAL_SPEC
    ),
    evaluation_entries: tuple[EVALUATION_AUTHORITY.EntrySpec, ...] = (
        EVALUATION_AUTHORITY.OFFICIAL_ENTRIES
    ),
) -> tuple[dict[str, Any], dict[str, Any]]:
    source_camera_root = pathlib.Path(os.path.abspath(source_camera_root))
    evaluation_root = pathlib.Path(os.path.abspath(evaluation_root))
    output = pathlib.Path(os.path.abspath(output))
    if output.exists() or output.is_symlink():
        fail(f"output already exists: {output}")
    for label, authority_root in (
        ("source-camera", source_camera_root),
        ("evaluation", evaluation_root),
    ):
        try:
            output.relative_to(authority_root)
        except ValueError:
            continue
        fail(f"output must not be inside the {label} authority root")
    if output.parent.is_symlink() or not output.parent.is_dir():
        fail("output parent must be a real existing directory")
    trace = generate_trace(
        source_camera_root,
        evaluation_root,
        source_spec=source_spec,
        evaluation_entries=evaluation_entries,
    )
    staging = pathlib.Path(
        tempfile.mkdtemp(prefix=f".{output.name}.staging-", dir=output.parent)
    )
    try:
        trace_path = staging / "camera-trace.json"
        trace_path.write_text(canonical_json(trace) + "\n", encoding="utf-8")
        receipt = receipt_for_trace(trace_path, trace)
        (staging / "receipt.json").write_text(
            canonical_json(receipt) + "\n", encoding="utf-8"
        )
        validate_output(
            staging,
            source_camera_root,
            evaluation_root,
            source_spec=source_spec,
            evaluation_entries=evaluation_entries,
        )
        CAMERA_AUTHORITY.publish_directory_noreplace(staging, output)
        return validate_output(
            output,
            source_camera_root,
            evaluation_root,
            source_spec=source_spec,
            evaluation_entries=evaluation_entries,
        )
    finally:
        if staging.exists():
            shutil.rmtree(staging)
