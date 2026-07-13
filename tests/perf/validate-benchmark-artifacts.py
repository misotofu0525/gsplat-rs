#!/usr/bin/env python3
"""Validate a gsplat-benchmark/v1 artifact directory using only stdlib."""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import re
import sys
from typing import Any


SCHEMA = "gsplat-benchmark/v1"
METRICS = (
    "call_ms",
    "frame_wall_ms",
    "preprocess_ms",
    "sort_ms",
    "geometry_submit_ms",
    "gpu_wait_ms",
    "gpu_complete_ms",
)
REQUIRED_TIMINGS = {"call_ms", "frame_wall_ms"}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
TOLERANCE = 1e-9


class ValidationError(ValueError):
    pass


def fail(message: str) -> None:
    raise ValidationError(message)


def reject_constant(value: str) -> None:
    fail(f"non-finite JSON number is forbidden: {value}")


def load_json(path: pathlib.Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle, parse_constant=reject_constant)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path.name}: {error}")
    if not isinstance(value, dict):
        fail(f"{path.name} must contain a JSON object")
    return value


def require_object(parent: dict[str, Any], key: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        fail(f"{key} must be an object")
    return value


def require_string(parent: dict[str, Any], key: str) -> str:
    value = parent.get(key)
    if not isinstance(value, str) or not value:
        fail(f"{key} must be a non-empty string")
    return value


def require_int(parent: dict[str, Any], key: str) -> int:
    value = parent.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        fail(f"{key} must be a non-negative integer")
    return value


def require_number(parent: dict[str, Any], key: str, nullable: bool = False) -> float | None:
    value = parent.get(key)
    if value is None and nullable:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        fail(f"{key} must be a non-negative finite number" + (" or null" if nullable else ""))
    result = float(value)
    if not math.isfinite(result) or result < 0:
        fail(f"{key} must be a non-negative finite number" + (" or null" if nullable else ""))
    return result


def validate_header(value: dict[str, Any], record_type: str, run_id: str | None = None) -> str:
    if value.get("schema") != SCHEMA:
        fail(f"schema must equal {SCHEMA}")
    if value.get("record_type") != record_type:
        fail(f"record_type must equal {record_type}")
    actual_run_id = require_string(value, "run_id")
    if run_id is not None and actual_run_id != run_id:
        fail(f"run_id mismatch: expected {run_id}, got {actual_run_id}")
    return actual_run_id


def validate_manifest(manifest: dict[str, Any]) -> str:
    run_id = validate_header(manifest, "manifest")
    for key in ("identity", "build", "dataset", "trace", "renderer", "display", "environment"):
        require_object(manifest, key)
    unavailable = manifest.get("unavailable_fields")
    if not isinstance(unavailable, list) or any(not isinstance(item, str) or not item for item in unavailable):
        fail("unavailable_fields must be an array of non-empty strings")

    identity = manifest["identity"]
    for key in (
        "series_id",
        "started_at_utc",
        "ended_at_utc",
        "measurement_started_at_utc",
        "measurement_ended_at_utc",
    ):
        require_string(identity, key)
    build = manifest["build"]
    repository_commit = build.get("repository_commit")
    if repository_commit is None:
        if "build.repository_commit" not in unavailable:
            fail("null build.repository_commit must be listed as unavailable")
    elif not isinstance(repository_commit, str) or not repository_commit:
        fail("build.repository_commit must be a non-empty string or null")
    dirty = build.get("dirty")
    if dirty is None:
        if "build.dirty" not in unavailable:
            fail("null build.dirty must be listed as unavailable")
    elif not isinstance(dirty, bool):
        fail("build.dirty must be boolean or null")
    for key in ("profile", "package_version"):
        require_string(build, key)
    dataset = manifest["dataset"]
    require_string(dataset, "id")
    if not SHA256_RE.fullmatch(require_string(dataset, "sha256")):
        fail("dataset.sha256 must be lowercase SHA-256")
    for key in ("bytes", "splat_count", "sh_degree"):
        require_int(dataset, key)
    trace = manifest["trace"]
    require_string(trace, "id")
    if not SHA256_RE.fullmatch(require_string(trace, "sha256")):
        fail("trace.sha256 must be lowercase SHA-256")
    renderer = manifest["renderer"]
    for key in ("implementation", "path", "backend", "sort_policy"):
        require_string(renderer, key)
    display = manifest["display"]
    for key in ("width", "height"):
        if require_int(display, key) == 0:
            fail(f"display.{key} must be positive")
    for key in ("dpr", "refresh_hz", "frame_budget_ms"):
        if require_number(display, key) == 0:
            fail(f"display.{key} must be positive")
    for key in ("refresh_hz_source", "frame_budget_source"):
        require_string(display, key)
    environment = manifest["environment"]
    for key in ("platform", "os"):
        require_string(environment, key)
    for key in ("device", "browser", "adapter", "driver"):
        if key not in environment or (environment[key] is not None and not isinstance(environment[key], str)):
            fail(f"environment.{key} must be a string or null")
    return run_id


def load_frames(path: pathlib.Path, run_id: str, unavailable: set[str]) -> list[dict[str, Any]]:
    frames: list[dict[str, Any]] = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        fail(f"cannot read frames.jsonl: {error}")
    for line_number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        try:
            frame = json.loads(line, parse_constant=reject_constant)
        except json.JSONDecodeError as error:
            fail(f"frames.jsonl line {line_number}: {error}")
        if not isinstance(frame, dict):
            fail(f"frames.jsonl line {line_number} must be an object")
        validate_header(frame, "frame", run_id)
        expected_index = len(frames)
        if require_int(frame, "frame_index") != expected_index:
            fail(f"frame_index must be contiguous from zero; expected {expected_index}")
        require_int(frame, "elapsed_ns")
        for metric in METRICS:
            nullable = metric not in REQUIRED_TIMINGS
            value = require_number(frame, metric, nullable=nullable)
            if value is None and f"frames[*].{metric}" not in unavailable:
                fail(f"null {metric} must be listed as unavailable")
        for key in ("visible", "drawn"):
            require_int(frame, key)
        if frame.get("sort_refreshed") is not None and not isinstance(frame.get("sort_refreshed"), bool):
            fail("sort_refreshed must be boolean or null")
        if frames and frame["elapsed_ns"] < frames[-1]["elapsed_ns"]:
            fail("elapsed_ns must be monotonic")
        frames.append(frame)
    if not frames:
        fail("frames.jsonl must contain at least one frame")
    return frames


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(math.ceil(fraction * len(ordered)) - 1, 0)
    return ordered[index]


def expected_distribution(frames: list[dict[str, Any]], metric: str) -> dict[str, float | int] | None:
    values = [float(frame[metric]) for frame in frames if frame[metric] is not None]
    if not values:
        return None
    total = 0.0
    for value in values:
        total += value
    return {
        "count": len(values),
        "mean": total / len(values),
        "p50": percentile(values, 0.50),
        "p90": percentile(values, 0.90),
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "max": max(values),
    }


def close(actual: Any, expected: float, field: str) -> None:
    if isinstance(actual, bool) or not isinstance(actual, (int, float)):
        fail(f"{field} must be numeric")
    if not math.isfinite(float(actual)) or abs(float(actual) - expected) > TOLERANCE:
        fail(f"{field} mismatch: expected {expected}, got {actual}")


def validate_summary(summary: dict[str, Any], frames: list[dict[str, Any]], run_id: str) -> None:
    validate_header(summary, "summary", run_id)
    if require_int(summary, "sample_count") != len(frames):
        fail("summary sample_count does not match frames.jsonl")
    require_int(summary, "warmup_count")
    budget = require_number(summary, "frame_budget_ms")
    assert budget is not None
    if budget == 0:
        fail("frame_budget_ms must be positive")
    missed = sum(1 for frame in frames if frame["frame_wall_ms"] > budget)
    if require_int(summary, "missed_frame_count") != missed:
        fail(f"missed_frame_count mismatch: expected {missed}")
    distributions = require_object(summary, "distributions")
    for metric in METRICS:
        expected = expected_distribution(frames, metric)
        actual = distributions.get(metric)
        if expected is None:
            if actual is not None:
                fail(f"distributions.{metric} must be null when unavailable")
            continue
        if not isinstance(actual, dict):
            fail(f"distributions.{metric} must be an object")
        if require_int(actual, "count") != expected["count"]:
            fail(f"distributions.{metric}.count mismatch")
        for field in ("mean", "p50", "p90", "p95", "p99", "max"):
            close(actual.get(field), float(expected[field]), f"distributions.{metric}.{field}")


def validate_async_sort_telemetry(frames: list[dict[str, Any]], summary: dict[str, Any]) -> None:
    boolean_fields = (
        "async_sort_scheduled",
        "async_sort_result_applied",
        "stale_async_sort_dropped",
        "sync_sort_fallback",
    )
    scheduled = completed = applied = dropped = fallbacks = stale_applied = 0
    max_presented_lag = 0
    for index, frame in enumerate(frames):
        camera_revision = require_int(frame, "camera_revision")
        applied_revision = require_int(frame, "applied_order_revision")
        presented_lag = require_int(frame, "presented_order_revision_lag")
        if camera_revision < applied_revision:
            fail(f"frame {index}: applied order revision exceeds camera revision")
        if camera_revision - applied_revision != presented_lag:
            fail(f"frame {index}: presented order lag does not match revisions")
        if presented_lag > 2:
            fail(f"frame {index}: presented async order lag exceeds 2")
        max_presented_lag = max(max_presented_lag, presented_lag)
        if not isinstance(frame.get("sort_refreshed"), bool):
            fail(f"frame {index}: async sort_refreshed must be boolean")
        for field in boolean_fields:
            if not isinstance(frame.get(field), bool):
                fail(f"frame {index}: {field} must be boolean")
        scheduled_revision = frame.get("async_sort_scheduled_revision")
        completed_revision = frame.get("async_sort_completed_revision")
        observed_lag = frame.get("async_sort_observed_result_lag")
        if scheduled_revision is not None and require_int(frame, "async_sort_scheduled_revision") > camera_revision:
            fail(f"frame {index}: scheduled revision exceeds camera revision")
        if completed_revision is not None and require_int(frame, "async_sort_completed_revision") > camera_revision:
            fail(f"frame {index}: completed revision exceeds camera revision")
        if observed_lag is not None and require_int(frame, "async_sort_observed_result_lag") < 0:
            fail(f"frame {index}: observed result lag must be non-negative")
        scheduled += int(frame["async_sort_scheduled"])
        completed += int(completed_revision is not None)
        applied += int(frame["async_sort_result_applied"])
        dropped += int(frame["stale_async_sort_dropped"])
        fallbacks += int(frame["sync_sort_fallback"])
        stale_applied += int(frame["async_sort_result_applied"] and frame["stale_async_sort_dropped"])

    telemetry = require_object(summary, "sort_telemetry")
    expected = {
        "scheduled_count": scheduled,
        "completed_count": completed,
        "applied_count": applied,
        "dropped_count": dropped,
        "sync_fallback_count": fallbacks,
        "max_presented_revision_lag": max_presented_lag,
        "stale_applied_count": stale_applied,
    }
    for field, value in expected.items():
        if require_int(telemetry, field) != value:
            fail(f"sort_telemetry.{field} mismatch: expected {value}")
    if stale_applied != 0:
        fail("async sort applied a result marked stale")


def validate(directory: pathlib.Path) -> None:
    if not directory.is_dir():
        fail(f"artifact directory does not exist: {directory}")
    manifest = load_json(directory / "manifest.json")
    run_id = validate_manifest(manifest)
    unavailable = set(manifest["unavailable_fields"])
    frames = load_frames(directory / "frames.jsonl", run_id, unavailable)
    summary = load_json(directory / "summary.json")
    validate_summary(summary, frames, run_id)
    renderer = require_object(manifest, "renderer")
    sort_policy = require_string(renderer, "sort_policy")
    if sort_policy.startswith("async_latest:"):
        validate_async_sort_telemetry(frames, summary)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact_directory", type=pathlib.Path)
    args = parser.parse_args()
    try:
        validate(args.artifact_directory)
    except ValidationError as error:
        print(f"benchmark artifact validation failed: {error}", file=sys.stderr)
        return 1
    print(f"benchmark artifact valid: {args.artifact_directory}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
