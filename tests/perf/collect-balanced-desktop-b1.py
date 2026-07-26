#!/usr/bin/env python3
"""Collect one fail-closed desktop B1 ExactFull32/CandidateStable24 suite.

The desktop host is invoked exactly twice.  Each invocation owns one continuous
post-warmup 0 -> 1 -> 0 capture sequence; this collector never joins captures
from separate host processes into one lane.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import re
import shutil
import subprocess
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Sequence


REPO_ROOT = Path(__file__).resolve().parents[2]
M2B_COLLECTOR_PATH = REPO_ROOT / "tests/perf/collect-desktop-surface-evidence.py"
BENCHMARK_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
BALANCED_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-balanced-image-gate.py"

SCHEMA = "gsplat-benchmark/v1"
SUITE_SCHEMA = "gsplat-balanced-image-gate/v1"
COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1"
FORMAL_SIZE = (1920, 1080)
CANONICAL_WARMUP = 20
CANONICAL_MEASURED = 80
CAPTURE_TRACE_FRAMES = (0, 1, 0)
CAPTURE_TRACE_TIMESTAMPS_NS = (0, 16_666_667, 0)
RUN_TIMEOUT_SECONDS = 30 * 60
PLAN_REQUESTED = "gpu_post_sort"
PLAN_CLI = "gpu-post-sort"
PLAN_CURRENT_STATS = "gpu_post_sort"
PLAN_CAPTURE_RECEIPT = "GpuPostSort"
COUNT_SEMANTICS_HOST = "indirect_draw_equals_visible"

PREFIXES = {
    "begin": "SURFACE_EXACT_EVIDENCE_BEGIN ",
    "terminal": "SURFACE_DIAGNOSTIC_MULTI_CAPTURE_TERMINAL ",
    "summary": "SURFACE_EXACT_EVIDENCE_SUMMARY ",
}


@dataclass(frozen=True)
class Lane:
    name: str
    profile: str
    cargo_feature: str


LANES = (
    Lane(
        name="exact",
        profile="ExactFull32",
        cargo_feature="diagnostic-surface-capture-receipt",
    ),
    Lane(
        name="candidate",
        profile="CandidateStable24",
        cargo_feature="diagnostic-surface-depth-key-candidate24",
    ),
)


@dataclass(frozen=True)
class Capture:
    capture_index: int
    trace_frame_index: int
    trace_timestamp_ns: int
    elapsed_ns: int
    call_ms: float
    frame_wall_ms: float
    path: Path
    png_sha256: str
    rgba8_sha256: str
    counts: dict[str, int]
    presentation: dict[str, Any]
    depth_precision: dict[str, Any]
    raster_generation: int
    encode_attempt: int


@dataclass(frozen=True)
class LaneSession:
    lane: Lane
    captures: tuple[Capture, ...]
    adapter: dict[str, str]
    started_at_utc: str
    ended_at_utc: str


def load_module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


M2B = load_module("collect_desktop_surface_evidence_shared", M2B_COLLECTOR_PATH)
ValidationError = M2B.ValidationError
require = M2B.require
utc_now = M2B.utc_now
sha256_file = M2B.sha256_file
parse_uint = M2B.parse_uint
parse_float = M2B.parse_float
parse_payload = M2B.parse_payload
only = M2B.only
require_keys = M2B.require_keys
assert_fields = M2B.assert_fields
git_receipt = M2B.git_receipt
write_json = M2B.write_json
package_version = M2B.package_version
host_device_name = M2B.host_device_name
validate_ignored_output = M2B.validate_ignored_output
distribution = M2B.distribution
BALANCED = load_module("validate_balanced_image_gate_shared", BALANCED_VALIDATOR_PATH)


def parse_session_records(
    stdout: str, stderr: str
) -> dict[str, list[dict[str, str]]]:
    records = {name: [] for name in PREFIXES}
    for stream_name, stream in (("stdout", stdout), ("stderr", stderr)):
        for line_number, line in enumerate(stream.splitlines(), 1):
            for name, prefix in PREFIXES.items():
                if line.startswith(prefix):
                    record = parse_payload(
                        line[len(prefix) :], f"{stream_name}:{line_number}:{name}"
                    )
                    record["__stream"] = stream_name
                    records[name].append(record)
                    break
    return records


def expected_capture_path(capture_index: int, trace_frame_index: int) -> PurePosixPath:
    return PurePosixPath(
        "capture.captures",
        f"capture-{capture_index}-trace-{trace_frame_index}.png",
    )


def resolve_capture_file(root: Path, value: str, context: str) -> Path:
    relative = PurePosixPath(value)
    require(
        not relative.is_absolute() and relative.parts and ".." not in relative.parts,
        f"{context} must stay inside the lane capture directory",
    )
    path = root.joinpath(*relative.parts)
    require(not path.is_symlink(), f"{context} must not be a symlink")
    resolved_root = root.resolve()
    resolved = path.resolve()
    try:
        resolved.relative_to(resolved_root)
    except ValueError as error:
        raise ValidationError(f"{context} escapes the lane capture directory") from error
    require(resolved.is_file(), f"{context} does not exist: {value}")
    return resolved


def validate_begin(
    record: dict[str, str],
    *,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    warmup: int,
    measured: int,
) -> dict[str, str]:
    assert_fields(
        record,
        {
            "trace_id": trace["trace_id"],
            "trace_sha256": trace["content_sha256"],
            "exact_plan_requested": PLAN_REQUESTED,
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
            "requested_width": str(FORMAL_SIZE[0]),
            "requested_height": str(FORMAL_SIZE[1]),
            "surface_width": str(FORMAL_SIZE[0]),
            "surface_height": str(FORMAL_SIZE[1]),
            "internal_render_width": str(FORMAL_SIZE[0]),
            "internal_render_height": str(FORMAL_SIZE[1]),
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": "true",
            "trace_frames": str(warmup + measured),
            "adapter_backend": "metal",
        },
        "begin",
    )
    require(record.get("__stream") == "stdout", "begin record must be emitted on stdout")
    require(record.get("adapter_name", "") not in ("", "unavailable"), "Metal adapter name unavailable")
    require(record.get("adapter_device_type", "") not in ("", "other"), "adapter device type unavailable")
    require_keys(record, ("adapter_driver", "adapter_driver_info"), "begin")
    require(bool(record["adapter_driver"]), "adapter driver field is not explicit")
    require(bool(record["adapter_driver_info"]), "adapter driver-info field is not explicit")
    return {
        "backend": record["adapter_backend"],
        "name": record["adapter_name"],
        "device_type": record["adapter_device_type"],
        "driver": record["adapter_driver"],
        "driver_info": record["adapter_driver_info"],
    }


def validate_terminal_records(
    records: Sequence[dict[str, str]],
    *,
    lane: Lane,
    source_count: int,
) -> list[dict[str, Any]]:
    require(len(records) == 3, f"{lane.name} requires exactly three terminal records, got {len(records)}")
    normalized: list[dict[str, Any]] = []
    previous_elapsed = -1
    previous_ticket = 0
    previous_presentation = 0
    for expected_index, (record, expected_trace, expected_timestamp) in enumerate(
        zip(records, CAPTURE_TRACE_FRAMES, CAPTURE_TRACE_TIMESTAMPS_NS, strict=True)
    ):
        context = f"{lane.name}.terminal[{expected_index}]"
        require(record.get("__stream", "stdout") == "stdout", f"{context} must be emitted on stdout")
        assert_fields(
            record,
            {
                "status": "ok",
                "capture_index": str(expected_index),
                "path": expected_capture_path(expected_index, expected_trace).as_posix(),
                "trace_frame": str(expected_trace),
                "trace_timestamp_ns": str(expected_timestamp),
                "current_stats_plan_id": PLAN_CURRENT_STATS,
                "count_semantics": COUNT_SEMANTICS_HOST,
                "source_count": str(source_count),
                "exact_contributor_compaction": "false",
                "capture_receipt_profile": lane.profile,
                "capture_receipt_plan_id": PLAN_CAPTURE_RECEIPT,
                "capture_receipt_width": str(FORMAL_SIZE[0]),
                "capture_receipt_height": str(FORMAL_SIZE[1]),
                "frame_presented": "true",
                "terminal_receipt": "ready",
            },
            context,
        )
        elapsed = parse_uint(record.get("elapsed_ns", ""), f"{context}.elapsed_ns", positive=True)
        require(elapsed > previous_elapsed, f"{context}.elapsed_ns must be strictly increasing")
        previous_elapsed = elapsed
        call_ms = parse_float(record.get("call_ms", ""), f"{context}.call_ms")
        frame_wall_ms = parse_float(record.get("frame_wall_ms", ""), f"{context}.frame_wall_ms")

        ticket = parse_uint(
            record.get("current_stats_ticket", ""),
            f"{context}.current_stats_ticket",
            positive=True,
        )
        presentation = parse_uint(
            record.get("current_stats_presentation_sequence", ""),
            f"{context}.current_stats_presentation_sequence",
            positive=True,
        )
        require(ticket > previous_ticket, f"{context} current-stats ticket is not strictly increasing")
        require(presentation > previous_presentation, f"{context} presentation sequence is not strictly increasing")
        previous_ticket = ticket
        previous_presentation = presentation

        current = {
            "scene_generation": parse_uint(record.get("current_stats_scene_generation", ""), f"{context}.scene", positive=True),
            "camera_revision": parse_uint(record.get("current_stats_camera_revision", ""), f"{context}.camera", positive=True),
            "viewport_generation": parse_uint(
                record.get("current_stats_viewport_generation", ""),
                f"{context}.viewport",
            ),
            "contract_generation": parse_uint(record.get("current_stats_contract_generation", ""), f"{context}.contract", positive=True),
            "plan_set_generation": parse_uint(record.get("current_stats_plan_set_generation", ""), f"{context}.plan_set", positive=True),
            "order_generation": parse_uint(record.get("current_stats_order_generation", ""), f"{context}.order", positive=True),
            "presentation_sequence": presentation,
        }
        captured = {
            "scene_generation": parse_uint(record.get("capture_receipt_scene_generation", ""), f"{context}.capture_scene", positive=True),
            "camera_revision": parse_uint(record.get("capture_receipt_camera_revision", ""), f"{context}.capture_camera", positive=True),
            "viewport_generation": parse_uint(
                record.get("capture_receipt_viewport_generation", ""),
                f"{context}.capture_viewport",
            ),
            "contract_generation": parse_uint(record.get("capture_receipt_contract_generation", ""), f"{context}.capture_contract", positive=True),
            "plan_set_generation": parse_uint(record.get("capture_receipt_plan_set_generation", ""), f"{context}.capture_plan_set", positive=True),
            "order_generation": parse_uint(record.get("capture_receipt_order_generation", ""), f"{context}.capture_order", positive=True),
            "presentation_sequence": parse_uint(record.get("capture_receipt_presentation_sequence", ""), f"{context}.capture_presentation", positive=True),
        }
        require(current == captured, f"{context} current-stats/capture receipt identity mismatch")

        visible = parse_uint(record.get("visible_count", ""), f"{context}.visible_count")
        contributor = parse_uint(record.get("contributor_count", ""), f"{context}.contributor_count")
        drawn = parse_uint(record.get("drawn_count", ""), f"{context}.drawn_count")
        require(contributor <= visible <= source_count, f"{context} violates C<=V<=S")
        require(drawn == visible, f"{context} GPU PostSort must prove D=V")
        rgba8_sha256 = record.get("capture_receipt_rgba8_sha256", "")
        require(re.fullmatch(r"[0-9a-f]{64}", rgba8_sha256) is not None, f"{context} has invalid RGBA8 SHA-256")

        normalized.append(
            {
                "capture_index": expected_index,
                "trace_frame_index": expected_trace,
                "trace_timestamp_ns": expected_timestamp,
                "elapsed_ns": elapsed,
                "call_ms": call_ms,
                "frame_wall_ms": frame_wall_ms,
                "path": record["path"],
                "rgba8_sha256": rgba8_sha256,
                "counts": {"source": source_count, "visible": visible, "contributor": contributor, "drawn": drawn},
                "ticket": ticket,
                "current": current,
                "raster_generation": parse_uint(record.get("current_stats_raster_generation", ""), f"{context}.raster", positive=True),
                "encode_attempt": parse_uint(record.get("current_stats_encode_attempt", ""), f"{context}.encode_attempt", positive=True),
            }
        )
    return normalized


def validate_summary(record: dict[str, str], *, warmup: int, measured: int) -> None:
    assert_fields(
        record,
        {
            "status": "ok",
            "exact_plan_requested": PLAN_REQUESTED,
            "actual_plan_set": PLAN_CURRENT_STATS,
            "trace_frames": str(warmup + measured),
            "measured_frames": str(measured),
            "terminal_receipts": str(warmup + measured + len(CAPTURE_TRACE_FRAMES)),
            "final_capture": "available",
        },
        "summary",
    )
    require(record.get("__stream") == "stdout", "summary record must be emitted on stdout")
    parse_uint(record.get("eligibility_retries", ""), "summary.eligibility_retries")
    parse_uint(record.get("capture_retries", ""), "summary.capture_retries")


def validate_lane_session(
    stdout: str,
    stderr: str,
    *,
    lane: Lane,
    raw_directory: Path,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    warmup: int,
    measured: int,
    started_at_utc: str,
    ended_at_utc: str,
) -> LaneSession:
    parsed = parse_session_records(stdout, stderr)
    begin = only(parsed["begin"], f"{lane.name} begin record")
    summary = only(parsed["summary"], f"{lane.name} summary record")
    adapter = validate_begin(begin, dataset=dataset, trace=trace, warmup=warmup, measured=measured)
    normalized = validate_terminal_records(
        parsed["terminal"], lane=lane, source_count=dataset["splat_count"]
    )
    validate_summary(summary, warmup=warmup, measured=measured)
    captures: list[Capture] = []
    for value in normalized:
        context = f"{lane.name}.capture[{value['capture_index']}]"
        path = resolve_capture_file(raw_directory, value["path"], context)
        data = path.read_bytes()
        decoded = BALANCED.decode_rgba8_png(data, context, FORMAL_SIZE)
        require(
            hashlib.sha256(decoded.rgba).hexdigest() == value["rgba8_sha256"],
            f"{context} RGBA8 bytes do not match the atomic capture receipt",
        )
        current = value["current"]
        presentation = {
            "ticket": value["ticket"],
            "outcome": "presented",
            "scene_generation": current["scene_generation"],
            "camera_generation": current["camera_revision"],
            "viewport_generation": current["viewport_generation"],
            "contract_generation": current["contract_generation"],
            "plan_generation": current["plan_set_generation"],
            "presentation_generation": current["presentation_sequence"],
        }
        depth_precision = {
            "profile": lane.profile,
            "scene_generation": current["scene_generation"],
            "camera_revision": current["camera_revision"],
            "viewport_generation": current["viewport_generation"],
            "contract_generation": current["contract_generation"],
            "plan_set_generation": current["plan_set_generation"],
            "plan_id": PLAN_CAPTURE_RECEIPT,
            "order_generation": current["order_generation"],
            "presentation_sequence": current["presentation_sequence"],
            "width": FORMAL_SIZE[0],
            "height": FORMAL_SIZE[1],
            "rgba8_sha256": value["rgba8_sha256"],
        }
        captures.append(
            Capture(
                capture_index=value["capture_index"],
                trace_frame_index=value["trace_frame_index"],
                trace_timestamp_ns=value["trace_timestamp_ns"],
                elapsed_ns=value["elapsed_ns"],
                call_ms=value["call_ms"],
                frame_wall_ms=value["frame_wall_ms"],
                path=path,
                png_sha256=hashlib.sha256(data).hexdigest(),
                rgba8_sha256=value["rgba8_sha256"],
                counts=value["counts"],
                presentation=presentation,
                depth_precision=depth_precision,
                raster_generation=value["raster_generation"],
                encode_attempt=value["encode_attempt"],
            )
        )
    return LaneSession(
        lane=lane,
        captures=tuple(captures),
        adapter=adapter,
        started_at_utc=started_at_utc,
        ended_at_utc=ended_at_utc,
    )


def validate_cross_lane_sessions(sessions: dict[str, LaneSession]) -> None:
    require(set(sessions) == {lane.name for lane in LANES}, "both B1 lanes are required")
    exact = sessions["exact"]
    candidate = sessions["candidate"]
    require(exact.adapter == candidate.adapter, "Exact/Candidate adapter receipts differ")
    require(len(exact.captures) == len(candidate.captures) == 3, "incomplete B1 capture pair")
    for capture_index, (exact_capture, candidate_capture) in enumerate(
        zip(exact.captures, candidate.captures, strict=True)
    ):
        context = f"capture pair {capture_index}"
        require(
            (exact_capture.capture_index, exact_capture.trace_frame_index)
            == (candidate_capture.capture_index, candidate_capture.trace_frame_index),
            f"{context} index/trace mismatch",
        )
        require(exact_capture.counts == candidate_capture.counts, f"{context} S/V/C/D mismatch")
        exact_lifecycle = {key: value for key, value in exact_capture.presentation.items() if key != "ticket"}
        candidate_lifecycle = {key: value for key, value in candidate_capture.presentation.items() if key != "ticket"}
        require(exact_lifecycle == candidate_lifecycle, f"{context} lifecycle/presentation mismatch")


def cargo_executable(stdout: str, target_dir: Path) -> Path:
    executables: set[Path] = set()
    for line in stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        target = message.get("target")
        if (
            message.get("reason") == "compiler-artifact"
            and isinstance(target, dict)
            and target.get("name") == "desktop-example"
            and "bin" in target.get("kind", [])
            and isinstance(message.get("executable"), str)
        ):
            executables.add(Path(message["executable"]).resolve())
    require(len(executables) == 1, "Cargo did not attest exactly one desktop-example executable")
    binary = next(iter(executables))
    try:
        binary.relative_to(target_dir.resolve())
    except ValueError as error:
        raise ValidationError("Cargo-reported executable is outside the private target root") from error
    require(binary.is_file() and os.access(binary, os.X_OK), "collector-built binary is unavailable")
    return binary


def build_desktop_binaries(
    repo: Path, stage: Path, expected_git: dict[str, Any]
) -> tuple[dict[str, Path], dict[str, Any]]:
    target_dir = stage / "cargo-target"
    target_dir.mkdir()
    build_root = stage / "build"
    build_root.mkdir()
    binaries: dict[str, Path] = {}
    receipts: dict[str, Any] = {}
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_dir.resolve())
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
        write_json(lane_dir / "command.json", {"argv": command, "environment": {"CARGO_TARGET_DIR": environment["CARGO_TARGET_DIR"]}})
        completed = subprocess.run(
            command,
            cwd=repo,
            env=environment,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        (lane_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
        (lane_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
        require(completed.returncode == 0, f"{lane.name} release build exited with {completed.returncode}")
        built = cargo_executable(completed.stdout, target_dir)
        retained = lane_dir / "desktop-example-bin"
        shutil.copy2(built, retained)
        retained.chmod(retained.stat().st_mode | 0o100)
        binaries[lane.name] = retained
        receipts[lane.name] = {
            "feature": lane.cargo_feature,
            "binary_sha256": sha256_file(retained),
            "command": f"build/{lane.name}/command.json",
            "stdout": f"build/{lane.name}/stdout.log",
            "stderr": f"build/{lane.name}/stderr.log",
        }
        require(git_receipt(repo) == expected_git, "git receipt changed during B1 build")
    return binaries, receipts


def require_binary_sha256(binary: Path, expected: str) -> None:
    require(sha256_file(binary) == expected, "collector-built binary changed during collection")


def make_command(
    binary: Path,
    dataset_path: Path,
    trace_path: Path,
    *,
    warmup: int,
    measured: int,
) -> list[str]:
    return [
        str(binary),
        str(dataset_path),
        "--geometry-path",
        "packed",
        "--interactive",
        "--camera-trace",
        str(trace_path),
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
        "--surface-evidence-plan",
        PLAN_CLI,
        "--surface-diagnostic-capture-receipt",
        "--surface-diagnostic-multi-capture",
        "--png",
        "capture.png",
    ]


HostInvoker = Callable[[Sequence[str], Path], subprocess.CompletedProcess[str]]
SessionValidator = Callable[..., LaneSession]


def default_host_invoker(command: Sequence[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(command),
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=RUN_TIMEOUT_SECONDS,
    )


def run_host_sessions(
    *,
    stage: Path,
    binaries: dict[str, Path],
    build_receipts: dict[str, Any],
    dataset: dict[str, Any],
    dataset_path: Path,
    trace: dict[str, Any],
    trace_path: Path,
    warmup: int,
    measured: int,
    invoke: HostInvoker = default_host_invoker,
    validate_session: SessionValidator = validate_lane_session,
) -> dict[str, LaneSession]:
    host_root = stage / "host"
    host_root.mkdir()
    sessions: dict[str, LaneSession] = {}
    for lane in LANES:
        raw_directory = host_root / lane.name
        raw_directory.mkdir()
        command = make_command(
            binaries[lane.name], dataset_path, trace_path, warmup=warmup, measured=measured
        )
        write_json(raw_directory / "command.json", {"argv": command, "cwd": f"host/{lane.name}"})
        require_binary_sha256(binaries[lane.name], build_receipts[lane.name]["binary_sha256"])
        started = utc_now()
        completed = invoke(command, raw_directory)
        ended = utc_now()
        (raw_directory / "stdout.log").write_text(completed.stdout, encoding="utf-8")
        (raw_directory / "stderr.log").write_text(completed.stderr, encoding="utf-8")
        require(completed.returncode == 0, f"{lane.name} host exited with {completed.returncode}")
        require_binary_sha256(binaries[lane.name], build_receipts[lane.name]["binary_sha256"])
        sessions[lane.name] = validate_session(
            completed.stdout,
            completed.stderr,
            lane=lane,
            raw_directory=raw_directory,
            dataset=dataset,
            trace=trace,
            warmup=warmup,
            measured=measured,
            started_at_utc=started,
            ended_at_utc=ended,
        )
    validate_cross_lane_sessions(sessions)
    return sessions


def camera_receipt(trace: dict[str, Any], trace_frame_index: int, balanced: Any) -> dict[str, str]:
    frame = trace["value"]["frames"][trace_frame_index]
    return {
        "trace_id": trace["trace_id"],
        "trace_content_sha256": trace["content_sha256"],
        "pose_intrinsics_sha256": balanced.canonical_sha256(
            {"pose": frame["pose"], "intrinsics": frame["intrinsics"]}
        ),
    }


def unavailable_environment(adapter: dict[str, str], device: str | None) -> list[str]:
    unavailable = [
        "frames[*].preprocess_ms",
        "frames[*].sort_ms",
        "frames[*].geometry_submit_ms",
        "frames[*].gpu_wait_ms",
        "frames[*].gpu_complete_ms",
        "frames[*].sort_refreshed",
        "environment.browser",
    ]
    if device is None:
        unavailable.append("environment.device")
    if adapter["driver"] == "unavailable":
        unavailable.append("environment.driver")
    if adapter["driver_info"] == "unavailable":
        unavailable.append("environment.driver_info")
    return unavailable


def build_run_artifact(
    directory: Path,
    *,
    lane_session: LaneSession,
    capture: Capture,
    pair_id: str,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    build: dict[str, Any],
    package: str,
    refresh_hz: float,
    device: str | None,
    camera: dict[str, str],
    warmup: int,
) -> dict[str, Any]:
    directory.mkdir(parents=True)
    image_path = directory / "final-frame.png"
    shutil.copyfile(capture.path, image_path)
    require(sha256_file(image_path) == capture.png_sha256, "capture PNG changed during materialization")
    run_id = f"b1-desktop-{build['git']['commit'][:12]}-{lane_session.lane.name}-{capture.capture_index}-{uuid.uuid4().hex[:10]}"
    frame_budget_ms = 1000.0 / refresh_hz
    unavailable = unavailable_environment(lane_session.adapter, device)
    manifest = {
        "schema": SCHEMA,
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {
            "series_id": "b1-balanced-desktop-moving-010",
            "started_at_utc": lane_session.started_at_utc,
            "ended_at_utc": lane_session.ended_at_utc,
            "measurement_started_at_utc": lane_session.started_at_utc,
            "measurement_ended_at_utc": lane_session.ended_at_utc,
        },
        "build": {
            "repository_commit": build["git"]["commit"],
            "dirty": build["git"]["dirty"],
            "profile": "release",
            "package_version": package,
            "executable_sha256": build["lanes"][lane_session.lane.name]["binary_sha256"],
            "status_porcelain_sha256": build["git"]["status_porcelain_sha256"],
            "provenance": "collector_cargo_build_locked_release_diagnostic_feature",
        },
        "dataset": {key: dataset[key] for key in ("id", "sha256", "bytes", "splat_count", "sh_degree")},
        "trace": {
            "id": trace["trace_id"],
            "sha256": trace["content_sha256"],
            "file_sha256": trace["file_sha256"],
        },
        "renderer": {
            "implementation": f"gsplat-rs B1 {lane_session.lane.profile} Surface",
            "path": "packed_atlas",
            "backend": lane_session.adapter["backend"],
            "sort_policy": "every_frame",
            "exact_plan_requested": PLAN_REQUESTED,
            "exact_plan_actual": PLAN_CURRENT_STATS,
            "count_semantics": COUNT_SEMANTICS,
            "raster_execution_plan": "projected_quads_exact",
            "blend_mode": "sorted_alpha",
            "depth_precision_profile": lane_session.lane.profile,
        },
        "display": {
            "width": FORMAL_SIZE[0],
            "height": FORMAL_SIZE[1],
            "dpr": 1.0,
            "refresh_hz": refresh_hz,
            "frame_budget_ms": frame_budget_ms,
            "refresh_hz_source": "collector_configured_budget",
            "frame_budget_source": "collector_configured_budget",
        },
        "environment": {
            "platform": "macOS",
            "os": platform.platform(),
            "device": device,
            "browser": None,
            "adapter": lane_session.adapter["name"],
            "driver": None if lane_session.adapter["driver"] == "unavailable" else lane_session.adapter["driver"],
            "adapter_device_type": lane_session.adapter["device_type"],
            "driver_info": None if lane_session.adapter["driver_info"] == "unavailable" else lane_session.adapter["driver_info"],
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
            **{f"{stage}_width": FORMAL_SIZE[0] for stage in ("requested", "surface", "internal_render", "presented")},
            **{f"{stage}_height": FORMAL_SIZE[1] for stage in ("requested", "surface", "internal_render", "presented")},
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": True,
        },
        "image": {
            "path": "final-frame.png",
            "sha256": capture.png_sha256,
            "width": FORMAL_SIZE[0],
            "height": FORMAL_SIZE[1],
        },
        "unavailable_fields": unavailable,
    }
    frame = {
        "schema": SCHEMA,
        "record_type": "frame",
        "run_id": run_id,
        "frame_index": 0,
        "elapsed_ns": capture.elapsed_ns,
        "call_ms": capture.call_ms,
        "frame_wall_ms": capture.frame_wall_ms,
        "preprocess_ms": None,
        "sort_ms": None,
        "geometry_submit_ms": None,
        "gpu_wait_ms": None,
        "gpu_complete_ms": None,
        "visible": capture.counts["visible"],
        "contributor": capture.counts["contributor"],
        "drawn": capture.counts["drawn"],
        "active_splats": capture.counts["source"],
        "exact_contributor_compaction": False,
        "sort_refreshed": None,
        "exact_plan_actual": PLAN_CURRENT_STATS,
        "pair_id": pair_id,
        "capture_index": capture.capture_index,
        "trace_frame_index": capture.trace_frame_index,
        "trace_timestamp_ns": capture.trace_timestamp_ns,
        "camera": camera,
        "terminal_outcome": "presented",
        "presentation": capture.presentation,
        "capture_depth_precision": capture.depth_precision,
        "raster_generation": capture.raster_generation,
        "encode_attempt": capture.encode_attempt,
    }
    summary = {
        "schema": SCHEMA,
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": 1,
        "warmup_count": warmup,
        "frame_budget_ms": frame_budget_ms,
        "missed_frame_count": int(capture.frame_wall_ms > frame_budget_ms),
        "distributions": {
            "call_ms": distribution([capture.call_ms]),
            "frame_wall_ms": distribution([capture.frame_wall_ms]),
            "preprocess_ms": None,
            "sort_ms": None,
            "geometry_submit_ms": None,
            "gpu_wait_ms": None,
            "gpu_complete_ms": None,
        },
    }
    write_json(directory / "manifest.json", manifest)
    (directory / "frames.jsonl").write_text(json.dumps(frame, sort_keys=True) + "\n", encoding="utf-8")
    write_json(directory / "summary.json", summary)
    return {"run_id": run_id, "frame_index": 0, "manifest": manifest}


def run_validator(command: Sequence[str], *, repo: Path, context: str) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        list(command),
        cwd=repo,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    detail = completed.stderr.strip() or completed.stdout.strip()
    require(completed.returncode == 0, f"{context} rejected evidence: {detail}")
    return completed


def materialize_suite(
    *,
    stage: Path,
    repo: Path,
    sessions: dict[str, LaneSession],
    dataset: dict[str, Any],
    trace: dict[str, Any],
    build: dict[str, Any],
    refresh_hz: float,
    warmup: int,
) -> dict[str, Any]:
    require(not (stage / "runs").exists(), "run artifacts were materialized before complete validation")
    validators_dir = stage / "validators"
    validators_dir.mkdir()
    device = host_device_name()
    run_receipts: dict[tuple[str, int], dict[str, Any]] = {}
    camera_receipts: dict[int, dict[str, str]] = {}
    for capture_index, trace_frame_index in enumerate(CAPTURE_TRACE_FRAMES):
        camera_receipts[capture_index] = camera_receipt(trace, trace_frame_index, BALANCED)
        pair_id = f"b1-desktop-{build['git']['commit'][:12]}-capture-{capture_index}"
        for lane in LANES:
            directory = stage / "runs" / lane.name / f"capture-{capture_index}"
            result = build_run_artifact(
                directory,
                lane_session=sessions[lane.name],
                capture=sessions[lane.name].captures[capture_index],
                pair_id=pair_id,
                dataset=dataset,
                trace=trace,
                build=build,
                package=build["package_version"],
                refresh_hz=refresh_hz,
                device=device,
                camera=camera_receipts[capture_index],
                warmup=warmup,
            )
            validator = run_validator(
                [sys.executable, str(BENCHMARK_VALIDATOR_PATH), str(directory)],
                repo=repo,
                context=f"{lane.name} capture {capture_index} standard validator",
            )
            prefix = validators_dir / f"benchmark-{lane.name}-capture-{capture_index}"
            prefix.with_suffix(".stdout.log").write_text(validator.stdout, encoding="utf-8")
            prefix.with_suffix(".stderr.log").write_text(validator.stderr, encoding="utf-8")
            run_receipts[(lane.name, capture_index)] = {
                "path": directory.relative_to(stage).as_posix(),
                "sha256": BALANCED.artifact_directory_sha256(directory),
                "run_id": result["run_id"],
                "frame_index": result["frame_index"],
                "depth_precision": sessions[lane.name].captures[capture_index].depth_precision,
            }

    frames: list[dict[str, Any]] = []
    decoded_frames: list[Any] = []
    for capture_index, trace_frame_index in enumerate(CAPTURE_TRACE_FRAMES):
        exact_capture = sessions["exact"].captures[capture_index]
        candidate_capture = sessions["candidate"].captures[capture_index]
        exact_run = stage / run_receipts[("exact", capture_index)]["path"]
        candidate_run = stage / run_receipts[("candidate", capture_index)]["path"]
        exact_image = {
            "path": (exact_run / "final-frame.png").relative_to(stage).as_posix(),
            "sha256": exact_capture.png_sha256,
            "width": FORMAL_SIZE[0],
            "height": FORMAL_SIZE[1],
            "depth_precision": exact_capture.depth_precision,
        }
        candidate_image = {
            "path": (candidate_run / "final-frame.png").relative_to(stage).as_posix(),
            "sha256": candidate_capture.png_sha256,
            "width": FORMAL_SIZE[0],
            "height": FORMAL_SIZE[1],
            "depth_precision": candidate_capture.depth_precision,
        }
        exact_decoded = BALANCED.decode_rgba8_png(
            (exact_run / "final-frame.png").read_bytes(), "Exact capture", FORMAL_SIZE
        )
        candidate_decoded = BALANCED.decode_rgba8_png(
            (candidate_run / "final-frame.png").read_bytes(), "Candidate capture", FORMAL_SIZE
        )
        metrics = BALANCED.compute_frame_metrics(exact_decoded, candidate_decoded)
        frame = {
            "capture_index": capture_index,
            "trace_frame_index": trace_frame_index,
            "presented": True,
            "camera": camera_receipts[capture_index],
            "presentation": {
                "exact": exact_capture.presentation,
                "candidate": candidate_capture.presentation,
            },
            "exact": exact_image,
            "candidate": candidate_image,
            "metrics": metrics,
            "benchmark_artifacts": {
                "pair_id": f"b1-desktop-{build['git']['commit'][:12]}-capture-{capture_index}",
                "exact": run_receipts[("exact", capture_index)],
                "candidate": run_receipts[("candidate", capture_index)],
            },
        }
        frames.append(frame)
        decoded_frames.append(
            BALANCED.FramePixels(
                capture_index=capture_index,
                trace_frame_index=trace_frame_index,
                exact=exact_decoded,
                candidate=candidate_decoded,
            )
        )

    transitions = []
    for index in range(len(decoded_frames) - 1):
        transitions.append(
            {
                "from_capture_index": index,
                "to_capture_index": index + 1,
                "from_trace_frame_index": CAPTURE_TRACE_FRAMES[index],
                "to_trace_frame_index": CAPTURE_TRACE_FRAMES[index + 1],
                "metrics": {
                    BALANCED.TEMPORAL_METRIC: BALANCED.compute_temporal_metric(
                        decoded_frames[index], decoded_frames[index + 1]
                    )
                },
            }
        )

    suite = {
        "schema": SUITE_SCHEMA,
        "evidence_class": "formal_quality",
        "authority": {
            "dataset_manifest": {
                "path": str(build["dataset_manifest_path"]),
                "sha256": build["dataset_manifest_sha256"],
                "dataset_id": dataset["id"],
                "asset_sha256": dataset["sha256"],
            },
            "trace": {
                "path": str(build["trace_path"]),
                "sha256": trace["file_sha256"],
                "trace_id": trace["trace_id"],
                "content_sha256": trace["content_sha256"],
            },
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
            "render_mode": "sorted_alpha",
            "partial_scene_published": False,
            "full_quality": True,
        },
        "resolution": {
            **{f"{stage}_width": FORMAL_SIZE[0] for stage in ("requested", "surface", "internal_render", "presented")},
            **{f"{stage}_height": FORMAL_SIZE[1] for stage in ("requested", "surface", "internal_render", "presented")},
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": True,
        },
        "camera": {"mode": "moving_sequence", "trace_frame_indices": list(CAPTURE_TRACE_FRAMES)},
        "frames": frames,
        "transitions": transitions,
        "collector": {
            "repository_commit": build["git"]["commit"],
            "dirty": build["git"]["dirty"],
            "host_invocations": 2,
            "lane_profiles": [lane.profile for lane in LANES],
            "balanced_validator_sha256": sha256_file(BALANCED_VALIDATOR_PATH),
            "benchmark_validator_sha256": sha256_file(BENCHMARK_VALIDATOR_PATH),
        },
    }
    suite_path = stage / "suite.json"
    write_json(suite_path, suite)
    validator = run_validator(
        [sys.executable, str(BALANCED_VALIDATOR_PATH), str(suite_path)],
        repo=repo,
        context="Balanced image-gate validator",
    )
    (validators_dir / "balanced.stdout.log").write_text(validator.stdout, encoding="utf-8")
    (validators_dir / "balanced.stderr.log").write_text(validator.stderr, encoding="utf-8")
    return suite


def remove_private_builds(stage: Path) -> None:
    target_dir = stage / "cargo-target"
    require(target_dir.is_dir() and not target_dir.is_symlink(), "private Cargo target is unavailable")
    shutil.rmtree(target_dir)
    for lane in LANES:
        binary = stage / "build" / lane.name / "desktop-example-bin"
        require(binary.is_file() and not binary.is_symlink(), f"{lane.name} retained binary is unavailable")
        binary.unlink()
        capture_directory = stage / "host" / lane.name / "capture.captures"
        require(capture_directory.is_dir() and not capture_directory.is_symlink(), f"{lane.name} raw capture directory is unavailable")
        shutil.rmtree(capture_directory)


def publish_suite(stage: Path, output: Path) -> None:
    require((stage / "suite.json").is_file(), "validated suite manifest is unavailable")
    for lane in LANES:
        for capture_index in range(3):
            artifact = stage / "runs" / lane.name / f"capture-{capture_index}"
            for name in ("manifest.json", "frames.jsonl", "summary.json", "final-frame.png"):
                require((artifact / name).is_file(), f"{lane.name} capture {capture_index} is missing {name}")
    require(not (stage / "cargo-target").exists(), "private Cargo target must not be published")
    executables = [
        path.relative_to(stage)
        for path in stage.rglob("*")
        if path.is_file() and path.stat().st_mode & 0o111
    ]
    require(not executables, f"staging contains executable files: {', '.join(map(str, executables))}")
    require(not output.exists(), f"output appeared during collection: {output}")
    os.rename(stage, output)


def collect(args: argparse.Namespace, repo: Path) -> dict[str, Any]:
    output = args.output.resolve()
    require(not output.exists(), f"output already exists: {output}")
    require((args.warmup, args.measured) == (CANONICAL_WARMUP, CANONICAL_MEASURED), "formal B1 requires warmup=20 and measured=80")
    require(math.isfinite(args.refresh_hz) and args.refresh_hz > 0.0, "refresh-hz must be positive")
    validate_ignored_output(repo, output)
    initial_git = git_receipt(repo)
    require(not initial_git["dirty"], "formal B1 evidence requires a clean repository")

    dataset, dataset_path = M2B.read_dataset_manifest(repo, args.dataset_manifest.resolve())
    trace = M2B.read_trace(repo, args.trace.resolve())
    trace_path = Path(trace["path"])
    trace["value"] = json.loads(trace_path.read_text(encoding="utf-8"))
    stage = output.parent / f".{output.name}.stage-{os.getpid()}-{uuid.uuid4().hex[:10]}"
    require(not stage.exists(), f"staging path exists: {stage}")
    stage.mkdir(parents=True)
    try:
        binaries, lane_builds = build_desktop_binaries(repo, stage, initial_git)
        build = {
            "git": initial_git,
            "package_version": package_version(repo),
            "lanes": lane_builds,
            "dataset_manifest_path": args.dataset_manifest.resolve().relative_to(repo.resolve()).as_posix(),
            "dataset_manifest_sha256": sha256_file(args.dataset_manifest.resolve()),
            "trace_path": trace_path.resolve().relative_to(repo.resolve()).as_posix(),
        }
        sessions = run_host_sessions(
            stage=stage,
            binaries=binaries,
            build_receipts=lane_builds,
            dataset=dataset,
            dataset_path=dataset_path,
            trace=trace,
            trace_path=trace_path,
            warmup=args.warmup,
            measured=args.measured,
        )
        require(sha256_file(dataset_path) == dataset["sha256"], "dataset changed during collection")
        require(sha256_file(trace_path) == trace["file_sha256"], "trace changed during collection")
        require(git_receipt(repo) == initial_git, "git receipt changed during collection")
        suite = materialize_suite(
            stage=stage,
            repo=repo,
            sessions=sessions,
            dataset=dataset,
            trace=trace,
            build=build,
            refresh_hz=args.refresh_hz,
            warmup=args.warmup,
        )
        remove_private_builds(stage)
        publish_suite(stage, output)
        return suite
    except Exception as error:
        failure = {"status": "failed", "error": str(error), "output_published": output.exists()}
        write_json(stage / "failure.json", failure)
        failed = output.parent / f"{output.name}.failed-{os.getpid()}-{uuid.uuid4().hex[:8]}"
        os.replace(stage, failed)
        raise ValidationError(f"{error}; retained_failed_evidence={failed}") from error


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect the two-process desktop B1 moving 0-1-0 suite")
    parser.add_argument("--dataset-manifest", required=True, type=Path)
    parser.add_argument("--trace", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--warmup", type=int, default=CANONICAL_WARMUP)
    parser.add_argument("--measured", type=int, default=CANONICAL_MEASURED)
    parser.add_argument("--refresh-hz", type=float, default=60.0)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        suite = collect(args, REPO_ROOT)
    except (OSError, subprocess.SubprocessError, ValidationError, ValueError) as error:
        print(f"desktop B1 collection failed: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "status": "ok",
                "output": str(args.output),
                "schema": suite["schema"],
                "host_invocations": 2,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
