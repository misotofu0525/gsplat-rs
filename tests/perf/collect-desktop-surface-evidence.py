#!/usr/bin/env python3
"""Collect canonical fail-closed M2b real-window Surface artifacts."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
import platform
import re
import shlex
import subprocess
import sys
import uuid
from pathlib import Path
from typing import Any, Iterable, Sequence


SCHEMA = "gsplat-benchmark/v1"
SUITE_SCHEMA = "gsplat-surface-evidence/v1"
COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1"
FORMAL_SIZE = (1920, 1080)
RUN_TIMEOUT_SECONDS = 30 * 60
ARMS = ("cpu_post_sort", "gpu_post_sort", "gpu_preproject", "adaptive")
CLI_PLAN = {
    "cpu_post_sort": "cpu-post-sort",
    "gpu_post_sort": "gpu-post-sort",
    "gpu_preproject": "gpu-preproject",
    "adaptive": "adaptive",
}
FORCED_ACTUAL = {
    "cpu_post_sort": "cpu_post_sort",
    "gpu_post_sort": "gpu_post_sort",
    "gpu_preproject": "gpu_preproject",
}
PLAN_SEMANTICS = {
    "cpu_post_sort": ("direct_draw_equals_visible", False, "cpu"),
    "gpu_post_sort": ("indirect_draw_equals_visible", False, "gpu"),
    "gpu_preproject": ("indirect_draw_equals_contributor", True, "gpu"),
}
PREFIXES = {
    "begin": "SURFACE_EXACT_EVIDENCE_BEGIN ",
    "frame": "SURFACE_EXACT_EVIDENCE_FRAME ",
    "capture": "SURFACE_EXACT_EVIDENCE_CAPTURE ",
    "summary": "SURFACE_EXACT_EVIDENCE_SUMMARY ",
}
HEX_64 = re.compile(r"^[0-9a-f]{64}$")


class ValidationError(ValueError):
    """The run cannot be published as canonical evidence."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def parse_bool(value: str, context: str) -> bool:
    require(value in ("true", "false"), f"{context} must be true or false")
    return value == "true"


def parse_uint(value: str, context: str, *, positive: bool = False) -> int:
    require(re.fullmatch(r"[0-9]+", value) is not None, f"{context} must be an integer")
    result = int(value)
    if positive:
        require(result > 0, f"{context} must be positive")
    return result


def parse_float(value: str, context: str) -> float:
    try:
        result = float(value)
    except ValueError as error:
        raise ValidationError(f"{context} must be a number") from error
    require(math.isfinite(result) and result >= 0.0, f"{context} must be finite and non-negative")
    return result


def parse_payload(payload: str, context: str) -> dict[str, str]:
    try:
        words = shlex.split(payload, posix=True)
    except ValueError as error:
        raise ValidationError(f"{context} has invalid quoting") from error
    record: dict[str, str] = {}
    for word in words:
        require("=" in word, f"{context} contains a non key=value token")
        key, value = word.split("=", 1)
        require(key and value and key not in record, f"{context} contains an invalid field")
        record[key] = value
    require(bool(record), f"{context} has no fields")
    return record


def parse_log(stdout: str, stderr: str = "") -> dict[str, list[dict[str, str]]]:
    records = {name: [] for name in PREFIXES}
    for stream_name, stream in (("stdout", stdout), ("stderr", stderr)):
        for line_number, line in enumerate(stream.splitlines(), 1):
            for name, prefix in PREFIXES.items():
                if line.startswith(prefix):
                    records[name].append(
                        parse_payload(line[len(prefix) :], f"{stream_name}:{line_number}")
                    )
                    break
    return records


def only(records: Sequence[dict[str, str]], context: str) -> dict[str, str]:
    require(len(records) == 1, f"expected exactly one {context}, got {len(records)}")
    return records[0]


def require_keys(record: dict[str, str], keys: Iterable[str], context: str) -> None:
    missing = sorted(set(keys) - record.keys())
    require(not missing, f"{context} is missing fields: {', '.join(missing)}")


def assert_fields(record: dict[str, str], expected: dict[str, str], context: str) -> None:
    require_keys(record, expected, context)
    for key, value in expected.items():
        require(record[key] == value, f"{context}.{key}: expected {value!r}, got {record[key]!r}")


