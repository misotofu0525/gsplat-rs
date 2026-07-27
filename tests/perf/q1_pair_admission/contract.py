"""Frozen Q0-derived schedule and normalized terminal receipt contract."""

from __future__ import annotations

import math
import pathlib
from datetime import datetime
from typing import Any

from .common import (
    array,
    canonical_sha256,
    fail,
    file_sha256,
    integer,
    load_json,
    number,
    obj,
    string,
    utc,
)


SCHEMA = "gsplat-q1-truck-paired-comparison/v1"
RESULT_SCHEMA = "gsplat-q1-truck-paired-result/v1"
TERMINAL_SCHEMA = "gsplat-q1-webgpu-terminal-window/v1"
IMAGE_SCHEMA = "gsplat-q1-reference-image-comparison/v1"
TRUCK = {
    "id": "inria-3dgs-truck-iteration-30000",
    "sha256": "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c",
    "bytes": 630_225_580,
    "splat_count": 2_541_226,
    "sh_degree": 3,
}
TRACE = {
    "id": "candidate-truck-quality-2view-1920x1080-v1",
    "sha256": "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3",
    "frame_indices": [0, 1],
}
TRACE_FRAME_POSE_INTRINSICS_SHA256 = {
    0: "a008beb20adfdc24af484b25112503edb636e03010028aaeae812c3454527b46",
    1: "4b1d63381a662226712fd58cf5b3ea120fe5378beb73c228a509f55ec393f265",
}


def _load_playcanvas_camera_authority() -> dict[int, dict[str, Any]]:
    module_directory = pathlib.Path(__file__).parent
    fixture = load_json(
        module_directory / "fixtures/playcanvas-truck-camera-receipts-v1.json",
        "PlayCanvas camera authority fixture",
    )
    if fixture.get("schema") != "gsplat-playcanvas-camera-authority-fixture/v1":
        fail("PlayCanvas camera authority fixture schema mismatch")
    if fixture.get("source_trace") != {
        "id": TRACE["id"],
        "content_sha256": TRACE["sha256"],
    }:
        fail("PlayCanvas camera authority fixture does not bind the frozen Truck trace")
    authority = obj(fixture, "authority", "PlayCanvas camera authority fixture")
    authority_path = "tests/competitive/playcanvas/public/trace-camera.js"
    if authority != {
        "path": authority_path,
        "sha256": file_sha256(module_directory.parents[2] / authority_path),
        "oracle_export": "canonicalPlayCanvasCameraOracle",
        "receipt_export": "createPlayCanvasCameraReceipt",
    }:
        fail("PlayCanvas camera authority fixture is stale; run its generator")
    receipts = obj(fixture, "receipts", "PlayCanvas camera authority fixture")
    if set(receipts) != {str(index) for index in TRACE["frame_indices"]}:
        fail("PlayCanvas camera authority fixture does not cover the frozen Truck trace")
    result: dict[int, dict[str, Any]] = {}
    for index in TRACE["frame_indices"]:
        receipt = receipts[str(index)]
        if not isinstance(receipt, dict):
            fail(f"PlayCanvas camera authority fixture receipt {index} must be an object")
        result[index] = receipt
    return result


PLAYCANVAS_CAMERA_AUTHORITY = _load_playcanvas_camera_authority()
WIDTH = 1920
HEIGHT = 1080
WARMUP = 20
MEASURED = 80
PLAYCANVAS = {
    "version": "2.21.0-beta.14",
    "revision": "d5fe88878e338936fe763bbce1a58bc315e89cbe",
    "runtime_revision": "d5fe888",
    "integrity": "sha512-qYN8vp9CBRBU8qs9eJudUM3fFkPbXz8qN5IqjFicULJoD0CNnEtlFsGoHVAOQZ5b2+LWlUxUFhXMSzNL86moWg==",
}
COMMON_ENVIRONMENT_FIELDS = (
    "os",
    "device",
    "browser",
    "browser_executable_sha256",
    "browser_launch_args_sha256",
    "adapter",
    "driver",
    "adapter_limits_sha256",
    "power_source",
    "collection_session_id",
)


