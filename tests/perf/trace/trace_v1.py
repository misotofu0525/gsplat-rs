#!/usr/bin/env python3
"""Shared stdlib helpers for gsplat-camera-trace/v1 tools."""

from __future__ import annotations

import hashlib
import json
import math
from typing import Any


SCHEMA = "gsplat-camera-trace/v1"
MATRIX_TOLERANCE = 1e-12


def mat4_multiply(a: list[float], b: list[float]) -> list[float]:
    return [
        sum(a[row * 4 + k] * b[k * 4 + column] for k in range(4))
        for row in range(4)
        for column in range(4)
    ]


def normalize_quaternion(q: list[float]) -> list[float]:
    length = math.sqrt(sum(value * value for value in q))
    if not math.isfinite(length) or length <= 0.0:
        raise ValueError("quaternion must have positive finite length")
    return [value / length for value in q]


def view_matrix(position: list[float], rotation_xyzw: list[float]) -> list[float]:
    x, y, z, w = normalize_quaternion(rotation_xyzw)
    # Inverse camera-to-world quaternion converted directly to row-major R.
    x, y, z = -x, -y, -z
    rotation = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - w * z),
        2.0 * (x * z + w * y),
        2.0 * (x * y + w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - w * x),
        2.0 * (x * z - w * y),
        2.0 * (y * z + w * x),
        1.0 - 2.0 * (x * x + y * y),
    ]
    tx = -(rotation[0] * position[0] + rotation[1] * position[1] + rotation[2] * position[2])
    ty = -(rotation[3] * position[0] + rotation[4] * position[1] + rotation[5] * position[2])
    tz = -(rotation[6] * position[0] + rotation[7] * position[1] + rotation[8] * position[2])
    return [
        rotation[0], rotation[1], rotation[2], tx,
        rotation[3], rotation[4], rotation[5], ty,
        rotation[6], rotation[7], rotation[8], tz,
        0.0, 0.0, 0.0, 1.0,
    ]


def projection_matrix(vertical_fov_radians: float, near: float, far: float, aspect: float) -> list[float]:
    f = 1.0 / math.tan(vertical_fov_radians * 0.5)
    depth = far / (far - near)
    return [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, f, 0.0, 0.0,
        0.0, 0.0, depth, -near * depth,
        0.0, 0.0, 1.0, 0.0,
    ]


def canonical_content_bytes(trace: dict[str, Any]) -> bytes:
    content = {key: value for key, value in trace.items() if key != "content_sha256"}
    return json.dumps(content, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def content_sha256(trace: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_content_bytes(trace)).hexdigest()


def with_content_hash(trace: dict[str, Any]) -> dict[str, Any]:
    result = dict(trace)
    result["content_sha256"] = content_sha256(result)
    return result
