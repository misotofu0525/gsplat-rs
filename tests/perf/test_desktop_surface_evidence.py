#!/usr/bin/env python3
"""Focused fail-closed tests for the M2b desktop Surface collector."""

from __future__ import annotations

import copy
import importlib.util
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tests/perf/collect-desktop-surface-evidence.py"
SPEC = importlib.util.spec_from_file_location("desktop_surface_evidence", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)

DATASET = {
    "id": "kitsune",
    "sha256": "a" * 64,
    "bytes": 123,
    "splat_count": 100,
    "sh_degree": 3,
}
TRACE = {
    "trace_id": "kitsune-1080p",
    "content_sha256": "b" * 64,
    "file_sha256": "c" * 64,
    "width": 1920,
    "height": 1080,
}
WARMUP = 1
MEASURED = 2


def line(prefix: str, fields: dict[str, str]) -> str:
    return prefix + " ".join(f"{key}={value}" for key, value in fields.items())


def actuals(arm: str) -> list[str]:
    if arm == "adaptive":
        return ["cpu_post_sort", "gpu_post_sort", "gpu_preproject", "gpu_post_sort"]
    return [arm] * 4


def semantics(plan: str) -> tuple[str, bool, int, int, str]:
    if plan == "cpu_post_sort":
        return "direct_draw_equals_visible", False, 80, 80, "cpu"
    if plan == "gpu_post_sort":
        return "indirect_draw_equals_visible", False, 80, 80, "gpu"
    return "indirect_draw_equals_contributor", True, 70, 70, "gpu"


def synthetic_records(arm: str) -> dict[str, list[dict[str, str]]]:
    plans = actuals(arm)
    begin = {
        "trace_id": TRACE["trace_id"],
        "trace_sha256": TRACE["content_sha256"],
        "exact_plan_requested": arm,
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "blend_mode": "sorted_alpha",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "source_count": "100",
        "decoded_count": "100",
        "encoded_count": "100",
        "resident_count": "100",
        "addressable_count": "100",
        "sh_degree": "3",
        "requested_width": "1920",
        "requested_height": "1080",
        "surface_width": "1920",
        "surface_height": "1080",
        "internal_render_width": "1920",
        "internal_render_height": "1080",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
        "trace_frames": "3",
        "adapter_backend": "metal",
        "adapter_name": "Apple-M4",
        "adapter_device_type": "integrated_gpu",
        "adapter_driver": "unavailable",
        "adapter_driver_info": "unavailable",
    }
    frames: list[dict[str, str]] = []
    for index in range(3):
        plan = plans[index]
        count_semantics, compacted, contributor, drawn, backend = semantics(plan)
        frames.append(
            {
                "trace_id": TRACE["trace_id"],
                "trace_sha256": TRACE["content_sha256"],
                "playback_index": str(index),
                "phase": "warmup" if index == 0 else "measure",
                "measured_sample": "none" if index == 0 else str(index - 1),
                "trace_frame": str(index % 2),
                "trace_timestamp_ns": str(index * 100),
                "elapsed_ns": str(1_000 + index),
                "exact_plan_requested": arm,
                "exact_plan_actual": plan,
                "current_stats_ticket": str(10 + index),
                "scene_generation": "1",
                "camera_revision": str(index),
                "viewport_generation": "1",
                "contract_generation": "1",
                "plan_set_generation": "1",
                "order_generation": str(20 + index),
                "raster_generation": "1",
                "encode_attempt": str(30 + index),
                "presentation_sequence": str(40 + index),
                "count_semantics": count_semantics,
                "source_count": "100",
                "visible_count": "80",
                "contributor_count": str(contributor),
                "drawn_count": str(drawn),
                "exact_contributor_compaction": str(compacted).lower(),
                "sort_refreshed": "true",
                "actual_backend": backend,
                "call_ms": f"{1.0 + index:.6f}",
                "frame_wall_ms": f"{1.5 + index:.6f}",
                "requested_width": "1920",
                "requested_height": "1080",
                "presented_width": "1920",
                "presented_height": "1080",
                "frame_presented": "true",
                "terminal_receipt": "ready",
            }
        )
    capture_plan = plans[3]
    count_semantics, compacted, contributor, drawn, backend = semantics(capture_plan)
    capture = {
        "status": "ok",
        "path": "/tmp/final-frame.png",
        "trace_frame": frames[-1]["trace_frame"],
        "exact_plan_requested": arm,
        "exact_plan_actual": capture_plan,
        "current_stats_ticket": "99",
        "camera_revision": "3",
        "presentation_sequence": "99",
        "count_semantics": count_semantics,
        "source_count": "100",
        "visible_count": "80",
        "contributor_count": str(contributor),
        "drawn_count": str(drawn),
        "exact_contributor_compaction": str(compacted).lower(),
        "actual_backend": backend,
        "requested_width": "1920",
        "requested_height": "1080",
        "captured_width": "1920",
        "captured_height": "1080",
        "frame_presented": "true",
        "terminal_receipt": "ready",
    }
    plan_set = sorted(set(plans))
    summary = {
        "status": "ok",
        "exact_plan_requested": arm,
        "actual_plan_set": ",".join(plan_set),
        "trace_frames": "3",
        "measured_frames": "2",
        "eligibility_retries": "0",
        "capture_retries": "0",
        "terminal_receipts": "4",
        "final_capture": "available",
    }
    return {"begin": [begin], "frame": frames, "capture": [capture], "summary": [summary]}


