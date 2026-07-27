#!/usr/bin/env python3
"""Focused fixtures for Q1 Truck schedule, admission, and finite verdicts."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone

from q1_pair_admission.common import ValidationError, canonical_sha256, file_sha256
from q1_pair_admission.contract import (
    HEIGHT,
    IMAGE_SCHEMA,
    MEASURED,
    PLAYCANVAS,
    SCHEMA,
    TERMINAL_SCHEMA,
    TRACE,
    TRUCK,
    WARMUP,
    WIDTH,
)
from q1_pair_admission.evaluate import admission_rejection, evaluate


SHA_A = "a" * 64
SHA_B = "b" * 64
COMMIT = "c" * 40
PREDECLARED = "2026-07-28T00:00:00Z"
ORDERS = ["playcanvas-first", "gsplat-rs-first", "playcanvas-first", "gsplat-rs-first", "playcanvas-first"]
CLI = pathlib.Path(__file__).with_name("validate-q1-truck-paired-comparison.py")


def write_json(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"{json.dumps(value, indent=2)}\n", encoding="utf-8")


def write_png_header(path: pathlib.Path) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n" + (13).to_bytes(4, "big") + b"IHDR"
        + WIDTH.to_bytes(4, "big") + HEIGHT.to_bytes(4, "big")
    )
    return file_sha256(path)


def distribution(value: float | None) -> dict[str, float | int] | None:
    if value is None:
        return None
    return {"count": MEASURED, "mean": value, "p50": value, "p90": value, "p95": value, "p99": value, "max": value}


def frame(run_id: str, index: int, endpoint: str, role: str) -> dict[str, object]:
    value: dict[str, object] = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "frame",
        "run_id": run_id,
        "frame_index": index,
        "elapsed_ns": index * 1_000_000,
        "call_ms": 1.0,
        "frame_wall_ms": 10.0,
        "preprocess_ms": None,
        "sort_ms": None,
        "geometry_submit_ms": None,
        "gpu_wait_ms": None,
        "gpu_complete_ms": None,
        "visible": None,
        "drawn": None,
        "sort_refreshed": True,
        "trace_frame_index": index % 2,
    }
    if endpoint == "playcanvas":
        value["active_splats"] = TRUCK["splat_count"]
    else:
        value.update({
            "order_backend": "gpu",
            "gpu_order_producer": "preproject",
            "projected_execution": "compact",
            "raster_execution_plan": "projected_quads_exact",
            "gpu_sort_fallback": False,
        })
        if role == "control":
            value.update({"visible": 1_500_000, "contributor": 900_000, "drawn": 900_000, "exact_contributor_compaction": True})
    return value


def summary(run_id: str, terminal_ms: float | None) -> dict[str, object]:
    value: dict[str, object] = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": MEASURED,
        "warmup_count": WARMUP,
        "frame_budget_ms": 16.666666666666668,
        "missed_frame_count": 0,
        "distributions": {
            "call_ms": distribution(1.0),
            "frame_wall_ms": distribution(10.0),
            "preprocess_ms": None,
            "sort_ms": None,
            "geometry_submit_ms": None,
            "gpu_wait_ms": None,
            "gpu_complete_ms": None,
        },
    }
    if terminal_ms is not None:
        value["sustained_throughput"] = {
            "measured_frame_count": MEASURED,
            "terminal_window_ms": terminal_ms,
            "mean_frame_ms": terminal_ms / MEASURED,
            "mean_fps": 1000 * MEASURED / terminal_ms,
        }
    return value


def terminal(duration: float) -> dict[str, object]:
    return {
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
        "submission_counter_before_first": 40,
        "submission_counter_after_last": 120,
        "started_at_monotonic_ms": 100.0,
        "completed_at_monotonic_ms": 100.0 + duration,
        "duration_ms": duration,
    }


def presentation(trace: int) -> dict[str, object]:
    return {
        "trace_frame_index": trace,
        "camera_receipt_sha256": hashlib.sha256(f"camera-{trace}".encode()).hexdigest(),
        "presentation_receipt_sha256": hashlib.sha256(f"present-{trace}".encode()).hexdigest(),
        "successful_present": True,
        "queue_terminal_complete": True,
        "captured_after_terminal": True,
        "dimensions": {
            **{f"{stage}_width": WIDTH for stage in ("requested", "surface", "internal_render", "presented")},
            **{f"{stage}_height": HEIGHT for stage in ("requested", "surface", "internal_render", "presented")},
        },
    }


def protocol() -> dict[str, object]:
    return {
        "dataset": TRUCK,
        "trace": TRACE,
        "display": {"width": WIDTH, "height": HEIGHT, "dpr": 1},
        "camera_mode": "trace_sequence",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_frames": WARMUP,
        "measured_frames": MEASURED,
        "terminal_boundary": "first_measured_camera_input_to_final_gpu_queue_completion",
        "claim_scope": "near-contract",
        "quality_gate": {"metric": "ssim-luma-srgb-window8", "minimum_ssim": 0.99},
    }


def manifest(
    *, endpoint: str, role: str, run_id: str, series_id: str, schedule_sha: str,
    protocol_sha: str, pair_id: str, order: str, position: int, configuration: str,
    terminal_ms: float | None, started_at: str, ended_at: str,
) -> dict[str, object]:
    renderer = (
        {"implementation": "playcanvas-d5fe888", "path": "GSplatHybridRenderer", "backend": "webgpu", "sort_policy": "raster_gpu_sort", "uses_gpu_sort": True}
        if endpoint == "playcanvas"
        else {"implementation": "gsplat-rs", "path": "wasm_packed_atlas", "backend": "webgpu", "sort_policy": "gpu_every_frame", "order_backend_requested": "gpu", "gpu_order_producer_actual": "preproject", "projected_policy_requested": "compact", "raster_execution_plan": "projected_quads_exact", "sort_interval": 1}
    )
    if endpoint == "gsplat_rs" and role == "control":
        renderer["count_semantics"] = "candidate_visible_contributor_issued_v1"
    build: dict[str, object] = {"repository_commit": COMMIT, "dirty": False, "profile": "browser", "package_version": "0.1.3", "artifacts": {"runtime": SHA_A if endpoint == "playcanvas" else SHA_B}}
    if endpoint == "playcanvas":
        build.update({"package_version": PLAYCANVAS["version"], "upstream_revision": PLAYCANVAS["revision"], "runtime_revision": PLAYCANVAS["runtime_revision"], "package_integrity": PLAYCANVAS["integrity"]})
    unavailable = ["frames[*].preprocess_ms", "frames[*].sort_ms", "frames[*].geometry_submit_ms", "frames[*].gpu_wait_ms", "frames[*].gpu_complete_ms"]
    if role == "throughput" or endpoint == "playcanvas":
        unavailable += ["frames[*].visible", "frames[*].contributor", "frames[*].drawn"]
    q1: dict[str, object] = {
        "artifact_role": role,
        "protocol_sha256": protocol_sha,
        "configuration_sha256": configuration,
        "performance_evidence": role == "throughput",
        "count_scope": {
            ("playcanvas", "control"): "full_membership_v_c_d_unavailable",
            ("playcanvas", "throughput"): "full_membership_v_c_d_unavailable",
            ("gsplat_rs", "control"): "exact_v_c_d_control_only",
            ("gsplat_rs", "throughput"): "control_bound_v_c_d_unavailable",
        }[(endpoint, role)],
    }
    if role == "control":
        q1["presentation_receipts"] = [presentation(0), presentation(1)]
    else:
        q1["terminal_window"] = terminal(terminal_ms or 1.0)
    return {
        "schema": "gsplat-benchmark/v1",
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {"series_id": series_id, "started_at_utc": started_at, "ended_at_utc": ended_at, "measurement_started_at_utc": started_at, "measurement_ended_at_utc": ended_at},
        "build": build,
        "dataset": TRUCK,
        "trace": {**TRACE, "camera_mode": "trace_sequence"},
        "renderer": renderer,
        "display": {"width": WIDTH, "height": HEIGHT, "dpr": 1, "refresh_hz": 60, "frame_budget_ms": 16.666666666666668, "refresh_hz_source": "configured", "frame_budget_source": "configured"},
        "environment": {"platform": "web", "os": "Darwin-test", "device": "M4-test", "browser": "Chrome-test", "adapter": "Apple M4", "driver": "Metal-test", "browser_executable_sha256": SHA_A, "browser_launch_args_sha256": SHA_B, "adapter_limits_sha256": SHA_A, "power_source": "ac", "collection_session_id": "session-1", "thermal": {"source": "host-probe", "pre": "nominal", "post": "nominal", "admitted": True}},
        "unavailable_fields": unavailable,
        "exactness": {"source_splat_count": TRUCK["splat_count"], "decoded_splat_count": TRUCK["splat_count"], "encoded_splat_count": TRUCK["splat_count"], "resident_splat_count": TRUCK["splat_count"], "addressable_splat_count": TRUCK["splat_count"], "source_sh_degree": 3, "resident_sh_degree": 3, "source_membership": "all", "sampling": "disabled", "lod": "disabled", "partial_scene_published": False, "full_quality": True},
        "resolution": {**{f"{stage}_width": WIDTH for stage in ("requested", "surface", "internal_render", "presented")}, **{f"{stage}_height": HEIGHT for stage in ("requested", "surface", "internal_render", "presented")}, "dynamic_resolution": "disabled", "upscaling": "disabled", "full_resolution": True},
        "pairing": {"series_id": series_id, "schedule_sha256": schedule_sha, "pair_id": pair_id, "run_order": order, "position": position, "fresh_output": True, "automatic_retry": False},
        "q1_comparison": q1,
    }


def write_artifact(root: pathlib.Path, relative: pathlib.Path, doc: dict[str, object], endpoint: str, role: str, terminal_ms: float | None) -> pathlib.Path:
    directory = root / relative
    directory.mkdir(parents=True)
    write_json(directory / "manifest.json", doc)
    records = [frame(str(doc["run_id"]), index, endpoint, role) for index in range(MEASURED)]
    (directory / "frames.jsonl").write_text("".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8")
    write_json(directory / "summary.json", summary(str(doc["run_id"]), terminal_ms))
    return directory


def build_series(root: pathlib.Path, *, gs_terminal_ms: float = 800.0, score: float = 0.995) -> pathlib.Path:
    series_id = "q1-truck-test"
    schedule_block = {"seed": 20260728, "predeclared_at_utc": PREDECLARED, "pairs": [{"pair_id": f"pair-{index + 1:02d}", "run_order": order} for index, order in enumerate(ORDERS)]}
    schedule_sha = canonical_sha256(schedule_block)
    protocol_value = protocol()
    protocol_sha = canonical_sha256(protocol_value)
    references = []
    for trace in (0, 1):
        path = pathlib.Path("reference") / f"view-{trace}.png"
        digest = write_png_header(root / path)
        references.append({"trace_frame_index": trace, "path": str(path), "sha256": digest})
    pairs = []
    for pair_index, order in enumerate(ORDERS, 1):
        pair_id = f"pair-{pair_index:02d}"
        pair: dict[str, object] = {"pair_id": pair_id, "run_order": order}
        for endpoint in ("playcanvas", "gsplat_rs"):
            position = 1 if (order == "playcanvas-first") == (endpoint == "playcanvas") else 2
            base = pathlib.Path("pairs") / pair_id / endpoint
            configuration = hashlib.sha256(f"config-{endpoint}".encode()).hexdigest()
            slot = (pair_index - 1) * 20 + (position - 1) * 8
            epoch = datetime(2026, 7, 28, tzinfo=timezone.utc)
            control_started = epoch + timedelta(seconds=slot + 1)
            control_ended = control_started + timedelta(seconds=2)
            throughput_started = control_ended + timedelta(seconds=1)
            throughput_ended = throughput_started + timedelta(seconds=2)
            control_doc = manifest(endpoint=endpoint, role="control", run_id=f"{pair_id}-{endpoint}-control", series_id=series_id, schedule_sha=schedule_sha, protocol_sha=protocol_sha, pair_id=pair_id, order=order, position=position, configuration=configuration, terminal_ms=None, started_at=control_started.isoformat(), ended_at=control_ended.isoformat())
            control_dir = write_artifact(root, base / "control", control_doc, endpoint, "control", None)
            throughput_ms = 1000.0 if endpoint == "playcanvas" else gs_terminal_ms
            throughput_doc = manifest(endpoint=endpoint, role="throughput", run_id=f"{pair_id}-{endpoint}-throughput", series_id=series_id, schedule_sha=schedule_sha, protocol_sha=protocol_sha, pair_id=pair_id, order=order, position=position, configuration=configuration, terminal_ms=throughput_ms, started_at=throughput_started.isoformat(), ended_at=throughput_ended.isoformat())
            throughput_doc["q1_comparison"]["control_binding"] = {"run_id": control_doc["run_id"], "manifest_sha256": file_sha256(control_dir / "manifest.json"), "configuration_sha256": configuration}
            write_artifact(root, base / "throughput", throughput_doc, endpoint, "throughput", throughput_ms)
            images = []
            for trace in (0, 1):
                image_path = base / f"view-{trace}.png"
                image_sha = write_png_header(root / image_path)
                comparison_path = base / f"view-{trace}-comparison.json"
                write_json(root / comparison_path, {"schema": IMAGE_SCHEMA, "metric": "ssim-luma-srgb-window8", "tool": "tests/perf/compare-image-ssim.mjs", "tool_sha256": SHA_A, "trace_frame_index": trace, "reference_sha256": references[trace]["sha256"], "candidate_sha256": image_sha, "width": WIDTH, "height": HEIGHT, "minimum_ssim": 0.99, "score": score})
                present = presentation(trace)
                images.append({"trace_frame_index": trace, "path": str(image_path), "sha256": image_sha, "camera_receipt_sha256": present["camera_receipt_sha256"], "presentation_receipt_sha256": present["presentation_receipt_sha256"], "comparison": str(comparison_path)})
            pair[endpoint] = {"position": position, "control": str(base / "control"), "throughput": str(base / "throughput"), "images": images}
        pairs.append(pair)
    schedule_path = root / "schedule.json"
    write_json(schedule_path, {"schema": SCHEMA, "series_id": series_id, "schedule": schedule_block, "protocol": protocol_value, "reference_images": references, "pairs": pairs})
    return schedule_path


class ScheduleAndAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.schedule = build_series(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def mutate_manifest(self, relative: str, callback) -> None:
        path = self.root / relative / "manifest.json"
        value = json.loads(path.read_text(encoding="utf-8"))
        callback(value)
        write_json(path, value)

    def test_complete_five_pair_series_is_accepted(self) -> None:
        result = evaluate(self.schedule)
        self.assertEqual(result["state"], "Accepted")
        self.assertTrue(result["evidence_admitted"])
        self.assertEqual(result["pair_count"], 5)
        self.assertLess(result["performance"]["gsplat_rs_over_playcanvas_ratio"], 1)

    def test_null_pairing_from_historical_artifact_is_rejected(self) -> None:
        self.mutate_manifest("pairs/pair-01/playcanvas/control", lambda value: value.__setitem__("pairing", {"pair_id": None, "run_order": None, "position": None}))
        with self.assertRaisesRegex(ValidationError, "pairing.series_id"):
            evaluate(self.schedule)

    def test_non_common_terminal_primitive_is_rejected(self) -> None:
        self.mutate_manifest("pairs/pair-01/gsplat_rs/throughput", lambda value: value["q1_comparison"]["terminal_window"].__setitem__("completion_primitive", "renderer_current_stats_map"))
        with self.assertRaisesRegex(ValidationError, "completion_primitive"):
            evaluate(self.schedule)

    def test_missing_build_hash_is_rejected(self) -> None:
        self.mutate_manifest("pairs/pair-01/playcanvas/control", lambda value: value["build"].__setitem__("artifacts", {}))
        with self.assertRaisesRegex(ValidationError, "build.artifacts"):
            evaluate(self.schedule)

    def test_missing_common_image_receipt_is_rejected(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["pairs"][0]["playcanvas"]["images"][0]["comparison"] = "missing.json"
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "comparison does not name a file"):
            evaluate(self.schedule)

    def test_image_receipt_without_comparator_hash_is_rejected(self) -> None:
        receipt = self.root / "pairs/pair-01/playcanvas/view-0-comparison.json"
        value = json.loads(receipt.read_text(encoding="utf-8"))
        del value["tool_sha256"]
        write_json(receipt, value)
        with self.assertRaisesRegex(ValidationError, "tool_sha256"):
            evaluate(self.schedule)

    def test_pair_order_label_without_timestamp_proof_is_rejected(self) -> None:
        control = self.root / "pairs/pair-01/gsplat_rs/control"
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/control",
            lambda value: value["identity"].__setitem__(
                "started_at_utc", "2026-07-28T00:00:02+00:00"
            ),
        )
        throughput = self.root / "pairs/pair-01/gsplat_rs/throughput/manifest.json"
        throughput_value = json.loads(throughput.read_text(encoding="utf-8"))
        throughput_value["q1_comparison"]["control_binding"]["manifest_sha256"] = (
            file_sha256(control / "manifest.json")
        )
        write_json(throughput, throughput_value)
        with self.assertRaisesRegex(ValidationError, "timestamps do not prove"):
            evaluate(self.schedule)

    def test_cross_endpoint_camera_receipt_mismatch_is_rejected(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/control",
            lambda value: value["q1_comparison"]["presentation_receipts"][0].__setitem__(
                "camera_receipt_sha256", SHA_B
            ),
        )
        control = self.root / "pairs/pair-01/gsplat_rs/control/manifest.json"
        throughput = self.root / "pairs/pair-01/gsplat_rs/throughput/manifest.json"
        throughput_value = json.loads(throughput.read_text(encoding="utf-8"))
        throughput_value["q1_comparison"]["control_binding"]["manifest_sha256"] = (
            file_sha256(control)
        )
        write_json(throughput, throughput_value)
        schedule_value = json.loads(self.schedule.read_text(encoding="utf-8"))
        schedule_value["pairs"][0]["gsplat_rs"]["images"][0][
            "camera_receipt_sha256"
        ] = SHA_B
        write_json(self.schedule, schedule_value)
        with self.assertRaisesRegex(ValidationError, "live-camera receipts differ"):
            evaluate(self.schedule)

    def test_playcanvas_vcd_must_remain_unavailable(self) -> None:
        path = self.root / "pairs/pair-01/playcanvas/control/frames.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()]
        records[0]["visible"] = TRUCK["splat_count"]
        records[0]["drawn"] = TRUCK["splat_count"]
        path.write_text("".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8")
        with self.assertRaisesRegex(ValidationError, "PlayCanvas V/C/D"):
            evaluate(self.schedule)

    def test_gsplat_throughput_cannot_copy_control_counts(self) -> None:
        path = self.root / "pairs/pair-01/gsplat_rs/throughput/frames.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()]
        records[0]["visible"] = 10
        records[0]["drawn"] = 10
        path.write_text("".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8")
        with self.assertRaises(ValidationError):
            evaluate(self.schedule)


class FiniteVerdictTests(unittest.TestCase):
    def test_cli_publishes_admitted_finite_result(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            schedule = build_series(root)
            output = root / "result.json"
            completed = subprocess.run(
                [sys.executable, str(CLI), str(schedule), "--output", str(output)],
                check=False,
                capture_output=True,
                text=True,
            )
            result = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertTrue(result["evidence_admitted"])

    def test_cli_admission_failure_is_exit_two_and_no_performance_claim(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            schedule = build_series(root)
            value = json.loads(schedule.read_text(encoding="utf-8"))
            value["pairs"][0]["pair_id"] = "not-predeclared"
            write_json(schedule, value)
            output = root / "result.json"
            completed = subprocess.run(
                [sys.executable, str(CLI), str(schedule), "--output", str(output)],
                check=False,
                capture_output=True,
                text=True,
            )
            result = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(completed.returncode, 2)
        self.assertFalse(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertFalse(result["retry_authorized"])

    def test_slower_valid_series_is_finite_rejected_without_retry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = evaluate(build_series(pathlib.Path(directory), gs_terminal_ms=1200.0))
        self.assertEqual(result["state"], "Rejected")
        self.assertTrue(result["evidence_admitted"])
        self.assertEqual(result["reasons"], ["gsplat_rs_slower_on_paired_median_terminal_window"])
        self.assertFalse(result["retry_authorized"])

    def test_quality_failure_has_no_performance_claim(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = evaluate(build_series(pathlib.Path(directory), score=0.98))
        self.assertEqual(result["state"], "Rejected")
        self.assertTrue(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertEqual(result["reasons"], ["common_reference_image_gate_failed"])

    def test_admission_rejection_has_no_performance_or_retry(self) -> None:
        result = admission_rejection("series", "missing common terminal")
        self.assertFalse(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertFalse(result["retry_authorized"])


if __name__ == "__main__":
    unittest.main()
