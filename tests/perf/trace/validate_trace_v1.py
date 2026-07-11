#!/usr/bin/env python3
"""Validate gsplat-camera-trace/v1 JSON using only Python stdlib."""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import sys
from typing import Any

from trace_v1 import MATRIX_TOLERANCE, SCHEMA, content_sha256, mat4_multiply, projection_matrix, view_matrix


class ValidationError(ValueError):
    pass


def fail(message: str) -> None:
    raise ValidationError(message)


def reject_constant(value: str) -> None:
    fail(f"non-finite JSON number is forbidden: {value}")


def number(value: Any, field: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        fail(f"{field} must be a finite number")
    result = float(value)
    if not math.isfinite(result):
        fail(f"{field} must be a finite number")
    return result


def vector(value: Any, length: int, field: str) -> list[float]:
    if not isinstance(value, list) or len(value) != length:
        fail(f"{field} must contain {length} numbers")
    return [number(item, f"{field}[{index}]") for index, item in enumerate(value)]


def close_vector(actual: list[float], expected: list[float], field: str) -> None:
    for index, (left, right) in enumerate(zip(actual, expected, strict=True)):
        if abs(left - right) > MATRIX_TOLERANCE:
            fail(f"{field}[{index}] mismatch: expected {right}, got {left}")


def require_object(parent: dict[str, Any], key: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        fail(f"{key} must be an object")
    return value


def validate(trace: dict[str, Any]) -> None:
    if trace.get("schema") != SCHEMA:
        fail(f"schema must equal {SCHEMA}")
    if not isinstance(trace.get("trace_id"), str) or not trace["trace_id"]:
        fail("trace_id must be a non-empty string")
    actual_hash = trace.get("content_sha256")
    expected_hash = content_sha256(trace)
    if actual_hash != expected_hash:
        fail(f"content_sha256 mismatch: expected {expected_hash}")

    coordinate = require_object(trace, "coordinate_system")
    expected_coordinate = {"handedness": "right", "axes": "RUF", "camera_forward": "+Z"}
    if coordinate != expected_coordinate:
        fail("coordinate_system does not match v1")
    convention = require_object(trace, "matrix_convention")
    expected_convention = {
        "storage_order": "row-major",
        "vector_convention": "column",
        "composition": "projection * view * world_position",
        "ndc_xy": "[-1,1]",
        "ndc_z": "[0,1]",
        "clip_w": "camera_z",
    }
    if convention != expected_convention:
        fail("matrix_convention does not match v1")

    display = require_object(trace, "display")
    width = display.get("width")
    height = display.get("height")
    if isinstance(width, bool) or not isinstance(width, int) or width <= 0:
        fail("display.width must be a positive integer")
    if isinstance(height, bool) or not isinstance(height, int) or height <= 0:
        fail("display.height must be a positive integer")

    frames = trace.get("frames")
    if not isinstance(frames, list) or not frames:
        fail("frames must be a non-empty array")
    previous_timestamp = -1
    for expected_index, frame in enumerate(frames):
        if not isinstance(frame, dict):
            fail(f"frames[{expected_index}] must be an object")
        if frame.get("frame_index") != expected_index:
            fail(f"frames[{expected_index}].frame_index must equal {expected_index}")
        timestamp = frame.get("timestamp_ns")
        if isinstance(timestamp, bool) or not isinstance(timestamp, int) or timestamp < 0:
            fail(f"frames[{expected_index}].timestamp_ns must be a non-negative integer")
        if timestamp <= previous_timestamp and expected_index > 0:
            fail("frame timestamps must be strictly increasing")
        previous_timestamp = timestamp

        pose = require_object(frame, "pose")
        position = vector(pose.get("position"), 3, f"frames[{expected_index}].pose.position")
        rotation = vector(pose.get("rotation_xyzw"), 4, f"frames[{expected_index}].pose.rotation_xyzw")
        norm2 = sum(value * value for value in rotation)
        if abs(norm2 - 1.0) > MATRIX_TOLERANCE:
            fail(f"frames[{expected_index}].pose.rotation_xyzw must be normalized")

        intrinsics = require_object(frame, "intrinsics")
        fov = number(intrinsics.get("vertical_fov_radians"), "vertical_fov_radians")
        near = number(intrinsics.get("near_plane"), "near_plane")
        far = number(intrinsics.get("far_plane"), "far_plane")
        if not 0.0 < fov < math.pi or near <= 0.0 or far <= near:
            fail(f"frames[{expected_index}].intrinsics are invalid")

        actual_view = vector(frame.get("view_matrix"), 16, f"frames[{expected_index}].view_matrix")
        actual_projection = vector(frame.get("projection_matrix"), 16, f"frames[{expected_index}].projection_matrix")
        actual_view_projection = vector(
            frame.get("view_projection_matrix"), 16, f"frames[{expected_index}].view_projection_matrix"
        )
        expected_view = view_matrix(position, rotation)
        expected_projection = projection_matrix(fov, near, far, width / height)
        close_vector(actual_view, expected_view, f"frames[{expected_index}].view_matrix")
        close_vector(actual_projection, expected_projection, f"frames[{expected_index}].projection_matrix")
        close_vector(
            actual_view_projection,
            mat4_multiply(actual_projection, actual_view),
            f"frames[{expected_index}].view_projection_matrix",
        )


def load(path: pathlib.Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"), parse_constant=reject_constant)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read trace: {error}")
    if not isinstance(value, dict):
        fail("trace root must be an object")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=pathlib.Path)
    args = parser.parse_args()
    try:
        validate(load(args.trace))
    except ValidationError as error:
        print(f"camera trace validation failed: {error}", file=sys.stderr)
        return 1
    print(f"camera trace valid: {args.trace}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
