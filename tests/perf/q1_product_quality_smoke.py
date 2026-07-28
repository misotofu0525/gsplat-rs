"""Offline admission for the Q1 formal Truck view ``000001`` quality smoke.

This module consumes renderer-owned capture artifacts that were produced by
existing endpoint collectors.  It never launches an endpoint and intentionally
does not retain benchmark timings.  Structural or provenance failures raise
``OneViewQualityError``; a well-formed image that misses a frozen threshold is
published as a finite ``Rejected`` endpoint decision.
"""

from __future__ import annotations

import ctypes
import errno
import hashlib
import importlib.util
import json
import math
import os
import pathlib
import shutil
import sys
import tempfile
from dataclasses import asdict
from typing import Any


PERF_DIR = pathlib.Path(__file__).resolve().parent
FORMAL_TRACE_FIXTURE = (
    PERF_DIR
    / "trace/fixtures/quality/formal-truck-product-quality-979x546-v1"
)
for module_dir in (PERF_DIR, PERF_DIR / "trace"):
    module_text = os.fspath(module_dir)
    if module_text not in sys.path:
        sys.path.insert(0, module_text)

import q1_formal_truck_trace_authority as TRACE_AUTHORITY
import q1_product_quality_evaluation_authority as EVALUATION_AUTHORITY
from validate_trace_v1 import ValidationError as TraceValidationError
from validate_trace_v1 import validate as validate_trace_v1


def _load_product_quality() -> Any:
    path = PERF_DIR / "q1_pair_admission/product_quality.py"
    spec = importlib.util.spec_from_file_location("q1_one_view_product_quality", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load the frozen Product Quality reducer")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


PRODUCT_QUALITY = _load_product_quality()
FORMAL_HEIGHT = PRODUCT_QUALITY.FORMAL_HEIGHT
FORMAL_WIDTH = PRODUCT_QUALITY.FORMAL_WIDTH
RGB_MAE_NORMALIZED_MAXIMUM = PRODUCT_QUALITY.RGB_MAE_NORMALIZED_MAXIMUM
SEVERE_PIXEL_FRACTION_MAXIMUM = PRODUCT_QUALITY.SEVERE_PIXEL_FRACTION_MAXIMUM
SEVERE_RGB_ERROR_THRESHOLD_8BIT = PRODUCT_QUALITY.SEVERE_RGB_ERROR_THRESHOLD_8BIT
SSIM_MINIMUM = PRODUCT_QUALITY.SSIM_MINIMUM


SCHEMA = "gsplat-q1-product-quality-one-view/v1"
VIEW_ID = "000001"
TRACE_FRAME_INDEX = 0
TRUCK_ID = "inria-3dgs-truck-iteration-30000"
TRUCK_SHA256 = "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c"
TRUCK_SPLAT_COUNT = 2_541_226
TRUCK_SH_DEGREE = 3
NATIVE_ARTIFACT_SCHEMA = "gsplat-benchmark/v1"
PLAYCANVAS_CAPTURE_SCHEMA = "gsplat-playcanvas-webgpu-renderer-capture/v1"
PLAYCANVAS_CAPTURE_PRODUCER = "playcanvas_webgpu_copy_texture_to_buffer"
PLAYCANVAS_MATERIALIZATION_SCHEMA = (
    "gsplat-playcanvas-renderer-capture-materialization/v1"
)
PLAYCANVAS_CAMERA_SCHEMA = "gsplat-playcanvas-runtime-camera-receipt/v1"
MAX_JSON_BYTES = 4 * 1024 * 1024
MAX_JSONL_BYTES = 64 * 1024 * 1024
SHA256_LENGTH = 64


class OneViewQualityError(ValueError):
    """The offline evidence cannot prove the one-view quality input."""


def fail(message: str) -> None:
    raise OneViewQualityError(message)


def _canonical_json(value: Any) -> str:
    try:
        return json.dumps(
            value,
            allow_nan=False,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        )
    except (TypeError, ValueError) as error:
        fail(f"value is not canonical JSON: {error}")


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(_canonical_json(value).encode()).hexdigest()


def file_sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(block)
    except OSError as error:
        fail(f"cannot hash {path}: {error}")
    return digest.hexdigest()


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"JSON repeats key {key!r}")
        result[key] = value
    return result


def _load_json(path: pathlib.Path, context: str, *, limit: int = MAX_JSON_BYTES) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        fail(f"{context} must be a regular file")
    if path.stat().st_size > limit:
        fail(f"{context} exceeds its bounded size")
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_unique_object,
            parse_constant=lambda token: fail(f"{context} contains non-finite {token}"),
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"cannot read {context}: {error}")
    if not isinstance(value, dict):
        fail(f"{context} must contain an object")
    return value


