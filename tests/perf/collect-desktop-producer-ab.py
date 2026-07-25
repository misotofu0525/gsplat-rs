#!/usr/bin/env python3
"""Collect a fail-closed desktop PostSort/Preproject paired A/B experiment.

The collector deliberately owns no renderer policy.  It invokes one already
built release binary twice per randomized pair and proves from the native
Surface receipts that both arms used the exact same full-quality contract.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
import random
import re
import shlex
import statistics
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Sequence


SCHEMA = "gsplat-desktop-producer-ab/v1"
FORMAL_DESKTOP_SIZE = (1920, 1080)
DEFAULT_BOOTSTRAP_SAMPLES = 20_000
RUN_TIMEOUT_SECONDS = 30 * 60
PRODUCERS = ("post_sort", "preproject")
CLI_PRODUCER = {"post_sort": "post-sort", "preproject": "preproject"}
RECORD_PREFIXES = {
    "begin": "SURFACE_BENCHMARK_BEGIN ",
    "frame": "SURFACE_FRAME_RECEIPT ",
    "gpu": "SURFACE_GPU_MEASUREMENT ",
    "producer": "SURFACE_GPU_PRODUCER_MEASUREMENT ",
    "capture": "SURFACE_CAPTURE ",
    "summary": "SURFACE_BENCHMARK_SUMMARY ",
}
HEX_64 = re.compile(r"^[0-9a-f]{64}$")


class ValidationError(ValueError):
    """Evidence is missing, contradictory, or outside the formal contract."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def png_dimensions(path: Path) -> tuple[int, int]:
    require(path.is_file(), f"capture PNG is missing: {path}")
    header = path.read_bytes()[:24]
    require(
        len(header) == 24
        and header[:8] == b"\x89PNG\r\n\x1a\n"
        and header[12:16] == b"IHDR",
        f"capture is not a PNG with an IHDR header: {path}",
    )
    return (int.from_bytes(header[16:20], "big"), int.from_bytes(header[20:24], "big"))


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def parse_bool(value: str, context: str) -> bool:
    require(value in ("true", "false"), f"{context} must be true or false, got {value!r}")
    return value == "true"


def parse_uint(value: str, context: str, *, positive: bool = False) -> int:
    require(re.fullmatch(r"[0-9]+", value) is not None, f"{context} must be an integer")
    parsed = int(value)
    if positive:
        require(parsed > 0, f"{context} must be positive")
    return parsed


def parse_float(value: str, context: str) -> float:
    try:
        parsed = float(value)
    except ValueError as error:
        raise ValidationError(f"{context} must be a finite number") from error
    require(math.isfinite(parsed) and parsed >= 0.0, f"{context} must be finite and non-negative")
    return parsed


def only(records: Sequence[dict[str, str]], context: str) -> dict[str, str]:
    require(len(records) == 1, f"expected exactly one {context}, got {len(records)}")
    return records[0]


def require_keys(record: dict[str, str], keys: Iterable[str], context: str) -> None:
    missing = sorted(set(keys) - record.keys())
    require(not missing, f"{context} is missing fields: {', '.join(missing)}")


def parse_key_value_payload(payload: str, context: str) -> dict[str, str]:
    try:
        words = shlex.split(payload, posix=True)
    except ValueError as error:
        raise ValidationError(f"{context} has invalid shell-style quoting") from error
    require(words, f"{context} has no fields")
    result: dict[str, str] = {}
    for word in words:
        require("=" in word, f"{context} contains a non key=value token: {word!r}")
        key, value = word.split("=", 1)
        require(key and value, f"{context} contains an empty key or value")
        require(key not in result, f"{context} repeats field {key!r}")
        result[key] = value
    return result


def parse_log(stdout: str, stderr: str = "") -> dict[str, list[dict[str, str]]]:
    records: dict[str, list[dict[str, str]]] = {name: [] for name in RECORD_PREFIXES}
    for stream_name, stream in (("stdout", stdout), ("stderr", stderr)):
        for line_number, line in enumerate(stream.splitlines(), 1):
            for name, prefix in RECORD_PREFIXES.items():
                if line.startswith(prefix):
                    records[name].append(
                        parse_key_value_payload(
                            line[len(prefix) :], f"{stream_name}:{line_number}:{prefix.strip()}"
                        )
                    )
                    break
    return records