def validate_count_record(record: dict[str, str], source_count: int, context: str) -> dict[str, Any]:
    actual_plan = record.get("exact_plan_actual", "")
    require(actual_plan in PLAN_SEMANTICS, f"{context} has invalid actual plan")
    semantics, compacted, backend = PLAN_SEMANTICS[actual_plan]
    assert_fields(
        record,
        {
            "count_semantics": semantics,
            "source_count": str(source_count),
            "exact_contributor_compaction": str(compacted).lower(),
            "actual_backend": backend,
            "frame_presented": "true",
            "terminal_receipt": "ready",
        },
        context,
    )
    visible = parse_uint(record.get("visible_count", ""), f"{context}.visible_count")
    contributor = parse_uint(record.get("contributor_count", ""), f"{context}.contributor_count")
    drawn = parse_uint(record.get("drawn_count", ""), f"{context}.drawn_count")
    require(contributor <= visible <= source_count, f"{context} violates C<=V<=S")
    require(drawn == (contributor if compacted else visible), f"{context} violates D=C or D=V")
    ticket = parse_uint(record.get("current_stats_ticket", ""), f"{context}.ticket", positive=True)
    presentation = parse_uint(
        record.get("presentation_sequence", ""), f"{context}.presentation_sequence", positive=True
    )
    camera = parse_uint(record.get("camera_revision", ""), f"{context}.camera_revision")
    return {
        "actual_plan": actual_plan,
        "visible": visible,
        "contributor": contributor,
        "drawn": drawn,
        "compacted": compacted,
        "ticket": ticket,
        "presentation_sequence": presentation,
        "camera_revision": camera,
    }


