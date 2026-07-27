#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from typing import Any
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
COLLECTOR_PATH = REPO_ROOT / "tests/perf/collect-q1-m4-native.py"


def load_collector() -> Any:
    spec = importlib.util.spec_from_file_location("collect_q1_m4_native_for_test", COLLECTOR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {COLLECTOR_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


COLLECTOR = load_collector()


def payload(prefix: str, values: dict[str, Any]) -> str:
    parts = []
    for key, value in values.items():
        if isinstance(value, bool):
            value = str(value).lower()
        if isinstance(value, str) and " " in value:
            value = json.dumps(value)
        parts.append(f"{key}={value}")
    return prefix + " ".join(parts)


def matching_line(stdout: str, prefix: str, *fields: str) -> str:
    return next(
        line
        for line in stdout.splitlines()
        if line.startswith(prefix) and all(field in line.split() for field in fields)
    )


def replace_field(line: str, field: str, value: int) -> str:
    token = next(item for item in line.split() if item.startswith(f"{field}="))
    return line.replace(token, f"{field}={value}", 1)


def floats(values: list[float]) -> str:
    return ",".join(f"{value:.9f}" for value in values)


def camera_fields(frame: dict[str, Any]) -> dict[str, str]:
    position = frame["pose"]["position"]
    rotation = frame["pose"]["rotation_xyzw"]
    intrinsics = frame["intrinsics"]
    aspect = 1920.0 / 1080.0
    view = COLLECTOR.live_view_matrix(position, rotation)
    projection = COLLECTOR.live_projection_matrix(
        intrinsics["vertical_fov_radians"],
        intrinsics["near_plane"],
        intrinsics["far_plane"],
        aspect,
    )
    return {
        "position": floats(position),
        "rotation_xyzw": floats(rotation),
        "vertical_fov_radians": f"{intrinsics['vertical_fov_radians']:.9f}",
        "near_plane": f"{intrinsics['near_plane']:.9f}",
        "far_plane": f"{intrinsics['far_plane']:.9f}",
        "aspect": f"{aspect:.9f}",
        "view_matrix": floats(view),
        "projection_matrix": floats(projection),
        "view_projection_matrix": floats(COLLECTOR.multiply_mat4(projection, view)),
    }


def fixture_log() -> tuple[str, dict[str, Any]]:
    trace_path = REPO_ROOT / COLLECTOR.TRACE["local_path"]
    trace = json.loads(trace_path.read_text(encoding="utf-8"))
    workload = {"trace": trace}
    lines = [
        payload(
            COLLECTOR.PREFIXES["begin"],
            {
                "schema": COLLECTOR.SCHEMA,
                "stages": "untimed_current_stats_control,terminal_throughput",
                "shared_process_identity": True,
                "trace_id": COLLECTOR.TRACE["id"],
                "trace_sha256": COLLECTOR.TRACE["content_sha256"],
                "geometry_path": "packed_atlas",
                "raster_execution_plan": "projected_quads_exact",
                "order_backend": "adaptive",
                "projected_draw_policy": "adaptive",
                "sort_policy": "every_frame",
                "sort_interval": 1,
                "host_cadence_hz": 60,
                "cadence_ns": COLLECTOR.CADENCE_NS,
                "control_warmup_frames": COLLECTOR.WARMUP,
                "control_measured_frames": COLLECTOR.MEASURED,
                "timed_warmup_frames": COLLECTOR.WARMUP,
                "timed_measured_frames": COLLECTOR.MEASURED,
                "capture_trace_frames": "0,1",
                "source_membership": "all",
                "sampling": "disabled",
                "lod": "disabled",
                "dynamic_resolution": "disabled",
                "upscaling": "disabled",
                "source_count": COLLECTOR.TRUCK["splat_count"],
                "decoded_count": COLLECTOR.TRUCK["splat_count"],
                "encoded_count": COLLECTOR.TRUCK["splat_count"],
                "resident_count": COLLECTOR.TRUCK["splat_count"],
                "addressable_count": COLLECTOR.TRUCK["splat_count"],
                "sh_degree": 3,
                "requested_width": 1920,
                "requested_height": 1080,
                "surface_width": 1920,
                "surface_height": 1080,
                "internal_render_width": 1920,
                "internal_render_height": 1080,
                "adapter_backend": "metal",
                "adapter_name": "Apple M4",
            },
        )
    ]
    members: list[dict[str, Any]] = []
    ticket = 0
    for phase, count, input_base in (
        ("warmup", COLLECTOR.WARMUP, 10_000_000),
        ("measure", COLLECTOR.MEASURED, 1_000_000_000),
        ("capture", 2, 7_000_000_000),
    ):
        for member in range(count):
            ticket += 1
            trace_frame = COLLECTOR.expected_trace_frame(phase, member)
            input_ns = input_base + member * COLLECTOR.CADENCE_NS
            executed_plan = ("cpu_post_sort", "gpu_post_sort", "gpu_preproject")[member % 3]
            camera_revision = 201 + member if phase == "capture" else ticket
            common = {
                "evidence_role": "untimed_correctness_control",
                "phase": phase,
                "member_index": member,
                "trace_frame": trace_frame,
                "trace_timestamp_ns": trace["frames"][trace_frame]["timestamp_ns"],
                "ticket": ticket,
                "executed_plan": executed_plan,
                "scene_generation": 1,
                "camera_revision": camera_revision,
                "viewport_generation": 1,
                "contract_generation": 1,
                "plan_set_generation": 1,
                "order_generation": ticket,
                "raster_generation": 1,
                "encode_attempt": ticket,
                "presentation_sequence": ticket,
            }
            lines.append(
                payload(
                    COLLECTOR.PREFIXES["presentation"],
                    {
                        "evidence_role": "untimed_correctness_control",
                        "observer_load": "current_stats",
                        "phase": phase,
                        "member_index": member,
                        "attempt": 0,
                        "member_first_input_ns": input_ns,
                        "cadence_start_ns": input_ns,
                        "input_ns": input_ns,
                        "trace_frame": trace_frame,
                        "trace_timestamp_ns": trace["frames"][trace_frame]["timestamp_ns"],
                        "request_state": "requested",
                        "submission": "issued",
                        "ticket": ticket,
                        "camera_revision": camera_revision,
                        "frame_presented": True,
                        "sort_refreshed": True,
                        "order_uploaded": executed_plan == "cpu_post_sort",
                        "actual_backend": "cpu" if executed_plan == "cpu_post_sort" else "gpu",
                        "whole_plan_adaptive_state": (
                            "cpu_learning" if executed_plan == "cpu_post_sort" else "gpu_probe"
                        ),
                        "projected_adaptive_state": "disabled",
                        "projected_draw_execution": (
                            "compact" if executed_plan == "gpu_preproject" else "candidate"
                        ),
                        "frame_wall_ms": "16.0",
                    },
                )
            )
            submission = {
                **common,
                "member_first_input_ns": input_ns,
                "issued_ns": input_ns + 1_000_000,
                "actual_backend": "cpu" if executed_plan == "cpu_post_sort" else "gpu",
                **camera_fields(trace["frames"][trace_frame]),
                "frame_presented": True,
            }
            lines.append(payload(COLLECTOR.PREFIXES["submission"], submission))
            members.append({"common": common, "input_ns": input_ns})

    warmup_end = 400_000_000
    measured_terminal_start = 1_050_000_000
    measured_terminal_end = measured_terminal_start + (COLLECTOR.MEASURED - 1) * COLLECTOR.CADENCE_NS
    lines.extend(
        [
            payload(
                COLLECTOR.PREFIXES["drain"],
                {"phase": "warmup", "event": "begin", "start_ns": 350_000_000, "issued_tickets": 20, "draws_during_drain": 0},
            ),
            payload(
                COLLECTOR.PREFIXES["drain"],
                {"phase": "warmup", "event": "end", "start_ns": 350_000_000, "end_ns": warmup_end, "draws_during_drain": 0, "terminal_tickets": 20},
            ),
            payload(
                COLLECTOR.PREFIXES["drain"],
                {"phase": "measure", "event": "begin", "start_ns": 2_340_000_000, "issued_tickets": 80, "draws_during_drain": 0},
            ),
        ]
    )
    for item in members:
        common = item["common"]
        phase = common["phase"]
        member = common["member_index"]
        if phase == "warmup":
            observed_ns = 100_000_000 + member * 10_000_000
        elif phase == "measure":
            observed_ns = measured_terminal_start + member * COLLECTOR.CADENCE_NS
        else:
            observed_ns = 7_100_000_000 + member * 100_000_000
        terminal = {
            "status": "ready",
            **common,
            "observed_ns": observed_ns,
            "count_semantics": (
                "indirect_draw_equals_contributor"
                if common["executed_plan"] == "gpu_preproject"
                else "indirect_draw_equals_visible"
            ),
            "source_count": COLLECTOR.TRUCK["splat_count"],
            "visible_count": 2_000_000,
            "contributor_count": 1_900_000,
            "drawn_count": (
                1_900_000 if common["executed_plan"] == "gpu_preproject" else 2_000_000
            ),
            "exact_contributor_compaction": common["executed_plan"] == "gpu_preproject",
        }
        lines.append(payload(COLLECTOR.PREFIXES["terminal"], terminal))
        if phase == "capture":
            lines.append(
                payload(
                    COLLECTOR.PREFIXES["capture"],
                    {
                        "status": "ok",
                        "evidence_role": "post_timing_capture_control",
                        "capture_index": member,
                        "trace_frame": member,
                        "path": f"capture.view-{member}-trace-{member}.png",
                        "ticket": common["ticket"],
                        "camera_revision": common["camera_revision"],
                        "presentation_sequence": common["presentation_sequence"],
                        "terminal_observed_ns": observed_ns,
                        "terminal_receipt": "ready",
                        "width": 1920,
                        "height": 1080,
                        "view_matrix": camera_fields(trace["frames"][member])["view_matrix"],
                        "projection_matrix": camera_fields(trace["frames"][member])["projection_matrix"],
                        "view_projection_matrix": camera_fields(trace["frames"][member])["view_projection_matrix"],
                    },
                )
            )
    lines.append(
        payload(
            COLLECTOR.PREFIXES["drain"],
            {"phase": "measure", "event": "end", "start_ns": 2_340_000_000, "end_ns": measured_terminal_end, "draws_during_drain": 0, "terminal_tickets": 80},
        )
    )
    timed_warmup_base = 3_000_000_000
    timed_warmup_drain_start = (
        timed_warmup_base
        + (COLLECTOR.WARMUP - 1) * COLLECTOR.CADENCE_NS
        + 5_000_000
    )
    timed_warmup_drain_end = timed_warmup_drain_start + 50_000_000
    timed_measure_base = timed_warmup_drain_end + COLLECTOR.CADENCE_NS
    timed_revision = 100
    for phase, count, input_base in (
        ("warmup", COLLECTOR.WARMUP, timed_warmup_base),
        ("measure", COLLECTOR.MEASURED, timed_measure_base),
    ):
        for member in range(count):
            timed_revision += 1
            trace_frame = COLLECTOR.expected_trace_frame(phase, member)
            input_ns = input_base + member * COLLECTOR.CADENCE_NS
            executed_plan = ("cpu_post_sort", "gpu_post_sort", "gpu_preproject")[member % 3]
            lines.append(
                payload(
                    COLLECTOR.PREFIXES["timed_presentation"],
                    {
                        "evidence_role": "formal_terminal_throughput",
                        "phase": phase,
                        "member_index": member,
                        "input_ns": input_ns,
                        "presented_ns": input_ns + 5_000_000,
                        "trace_frame": trace_frame,
                        "trace_timestamp_ns": trace["frames"][trace_frame]["timestamp_ns"],
                        "camera_revision": timed_revision,
                        "frame_presented": True,
                        "sort_refreshed": True,
                        "order_uploaded": executed_plan == "cpu_post_sort",
                        "actual_plan": executed_plan,
                        "actual_backend": "cpu" if executed_plan == "cpu_post_sort" else "gpu",
                        "whole_plan_adaptive_state": (
                            "cpu_learning" if executed_plan == "cpu_post_sort" else "gpu_probe"
                        ),
                        "projected_adaptive_state": "disabled",
                        "projected_draw_execution": (
                            "compact" if executed_plan == "gpu_preproject" else "candidate"
                        ),
                        "current_stats_requests": 0,
                        "current_stats_submission": "not_requested",
                        **camera_fields(trace["frames"][trace_frame]),
                        "frame_wall_ms": "5.0",
                    },
                )
            )
        if phase == "warmup":
            for event, values in (
                (
                    "begin",
                    {
                        "start_ns": timed_warmup_drain_start,
                        "queue_completion": "pending",
                    },
                ),
                (
                    "end",
                    {
                        "start_ns": timed_warmup_drain_start,
                        "end_ns": timed_warmup_drain_end,
                        "queue_completion": True,
                    },
                ),
            ):
                lines.append(
                    payload(
                        COLLECTOR.PREFIXES["timed_drain"],
                        {
                            "phase": "warmup",
                            "event": event,
                            **values,
                            "draw_count": 0,
                            "draws_during_drain": 0,
                            "current_stats_count": 0,
                            "current_stats_requests": 0,
                            "current_stats_submissions": 0,
                            "capture_count": 0,
                            "capture_requests": 0,
                            "warmup_submissions": COLLECTOR.WARMUP,
                        },
                    )
                )
    timed_measure_drain_start = timed_measure_base + COLLECTOR.MEASURED * COLLECTOR.CADENCE_NS
    timed_measure_drain_end = timed_measure_drain_start + 50_000_000
    for phase, start_ns, end_ns, measured_submissions in (
        ("measure", timed_measure_drain_start, timed_measure_drain_end, COLLECTOR.MEASURED),
    ):
        lines.append(
            payload(
                COLLECTOR.PREFIXES["timed_drain"],
                {
                    "phase": phase,
                    "event": "begin",
                    "start_ns": start_ns,
                    "queue_completion": "pending",
                    "draw_count": 0,
                    "draws_during_drain": 0,
                    "current_stats_count": 0,
                    "current_stats_requests": 0,
                    "current_stats_submissions": 0,
                    "capture_count": 0,
                    "capture_requests": 0,
                    "measured_submissions": measured_submissions,
                },
            )
        )
        lines.append(
            payload(
                COLLECTOR.PREFIXES["timed_drain"],
                {
                    "phase": phase,
                    "event": "end",
                    "start_ns": start_ns,
                    "end_ns": end_ns,
                    "queue_completion": True,
                    "draw_count": 0,
                    "draws_during_drain": 0,
                    "current_stats_count": 0,
                    "current_stats_requests": 0,
                    "current_stats_submissions": 0,
                    "capture_count": 0,
                    "capture_requests": 0,
                    "measured_submissions": measured_submissions,
                },
            )
        )
    timed_duration = timed_measure_drain_end - timed_measure_base
    lines.append(
        payload(
            COLLECTOR.PREFIXES["timed_summary"],
            {
                "status": "ok",
                "evidence_role": "formal_terminal_throughput",
                "clock": "std_instant_monotonic",
                "window_start": "first_measured_camera_input",
                "window_end": "measured_queue_completion",
                "window_start_ns": timed_measure_base,
                "window_end_ns": timed_measure_drain_end,
                "window_duration_ns": timed_duration,
                "n": COLLECTOR.MEASURED,
                "terminal_fps": f"{COLLECTOR.MEASURED * 1_000_000_000.0 / timed_duration:.9f}",
                "warmup_presentations": COLLECTOR.WARMUP,
                "measured_presentations": COLLECTOR.MEASURED,
                "current_stats_requests": 0,
                "current_stats_submissions": 0,
                "warmup_queue_drains": 1,
                "queue_drains": 1,
                "terminal_drain_draws": 0,
                "actual_plan_set": "cpu_post_sort,gpu_post_sort,gpu_preproject",
                "whole_plan_adaptive_state_set": "cpu_learning,gpu_probe",
                "projected_adaptive_state_set": "disabled",
                "projected_execution_set": "candidate,compact",
            },
        )
    )
    control_start = 1_000_000_000
    control_duration = measured_terminal_end - control_start
    lines.append(
        payload(
            COLLECTOR.PREFIXES["summary"],
            {
                "status": "ok",
                "evidence_role": "untimed_correctness_control",
                "observer_load": "current_stats_every_member",
                "timing_eligible": False,
                "throughput_n": "null",
                "throughput_fps": "null",
                "clock": "std_instant_monotonic",
                "control_start": "first_control_measured_camera_input",
                "control_end": "last_control_measured_current_stats_terminal",
                "control_start_ns": control_start,
                "control_end_ns": measured_terminal_end,
                "control_duration_ns": control_duration,
                "control_measured_members": COLLECTOR.MEASURED,
                "warmup_issued": 20,
                "measured_issued": 80,
                "capture_issued": 2,
                "total_issued": 102,
                "total_terminals": 102,
                "total_presentations": 102,
                "auxiliary_presentations": 0,
                "receipt_polls": 300,
                "actual_plan_set": "cpu_post_sort,gpu_post_sort,gpu_preproject",
                "projected_adaptive_state_set": "disabled",
                "projected_execution_set": "candidate,compact",
                "capture_count": 2,
                "drain_draws": 0,
            },
        )
    )
    return "\n".join(lines) + "\n", workload


class Q1NativeCollectorTests(unittest.TestCase):
    def collect_with_finalization_fault(
        self,
        root: Path,
        output: Path,
        fault: Any,
    ) -> dict[str, Any]:
        dataset_path = root / "truck.ply"
        trace_path = root / "trace.json"
        dataset_path.write_bytes(b"truck")
        trace_path.write_text("{}", encoding="utf-8")
        workload = {
            "dataset": {"sha256": COLLECTOR.TRUCK["sha256"]},
            "dataset_path": dataset_path,
            "trace": {
                "trace_id": COLLECTOR.TRACE["id"],
                "content_sha256": COLLECTOR.TRACE["content_sha256"],
            },
            "trace_path": trace_path,
            "trace_file_sha256": COLLECTOR.TRACE["file_sha256"],
        }
        git = {"commit": "a" * 40, "dirty": False, "status_porcelain_sha256": "b" * 64}

        def fake_build(_repo: Path, build_output: Path, _git: dict[str, Any]) -> dict[str, Any]:
            target = build_output / "cargo-target"
            target.mkdir()
            binary = target / "desktop-example"
            binary.write_bytes(b"binary")
            return {"path": binary, "sha256": "binary-sha", "locked": True}

        def fake_sha(path: Path) -> str:
            if path == dataset_path:
                return COLLECTOR.TRUCK["sha256"]
            if path == trace_path:
                return COLLECTOR.TRACE["file_sha256"]
            return "binary-sha"

        validated = {
            "begin": {"schema": "fixture"},
            "summary": {},
            "control_presentations": [],
            "timed_presentations": [],
            "timed_drains": [],
            "submissions": [],
            "terminals": [],
            "frames": [],
            "captures": [],
        }
        completed = SimpleNamespace(
            returncode=0,
            stdout=COLLECTOR.PREFIXES["begin"] + "schema=fixture\n",
            stderr="",
        )
        with (
            mock.patch.object(COLLECTOR, "validate_ignored_output"),
            mock.patch.object(COLLECTOR, "git_receipt", return_value=git),
            mock.patch.object(
                COLLECTOR,
                "host_receipt",
                return_value={"system": "Darwin", "machine": "arm64", "cpu_brand": "Apple M4"},
            ),
            mock.patch.object(COLLECTOR, "load_workload", return_value=workload),
            mock.patch.object(COLLECTOR, "build_host", side_effect=fake_build),
            mock.patch.object(COLLECTOR.subprocess, "run", return_value=completed),
            mock.patch.object(COLLECTOR, "sha256_file", side_effect=fake_sha),
            mock.patch.object(COLLECTOR, "validate_run_log", return_value=validated),
            fault,
        ):
            return COLLECTOR.collect(SimpleNamespace(output=output), repo=root)

    def test_control_and_zero_current_stats_terminal_window_validate(self) -> None:
        stdout, workload = fixture_log()
        validated = COLLECTOR.validate_run_log(stdout, "", workload=workload)
        self.assertEqual(len(validated["submissions"]), 102)
        self.assertEqual(len(validated["terminals"]), 102)
        self.assertEqual(len(validated["frames"]), 80)
        self.assertEqual(len(validated["timed_presentations"]), 100)
        self.assertEqual(len(validated["timed_drains"]), 4)
        self.assertIsNone(validated["summary"]["control"]["throughput_fps"])
        self.assertEqual(validated["summary"]["terminal_throughput"]["n"], 80)
        self.assertEqual([item["trace_frame"] for item in validated["captures"]], ["0", "1"])

    def test_q1_phase_machine_has_no_surface_session_owner(self) -> None:
        q1_source = (REPO_ROOT / "examples/desktop/src/surface_sustained.rs").read_text(
            encoding="utf-8"
        )
        for forbidden in (
            "SurfaceRenderSession",
            ".render_frame(",
            ".poll_current_stats(",
            ".request_current_stats(",
            ".take_surface_capture(",
            ".request_surface_capture(",
            ".pump_receipts(",
            ".set_camera(",
        ):
            self.assertNotIn(forbidden, q1_source)
        self.assertIn("SurfaceRuntimeCommand::Present", q1_source)
        self.assertIn("SurfaceRuntimeEvent::Presented", q1_source)
        self.assertIn("RunPhase::TimedWarmupDrain", q1_source)
        self.assertEqual(q1_source.count("SurfaceRuntimeCommand::CompleteQueue"), 2)

    def test_terminal_window_bounds_accept_n1_and_n_greater_than_one(self) -> None:
        COLLECTOR.validate_phase_presentation_timeline(
            phase="measure",
            input_ns=[100],
            presented_ns=[110],
        )
        self.assertEqual(
            COLLECTOR.validate_terminal_window_bounds(
                expected_n=1,
                warmup_drain_end_ns=100,
                measured_input_ns=[100],
                measured_presented_ns=[110],
                terminal_end_ns=120,
            ),
            (100, 120, 20),
        )
        self.assertEqual(
            COLLECTOR.validate_terminal_window_bounds(
                expected_n=3,
                warmup_drain_end_ns=99,
                measured_input_ns=[100, 120, 140],
                measured_presented_ns=[110, 130, 150],
                terminal_end_ns=160,
            ),
            (100, 160, 60),
        )

    def test_terminal_window_bounds_fail_closed_at_invalid_boundaries(self) -> None:
        cases = (
            {
                "expected_n": 0,
                "warmup_drain_end_ns": 100,
                "measured_input_ns": [],
                "measured_presented_ns": [],
                "terminal_end_ns": 120,
            },
            {
                "expected_n": 2,
                "warmup_drain_end_ns": 100,
                "measured_input_ns": [100],
                "measured_presented_ns": [110],
                "terminal_end_ns": 120,
            },
            {
                "expected_n": 1,
                "warmup_drain_end_ns": 101,
                "measured_input_ns": [100],
                "measured_presented_ns": [110],
                "terminal_end_ns": 120,
            },
            {
                "expected_n": 1,
                "warmup_drain_end_ns": 99,
                "measured_input_ns": [100],
                "measured_presented_ns": [121],
                "terminal_end_ns": 120,
            },
            {
                "expected_n": 2,
                "warmup_drain_end_ns": 99,
                "measured_input_ns": [100, 120],
                "measured_presented_ns": [150, 130],
                "terminal_end_ns": 140,
            },
        )
        for case in cases:
            with self.subTest(case=case), self.assertRaises(
                COLLECTOR.IntegrityRejectedError
            ):
                COLLECTOR.validate_terminal_window_bounds(**case)

    def test_timed_member_presented_after_next_input_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        member_zero = matching_line(
            stdout,
            COLLECTOR.PREFIXES["timed_presentation"],
            "phase=warmup",
            "member_index=0",
        )
        member_one = matching_line(
            stdout,
            COLLECTOR.PREFIXES["timed_presentation"],
            "phase=warmup",
            "member_index=1",
        )
        member_one_input = int(
            next(item.split("=", 1)[1] for item in member_one.split() if item.startswith("input_ns="))
        )
        crossed = stdout.replace(
            member_zero,
            replace_field(member_zero, "presented_ns", member_one_input + 1),
            1,
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "presentation crossed the next camera input",
        ):
            COLLECTOR.validate_run_log(crossed, "", workload=workload)

    def test_early_member_presented_after_terminal_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        member_zero = matching_line(
            stdout,
            COLLECTOR.PREFIXES["timed_presentation"],
            "phase=warmup",
            "member_index=0",
        )
        terminal_end = matching_line(
            stdout,
            COLLECTOR.PREFIXES["timed_drain"],
            "phase=measure",
            "event=end",
        )
        terminal_end_ns = int(
            next(item.split("=", 1)[1] for item in terminal_end.split() if item.startswith("end_ns="))
        )
        after_terminal = stdout.replace(
            member_zero,
            replace_field(member_zero, "presented_ns", terminal_end_ns + 1),
            1,
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "overall presentation timeline is not monotonic",
        ):
            COLLECTOR.validate_run_log(after_terminal, "", workload=workload)

    def test_non_monotonic_presented_timeline_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        member_zero = matching_line(
            stdout,
            COLLECTOR.PREFIXES["timed_presentation"],
            "phase=measure",
            "member_index=0",
        )
        member_one = matching_line(
            stdout,
            COLLECTOR.PREFIXES["timed_presentation"],
            "phase=measure",
            "member_index=1",
        )
        member_zero_presented = int(
            next(
                item.split("=", 1)[1]
                for item in member_zero.split()
                if item.startswith("presented_ns=")
            )
        )
        non_monotonic = stdout.replace(
            member_one,
            replace_field(member_one, "presented_ns", member_zero_presented - 1),
            1,
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "overall presentation timeline is not monotonic",
        ):
            COLLECTOR.validate_run_log(non_monotonic, "", workload=workload)

    def test_warmup_and_terminal_drains_before_presentations_fail_closed(self) -> None:
        stdout, workload = fixture_log()
        for phase, expected in (
            ("warmup", "warmup drain began before warmup drawing stopped"),
            ("measure", "terminal drain began before measured drawing stopped"),
        ):
            begin = matching_line(
                stdout,
                COLLECTOR.PREFIXES["timed_drain"],
                f"phase={phase}",
                "event=begin",
            )
            end = matching_line(
                stdout,
                COLLECTOR.PREFIXES["timed_drain"],
                f"phase={phase}",
                "event=end",
            )
            first_presentation = matching_line(
                stdout,
                COLLECTOR.PREFIXES["timed_presentation"],
                f"phase={phase}",
                "member_index=0",
            )
            first_input_ns = int(
                next(
                    item.split("=", 1)[1]
                    for item in first_presentation.split()
                    if item.startswith("input_ns=")
                )
            )
            early_start = first_input_ns
            early_end = first_input_ns + 1
            drifted_begin = replace_field(begin, "start_ns", early_start)
            drifted_end = replace_field(
                replace_field(end, "start_ns", early_start),
                "end_ns",
                early_end,
            )
            early_drain = stdout.replace(begin, drifted_begin, 1).replace(
                end, drifted_end, 1
            )
            with self.subTest(phase=phase), self.assertRaisesRegex(
                COLLECTOR.IntegrityRejectedError,
                expected,
            ):
                COLLECTOR.validate_run_log(early_drain, "", workload=workload)

    def test_duplicate_or_missing_ticket_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        terminal_lines = [
            line for line in stdout.splitlines() if line.startswith(COLLECTOR.PREFIXES["terminal"])
        ]
        duplicate = stdout + terminal_lines[0] + "\n"
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "ledger length|duplicate"):
            COLLECTOR.validate_run_log(duplicate, "", workload=workload)
        missing = stdout.replace(terminal_lines[0] + "\n", "", 1)
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "ledger length"):
            COLLECTOR.validate_run_log(missing, "", workload=workload)

    def test_cross_ticket_member_and_frame_pairing_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        terminal_lines = [
            line for line in stdout.splitlines() if line.startswith(COLLECTOR.PREFIXES["terminal"])
        ]
        first = terminal_lines[0]
        second = terminal_lines[1]
        crossed_first = first.replace("member_index=0", "member_index=1", 1).replace(
            "trace_frame=0", "trace_frame=1", 1
        )
        crossed_second = second.replace("member_index=1", "member_index=0", 1).replace(
            "trace_frame=1", "trace_frame=0", 1
        )
        crossed_terminals = stdout.replace(first, crossed_first, 1).replace(
            second, crossed_second, 1
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "member_index join mismatch|trace_frame join mismatch"
        ):
            COLLECTOR.validate_run_log(crossed_terminals, "", workload=workload)

        presentation_lines = [
            line
            for line in stdout.splitlines()
            if line.startswith(COLLECTOR.PREFIXES["presentation"])
        ]
        first = presentation_lines[0]
        second = presentation_lines[1]
        crossed_first = first.replace("member_index=0", "member_index=1", 1).replace(
            "trace_frame=0", "trace_frame=1", 1
        )
        crossed_second = second.replace("member_index=1", "member_index=0", 1).replace(
            "trace_frame=1", "trace_frame=0", 1
        )
        crossed_presentations = stdout.replace(first, crossed_first, 1).replace(
            second, crossed_second, 1
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "does not match ticket",
        ):
            COLLECTOR.validate_run_log(crossed_presentations, "", workload=workload)

    def test_unknown_count_semantics_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        unknown = stdout.replace(
            "count_semantics=indirect_draw_equals_visible",
            "count_semantics=totally_unknown",
            1,
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "closed-enum"):
            COLLECTOR.validate_run_log(unknown, "", workload=workload)

    def test_camera_matrix_or_capture_identity_drift_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        matrix_drift = stdout.replace("view_matrix=", "view_matrix=999,", 1)
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "expected 16 values"):
            COLLECTOR.validate_run_log(matrix_drift, "", workload=workload)
        capture_drift = stdout.replace(
            "camera_revision=201 presentation_sequence=101",
            "camera_revision=999 presentation_sequence=101",
            1,
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "camera join mismatch"):
            COLLECTOR.validate_run_log(capture_drift, "", workload=workload)

    def test_unknown_capture_ticket_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        capture_line = next(
            line
            for line in stdout.splitlines()
            if line.startswith(COLLECTOR.PREFIXES["capture"])
        )
        unknown = stdout.replace(
            capture_line,
            capture_line.replace("ticket=101", "ticket=999999", 1),
            1,
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "unknown ticket"):
            COLLECTOR.validate_run_log(unknown, "", workload=workload)

    def test_post_protocol_program_error_is_rejected_not_deferred(self) -> None:
        self.assertEqual(
            COLLECTOR.classify_failure(
                KeyError("capture"), COLLECTOR.CollectionPhase.PROTOCOL_STARTED
            ),
            ("Rejected", "integrity_rejected"),
        )

    def test_cleanup_failure_publishes_only_immutable_rejected_result(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            result = self.collect_with_finalization_fault(
                root,
                output,
                mock.patch.object(
                    COLLECTOR.SURFACE_EVIDENCE,
                    "remove_private_cargo_target",
                    side_effect=OSError("cleanup failed"),
                ),
            )
            self.assertEqual(result["native_prerequisite"], "Rejected")
            self.assertEqual(result["failure_phase"], "finalization_started")
            self.assertEqual(result["publication"], "immutable_non_success_root")
            published = json.loads((output / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(published["native_prerequisite"], "Rejected")
            self.assertFalse((output.stat().st_mode & 0o222))
            self.assertFalse(((output / "result.json").stat().st_mode & 0o222))

    def test_successful_transaction_publishes_one_immutable_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            transaction = COLLECTOR.SURFACE_EVIDENCE.ImmutableOutputTransaction(output)
            run_dir = transaction.staging / "run"
            run_dir.mkdir()
            (run_dir / "receipt.txt").write_text("ready\n", encoding="utf-8")
            transaction.publish_result(
                {
                    "schema": COLLECTOR.SCHEMA,
                    "cell": COLLECTOR.CELL,
                    "native_prerequisite": "Accepted",
                }
            )
            self.assertTrue(output.is_dir())
            self.assertEqual(
                json.loads((output / "result.json").read_text(encoding="utf-8"))[
                    "native_prerequisite"
                ],
                "Accepted",
            )
            self.assertFalse((output.stat().st_mode & 0o222))
            self.assertFalse(list(root.glob(".output.*")))

    def test_write_failure_leaves_no_accepted_or_mutable_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            result = self.collect_with_finalization_fault(
                root,
                output,
                mock.patch.object(
                    COLLECTOR.SURFACE_EVIDENCE,
                    "write_json",
                    side_effect=OSError("write failed"),
                ),
            )
            self.assertEqual(result["native_prerequisite"], "Rejected")
            self.assertEqual(result["failure_phase"], "finalization_started")
            self.assertEqual(result["publication"], "blocked")
            self.assertFalse(output.exists())
            self.assertFalse(list(root.glob(".output.*")))

    def test_chmod_failure_leaves_no_accepted_or_mutable_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            result = self.collect_with_finalization_fault(
                root,
                output,
                mock.patch.object(
                    COLLECTOR.SURFACE_EVIDENCE,
                    "make_tree_immutable",
                    side_effect=OSError("chmod failed"),
                ),
            )
            self.assertEqual(result["native_prerequisite"], "Rejected")
            self.assertEqual(result["failure_phase"], "finalization_started")
            self.assertEqual(result["publication"], "blocked")
            self.assertFalse(output.exists())
            self.assertFalse(list(root.glob(".output.*")))

    def test_collect_post_protocol_program_error_publishes_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            dataset_path = root / "truck.ply"
            trace_path = root / "trace.json"
            dataset_path.write_bytes(b"truck")
            trace_path.write_text("{}", encoding="utf-8")
            workload = {
                "dataset": {"sha256": COLLECTOR.TRUCK["sha256"]},
                "dataset_path": dataset_path,
                "trace": {
                    "trace_id": COLLECTOR.TRACE["id"],
                    "content_sha256": COLLECTOR.TRACE["content_sha256"],
                },
                "trace_path": trace_path,
                "trace_file_sha256": COLLECTOR.TRACE["file_sha256"],
            }
            git = {"commit": "a" * 40, "dirty": False, "status_porcelain_sha256": "b" * 64}

            def fake_build(_repo: Path, build_output: Path, _git: dict[str, Any]) -> dict[str, Any]:
                target = build_output / "cargo-target"
                target.mkdir()
                binary = target / "desktop-example"
                binary.write_bytes(b"binary")
                return {"path": binary, "sha256": "binary-sha", "locked": True}

            def fake_sha(path: Path) -> str:
                if path == dataset_path:
                    return COLLECTOR.TRUCK["sha256"]
                if path == trace_path:
                    return COLLECTOR.TRACE["file_sha256"]
                return "binary-sha"

            completed = SimpleNamespace(
                returncode=0,
                stdout=COLLECTOR.PREFIXES["begin"] + "schema=fixture\n",
                stderr="",
            )
            with (
                mock.patch.object(COLLECTOR, "validate_ignored_output"),
                mock.patch.object(COLLECTOR, "git_receipt", return_value=git),
                mock.patch.object(
                    COLLECTOR,
                    "host_receipt",
                    return_value={"system": "Darwin", "machine": "arm64", "cpu_brand": "Apple M4"},
                ),
                mock.patch.object(COLLECTOR, "load_workload", return_value=workload),
                mock.patch.object(COLLECTOR, "build_host", side_effect=fake_build),
                mock.patch.object(COLLECTOR.subprocess, "run", return_value=completed),
                mock.patch.object(COLLECTOR, "sha256_file", side_effect=fake_sha),
                mock.patch.object(COLLECTOR, "validate_run_log", side_effect=KeyError("capture")),
            ):
                result = COLLECTOR.collect(SimpleNamespace(output=output), repo=root)
            self.assertEqual(result["native_prerequisite"], "Rejected")
            self.assertEqual(result["failure_phase"], "protocol_started")
            self.assertEqual(result["reason"], "integrity_rejected")
            published = json.loads((output / "result.json").read_text(encoding="utf-8"))
            self.assertEqual(published["native_prerequisite"], "Rejected")
            self.assertNotIn("run", published)
            self.assertFalse((output.stat().st_mode & 0o222))
        self.assertEqual(
            COLLECTOR.classify_failure(
                COLLECTOR.EnvironmentPrerequisiteError("missing Truck"),
                COLLECTOR.CollectionPhase.PREFLIGHT,
            ),
            ("Deferred", "environment_prerequisite"),
        )
        self.assertEqual(
            COLLECTOR.classify_failure(
                COLLECTOR.EnvironmentPrerequisiteError("build failed"),
                COLLECTOR.CollectionPhase.BUILD_STARTED,
            ),
            ("Rejected", "integrity_rejected"),
        )

    def test_drain_draw_or_non_monotonic_window_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        drain_draw = stdout.replace("draws_during_drain=0", "draws_during_drain=1", 1)
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "added a drain draw"):
            COLLECTOR.validate_run_log(drain_draw, "", workload=workload)
        timed_terminal = next(
            line
            for line in stdout.splitlines()
            if line.startswith(COLLECTOR.PREFIXES["timed_drain"])
        )
        intermediate_drain = stdout + timed_terminal + "\n"
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "warmup/terminal drain ledger"
        ):
            COLLECTOR.validate_run_log(intermediate_drain, "", workload=workload)
        timed_summary = next(
            line
            for line in stdout.splitlines()
            if line.startswith(COLLECTOR.PREFIXES["timed_summary"])
        )
        timed_terminal_end = next(
            field.split("=", 1)[1]
            for field in timed_summary.split()
            if field.startswith("window_end_ns=")
        )
        bad_window = stdout.replace(
            f"window_end_ns={timed_terminal_end}", "window_end_ns=999999999", 1
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "window end"):
            COLLECTOR.validate_run_log(bad_window, "", workload=workload)

    def test_timed_warmup_drain_schema_and_boundary_fail_closed(self) -> None:
        stdout, workload = fixture_log()
        timed_drain_lines = [
            line
            for line in stdout.splitlines()
            if line.startswith(COLLECTOR.PREFIXES["timed_drain"])
        ]
        warmup_begin = next(
            line for line in timed_drain_lines if "phase=warmup event=begin" in line
        )
        warmup_end = next(
            line for line in timed_drain_lines if "phase=warmup event=end" in line
        )
        missing = stdout.replace(warmup_begin + "\n", "", 1)
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "warmup/terminal drain ledger"
        ):
            COLLECTOR.validate_run_log(missing, "", workload=workload)

        for field in ("draw_count", "current_stats_count", "capture_count"):
            drifted = stdout.replace(
                warmup_begin,
                warmup_begin.replace(f"{field}=0", f"{field}=1", 1),
                1,
            )
            with self.subTest(field=field), self.assertRaisesRegex(
                COLLECTOR.IntegrityRejectedError, f"{field} must be zero"
            ):
                COLLECTOR.validate_run_log(drifted, "", workload=workload)

        records = COLLECTOR.parse_log(stdout, "")
        first_measured_input = int(
            next(
                item
                for item in records["timed_presentation"]
                if item.get("phase") == "measure" and item.get("member_index") == "0"
            )["input_ns"]
        )
        original_end = next(
            field.split("=", 1)[1]
            for field in warmup_end.split()
            if field.startswith("end_ns=")
        )
        late_end_line = warmup_end.replace(
            f"end_ns={original_end}", f"end_ns={first_measured_input + 1}", 1
        )
        late_end = stdout.replace(warmup_end, late_end_line, 1)
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "measured input preceded warmup queue completion",
        ):
            COLLECTOR.validate_run_log(late_end, "", workload=workload)

    def test_timed_current_stats_or_control_counts_fail_closed(self) -> None:
        stdout, workload = fixture_log()
        observer_load = stdout.replace(
            "current_stats_requests=0 current_stats_submission=not_requested",
            "current_stats_requests=1 current_stats_submission=issued",
            1,
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "current-stats observer load"
        ):
            COLLECTOR.validate_run_log(observer_load, "", workload=workload)
        fabricated_counts = stdout.replace(
            "current_stats_submission=not_requested position=",
            "current_stats_submission=not_requested visible_count=1 position=",
            1,
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "fabricated timed"):
            COLLECTOR.validate_run_log(fabricated_counts, "", workload=workload)

    def test_timed_actual_plan_execution_mismatch_fails_closed(self) -> None:
        stdout, workload = fixture_log()
        mismatch = stdout.replace(
            "projected_draw_execution=compact current_stats_requests=0",
            "projected_draw_execution=candidate current_stats_requests=0",
            1,
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "execution/plan mismatch"):
            COLLECTOR.validate_run_log(mismatch, "", workload=workload)

    def test_command_is_one_locked_profile_with_no_outer_pair_or_retry_controls(self) -> None:
        command = COLLECTOR.make_command(
            Path("desktop-example-bin"),
            {"dataset_path": Path("truck.ply"), "trace_path": Path("trace.json")},
        )
        self.assertEqual(command.count("--surface-q1-m4-native"), 1)
        self.assertEqual(command[command.index("--camera-warmup-frames") + 1], "20")
        self.assertEqual(command[command.index("--camera-measured-frames") + 1], "80")
        self.assertEqual(command[command.index("--order-backend") + 1], "adaptive")
        self.assertNotIn("playcanvas", " ".join(command).lower())
        self.assertNotIn("--repetitions", command)


if __name__ == "__main__":
    unittest.main()