def read_ply_receipt(path: Path) -> dict[str, Any]:
    require(path.is_file(), f"dataset is not a file: {path}")
    with path.open("rb") as handle:
        header = handle.read(4 * 1024 * 1024)
    end = header.find(b"end_header")
    require(end >= 0, "PLY end_header was not found in the first 4 MiB")
    newline = header.find(b"\n", end)
    require(newline >= 0, "PLY end_header is not newline terminated")
    try:
        lines = header[: newline + 1].decode("ascii").splitlines()
    except UnicodeDecodeError as error:
        raise ValidationError("PLY header is not ASCII") from error
    require(lines and lines[0].strip() == "ply", "dataset is not a PLY file")

    vertex_count: int | None = None
    in_vertex = False
    rest_indices: list[int] = []
    for line in lines:
        fields = line.split()
        if fields[:2] == ["element", "vertex"]:
            require(len(fields) == 3, "invalid PLY vertex element")
            vertex_count = parse_uint(fields[2], "PLY vertex count", positive=True)
            in_vertex = True
        elif fields and fields[0] == "element":
            in_vertex = False
        elif in_vertex and len(fields) >= 3 and fields[0] == "property":
            name = fields[-1]
            match = re.fullmatch(r"f_rest_([0-9]+)", name)
            if match:
                rest_indices.append(int(match.group(1)))

    require(vertex_count is not None, "PLY header has no vertex element")
    require(sorted(rest_indices) == list(range(45)), "formal producer A/B requires complete SH3")
    return {
        "path": str(path.resolve()),
        "sha256": sha256_file(path),
        "bytes": path.stat().st_size,
        "splat_count": vertex_count,
        "sh_degree": 3,
    }


def read_trace_receipt(path: Path) -> dict[str, Any]:
    require(path.is_file(), f"trace is not a file: {path}")
    try:
        trace = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"cannot parse trace JSON: {path}") from error
    require(isinstance(trace, dict), "trace root must be an object")
    display = trace.get("display")
    require(isinstance(display, dict), "trace.display must be an object")
    size = (display.get("width"), display.get("height"))
    require(size == FORMAL_DESKTOP_SIZE, "desktop producer A/B requires a 1920x1080 formal trace")
    trace_id = trace.get("trace_id")
    content_sha = trace.get("content_sha256")
    frames = trace.get("frames")
    require(isinstance(trace_id, str) and trace_id, "trace.trace_id must be non-empty")
    require(isinstance(content_sha, str) and HEX_64.fullmatch(content_sha), "invalid trace content_sha256")
    require(isinstance(frames, list) and frames, "trace.frames must be non-empty")
    return {
        "path": str(path.resolve()),
        "file_sha256": sha256_file(path),
        "content_sha256": content_sha,
        "trace_id": trace_id,
        "width": size[0],
        "height": size[1],
        "frame_count": len(frames),
    }


def git_receipt(repo: Path) -> dict[str, Any]:
    def git(*args: str) -> bytes:
        completed = subprocess.run(
            ["git", *args], cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
        return completed.stdout

    commit = git("rev-parse", "HEAD").decode("ascii").strip()
    status = git("status", "--porcelain=v1", "-z", "--untracked-files=all")
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
    completed = subprocess.run(
        ["git", "check-ignore", "-q", "--", str(relative)], cwd=repo, check=False
    )
    require(completed.returncode == 0, "output inside the repository must be gitignored")


def assert_equal(record: dict[str, str], expected: dict[str, str], context: str) -> None:
    require_keys(record, expected, context)
    for key, value in expected.items():
        require(record[key] == value, f"{context}.{key}: expected {value!r}, got {record[key]!r}")


def validate_dimensions(record: dict[str, str], width: int, height: int, context: str) -> None:
    expected = {
        "requested_width": str(width),
        "requested_height": str(height),
        "surface_width": str(width),
        "surface_height": str(height),
        "internal_render_width": str(width),
        "internal_render_height": str(height),
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
    }
    if "presented_width" in record or "presented_height" in record:
        expected.update({"presented_width": str(width), "presented_height": str(height)})
    assert_equal(record, expected, context)


def percentile(values: Sequence[float], fraction: float) -> float:
    require(bool(values), "cannot summarize an empty distribution")
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * fraction)]


def distribution(values: Sequence[float]) -> dict[str, Any]:
    require(bool(values), "timing distribution is empty")
    for value in values:
        require(math.isfinite(value) and value >= 0.0, "timing distribution is invalid")
    return {
        "count": len(values),
        "mean": statistics.fmean(values),
        "median": statistics.median(values),
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "min": min(values),
        "max": max(values),
    }


