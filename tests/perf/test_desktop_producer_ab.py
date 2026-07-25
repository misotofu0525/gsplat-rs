#!/usr/bin/env python3
"""Synthetic contract tests for collect-desktop-producer-ab.py."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("collect-desktop-producer-ab.py")
SPEC = importlib.util.spec_from_file_location("desktop_producer_ab", MODULE_PATH)
assert SPEC and SPEC.loader
COLLECTOR = importlib.util.module_from_spec(SPEC)
import sys

sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


COUNT = 100
WARMUP = 1
MEASURED = 2
TRACE = {
    "trace_id": "formal-truck-1080p-v1",
    "content_sha256": "a" * 64,
    "file_sha256": "b" * 64,
    "width": 1920,
    "height": 1080,
}
DATASET = {"splat_count": COUNT, "sh_degree": 3}


def dimensions(*, presented: bool = False) -> dict[str, str]:
    value = {
        "requested_width": "1920",
        "requested_height": "1080",
        "surface_width": "1920",
        "surface_height": "1080",
        "internal_render_width": "1920",
        "internal_render_height": "1080",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
    }
    if presented:
        value.update({"presented_width": "1920", "presented_height": "1080"})
    return value


def identity(producer: str) -> dict[str, str]:
    return {
        "trace_id": TRACE["trace_id"],
        "trace_sha256": TRACE["content_sha256"],
        "benchmark_mode": "isolated",
        "sort_policy": "every_frame",
        "requested_backend": "gpu",
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "gpu_order_producer": producer,
        "projected_draw_policy": "compact",
        "producer_measurement_enabled": "true",
        "source_count": str(COUNT),
        "resident_count": str(COUNT),
        "sh_degree": "3",
    }


def synthetic_records(producer: str = "post_sort") -> dict[str, list[dict[str, str]]]:
    begin = {**identity(producer), **dimensions(), "trace_frames": "3"}
    frames: list[dict[str, str]] = []
    gpu: list[dict[str, str]] = []
    producer_terminals: list[dict[str, str]] = []
    queue_times = (0.8, 1.2, 1.4)
    producer_times = (1.8, 2.2, 2.4)
    for index in range(3):
        measured = index >= WARMUP
        # The runtime's first applied camera state is revision zero.
        revision = index
        order_ticket = index + 100
        producer_ticket = index + 200
        frames.append(
            {
                "playback_index": str(index),
                "phase": "measure" if measured else "warmup",
                "measured_sample": str(index - WARMUP) if measured else "none",
                "camera_revision": str(revision),
                "sort_policy": "every_frame",
                "requested_backend": "gpu",
                "actual_backend": "gpu",
                "raster_execution_plan": "projected_quads_exact",
                "gpu_order_producer_requested": producer,
                "gpu_order_producer_actual": producer,
                "producer_ticket_submitted": str(producer_ticket),
                "producer_unsampled_reason": "none",
                "frame_presented": "true",
                "tiled_preparation_pending": "false",
                **dimensions(presented=True),
                "sort_refreshed": "true",
                "gpu_sort_fallback": "false",
                "source_count": str(COUNT),
                "resident_count": str(COUNT),
                "measurement_ticket_submitted": str(order_ticket),
                "measurement_backend": "gpu",
                "measurement_unsampled_reason": "none",
                "gpu_ticket_submitted": str(order_ticket),
            }
        )
        gpu.append(
            {
                "ticket": str(order_ticket),
                "camera_revision": str(revision),
                "measured": str(measured).lower(),
                "gpu_completion_ms": str(queue_times[index]),
                "visible_count": "90",
                "contributor_count": "80",
                "drawn_count": "80",
                "exact_contributor_compaction": "true",
            }
        )
        producer_terminals.append(
            {
                "ticket": str(producer_ticket),
                "camera_revision": str(revision),
                "measured": str(measured).lower(),
                "producer": producer,
                "order_generation": str(index + 1),
                "projection_generation": str(index + 20),
                "source_count": str(COUNT),
                "contributor_count": "80",
                "drawn_count": "80",
                "order_refreshed": "true",
                "draw_scope": "exact_current_contributors",
                "exact_current_contributor_draw": "true",
                "stale_order": "false",
                "frame_completion_ms": str(producer_times[index]),
            }
        )
    summary = {
        **identity(producer),
        **dimensions(presented=True),
        "status": "ok",
        "final_actual_backend": "gpu",
        "trace_frames": "3",
        "presented_frames": "3",
        "measured_frames": "2",
        "measured_cpu_frames": "0",
        "measured_gpu_frames": "2",
        "sort_refreshes": "3",
        "gpu_fallback_frames": "0",
        "gpu_refreshes_without_ticket": "0",
        "cpu_requests_without_ticket": "0",
        "surface_unavailable_measurements": "0",
        "producer_exact_frames": "3",
        "producer_stale_frames": "0",
        "producer_ring_busy": "0",
        "producer_surface_unavailable": "0",
        "terminal_order_tickets": "3",
        "terminal_producer_tickets": "3",
        "outstanding_cpu_tickets": "0",
        "outstanding_gpu_tickets": "0",
        "outstanding_producer_tickets": "0",
        "mean_gpu_completion_ms": "1.3",
        "mean_gpu_producer_completion_ms": "2.3",
    }
    return {
        "begin": [begin],
        "frame": frames,
        "gpu": gpu,
        "producer": producer_terminals,
        "capture": [{
            "status": "ok",
            "path": "/tmp/frame-0.png",
            "trace_frame": "0",
            "requested_width": "1920",
            "requested_height": "1080",
            "captured_width": "1920",
            "captured_height": "1080",
            "gpu_order_producer_requested": producer,
            "gpu_order_producer_actual": producer,
            "producer_measurement_enabled": "false",
            "measured": "false",
            "order_ticket": "999",
            "terminal_receipt": "success",
        }],
        "summary": [summary],
    }


def render(records: dict[str, list[dict[str, str]]]) -> str:
    lines: list[str] = []
    for name in ("begin", "frame", "gpu", "producer", "capture", "summary"):
        prefix = COLLECTOR.RECORD_PREFIXES[name]
        for record in records[name]:
            lines.append(prefix + " ".join(f"{key}={value}" for key, value in record.items()))
    return "\n".join(lines) + "\n"


def validate(records: dict[str, list[dict[str, str]]], producer: str = "post_sort"):
    return COLLECTOR.validate_run_log(
        render(records),
        "",
        producer=producer,
        dataset=DATASET,
        trace=TRACE,
        warmup=WARMUP,
        measured=MEASURED,
        mode="isolated",
    )


class RunReceiptTests(unittest.TestCase):
    def test_valid_post_sort_and_preproject_receipts(self) -> None:
        post = validate(synthetic_records("post_sort"), "post_sort")
        pre = validate(synthetic_records("preproject"), "preproject")
        self.assertEqual(post["queue_complete_ms"]["count"], MEASURED)
        self.assertEqual(pre["producer_completion_ms"]["median"], 2.3)

    def test_requested_and_actual_producer_must_match(self) -> None:
        records = synthetic_records()
        records["frame"][1]["gpu_order_producer_actual"] = "preproject"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "actual"):
            validate(records)

    def test_every_measured_frame_requires_a_ticket(self) -> None:
        records = synthetic_records()
        records["frame"][1]["producer_ticket_submitted"] = "none"
        records["frame"][1]["producer_unsampled_reason"] = "ring_busy"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "unsampled|ticket"):
            validate(records)

    def test_duplicate_terminal_ticket_fails(self) -> None:
        records = synthetic_records()
        records["producer"].append(dict(records["producer"][1]))
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "more than one terminal"):
            validate(records)

    def test_missing_terminal_ticket_fails(self) -> None:
        records = synthetic_records()
        records["gpu"].pop()
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "exactly one terminal"):
            validate(records)

    def test_contributor_must_equal_drawn(self) -> None:
        records = synthetic_records()
        records["producer"][1]["drawn_count"] = "79"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "C=D<=S"):
            validate(records)

    def test_same_frame_order_and_producer_counts_must_agree(self) -> None:
        records = synthetic_records()
        # This producer receipt is internally valid (C=D), but it belongs to a
        # frame whose order terminal proves C=D=80.  The frame ticket join must
        # reject the cross-stream contradiction.
        records["producer"][1]["contributor_count"] = "79"
        records["producer"][1]["drawn_count"] = "79"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "same-frame.*disagree"):
            validate(records)

    def test_stale_producer_receipt_fails(self) -> None:
        records = synthetic_records()
        records["producer"][1].update(
            {
                "order_refreshed": "false",
                "draw_scope": "stale_order_candidates",
                "exact_current_contributor_draw": "false",
                "stale_order": "true",
            }
        )
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "order_refreshed|draw_scope"):
            validate(records)

    def test_internal_resolution_downscale_fails(self) -> None:
        records = synthetic_records()
        records["frame"][1]["internal_render_width"] = "960"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "internal_render_width"):
            validate(records)

    def test_summary_outstanding_or_ring_busy_fails(self) -> None:
        records = synthetic_records()
        records["summary"][0]["outstanding_producer_tickets"] = "1"
        records["summary"][0]["producer_ring_busy"] = "1"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "producer_ring_busy|outstanding"):
            validate(records)

    def test_incomplete_source_or_sh_fails(self) -> None:
        records = synthetic_records()
        records["begin"][0]["resident_count"] = "99"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "resident_count"):
            validate(records)

    def test_summary_mean_must_match_terminal_distribution(self) -> None:
        records = synthetic_records()
        records["summary"][0]["mean_gpu_completion_ms"] = "9.0"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "mean disagrees"):
            validate(records)

    def test_missing_or_malformed_surface_capture_fails(self) -> None:
        records = synthetic_records()
        records["capture"].clear()
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "SURFACE_CAPTURE"):
            validate(records)

        records = synthetic_records()
        records["capture"][0]["measured"] = "true"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "capture.measured"):
            validate(records)

        records = synthetic_records()
        records["capture"][0]["gpu_order_producer_actual"] = "preproject"
        with self.assertRaisesRegex(
            COLLECTOR.ValidationError, "capture.gpu_order_producer_actual"
        ):
            validate(records)


class CollectorUtilityTests(unittest.TestCase):
    def test_schedule_and_bootstrap_are_deterministic(self) -> None:
        self.assertEqual(COLLECTOR.schedule_pairs(8, 41), COLLECTOR.schedule_pairs(8, 41))
        for pairs in range(1, 12):
            for seed in (0, 1, 41, 0x4753504C4154):
                schedule = COLLECTOR.schedule_pairs(pairs, seed)
                balance = COLLECTOR.pair_order_balance(schedule)
                self.assertEqual(len(schedule), pairs)
                self.assertLessEqual(balance["absolute_difference"], 1)
                self.assertEqual(balance["ab_pairs"] + balance["ba_pairs"], pairs)
                self.assertEqual(
                    balance["odd_pair_extra_order"] is None,
                    pairs % 2 == 0,
                )
        odd_extra_orders = {
            tuple(COLLECTOR.pair_order_balance(COLLECTOR.schedule_pairs(5, seed))[
                "odd_pair_extra_order"
            ])
            for seed in range(64)
        }
        self.assertEqual(
            odd_extra_orders,
            {tuple(COLLECTOR.PRODUCERS), tuple(reversed(COLLECTOR.PRODUCERS))},
        )
        values = [0.8, 1.0, 1.2, 0.9, 1.1]
        first = COLLECTOR.bootstrap_median_ci(values, seed=7, samples=2_000)
        second = COLLECTOR.bootstrap_median_ci(values, seed=7, samples=2_000)
        self.assertEqual(first, second)

    def test_command_pins_full_quality_ab_contract(self) -> None:
        inputs = COLLECTOR.Inputs(
            binary=pathlib.Path("/tmp/release/desktop-example"),
            dataset_path=pathlib.Path("/tmp/truck.ply"),
            trace_path=pathlib.Path("/tmp/trace.json"),
            output=pathlib.Path("/tmp/out"),
            pairs=5,
            seed=1,
            warmup=20,
            measured=80,
            mode="isolated",
        )
        command = COLLECTOR.make_command(
            inputs,
            "preproject",
            pathlib.Path("/tmp/out/frame-0.png"),
        )
        joined = " ".join(command)
        for expected in (
            "--geometry-path packed",
            "--surface-sort-policy every-frame",
            "--order-backend gpu",
            "--surface-gpu-producer preproject",
            "--png /tmp/out/frame-0.png",
        ):
            self.assertIn(expected, joined)
        self.assertNotIn("--surface-raster-plan", joined)

    def test_png_dimensions_reads_the_ihdr_and_rejects_non_png(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "capture.png"
            path.write_bytes(
                b"\x89PNG\r\n\x1a\n"
                + (13).to_bytes(4, "big")
                + b"IHDR"
                + (1920).to_bytes(4, "big")
                + (1080).to_bytes(4, "big")
            )
            self.assertEqual(COLLECTOR.png_dimensions(path), (1920, 1080))
            path.write_bytes(b"not a png")
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "not a PNG"):
                COLLECTOR.png_dimensions(path)

    def test_ply_receipt_requires_all_45_sh3_rest_properties(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "scene.ply"
            properties = "".join(f"property float f_rest_{index}\n" for index in range(45))
            path.write_bytes(
                ("ply\nformat binary_little_endian 1.0\nelement vertex 7\n"
                 "property float x\n" + properties + "end_header\n").encode("ascii")
            )
            receipt = COLLECTOR.read_ply_receipt(path)
            self.assertEqual(receipt["splat_count"], 7)
            self.assertEqual(receipt["sh_degree"], 3)
            path.write_bytes(path.read_bytes().replace(b"f_rest_44", b"f_other_44"))
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "complete SH3"):
                COLLECTOR.read_ply_receipt(path)

    def test_trace_receipt_rejects_diagnostic_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "trace.json"
            path.write_text(
                json.dumps(
                    {
                        "trace_id": "diagnostic",
                        "content_sha256": "c" * 64,
                        "display": {"width": 640, "height": 480},
                        "frames": [{}],
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "1920x1080"):
                COLLECTOR.read_trace_receipt(path)


if __name__ == "__main__":
    unittest.main()