def validate_run_log(
    stdout: str,
    stderr: str,
    *,
    arm: str,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    warmup: int,
    measured: int,
    capture_path: Path | None = None,
) -> dict[str, Any]:
    require(arm in ARMS, f"unsupported Surface evidence arm: {arm}")
    records = parse_log(stdout, stderr)
    begin = only(records["begin"], "SURFACE_EXACT_EVIDENCE_BEGIN")
    capture = only(records["capture"], "SURFACE_EXACT_EVIDENCE_CAPTURE")
    summary = only(records["summary"], "SURFACE_EXACT_EVIDENCE_SUMMARY")
    total = warmup + measured
    common = {
        "trace_id": trace["trace_id"],
        "trace_sha256": trace["content_sha256"],
        "exact_plan_requested": arm,
    }
    assert_fields(
        begin,
        {
            **common,
            "geometry_path": "packed_atlas",
            "raster_execution_plan": "projected_quads_exact",
            "blend_mode": "sorted_alpha",
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "source_count": str(dataset["splat_count"]),
            "decoded_count": str(dataset["splat_count"]),
            "encoded_count": str(dataset["splat_count"]),
            "resident_count": str(dataset["splat_count"]),
            "addressable_count": str(dataset["splat_count"]),
            "sh_degree": str(dataset["sh_degree"]),
            "requested_width": str(trace["width"]),
            "requested_height": str(trace["height"]),
            "surface_width": str(trace["width"]),
            "surface_height": str(trace["height"]),
            "internal_render_width": str(trace["width"]),
            "internal_render_height": str(trace["height"]),
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": "true",
            "trace_frames": str(total),
            "adapter_backend": "metal",
        },
        "begin",
    )
    require(begin.get("adapter_name", "") not in ("", "unavailable"), "Metal adapter name unavailable")
    require(
        begin.get("adapter_device_type", "") not in ("", "other"),
        "adapter device type unavailable",
    )
    require_keys(begin, ("adapter_driver", "adapter_driver_info"), "begin")
    require(bool(begin["adapter_driver"]), "adapter driver field is not explicit")
    require(bool(begin["adapter_driver_info"]), "adapter driver-info field is not explicit")
    require(len(records["frame"]) == total, f"expected {total} frame receipts")

    tickets: set[int] = set()
    presentations: set[int] = set()
    actual_plans: set[str] = set()
    measured_records: list[dict[str, Any]] = []
    previous_elapsed = -1
    last_trace_frame: int | None = None
    for index, record in enumerate(records["frame"]):
        context = f"frame[{index}]"
        assert_fields(
            record,
            {
                **common,
                "sort_refreshed": "true",
                "requested_width": str(trace["width"]),
                "requested_height": str(trace["height"]),
                "presented_width": str(trace["width"]),
                "presented_height": str(trace["height"]),
            },
            context,
        )
        require(parse_uint(record.get("playback_index", ""), f"{context}.playback_index") == index,
                f"{context} playback index mismatch")
        phase = record.get("phase")
        require(phase in ("warmup", "measure"), f"{context}.phase is invalid")
        elapsed = parse_uint(record.get("elapsed_ns", ""), f"{context}.elapsed_ns")
        require(elapsed >= previous_elapsed, f"{context}.elapsed_ns is not monotonic")
        previous_elapsed = elapsed
        counts = validate_count_record(record, dataset["splat_count"], context)
        require(counts["ticket"] not in tickets, f"duplicate current-stats ticket {counts['ticket']}")
        require(counts["presentation_sequence"] not in presentations,
                f"duplicate presentation sequence {counts['presentation_sequence']}")
        tickets.add(counts["ticket"])
        presentations.add(counts["presentation_sequence"])
        actual_plans.add(counts["actual_plan"])
        if arm in FORCED_ACTUAL:
            require(counts["actual_plan"] == FORCED_ACTUAL[arm], f"{context} forced plan drift")
        call_ms = parse_float(record.get("call_ms", ""), f"{context}.call_ms")
        frame_wall_ms = parse_float(record.get("frame_wall_ms", ""), f"{context}.frame_wall_ms")
        last_trace_frame = parse_uint(record.get("trace_frame", ""), f"{context}.trace_frame")
        if phase == "measure":
            sample = parse_uint(record.get("measured_sample", ""), f"{context}.measured_sample")
            require(sample == len(measured_records), f"{context} measured sample index mismatch")
            measured_records.append(
                {
                    "elapsed_ns": elapsed,
                    "call_ms": call_ms,
                    "frame_wall_ms": frame_wall_ms,
                    "visible": counts["visible"],
                    "contributor": counts["contributor"],
                    "drawn": counts["drawn"],
                    "exact_contributor_compaction": counts["compacted"],
                    "sort_refreshed": parse_bool(record.get("sort_refreshed", ""), f"{context}.sort_refreshed"),
                    "exact_plan_actual": counts["actual_plan"],
                    "camera_revision": counts["camera_revision"],
                    "current_stats_ticket": counts["ticket"],
                    "presentation_sequence": counts["presentation_sequence"],
                }
            )
    require(len(measured_records) == measured, "measured frame count mismatch")

    assert_fields(
        capture,
        {
            "status": "ok",
            "exact_plan_requested": arm,
            "requested_width": str(trace["width"]),
            "requested_height": str(trace["height"]),
            "captured_width": str(trace["width"]),
            "captured_height": str(trace["height"]),
        },
        "capture",
    )
    capture_counts = validate_count_record(capture, dataset["splat_count"], "capture")
    require(capture_counts["ticket"] not in tickets, "capture reused a frame current-stats ticket")
    require(capture_counts["presentation_sequence"] not in presentations,
            "capture reused a frame presentation sequence")
    require(parse_uint(capture.get("trace_frame", ""), "capture.trace_frame") == last_trace_frame,
            "capture is not the final scheduled trace pose")
    if arm in FORCED_ACTUAL:
        require(capture_counts["actual_plan"] == FORCED_ACTUAL[arm], "capture forced plan drift")
    actual_plans.add(capture_counts["actual_plan"])
    if capture_path is not None:
        require(Path(capture.get("path", "")).resolve() == capture_path.resolve(),
                "capture.path does not match the collector path")

    assert_fields(
        summary,
        {
            "status": "ok",
            "exact_plan_requested": arm,
            "actual_plan_set": ",".join(sorted(actual_plans)),
            "trace_frames": str(total),
            "measured_frames": str(measured),
            "terminal_receipts": str(total + 1),
            "final_capture": "available",
        },
        "summary",
    )
    return {
        "frames": measured_records,
        "actual_plans": sorted(actual_plans),
        "final_actual_plan": capture_counts["actual_plan"],
        "eligibility_retries": parse_uint(summary.get("eligibility_retries", ""), "summary.eligibility_retries"),
        "capture_retries": parse_uint(summary.get("capture_retries", ""), "summary.capture_retries"),
        "adapter": {
            "backend": begin["adapter_backend"],
            "name": begin["adapter_name"],
            "device_type": begin["adapter_device_type"],
            "driver": begin.get("adapter_driver", "unavailable"),
            "driver_info": begin.get("adapter_driver_info", "unavailable"),
        },
    }


