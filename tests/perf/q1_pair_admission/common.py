"""Safe stdlib-only loading primitives shared by Q1 admission modules."""

from __future__ import annotations

import hashlib
import json
import math
import pathlib
from datetime import datetime
from typing import Any


class ValidationError(ValueError):
    pass


def fail(message: str) -> None:
    raise ValidationError(message)


def _reject_constant(value: str) -> None:
    fail(f"non-finite JSON number is forbidden: {value}")


def load_json(path: pathlib.Path, context: str) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle, parse_constant=_reject_constant)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {context}: {error}")
    if not isinstance(value, dict):
        fail(f"{context} must contain a JSON object")
    return value


def load_jsonl(path: pathlib.Path, context: str) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    try:
        with path.open(encoding="utf-8") as handle:
            for line_number, line in enumerate(handle, 1):
                if not line.strip():
                    continue
                value = json.loads(line, parse_constant=_reject_constant)
                if not isinstance(value, dict):
                    fail(f"{context}:{line_number} must contain a JSON object")
                records.append(value)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {context}: {error}")
    return records


def obj(parent: dict[str, Any], key: str, context: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        fail(f"{context}.{key} must be an object")
    return value


def array(parent: dict[str, Any], key: str, context: str) -> list[Any]:
    value = parent.get(key)
    if not isinstance(value, list):
        fail(f"{context}.{key} must be an array")
    return value


def string(parent: dict[str, Any], key: str, context: str) -> str:
    value = parent.get(key)
    if not isinstance(value, str) or not value:
        fail(f"{context}.{key} must be a non-empty string")
    return value


def integer(parent: dict[str, Any], key: str, context: str) -> int:
    value = parent.get(key)
    if not isinstance(value, int) or isinstance(value, bool):
        fail(f"{context}.{key} must be an integer")
    return value


def number(parent: dict[str, Any], key: str, context: str) -> float:
    value = parent.get(key)
    if not isinstance(value, (int, float)) or isinstance(value, bool) or not math.isfinite(value):
        fail(f"{context}.{key} must be a finite number")
    return float(value)


def sha256(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        fail(f"{context} must be a lowercase SHA-256")
    return value


def canonical_sha256(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def file_sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        fail(f"cannot hash {path}: {error}")
    return digest.hexdigest()


def inside(root: pathlib.Path, value: Any, context: str, *, directory: bool = False) -> pathlib.Path:
    if not isinstance(value, str) or not value:
        fail(f"{context} must be a non-empty relative path")
    relative = pathlib.Path(value)
    if relative.is_absolute() or ".." in relative.parts:
        fail(f"{context} must stay inside the series root")
    resolved_root = root.resolve()
    resolved = (root / relative).resolve()
    try:
        resolved.relative_to(resolved_root)
    except ValueError:
        fail(f"{context} escapes the series root")
    if directory and not resolved.is_dir():
        fail(f"{context} does not name an artifact directory")
    if not directory and not resolved.is_file():
        fail(f"{context} does not name a file")
    return resolved


def utc(value: Any, context: str) -> datetime:
    if not isinstance(value, str) or not value:
        fail(f"{context} must be a timestamp")
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        fail(f"{context} is not an ISO timestamp: {error}")
    if result.tzinfo is None:
        fail(f"{context} must include a timezone")
    return result
