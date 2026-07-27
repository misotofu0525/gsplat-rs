#!/usr/bin/env python3
"""Collect the one-shot Q3 M4 Scalar/Neon native whole-plan matrix.

The two lanes are compile-time qualification builds of the same private Packed
Exact CPU renderer plan.  Product builds keep their ordinary AArch64 dispatch.
Every measured frame joins the existing CPU terminal ticket before this
collector publishes a finite Accepted, Rejected, or Deferred cell.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import random
import shutil
import subprocess
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence


REPO_ROOT = Path(__file__).resolve().parents[2]
BASE_PATH = REPO_ROOT / "tests/perf/collect-desktop-producer-ab.py"
MICRO_PATH = REPO_ROOT / "tests/perf/collect-q3-m4-simd.py"
MATRIX_PATH = REPO_ROOT / "tests/perf/full-quality-matrix-plan-v1.json"
TRACE_VALIDATOR_PATH = REPO_ROOT / "tests/perf/trace/validate_trace_v1.py"

SCHEMA = "gsplat-q3-renderer-simd/v1"
CELL = "Q3.M4.PackedCpuExact.ScalarVsNeon.WholePlan"
DECISIONS = {"Accepted", "Rejected", "Deferred"}
FORMAL_SIZE = (1920, 1080)
DEFAULT_PAIRS = 5
DEFAULT_WARMUP = 20
DEFAULT_MEASURED = 80
RUN_TIMEOUT_SECONDS = 30 * 60
CORRECTNESS_FIELDS = {
    "key",
    "source_id",
    "nan_bits",
    "boundary_bits",
    "fma_derived_key",
    "stable_tie",
}
PREFIXES = {
    "begin": "SURFACE_BENCHMARK_BEGIN ",
    "frame": "SURFACE_FRAME_RECEIPT ",
    "cpu": "SURFACE_CPU_MEASUREMENT ",
    "summary": "SURFACE_BENCHMARK_SUMMARY ",
}


def load_module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


BASE = load_module("collect_desktop_producer_ab_for_q3", BASE_PATH)
MICRO = load_module("collect_q3_m4_simd_for_renderer", MICRO_PATH)
ValidationError = BASE.ValidationError
require = BASE.require
sha256_file = BASE.sha256_file
utc_now = BASE.utc_now
parse_bool = BASE.parse_bool
parse_uint = BASE.parse_uint
parse_float = BASE.parse_float
only = BASE.only
require_keys = BASE.require_keys
assert_equal = BASE.assert_equal
validate_dimensions = BASE.validate_dimensions
git_receipt = BASE.git_receipt
validate_ignored_output = BASE.validate_ignored_output
distribution = BASE.distribution


@dataclass(frozen=True)
class Lane:
    name: str
    cargo_feature: str


LANES = (
    Lane("scalar", "qualification-q3-cpu-scalar"),
    Lane("neon", "qualification-q3-cpu-neon"),
)


@dataclass(frozen=True)
class Workload:
    dataset: dict[str, Any]
    dataset_path: Path
    trace: dict[str, Any]
    trace_path: Path


def require_object(value: Any, context: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{context} must be an object")
    return value


def require_string(value: Any, context: str) -> str:
    require(isinstance(value, str) and value, f"{context} must be a non-empty string")
    return value


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def host_receipt() -> dict[str, str]:
    return {
        "system": platform.system(),
        "machine": platform.machine(),
        "cpu_brand": MICRO.apple_cpu_brand(),
    }


def load_workload(repo: Path, matrix_path: Path) -> Workload:
    matrix_path = matrix_path.resolve()
    require(
        matrix_path == MATRIX_PATH.resolve(),
        "Q3 whole-plan qualification requires the committed full-quality matrix",
    )
    matrix = require_object(json.loads(matrix_path.read_text(encoding="utf-8")), "matrix")
    datasets = matrix.get("datasets")
    traces = matrix.get("traces")
    require(isinstance(datasets, list) and isinstance(traces, list), "matrix lacks datasets/traces")
    dataset_entry = next(
        (item for item in datasets if isinstance(item, dict) and item.get("id") == "truck-full"),
        None,
    )
    trace_entry = next(
        (
            item
            for item in traces
            if isinstance(item, dict)
            and item.get("id") == "candidate-truck-quality-2view-1920x1080-v1"
        ),
        None,
    )
    dataset_entry = require_object(dataset_entry, "matrix Truck dataset")
    trace_entry = require_object(trace_entry, "matrix Truck trace")
    require(dataset_entry.get("role") == "full_scene", "Q3 requires complete Truck")
    require(trace_entry.get("dataset_id") == "truck-full", "Truck trace dataset mismatch")
    require(
        (trace_entry.get("width"), trace_entry.get("height")) == FORMAL_SIZE,
        "Q3 requires the 1920x1080 Truck trace",
    )
    dataset_path = (repo / require_string(dataset_entry.get("local_path"), "dataset.local_path")).resolve()
    trace_path = (repo / require_string(trace_entry.get("local_path"), "trace.local_path")).resolve()
    require(dataset_path.is_file(), f"full Truck dataset is unavailable: {dataset_path}")
    require(trace_path.is_file(), f"Truck trace is unavailable: {trace_path}")
    dataset = BASE.read_ply_receipt(dataset_path)
    require(dataset["sha256"] == dataset_entry.get("sha256"), "Truck dataset SHA-256 mismatch")
    require(dataset["bytes"] == dataset_entry.get("bytes"), "Truck dataset byte count mismatch")
    require(dataset["splat_count"] == dataset_entry.get("splat_count"), "Truck splat count mismatch")
    require(dataset["sh_degree"] == dataset_entry.get("sh_degree"), "Truck SH degree mismatch")
    dataset["id"] = "truck-full"

    checked = subprocess.run(
        [sys.executable, str(TRACE_VALIDATOR_PATH), str(trace_path)],
        cwd=repo,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    detail = checked.stderr.strip() or checked.stdout.strip()
    require(checked.returncode == 0, f"Truck trace validator rejected input: {detail}")
    trace = BASE.read_trace_receipt(trace_path)
    require(trace["trace_id"] == trace_entry.get("id"), "Truck trace identity mismatch")
    require(trace["content_sha256"] == trace_entry.get("sha256"), "Truck trace content SHA mismatch")
    trace_value = require_object(json.loads(trace_path.read_text(encoding="utf-8")), "Truck trace")
    frames = trace_value.get("frames")
    require(isinstance(frames, list) and len(frames) >= 2, "Truck trace is not moving")
    trace["frame_indices"] = tuple(frame.get("frame_index") for frame in frames)
    trace["frame_timestamps_ns"] = tuple(frame.get("timestamp_ns") for frame in frames)
    require(
        all(isinstance(value, int) and value >= 0 for value in trace["frame_indices"]),
        "Truck trace frame indices are invalid",
    )
    require(
        all(isinstance(value, int) and value >= 0 for value in trace["frame_timestamps_ns"]),
        "Truck trace timestamps are invalid",
    )
    derivation = require_object(trace_value.get("derivation"), "Truck trace derivation")
    require(derivation.get("source_sha256") == dataset["sha256"], "Truck trace source SHA mismatch")
    require(derivation.get("source_splat_count") == dataset["splat_count"], "Truck trace source count mismatch")
    require(derivation.get("source_sh_degree") == dataset["sh_degree"], "Truck trace source SH mismatch")
    return Workload(dataset, dataset_path, trace, trace_path)


def validate_correctness(path: Path, commit: str, host: dict[str, str]) -> dict[str, Any]:
    require(path.is_file(), f"fixed correctness artifact is unavailable: {path}")
    receipt = require_object(json.loads(path.read_text(encoding="utf-8")), "correctness artifact")
    MICRO.validate_receipt(receipt)
    require(receipt.get("decision") in {"Deferred", "Rejected"}, "correctness cell is not terminal")
    require(receipt.get("host") == host, "correctness artifact host differs from whole-plan host")
    build = require_object(receipt.get("build"), "correctness build receipt")
    require(build.get("commit") == commit, "correctness artifact commit differs from whole-plan commit")
    require(build.get("dirty") is False, "correctness artifact was not collected from a clean tree")
    correctness = require_object(receipt.get("correctness"), "correctness receipt")
    require(set(correctness) == CORRECTNESS_FIELDS, "correctness field set drifted")
    if not all(correctness.values()):
        require(receipt.get("decision") == "Rejected", "failed correctness must be Rejected")
        return receipt
    require(receipt.get("decision") == "Deferred", "passing micro parity must await whole-plan terminal")
    require(receipt.get("reason") == "microbenchmark_only_whole_plan_terminal_pending", "unexpected correctness reason")
    require(receipt.get("input_id") == "q3-packed-exact-lcg-v1-200003", "fixed correctness input drifted")
    require(receipt.get("input_len") == 200_003, "fixed correctness input length drifted")
    timing = require_object(receipt.get("timing"), "correctness timing")
    require(timing.get("interleaved") is True, "correctness probe was not interleaved")
    require(isinstance(timing.get("scalar_median_ns"), int), "Scalar micro timing unavailable")
    require(isinstance(timing.get("neon_median_ns"), int), "Neon micro timing unavailable")
    return receipt


def schedule_pairs(pairs: int, seed: int) -> list[tuple[str, str]]:
    require(pairs == DEFAULT_PAIRS, f"formal Q3 matrix requires exactly {DEFAULT_PAIRS} pairs")
    forward = ("scalar", "neon")
    reverse = tuple(reversed(forward))
    schedule = [forward] * (pairs // 2) + [reverse] * (pairs // 2)
    if pairs % 2:
        bit = hashlib.sha256(f"gsplat-q3-renderer-simd/{seed}".encode("ascii")).digest()[0] & 1
        schedule.append(forward if bit == 0 else reverse)
    random.Random(seed).shuffle(schedule)
    require(abs(schedule.count(forward) - schedule.count(reverse)) <= 1, "Q3 schedule is not counterbalanced")
    return schedule


def parse_log(stdout: str, stderr: str = "") -> dict[str, list[dict[str, str]]]:
    records = {name: [] for name in PREFIXES}
    for stream_name, stream in (("stdout", stdout), ("stderr", stderr)):
        for line_number, line in enumerate(stream.splitlines(), 1):
            for name, prefix in PREFIXES.items():
                if line.startswith(prefix):
                    records[name].append(
                        BASE.parse_key_value_payload(
                            line[len(prefix) :], f"{stream_name}:{line_number}:{prefix.strip()}"
                        )
                    )
                    break
    return records


def close_enough(left: float, right: float) -> bool:
    return math.isclose(left, right, rel_tol=1e-5, abs_tol=1e-4)


def validate_run_log(
    stdout: str,
    stderr: str,
    *,
    lane: str,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    warmup: int,
    measured: int,
) -> dict[str, Any]:
    require(lane in {item.name for item in LANES}, f"unknown Q3 lane {lane}")
    records = parse_log(stdout, stderr)
    begin = only(records["begin"], "SURFACE_BENCHMARK_BEGIN")
    summary = only(records["summary"], "SURFACE_BENCHMARK_SUMMARY")
    expected_total = warmup + measured
    common = {
        "trace_id": trace["trace_id"],
        "trace_sha256": trace["content_sha256"],
        "benchmark_mode": "isolated",
        "sort_policy": "every_frame",
        "requested_backend": "cpu",
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "gpu_order_producer": "product-default",
        "producer_measurement_enabled": "false",
        "source_count": str(dataset["splat_count"]),
        "resident_count": str(dataset["splat_count"]),
        "sh_degree": "3",
    }
    assert_equal(begin, common, "begin")
    validate_dimensions(begin, trace["width"], trace["height"], "begin")
    require(begin.get("sort_interval") == "1", "begin.sort_interval must equal one")
    require(parse_uint(begin.get("trace_frames", ""), "begin.trace_frames") == expected_total, "trace frame count mismatch")

    scheduled = [frame for frame in records["frame"] if frame.get("phase") != "drain"]
    require(len(scheduled) == expected_total, f"expected {expected_total} scheduled frames, got {len(scheduled)}")
    issued: dict[int, tuple[dict[str, str], bool]] = {}
    measured_frames: list[dict[str, Any]] = []
    for index, frame in enumerate(scheduled):
        context = f"frame[{index}]"
        phase = "warmup" if index < warmup else "measure"
        require(frame.get("phase") == phase, f"{context}.phase violates the fixed schedule")
        require(parse_uint(frame.get("playback_index", ""), f"{context}.playback_index") == index, f"{context}.playback_index mismatch")
        phase_index = index if phase == "warmup" else index - warmup
        trace_slot = phase_index % len(trace["frame_indices"])
        require(parse_uint(frame.get("trace_frame", ""), f"{context}.trace_frame") == trace["frame_indices"][trace_slot], f"{context}.trace_frame mismatch")
        require(parse_uint(frame.get("trace_timestamp_ns", ""), f"{context}.trace_timestamp_ns") == trace["frame_timestamps_ns"][trace_slot], f"{context}.trace_timestamp mismatch")
        assert_equal(
            frame,
            {
                "sort_policy": "every_frame",
                "requested_backend": "cpu",
                "actual_backend": "cpu",
                "raster_execution_plan": "projected_quads_exact",
                "frame_presented": "true",
                "gpu_order_preparation_pending": "false",
                "sort_refreshed": "true",
                "order_uploaded": "true",
                "gpu_sort_fallback": "false",
                "source_count": str(dataset["splat_count"]),
                "resident_count": str(dataset["splat_count"]),
                "measurement_backend": "cpu",
                "measurement_unsampled_reason": "none",
                "gpu_ticket_submitted": "none",
            },
            context,
        )
        validate_dimensions(frame, trace["width"], trace["height"], context)
        ticket = parse_uint(frame.get("measurement_ticket_submitted", ""), f"{context}.ticket", positive=True)
        require(ticket not in issued, f"CPU ticket {ticket} was issued twice")
        is_measured = phase == "measure"
        issued[ticket] = (frame, is_measured)
        if is_measured:
            require(parse_uint(frame.get("measured_sample", ""), f"{context}.measured_sample") == len(measured_frames), f"{context}.measured_sample mismatch")
            measured_frames.append({"ticket": ticket})
        else:
            require(frame.get("measured_sample") == "none", f"{context}.measured_sample must be none")

    terminal_by_ticket: dict[int, dict[str, str]] = {}
    measured_by_ticket: dict[int, dict[str, Any]] = {}
    for index, terminal in enumerate(records["cpu"]):
        context = f"cpu_terminal[{index}]"
        require_keys(
            terminal,
            (
                "ticket",
                "camera_revision",
                "measured",
                "cpu_preprocess_ms",
                "cpu_sort_ms",
                "frame_completion_ms",
                "visible_count",
                "contributor_count",
                "drawn_count",
                "exact_contributor_compaction",
            ),
            context,
        )
        ticket = parse_uint(terminal["ticket"], f"{context}.ticket", positive=True)
        require(ticket in issued, f"unknown CPU terminal ticket {ticket}")
        require(ticket not in terminal_by_ticket, f"CPU ticket {ticket} has more than one terminal")
        frame, expected_measured = issued[ticket]
        require(parse_uint(terminal["camera_revision"], f"{context}.camera_revision") == parse_uint(frame["camera_revision"], f"{context}.frame_camera_revision"), f"CPU ticket {ticket} camera revision mismatch")
        terminal_measured = parse_bool(terminal["measured"], f"{context}.measured")
        require(terminal_measured == expected_measured, f"CPU ticket {ticket} measured flag mismatch")
        preprocess = parse_float(terminal["cpu_preprocess_ms"], f"{context}.cpu_preprocess_ms")
        sort = parse_float(terminal["cpu_sort_ms"], f"{context}.cpu_sort_ms")
        completion = parse_float(terminal["frame_completion_ms"], f"{context}.frame_completion_ms")
        require(close_enough(preprocess, parse_float(frame["cpu_preprocess_ms"], f"{context}.frame_preprocess")), f"CPU ticket {ticket} preprocess join mismatch")
        require(close_enough(sort, parse_float(frame["cpu_sort_ms"], f"{context}.frame_sort")), f"CPU ticket {ticket} sort join mismatch")
        visible = parse_uint(terminal["visible_count"], f"{context}.visible_count")
        contributor = parse_uint(terminal["contributor_count"], f"{context}.contributor_count")
        drawn = parse_uint(terminal["drawn_count"], f"{context}.drawn_count")
        compacted = parse_bool(terminal["exact_contributor_compaction"], f"{context}.exact_contributor_compaction")
        require(0 <= contributor <= visible <= dataset["splat_count"], f"CPU ticket {ticket} violates C<=V<=S")
        require(drawn == (contributor if compacted else visible), f"CPU ticket {ticket} violates the issued draw contract")
        require(visible == parse_uint(frame["visible_count"], f"{context}.frame_visible"), f"CPU ticket {ticket} visible count mismatch")
        require(drawn == parse_uint(frame["drawn_count"], f"{context}.frame_drawn"), f"CPU ticket {ticket} drawn count mismatch")
        terminal_by_ticket[ticket] = terminal
        if terminal_measured:
            measured_by_ticket[ticket] = {
                "frame_index": len(measured_by_ticket),
                "ticket": ticket,
                "trace_frame": parse_uint(frame["trace_frame"], f"{context}.trace_frame"),
                "preprocess_ms": preprocess,
                "sort_ms": sort,
                "order_uploaded": True,
                "render_submit_ms": parse_float(frame["cpu_render_submit_ms"], f"{context}.render_submit_ms"),
                "frame_wall_ms": parse_float(frame["frame_wall_ms"], f"{context}.frame_wall_ms"),
                "frame_completion_ms": completion,
                "visible": visible,
                "contributor": contributor,
                "drawn": drawn,
                "exact_contributor_compaction": compacted,
                "raster_execution_plan": "projected_quads_exact",
                "frame_presented": True,
            }
    require(set(terminal_by_ticket) == set(issued), "CPU tickets do not have exactly one terminal receipt")
    frames = [measured_by_ticket[item["ticket"]] for item in measured_frames]
    require(len(frames) == measured, "measured CPU terminal count mismatch")

    assert_equal(summary, {**common, "status": "ok", "final_actual_backend": "cpu"}, "summary")
    validate_dimensions(summary, trace["width"], trace["height"], "summary")
    assert_equal(
        summary,
        {
            "trace_frames": str(expected_total),
            "presented_frames": str(expected_total),
            "measured_frames": str(measured),
            "measured_cpu_frames": str(measured),
            "measured_gpu_frames": "0",
            "sort_refreshes": str(expected_total),
            "gpu_fallback_frames": "0",
            "gpu_refreshes_without_ticket": "0",
            "cpu_requests_without_ticket": "0",
            "surface_unavailable_measurements": "0",
            "terminal_order_tickets": str(expected_total),
            "terminal_producer_tickets": "0",
            "outstanding_cpu_tickets": "0",
            "outstanding_gpu_tickets": "0",
            "outstanding_producer_tickets": "0",
        },
        "summary",
    )
    for field, values in (
        ("mean_cpu_preprocess_ms", [frame["preprocess_ms"] for frame in frames]),
        ("mean_cpu_sort_ms", [frame["sort_ms"] for frame in frames]),
        ("mean_cpu_completion_ms", [frame["frame_completion_ms"] for frame in frames]),
        ("mean_frame_wall_ms", [frame["frame_wall_ms"] for frame in frames]),
    ):
        require(close_enough(parse_float(summary[field], f"summary.{field}"), sum(values) / len(values)), f"summary.{field} disagrees with measured receipts")
    return {
        "lane": lane,
        "scheduled_frames": expected_total,
        "warmup_frames": warmup,
        "measured_frames": measured,
        "terminal_ticket_count": len(terminal_by_ticket),
        "frames": frames,
        "distributions": {
            "preprocess_ms": distribution([frame["preprocess_ms"] for frame in frames]),
            "sort_ms": distribution([frame["sort_ms"] for frame in frames]),
            "render_submit_ms": distribution([frame["render_submit_ms"] for frame in frames]),
            "frame_wall_ms": distribution([frame["frame_wall_ms"] for frame in frames]),
            "frame_completion_ms": distribution([frame["frame_completion_ms"] for frame in frames]),
        },
    }


def cargo_executable(stdout: str) -> Path:
    executables: set[Path] = set()
    for line in stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        target = message.get("target")
        executable = message.get("executable")
        if (
            message.get("reason") == "compiler-artifact"
            and isinstance(target, dict)
            and target.get("name") == "desktop-example"
            and "bin" in target.get("kind", [])
            and isinstance(executable, str)
        ):
            executables.add(Path(executable).resolve())
    require(len(executables) == 1, f"expected one desktop executable, got {len(executables)}")
    return next(iter(executables))


def build_binaries(repo: Path, stage: Path, expected_git: dict[str, Any]) -> tuple[dict[str, Path], dict[str, Any]]:
    target_dir = stage / "cargo-target"
    target_dir.mkdir()
    build_root = stage / "build"
    build_root.mkdir()
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_dir.resolve())
    environment.setdefault("CARGO_BUILD_JOBS", "1")
    binaries: dict[str, Path] = {}
    receipts: dict[str, Any] = {}
    for lane in LANES:
        lane_dir = build_root / lane.name
        lane_dir.mkdir()
        command = [
            "cargo",
            "build",
            "--locked",
            "--release",
            "-p",
            "desktop-example",
            "--features",
            lane.cargo_feature,
            "--message-format=json-render-diagnostics",
        ]
        write_json(lane_dir / "command.json", {"argv": command, "environment": {"CARGO_TARGET_DIR": environment["CARGO_TARGET_DIR"], "CARGO_BUILD_JOBS": environment["CARGO_BUILD_JOBS"]}})
        completed = subprocess.run(command, cwd=repo, env=environment, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (lane_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
        (lane_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
        require(completed.returncode == 0, f"{lane.name} release build exited with {completed.returncode}")
        built = cargo_executable(completed.stdout)
        retained = lane_dir / "desktop-example-bin"
        shutil.copy2(built, retained)
        retained.chmod(retained.stat().st_mode | 0o100)
        binaries[lane.name] = retained
        receipts[lane.name] = {
            "cargo_feature": lane.cargo_feature,
            "binary_sha256": sha256_file(retained),
            "command": f"build/{lane.name}/command.json",
            "stdout": f"build/{lane.name}/stdout.log",
            "stderr": f"build/{lane.name}/stderr.log",
        }
        require(git_receipt(repo) == expected_git, "git receipt changed during Q3 build")
    require(receipts["scalar"]["binary_sha256"] != receipts["neon"]["binary_sha256"], "Scalar and Neon builds produced the same binary hash")
    return binaries, receipts


def make_command(binary: Path, workload: Workload, warmup: int, measured: int) -> list[str]:
    return [
        str(binary),
        str(workload.dataset_path),
        "--geometry-path",
        "packed",
        "--interactive",
        "--camera-trace",
        str(workload.trace_path),
        "--camera-sequence",
        "--camera-warmup-frames",
        str(warmup),
        "--camera-measured-frames",
        str(measured),
        "--camera-loops",
        "1",
        "--surface-benchmark-mode",
        "isolated",
        "--surface-sort-policy",
        "every-frame",
        "--order-backend",
        "cpu",
    ]


def lane_by_name(name: str) -> Lane:
    return next(lane for lane in LANES if lane.name == name)


def paired_metric(pair_results: Sequence[dict[str, Any]], metric: str) -> dict[str, Any]:
    deltas = [
        pair["runs"]["neon"]["distributions"][metric]["mean"]
        - pair["runs"]["scalar"]["distributions"][metric]["mean"]
        for pair in pair_results
    ]
    return {
        "neon_minus_scalar_mean_ms_by_pair": deltas,
        "median_paired_difference_ms": sorted(deltas)[len(deltas) // 2],
        "neon_faster_pair_count": sum(delta < 0.0 for delta in deltas),
        "scalar_faster_or_tied_pair_count": sum(delta >= 0.0 for delta in deltas),
    }


def terminal_decision(pair_results: Sequence[dict[str, Any]]) -> tuple[str, str]:
    require(len(pair_results) == DEFAULT_PAIRS, "formal Q3 matrix is incomplete")
    completion = paired_metric(pair_results, "frame_completion_ms")
    if completion["neon_faster_pair_count"] == DEFAULT_PAIRS:
        return "Accepted", "stable_neon_whole_plan_queue_completion_benefit"
    return "Rejected", "no_stable_neon_whole_plan_benefit"


def remove_private_builds(stage: Path) -> None:
    target_dir = stage / "cargo-target"
    if target_dir.exists():
        require(target_dir.is_dir() and not target_dir.is_symlink(), "private Cargo target is unsafe")
        shutil.rmtree(target_dir)
    for binary in (stage / "build").glob("*/desktop-example-bin") if (stage / "build").is_dir() else ():
        require(binary.is_file() and not binary.is_symlink(), f"private binary is unsafe: {binary}")
        binary.unlink()


def publish(stage: Path, output: Path, result: dict[str, Any]) -> None:
    require(result.get("decision") in DECISIONS, "Q3 result is not a finite terminal cell")
    result["ended_at_utc"] = utc_now()
    write_json(stage / "experiment.json", result)
    require(not (stage / "cargo-target").exists(), "private Cargo target must not be published")
    retained = [path.relative_to(stage) for path in stage.rglob("desktop-example-bin")]
    require(not retained, f"private binaries remain: {retained}")
    require(not output.exists(), f"output appeared during collection: {output}")
    os.rename(stage, output)


def collect(args: argparse.Namespace, repo: Path = REPO_ROOT) -> dict[str, Any]:
    require(args.pairs == DEFAULT_PAIRS, f"formal Q3 matrix requires pairs={DEFAULT_PAIRS}")
    require((args.warmup, args.measured) == (DEFAULT_WARMUP, DEFAULT_MEASURED), "formal Q3 matrix requires warmup=20 and measured=80")
    output = args.output.resolve()
    require(not output.exists(), f"Q3 output already exists: {output}")
    validate_ignored_output(repo, output)
    initial_git = git_receipt(repo)
    require(not initial_git["dirty"], "formal Q3 matrix requires a clean exact commit")
    host = host_receipt()
    output.parent.mkdir(parents=True, exist_ok=True)
    stage = output.parent / f".{output.name}.stage-{os.getpid()}-{uuid.uuid4().hex[:10]}"
    stage.mkdir()
    result: dict[str, Any] = {
        "schema": SCHEMA,
        "cell": CELL,
        "decision": "Deferred",
        "reason": "whole_plan_owner_protocol_incomplete",
        "started_at_utc": utc_now(),
        "build": initial_git,
        "host": host,
        "config": {
            "pairs": args.pairs,
            "seed": args.seed,
            "warmup_frames": args.warmup,
            "measured_frames": args.measured,
            "schedule": "counterbalanced_seeded_interleaved",
            "geometry_path": "packed_atlas",
            "raster_execution_plan": "projected_quads_exact",
            "order_backend": "cpu",
            "sort_policy": "every_frame",
            "resolution": list(FORMAL_SIZE),
            "decision_rule": "Accepted iff Neon queue-completion mean is lower in all five pairs; otherwise Rejected",
        },
        "runs": [],
        "pairs": [],
    }
    try:
        deferral = MICRO.host_deferral(host["system"], host["machine"], host["cpu_brand"])
        require(deferral is None, deferral or "unknown host deferral")
        correctness = validate_correctness(args.correctness.resolve(), initial_git["commit"], host)
        result["correctness"] = {
            "path": str(args.correctness.resolve()),
            "sha256": sha256_file(args.correctness.resolve()),
            "receipt": correctness,
        }
        if correctness["decision"] == "Rejected":
            result.update({"decision": "Rejected", "reason": "fixed_scalar_neon_correctness_failed"})
            remove_private_builds(stage)
            publish(stage, output, result)
            return result

        workload = load_workload(repo, args.matrix.resolve())
        result["workload"] = {
            "dataset": workload.dataset,
            "trace": workload.trace,
        }
        schedule = schedule_pairs(args.pairs, args.seed)
        result["schedule"] = [
            {"pair_index": index + 1, "order": list(order)}
            for index, order in enumerate(schedule)
        ]
        binaries, build_receipts = build_binaries(repo, stage, initial_git)
        result["lane_builds"] = build_receipts
        run_index = 0
        for pair_index, order in enumerate(schedule, 1):
            pair_runs: dict[str, Any] = {}
            for position, lane_name in enumerate(order, 1):
                run_index += 1
                lane = lane_by_name(lane_name)
                run_dir = stage / f"pair-{pair_index:03d}" / f"{position:02d}-{lane_name}"
                run_dir.mkdir(parents=True)
                command = make_command(binaries[lane_name], workload, args.warmup, args.measured)
                write_json(run_dir / "command.json", {"argv": command, "cwd": str(run_dir.relative_to(stage))})
                started = utc_now()
                completed = subprocess.run(command, cwd=run_dir, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=RUN_TIMEOUT_SECONDS)
                ended = utc_now()
                (run_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
                (run_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
                require(completed.returncode == 0, f"pair {pair_index} {lane_name} exited with {completed.returncode}")
                require(sha256_file(binaries[lane_name]) == build_receipts[lane_name]["binary_sha256"], f"{lane_name} binary changed during collection")
                require(sha256_file(workload.dataset_path) == workload.dataset["sha256"], "Truck dataset changed during collection")
                require(sha256_file(workload.trace_path) == workload.trace["file_sha256"], "Truck trace changed during collection")
                require(git_receipt(repo) == initial_git, "git receipt changed during Q3 matrix")
                validated = validate_run_log(completed.stdout, completed.stderr, lane=lane_name, dataset=workload.dataset, trace=workload.trace, warmup=args.warmup, measured=args.measured)
                frames_path = run_dir / "frames.jsonl"
                frames_path.write_text("".join(json.dumps(frame, sort_keys=True) + "\n" for frame in validated["frames"]), encoding="utf-8")
                run = {
                    "run_index": run_index,
                    "pair_index": pair_index,
                    "position": position,
                    "lane": lane_name,
                    "cargo_feature": lane.cargo_feature,
                    "binary_sha256": build_receipts[lane_name]["binary_sha256"],
                    "started_at_utc": started,
                    "ended_at_utc": ended,
                    "command": str((run_dir / "command.json").relative_to(stage)),
                    "stdout": str((run_dir / "stdout.log").relative_to(stage)),
                    "stderr": str((run_dir / "stderr.log").relative_to(stage)),
                    "frames": str(frames_path.relative_to(stage)),
                    **{key: value for key, value in validated.items() if key != "frames"},
                }
                write_json(run_dir / "run.json", run)
                result["runs"].append(run)
                pair_runs[lane_name] = run
            require(set(pair_runs) == {"scalar", "neon"}, f"pair {pair_index} is incomplete")
            result["pairs"].append({"pair_index": pair_index, "order": list(order), "runs": pair_runs})

        aggregate = {
            metric: paired_metric(result["pairs"], metric)
            for metric in (
                "preprocess_ms",
                "sort_ms",
                "render_submit_ms",
                "frame_wall_ms",
                "frame_completion_ms",
            )
        }
        decision, reason = terminal_decision(result["pairs"])
        result.update({"decision": decision, "reason": reason, "aggregate": aggregate})
        remove_private_builds(stage)
        publish(stage, output, result)
        return result
    except Exception as error:
        result.update({"decision": "Deferred", "reason": "whole_plan_owner_protocol_incomplete", "error": str(error)})
        remove_private_builds(stage)
        publish(stage, output, result)
        return result


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect one Q3 M4 Scalar/Neon whole-plan matrix")
    parser.add_argument("--correctness", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--matrix", type=Path, default=MATRIX_PATH)
    parser.add_argument("--pairs", type=int, default=DEFAULT_PAIRS)
    parser.add_argument("--seed", type=int, default=0x51334D3453494D44)
    parser.add_argument("--warmup", type=int, default=DEFAULT_WARMUP)
    parser.add_argument("--measured", type=int, default=DEFAULT_MEASURED)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        result = collect(args)
    except (OSError, subprocess.SubprocessError, ValidationError, ValueError) as error:
        print(f"Q3 renderer SIMD collection failed before terminal publication: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"cell": result["cell"], "decision": result["decision"], "reason": result["reason"], "output": str(args.output)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