def _load_jsonl(path: pathlib.Path, context: str) -> list[dict[str, Any]]:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_JSONL_BYTES:
        fail(f"{context} must be a bounded regular file")
    records: list[dict[str, Any]] = []
    try:
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if not line.strip():
                continue
            value = json.loads(
                line,
                object_pairs_hook=_unique_object,
                parse_constant=lambda token: fail(
                    f"{context}:{line_number} contains non-finite {token}"
                ),
            )
            if not isinstance(value, dict):
                fail(f"{context}:{line_number} must contain an object")
            records.append(value)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"cannot read {context}: {error}")
    return records


def _sha256(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != SHA256_LENGTH
        or any(character not in "0123456789abcdef" for character in value)
    ):
        fail(f"{context} must be a lowercase SHA-256")
    return value


def _object(parent: dict[str, Any], key: str, context: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        fail(f"{context}.{key} must be an object")
    return value


def _positive_integer(value: Any, context: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        fail(f"{context} must be a positive integer")
    return value


def _finite_close(actual: Any, expected: float, context: str) -> None:
    if (
        not isinstance(actual, (int, float))
        or isinstance(actual, bool)
        or not math.isfinite(actual)
        or abs(float(actual) - expected) > 2.0e-4 + 2.0e-5 * abs(expected)
    ):
        fail(f"{context} does not match the formal camera")


def _regular_child(root: pathlib.Path, name: str, context: str) -> pathlib.Path:
    if root.is_symlink() or not root.is_dir():
        fail(f"{context} root must be a real directory")
    path = root / name
    if path.is_symlink() or not path.is_file():
        fail(f"{context}/{name} must be a regular file")
    try:
        path.resolve(strict=True).relative_to(root.resolve(strict=True))
    except (OSError, ValueError):
        fail(f"{context}/{name} escapes its artifact root")
    return path


def _load_png_decoder() -> Any:
    path = PERF_DIR / "validate-balanced-image-gate.py"
    spec = importlib.util.spec_from_file_location("q1_one_view_png_decoder", path)
    if spec is None or spec.loader is None:
        fail("cannot load the repository PNG decoder")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


PNG = _load_png_decoder()


def _decode_png(path: pathlib.Path, context: str) -> bytes:
    try:
        decoded = PNG.decode_rgba8_png(
            path.read_bytes(), context, (FORMAL_WIDTH, FORMAL_HEIGHT)
        )
    except (OSError, PNG.ValidationError) as error:
        fail(f"{context} is not a formal raw RGB8/RGBA8 PNG: {error}")
    return decoded.rgba


def _formal_trace(trace_root: pathlib.Path, evaluation_root: pathlib.Path) -> dict[str, Any]:
    """Validate the immutable trace and bind its evaluation input tree."""

    trace_path = _regular_child(trace_root, "camera-trace.json", "formal trace")
    receipt_path = _regular_child(trace_root, "receipt.json", "formal trace")
    if {path.name for path in trace_root.iterdir()} != {"camera-trace.json", "receipt.json"}:
        fail("formal trace authority file set mismatch")
    trace = _load_json(trace_path, "formal trace camera-trace.json")
    receipt = _load_json(receipt_path, "formal trace receipt.json")
    try:
        validate_trace_v1(trace)
        evaluation = EVALUATION_AUTHORITY.validate_authority(evaluation_root)
    except (TraceValidationError, EVALUATION_AUTHORITY.EvaluationAuthorityError) as error:
        fail(f"formal input authority rejected: {error}")

    fixture_trace = _load_json(
        FORMAL_TRACE_FIXTURE / "camera-trace.json", "checked formal trace fixture"
    )
    fixture_receipt = _load_json(
        FORMAL_TRACE_FIXTURE / "receipt.json", "checked formal trace fixture receipt"
    )
    if _canonical_json(trace) != _canonical_json(fixture_trace):
        fail("formal trace differs from the reviewed checked-in authority")
    expected_receipt = TRACE_AUTHORITY.receipt_for_trace(trace_path, trace)
    if _canonical_json(receipt) != _canonical_json(expected_receipt):
        fail("formal trace receipt does not match its retained trace bytes")
    if (
        receipt.get("schema") != TRACE_AUTHORITY.SCHEMA
        or receipt.get("authority_class") != TRACE_AUTHORITY.AUTHORITY_CLASS
        or receipt.get("qualification")
        != {
            "product_quality": "Deferred",
            "performance_authorized": False,
            "endpoint_output": False,
        }
    ):
        fail("formal trace receipt has an invalid authority boundary")

    # Recheck the external Evaluation Images tree and require the trace to bind
    # exactly that retained tree. The source-camera tree is locked by the
    # reviewed formal fixture because it is not a CLI input to this gate.
    actual_evaluation_tree = TRACE_AUTHORITY.tree_identity(evaluation_root, evaluation)
    bound_inputs = _object(trace.get("derivation", {}), "input_authorities", "trace.derivation")
    if bound_inputs.get("evaluation_images") != actual_evaluation_tree:
        fail("formal trace does not bind the supplied Evaluation Images authority")
    if bound_inputs.get("source_camera") != fixture_receipt["input_authorities"]["source_camera"]:
        fail("formal trace source-camera authority identity drifted")

    frame = trace["frames"][TRACE_FRAME_INDEX]
    if (
        trace.get("trace_id") != TRACE_AUTHORITY.TRACE_ID
        or trace.get("content_sha256") != fixture_trace["content_sha256"]
        or trace.get("display") != {"width": FORMAL_WIDTH, "height": FORMAL_HEIGHT}
        or trace["derivation"].get("view_ids") != list(TRACE_AUTHORITY.VIEW_IDS)
        or frame.get("frame_index") != TRACE_FRAME_INDEX
    ):
        fail("formal trace does not identify view 000001 at 979x546")
    ratio = frame.get("intrinsics", {}).get("focal_length_x_over_y")
    _finite_close(ratio, 1.005624011459175, "formal focal_length_x_over_y")
    return {
        "trace": trace,
        "trace_receipt": receipt,
        "trace_receipt_sha256": file_sha256(receipt_path),
        "trace_file_sha256": file_sha256(trace_path),
        "evaluation": evaluation,
        "evaluation_receipt_sha256": file_sha256(evaluation_root / "authority.json"),
        "pose_intrinsics_sha256": canonical_sha256(
            {"pose": frame["pose"], "intrinsics": frame["intrinsics"]}
        ),
        "focal_length_x_over_y": float(ratio),
    }


def _require_truck(manifest: dict[str, Any], context: str) -> None:
    dataset = _object(manifest, "dataset", context)
    if (
        dataset.get("sha256") != TRUCK_SHA256
        or dataset.get("splat_count") != TRUCK_SPLAT_COUNT
        or dataset.get("sh_degree") != TRUCK_SH_DEGREE
        or dataset.get("id") not in {TRUCK_ID, "truck.ply"}
    ):
        fail(f"{context}.dataset is not complete formal Truck")
    exactness = _object(manifest, "exactness", context)
    for field in (
        "source_splat_count",
        "decoded_splat_count",
        "resident_splat_count",
    ):
        if exactness.get(field) != TRUCK_SPLAT_COUNT:
            fail(f"{context}.exactness.{field} mismatch")
    if (
        exactness.get("source_sh_degree") != TRUCK_SH_DEGREE
        or exactness.get("resident_sh_degree") != TRUCK_SH_DEGREE
        or exactness.get("source_membership") != "all"
        or exactness.get("sampling") != "disabled"
        or exactness.get("lod") != "disabled"
        or exactness.get("partial_scene_published") is not False
        or exactness.get("full_quality") is not True
    ):
        fail(f"{context}.exactness does not preserve the complete source")


def _require_resolution(value: dict[str, Any], context: str, *, presented: bool) -> None:
    stages = ["requested", "surface", "internal_render"]
    if presented:
        stages.append("presented")
    for stage in stages:
        if (
            value.get(f"{stage}_width"),
            value.get(f"{stage}_height"),
        ) != (FORMAL_WIDTH, FORMAL_HEIGHT):
            fail(f"{context}.{stage} must equal 979x546")
    if (
        value.get("dynamic_resolution") != "disabled"
        or value.get("upscaling") != "disabled"
    ):
        fail(f"{context} must disable dynamic resolution and upscaling")
    full_key = "full_resolution" if presented else "internal_full_resolution"
    if value.get(full_key) is not True:
        fail(f"{context}.{full_key} must be true")


def _native_capture(root: pathlib.Path, formal: dict[str, Any]) -> tuple[bytes, dict[str, Any]]:
    manifest_path = _regular_child(root, "manifest.json", "gsplat-rs capture")
    frames_path = _regular_child(root, "frames.jsonl", "gsplat-rs capture")
    png_path = _regular_child(root, "final-frame.png", "gsplat-rs capture")
    for blocker in ("blocker.json", "cleanup-blocker.json"):
        if (root / blocker).exists() or (root / blocker).is_symlink():
            fail(f"gsplat-rs capture contains {blocker}")
    manifest = _load_json(manifest_path, "gsplat-rs manifest")
    frames = _load_jsonl(frames_path, "gsplat-rs frames")
    if manifest.get("schema") != NATIVE_ARTIFACT_SCHEMA:
        fail("gsplat-rs manifest schema mismatch")
    _require_truck(manifest, "gsplat-rs manifest")
    _require_resolution(_object(manifest, "resolution", "gsplat-rs manifest"), "gsplat-rs resolution", presented=True)
    trace = _object(manifest, "trace", "gsplat-rs manifest")
    if (
        trace.get("id") != formal["trace"]["trace_id"]
        or trace.get("sha256") != formal["trace"]["content_sha256"]
    ):
        fail("gsplat-rs manifest does not bind the formal trace")
    q1 = _object(manifest, "q1_comparison", "gsplat-rs manifest")
    if q1.get("artifact_role") != "control" or q1.get("performance_evidence") is not False:
        fail("gsplat-rs artifact must be a non-performance control")
    presentation = _object(q1, "presentation_identity", "gsplat-rs q1_comparison")
    if presentation.get("trace_frame_index") != TRACE_FRAME_INDEX:
        fail("gsplat-rs presentation does not select view 000001")
    camera = _object(presentation, "camera", "gsplat-rs presentation")
    expected_camera = {
        "trace_id": formal["trace"]["trace_id"],
        "trace_content_sha256": formal["trace"]["content_sha256"],
        "trace_frame_index": TRACE_FRAME_INDEX,
        "pose_intrinsics_sha256": formal["pose_intrinsics_sha256"],
    }
    for field, expected in expected_camera.items():
        if camera.get(field) != expected:
            fail(f"gsplat-rs camera.{field} mismatch")
    camera_revision = _positive_integer(camera.get("camera_revision"), "gsplat-rs camera_revision")
    terminal = _object(presentation, "terminal_identity", "gsplat-rs presentation")
    terminal_index = terminal.get("frame_index")
    if not isinstance(terminal_index, int) or isinstance(terminal_index, bool):
        fail("gsplat-rs terminal frame index must be an integer")
    if terminal_index < 0 or terminal_index >= len(frames):
        fail("gsplat-rs terminal frame index is out of range")
    terminal_frame = frames[terminal_index]
    if terminal.get("frame_sha256") != canonical_sha256(terminal_frame):
        fail("gsplat-rs terminal frame hash mismatch")
    presentation_sequence = _positive_integer(
        terminal.get("presentation_sequence"), "gsplat-rs presentation sequence"
    )
    if (
        terminal_frame.get("trace_frame_index") != TRACE_FRAME_INDEX
        or terminal_frame.get("camera_revision") != camera_revision
        or terminal_frame.get("presentation_sequence") != presentation_sequence
    ):
        fail("gsplat-rs terminal frame is not the bound presentation")
    capture = _object(terminal_frame, "capture_depth_precision", "gsplat-rs terminal frame")
    expected_capture_fields = {
        "scene_generation",
        "camera_revision",
        "viewport_generation",
        "contract_generation",
        "plan_set_generation",
        "plan_id",
        "order_generation",
        "presentation_sequence",
        "width",
        "height",
        "rgba8_sha256",
        "profile",
    }
    if set(capture) != expected_capture_fields:
        fail("gsplat-rs diagnostic capture receipt fields are not frozen")
    if (
        capture.get("camera_revision") != camera_revision
        or capture.get("presentation_sequence") != presentation_sequence
        or capture.get("width") != FORMAL_WIDTH
        or capture.get("height") != FORMAL_HEIGHT
        or capture.get("profile") != "ExactFull32"
        or capture.get("plan_id") not in {"CpuPostSort", "GpuPostSort", "GpuPreproject"}
    ):
        fail("gsplat-rs diagnostic capture receipt identity mismatch")
    for field in (
        "scene_generation",
        "contract_generation",
        "plan_set_generation",
        "order_generation",
    ):
        _positive_integer(capture.get(field), f"gsplat-rs capture.{field}")
    if not isinstance(capture.get("viewport_generation"), int) or isinstance(
        capture.get("viewport_generation"), bool
    ):
        fail("gsplat-rs capture.viewport_generation must be an integer")
    rgba_sha = _sha256(capture.get("rgba8_sha256"), "gsplat-rs capture rgba8")
    if any(
        presentation.get(field) is not True
        for field in ("successful_present", "queue_terminal_complete", "captured_after_terminal")
    ):
        fail("gsplat-rs capture lacks a successful terminal presentation")
    _require_resolution(
        _object(presentation, "dimensions", "gsplat-rs presentation"),
        "gsplat-rs presentation dimensions",
        presented=True,
    )
    rgba = _decode_png(png_path, "gsplat-rs final-frame.png")
    if hashlib.sha256(rgba).hexdigest() != rgba_sha:
        fail("gsplat-rs PNG bytes do not match the renderer-owned capture receipt")
    return rgba, {
        "artifact_schema": NATIVE_ARTIFACT_SCHEMA,
        "producer": "DiagnosticSurfaceCaptureReceipt",
        "manifest_sha256": file_sha256(manifest_path),
        "rgba8_sha256": rgba_sha,
        "pixel_format": "rgba8unorm",
        "row_origin": "top_left",
        "width": FORMAL_WIDTH,
        "height": FORMAL_HEIGHT,
    }


def _playcanvas_camera(receipt: dict[str, Any], formal: dict[str, Any], context: str) -> None:
    frame = formal["trace"]["frames"][TRACE_FRAME_INDEX]
    if (
        receipt.get("schema") != PLAYCANVAS_CAMERA_SCHEMA
        or receipt.get("trace_frame_index") != TRACE_FRAME_INDEX
        or receipt.get("horizontal_fov") is not False
        or receipt.get("render_target_flip_y") is not False
        or receipt.get("webgpu_depth_range_applied") is not True
        or receipt.get("validation", {}).get("passed") is not True
        or receipt.get("custom_projection", {}).get("active") is not True
        or receipt.get("custom_projection", {}).get("hook")
        != "CameraComponent.calculateProjection"
    ):
        fail(f"{context} does not prove the exact PlayCanvas camera path")
    for field, expected in (
        ("vertical_fov_radians", frame["intrinsics"]["vertical_fov_radians"]),
        ("near_plane", frame["intrinsics"]["near_plane"]),
        ("far_plane", frame["intrinsics"]["far_plane"]),
        ("aspect", FORMAL_WIDTH / FORMAL_HEIGHT),
        ("focal_length_x_over_y", formal["focal_length_x_over_y"]),
    ):
        _finite_close(receipt.get(field), expected, f"{context}.{field}")
    expected_projection = frame["projection_matrix"]
    configured = receipt.get("custom_projection", {}).get(
        "configured_projection_matrix_opengl_column_major"
    )
    if not isinstance(configured, list) or len(configured) != 16:
        fail(f"{context} custom projection matrix is unavailable")
    # Exact-pinhole proof: the x and y scales must correspond to the canonical
    # projection. Remaining matrix elements are already producer-oracle checked
    # and bound below by the camera JSON SHA.
    _finite_close(configured[0], expected_projection[0], f"{context} projection x scale")
    _finite_close(configured[5], expected_projection[5], f"{context} projection y scale")


def _playcanvas_capture(root: pathlib.Path, formal: dict[str, Any]) -> tuple[bytes, dict[str, Any]]:
    manifest_path = _regular_child(root, "manifest.json", "PlayCanvas capture")
    rgba_path = _regular_child(root, "final-frame.rgba8", "PlayCanvas capture")
    png_path = _regular_child(root, "final-frame.png", "PlayCanvas capture")
    manifest = _load_json(manifest_path, "PlayCanvas manifest")
    if manifest.get("schema") != NATIVE_ARTIFACT_SCHEMA:
        fail("PlayCanvas manifest schema mismatch")
    _require_truck(manifest, "PlayCanvas manifest")
    _require_resolution(
        _object(manifest, "resolution", "PlayCanvas manifest"),
        "PlayCanvas resolution",
        presented=True,
    )
    trace = _object(manifest, "trace", "PlayCanvas manifest")
    if (
        trace.get("id") != formal["trace"]["trace_id"]
        or trace.get("sha256") != formal["trace"]["content_sha256"]
        or trace.get("capture_frame_index") != TRACE_FRAME_INDEX
    ):
        fail("PlayCanvas manifest does not bind formal view 000001")
    capture = _object(manifest, "renderer_capture", "PlayCanvas manifest")
    if (
        capture.get("schema") != PLAYCANVAS_CAPTURE_SCHEMA
        or capture.get("producer") != PLAYCANVAS_CAPTURE_PRODUCER
        or capture.get("status") != "terminal"
        or capture.get("copy_map_complete") is not True
        or capture.get("queue_terminal_complete") is not True
        or capture.get("pixel_format") != "rgba8unorm"
        or capture.get("row_origin") != "top_left"
        or capture.get("width") != FORMAL_WIDTH
        or capture.get("height") != FORMAL_HEIGHT
        or capture.get("row_bytes") != FORMAL_WIDTH * 4
        or capture.get("byte_length") != FORMAL_WIDTH * FORMAL_HEIGHT * 4
    ):
        fail("PlayCanvas renderer capture terminal identity mismatch")
    camera_json = capture.get("camera_receipt_json")
    if not isinstance(camera_json, str):
        fail("PlayCanvas renderer capture lacks camera JSON")
    camera_sha = _sha256(capture.get("camera_receipt_sha256"), "PlayCanvas camera receipt")
    if hashlib.sha256(camera_json.encode()).hexdigest() != camera_sha:
        fail("PlayCanvas camera JSON hash mismatch")
    try:
        parsed_camera = json.loads(
            camera_json,
            object_pairs_hook=_unique_object,
            parse_constant=lambda token: fail(f"PlayCanvas camera contains non-finite {token}"),
        )
    except json.JSONDecodeError as error:
        fail(f"PlayCanvas camera JSON is invalid: {error}")
    camera = _object(capture, "camera_receipt", "PlayCanvas capture")
    if parsed_camera != camera:
        fail("PlayCanvas camera JSON does not match its receipt")
    _playcanvas_camera(camera, formal, "PlayCanvas camera")
    source = _object(capture, "source", "PlayCanvas capture")
    if (
        source.get("dataset_id") != TRUCK_ID
        or source.get("dataset_sha256") != TRUCK_SHA256
        or source.get("source_splat_count") != TRUCK_SPLAT_COUNT
        or source.get("decoded_splat_count") != TRUCK_SPLAT_COUNT
        or source.get("resident_splat_count") != TRUCK_SPLAT_COUNT
        or source.get("source_sh_degree") != TRUCK_SH_DEGREE
        or source.get("resident_sh_degree") != TRUCK_SH_DEGREE
        or source.get("source_membership") != "all"
        or source.get("sampling") != "disabled"
        or source.get("lod") != "disabled"
        or source.get("partial_scene_published") is not False
        or source.get("full_quality") is not True
    ):
        fail("PlayCanvas renderer capture source is not complete formal Truck")
    _require_resolution(
        _object(capture, "resolution", "PlayCanvas capture"),
        "PlayCanvas capture resolution",
        presented=False,
    )
    copy_after = _positive_integer(
        capture.get("copy_submit_version_after"), "PlayCanvas copy submit version"
    )
    terminal = _object(capture, "terminal_queue_drain", "PlayCanvas capture")
    if terminal != {
        "phase": "post_capture_presentation",
        "submit_version_before": copy_after,
        "submit_version_after": copy_after,
        "submit_version_stable": True,
    }:
        fail("PlayCanvas capture terminal queue drain mismatch")
    materialization = _object(
        manifest, "renderer_capture_materialization", "PlayCanvas manifest"
    )
    expected_materialization_fields = {
        "schema",
        "source",
        "source_capture_schema",
        "source_capture_producer",
        "source_rgba8_sha256",
        "rgba8_file",
        "rgba8_byte_length",
        "png_file",
        "png_byte_length",
        "png_sha256",
        "width",
        "height",
    }
    if (
        set(materialization) != expected_materialization_fields
        or materialization.get("schema") != PLAYCANVAS_MATERIALIZATION_SCHEMA
        or materialization.get("source")
        != "host_png_from_renderer_owned_webgpu_rgba8"
        or materialization.get("source_capture_schema") != PLAYCANVAS_CAPTURE_SCHEMA
        or materialization.get("source_capture_producer") != PLAYCANVAS_CAPTURE_PRODUCER
        or materialization.get("source_rgba8_sha256") != capture.get("rgba8_sha256")
        or materialization.get("rgba8_file") != "final-frame.rgba8"
        or materialization.get("rgba8_byte_length") != FORMAL_WIDTH * FORMAL_HEIGHT * 4
        or materialization.get("png_file") != "final-frame.png"
        or materialization.get("png_byte_length") != png_path.stat().st_size
        or materialization.get("png_sha256") != file_sha256(png_path)
        or materialization.get("width") != FORMAL_WIDTH
        or materialization.get("height") != FORMAL_HEIGHT
    ):
        fail("PlayCanvas renderer capture materialization mismatch")
    rgba = rgba_path.read_bytes()
    rgba_sha = _sha256(capture.get("rgba8_sha256"), "PlayCanvas renderer RGBA8")
    if len(rgba) != FORMAL_WIDTH * FORMAL_HEIGHT * 4 or hashlib.sha256(rgba).hexdigest() != rgba_sha:
        fail("PlayCanvas materialized RGBA8 bytes do not match the producer receipt")
    if _decode_png(png_path, "PlayCanvas final-frame.png") != rgba:
        fail("PlayCanvas PNG does not derive from its materialized RGBA8 bytes")
    return rgba, {
        "artifact_schema": NATIVE_ARTIFACT_SCHEMA,
        "capture_schema": PLAYCANVAS_CAPTURE_SCHEMA,
        "producer": PLAYCANVAS_CAPTURE_PRODUCER,
        "materialization_schema": PLAYCANVAS_MATERIALIZATION_SCHEMA,
        "manifest_sha256": file_sha256(manifest_path),
        "camera_receipt_sha256": camera_sha,
        "rgba8_sha256": rgba_sha,
        "pixel_format": "rgba8unorm",
        "row_origin": "top_left",
        "width": FORMAL_WIDTH,
        "height": FORMAL_HEIGHT,
    }


def _ground_truth(evaluation_root: pathlib.Path, formal: dict[str, Any]) -> tuple[bytes, dict[str, Any]]:
    entry = next(
        (
            value
            for value in formal["evaluation"].get("entries", [])
            if value.get("source_relative_path") == f"gt/{VIEW_ID}.png"
        ),
        None,
    )
    if not isinstance(entry, dict):
        fail("Evaluation Images authority lacks gt/000001.png")
    path = evaluation_root / entry.get("retained_relative_path", "")
    if path.is_symlink() or not path.is_file():
        fail("Evaluation Images ground truth is unavailable")
    if path.stat().st_size != entry.get("bytes") or file_sha256(path) != entry.get("sha256"):
        fail("Evaluation Images ground-truth byte identity drifted")
    rgba = _decode_png(path, "Evaluation Images gt/000001.png")
    if any(rgba[offset + 3] != 255 for offset in range(0, len(rgba), 4)):
        fail("Evaluation Images ground truth must be opaque")
    rgb = bytes(
        channel
        for offset in range(0, len(rgba), 4)
        for channel in rgba[offset : offset + 3]
    )
    return rgb, {
        "authority_schema": EVALUATION_AUTHORITY.SCHEMA,
        "authority_class": EVALUATION_AUTHORITY.AUTHORITY_CLASS,
        "authority_receipt_sha256": formal["evaluation_receipt_sha256"],
        "path": entry["retained_relative_path"],
        "bytes": entry["bytes"],
        "sha256": entry["sha256"],
        "decoded_rgb8_sha256": hashlib.sha256(rgb).hexdigest(),
    }


def _decision(value: Any) -> dict[str, Any]:
    return {
        "state": value.state.value,
        "reason": value.reason,
        "metrics": asdict(value.metrics),
    }


def _forbidden_output_key(key: str) -> bool:
    lowered = key.lower()
    if lowered in {"performance_eligible", "focal_length_x_over_y"}:
        return False
    return any(
        token in lowered
        for token in (
            "timing",
            "frame_wall",
            "call_ms",
            "fps",
            "ratio",
            "winner",
            "pairing",
            "throughput",
            "speedup",
        )
    ) or lowered == "performance"


def validate_result(result: dict[str, Any]) -> None:
    """Validate the intentionally non-performance publication shape."""

    def walk(value: Any, path: str) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                if _forbidden_output_key(key):
                    fail(f"{path}.{key} is forbidden in a Product Quality result")
                walk(child, f"{path}.{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{path}[{index}]")

    walk(result, "result")
    if (
        result.get("schema") != SCHEMA
        or result.get("view_id") != VIEW_ID
        or result.get("resolution") != {"width": FORMAL_WIDTH, "height": FORMAL_HEIGHT}
    ):
        fail("one-view result identity mismatch")
    qualification = _object(result, "qualification", "result")
    if qualification != {
        "one_view_smoke": result.get("status"),
        "product_quality": "Deferred",
        "reason": "second_formal_view_required",
        "performance_eligible": False,
    }:
        fail("one-view result must remain performance-ineligible and Product Quality Deferred")
    endpoints = _object(result, "endpoints", "result")
    if set(endpoints) != {"gsplat_rs", "playcanvas"}:
        fail("one-view result must contain exactly two endpoints")
    states = []
    for endpoint in ("gsplat_rs", "playcanvas"):
        state = endpoints[endpoint].get("state")
        if state not in {"Accepted", "Rejected"}:
            fail(f"{endpoint} has an invalid finite quality state")
        states.append(state)
    expected = "Accepted" if all(state == "Accepted" for state in states) else "Rejected"
    if result.get("status") != expected:
        fail("one-view aggregate status does not reduce endpoint states")


def evaluate_one_view(
    *,
    formal_trace_authority: pathlib.Path,
    evaluation_authority: pathlib.Path,
    gsplat_capture: pathlib.Path,
    playcanvas_capture: pathlib.Path,
) -> dict[str, Any]:
    formal = _formal_trace(formal_trace_authority, evaluation_authority)
    source_rgb, source_identity = _ground_truth(evaluation_authority, formal)
    native_rgba, native_identity = _native_capture(gsplat_capture, formal)
    playcanvas_rgba, playcanvas_identity = _playcanvas_capture(playcanvas_capture, formal)
    try:
        evaluation = PRODUCT_QUALITY.evaluate_formal_product_quality(
            source_rgb,
            {"gsplat_rs": native_rgba, "playcanvas": playcanvas_rgba},
        )
    except PRODUCT_QUALITY.ProductQualityError as error:
        fail(f"endpoint pixel contract rejected: {error}")
    endpoints = {
        "gsplat_rs": {**_decision(evaluation.gsplat_rs), "capture": native_identity},
        "playcanvas": {
            **_decision(evaluation.playcanvas),
            "capture": playcanvas_identity,
        },
    }
    status = (
        "Accepted"
        if all(value["state"] == "Accepted" for value in endpoints.values())
        else "Rejected"
    )
    result = {
        "schema": SCHEMA,
        "status": status,
        "view_id": VIEW_ID,
        "resolution": {"width": FORMAL_WIDTH, "height": FORMAL_HEIGHT},
        "camera": {
            "trace_id": formal["trace"]["trace_id"],
            "trace_content_sha256": formal["trace"]["content_sha256"],
            "trace_file_sha256": formal["trace_file_sha256"],
            "trace_receipt_sha256": formal["trace_receipt_sha256"],
            "pose_intrinsics_sha256": formal["pose_intrinsics_sha256"],
            "focal_length_x_over_y": formal["focal_length_x_over_y"],
        },
        "source": source_identity,
        "thresholds": {
            "ssim_luma_srgb_window8_minimum": SSIM_MINIMUM,
            "rgb_mae_normalized_maximum": RGB_MAE_NORMALIZED_MAXIMUM,
            "severe_rgb_error_threshold_8bit": SEVERE_RGB_ERROR_THRESHOLD_8BIT,
            "severe_pixel_fraction_maximum": SEVERE_PIXEL_FRACTION_MAXIMUM,
            "opaque_alpha_required": True,
        },
        "endpoints": endpoints,
        "qualification": {
            "one_view_smoke": status,
            "product_quality": "Deferred",
            "reason": "second_formal_view_required",
            "performance_eligible": False,
        },
    }
    validate_result(result)
    return result


def _publish_directory_noreplace(staging: pathlib.Path, output: pathlib.Path) -> None:
    libc = ctypes.CDLL(None, use_errno=True)
    source = os.fsencode(staging)
    destination = os.fsencode(output)
    if sys.platform == "darwin" and hasattr(libc, "renamex_np"):
        function = libc.renamex_np
        function.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
        function.restype = ctypes.c_int
        result = function(source, destination, 0x00000004)
    elif sys.platform.startswith("linux") and hasattr(libc, "renameat2"):
        function = libc.renameat2
        function.argtypes = [
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_uint,
        ]
        function.restype = ctypes.c_int
        result = function(-100, source, -100, destination, 0x00000001)
    else:
        fail("atomic no-replace directory publication is unsupported")
    if result == 0:
        return
    error = ctypes.get_errno()
    if error in {errno.EEXIST, errno.ENOTEMPTY}:
        fail(f"output already exists: {output}")
    raise OSError(error, os.strerror(error), output)


def publish_one_view(result: dict[str, Any], output: pathlib.Path) -> None:
    validate_result(result)
    output = pathlib.Path(os.path.abspath(output))
    if output.exists() or output.is_symlink():
        fail(f"output already exists: {output}")
    if output.parent.is_symlink() or not output.parent.is_dir():
        fail("output parent must be a real existing directory")
    staging = pathlib.Path(
        tempfile.mkdtemp(prefix=f".{output.name}.staging-", dir=output.parent)
    )
    try:
        (staging / "result.json").write_text(_canonical_json(result) + "\n", encoding="utf-8")
        retained = _load_json(staging / "result.json", "staged one-view result")
        validate_result(retained)
        _publish_directory_noreplace(staging, output)
        validate_result(_load_json(output / "result.json", "published one-view result"))
    finally:
        if staging.exists():
            shutil.rmtree(staging)