def validate_run_log(
    stdout: str,
    stderr: str,
    *,
    producer: str,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    warmup: int,
    measured: int,
    mode: str,
    capture_path: Path | None = None,
) -> dict[str, Any]:
    require(producer in PRODUCERS, f"unsupported producer: {producer}")
    records = parse_log(stdout, stderr)
    begin = only(records["begin"], "SURFACE_BENCHMARK_BEGIN")
    summary = only(records["summary"], "SURFACE_BENCHMARK_SUMMARY")
    expected_total = warmup + measured
    common_identity = {
        "trace_id": trace["trace_id"],
        "trace_sha256": trace["content_sha256"],
        "benchmark_mode": mode,
        "sort_policy": "every_frame",
        "requested_backend": "gpu",
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "gpu_order_producer": producer,
        "projected_draw_policy": "compact",
        "producer_measurement_enabled": "true",
        "source_count": str(dataset["splat_count"]),
        "resident_count": str(dataset["splat_count"]),
        "sh_degree": "3",
    }
    assert_equal(begin, common_identity, "begin")
    validate_dimensions(begin, trace["width"], trace["height"], "begin")
    require(parse_uint(begin.get("trace_frames", ""), "begin.trace_frames") == expected_total,
            "begin.trace_frames does not match warmup + measured")

    scheduled: list[dict[str, str]] = []
    for index, frame in enumerate(records["frame"]):
        context = f"frame[{index}]"
        phase = frame.get("phase")
        if phase == "drain":
            continue
        require(phase in ("warmup", "measure"), f"{context}.phase is invalid")
        scheduled.append(frame)
        assert_equal(
            frame,
            {
                "sort_policy": "every_frame",
                "requested_backend": "gpu",
                "actual_backend": "gpu",
                "raster_execution_plan": "projected_quads_exact",
                "gpu_order_producer_requested": producer,
                "gpu_order_producer_actual": producer,
                "producer_unsampled_reason": "none",
                "frame_presented": "true",
                "tiled_preparation_pending": "false",
                "sort_refreshed": "true",
                "gpu_sort_fallback": "false",
                "source_count": str(dataset["splat_count"]),
                "resident_count": str(dataset["splat_count"]),
                "measurement_backend": "gpu",
                "measurement_unsampled_reason": "none",
            },
            context,
        )
        validate_dimensions(frame, trace["width"], trace["height"], context)

    require(len(scheduled) == expected_total, f"expected {expected_total} scheduled frames, got {len(scheduled)}")
    playback = [parse_uint(frame.get("playback_index", ""), "frame.playback_index") for frame in scheduled]
    require(sorted(playback) == list(range(expected_total)), "scheduled playback indices are not complete and unique")
    phases = [frame["phase"] for frame in scheduled]
    require(phases.count("warmup") == warmup and phases.count("measure") == measured,
            "warmup/measured frame counts do not match the request")
    measured_samples = sorted(
        parse_uint(frame.get("measured_sample", ""), "frame.measured_sample")
        for frame in scheduled
        if frame["phase"] == "measure"
    )
    require(measured_samples == list(range(measured)), "measured_sample indices are not complete")

    # A frame is the authoritative join between independently numbered order
    # and producer tickets.  Keeping the peer ticket in both ledgers prevents
    # two individually valid terminal streams from being combined across
    # different frames.
    order_issued: dict[int, tuple[int, bool, int]] = {}
    producer_issued: dict[int, tuple[int, bool, int]] = {}
    for frame in scheduled:
        # Camera revisions are monotonically increasing identities, but the
        # first applied trace frame is legitimately revision zero.
        camera_revision = parse_uint(
            frame.get("camera_revision", ""), "frame.camera_revision"
        )
        is_measured = frame["phase"] == "measure"
        order_ticket = parse_uint(
            frame.get("measurement_ticket_submitted", ""), "frame.measurement_ticket_submitted", positive=True
        )
        producer_ticket = parse_uint(
            frame.get("producer_ticket_submitted", ""), "frame.producer_ticket_submitted", positive=True
        )
        require(order_ticket not in order_issued, f"order ticket {order_ticket} was issued twice")
        require(producer_ticket not in producer_issued, f"producer ticket {producer_ticket} was issued twice")
        gpu_ticket = parse_uint(frame.get("gpu_ticket_submitted", ""), "frame.gpu_ticket_submitted", positive=True)
        require(gpu_ticket == order_ticket, f"frame order/gpu ticket mismatch for ticket {order_ticket}")
        order_issued[order_ticket] = (camera_revision, is_measured, producer_ticket)
        producer_issued[producer_ticket] = (camera_revision, is_measured, order_ticket)

    order_terminal: dict[int, dict[str, str]] = {}
    queue_measured: list[float] = []
    for index, terminal in enumerate(records["gpu"]):
        context = f"gpu_terminal[{index}]"
        require_keys(
            terminal,
            (
                "ticket", "camera_revision", "measured", "gpu_completion_ms", "visible_count",
                "contributor_count", "drawn_count", "exact_contributor_compaction",
            ),
            context,
        )
        ticket = parse_uint(terminal["ticket"], f"{context}.ticket", positive=True)
        require(ticket in order_issued, f"unknown order terminal ticket {ticket}")
        require(ticket not in order_terminal, f"order ticket {ticket} has more than one terminal receipt")
        revision, expected_measured, _producer_ticket = order_issued[ticket]
        require(parse_uint(terminal["camera_revision"], f"{context}.camera_revision") == revision,
                f"order ticket {ticket} camera revision mismatch")
        terminal_measured = parse_bool(terminal["measured"], f"{context}.measured")
        require(terminal_measured == expected_measured, f"order ticket {ticket} measured flag mismatch")
        visible = parse_uint(terminal["visible_count"], f"{context}.visible_count")
        contributor = parse_uint(terminal["contributor_count"], f"{context}.contributor_count")
        drawn = parse_uint(terminal["drawn_count"], f"{context}.drawn_count")
        require(0 <= contributor <= visible <= dataset["splat_count"], f"order ticket {ticket} violates C<=V<=S")
        require(drawn == contributor, f"order ticket {ticket} does not prove D=C")
        require(parse_bool(terminal["exact_contributor_compaction"], f"{context}.exact_contributor_compaction"),
                f"order ticket {ticket} is not exact contributor compaction")
        completion = parse_float(terminal["gpu_completion_ms"], f"{context}.gpu_completion_ms")
        if terminal_measured:
            queue_measured.append(completion)
        order_terminal[ticket] = terminal
    require(set(order_terminal) == set(order_issued), "order tickets do not have exactly one terminal receipt")

    producer_terminal: dict[int, dict[str, str]] = {}
    producer_measured: list[float] = []
    for index, terminal in enumerate(records["producer"]):
        context = f"producer_terminal[{index}]"
        require_keys(
            terminal,
            (
                "ticket", "camera_revision", "measured", "producer", "order_generation",
                "projection_generation", "source_count", "contributor_count", "drawn_count",
                "order_refreshed", "draw_scope", "exact_current_contributor_draw", "stale_order",
                "frame_completion_ms",
            ),
            context,
        )
        ticket = parse_uint(terminal["ticket"], f"{context}.ticket", positive=True)
        require(ticket in producer_issued, f"unknown producer terminal ticket {ticket}")
        require(ticket not in producer_terminal, f"producer ticket {ticket} has more than one terminal receipt")
        revision, expected_measured, _order_ticket = producer_issued[ticket]
        require(parse_uint(terminal["camera_revision"], f"{context}.camera_revision") == revision,
                f"producer ticket {ticket} camera revision mismatch")
        terminal_measured = parse_bool(terminal["measured"], f"{context}.measured")
        require(terminal_measured == expected_measured, f"producer ticket {ticket} measured flag mismatch")
        require(terminal["producer"] == producer, f"producer ticket {ticket} actual producer mismatch")
        require(parse_uint(terminal["order_generation"], f"{context}.order_generation", positive=True) > 0,
                f"producer ticket {ticket} lacks an order generation")
        require(parse_uint(terminal["projection_generation"], f"{context}.projection_generation", positive=True) > 0,
                f"producer ticket {ticket} lacks a projection generation")
        source = parse_uint(terminal["source_count"], f"{context}.source_count")
        contributor = parse_uint(terminal["contributor_count"], f"{context}.contributor_count")
        drawn = parse_uint(terminal["drawn_count"], f"{context}.drawn_count")
        require(source == dataset["splat_count"], f"producer ticket {ticket} source count mismatch")
        require(drawn == contributor <= source, f"producer ticket {ticket} violates C=D<=S")
        assert_equal(
            terminal,
            {
                "order_refreshed": "true",
                "draw_scope": "exact_current_contributors",
                "exact_current_contributor_draw": "true",
                "stale_order": "false",
            },
            context,
        )
        completion = parse_float(terminal["frame_completion_ms"], f"{context}.frame_completion_ms")
        if terminal_measured:
            producer_measured.append(completion)
        producer_terminal[ticket] = terminal
    require(set(producer_terminal) == set(producer_issued),
            "producer tickets do not have exactly one terminal receipt")
    for producer_ticket, (_revision, _measured, order_ticket) in producer_issued.items():
        order_receipt = order_terminal[order_ticket]
        producer_receipt = producer_terminal[producer_ticket]
        for count_name in ("contributor_count", "drawn_count"):
            order_count = parse_uint(
                order_receipt[count_name], f"order ticket {order_ticket}.{count_name}"
            )
            producer_count = parse_uint(
                producer_receipt[count_name], f"producer ticket {producer_ticket}.{count_name}"
            )
            require(
                order_count == producer_count,
                f"same-frame order ticket {order_ticket} and producer ticket {producer_ticket} "
                f"disagree on {count_name}: {order_count} != {producer_count}",
            )
    require(len(queue_measured) == measured, "queue-complete measured ticket count mismatch")
    require(len(producer_measured) == measured, "producer-complete measured ticket count mismatch")

    capture = only(records["capture"], "SURFACE_CAPTURE")
    expected_capture = {
        "status": "ok",
        "trace_frame": "0",
        "requested_width": str(trace["width"]),
        "requested_height": str(trace["height"]),
        "captured_width": str(trace["width"]),
        "captured_height": str(trace["height"]),
        "gpu_order_producer_requested": producer,
        "gpu_order_producer_actual": producer,
        "producer_measurement_enabled": "false",
        "measured": "false",
        "terminal_receipt": "success",
    }
    assert_equal(capture, expected_capture, "capture")
    parse_uint(capture.get("order_ticket", ""), "capture.order_ticket", positive=True)
    if capture_path is not None:
        require(
            Path(capture.get("path", "")).resolve() == capture_path.resolve(),
            "capture.path does not match the collector-owned PNG path",
        )

    assert_equal(summary, {**common_identity, "status": "ok"}, "summary")
    validate_dimensions(summary, trace["width"], trace["height"], "summary")
    expected_summary = {
        "final_actual_backend": "gpu",
        "trace_frames": str(expected_total),
        "presented_frames": str(expected_total),
        "measured_frames": str(measured),
        "measured_cpu_frames": "0",
        "measured_gpu_frames": str(measured),
        "sort_refreshes": str(expected_total),
        "gpu_fallback_frames": "0",
        "gpu_refreshes_without_ticket": "0",
        "cpu_requests_without_ticket": "0",
        "surface_unavailable_measurements": "0",
        "producer_exact_frames": str(expected_total),
        "producer_stale_frames": "0",
        "producer_ring_busy": "0",
        "producer_surface_unavailable": "0",
        "terminal_order_tickets": str(expected_total),
        "terminal_producer_tickets": str(expected_total),
        "outstanding_cpu_tickets": "0",
        "outstanding_gpu_tickets": "0",
        "outstanding_producer_tickets": "0",
    }
    assert_equal(summary, expected_summary, "summary")
    mean_queue = parse_float(summary.get("mean_gpu_completion_ms", ""), "summary.mean_gpu_completion_ms")
    mean_producer = parse_float(
        summary.get("mean_gpu_producer_completion_ms", ""), "summary.mean_gpu_producer_completion_ms"
    )
    require(math.isclose(mean_queue, statistics.fmean(queue_measured), rel_tol=1e-5, abs_tol=1e-4),
            "summary queue-complete mean disagrees with terminal receipts")
    require(math.isclose(mean_producer, statistics.fmean(producer_measured), rel_tol=1e-5, abs_tol=1e-4),
            "summary producer-complete mean disagrees with terminal receipts")

    return {
        "producer": producer,
        "scheduled_frames": expected_total,
        "warmup_frames": warmup,
        "measured_frames": measured,
        "order_ticket_count": len(order_terminal),
        "producer_ticket_count": len(producer_terminal),
        "queue_complete_ms": distribution(queue_measured),
        "producer_completion_ms": distribution(producer_measured),
    }