def png_dimensions(path: Path) -> tuple[int, int]:
    require(path.is_file(), f"final frame is missing: {path}")
    header = path.read_bytes()[:24]
    require(
        len(header) == 24
        and header[:8] == b"\x89PNG\r\n\x1a\n"
        and header[12:16] == b"IHDR",
        "final frame is not a PNG",
    )
    return int.from_bytes(header[16:20], "big"), int.from_bytes(header[20:24], "big")


def read_json(path: Path, context: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"cannot read {context}: {error}") from error
    require(isinstance(value, dict), f"{context} must be an object")
    return value


def read_dataset_manifest(repo: Path, path: Path) -> tuple[dict[str, Any], Path]:
    manifest = read_json(path, "dataset manifest")
    require(manifest.get("schema") == "gsplat-dataset/v1", "invalid dataset manifest schema")
    for key in ("id", "local_path", "sha256", "bytes", "splat_count", "sh_degree"):
        require(key in manifest, f"dataset manifest is missing {key}")
    require(HEX_64.fullmatch(str(manifest["sha256"])) is not None, "invalid dataset SHA-256")
    dataset_path = (repo / str(manifest["local_path"])).resolve()
    require(dataset_path.is_file(), f"dataset is unavailable: {dataset_path}")
    require(sha256_file(dataset_path) == manifest["sha256"], "dataset SHA-256 mismatch")
    require(dataset_path.stat().st_size == manifest["bytes"], "dataset byte count mismatch")
    require(manifest["sh_degree"] == 3, "formal M2b evidence requires source SH3")
    return manifest, dataset_path


def read_trace(path: Path) -> dict[str, Any]:
    value = read_json(path, "camera trace")
    display = value.get("display")
    require(isinstance(display, dict), "trace.display must be an object")
    require((display.get("width"), display.get("height")) == FORMAL_SIZE,
            "formal M2b evidence requires a 1920x1080 trace")
    trace_id = value.get("trace_id")
    content_sha = value.get("content_sha256")
    require(isinstance(trace_id, str) and trace_id, "trace_id is missing")
    require(isinstance(content_sha, str) and HEX_64.fullmatch(content_sha), "trace hash is invalid")
    return {
        "trace_id": trace_id,
        "content_sha256": content_sha,
        "file_sha256": sha256_file(path),
        "width": display["width"],
        "height": display["height"],
        "path": str(path.resolve()),
    }


def git_receipt(repo: Path) -> dict[str, Any]:
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=repo, check=True, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ).stdout.strip()
    status = subprocess.run(
        ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"], cwd=repo,
        check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ).stdout
    return {
        "commit": commit,
        "dirty": bool(status),
        "status_porcelain_sha256": hashlib.sha256(status).hexdigest(),
    }


def validate_ignored_output(repo: Path, output: Path) -> None:
    try:
        relative = output.resolve().relative_to(repo.resolve())
    except ValueError:
        return
    result = subprocess.run(["git", "check-ignore", "-q", "--", str(relative)], cwd=repo)
    require(result.returncode == 0, "output inside the repository must be gitignored")


def distribution(values: Sequence[float]) -> dict[str, Any]:
    require(bool(values), "cannot summarize an empty metric")
    ordered = sorted(values)
    total = 0.0
    for value in values:
        total += value

    def nearest(fraction: float) -> float:
        return ordered[max(math.ceil(fraction * len(ordered)) - 1, 0)]

    return {
        "count": len(values),
        "mean": total / len(values),
        "p50": nearest(0.50),
        "p90": nearest(0.90),
        "p95": nearest(0.95),
        "p99": nearest(0.99),
        "max": max(values),
    }


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def publish_validated_suite(stage: Path, output: Path, suite: dict[str, Any]) -> None:
    require(suite.get("schema") == SUITE_SCHEMA, "suite schema is not publishable")
    require(suite.get("status") == "ok", "suite status is not publishable")
    runs = suite.get("runs")
    require(isinstance(runs, list), "suite runs are unavailable")
    require([run.get("arm") for run in runs] == list(ARMS), "suite does not contain all four arms")
    for run in runs:
        require(run.get("validator_exit_status") == 0, f"{run.get('arm')} validator did not pass")
        artifact = stage / str(run.get("artifact", ""))
        for name in ("manifest.json", "frames.jsonl", "summary.json", "final-frame.png"):
            require((artifact / name).is_file(), f"{run.get('arm')} is missing {name}")
    require(not output.exists(), f"output appeared during collection: {output}")
    write_json(stage / "suite.json", suite)
    os.rename(stage, output)


