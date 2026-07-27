#!/usr/bin/env python3
"""Validate a gsplat-benchmark/v1 artifact directory using only stdlib."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import pathlib
import re
import struct
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
COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1"
BOUND_CONTROL_COUNT_SEMANTICS = "bound_current_stats_control_artifact"
TERMINAL_QUEUE_THROUGHPUT_MODE = "terminal_queue_throughput_window"


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


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        fail(f"cannot hash {path.name}: {error}")
    return digest.hexdigest()


def png_dimensions(path: pathlib.Path) -> tuple[int, int]:
    try:
        with path.open("rb") as handle:
            header = handle.read(24)
    except OSError as error:
        fail(f"cannot read image {path.name}: {error}")
    if (
        len(header) != 24
        or header[:8] != b"\x89PNG\r\n\x1a\n"
        or header[12:16] != b"IHDR"
    ):
        fail(f"image is not a PNG with an IHDR header: {path.name}")
    return struct.unpack(">II", header[16:24])


def resolve_artifact_file(directory: pathlib.Path, value: str, context: str) -> pathlib.Path:
    relative = pathlib.Path(value)
    if relative.is_absolute() or not relative.parts or ".." in relative.parts:
        fail(f"{context} must be a relative path inside the artifact directory")
    root = directory.resolve()
    path = (directory / relative).resolve()
    try:
        path.relative_to(root)
    except ValueError:
        fail(f"{context} escapes the artifact directory")
    if not path.is_file():
        fail(f"{context} does not name an artifact file: {value}")
    return path


def validate_image(directory: pathlib.Path, manifest: dict[str, Any]) -> None:
    value = manifest.get("image")
    if value is None:
        return
    image = require_object(manifest, "image")
    path_text = require_string(image, "path")
    expected_sha256 = require_string(image, "sha256")
    if not SHA256_RE.fullmatch(expected_sha256):
        fail("image.sha256 must be lowercase SHA-256")
    width = require_int(image, "width")
    height = require_int(image, "height")
    if width == 0 or height == 0:
        fail("image dimensions must be positive")
    display = require_object(manifest, "display")
    if (width, height) != (display.get("width"), display.get("height")):
        fail("image dimensions must equal display dimensions")
    path = resolve_artifact_file(directory, path_text, "image.path")
    if png_dimensions(path) != (width, height):
        fail("image PNG dimensions do not match its receipt")
    if sha256_file(path) != expected_sha256:
        fail("image SHA-256 mismatch")


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
    for key in ("exact_plan_requested", "exact_plan_actual"):
        if key in renderer and renderer[key] is not None:
            require_string(renderer, key)
    if (
        renderer.get("exact_plan_requested") is not None
        and renderer.get("exact_plan_actual") is None
    ):
        if "renderer.exact_plan_actual" not in unavailable:
            fail("unavailable renderer.exact_plan_actual must be listed as unavailable")
    count_semantics = renderer.get("count_semantics")
    if count_semantics is not None and count_semantics not in {
        COUNT_SEMANTICS,
        BOUND_CONTROL_COUNT_SEMANTICS,
    }:
        fail(
            "renderer.count_semantics must be an admitted count contract when present"
        )
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


def load_frames(
    path: pathlib.Path,
    run_id: str,
    unavailable: set[str],
    count_semantics: str | None = None,
    source_count: int | None = None,
) -> list[dict[str, Any]]:
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
        count_values: dict[str, int | None] = {}
        for key in ("visible", "drawn"):
            if key not in frame:
                fail(f"frame must contain {key}")
            if frame[key] is None:
                if f"frames[*].{key}" not in unavailable:
                    fail(f"null {key} must be listed as unavailable")
                count_values[key] = None
            else:
                count_values[key] = require_int(frame, key)
                if source_count is not None and count_values[key] > source_count:
                    fail(f"{key} must not exceed dataset.splat_count")
        if (count_values["visible"] is None) != (count_values["drawn"] is None):
            fail("visible and drawn must be unavailable together")
        if "active_splats" in frame:
            active_splats = require_int(frame, "active_splats")
            if source_count is not None and active_splats > source_count:
                fail("active_splats must not exceed dataset.splat_count")
        contributor_present = "contributor" in frame
        compaction_present = "exact_contributor_compaction" in frame
        if contributor_present != compaction_present:
            fail(
                "contributor and exact_contributor_compaction must be emitted together"
            )
        if count_semantics == COUNT_SEMANTICS and count_values["visible"] is None:
            fail("renderer count_semantics forbids unavailable visible/drawn counts")
        if count_semantics == COUNT_SEMANTICS and not contributor_present:
            fail(
                "renderer count_semantics requires contributor and "
                "exact_contributor_compaction on every frame"
            )
        if contributor_present and count_semantics != COUNT_SEMANTICS:
            fail(
                "contributor fields require renderer.count_semantics="
                f"{COUNT_SEMANTICS!r}"
            )
        if contributor_present:
            if count_values["visible"] is None or count_values["drawn"] is None:
                fail("contributor fields require available visible/drawn counts")
            contributor = require_int(frame, "contributor")
            if not isinstance(frame.get("exact_contributor_compaction"), bool):
                fail("exact_contributor_compaction must be boolean")
            if contributor > count_values["visible"]:
                fail("contributor must not exceed visible candidates")
            if frame["exact_contributor_compaction"]:
                if count_values["drawn"] != contributor:
                    fail("exact contributor draw requires drawn == contributor")
            elif count_values["drawn"] != count_values["visible"]:
                fail("non-compacted draw requires drawn == visible")
        elif count_values["visible"] is not None and \
                count_values["drawn"] != count_values["visible"]:
            fail("legacy frame requires drawn == visible; a draw budget is forbidden")
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


def validate_terminal_queue_throughput(
    manifest: dict[str, Any],
    frames: list[dict[str, Any]],
    summary: dict[str, Any],
) -> None:
    window_value = manifest.get("benchmark_window")
    if not isinstance(window_value, dict) or \
            window_value.get("mode") != TERMINAL_QUEUE_THROUGHPUT_MODE:
        return
    window = window_value
    timing = require_object(manifest, "timing")
    ordering = require_object(manifest, "ordering_window")
    renderer = require_object(manifest, "renderer")
    if timing.get("performance_evidence") is not True or \
            window.get("performance_evidence") is not True:
        fail("terminal-queue throughput must declare timing.performance_evidence=true")
    if renderer.get("count_semantics") != BOUND_CONTROL_COUNT_SEMANTICS:
        fail("terminal-queue throughput must bind render counts to its control artifact")
    if window.get("evidence_role") != "cross_implementation_terminal_queue_throughput":
        fail("terminal-queue throughput evidence_role mismatch")
    if window.get("current_stats_policy") != \
            "one_untimed_warmup_boundary_and_one_final_measured_receipt":
        fail("terminal-queue throughput current-stats boundary policy mismatch")
    if window.get("terminal_policy") != \
            "drain_warmup_before_first_measured_input_and_stop_after_final_measured_submit":
        fail("terminal-queue throughput terminal policy mismatch")
    configuration_sha256 = require_string(window, "configuration_sha256")
    if not SHA256_RE.fullmatch(configuration_sha256):
        fail("benchmark_window.configuration_sha256 must be lowercase SHA-256")
    control = require_object(window, "control_artifact_identity")
    require_string(control, "run_id")
    if require_string(control, "configuration_sha256") != configuration_sha256:
        fail("terminal-queue throughput control configuration identity mismatch")

    warmup_count = require_int(summary, "warmup_count")
    measured_count = len(frames)
    if require_int(window, "warmup_submit_count") != warmup_count:
        fail("terminal-queue throughput warmup submit count mismatch")
    if require_int(window, "measured_submit_count") != measured_count:
        fail("terminal-queue throughput measured submit count mismatch")
    if require_int(window, "measured_wait_count_before_final_submit") != 0:
        fail("terminal-queue throughput waited between measured submissions")
    if require_int(window, "terminal_current_stats_submission_count") != 1 or \
            require_int(window, "terminal_current_stats_terminal_count") != 1:
        fail("terminal-queue throughput requires one final measured receipt")
    adaptive = window.get("exact_adaptive_measured")
    if not isinstance(adaptive, list) or len(adaptive) != measured_count:
        fail("terminal-queue throughput lacks its complete Exact adaptive ledger")
    for index, record in enumerate(adaptive):
        if not isinstance(record, dict) or record.get("state") == "disabled" or \
                record.get("plan") not in {
                    "cpu_post_sort", "gpu_post_sort", "gpu_preproject"
                } or record.get("projected_execution") not in {"candidate", "compact"}:
            fail(f"terminal-queue throughput Exact adaptive record {index} is invalid")

    final_ticket = require_int(window, "final_measured_current_stats_ticket")
    final_receipt = require_object(window, "terminal_receipt")
    if final_ticket == 0 or final_receipt.get("phase") != "final_measured" or \
            final_receipt.get("status") != "ready" or \
            require_int(final_receipt, "ticket") != final_ticket:
        fail("terminal-queue throughput final receipt identity mismatch")
    first_input = require_number(window, "first_measured_input_monotonic_ms")
    first_submit = require_number(window, "first_measured_submit_monotonic_ms")
    last_submit = require_number(window, "last_measured_submit_monotonic_ms")
    last_terminal = require_number(window, "last_measured_terminal_monotonic_ms")
    assert first_input is not None and first_submit is not None
    assert last_submit is not None and last_terminal is not None
    if not first_input <= first_submit <= last_submit <= last_terminal:
        fail("terminal-queue throughput monotonic boundary ordering is invalid")
    if require_number(final_receipt, "submitted_at_monotonic_ms") != last_submit or \
            require_number(final_receipt, "terminal_at_monotonic_ms") != last_terminal:
        fail("terminal-queue throughput final receipt timestamps mismatch")
    if require_number(final_receipt, "requested_at_monotonic_ms") > last_submit:
        fail("terminal-queue throughput final receipt was requested after submission")
    close(window.get("input_to_first_submit_ms"), first_submit - first_input,
          "benchmark_window.input_to_first_submit_ms")
    close(window.get("submit_span_ms"), last_submit - first_submit,
          "benchmark_window.submit_span_ms")
    close(window.get("terminal_tail_ms"), last_terminal - last_submit,
          "benchmark_window.terminal_tail_ms")
    close(window.get("terminal_window_ms"), last_terminal - first_input,
          "benchmark_window.terminal_window_ms")

    warmup_ticket = window.get("warmup_boundary_current_stats_ticket")
    warmup_receipt = window.get("warmup_terminal_receipt")
    if warmup_count > 0:
        if require_int(window, "warmup_terminal_receipt_submission_count") != 1 or \
                require_int(window, "warmup_terminal_receipt_terminal_count") != 1 or \
                not isinstance(warmup_receipt, dict):
            fail("terminal-queue throughput requires one untimed warmup boundary receipt")
        warmup_ticket = require_int(window, "warmup_boundary_current_stats_ticket")
        if warmup_ticket == 0 or warmup_ticket == final_ticket or \
                warmup_receipt.get("phase") != "warmup_boundary" or \
                warmup_receipt.get("status") != "ready" or \
                require_int(warmup_receipt, "ticket") != warmup_ticket:
            fail("terminal-queue throughput warmup receipt identity mismatch")
        warmup_submitted = require_number(warmup_receipt, "submitted_at_monotonic_ms")
        warmup_terminal = require_number(warmup_receipt, "terminal_at_monotonic_ms")
        assert warmup_submitted is not None and warmup_terminal is not None
        if require_number(warmup_receipt, "requested_at_monotonic_ms") > warmup_submitted or \
                warmup_submitted > warmup_terminal or warmup_terminal > first_input:
            fail("terminal-queue throughput did not drain warmup before measured input")
        if require_int(window, "draw_count_at_warmup_drain_start") != warmup_count or \
                require_int(window, "draw_count_at_warmup_drain_completion") != warmup_count:
            fail("terminal-queue throughput drew while draining warmup")
    elif require_int(window, "warmup_terminal_receipt_submission_count") != 0 or \
            require_int(window, "warmup_terminal_receipt_terminal_count") != 0 or \
            warmup_ticket is not None or warmup_receipt is not None:
        fail("zero-warmup throughput must not claim a warmup receipt")

    expected_draw_count = warmup_count + measured_count
    if require_int(window, "draw_count_at_final_drain_start") != expected_draw_count or \
            require_int(window, "draw_count_at_completion") != expected_draw_count:
        fail("terminal-queue throughput drew while draining its final receipt")
    overhead = require_object(window, "terminal_receipt_overhead")
    expected_warmup_boundary = "same_submission_result_ready_before_first_measured_input" \
        if warmup_count > 0 else "not_applicable_no_warmup"
    expected_residual = "excluded" if warmup_count > 0 else "not_applicable"
    if overhead.get("kind") != "renderer_current_stats_same_submission_map_v1" or \
            require_int(overhead, "readback_buffer_bytes") != 8 or \
            require_int(overhead, "encoded_copy_bytes") not in {4, 8} or \
            require_int(overhead, "extra_queue_submissions") != 0 or \
            overhead.get("map_async_result_required") is not True or \
            overhead.get("included_in_terminal_window") is not True or \
            overhead.get("warmup_terminal_boundary") != expected_warmup_boundary or \
            overhead.get("residual_warmup_queue_tail") != expected_residual:
        fail("terminal-queue throughput terminal receipt overhead contract mismatch")

    identity_fields = (
        "current_stats_ticket",
        "current_stats_plan",
        "current_stats_scene_generation",
        "current_stats_camera_revision",
        "current_stats_viewport_generation",
        "current_stats_contract_generation",
        "current_stats_plan_set_generation",
        "current_stats_order_generation",
        "current_stats_raster_generation",
        "current_stats_encode_attempt",
        "current_stats_presentation_sequence",
    )
    for index, frame in enumerate(frames[:-1]):
        if frame.get("current_stats_submission") != "not_requested" or \
                any(field not in frame or frame[field] is not None for field in identity_fields):
            fail(f"terminal-queue throughput frame {index} requested current stats")
    final_frame = frames[-1]
    if final_frame.get("current_stats_submission") != "issued" or \
            final_frame.get("current_stats_ticket") != final_ticket or \
            any(final_frame.get(field) is None for field in identity_fields):
        fail("terminal-queue throughput final frame lacks its same-submission receipt")

    expected_receipt_count = 2 if warmup_count > 0 else 1
    ordering_expected = {
        "measured_submit_count": measured_count,
        "terminal_queue_done_count": 1,
        "warmup_queue_done_count": 1 if warmup_count > 0 else 0,
        "terminal_current_stats_receipts": expected_receipt_count,
        "last_measured_ticket": final_ticket,
    }
    for field, expected in ordering_expected.items():
        if ordering.get(field) != expected:
            fail(f"ordering_window.{field} mismatch: expected {expected}")
    if ordering.get("queue_done_proven") is not True:
        fail("terminal-queue throughput queue completion is not proven")
    for field in (
        "first_measured_input_monotonic_ms",
        "first_measured_submit_monotonic_ms",
        "last_measured_submit_monotonic_ms",
        "last_measured_terminal_monotonic_ms",
        "input_to_first_submit_ms",
        "submit_span_ms",
        "terminal_tail_ms",
        "terminal_window_ms",
    ):
        if ordering.get(field) != window.get(field):
            fail(f"ordering_window.{field} does not match benchmark_window")


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
    validate_image(directory, manifest)
    unavailable = set(manifest["unavailable_fields"])
    renderer = require_object(manifest, "renderer")
    frames = load_frames(
        directory / "frames.jsonl",
        run_id,
        unavailable,
        count_semantics=renderer.get("count_semantics"),
        source_count=require_int(require_object(manifest, "dataset"), "splat_count"),
    )
    summary = load_json(directory / "summary.json")
    validate_summary(summary, frames, run_id)
    validate_terminal_queue_throughput(manifest, frames, summary)
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