def pair_order_balance(schedule: Sequence[Sequence[str]]) -> dict[str, Any]:
    ab = list(PRODUCERS)
    ba = list(reversed(PRODUCERS))
    ab_pairs = sum(list(order) == ab for order in schedule)
    ba_pairs = sum(list(order) == ba for order in schedule)
    require(
        ab_pairs + ba_pairs == len(schedule),
        "every pair order must contain PostSort and Preproject exactly once",
    )
    absolute_difference = abs(ab_pairs - ba_pairs)
    require(
        absolute_difference <= 1,
        f"AB/BA pair-order imbalance exceeds one: AB={ab_pairs}, BA={ba_pairs}",
    )
    return {
        "ab_order": ab,
        "ba_order": ba,
        "ab_pairs": ab_pairs,
        "ba_pairs": ba_pairs,
        "absolute_difference": absolute_difference,
        "odd_pair_extra_order": (
            ab if ab_pairs > ba_pairs else ba if ba_pairs > ab_pairs else None
        ),
    }


def odd_pair_extra_is_ab(seed: int) -> bool:
    """Choose the odd-pair extra exposure from a stable seed-derived bit."""

    digest = hashlib.sha256(
        f"gsplat-desktop-producer-ab/odd-extra/{seed}".encode("ascii")
    ).digest()
    return digest[0] & 1 == 0