def render(records: dict[str, list[dict[str, str]]]) -> str:
    output: list[str] = []
    for name in ("begin", "frame", "capture", "summary"):
        prefix = COLLECTOR.PREFIXES[name]
        output.extend(line(prefix, record) for record in records[name])
    return "\n".join(output) + "\n"


def validate(records: dict[str, list[dict[str, str]]], arm: str):
    return COLLECTOR.validate_run_log(
        render(records), "", arm=arm, dataset=DATASET, trace=TRACE,
        warmup=WARMUP, measured=MEASURED,
    )


def minimal_png(path: pathlib.Path, width: int = 1920, height: int = 1080) -> None:
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + (13).to_bytes(4, "big")
        + b"IHDR"
        + width.to_bytes(4, "big")
        + height.to_bytes(4, "big")
    )


class ReceiptValidationTests(unittest.TestCase):
    def test_all_four_plan_arms_validate(self) -> None:
        for arm in COLLECTOR.ARMS:
            with self.subTest(arm=arm):
                result = validate(synthetic_records(arm), arm)
                self.assertEqual(len(result["frames"]), MEASURED)
                self.assertIn(result["final_actual_plan"], COLLECTOR.PLAN_SEMANTICS)

    def test_missing_capture_or_terminal_never_validates(self) -> None:
        records = synthetic_records("cpu_post_sort")
        records["capture"].clear()
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "exactly one"):
            validate(records, "cpu_post_sort")

        records = synthetic_records("cpu_post_sort")
        records["frame"][1]["terminal_receipt"] = "map_failure"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "terminal_receipt"):
            validate(records, "cpu_post_sort")

    def test_forced_plan_drift_and_adaptive_unknown_plan_fail(self) -> None:
        records = synthetic_records("gpu_post_sort")
        records["frame"][1]["exact_plan_actual"] = "cpu_post_sort"
        records["frame"][1]["count_semantics"] = "direct_draw_equals_visible"
        records["frame"][1]["actual_backend"] = "cpu"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "forced plan drift"):
            validate(records, "gpu_post_sort")

        records = synthetic_records("adaptive")
        records["frame"][1]["exact_plan_actual"] = "adaptive"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "invalid actual plan"):
            validate(records, "adaptive")

    def test_counts_resolution_and_membership_fail_closed(self) -> None:
        mutations = [
            ("frame", 1, "drawn_count", "79", "D=C or D=V"),
            ("frame", 1, "visible_count", "101", "C<=V<=S"),
            ("frame", 1, "actual_backend", "cpu", "actual_backend"),
            ("frame", 1, "sort_refreshed", "false", "sort_refreshed"),
            ("frame", 1, "presented_width", "960", "presented_width"),
            ("begin", 0, "resident_count", "99", "resident_count"),
            ("begin", 0, "internal_render_width", "960", "internal_render_width"),
            ("capture", 0, "captured_width", "960", "captured_width"),
        ]
        for group, index, key, value, message in mutations:
            with self.subTest(group=group, key=key):
                records = synthetic_records("gpu_post_sort")
                records[group][index][key] = value
                with self.assertRaisesRegex(COLLECTOR.ValidationError, message):
                    validate(records, "gpu_post_sort")

    def test_duplicate_ticket_or_presentation_fails(self) -> None:
        records = synthetic_records("gpu_preproject")
        records["frame"][1]["current_stats_ticket"] = records["frame"][0]["current_stats_ticket"]
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "duplicate current-stats"):
            validate(records, "gpu_preproject")

        records = synthetic_records("gpu_preproject")
        records["capture"][0]["presentation_sequence"] = records["frame"][0]["presentation_sequence"]
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "reused.*presentation"):
            validate(records, "gpu_preproject")


