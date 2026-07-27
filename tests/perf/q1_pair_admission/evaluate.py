"""Series orchestration and finite Accepted/Rejected verdicts."""

from __future__ import annotations

import json
import os
import pathlib
import statistics
from typing import Any

from .artifacts import artifact, bind_control, endpoint_images, reference_images
from .common import ValidationError, array, fail, load_json, obj, string
from .contract import MEASURED, RESULT_SCHEMA, SCHEMA, validate_protocol, validate_schedule


def evaluate(path: pathlib.Path) -> dict[str, Any]:
    root = path.resolve().parent
    document = load_json(path, "schedule")
    if document.get("schema") != SCHEMA:
        fail(f"schedule.schema must equal {SCHEMA!r}")
    series_id = string(document, "series_id", "schedule")
    protocol_sha, minimum_ssim = validate_protocol(document)
    planned, schedule_sha, predeclared = validate_schedule(document)
    evidence_values = array(document, "pairs", "schedule")
    evidence: dict[str, dict[str, Any]] = {}
    for index, value in enumerate(evidence_values):
        if not isinstance(value, dict):
            fail(f"schedule.pairs[{index}] must be an object")
        pair_id = string(value, "pair_id", f"schedule.pairs[{index}]")
        if pair_id in evidence:
            fail(f"schedule.pairs repeats {pair_id}")
        evidence[pair_id] = value
    if len(evidence) != 5:
        fail("schedule.pairs must bind all five predeclared pairs")
    references = reference_images(root, document)
    seen_paths: set[pathlib.Path] = set()
    seen_runs: set[str] = set()
    frozen_commit: str | None = None
    frozen_builds: dict[str, dict[str, str]] = {}
    frozen_environment: dict[str, Any] | None = None
    frozen_display: dict[str, Any] | None = None
    previous_pair_end = None
    pair_results: list[dict[str, Any]] = []
    scores: list[float] = []
    for declaration in planned:
        pair_id = declaration["pair_id"]
        order = declaration["run_order"]
        pair = evidence.get(pair_id)
        if pair is None or pair.get("run_order") != order:
            fail(f"{pair_id}: evidence does not match its predeclared order")
        endpoints: dict[str, dict[str, Any]] = {}
        for endpoint in ("playcanvas", "gsplat_rs"):
            values = obj(pair, endpoint, pair_id)
            position = 1 if (order == "playcanvas-first") == (endpoint == "playcanvas") else 2
            if values.get("position") != position:
                fail(f"{pair_id}.{endpoint}.position does not match {order}")
            common = dict(endpoint=endpoint, series_id=series_id, schedule_sha=schedule_sha, protocol_sha=protocol_sha, pair_id=pair_id, order=order, position=position, predeclared=predeclared, seen_paths=seen_paths, seen_runs=seen_runs)
            control = artifact(root, values.get("control"), role="control", **common)
            throughput = artifact(root, values.get("throughput"), role="throughput", **common)
            bind_control(throughput, control, f"{pair_id}.{endpoint}.throughput.control_binding")
            if throughput["started"] < control["ended"]:
                fail(f"{pair_id}.{endpoint} throughput overlaps its control artifact")
            if control["commit"] != throughput["commit"] or control["build_artifacts"] != throughput["build_artifacts"] or control["environment"] != throughput["environment"] or control["display"] != throughput["display"]:
                fail(f"{pair_id}.{endpoint} control/throughput identity drift")
            images = endpoint_images(root, values.get("images"), endpoint=endpoint, pair_id=pair_id, control=control, references=references, minimum=minimum_ssim)
            scores.extend(image["score"] for image in images)
            endpoints[endpoint] = {"control": control, "throughput": throughput, "images": images}
            if frozen_commit is None:
                frozen_commit = control["commit"]
            elif control["commit"] != frozen_commit:
                fail(f"{pair_id}.{endpoint} changed the series Git commit")
            if frozen_builds.setdefault(endpoint, control["build_artifacts"]) != control["build_artifacts"]:
                fail(f"{pair_id}.{endpoint} changed built artifacts")
            if frozen_environment is None:
                frozen_environment = control["environment"]
            elif control["environment"] != frozen_environment:
                fail(f"{pair_id}.{endpoint} changed the collection environment")
            if frozen_display is None:
                frozen_display = control["display"]
            elif control["display"] != frozen_display:
                fail(f"{pair_id}.{endpoint} changed presentation cadence")
        for trace_index in (0, 1):
            pc_camera = endpoints["playcanvas"]["control"]["presentations"][trace_index][
                "camera_receipt_sha256"
            ]
            gs_camera = endpoints["gsplat_rs"]["control"]["presentations"][trace_index][
                "camera_receipt_sha256"
            ]
            if pc_camera != gs_camera:
                fail(f"{pair_id}: trace view {trace_index} live-camera receipts differ")
        first = "playcanvas" if order == "playcanvas-first" else "gsplat_rs"
        second = "gsplat_rs" if first == "playcanvas" else "playcanvas"
        first_start = endpoints[first]["control"]["started"]
        first_end = endpoints[first]["throughput"]["ended"]
        second_start = endpoints[second]["control"]["started"]
        pair_end = endpoints[second]["throughput"]["ended"]
        if first_end > second_start:
            fail(f"{pair_id}: timestamps do not prove predeclared {order} execution")
        if previous_pair_end is not None and first_start < previous_pair_end:
            fail(f"{pair_id}: pair execution overlaps the prior predeclared pair")
        previous_pair_end = pair_end
        pc = endpoints["playcanvas"]["throughput"]["terminal_ms"] / MEASURED
        gs = endpoints["gsplat_rs"]["throughput"]["terminal_ms"] / MEASURED
        pair_results.append({
            "pair_id": pair_id,
            "run_order": order,
            "playcanvas_terminal_mean_ms": pc,
            "gsplat_rs_terminal_mean_ms": gs,
            "gsplat_rs_minus_playcanvas_ms": gs - pc,
            "gsplat_rs_over_playcanvas_ratio": gs / pc,
            "images": {endpoint: endpoints[endpoint]["images"] for endpoint in endpoints},
            "count_scope": {"playcanvas": "full_membership_v_c_d_unavailable", "gsplat_rs": "control_only_exact_v_c_d_throughput_unobserved"},
        })
    quality_passed = min(scores) >= minimum_ssim
    median_delta = statistics.median(pair["gsplat_rs_minus_playcanvas_ms"] for pair in pair_results)
    reasons: list[str] = []
    performance = None
    if not quality_passed:
        reasons.append("common_reference_image_gate_failed")
    else:
        performance = {
            "metric": "paired_median_terminal_mean_ms",
            "playcanvas_terminal_mean_ms": statistics.median(pair["playcanvas_terminal_mean_ms"] for pair in pair_results),
            "gsplat_rs_terminal_mean_ms": statistics.median(pair["gsplat_rs_terminal_mean_ms"] for pair in pair_results),
            "gsplat_rs_minus_playcanvas_ms": median_delta,
            "gsplat_rs_over_playcanvas_ratio": statistics.median(pair["gsplat_rs_over_playcanvas_ratio"] for pair in pair_results),
            "required_lead_percentage": None,
        }
        if median_delta > 0:
            reasons.append("gsplat_rs_slower_on_paired_median_terminal_window")
    return {
        "schema": RESULT_SCHEMA,
        "series_id": series_id,
        "state": "Accepted" if not reasons else "Rejected",
        "evidence_admitted": True,
        "claim_scope": "chrome_webgpu_truck_1080p_near_contract",
        "schedule_sha256": schedule_sha,
        "protocol_sha256": protocol_sha,
        "pair_count": 5,
        "minimum_ssim": minimum_ssim,
        "minimum_observed_ssim": min(scores),
        "quality_passed": quality_passed,
        "performance": performance,
        "reasons": reasons,
        "pairs": pair_results,
        "limitations": [
            "PlayCanvas V/C/D remain unavailable and are not inferred from source membership",
            "gsplat-rs control V/C/D are not copied into throughput",
            "the result is a near-contract for the named Chrome/M4/Truck configuration only",
        ],
        "retry_authorized": False,
    }


def admission_rejection(series_id: str | None, reason: str) -> dict[str, Any]:
    return {"schema": RESULT_SCHEMA, "series_id": series_id, "state": "Rejected", "evidence_admitted": False, "claim_scope": None, "performance": None, "reasons": [reason], "retry_authorized": False}


def write_result(path: pathlib.Path | None, result: dict[str, Any]) -> None:
    encoded = f"{json.dumps(result, indent=2, sort_keys=True)}\n"
    if path is None:
        print(encoded, end="")
        return
    if path.exists():
        fail(f"output already exists: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    staging = path.with_name(f".{path.name}.staging-{os.getpid()}")
    try:
        with staging.open("x", encoding="utf-8") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(staging, path)
    finally:
        try:
            staging.unlink()
        except FileNotFoundError:
            pass