def build_artifact(
    directory: Path,
    *,
    arm: str,
    validated: dict[str, Any],
    dataset: dict[str, Any],
    trace: dict[str, Any],
    build: dict[str, Any],
    capture_path: Path,
    started_at: str,
    ended_at: str,
    warmup: int,
    refresh_hz: float,
    host_device: str | None,
) -> dict[str, Any]:
    run_id = f"m2b-{build['git']['commit'][:12]}-{arm}-{uuid.uuid4().hex[:12]}"
    frame_budget_ms = 1000.0 / refresh_hz
    image = {
        "path": "final-frame.png",
        "sha256": sha256_file(capture_path),
        "width": trace["width"],
        "height": trace["height"],
    }
    unavailable = [
        "frames[*].preprocess_ms",
        "frames[*].sort_ms",
        "frames[*].geometry_submit_ms",
        "frames[*].gpu_wait_ms",
        "frames[*].gpu_complete_ms",
        "environment.browser",
    ]
    if host_device is None:
        unavailable.append("environment.device")
    if validated["adapter"]["driver"] == "unavailable":
        unavailable.append("environment.driver")
    if validated["adapter"]["driver_info"] == "unavailable":
        unavailable.append("environment.driver_info")
    manifest = {
        "schema": SCHEMA,
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {
            "series_id": "m2b-real-window-surface",
            "started_at_utc": started_at,
            "ended_at_utc": ended_at,
            "measurement_started_at_utc": started_at,
            "measurement_ended_at_utc": ended_at,
        },
        "build": {
            "repository_commit": build["git"]["commit"],
            "dirty": build["git"]["dirty"],
            "profile": "release",
            "package_version": build["package_version"],
            "executable_sha256": build["binary_sha256"],
            "status_porcelain_sha256": build["git"]["status_porcelain_sha256"],
        },
        "dataset": {
            "id": dataset["id"],
            "sha256": dataset["sha256"],
            "bytes": dataset["bytes"],
            "splat_count": dataset["splat_count"],
            "sh_degree": dataset["sh_degree"],
        },
        "trace": {
            "id": trace["trace_id"],
            "sha256": trace["content_sha256"],
            "file_sha256": trace["file_sha256"],
        },
        "renderer": {
            "implementation": "gsplat-rs Exact Surface",
            "path": "packed_atlas",
            "backend": validated["adapter"]["backend"],
            "sort_policy": "every_frame",
            "exact_plan_requested": arm,
            "exact_plan_actual": validated["final_actual_plan"],
            "exact_plan_actual_scope": "final_presented_capture",
            "actual_plans": validated["actual_plans"],
            "count_semantics": COUNT_SEMANTICS,
            "raster_execution_plan": "projected_quads_exact",
            "blend_mode": "sorted_alpha",
        },
        "display": {
            "width": trace["width"],
            "height": trace["height"],
            "dpr": 1.0,
            "refresh_hz": refresh_hz,
            "frame_budget_ms": frame_budget_ms,
            "refresh_hz_source": "collector_configured_budget",
            "frame_budget_source": "collector_configured_budget",
        },
        "environment": {
            "platform": "macOS",
            "os": platform.platform(),
            "device": host_device,
            "browser": None,
            "adapter": validated["adapter"]["name"],
            "driver": None
            if validated["adapter"]["driver"] == "unavailable"
            else validated["adapter"]["driver"],
            "adapter_device_type": validated["adapter"]["device_type"],
            "driver_info": None
            if validated["adapter"]["driver_info"] == "unavailable"
            else validated["adapter"]["driver_info"],
        },
        "exactness": {
            "source_splat_count": dataset["splat_count"],
            "decoded_splat_count": dataset["splat_count"],
            "encoded_splat_count": dataset["splat_count"],
            "resident_splat_count": dataset["splat_count"],
            "addressable_splat_count": dataset["splat_count"],
            "source_sh_degree": dataset["sh_degree"],
            "resident_sh_degree": dataset["sh_degree"],
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "sh_degree_policy": "source",
            "partial_scene_published": False,
            "full_quality": True,
        },
        "resolution": {
            "requested_width": trace["width"],
            "requested_height": trace["height"],
            "surface_width": trace["width"],
            "surface_height": trace["height"],
            "internal_render_width": trace["width"],
            "internal_render_height": trace["height"],
            "presented_width": trace["width"],
            "presented_height": trace["height"],
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": True,
        },
        "image": image,
        "unavailable_fields": unavailable,
    }
    frames = []
    for index, value in enumerate(validated["frames"]):
        frames.append(
            {
                "schema": SCHEMA,
                "record_type": "frame",
                "run_id": run_id,
                "frame_index": index,
                "elapsed_ns": value["elapsed_ns"],
                "call_ms": value["call_ms"],
                "frame_wall_ms": value["frame_wall_ms"],
                "preprocess_ms": None,
                "sort_ms": None,
                "geometry_submit_ms": None,
                "gpu_wait_ms": None,
                "gpu_complete_ms": None,
                "visible": value["visible"],
                "contributor": value["contributor"],
                "drawn": value["drawn"],
                "exact_contributor_compaction": value["exact_contributor_compaction"],
                "sort_refreshed": value["sort_refreshed"],
                "exact_plan_actual": value["exact_plan_actual"],
                "camera_revision": value["camera_revision"],
                "current_stats_ticket": value["current_stats_ticket"],
                "presentation_sequence": value["presentation_sequence"],
            }
        )
    call_values = [frame["call_ms"] for frame in frames]
    wall_values = [frame["frame_wall_ms"] for frame in frames]
    summary = {
        "schema": SCHEMA,
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": len(frames),
        "warmup_count": warmup,
        "frame_budget_ms": frame_budget_ms,
        "missed_frame_count": sum(value > frame_budget_ms for value in wall_values),
        "distributions": {
            "call_ms": distribution(call_values),
            "frame_wall_ms": distribution(wall_values),
            "preprocess_ms": None,
            "sort_ms": None,
            "geometry_submit_ms": None,
            "gpu_wait_ms": None,
            "gpu_complete_ms": None,
        },
        "evidence": {
            "eligibility_retries": validated["eligibility_retries"],
            "capture_retries": validated["capture_retries"],
            "final_capture": "available",
        },
    }
    write_json(directory / "manifest.json", manifest)
    (directory / "frames.jsonl").write_text(
        "".join(json.dumps(frame, sort_keys=True) + "\n" for frame in frames), encoding="utf-8"
    )
    write_json(directory / "summary.json", summary)
    return manifest