class CanonicalArtifactTests(unittest.TestCase):
    def test_built_artifact_passes_repository_validator(self) -> None:
        validated = validate(synthetic_records("adaptive"), "adaptive")
        build = {
            "binary_sha256": "d" * 64,
            "package_version": "0.1.3",
            "git": {
                "commit": "e" * 40,
                "dirty": False,
                "status_porcelain_sha256": "f" * 64,
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            artifact = pathlib.Path(temporary)
            capture = artifact / "final-frame.png"
            minimal_png(capture)
            COLLECTOR.build_artifact(
                artifact, arm="adaptive", validated=validated, dataset=DATASET, trace=TRACE,
                build=build, capture_path=capture,
                started_at="2026-07-25T00:00:00Z", ended_at="2026-07-25T00:00:01Z",
                warmup=WARMUP, refresh_hz=60.0, host_device=None,
            )
            completed = subprocess.run(
                [sys.executable, str(ROOT / "tests/perf/validate-benchmark-artifacts.py"), str(artifact)],
                cwd=ROOT, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)

    def test_damaged_or_mismatched_image_is_rejected(self) -> None:
        validated = validate(synthetic_records("cpu_post_sort"), "cpu_post_sort")
        build = {
            "binary_sha256": "d" * 64,
            "package_version": "0.1.3",
            "git": {
                "commit": "e" * 40,
                "dirty": False,
                "status_porcelain_sha256": "f" * 64,
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            artifact = pathlib.Path(temporary)
            capture = artifact / "final-frame.png"
            minimal_png(capture)
            COLLECTOR.build_artifact(
                artifact, arm="cpu_post_sort", validated=validated, dataset=DATASET, trace=TRACE,
                build=build, capture_path=capture,
                started_at="2026-07-25T00:00:00Z", ended_at="2026-07-25T00:00:01Z",
                warmup=WARMUP, refresh_hz=60.0, host_device=None,
            )
            capture.write_bytes(b"not a png")
            completed = subprocess.run(
                [sys.executable, str(ROOT / "tests/perf/validate-benchmark-artifacts.py"), str(artifact)],
                cwd=ROOT, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            self.assertNotEqual(completed.returncode, 0)

    def test_incomplete_or_unvalidated_suite_is_never_published(self) -> None:
        complete_runs = []
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            stage = root / ".stage"
            output = root / "canonical"
            for arm in COLLECTOR.ARMS:
                artifact = stage / arm / "artifact"
                artifact.mkdir(parents=True)
                for name in ("manifest.json", "frames.jsonl", "summary.json", "final-frame.png"):
                    (artifact / name).write_bytes(b"retained")
                complete_runs.append(
                    {
                        "arm": arm,
                        "artifact": f"{arm}/artifact",
                        "validator_exit_status": 0,
                    }
                )

            for runs, message in (
                (complete_runs[:-1], "all four arms"),
                (
                    [*complete_runs[:-1], {**complete_runs[-1], "validator_exit_status": 1}],
                    "validator did not pass",
                ),
            ):
                with self.subTest(message=message):
                    suite = {
                        "schema": COLLECTOR.SUITE_SCHEMA,
                        "status": "ok",
                        "runs": runs,
                    }
                    with self.assertRaisesRegex(COLLECTOR.ValidationError, message):
                        COLLECTOR.publish_validated_suite(stage, output, suite)
                    self.assertFalse(output.exists())
                    self.assertTrue(stage.is_dir())


if __name__ == "__main__":
    unittest.main()