def schedule_pairs(pairs: int, seed: int) -> list[list[str]]:
    require(pairs > 0, "pairs must be positive")
    ab = list(PRODUCERS)
    ba = list(reversed(PRODUCERS))
    # Counterbalance the exact halves first. For an odd pair count, a separate
    # stable seed-derived bit chooses which order receives the unavoidable
    # one-extra exposure, avoiding systematic PostSort bias. The seeded shuffle
    # then controls only where those pre-balanced orders occur in time.
    half = pairs // 2
    schedule = [ab.copy() for _ in range(half)]
    schedule.extend(ba.copy() for _ in range(half))
    if pairs % 2 != 0:
        schedule.append((ab if odd_pair_extra_is_ab(seed) else ba).copy())
    rng = random.Random(seed)
    rng.shuffle(schedule)
    pair_order_balance(schedule)
    return schedule


def bootstrap_median_ci(values: Sequence[float], *, seed: int, samples: int = DEFAULT_BOOTSTRAP_SAMPLES) -> list[float]:
    require(bool(values), "paired bootstrap requires at least one value")
    require(samples > 0, "bootstrap sample count must be positive")
    rng = random.Random(seed)
    count = len(values)
    medians = [
        statistics.median(values[rng.randrange(count)] for _ in range(count))
        for _ in range(samples)
    ]
    return [percentile(medians, 0.025), percentile(medians, 0.975)]


