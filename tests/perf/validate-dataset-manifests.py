#!/usr/bin/env python3
"""Validate committed gsplat-dataset/v1 manifests and optional local assets."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys
from typing import Any


SCHEMA = "gsplat-dataset/v1"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST_DIR = pathlib.Path(__file__).resolve().parent / "datasets"


class ValidationError(ValueError):
    pass


def require_string(value: dict[str, Any], key: str) -> str:
    item = value.get(key)
    if not isinstance(item, str) or not item:
        raise ValidationError(f"{key} must be a non-empty string")
    return item


def require_non_negative_int(value: dict[str, Any], key: str) -> int:
    item = value.get(key)
    if isinstance(item, bool) or not isinstance(item, int) or item < 0:
        raise ValidationError(f"{key} must be a non-negative integer")
    return item


def validate_bounds(value: dict[str, Any]) -> None:
    minimum = value.get("bounds_min")
    maximum = value.get("bounds_max")
    if not isinstance(minimum, list) or not isinstance(maximum, list):
        raise ValidationError("bounds_min and bounds_max must be arrays")
    if len(minimum) != 3 or len(maximum) != 3:
        raise ValidationError("bounds arrays must contain three values")
    for axis, (lower, upper) in enumerate(zip(minimum, maximum, strict=True)):
        if isinstance(lower, bool) or not isinstance(lower, (int, float)):
            raise ValidationError(f"bounds_min[{axis}] must be numeric")
        if isinstance(upper, bool) or not isinstance(upper, (int, float)):
            raise ValidationError(f"bounds_max[{axis}] must be numeric")
        if lower > upper:
            raise ValidationError(f"bounds axis {axis} is inverted")


def parse_ply_header(path: pathlib.Path) -> tuple[int, int]:
    header = bytearray()
    with path.open("rb") as handle:
        while b"end_header\n" not in header:
            chunk = handle.read(4096)
            if not chunk or len(header) + len(chunk) > 65536:
                raise ValidationError(f"{path}: missing bounded PLY header")
            header.extend(chunk)
    text = bytes(header).split(b"end_header\n", 1)[0].decode("ascii", errors="strict")
    vertex_count: int | None = None
    rest_count = 0
    for line in text.splitlines():
        if line.startswith("element vertex "):
            vertex_count = int(line.split()[2])
        elif line.startswith("property ") and line.split()[-1].startswith("f_rest_"):
            rest_count += 1
    if vertex_count is None:
        raise ValidationError(f"{path}: missing vertex count")
    degree_by_rest_count = {0: 0, 9: 1, 24: 2, 45: 3}
    if rest_count not in degree_by_rest_count:
        raise ValidationError(f"{path}: unsupported SH rest property count {rest_count}")
    return vertex_count, degree_by_rest_count[rest_count]


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def validate_manifest(path: pathlib.Path, verify_file: bool) -> None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValidationError(f"{path.name}: cannot load JSON: {error}") from error
    if not isinstance(value, dict) or value.get("schema") != SCHEMA:
        raise ValidationError(f"{path.name}: schema must equal {SCHEMA}")
    require_string(value, "id")
    status = require_string(value, "qualification_status")
    if status not in {"qualified", "local_candidate"}:
        raise ValidationError(f"{path.name}: invalid qualification_status")
    local_path = require_string(value, "local_path")
    digest = require_string(value, "sha256")
    if not SHA256_RE.fullmatch(digest):
        raise ValidationError(f"{path.name}: invalid lowercase SHA-256")
    require_non_negative_int(value, "bytes")
    require_non_negative_int(value, "splat_count")
    degree = require_non_negative_int(value, "sh_degree")
    if degree > 3:
        raise ValidationError(f"{path.name}: sh_degree must be in 0..3")
    if status == "qualified" and value.get("license") in (None, ""):
        raise ValidationError(f"{path.name}: qualified dataset requires a license")
    validate_bounds(value)

    asset = ROOT / local_path
    if not verify_file or not asset.exists():
        return
    actual_bytes = asset.stat().st_size
    if actual_bytes != value["bytes"]:
        raise ValidationError(
            f"{path.name}: byte mismatch expected={value['bytes']} actual={actual_bytes}"
        )
    actual_digest = sha256(asset)
    if actual_digest != digest:
        raise ValidationError(f"{path.name}: SHA-256 mismatch")
    actual_count, actual_degree = parse_ply_header(asset)
    if actual_count != value["splat_count"] or actual_degree != degree:
        raise ValidationError(
            f"{path.name}: PLY header mismatch count={actual_count} degree={actual_degree}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--verify-available",
        action="store_true",
        help="hash and inspect each manifest asset that exists locally",
    )
    args = parser.parse_args()
    manifests = sorted(MANIFEST_DIR.glob("*.json"))
    if not manifests:
        print("dataset manifest validation failed: no manifests", file=sys.stderr)
        return 1
    try:
        for manifest in manifests:
            validate_manifest(manifest, args.verify_available)
    except ValidationError as error:
        print(f"dataset manifest validation failed: {error}", file=sys.stderr)
        return 1
    print(f"dataset manifests valid: {len(manifests)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