def validate_protocol(document: dict[str, Any]) -> tuple[str, float]:
    protocol = obj(document, "protocol", "schedule")
    if obj(protocol, "dataset", "schedule.protocol") != TRUCK:
        fail("schedule.protocol.dataset is not the frozen complete Truck SH3 source")
    if obj(protocol, "trace", "schedule.protocol") != TRACE:
        fail("schedule.protocol.trace is not the frozen two-view Truck trace")
    if obj(protocol, "display", "schedule.protocol") != {"width": WIDTH, "height": HEIGHT, "dpr": 1}:
        fail("schedule.protocol.display must be exact 1920x1080 DPR-1")
    expected = {
        "camera_mode": "trace_sequence",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_frames": WARMUP,
        "measured_frames": MEASURED,
        "terminal_boundary": "first_measured_camera_input_to_final_gpu_queue_completion",
        "claim_scope": "near-contract",
    }
    for key, value in expected.items():
        if protocol.get(key) != value:
            fail(f"schedule.protocol.{key} must equal {value!r}")
    quality = obj(protocol, "quality_gate", "schedule.protocol")
    if quality.get("metric") != "ssim-luma-srgb-window8":
        fail("schedule.protocol.quality_gate.metric mismatch")
    minimum = number(quality, "minimum_ssim", "schedule.protocol.quality_gate")
    if not 0 <= minimum <= 1:
        fail("schedule.protocol.quality_gate.minimum_ssim must be in [0,1]")
    return canonical_sha256(protocol), minimum


def validate_schedule(document: dict[str, Any]) -> tuple[list[dict[str, Any]], str, datetime]:
    schedule = obj(document, "schedule", "schedule")
    integer(schedule, "seed", "schedule.schedule")
    predeclared = utc(schedule.get("predeclared_at_utc"), "schedule.schedule.predeclared_at_utc")
    pairs = array(schedule, "pairs", "schedule.schedule")
    references = array(schedule, "reference_images", "schedule.schedule")
    if len(references) != 2:
        fail("schedule.schedule.reference_images must cover both views")
    if len(pairs) != 5:
        fail("schedule.schedule.pairs must contain exactly five pairs")
    seen: set[str] = set()
    orders: list[str] = []
    for index, pair in enumerate(pairs):
        if not isinstance(pair, dict):
            fail(f"schedule.schedule.pairs[{index}] must be an object")
        pair_id = string(pair, "pair_id", f"schedule.schedule.pairs[{index}]")
        if pair_id in seen:
            fail(f"duplicate predeclared pair id: {pair_id}")
        seen.add(pair_id)
        order = string(pair, "run_order", f"schedule.schedule.pairs[{index}]")
        if order not in {"playcanvas-first", "gsplat-rs-first"}:
            fail(f"{pair_id}: invalid run_order")
        orders.append(order)
    if set(orders) != {"playcanvas-first", "gsplat-rs-first"} or abs(
        orders.count("playcanvas-first") - orders.count("gsplat-rs-first")
    ) > 1:
        fail("predeclared order is not counterbalanced AB/BA")
    return pairs, canonical_sha256(schedule), predeclared


def validate_terminal(q1: dict[str, Any], summary: dict[str, Any], context: str) -> float:
    terminal = obj(q1, "terminal_window", context)
    expected = {
        "schema": TERMINAL_SCHEMA,
        "clock": "performance_now_monotonic",
        "start_boundary": "first_measured_camera_input_accepted",
        "end_boundary": "final_measured_gpu_queue_completion",
        "completion_primitive": "gpu_queue_on_submitted_work_done",
        "frame_loop_policy": "controlled_presented_raf",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_queue_drained": True,
        "continuous_submissions": True,
        "per_frame_observer_reads": 0,
        "extra_submissions_during_terminal_drain": 0,
        "measured_camera_input_count": MEASURED,
        "measured_submission_count": MEASURED,
        "dropped_frame_count": 0,
        "submission_counter_stable_during_drain": True,
    }
    for key, value in expected.items():
        if terminal.get(key) != value:
            fail(f"{context}.terminal_window.{key} must equal {value!r}")
    before = integer(terminal, "submission_counter_before_first", f"{context}.terminal_window")
    after = integer(terminal, "submission_counter_after_last", f"{context}.terminal_window")
    if after - before != MEASURED:
        fail(f"{context}.terminal_window does not prove exactly 80 submissions")
    start = number(terminal, "started_at_monotonic_ms", f"{context}.terminal_window")
    end = number(terminal, "completed_at_monotonic_ms", f"{context}.terminal_window")
    duration = number(terminal, "duration_ms", f"{context}.terminal_window")
    if not start < end or not math.isclose(duration, end - start, abs_tol=1e-6):
        fail(f"{context}.terminal_window has inconsistent boundaries")
    sustained = obj(summary, "sustained_throughput", context.replace("manifest", "summary"))
    expected_metrics = {
        "measured_frame_count": MEASURED,
        "terminal_window_ms": duration,
        "mean_frame_ms": duration / MEASURED,
        "mean_fps": 1000 * MEASURED / duration,
    }
    for key, value in expected_metrics.items():
        observed = sustained.get(key)
        if not isinstance(observed, (int, float)) or isinstance(observed, bool) or not math.isclose(
            float(observed), float(value), rel_tol=1e-9, abs_tol=1e-9
        ):
            fail(f"{context} summary.sustained_throughput.{key} mismatch")
    return duration