def host_device_name() -> str | None:
    result = subprocess.run(
        ["sysctl", "-n", "machdep.cpu.brand_string"], check=False, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    value = result.stdout.strip()
    return value or None


def package_version(repo: Path) -> str:
    text = (repo / "examples/desktop/Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    require(match is not None, "desktop package version is unavailable")
    return match.group(1)


def make_command(
    binary: Path,
    dataset_path: Path,
    trace_path: Path,
    arm: str,
    warmup: int,
    measured: int,
    capture_path: Path,
) -> list[str]:
    return [
        str(binary), str(dataset_path),
        "--geometry-path", "packed",
        "--interactive",
        "--camera-trace", str(trace_path),
        "--camera-sequence",
        "--camera-warmup-frames", str(warmup),
        "--camera-measured-frames", str(measured),
        "--camera-loops", "1",
        "--surface-benchmark-mode", "isolated",
        "--surface-sort-policy", "every-frame",
        "--surface-raster-plan", "projected",
        "--surface-evidence-plan", CLI_PLAN[arm],
        "--png", str(capture_path),
    ]


def collect(args: argparse.Namespace, repo: Path) -> dict[str, Any]:
    output = args.output.resolve()
    require(not output.exists(), f"output already exists: {output}")
    require(args.binary.is_file() and os.access(args.binary, os.X_OK), "binary is unavailable")
    require("release" in args.binary.resolve().parts, "formal evidence requires a release binary")
    require(args.warmup >= 0 and args.measured > 0, "invalid warmup/measured counts")
    require(math.isfinite(args.refresh_hz) and args.refresh_hz > 0.0, "refresh-hz must be positive")
    validate_ignored_output(repo, output)
    dataset, dataset_path = read_dataset_manifest(repo, args.dataset_manifest.resolve())
    trace = read_trace(args.trace.resolve())
    build = {
        "binary_sha256": sha256_file(args.binary),
        "package_version": package_version(repo),
        "git": git_receipt(repo),
    }
    stage = output.parent / f".{output.name}.stage-{os.getpid()}-{uuid.uuid4().hex[:12]}"
    require(not stage.exists(), f"staging path exists: {stage}")
    stage.mkdir(parents=True)
    started_at = utc_now()
    suite: dict[str, Any] = {
        "schema": SUITE_SCHEMA,
        "status": "running",
        "started_at_utc": started_at,
        "build": build,
        "dataset": dataset,
        "trace": trace,
        "required_adapter_backend": "metal",
        "runs": [],
    }
    try:
        for arm in ARMS:
            arm_dir = stage / arm
            artifact_dir = arm_dir / "artifact"
            artifact_dir.mkdir(parents=True)
            capture_path = artifact_dir / "final-frame.png"
            command = make_command(
                args.binary.resolve(), dataset_path, args.trace.resolve(), arm,
                args.warmup, args.measured, capture_path,
            )
            write_json(arm_dir / "command.json", {"argv": command})
            run_started = utc_now()
            completed = subprocess.run(
                command, cwd=repo, check=False, text=True,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=RUN_TIMEOUT_SECONDS,
            )
            (arm_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
            (arm_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
            require(completed.returncode == 0, f"{arm} exited with {completed.returncode}")
            require(sha256_file(args.binary) == build["binary_sha256"], "binary changed during collection")
            require(sha256_file(dataset_path) == dataset["sha256"], "dataset changed during collection")
            require(sha256_file(args.trace) == trace["file_sha256"], "trace changed during collection")
            require(git_receipt(repo) == build["git"], "git receipt changed during collection")
            validated = validate_run_log(
                completed.stdout, completed.stderr, arm=arm, dataset=dataset, trace=trace,
                warmup=args.warmup, measured=args.measured, capture_path=capture_path,
            )
            require(png_dimensions(capture_path) == FORMAL_SIZE, f"{arm} final frame size mismatch")
            ended_at = utc_now()
            manifest = build_artifact(
                artifact_dir, arm=arm, validated=validated, dataset=dataset, trace=trace,
                build=build, capture_path=capture_path, started_at=run_started, ended_at=ended_at,
                warmup=args.warmup, refresh_hz=args.refresh_hz, host_device=host_device_name(),
            )
            validator = subprocess.run(
                [sys.executable, str(repo / "tests/perf/validate-benchmark-artifacts.py"), str(artifact_dir)],
                cwd=repo, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            (arm_dir / "validator.stdout.log").write_text(validator.stdout, encoding="utf-8")
            (arm_dir / "validator.stderr.log").write_text(validator.stderr, encoding="utf-8")
            require(validator.returncode == 0, f"{arm} canonical artifact validation failed")
            suite["runs"].append(
                {
                    "arm": arm,
                    "artifact": f"{arm}/artifact",
                    "run_id": manifest["run_id"],
                    "exact_plan_actual": manifest["renderer"]["exact_plan_actual"],
                    "image_sha256": manifest["image"]["sha256"],
                    "command": f"{arm}/command.json",
                    "stdout": f"{arm}/stdout.log",
                    "stderr": f"{arm}/stderr.log",
                    "validator_exit_status": validator.returncode,
                }
            )
        suite.update({"status": "ok", "ended_at_utc": utc_now()})
        publish_validated_suite(stage, output, suite)
        return suite
    except Exception as error:
        suite.update({"status": "failed", "ended_at_utc": utc_now(), "error": str(error)})
        write_json(stage / "suite.json", suite)
        failed = output.parent / f"{output.name}.failed-{os.getpid()}-{uuid.uuid4().hex[:8]}"
        os.replace(stage, failed)
        raise ValidationError(f"{error}; retained_failed_evidence={failed}") from error


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect four strict M2b real-window Surface artifacts")
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dataset-manifest", required=True, type=Path)
    parser.add_argument("--trace", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--warmup", type=int, default=20)
    parser.add_argument("--measured", type=int, default=80)
    parser.add_argument("--refresh-hz", type=float, default=60.0)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    repo = Path(__file__).resolve().parents[2]
    try:
        suite = collect(args, repo)
    except (OSError, subprocess.SubprocessError, ValidationError) as error:
        print(f"desktop Surface evidence failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"status": suite["status"], "output": str(args.output), "arms": list(ARMS)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
