#!/usr/bin/env python3
"""Collect one immutable Q1 M4 native control plus terminal-throughput prerequisite.

This command builds one private locked-release desktop host and invokes it
once. Sustained current-stats is untimed control evidence; the separate formal
window contains only cadence-matched presents and one terminal queue drain. It
does not run PlayCanvas, a common reference-image gate, or the five outer pairs
required for a final Q1 comparison.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import subprocess
import sys
from enum import Enum
from pathlib import Path
from typing import Any, Sequence


REPO_ROOT = Path(__file__).resolve().parents[2]
BASE_PATH = REPO_ROOT / "tests/perf/collect-desktop-producer-ab.py"
SURFACE_EVIDENCE_PATH = REPO_ROOT / "tests/perf/collect-desktop-surface-evidence.py"
PAIRED_TIMING_PATH = REPO_ROOT / "tests/perf/collect-balanced-paired-timing.py"
TRACE_V1_PATH = REPO_ROOT / "tests/perf/trace/trace_v1.py"
MATRIX_PATH = REPO_ROOT / "tests/perf/full-quality-matrix-plan-v1.json"

SCHEMA = "gsplat-q1-m4-native-sustained/v3"
CELL = "Q1.M4.Native.PackedExact.Adaptive.ControlAndTerminalThroughput"
FEATURE = "qualification-q1-m4-native"
FORMAL_SIZE = (1920, 1080)
WARMUP = 20
MEASURED = 80
CADENCE_NS = 16_666_667
RUN_TIMEOUT_SECONDS = 30 * 60
TRUCK = {
    "id": "truck-full",
    "local_path": "tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
    "sha256": "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c",
    "bytes": 630_225_580,
    "splat_count": 2_541_226,
    "sh_degree": 3,
}
TRACE = {
    "id": "candidate-truck-quality-2view-1920x1080-v1",
    "local_path": "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
    "content_sha256": "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3",
    "file_sha256": "13081183bf2d1c6b6ec165324f53185cc30af2f304db3fefa7aa3d9044ac7c5a",
    "bytes": 6_617,
}
DEFERRED_BOUNDARIES = (
    "playcanvas_headful_external_presentation",
    "common_reference_image_gate",
    "five_outer_counterbalanced_pairs",
)
PREFIXES = {
    "begin": "SURFACE_Q1_SUSTAINED_BEGIN ",
    "presentation": "SURFACE_Q1_SUSTAINED_PRESENTATION ",
    "submission": "SURFACE_Q1_SUSTAINED_SUBMISSION ",
    "terminal": "SURFACE_Q1_SUSTAINED_TERMINAL ",
    "drain": "SURFACE_Q1_SUSTAINED_DRAIN ",
    "capture": "SURFACE_Q1_SUSTAINED_CAPTURE ",
    "summary": "SURFACE_Q1_SUSTAINED_SUMMARY ",
    "timed_presentation": "SURFACE_Q1_TIMED_PRESENTATION ",
    "timed_drain": "SURFACE_Q1_TIMED_DRAIN ",
    "timed_summary": "SURFACE_Q1_TIMED_SUMMARY ",
}
JOIN_FIELDS = (
    "ticket",
    "phase",
    "member_index",
    "trace_frame",
    "executed_plan",
    "scene_generation",
    "camera_revision",
    "viewport_generation",
    "contract_generation",
    "plan_set_generation",
    "order_generation",
    "raster_generation",
    "encode_attempt",
    "presentation_sequence",
)
PRESENTATION_JOIN_FIELDS = (
    "phase",
    "member_index",
    "trace_frame",
    "camera_revision",
)
COUNT_SEMANTICS_DRAW_SOURCE = {
    "direct_draw_equals_visible": "visible",
    "indirect_draw_equals_visible": "visible",
    "indirect_draw_equals_contributor": "contributor",
}
CAMERA_TOLERANCE = 5.0e-5
WHOLE_PLAN_ADAPTIVE_STATES = {
    "disabled",
    "cpu_learning",
    "cpu_stable",
    "gpu_probe",
    "gpu_stable",
    "cpu_probe",
    "cooldown",
}
PROJECTED_ADAPTIVE_STATES = {
    "disabled",
    "candidate_learning",
    "candidate_stable",
    "compact_probe",
    "compact_stable",
    "candidate_probe",
    "candidate_only",
    "cooldown",
}


def load_module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


BASE = load_module("collect_desktop_producer_ab_for_q1_native", BASE_PATH)
SURFACE_EVIDENCE = load_module(
    "collect_desktop_surface_evidence_for_q1_native", SURFACE_EVIDENCE_PATH
)
PAIRED_TIMING = load_module("collect_balanced_paired_timing_for_q1_native", PAIRED_TIMING_PATH)
TRACE_V1 = load_module("trace_v1_for_q1_native", TRACE_V1_PATH)
ValidationError = BASE.ValidationError
require = BASE.require
only = BASE.only
parse_uint = BASE.parse_uint
parse_float = BASE.parse_float
sha256_file = BASE.sha256_file
git_receipt = BASE.git_receipt
validate_ignored_output = BASE.validate_ignored_output
utc_now = BASE.utc_now
write_json = SURFACE_EVIDENCE.write_json
write_jsonl = SURFACE_EVIDENCE.write_jsonl
timeout_stream = SURFACE_EVIDENCE.timeout_stream
TicketJoinLedger = SURFACE_EVIDENCE.TicketJoinLedger
live_view_matrix = TRACE_V1.view_matrix
live_projection_matrix = TRACE_V1.projection_matrix
multiply_mat4 = TRACE_V1.mat4_multiply


class EnvironmentPrerequisiteError(ValidationError):
    """The fixed M4/Metal/Truck/build prerequisite is unavailable."""


class IntegrityRejectedError(ValidationError):
    """The host emitted evidence but violated the frozen Q1 native contract."""


class CollectionPhase(Enum):
    PREFLIGHT = "preflight"
    BUILD_STARTED = "build_started"
    HOST_STARTED = "host_started"
    PROTOCOL_STARTED = "protocol_started"
    FINALIZATION_STARTED = "finalization_started"


def classify_failure(error: Exception, phase: CollectionPhase) -> tuple[str, str]:
    if isinstance(error, EnvironmentPrerequisiteError) and phase is CollectionPhase.PREFLIGHT:
        return "Deferred", "environment_prerequisite"
    return "Rejected", "integrity_rejected"


def integrity_require(condition: bool, message: str) -> None:
    if not condition:
        raise IntegrityRejectedError(message)


def require_object(value: Any, context: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{context} must be an object")
    return value


def require_string(value: Any, context: str) -> str:
    require(isinstance(value, str) and value, f"{context} must be a non-empty string")
    return value


def apple_cpu_brand() -> str:
    if platform.system() != "Darwin":
        return "unavailable"
    completed = subprocess.run(
        ["sysctl", "-n", "machdep.cpu.brand_string"],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    brand = completed.stdout.strip()
    if brand:
        return brand
    completed = subprocess.run(
        ["sysctl", "-n", "hw.model"],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    return completed.stdout.strip() or "unavailable"


def host_receipt() -> dict[str, str]:
    return {
        "system": platform.system(),
        "machine": platform.machine(),
        "cpu_brand": apple_cpu_brand(),
    }


def load_workload(repo: Path) -> dict[str, Any]:
    dataset_path = (repo / TRUCK["local_path"]).resolve()
    trace_path = (repo / TRACE["local_path"]).resolve()
    if not dataset_path.is_file():
        raise EnvironmentPrerequisiteError(f"full Truck dataset is unavailable: {dataset_path}")
    if not trace_path.is_file():
        raise EnvironmentPrerequisiteError(f"frozen Truck trace is unavailable: {trace_path}")
    shared = PAIRED_TIMING.load_workload(repo, MATRIX_PATH)
    dataset_entry = shared.dataset
    integrity_require(shared.dataset_path == dataset_path, "matrix Truck dataset path drifted")
    integrity_require(shared.trace_path == trace_path, "matrix Truck trace path drifted")
    for field in ("local_path", "sha256", "bytes", "splat_count", "sh_degree"):
        require(dataset_entry.get(field) == TRUCK[field], f"matrix Truck {field} drifted")
    require(
        (shared.trace.get("width"), shared.trace.get("height")) == FORMAL_SIZE,
        "matrix Truck trace size drifted",
    )
    integrity_require(trace_path.stat().st_size == TRACE["bytes"], "Truck trace byte count drifted")
    integrity_require(shared.trace["file_sha256"] == TRACE["file_sha256"], "Truck trace file SHA drifted")
    trace_value = require_object(json.loads(trace_path.read_text(encoding="utf-8")), "Truck trace")
    integrity_require(trace_value.get("trace_id") == TRACE["id"], "Truck trace ID drifted")
    integrity_require(
        trace_value.get("content_sha256") == TRACE["content_sha256"],
        "Truck trace content SHA drifted",
    )
    frames = trace_value.get("frames")
    integrity_require(isinstance(frames, list) and len(frames) == 2, "Truck trace must have two views")
    return {
        "dataset": dataset_entry,
        "dataset_path": dataset_path,
        "trace": trace_value,
        "trace_path": trace_path,
        "trace_file_sha256": TRACE["file_sha256"],
    }


def parse_log(stdout: str, stderr: str = "") -> dict[str, list[dict[str, str]]]:
    records: dict[str, list[dict[str, str]]] = {key: [] for key in PREFIXES}
    for stream_name, stream in (("stdout", stdout), ("stderr", stderr)):
        for line_number, line in enumerate(stream.splitlines(), 1):
            for key, prefix in PREFIXES.items():
                if line.startswith(prefix):
                    records[key].append(
                        BASE.parse_key_value_payload(
                            line[len(prefix) :], f"{stream_name}:{line_number}:{key}"
                        )
                    )
                    break
    return records


def parse_floats(value: str, expected: int, context: str) -> list[float]:
    parts = value.split(",")
    integrity_require(len(parts) == expected, f"{context} expected {expected} values")
    parsed: list[float] = []
    for index, part in enumerate(parts):
        try:
            parsed.append(float(part))
        except ValueError as error:
            raise IntegrityRejectedError(f"{context}[{index}] must be numeric") from error
    integrity_require(all(math.isfinite(item) for item in parsed), f"{context} is not finite")
    return parsed


def close_values(actual: Sequence[float], expected: Sequence[float], context: str) -> None:
    integrity_require(len(actual) == len(expected), f"{context} length mismatch")
    for index, (left, right) in enumerate(zip(actual, expected)):
        integrity_require(
            math.isclose(left, right, rel_tol=0.0, abs_tol=CAMERA_TOLERANCE),
            f"{context}[{index}] mismatch: {left} != {right}",
        )


def validate_camera(record: dict[str, str], trace_frame: dict[str, Any], context: str) -> None:
    position = parse_floats(record.get("position", ""), 3, f"{context}.position")
    rotation = parse_floats(record.get("rotation_xyzw", ""), 4, f"{context}.rotation")
    vfov = parse_float(record.get("vertical_fov_radians", ""), f"{context}.vfov")
    near = parse_float(record.get("near_plane", ""), f"{context}.near")
    far = parse_float(record.get("far_plane", ""), f"{context}.far")
    aspect = parse_float(record.get("aspect", ""), f"{context}.aspect")
    view = parse_floats(record.get("view_matrix", ""), 16, f"{context}.view")
    projection = parse_floats(record.get("projection_matrix", ""), 16, f"{context}.projection")
    view_projection = parse_floats(
        record.get("view_projection_matrix", ""), 16, f"{context}.view_projection"
    )
    close_values(position, trace_frame["pose"]["position"], f"{context}.trace_position")
    close_values(rotation, trace_frame["pose"]["rotation_xyzw"], f"{context}.trace_rotation")
    close_values(
        [vfov, near, far],
        [
            trace_frame["intrinsics"]["vertical_fov_radians"],
            trace_frame["intrinsics"]["near_plane"],
            trace_frame["intrinsics"]["far_plane"],
        ],
        f"{context}.trace_intrinsics",
    )
    close_values([aspect], [FORMAL_SIZE[0] / FORMAL_SIZE[1]], f"{context}.aspect")
    recomputed_view = live_view_matrix(position, rotation)
    recomputed_projection = live_projection_matrix(vfov, near, far, aspect)
    close_values(view, recomputed_view, f"{context}.recomputed_view")
    close_values(projection, recomputed_projection, f"{context}.recomputed_projection")
    close_values(
        view_projection,
        multiply_mat4(recomputed_projection, recomputed_view),
        f"{context}.recomputed_view_projection",
    )
    close_values(view, trace_frame["view_matrix"], f"{context}.trace_view")
    close_values(projection, trace_frame["projection_matrix"], f"{context}.trace_projection")
    close_values(
        view_projection,
        trace_frame["view_projection_matrix"],
        f"{context}.trace_view_projection",
    )


def expected_trace_frame(phase: str, member_index: int) -> int:
    if phase in {"warmup", "measure"}:
        return member_index % 2
    if phase == "capture":
        return member_index
    raise IntegrityRejectedError(f"unknown Q1 phase {phase!r}")


def validate_actual_plan_receipt(record: dict[str, str], context: str) -> None:
    expected = {
        "cpu_post_sort": ("cpu", "candidate"),
        "gpu_post_sort": ("gpu", "candidate"),
        "gpu_preproject": ("gpu", "compact"),
    }
    plan = record.get("actual_plan") or record.get("executed_plan")
    integrity_require(plan in expected, f"{context}.actual plan drifted")
    backend, execution = expected[plan]
    integrity_require(record.get("actual_backend") == backend, f"{context}.backend/plan mismatch")
    integrity_require(
        record.get("projected_draw_execution") == execution,
        f"{context}.execution/plan mismatch",
    )


def validate_terminal_window_bounds(
    *,
    expected_n: int,
    warmup_drain_end_ns: int,
    measured_input_ns: Sequence[int],
    measured_presented_ns: Sequence[int],
    terminal_end_ns: int,
) -> tuple[int, int, int]:
    integrity_require(expected_n > 0, "Q1 timed terminal window N must be positive")
    integrity_require(
        len(measured_input_ns) == expected_n
        and len(measured_presented_ns) == expected_n,
        "Q1 timed terminal window membership does not match N",
    )
    window_start_ns = measured_input_ns[0]
    integrity_require(
        warmup_drain_end_ns <= window_start_ns,
        "Q1 timed measured input preceded warmup queue completion",
    )
    integrity_require(
        terminal_end_ns >= max(measured_presented_ns),
        "Q1 timed queue completion preceded the final presentation",
    )
    integrity_require(
        terminal_end_ns > window_start_ns,
        "Q1 timed terminal window is empty or non-monotonic",
    )
    return window_start_ns, terminal_end_ns, terminal_end_ns - window_start_ns


def validate_phase_presentation_timeline(
    *,
    phase: str,
    input_ns: Sequence[int],
    presented_ns: Sequence[int],
) -> None:
    integrity_require(
        bool(input_ns) and len(input_ns) == len(presented_ns),
        f"Q1 timed {phase} presentation timeline is incomplete",
    )
    for index, (input_at, presented_at) in enumerate(zip(input_ns, presented_ns)):
        integrity_require(
            input_at <= presented_at,
            f"Q1 timed {phase} member {index} presented before camera input",
        )
        if index + 1 < len(input_ns):
            integrity_require(
                presented_at <= input_ns[index + 1],
                f"Q1 timed {phase} member {index} presentation crossed the next camera input",
            )


def validate_run_log(
    stdout: str,
    stderr: str,
    *,
    workload: dict[str, Any],
    run_dir: Path | None = None,
) -> dict[str, Any]:
    records = parse_log(stdout, stderr)
    begin = only(records["begin"], "SURFACE_Q1_SUSTAINED_BEGIN")
    control_summary = only(records["summary"], "SURFACE_Q1_SUSTAINED_SUMMARY")
    timed_summary = only(records["timed_summary"], "SURFACE_Q1_TIMED_SUMMARY")
    integrity_require(begin.get("schema") == SCHEMA, "Q1 host schema drifted")
    fixed_begin = {
        "stages": "untimed_current_stats_control,terminal_throughput",
        "shared_process_identity": "true",
        "trace_id": TRACE["id"],
        "trace_sha256": TRACE["content_sha256"],
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "order_backend": "adaptive",
        "projected_draw_policy": "adaptive",
        "sort_policy": "every_frame",
        "sort_interval": "1",
        "host_cadence_hz": "60",
        "cadence_ns": str(CADENCE_NS),
        "control_warmup_frames": str(WARMUP),
        "control_measured_frames": str(MEASURED),
        "timed_warmup_frames": str(WARMUP),
        "timed_measured_frames": str(MEASURED),
        "capture_trace_frames": "0,1",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "source_count": str(TRUCK["splat_count"]),
        "decoded_count": str(TRUCK["splat_count"]),
        "encoded_count": str(TRUCK["splat_count"]),
        "resident_count": str(TRUCK["splat_count"]),
        "addressable_count": str(TRUCK["splat_count"]),
        "sh_degree": "3",
        "requested_width": "1920",
        "requested_height": "1080",
        "surface_width": "1920",
        "surface_height": "1080",
        "internal_render_width": "1920",
        "internal_render_height": "1080",
        "adapter_backend": "metal",
    }
    for field, expected in fixed_begin.items():
        integrity_require(begin.get(field) == expected, f"Q1 begin {field} drifted")
    integrity_require("Apple M4" in begin.get("adapter_name", ""), "Q1 adapter is not Apple M4")

    submissions = records["submission"]
    terminals = records["terminal"]
    presentations = records["presentation"]
    captures = records["capture"]
    integrity_require(len(submissions) == WARMUP + MEASURED + 2, "Q1 submission ledger length mismatch")
    integrity_require(len(terminals) == len(submissions), "Q1 terminal ledger length mismatch")
    integrity_require(len(captures) == 2, "Q1 capture ledger length mismatch")

    expected_by_phase = {"warmup": WARMUP, "measure": MEASURED, "capture": 2}
    ticket_ledger = TicketJoinLedger(strict_order=True, validator=integrity_require)
    submission_by_ticket = ticket_ledger.issued
    submission_keys: dict[tuple[str, int], dict[str, str]] = {}
    frames = workload["trace"]["frames"]
    for index, submission in enumerate(submissions):
        context = f"submission[{index}]"
        phase = submission.get("phase", "")
        integrity_require(phase in expected_by_phase, f"{context}.phase is invalid")
        member = parse_uint(submission.get("member_index", ""), f"{context}.member")
        integrity_require(member < expected_by_phase[phase], f"{context}.member is out of range")
        key = (phase, member)
        integrity_require(key not in submission_keys, f"duplicate Q1 member {key}")
        trace_frame = parse_uint(submission.get("trace_frame", ""), f"{context}.trace_frame")
        integrity_require(trace_frame == expected_trace_frame(phase, member), f"{context}.trace frame drifted")
        ticket = parse_uint(submission.get("ticket", ""), f"{context}.ticket", positive=True)
        presentation = parse_uint(
            submission.get("presentation_sequence", ""), f"{context}.presentation", positive=True
        )
        integrity_require(
            submission.get("executed_plan")
            in {"cpu_post_sort", "gpu_post_sort", "gpu_preproject"},
            f"{context}.executed_plan drifted",
        )
        integrity_require(
            submission.get("evidence_role") == "untimed_correctness_control",
            f"{context}.evidence_role drifted",
        )
        integrity_require(submission.get("frame_presented") == "true", f"{context} was not presented")
        validate_camera(submission, frames[trace_frame], context)
        ticket_ledger.issue(ticket, presentation, submission, context)
        submission_keys[key] = submission
    for phase, expected in expected_by_phase.items():
        integrity_require(
            sum(key[0] == phase for key in submission_keys) == expected,
            f"Q1 {phase} submission membership is incomplete",
        )

    terminal_observed: list[int] = []
    measured_terminal_observed: list[int] = []
    joined_frames: list[dict[str, Any]] = []
    for index, terminal in enumerate(terminals):
        context = f"terminal[{index}]"
        integrity_require(terminal.get("status") == "ready", f"{context} is not Ready")
        integrity_require(
            terminal.get("evidence_role") == "untimed_correctness_control",
            f"{context}.evidence_role drifted",
        )
        ticket = parse_uint(terminal.get("ticket", ""), f"{context}.ticket", positive=True)
        submission = ticket_ledger.resolve(
            ticket, terminal, context, join_fields=JOIN_FIELDS
        )
        phase = terminal.get("phase", "")
        member = parse_uint(terminal.get("member_index", ""), f"{context}.member")
        integrity_require((phase, member) in submission_keys, f"{context} member is unknown")
        source = parse_uint(terminal.get("source_count", ""), f"{context}.source")
        visible = parse_uint(terminal.get("visible_count", ""), f"{context}.visible")
        contributor = parse_uint(terminal.get("contributor_count", ""), f"{context}.contributor")
        drawn = parse_uint(terminal.get("drawn_count", ""), f"{context}.drawn")
        integrity_require(source == TRUCK["splat_count"], f"{context}.source count drifted")
        integrity_require(0 <= contributor <= visible <= source, f"{context} violates C<=V<=S")
        semantics = terminal.get("count_semantics")
        integrity_require(
            semantics in COUNT_SEMANTICS_DRAW_SOURCE,
            f"{context}.count_semantics is not a closed-enum value",
        )
        expected_drawn = (
            contributor
            if COUNT_SEMANTICS_DRAW_SOURCE[semantics] == "contributor"
            else visible
        )
        integrity_require(drawn == expected_drawn, f"{context}.drawn violates semantics")
        observed = parse_uint(terminal.get("observed_ns", ""), f"{context}.observed")
        terminal_observed.append(observed)
        if phase == "measure":
            measured_terminal_observed.append(observed)
            joined_frames.append(
                {
                    "evidence_role": "untimed_correctness_control",
                    "phase": phase,
                    "member_index": member,
                    "trace_frame": parse_uint(terminal["trace_frame"], f"{context}.trace_frame"),
                    "ticket": ticket,
                    "presentation_sequence": parse_uint(
                        terminal["presentation_sequence"], f"{context}.presentation"
                    ),
                    "executed_plan": terminal["executed_plan"],
                    "terminal_observed_ns": observed,
                    "source_count": source,
                    "visible_count": visible,
                    "contributor_count": contributor,
                    "drawn_count": drawn,
                }
            )
    ticket_ledger.require_complete("Q1")
    integrity_require(
        all(left <= right for left, right in zip(terminal_observed, terminal_observed[1:])),
        "Q1 terminal observations are not monotonic",
    )
    joined_frames.sort(key=lambda item: item["member_index"])
    integrity_require(
        [item["member_index"] for item in joined_frames] == list(range(MEASURED)),
        "Q1 measured terminal membership is not contiguous",
    )

    presentation_by_ticket: dict[int, dict[str, str]] = {}
    attempts: dict[tuple[str, int], list[int]] = {}
    cadence_starts: dict[str, list[int]] = {phase: [] for phase in expected_by_phase}
    for index, presentation in enumerate(presentations):
        context = f"presentation[{index}]"
        phase = presentation.get("phase", "")
        integrity_require(phase in expected_by_phase, f"{context}.phase is invalid")
        member = parse_uint(presentation.get("member_index", ""), f"{context}.member")
        attempt = parse_uint(presentation.get("attempt", ""), f"{context}.attempt")
        attempts.setdefault((phase, member), []).append(attempt)
        cadence_starts[phase].append(
            parse_uint(presentation.get("cadence_start_ns", ""), f"{context}.cadence_start")
        )
        integrity_require(presentation.get("frame_presented") == "true", f"{context} was dropped")
        integrity_require(
            presentation.get("evidence_role") == "untimed_correctness_control",
            f"{context}.evidence_role drifted",
        )
        integrity_require(
            presentation.get("observer_load") == "current_stats",
            f"{context}.observer_load drifted",
        )
        integrity_require(
            presentation.get("whole_plan_adaptive_state") in WHOLE_PLAN_ADAPTIVE_STATES,
            f"{context}.whole_plan_adaptive_state drifted",
        )
        integrity_require(
            presentation.get("projected_adaptive_state") in PROJECTED_ADAPTIVE_STATES,
            f"{context}.projected_adaptive_state drifted",
        )
        integrity_require(
            presentation.get("projected_draw_execution") in {"candidate", "compact"},
            f"{context}.projected_draw_execution drifted",
        )
        integrity_require(presentation.get("sort_refreshed") == "true", f"{context} did not refresh sort")
        submission_state = presentation.get("submission")
        if submission_state == "issued":
            ticket = parse_uint(presentation.get("ticket", ""), f"{context}.ticket", positive=True)
            integrity_require(ticket in submission_by_ticket, f"{context} issued unknown ticket")
            integrity_require(ticket not in presentation_by_ticket, f"duplicate presentation ticket {ticket}")
            submission = submission_by_ticket[ticket]
            for field in PRESENTATION_JOIN_FIELDS:
                integrity_require(
                    presentation.get(field) == submission.get(field),
                    f"{context}.{field} does not match ticket {ticket} submission",
                )
            validate_actual_plan_receipt(
                {
                    **presentation,
                    "actual_plan": submission["executed_plan"],
                },
                context,
            )
            presentation_by_ticket[ticket] = presentation
        else:
            integrity_require(submission_state == "not_requested", f"{context}.submission is invalid")
            integrity_require(presentation.get("ticket") == "none", f"{context} fabricated a ticket")
    integrity_require(
        set(presentation_by_ticket) == set(submission_by_ticket),
        "Q1 presentation/submission ticket set mismatch",
    )
    for frame in joined_frames:
        ticket = frame["ticket"]
        submission = submission_by_ticket[ticket]
        terminal = ticket_ledger.terminals[ticket]
        presentation = presentation_by_ticket[ticket]
        integrity_require(
            frame["phase"]
            == terminal["phase"]
            == submission["phase"]
            == presentation["phase"],
            f"Q1 frame ticket {ticket} phase join mismatch",
        )
        integrity_require(
            frame["member_index"]
            == parse_uint(terminal["member_index"], f"Q1 frame ticket {ticket}.terminal member")
            == parse_uint(submission["member_index"], f"Q1 frame ticket {ticket}.submission member")
            == parse_uint(presentation["member_index"], f"Q1 frame ticket {ticket}.presentation member"),
            f"Q1 frame ticket {ticket} member join mismatch",
        )
        integrity_require(
            frame["trace_frame"]
            == parse_uint(terminal["trace_frame"], f"Q1 frame ticket {ticket}.terminal trace")
            == parse_uint(submission["trace_frame"], f"Q1 frame ticket {ticket}.submission trace")
            == parse_uint(presentation["trace_frame"], f"Q1 frame ticket {ticket}.presentation trace"),
            f"Q1 frame ticket {ticket} trace join mismatch",
        )
        integrity_require(
            frame["presentation_sequence"]
            == parse_uint(
                terminal["presentation_sequence"],
                f"Q1 frame ticket {ticket}.terminal presentation",
            )
            == parse_uint(
                submission["presentation_sequence"],
                f"Q1 frame ticket {ticket}.submission presentation",
            ),
            f"Q1 frame ticket {ticket} presentation sequence join mismatch",
        )
    for key, values in attempts.items():
        integrity_require(values == list(range(len(values))), f"Q1 member {key} attempts are not contiguous")
        integrity_require(
            presentation_by_ticket[
                parse_uint(submission_keys[key]["ticket"], f"Q1 member {key}.ticket", positive=True)
            ].get("attempt")
            == str(values[-1]),
            f"Q1 member {key} did not end with its issued presentation",
        )
    for phase, starts in cadence_starts.items():
        integrity_require(
            all(right - left >= CADENCE_NS for left, right in zip(starts, starts[1:])),
            f"Q1 {phase} host cadence exceeded 60 Hz",
        )

    drains = records["drain"]
    integrity_require(len(drains) == 4, "Q1 must retain begin/end for both drains")
    drain_by_key = {(item.get("phase"), item.get("event")): item for item in drains}
    integrity_require(
        set(drain_by_key)
        == {("warmup", "begin"), ("warmup", "end"), ("measure", "begin"), ("measure", "end")},
        "Q1 drain ledger is incomplete",
    )
    for key, drain in drain_by_key.items():
        integrity_require(drain.get("draws_during_drain") == "0", f"Q1 {key} added a drain draw")
    warmup_end = parse_uint(drain_by_key[("warmup", "end")]["end_ns"], "warmup drain end")
    measure_end = parse_uint(drain_by_key[("measure", "end")]["end_ns"], "measure drain end")
    first_measured_input = parse_uint(
        submission_keys[("measure", 0)]["member_first_input_ns"], "first measured input"
    )
    integrity_require(warmup_end <= first_measured_input, "Q1 measured trace began before warmup drain")

    timed_presentations = records["timed_presentation"]
    integrity_require(
        len(timed_presentations) == WARMUP + MEASURED,
        "Q1 timed presentation ledger length mismatch",
    )
    timed_presented_ns = [
        parse_uint(item.get("presented_ns", ""), f"timed_presentation[{index}].presented_ns")
        for index, item in enumerate(timed_presentations)
    ]
    integrity_require(
        all(left <= right for left, right in zip(timed_presented_ns, timed_presented_ns[1:])),
        "Q1 timed overall presentation timeline is not monotonic",
    )
    timed_by_phase: dict[str, list[dict[str, str]]] = {"warmup": [], "measure": []}
    previous_timed_revision = 0
    for index, presentation in enumerate(timed_presentations):
        context = f"timed_presentation[{index}]"
        phase = presentation.get("phase", "")
        integrity_require(phase in timed_by_phase, f"{context}.phase is invalid")
        member = parse_uint(presentation.get("member_index", ""), f"{context}.member")
        expected_member = len(timed_by_phase[phase])
        integrity_require(
            member == expected_member,
            f"{context}.member is not contiguous: expected={expected_member} actual={member}",
        )
        expected_count = WARMUP if phase == "warmup" else MEASURED
        integrity_require(member < expected_count, f"{context}.member is out of range")
        trace_frame = parse_uint(presentation.get("trace_frame", ""), f"{context}.trace_frame")
        integrity_require(
            trace_frame == expected_trace_frame(phase, member),
            f"{context}.trace frame drifted",
        )
        integrity_require(
            presentation.get("evidence_role") == "formal_terminal_throughput",
            f"{context}.evidence_role drifted",
        )
        integrity_require(presentation.get("frame_presented") == "true", f"{context} was dropped")
        integrity_require(
            presentation.get("sort_refreshed") == "true",
            f"{context} did not refresh sort",
        )
        integrity_require(
            presentation.get("actual_plan")
            in {"cpu_post_sort", "gpu_post_sort", "gpu_preproject"},
            f"{context}.actual_plan drifted",
        )
        validate_actual_plan_receipt(presentation, context)
        integrity_require(
            presentation.get("whole_plan_adaptive_state") in WHOLE_PLAN_ADAPTIVE_STATES,
            f"{context}.whole_plan_adaptive_state drifted",
        )
        integrity_require(
            presentation.get("projected_adaptive_state") in PROJECTED_ADAPTIVE_STATES,
            f"{context}.projected_adaptive_state drifted",
        )
        integrity_require(
            presentation.get("current_stats_requests") == "0"
            and presentation.get("current_stats_submission") == "not_requested",
            f"{context} carried current-stats observer load",
        )
        for forbidden in (
            "ticket",
            "source_count",
            "visible_count",
            "contributor_count",
            "drawn_count",
        ):
            integrity_require(
                forbidden not in presentation,
                f"{context} fabricated timed {forbidden} from control evidence",
            )
        revision = parse_uint(
            presentation.get("camera_revision", ""), f"{context}.camera_revision", positive=True
        )
        integrity_require(
            revision > previous_timed_revision,
            f"{context}.camera_revision is not strictly increasing",
        )
        previous_timed_revision = revision
        input_ns = parse_uint(presentation.get("input_ns", ""), f"{context}.input_ns")
        presented_ns = parse_uint(
            presentation.get("presented_ns", ""), f"{context}.presented_ns"
        )
        integrity_require(presented_ns >= input_ns, f"{context} presented before camera input")
        validate_camera(presentation, frames[trace_frame], context)
        timed_by_phase[phase].append(presentation)
    integrity_require(
        len(timed_by_phase["warmup"]) == WARMUP
        and len(timed_by_phase["measure"]) == MEASURED,
        "Q1 timed phase membership is incomplete",
    )
    first_timed_input = parse_uint(
        timed_by_phase["warmup"][0]["input_ns"], "first timed warmup input"
    )
    first_timed_revision = parse_uint(
        timed_by_phase["warmup"][0]["camera_revision"], "first timed camera revision"
    )
    last_control_revision = max(
        parse_uint(item["camera_revision"], "control camera revision")
        for (phase, _), item in submission_keys.items()
        if phase in {"warmup", "measure"}
    )
    integrity_require(
        measure_end <= first_timed_input,
        "Q1 timed stage overlapped the untimed control drain",
    )
    integrity_require(
        first_timed_revision > last_control_revision,
        "Q1 timed camera revision did not continue the control session identity",
    )
    for phase, phase_presentations in timed_by_phase.items():
        input_starts = [
            parse_uint(item["input_ns"], f"timed {phase} input")
            for item in phase_presentations
        ]
        phase_presented_ns = [
            parse_uint(item["presented_ns"], f"timed {phase} presentation")
            for item in phase_presentations
        ]
        integrity_require(
            all(right - left >= CADENCE_NS for left, right in zip(input_starts, input_starts[1:])),
            f"Q1 timed {phase} host cadence exceeded 60 Hz",
        )
        validate_phase_presentation_timeline(
            phase=phase,
            input_ns=input_starts,
            presented_ns=phase_presented_ns,
        )

    timed_drains = records["timed_drain"]
    integrity_require(
        len(timed_drains) == 4,
        "Q1 timed warmup/terminal drain ledger is incomplete",
    )
    timed_drain_by_key = {(item.get("phase"), item.get("event")): item for item in timed_drains}
    integrity_require(
        set(timed_drain_by_key)
        == {("warmup", "begin"), ("warmup", "end"), ("measure", "begin"), ("measure", "end")},
        "Q1 timed warmup/terminal drain ledger is incomplete or duplicated",
    )
    for key, drain in timed_drain_by_key.items():
        for field in (
            "draw_count",
            "draws_during_drain",
            "current_stats_count",
            "current_stats_requests",
            "current_stats_submissions",
            "capture_count",
            "capture_requests",
        ):
            integrity_require(
                drain.get(field) == "0",
                f"Q1 timed {key} {field} must be zero",
            )
        expected_completion = "pending" if key[1] == "begin" else "true"
        integrity_require(
            drain.get("queue_completion") == expected_completion,
            f"Q1 timed {key} queue-completion state drifted",
        )
        expected_submission_field = (
            ("warmup_submissions", str(WARMUP))
            if key[0] == "warmup"
            else ("measured_submissions", str(MEASURED))
        )
        integrity_require(
            drain.get(expected_submission_field[0]) == expected_submission_field[1],
            f"Q1 timed {key} presentation membership drifted",
        )
    for phase in ("warmup", "measure"):
        begin = timed_drain_by_key[(phase, "begin")]
        end = timed_drain_by_key[(phase, "end")]
        begin_ns = parse_uint(begin.get("start_ns", ""), f"timed {phase} drain begin")
        integrity_require(
            parse_uint(end.get("start_ns", ""), f"timed {phase} drain end start")
            == begin_ns,
            f"Q1 timed {phase} drain start receipt drifted",
        )
        integrity_require(
            parse_uint(end.get("end_ns", ""), f"timed {phase} drain end") >= begin_ns,
            f"Q1 timed {phase} drain is non-monotonic",
        )
    timed_first_input = parse_uint(
        timed_by_phase["measure"][0]["input_ns"], "timed first measured input"
    )
    timed_warmup_presented = [
        parse_uint(item["presented_ns"], "timed warmup presentation")
        for item in timed_by_phase["warmup"]
    ]
    timed_measured_presented = [
        parse_uint(item["presented_ns"], "timed measured presentation")
        for item in timed_by_phase["measure"]
    ]
    timed_warmup_drain_start = parse_uint(
        timed_drain_by_key[("warmup", "begin")]["start_ns"],
        "timed warmup drain start",
    )
    timed_warmup_drain_end = parse_uint(
        timed_drain_by_key[("warmup", "end")]["end_ns"],
        "timed warmup drain end",
    )
    timed_terminal_end = parse_uint(
        timed_drain_by_key[("measure", "end")]["end_ns"], "timed terminal drain end"
    )
    timed_measure_drain_start = parse_uint(
        timed_drain_by_key[("measure", "begin")]["start_ns"],
        "timed terminal drain start",
    )
    integrity_require(
        timed_warmup_drain_start >= max(timed_warmup_presented),
        "Q1 timed warmup drain began before warmup drawing stopped",
    )
    integrity_require(
        timed_measure_drain_start >= max(timed_measured_presented),
        "Q1 timed terminal drain began before measured drawing stopped",
    )
    timed_window_start_from_members, timed_window_end_from_members, timed_duration_from_members = (
        validate_terminal_window_bounds(
            expected_n=MEASURED,
            warmup_drain_end_ns=timed_warmup_drain_end,
            measured_input_ns=[
                parse_uint(item["input_ns"], "timed measured input")
                for item in timed_by_phase["measure"]
            ],
            measured_presented_ns=timed_measured_presented,
            terminal_end_ns=timed_terminal_end,
        )
    )
    integrity_require(
        timed_first_input == timed_window_start_from_members,
        "Q1 timed first measured input drifted",
    )

    integrity_require(timed_summary.get("status") == "ok", "Q1 timed summary is not successful")
    fixed_timed_summary = {
        "evidence_role": "formal_terminal_throughput",
        "clock": "std_instant_monotonic",
        "window_start": "first_measured_camera_input",
        "window_end": "measured_queue_completion",
        "n": str(MEASURED),
        "warmup_presentations": str(WARMUP),
        "measured_presentations": str(MEASURED),
        "current_stats_requests": "0",
        "current_stats_submissions": "0",
        "warmup_queue_drains": "1",
        "queue_drains": "1",
        "terminal_drain_draws": "0",
    }
    for field, expected in fixed_timed_summary.items():
        integrity_require(
            timed_summary.get(field) == expected,
            f"Q1 timed summary {field} drifted",
        )
    timed_window_start = parse_uint(
        timed_summary.get("window_start_ns", ""), "timed_summary.window_start"
    )
    timed_window_end = parse_uint(
        timed_summary.get("window_end_ns", ""), "timed_summary.window_end"
    )
    timed_window_duration = parse_uint(
        timed_summary.get("window_duration_ns", ""),
        "timed_summary.duration",
        positive=True,
    )
    terminal_fps = parse_float(timed_summary.get("terminal_fps", ""), "timed_summary.fps")
    integrity_require(
        timed_window_start == timed_window_start_from_members,
        "Q1 timed terminal window start drifted",
    )
    integrity_require(
        timed_window_end == timed_window_end_from_members,
        "Q1 timed terminal window end drifted",
    )
    integrity_require(
        timed_window_duration == timed_duration_from_members,
        "Q1 timed terminal window duration drifted",
    )
    integrity_require(
        math.isclose(
            terminal_fps,
            MEASURED * 1_000_000_000.0 / timed_window_duration,
            rel_tol=1e-9,
        ),
        "Q1 terminal FPS is not derived from N/window",
    )
    timed_plan_set = set(timed_summary.get("actual_plan_set", "").split(","))
    integrity_require(
        timed_plan_set
        and timed_plan_set <= {"cpu_post_sort", "gpu_post_sort", "gpu_preproject"},
        "Q1 timed actual plan set drifted",
    )
    integrity_require(
        timed_plan_set == {item["actual_plan"] for item in timed_by_phase["measure"]},
        "Q1 timed actual plan summary does not match measured presentations",
    )
    measured_whole_plan_states = {
        item["whole_plan_adaptive_state"] for item in timed_by_phase["measure"]
    }
    measured_projected_states = {
        item["projected_adaptive_state"] for item in timed_by_phase["measure"]
    }
    measured_projected_executions = {
        item["projected_draw_execution"] for item in timed_by_phase["measure"]
    }
    integrity_require(
        set(timed_summary.get("whole_plan_adaptive_state_set", "").split(","))
        == measured_whole_plan_states,
        "Q1 timed whole-plan Adaptive state summary drifted",
    )
    integrity_require(
        set(timed_summary.get("projected_adaptive_state_set", "").split(","))
        == measured_projected_states,
        "Q1 timed projected Adaptive state summary drifted",
    )
    integrity_require(
        set(timed_summary.get("projected_execution_set", "").split(","))
        == measured_projected_executions,
        "Q1 timed projected execution summary drifted",
    )

    capture_receipts: list[dict[str, Any]] = []
    for index, capture in enumerate(captures):
        context = f"capture[{index}]"
        integrity_require(capture.get("status") == "ok", f"{context} is not successful")
        integrity_require(
            capture.get("evidence_role") == "post_timing_capture_control",
            f"{context}.evidence_role drifted",
        )
        capture_index = parse_uint(capture.get("capture_index", ""), f"{context}.index")
        integrity_require(capture_index == index, f"{context}.index drifted")
        integrity_require(capture.get("trace_frame") == str(index), f"{context}.trace frame drifted")
        ticket = parse_uint(capture.get("ticket", ""), f"{context}.ticket", positive=True)
        submission = submission_by_ticket.get(ticket)
        integrity_require(submission is not None, f"{context} has unknown ticket {ticket}")
        integrity_require(submission.get("phase") == "capture", f"{context} ticket is not a capture")
        integrity_require(
            parse_uint(submission.get("camera_revision", ""), f"{context}.camera_revision")
            > previous_timed_revision,
            f"{context} camera revision did not continue the timed session identity",
        )
        integrity_require(capture.get("camera_revision") == submission.get("camera_revision"), f"{context} camera join mismatch")
        integrity_require(capture.get("presentation_sequence") == submission.get("presentation_sequence"), f"{context} presentation join mismatch")
        integrity_require(capture.get("terminal_receipt") == "ready", f"{context} lacks Ready terminal")
        integrity_require(
            (capture.get("width"), capture.get("height")) == ("1920", "1080"),
            f"{context} dimensions drifted",
        )
        for matrix_field in (
            "view_matrix",
            "projection_matrix",
            "view_projection_matrix",
        ):
            integrity_require(
                capture.get(matrix_field) == submission.get(matrix_field),
                f"{context} {matrix_field} join mismatch",
            )
        terminal_ns = parse_uint(capture.get("terminal_observed_ns", ""), f"{context}.terminal")
        integrity_require(
            terminal_ns >= timed_window_end,
            f"{context} was captured before the timed queue completion",
        )
        capture_record: dict[str, Any] = dict(capture)
        if run_dir is not None:
            path = (run_dir / require_string(capture.get("path"), f"{context}.path")).resolve()
            integrity_require(path.is_relative_to(run_dir.resolve()), f"{context} escaped run directory")
            integrity_require(path.is_file(), f"{context} PNG is unavailable")
            integrity_require(SURFACE_EVIDENCE.png_dimensions(path) == FORMAL_SIZE, f"{context} PNG size drifted")
            capture_record.update(
                {
                    "path": str(path.relative_to(run_dir.resolve())),
                    "sha256": sha256_file(path),
                    "bytes": path.stat().st_size,
                }
            )
        capture_receipts.append(capture_record)

    integrity_require(
        control_summary.get("status") == "ok", "Q1 control summary is not successful"
    )
    fixed_control_summary = {
        "evidence_role": "untimed_correctness_control",
        "observer_load": "current_stats_every_member",
        "timing_eligible": "false",
        "throughput_n": "null",
        "throughput_fps": "null",
        "clock": "std_instant_monotonic",
        "control_start": "first_control_measured_camera_input",
        "control_end": "last_control_measured_current_stats_terminal",
        "control_measured_members": str(MEASURED),
        "warmup_issued": str(WARMUP),
        "measured_issued": str(MEASURED),
        "capture_issued": "2",
        "total_issued": str(WARMUP + MEASURED + 2),
        "total_terminals": str(WARMUP + MEASURED + 2),
        "capture_count": "2",
        "drain_draws": "0",
    }
    for field, expected in fixed_control_summary.items():
        integrity_require(
            control_summary.get(field) == expected,
            f"Q1 control summary {field} drifted",
        )
    control_start = parse_uint(
        control_summary.get("control_start_ns", ""), "control_summary.start"
    )
    control_end = parse_uint(
        control_summary.get("control_end_ns", ""), "control_summary.end"
    )
    control_duration = parse_uint(
        control_summary.get("control_duration_ns", ""),
        "control_summary.duration",
        positive=True,
    )
    integrity_require(
        control_start == first_measured_input,
        "Q1 control interval start drifted",
    )
    integrity_require(
        control_end == max(measured_terminal_observed),
        "Q1 control interval end drifted",
    )
    integrity_require(
        control_end - control_start == control_duration,
        "Q1 control interval duration drifted",
    )
    integrity_require(
        measure_end == control_end,
        "Q1 control measured drain did not end at its last terminal",
    )
    integrity_require(
        parse_uint(
            control_summary.get("total_presentations", ""),
            "control_summary.presentations",
        )
        == len(presentations),
        "Q1 presentation count drifted",
    )
    integrity_require(
        parse_uint(
            control_summary.get("auxiliary_presentations", ""),
            "control_summary.auxiliary",
        )
        == sum(item.get("submission") == "not_requested" for item in presentations),
        "Q1 auxiliary presentation count drifted",
    )
    plan_set = set(control_summary.get("actual_plan_set", "").split(","))
    integrity_require(
        plan_set and plan_set <= {"cpu_post_sort", "gpu_post_sort", "gpu_preproject"},
        "Q1 actual plan set drifted",
    )
    integrity_require(
        plan_set == {item["executed_plan"] for item in submissions},
        "Q1 control actual plan summary does not match submissions",
    )
    integrity_require(
        set(control_summary.get("projected_adaptive_state_set", "").split(","))
        == {item["projected_adaptive_state"] for item in presentations},
        "Q1 control projected Adaptive state summary drifted",
    )
    integrity_require(
        set(control_summary.get("projected_execution_set", "").split(","))
        == {item["projected_draw_execution"] for item in presentations},
        "Q1 control projected execution summary drifted",
    )

    return {
        "begin": begin,
        "control_presentations": presentations,
        "timed_presentations": timed_presentations,
        "timed_drains": timed_drains,
        "submissions": submissions,
        "terminals": terminals,
        "frames": joined_frames,
        "captures": capture_receipts,
        "summary": {
            "schema": SCHEMA,
            "control": {
                **control_summary,
                "control_start_ns": control_start,
                "control_end_ns": control_end,
                "control_duration_ns": control_duration,
                "control_measured_members": MEASURED,
                "throughput_n": None,
                "throughput_fps": None,
            },
            "terminal_throughput": {
                **timed_summary,
                "window_start_ns": timed_window_start,
                "window_end_ns": timed_window_end,
                "window_duration_ns": timed_window_duration,
                "n": MEASURED,
                "terminal_fps": terminal_fps,
            },
        },
    }


def build_host(repo: Path, output: Path, expected_git: dict[str, Any]) -> dict[str, Any]:
    return SURFACE_EVIDENCE.build_locked_desktop_binary(
        repo,
        output,
        expected_git,
        feature=FEATURE,
        build_jobs=1,
    )


def make_command(binary: Path, workload: dict[str, Any]) -> list[str]:
    return [
        str(binary),
        str(workload["dataset_path"]),
        "--geometry-path", "packed",
        "--interactive",
        "--camera-trace", str(workload["trace_path"]),
        "--camera-sequence",
        "--camera-frame-indices", "0,1",
        "--camera-warmup-frames", str(WARMUP),
        "--camera-measured-frames", str(MEASURED),
        "--camera-loops", "1",
        "--surface-benchmark-mode", "throughput",
        "--surface-sort-policy", "every-frame",
        "--order-backend", "adaptive",
        "--surface-q1-m4-native",
        "--png", "capture.png",
    ]


def failure_artifact(result: dict[str, Any]) -> dict[str, Any]:
    """Return a bounded terminal payload with no Accepted evidence claim."""

    return {
        key: result[key]
        for key in (
            "schema",
            "cell",
            "native_prerequisite",
            "q1_state",
            "reason",
            "started_at_utc",
            "ended_at_utc",
            "build",
            "host",
            "failure_phase",
            "error",
            "deferred_boundaries",
        )
        if key in result
    }


def collect(args: argparse.Namespace, repo: Path = REPO_ROOT) -> dict[str, Any]:
    output = args.output.resolve()
    require(not output.exists(), f"Q1 native output already exists: {output}")
    validate_ignored_output(repo, output)
    initial_git = git_receipt(repo)
    require(not initial_git["dirty"], "Q1 native collection requires a clean exact commit")
    output.parent.mkdir(parents=True, exist_ok=True)
    result: dict[str, Any] = {
        "schema": SCHEMA,
        "cell": CELL,
        "native_prerequisite": "Deferred",
        "q1_state": "Deferred",
        "reason": "environment_prerequisite",
        "started_at_utc": utc_now(),
        "build": initial_git,
        "host": None,
        "deferred_boundaries": list(DEFERRED_BOUNDARIES),
        "config": {
            "geometry_path": "packed_atlas",
            "raster_execution_plan": "projected_quads_exact",
            "order_backend": "adaptive",
            "projected_draw_policy": "adaptive",
            "sort_policy": "every_frame",
            "host_cadence_hz": 60,
            "cadence_ns": CADENCE_NS,
            "control": {
                "evidence_role": "untimed_correctness_control",
                "warmup_frames": WARMUP,
                "measured_frames": MEASURED,
                "current_stats": "every_member",
            },
            "terminal_throughput": {
                "evidence_role": "formal_terminal_throughput",
                "warmup_frames": WARMUP,
                "measured_frames": MEASURED,
                "current_stats_requests": 0,
                "warmup_completion": "surface_runtime_complete_queue_before_measurement",
                "terminal_owner": "surface_session_pump_receipts",
            },
            "capture_trace_frames": [0, 1],
            "run_attempts": 1,
            "automatic_retry": False,
        },
    }
    phase = CollectionPhase.PREFLIGHT
    transaction: Any | None = None
    try:
        transaction = SURFACE_EVIDENCE.ImmutableOutputTransaction(output)
        stage = transaction.staging
        host = host_receipt()
        result["host"] = host
        if (
            host["system"] != "Darwin"
            or host["machine"] != "arm64"
            or "M4" not in host["cpu_brand"]
        ):
            raise EnvironmentPrerequisiteError(
                f"Q1 native endpoint requires Apple M4, got {host}"
            )
        workload = load_workload(repo)
        result["workload"] = {
            "dataset": workload["dataset"],
            "trace_id": workload["trace"]["trace_id"],
            "trace_content_sha256": workload["trace"]["content_sha256"],
            "trace_file_sha256": workload["trace_file_sha256"],
            "trace_bytes": workload["trace_path"].stat().st_size,
        }
        phase = CollectionPhase.BUILD_STARTED
        binary = build_host(repo, stage, initial_git)
        result["private_host_build"] = {key: value for key, value in binary.items() if key != "path"}
        run_dir = stage / "run"
        run_dir.mkdir()
        command = make_command(binary["path"], workload)
        write_json(
            run_dir / "command.json",
            {"argv": command, "cwd": "run", "environment": {"WGPU_BACKEND": "metal"}},
        )
        environment = os.environ.copy()
        environment["WGPU_BACKEND"] = "metal"
        phase = CollectionPhase.HOST_STARTED
        try:
            completed = subprocess.run(
                command,
                cwd=run_dir,
                env=environment,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=RUN_TIMEOUT_SECONDS,
            )
        except subprocess.TimeoutExpired as error:
            timed_out_stdout = timeout_stream(error.stdout)
            timed_out_stderr = timeout_stream(error.stderr)
            (run_dir / "stdout.log").write_text(timed_out_stdout, encoding="utf-8")
            (run_dir / "stderr.log").write_text(timed_out_stderr, encoding="utf-8")
            entered_protocol = any(
                line.startswith(PREFIXES["begin"])
                for stream in (timed_out_stdout, timed_out_stderr)
                for line in stream.splitlines()
            )
            if entered_protocol or timed_out_stdout or timed_out_stderr:
                phase = CollectionPhase.PROTOCOL_STARTED
            if entered_protocol:
                raise IntegrityRejectedError(
                    f"private host timed out after evidence protocol at {RUN_TIMEOUT_SECONDS}s"
                ) from error
            raise EnvironmentPrerequisiteError(
                f"private host timed out before evidence protocol at {RUN_TIMEOUT_SECONDS}s"
            ) from error
        (run_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
        (run_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
        entered_protocol = any(
            line.startswith(PREFIXES["begin"])
            for stream in (completed.stdout, completed.stderr)
            for line in stream.splitlines()
        )
        if entered_protocol or completed.stdout or completed.stderr:
            phase = CollectionPhase.PROTOCOL_STARTED
        if completed.returncode != 0:
            if not entered_protocol:
                raise EnvironmentPrerequisiteError(
                    f"private host exited with {completed.returncode} before evidence protocol"
                )
            raise IntegrityRejectedError(
                f"private host exited with {completed.returncode} after evidence protocol"
            )
        integrity_require(sha256_file(binary["path"]) == binary["sha256"], "private host binary changed")
        integrity_require(sha256_file(workload["dataset_path"]) == TRUCK["sha256"], "Truck changed during run")
        integrity_require(sha256_file(workload["trace_path"]) == TRACE["file_sha256"], "trace changed during run")
        integrity_require(git_receipt(repo) == initial_git, "git receipt changed during Q1 run")
        validated = validate_run_log(
            completed.stdout,
            completed.stderr,
            workload=workload,
            run_dir=run_dir,
        )
        phase_binding: dict[str, Any] = {
            "git_sha": initial_git["commit"],
            "binary_sha256": binary["sha256"],
            "dataset_sha256": workload["dataset"]["sha256"],
            "trace_file_sha256": workload["trace_file_sha256"],
            "trace_content_sha256": workload["trace"]["content_sha256"],
            "trace_id": workload["trace"]["trace_id"],
            "host_begin": validated["begin"],
            "config": result["config"],
            "stages": ["untimed_current_stats_control", "terminal_throughput"],
            "same_process_invocation": True,
        }
        phase_binding["sha256"] = hashlib.sha256(
            json.dumps(phase_binding, sort_keys=True, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        validated["summary"]["phase_binding_sha256"] = phase_binding["sha256"]
        write_jsonl(
            run_dir / "control-presentations.jsonl", validated["control_presentations"]
        )
        write_jsonl(run_dir / "timed-presentations.jsonl", validated["timed_presentations"])
        write_jsonl(run_dir / "timed-drains.jsonl", validated["timed_drains"])
        write_jsonl(run_dir / "control-submissions.jsonl", validated["submissions"])
        write_jsonl(run_dir / "control-terminals.jsonl", validated["terminals"])
        write_jsonl(run_dir / "control-current-stats-frames.jsonl", validated["frames"])
        write_json(run_dir / "captures.json", validated["captures"])
        write_json(run_dir / "summary.json", validated["summary"])
        write_json(run_dir / "phase-binding.json", phase_binding)
        manifest = {
            "schema": SCHEMA,
            "cell": CELL,
            "build": initial_git,
            "binary_sha256": binary["sha256"],
            "workload": result["workload"],
            "host": host,
            "config": result["config"],
            "runtime_log": "stdout.log",
            "phase_binding": "phase-binding.json",
            "control_presentations": "control-presentations.jsonl",
            "timed_presentations": "timed-presentations.jsonl",
            "timed_drains": "timed-drains.jsonl",
            "control_submissions": "control-submissions.jsonl",
            "control_terminals": "control-terminals.jsonl",
            "control_current_stats_frames": "control-current-stats-frames.jsonl",
            "captures": "captures.json",
            "summary": "summary.json",
            "native_prerequisite": "Accepted",
            "q1_state": "Deferred",
            "deferred_boundaries": list(DEFERRED_BOUNDARIES),
        }
        write_json(run_dir / "manifest.json", manifest)
        result.update(
            {
                "native_prerequisite": "Accepted",
                "q1_state": "Deferred",
                "reason": "native_control_and_terminal_throughput_prerequisite_only",
                "run": {
                    "command": "run/command.json",
                    "stdout": "run/stdout.log",
                    "stderr": "run/stderr.log",
                    "manifest": "run/manifest.json",
                    "summary": validated["summary"],
                    "phase_binding": phase_binding,
                    "captures": validated["captures"],
                },
            }
        )
        phase = CollectionPhase.FINALIZATION_STARTED
        result["ended_at_utc"] = utc_now()
        result["publication"] = "immutable_atomic_root"
        transaction.publish_result(result)
        return result
    except EnvironmentPrerequisiteError as error:
        native_prerequisite, reason = classify_failure(error, phase)
        result.update(
            {
                "native_prerequisite": native_prerequisite,
                "reason": reason,
                "failure_phase": phase.value,
                "error": str(error),
            }
        )
    except Exception as error:
        result.update(
            {
                "native_prerequisite": "Rejected",
                "reason": "integrity_rejected",
                "failure_phase": phase.value,
                "error": str(error),
            }
        )
    result["ended_at_utc"] = utc_now()
    terminal = failure_artifact(result)
    if transaction is None:
        result["publication"] = "blocked"
        result["publication_blocker"] = "artifact transaction could not be created"
        return result
    try:
        transaction.replace_with_failure(terminal)
        result["publication"] = "immutable_non_success_root"
    except Exception as publication_error:
        try:
            transaction.discard()
        except Exception as discard_error:
            result["discard_blocker"] = str(discard_error)
        result["native_prerequisite"] = "Rejected"
        result["reason"] = "finalization_rejected"
        result["failure_phase"] = CollectionPhase.FINALIZATION_STARTED.value
        result["publication"] = "blocked"
        result["publication_blocker"] = str(publication_error)
    return result


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Collect one Q1 M4 native control plus terminal-throughput prerequisite"
    )
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        result = collect(args)
    except (
        OSError,
        subprocess.SubprocessError,
        ValidationError,
        SURFACE_EVIDENCE.ValidationError,
        ValueError,
    ) as error:
        print(f"Q1 M4 native collection failed before output claim: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "cell": result["cell"],
                "native_prerequisite": result["native_prerequisite"],
                "q1_state": result["q1_state"],
                "output": str(args.output),
                "publication": result.get("publication", "blocked"),
                "failure_phase": result.get("failure_phase"),
                "publication_blocker": result.get("publication_blocker"),
            },
            sort_keys=True,
        )
    )
    return 0 if result["native_prerequisite"] == "Accepted" else 1


if __name__ == "__main__":
    raise SystemExit(main())