def aggregate_pairs(pair_results: Sequence[dict[str, Any]], seed: int) -> dict[str, Any]:
    require(bool(pair_results), "cannot aggregate zero pairs")
    result: dict[str, Any] = {}
    for metric_index, metric in enumerate(("queue_complete_ms", "producer_completion_ms")):
        metric_result: dict[str, Any] = {}
        for statistic_index, statistic in enumerate(("mean", "median", "p95", "p99")):
            ratios: list[float] = []
            differences: list[float] = []
            for pair in pair_results:
                post = pair["runs"]["post_sort"][metric][statistic]
                pre = pair["runs"]["preproject"][metric][statistic]
                require(post > 0.0, f"{metric}.{statistic} PostSort denominator must be positive")
                ratios.append(pre / post)
                differences.append(pre - post)
            bootstrap_seed = seed ^ 0x4753504C4154 ^ (metric_index << 16) ^ statistic_index
            metric_result[statistic] = {
                "preproject_over_post_sort_ratios": ratios,
                "paired_differences_ms": differences,
                "median_ratio": statistics.median(ratios),
                "median_difference_ms": statistics.median(differences),
                "ratio_median_bootstrap_95_ci": bootstrap_median_ci(
                    ratios, seed=bootstrap_seed
                ),
            }
        result[metric] = metric_result
    return result


@dataclass(frozen=True)
class Inputs:
    binary: Path
    dataset_path: Path
    trace_path: Path
    output: Path
    pairs: int
    seed: int
    warmup: int
    measured: int
    mode: str


