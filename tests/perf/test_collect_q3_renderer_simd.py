import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("collect-q3-renderer-simd.py")
SPEC = importlib.util.spec_from_file_location("collect_q3_renderer_simd", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


def line(prefix: str, fields: dict[str, object]) -> str:
    return prefix + " ".join(f"{key}={str(value).lower() if isinstance(value, bool) else value}" for key, value in fields.items())


def dimensions() -> dict[str, object]:
    return {
        "requested_width": 1920,
        "requested_height": 1080,
        "surface_width": 1920,
        "surface_height": 1080,
        "internal_render_width": 1920,
        "internal_render_height": 1080,
        "presented_width": 1920,
        "presented_height": 1080,
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": True,
    }


def fixture_log() -> str:
    common = {
        "trace_id": "truck-trace",
        "trace_sha256": "a" * 64,
        "benchmark_mode": "isolated",
        "sort_policy": "every_frame",
        "requested_backend": "cpu",
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "gpu_order_producer": "product-default",
        "producer_measurement_enabled": False,
        "source_count": 100,
        "resident_count": 100,
        "sh_degree": 3,
    }
    begin = {
        **common,
        "sort_interval": 1,
        **{key: value for key, value in dimensions().items() if not key.startswith("presented_")},
        "trace_frames": 3,
    }
    lines = [line("SURFACE_BENCHMARK_BEGIN ", begin)]
    frame_values = [
        ("warmup", "none", 0, 0, 1.0, 2.0, 5.0),
        ("measure", 0, 0, 0, 1.5, 2.5, 6.0),
        ("measure", 1, 1, 16_666_667, 2.5, 3.5, 8.0),
    ]
    for index, (phase, sample, trace_frame, timestamp, preprocess, sort, completion) in enumerate(frame_values):
        frame = {
            "playback_index": index,
            "phase": phase,
            "measured_sample": sample,
            "trace_frame": trace_frame,
            "trace_timestamp_ns": timestamp,
            "camera_revision": index,
            "sort_policy": "every_frame",
            "requested_backend": "cpu",
            "actual_backend": "cpu",
            "raster_execution_plan": "projected_quads_exact",
            "frame_presented": True,
            "gpu_order_preparation_pending": False,
            "sort_refreshed": True,
            "order_uploaded": True,
            "gpu_sort_fallback": False,
            "source_count": 100,
            "resident_count": 100,
            "visible_count": 80,
            "drawn_count": 80,
            "measurement_ticket_submitted": index + 1,
            "measurement_backend": "cpu",
            "measurement_unsampled_reason": "none",
            "gpu_ticket_submitted": "none",
            "cpu_preprocess_ms": preprocess,
            "cpu_sort_ms": sort,
            "cpu_render_submit_ms": preprocess + sort + 1.0,
            "frame_wall_ms": completion + 1.0,
            **dimensions(),
        }
        terminal = {
            "ticket": index + 1,
            "camera_revision": index,
            "measured": phase == "measure",
            "cpu_preprocess_ms": preprocess,
            "cpu_sort_ms": sort,
            "frame_completion_ms": completion,
            "visible_count": 80,
            "contributor_count": 70,
            "drawn_count": 80,
            "exact_contributor_compaction": False,
        }
        lines.append(line("SURFACE_FRAME_RECEIPT ", frame))
        lines.append(line("SURFACE_CPU_MEASUREMENT ", terminal))
    summary = {
        "status": "ok",
        **common,
        "final_actual_backend": "cpu",
        **dimensions(),
        "trace_frames": 3,
        "presented_frames": 3,
        "measured_frames": 2,
        "measured_cpu_frames": 2,
        "measured_gpu_frames": 0,
        "sort_refreshes": 3,
        "gpu_fallback_frames": 0,
        "gpu_refreshes_without_ticket": 0,
        "cpu_requests_without_ticket": 0,
        "surface_unavailable_measurements": 0,
        "terminal_order_tickets": 3,
        "terminal_producer_tickets": 0,
        "outstanding_cpu_tickets": 0,
        "outstanding_gpu_tickets": 0,
        "outstanding_producer_tickets": 0,
        "mean_cpu_preprocess_ms": 2.0,
        "mean_cpu_sort_ms": 3.0,
        "mean_cpu_completion_ms": 7.0,
        "mean_frame_wall_ms": 8.0,
    }
    lines.append(line("SURFACE_BENCHMARK_SUMMARY ", summary))
    return "\n".join(lines) + "\n"


class Q3RendererSimdCollectorTests(unittest.TestCase):
    def test_schedule_is_fixed_and_counterbalanced(self):
        schedule = COLLECTOR.schedule_pairs(5, 0x51334D3453494D44)
        self.assertEqual(len(schedule), 5)
        self.assertLessEqual(
            abs(schedule.count(("scalar", "neon")) - schedule.count(("neon", "scalar"))),
            1,
        )
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "exactly 5"):
            COLLECTOR.schedule_pairs(6, 1)

    def test_fixed_correctness_must_match_host_and_commit(self):
        host = {"system": "Darwin", "machine": "arm64", "cpu_brand": "Apple M4"}
        receipt = {
            "schema": "gsplat-q3-simd-cell/v1",
            "cell": "Q3.M4.PackedCpuExact.ScalarVsNeon",
            "decision": "Deferred",
            "reason": "microbenchmark_only_whole_plan_terminal_pending",
            "host": host,
            "build": {"commit": "1" * 40, "dirty": False},
            "input_id": "q3-packed-exact-lcg-v1-200003",
            "input_len": 200_003,
            "correctness": {field: True for field in COLLECTOR.CORRECTNESS_FIELDS},
            "timing": {
                "scalar_median_ns": 10,
                "neon_median_ns": 9,
                "interleaved": True,
            },
            "whole_plan_promotion": False,
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "correctness.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            checked = COLLECTOR.validate_correctness(path, "1" * 40, host)
            self.assertEqual(checked["decision"], "Deferred")
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "commit differs"):
                COLLECTOR.validate_correctness(path, "2" * 40, host)

    def test_terminal_decision_accepts_only_five_consistent_queue_wins(self):
        def pairs(deltas: list[float]):
            return [
                {
                    "runs": {
                        "scalar": {"distributions": {"frame_completion_ms": {"mean": 10.0}}},
                        "neon": {"distributions": {"frame_completion_ms": {"mean": 10.0 + delta}}},
                    }
                }
                for delta in deltas
            ]

        self.assertEqual(COLLECTOR.terminal_decision(pairs([-1, -2, -1, -3, -1]))[0], "Accepted")
        self.assertEqual(COLLECTOR.terminal_decision(pairs([-1, -2, 0.1, -3, -1]))[0], "Rejected")

    def test_cpu_terminals_join_every_measured_whole_plan_phase(self):
        validated = COLLECTOR.validate_run_log(
            fixture_log(),
            "",
            lane="scalar",
            dataset={"splat_count": 100},
            trace={
                "trace_id": "truck-trace",
                "content_sha256": "a" * 64,
                "width": 1920,
                "height": 1080,
                "frame_indices": (0, 1),
                "frame_timestamps_ns": (0, 16_666_667),
            },
            warmup=1,
            measured=2,
        )
        self.assertEqual(validated["terminal_ticket_count"], 3)
        self.assertEqual(validated["frames"][0]["preprocess_ms"], 1.5)
        self.assertEqual(validated["frames"][0]["sort_ms"], 2.5)
        self.assertTrue(validated["frames"][0]["order_uploaded"])
        self.assertEqual(validated["frames"][0]["drawn"], 80)
        self.assertEqual(validated["frames"][0]["frame_completion_ms"], 6.0)

    def test_missing_cpu_terminal_is_deferred_protocol_input(self):
        stdout = "\n".join(
            line
            for line in fixture_log().splitlines()
            if not (line.startswith("SURFACE_CPU_MEASUREMENT ") and "ticket=3 " in line)
        )
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "exactly one terminal"):
            COLLECTOR.validate_run_log(
                stdout,
                "",
                lane="neon",
                dataset={"splat_count": 100},
                trace={
                    "trace_id": "truck-trace",
                    "content_sha256": "a" * 64,
                    "width": 1920,
                    "height": 1080,
                    "frame_indices": (0, 1),
                    "frame_timestamps_ns": (0, 16_666_667),
                },
                warmup=1,
                measured=2,
            )


if __name__ == "__main__":
    unittest.main()
