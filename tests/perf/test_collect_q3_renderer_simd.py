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


def fixture_log(lane: str = "scalar") -> str:
    begin = {
        "trace_id": "truck-trace",
        "trace_sha256": "a" * 64,
        "exact_plan_requested": "cpu_post_sort",
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "blend_mode": "sorted_alpha",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "adapter_backend": "metal",
        "adapter_name": "Apple-M4",
        "adapter_device_type": "integrated_gpu",
        "adapter_driver": "Metal",
        "adapter_driver_info": "available",
        "source_count": 100,
        "decoded_count": 100,
        "encoded_count": 100,
        "resident_count": 100,
        "addressable_count": 100,
        "sh_degree": 3,
        "requested_width": 1920,
        "requested_height": 1080,
        "surface_width": 1920,
        "surface_height": 1080,
        "internal_render_width": 1920,
        "internal_render_height": 1080,
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": True,
        "trace_frames": 3,
    }
    lines = [line("SURFACE_EXACT_EVIDENCE_BEGIN ", begin)]
    frame_values = [
        ("warmup", "none", 0, 0, 1.0, 2.0, 5.0),
        ("measure", 0, 0, 0, 1.5, 2.5, 6.0),
        ("measure", 1, 1, 16_666_667, 2.5, 3.5, 8.0),
    ]
    for index, (phase, sample, trace_frame, timestamp, preprocess, sort, completion) in enumerate(frame_values):
        frame = {
            "trace_id": "truck-trace",
            "trace_sha256": "a" * 64,
            "playback_index": index,
            "phase": phase,
            "measured_sample": sample,
            "trace_frame": trace_frame,
            "trace_timestamp_ns": timestamp,
            "elapsed_ns": (index + 1) * 1_000_000,
            "exact_plan_requested": "cpu_post_sort",
            "exact_plan_actual": "cpu_post_sort",
            "current_stats_ticket": index + 1,
            "scene_generation": 1,
            "camera_revision": index,
            "viewport_generation": 0,
            "contract_generation": 1,
            "plan_set_generation": 1,
            "order_generation": index + 1,
            "raster_generation": 1,
            "encode_attempt": index + 1,
            "presentation_sequence": index + 1,
            "count_semantics": "direct_draw_equals_visible",
            "source_count": 100,
            "visible_count": 80,
            "contributor_count": 70,
            "drawn_count": 80,
            "exact_contributor_compaction": False,
            "sort_refreshed": True,
            "order_uploaded": True,
            "actual_backend": "cpu",
            "cpu_preprocess_ms": preprocess,
            "cpu_sort_ms": sort,
            "cpu_render_submit_ms": preprocess + sort + 1.0,
            "call_ms": completion + 0.5,
            "frame_wall_ms": completion + 1.0,
            "requested_width": 1920,
            "requested_height": 1080,
            "presented_width": 1920,
            "presented_height": 1080,
            "frame_presented": True,
            "terminal_receipt": "ready",
        }
        terminal = {
            "status": "ready",
            "ticket_namespace": "current_stats",
            "ticket": index + 1,
            "executed_plan": "cpu_post_sort",
            "scene_generation": 1,
            "camera_revision": index,
            "viewport_generation": 0,
            "contract_generation": 1,
            "plan_set_generation": 1,
            "order_generation": index + 1,
            "raster_generation": 1,
            "encode_attempt": index + 1,
            "presentation_sequence": index + 1,
            "qualification_cpu_kernel": lane,
            "count_semantics": "direct_draw_equals_visible",
            "source_count": 100,
            "visible_count": 80,
            "contributor_count": 70,
            "drawn_count": 80,
            "cpu_preprocess_ms": preprocess,
            "cpu_sort_ms": sort,
            "queue_completion_ms": completion,
        }
        lines.append(line("SURFACE_CURRENT_STATS_TERMINAL ", terminal))
        lines.append(line("SURFACE_EXACT_EVIDENCE_FRAME ", frame))
    capture = {
        "status": "ok",
        "path": "final-frame.png",
        "trace_frame": 1,
        "exact_plan_requested": "cpu_post_sort",
        "exact_plan_actual": "cpu_post_sort",
        "current_stats_ticket": 4,
        "scene_generation": 1,
        "camera_revision": 3,
        "viewport_generation": 0,
        "contract_generation": 1,
        "plan_set_generation": 1,
        "order_generation": 4,
        "raster_generation": 1,
        "encode_attempt": 4,
        "presentation_sequence": 4,
        "qualification_cpu_kernel": "scalar",
        "count_semantics": "direct_draw_equals_visible",
        "source_count": 100,
        "visible_count": 80,
        "contributor_count": 70,
        "drawn_count": 80,
        "exact_contributor_compaction": False,
        "actual_backend": "cpu",
        "requested_width": 1920,
        "requested_height": 1080,
        "captured_width": 1920,
        "captured_height": 1080,
        "frame_presented": True,
        "terminal_receipt": "ready",
    }
    capture_terminal = {
        "status": "ready",
        "ticket_namespace": "current_stats",
        "ticket": 4,
        "executed_plan": "cpu_post_sort",
        "scene_generation": 1,
        "camera_revision": 3,
        "viewport_generation": 0,
        "contract_generation": 1,
        "plan_set_generation": 1,
        "order_generation": 4,
        "raster_generation": 1,
        "encode_attempt": 4,
        "presentation_sequence": 4,
        "qualification_cpu_kernel": lane,
        "count_semantics": "direct_draw_equals_visible",
        "source_count": 100,
        "visible_count": 80,
        "contributor_count": 70,
        "drawn_count": 80,
        "cpu_preprocess_ms": 2.5,
        "cpu_sort_ms": 3.5,
        "queue_completion_ms": 8.0,
    }
    lines.append(line("SURFACE_CURRENT_STATS_TERMINAL ", capture_terminal))
    lines.append(line("SURFACE_EXACT_EVIDENCE_CAPTURE ", capture))
    summary = {
        "status": "ok",
        "exact_plan_requested": "cpu_post_sort",
        "actual_plan_set": "cpu_post_sort",
        "trace_frames": 3,
        "measured_frames": 2,
        "eligibility_retries": 0,
        "capture_retries": 0,
        "terminal_receipts": 4,
        "final_capture": "available",
    }
    lines.append(line("SURFACE_EXACT_EVIDENCE_SUMMARY ", summary))
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

    def test_current_stats_terminals_join_identity_counts_and_queue_completion(self):
        validated = COLLECTOR.validate_run_log(
            fixture_log(),
            "",
            lane="scalar",
            dataset={"splat_count": 100, "sh_degree": 3},
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
            capture_path=Path("/tmp/final-frame.png"),
        )
        self.assertEqual(validated["terminal_ticket_count"], 4)
        self.assertEqual(validated["capture_ticket"], 4)
        self.assertEqual(validated["frames"][0]["preprocess_ms"], 1.5)
        self.assertEqual(validated["frames"][0]["sort_ms"], 2.5)
        self.assertTrue(validated["frames"][0]["order_uploaded"])
        self.assertEqual(validated["frames"][0]["drawn"], 80)
        self.assertEqual(validated["frames"][0]["frame_completion_ms"], 6.0)

    def test_missing_current_stats_terminal_is_owner_protocol_incomplete(self):
        stdout = "\n".join(
            line
            for line in fixture_log("neon").splitlines()
            if not (
                line.startswith("SURFACE_CURRENT_STATS_TERMINAL ") and "ticket=3 " in line
            )
        )
        with self.assertRaisesRegex(
            COLLECTOR.OwnerProtocolIncompleteError, "lack canonical terminal"
        ):
            COLLECTOR.validate_run_log(
                stdout,
                "",
                lane="neon",
                dataset={"splat_count": 100, "sh_degree": 3},
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

    def test_current_stats_identity_mismatch_is_integrity_rejected(self):
        stdout = fixture_log().replace(
            "ticket_namespace=current_stats ticket=2 executed_plan=cpu_post_sort scene_generation=1 camera_revision=1",
            "ticket_namespace=current_stats ticket=2 executed_plan=cpu_post_sort scene_generation=1 camera_revision=99",
        )
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "camera_revision"):
            COLLECTOR.validate_run_log(
                stdout,
                "",
                lane="scalar",
                dataset={"splat_count": 100, "sh_degree": 3},
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

    def test_old_order_measurement_terminal_cannot_satisfy_current_stats_owner(self):
        stdout = "\n".join(
            line
            for line in fixture_log().splitlines()
            if not line.startswith("SURFACE_CURRENT_STATS_TERMINAL ")
        )
        stdout += "\nSURFACE_CPU_MEASUREMENT ticket=1 camera_revision=0 measured=false\n"
        with self.assertRaises(COLLECTOR.OwnerProtocolIncompleteError):
            COLLECTOR.validate_run_log(
                stdout,
                "",
                lane="neon",
                dataset={"splat_count": 100, "sh_degree": 3},
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

    def test_failure_classes_map_to_finite_cells(self):
        self.assertEqual(
            COLLECTOR.terminal_for_error(COLLECTOR.OwnerProtocolIncompleteError("missing")),
            ("Deferred", "owner_protocol_incomplete"),
        )
        self.assertEqual(
            COLLECTOR.terminal_for_error(COLLECTOR.EnvironmentPrerequisiteError("host")),
            ("Deferred", "environment_prerequisite"),
        )
        self.assertEqual(
            COLLECTOR.terminal_for_error(COLLECTOR.IntegrityRejectedError("mismatch")),
            ("Rejected", "integrity_rejected"),
        )

    def test_non_ready_terminal_plus_nonzero_exit_is_rejected(self):
        stdout = fixture_log().replace(
            "SURFACE_CURRENT_STATS_TERMINAL status=ready",
            "SURFACE_CURRENT_STATS_TERMINAL status=map_failure",
            1,
        )
        error = COLLECTOR.nonzero_run_error(1, stdout, "", "pair 1 scalar")
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
        self.assertIn("non-ready current-stats terminal", str(error))
        self.assertEqual(
            COLLECTOR.terminal_for_error(error),
            ("Rejected", "integrity_rejected"),
        )

    def test_ready_evidence_plus_nonzero_exit_is_rejected(self):
        error = COLLECTOR.nonzero_run_error(
            1, fixture_log(), "host validation failed", "pair 2 neon"
        )
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
        self.assertIn("after ready current-stats evidence", str(error))
        self.assertEqual(
            COLLECTOR.terminal_for_error(error),
            ("Rejected", "integrity_rejected"),
        )

    def test_nonzero_exit_without_evidence_is_environment_deferred(self):
        error = COLLECTOR.nonzero_run_error(
            1, "", "surface creation unavailable", "pair 3 scalar"
        )
        self.assertIsInstance(error, COLLECTOR.EnvironmentPrerequisiteError)
        self.assertIn("before entering the evidence protocol", str(error))
        self.assertEqual(
            COLLECTOR.terminal_for_error(error),
            ("Deferred", "environment_prerequisite"),
        )

    def test_host_protocol_marker_without_terminal_is_still_rejected(self):
        error = COLLECTOR.nonzero_run_error(
            1,
            "SURFACE_EXACT_EVIDENCE_BEGIN trace_id=truck\n",
            "host join failed",
            "pair 4 neon",
        )
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
        self.assertIn("after entering the evidence protocol", str(error))

    def test_command_uses_strict_evidence_and_no_order_measurement_ticket(self):
        workload = COLLECTOR.Workload(
            {"splat_count": 100}, Path("truck.ply"), {}, Path("trace.json")
        )
        command = COLLECTOR.make_command(Path("desktop-example"), workload, 1, 2)
        self.assertIn("--surface-evidence-plan", command)
        self.assertIn("cpu-post-sort", command)
        self.assertNotIn("--order-backend", command)


if __name__ == "__main__":
    unittest.main()