def make_command(inputs: Inputs, producer: str, capture_path: Path | None = None) -> list[str]:
    command = [
        str(inputs.binary),
        str(inputs.dataset_path),
        "--geometry-path", "packed",
        "--interactive",
        "--camera-trace", str(inputs.trace_path),
        "--camera-sequence",
        "--camera-warmup-frames", str(inputs.warmup),
        "--camera-measured-frames", str(inputs.measured),
        "--camera-loops", "1",
        "--surface-benchmark-mode", inputs.mode,
        "--surface-sort-policy", "every-frame",
        "--order-backend", "gpu",
        "--surface-gpu-producer", CLI_PRODUCER[producer],
    ]
    if capture_path is not None:
        command.extend(("--png", str(capture_path)))
    return command


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def collect(inputs: Inputs, repo: Path) -> dict[str, Any]:
    require(not inputs.output.exists(), f"output directory already exists: {inputs.output}")
    require(inputs.pairs > 0, "pairs must be positive")
    require(inputs.warmup >= 0, "warmup must be non-negative")
    require(inputs.measured > 0, "measured must be positive")
    require(inputs.mode in ("isolated", "throughput"), "mode must be isolated or throughput")
    require(inputs.binary.is_file(), f"binary is not a file: {inputs.binary}")
    require(os.access(inputs.binary, os.X_OK), f"binary is not executable: {inputs.binary}")
    require("release" in inputs.binary.resolve().parts, "formal A/B requires a release binary path")
    validate_ignored_output(repo, inputs.output)

    dataset = read_ply_receipt(inputs.dataset_path)
    trace = read_trace_receipt(inputs.trace_path)
    binary_sha = sha256_file(inputs.binary)
    build = {
        "binary": str(inputs.binary.resolve()),
        "binary_sha256": binary_sha,
        "profile": "release",
        "git": git_receipt(repo),
    }
    schedule = schedule_pairs(inputs.pairs, inputs.seed)
    schedule_balance = pair_order_balance(schedule)
    started_at = utc_now()
    inputs.output.mkdir(parents=True)
    run_results: list[dict[str, Any]] = []
    pair_results: list[dict[str, Any]] = []

    base_experiment: dict[str, Any] = {
        "schema": SCHEMA,
        "status": "running",
        "started_at_utc": started_at,
        "build": build,
        "dataset": dataset,
        "trace": trace,
        "config": {
            "pairs": inputs.pairs,
            "seed": inputs.seed,
            "warmup_frames": inputs.warmup,
            "measured_frames": inputs.measured,
            "mode": inputs.mode,
            "geometry_path": "packed_atlas",
            "raster_execution_plan": "projected_quads_exact",
            "projected_draw_policy": "compact",
            "order_backend": "gpu",
            "sort_policy": "every_frame",
            "resolution": [trace["width"], trace["height"]],
            "pair_order_balance": schedule_balance,
        },
        "schedule": [
            {"pair_index": index + 1, "run_order": order}
            for index, order in enumerate(schedule)
        ],
        "runs": run_results,
        "pairs": pair_results,
    }

    try:
        schedule_index = 0
        for pair_index, order in enumerate(schedule, 1):
            pair_dir = inputs.output / f"pair-{pair_index:03d}"
            pair_dir.mkdir()
            pair_runs: dict[str, Any] = {}
            for position, producer in enumerate(order, 1):
                schedule_index += 1
                run_dir = pair_dir / f"{position:02d}-{producer.replace('_', '-')}"
                run_dir.mkdir()
                capture_path = run_dir / "frame-0.png"
                command = make_command(inputs, producer, capture_path)
                write_json(run_dir / "command.json", {"argv": command})
                run_started = utc_now()
                try:
                    completed = subprocess.run(
                        command,
                        cwd=repo,
                        check=False,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        text=True,
                        timeout=RUN_TIMEOUT_SECONDS,
                    )
                except subprocess.TimeoutExpired as error:
                    stdout = error.stdout if isinstance(error.stdout, str) else ""
                    stderr = error.stderr if isinstance(error.stderr, str) else ""
                    (run_dir / "stdout.log").write_text(stdout, encoding="utf-8")
                    (run_dir / "stderr.log").write_text(stderr, encoding="utf-8")
                    raise ValidationError(f"pair {pair_index} {producer} timed out") from error
                (run_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
                (run_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
                require(completed.returncode == 0,
                        f"pair {pair_index} {producer} exited with {completed.returncode}")
                require(sha256_file(inputs.binary) == binary_sha, "release binary changed during the experiment")
                require(sha256_file(inputs.dataset_path) == dataset["sha256"], "dataset changed during the experiment")
                require(sha256_file(inputs.trace_path) == trace["file_sha256"], "trace changed during the experiment")
                require(git_receipt(repo) == build["git"], "git commit or dirty receipt changed during the experiment")
                validated = validate_run_log(
                    completed.stdout,
                    completed.stderr,
                    producer=producer,
                    dataset=dataset,
                    trace=trace,
                    warmup=inputs.warmup,
                    measured=inputs.measured,
                    mode=inputs.mode,
                    capture_path=capture_path,
                )
                capture_size = png_dimensions(capture_path)
                require(
                    capture_size == (trace["width"], trace["height"]),
                    f"pair {pair_index} {producer} capture size mismatch: {capture_size}",
                )
                capture_receipt = {
                    "path": str(capture_path.relative_to(inputs.output)),
                    "sha256": sha256_file(capture_path),
                    "width": capture_size[0],
                    "height": capture_size[1],
                }
                run_result = {
                    "schedule_index": schedule_index,
                    "pair_index": pair_index,
                    "position": position,
                    "producer": producer,
                    "started_at_utc": run_started,
                    "ended_at_utc": utc_now(),
                    "binary_sha256": binary_sha,
                    "stdout": str((run_dir / "stdout.log").relative_to(inputs.output)),
                    "stderr": str((run_dir / "stderr.log").relative_to(inputs.output)),
                    "capture": capture_receipt,
                    **validated,
                }
                write_json(run_dir / "run.json", run_result)
                run_results.append(run_result)
                pair_runs[producer] = run_result
            require(set(pair_runs) == set(PRODUCERS), f"pair {pair_index} is incomplete")
            require(
                pair_runs["post_sort"]["capture"]["sha256"]
                == pair_runs["preproject"]["capture"]["sha256"],
                f"pair {pair_index} PostSort/Preproject framebuffer PNGs are not byte-identical",
            )
            pair_result = {
                "pair_index": pair_index,
                "run_order": order,
                "capture_sha256": pair_runs["post_sort"]["capture"]["sha256"],
                "framebuffer_equivalence": "byte_identical_png",
                "runs": pair_runs,
            }
            pair_results.append(pair_result)
        capture_hashes = {pair["capture_sha256"] for pair in pair_results}
        require(
            len(capture_hashes) == 1,
            "the fixed trace-frame framebuffer changed between randomized pairs",
        )
        base_experiment.update(
            {
                "status": "ok",
                "ended_at_utc": utc_now(),
                "bootstrap_samples": DEFAULT_BOOTSTRAP_SAMPLES,
                "framebuffer_gate": "byte_identical_png_within_every_pair",
                "fixed_frame_capture_sha256": next(iter(capture_hashes)),
                "aggregate": aggregate_pairs(pair_results, inputs.seed),
            }
        )
        write_json(inputs.output / "experiment.json", base_experiment)
        return base_experiment
    except Exception as error:
        base_experiment.update(
            {
                "status": "failed",
                "ended_at_utc": utc_now(),
                "error": str(error),
            }
        )
        write_json(inputs.output / "experiment.json", base_experiment)
        raise


def parse_args(argv: Sequence[str] | None = None) -> Inputs:
    parser = argparse.ArgumentParser(
        description="Collect strict same-binary desktop PostSort/Preproject paired evidence"
    )
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--dataset", required=True, type=Path)
    parser.add_argument("--trace", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--pairs", type=int, default=5)
    parser.add_argument("--seed", type=int, default=0x4753504C4154)
    parser.add_argument("--warmup", type=int, default=20)
    parser.add_argument("--measured", type=int, default=80)
    parser.add_argument("--mode", choices=("isolated", "throughput"), default="isolated")
    args = parser.parse_args(argv)
    return Inputs(
        binary=args.binary.resolve(),
        dataset_path=args.dataset.resolve(),
        trace_path=args.trace.resolve(),
        output=args.output.resolve(),
        pairs=args.pairs,
        seed=args.seed,
        warmup=args.warmup,
        measured=args.measured,
        mode=args.mode,
    )


def main(argv: Sequence[str] | None = None) -> int:
    inputs = parse_args(argv)
    repo = Path(__file__).resolve().parents[2]
    try:
        experiment = collect(inputs, repo)
    except (OSError, subprocess.SubprocessError, ValidationError) as error:
        print(f"desktop producer A/B failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps({
        "status": experiment["status"],
        "output": str(inputs.output),
        "pairs": inputs.pairs,
        "binary_sha256": experiment["build"]["binary_sha256"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
