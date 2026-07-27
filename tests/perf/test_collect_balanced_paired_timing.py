from __future__ import annotations

import importlib.util
import pathlib
import sys
import unittest
from types import SimpleNamespace


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tests/perf/collect-balanced-paired-timing.py"
SPEC = importlib.util.spec_from_file_location("collect_balanced_paired_timing", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


class BalancedPairedTimingTests(unittest.TestCase):
    def test_schedule_is_deterministic_counterbalanced_and_has_three_pairs(self) -> None:
        schedule = COLLECTOR.schedule_pairs(3, 41)
        self.assertEqual(schedule, COLLECTOR.schedule_pairs(3, 41))
        self.assertEqual(len(schedule), 3)
        self.assertTrue(all(set(order) == {"exact", "candidate"} for order in schedule))
        forward = sum(order == ("exact", "candidate") for order in schedule)
        reverse = sum(order == ("candidate", "exact") for order in schedule)
        self.assertLessEqual(abs(forward - reverse), 1)

    def test_schedule_rejects_fewer_than_three_pairs(self) -> None:
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "at least 3"):
            COLLECTOR.schedule_pairs(2, 1)

    def test_timing_command_keeps_full_quality_and_every_frame_ordering(self) -> None:
        workload = SimpleNamespace(
            dataset_path=pathlib.Path("/tmp/truck.ply"),
            trace_path=pathlib.Path("/tmp/truck-trace.json"),
        )
        command = COLLECTOR.make_command(pathlib.Path("/tmp/desktop"), workload, warmup=20, measured=80)
        self.assertIn("--geometry-path", command)
        self.assertIn("packed", command)
        self.assertIn("--camera-sequence", command)
        self.assertIn("--surface-sort-policy", command)
        self.assertIn("every-frame", command)
        self.assertIn("--surface-diagnostic-capture-receipt", command)
        self.assertNotIn("--surface-diagnostic-multi-capture", command)

    def test_terminal_deltas_have_only_three_explicit_outcomes(self) -> None:
        self.assertEqual(
            COLLECTOR.verdict([
                {"candidate_minus_exact_frame_wall_ms": -0.2},
                {"candidate_minus_exact_frame_wall_ms": -0.1},
                {"candidate_minus_exact_frame_wall_ms": -0.3},
            ]),
            "candidate",
        )
        self.assertEqual(
            COLLECTOR.verdict([
                {"candidate_minus_exact_frame_wall_ms": 0.2},
                {"candidate_minus_exact_frame_wall_ms": 0.1},
                {"candidate_minus_exact_frame_wall_ms": 0.3},
            ]),
            "exact",
        )
        self.assertEqual(
            COLLECTOR.verdict([
                {"candidate_minus_exact_frame_wall_ms": -0.2},
                {"candidate_minus_exact_frame_wall_ms": 0.1},
                {"candidate_minus_exact_frame_wall_ms": -0.3},
            ]),
            "inconclusive",
        )

    def test_diagnostic_capture_profile_and_final_identity_must_join(self) -> None:
        lane = COLLECTOR.B1.EXPERIMENTS["b1-20"].lanes[1]
        workload = SimpleNamespace(
            dataset={"splat_count": 2_541_226, "sh_degree": 3},
            trace={"width": 1920, "height": 1080},
        )
        capture = {
            "scene_generation": "1", "camera_revision": "2", "viewport_generation": "3",
            "contract_generation": "4", "plan_set_generation": "5", "order_generation": "6",
            "presentation_sequence": "7",
        }
        line = (
            "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT "
            "depth_precision_profile=CandidateStable20 "
            "projected_cache_precision_profile=ExactAxes32 "
            "resident_sh_codec_profile=ExactSigned11BandScale5 "
            "resident_sh_source_count=2541226 resident_sh_encoded_count=2541226 "
            "resident_sh_resident_count=2541226 resident_sh_addressable_count=2541226 "
            "resident_sh_source_degree=3 resident_sh_resident_degree=3 "
            "plan_id=GpuPostSort width=1920 height=1080 "
            "scene_generation=1 camera_revision=2 viewport_generation=3 "
            "contract_generation=4 plan_set_generation=5 order_generation=6 "
            "presentation_sequence=7 rgba8_sha256=" + "a" * 64
        )
        receipt = COLLECTOR.parse_diagnostic_capture(line, "", lane=lane, capture=capture, workload=workload)
        self.assertEqual(receipt["depth_precision_profile"], "CandidateStable20")
        bad = line.replace("presentation_sequence=7", "presentation_sequence=8")
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "presentation_sequence"):
            COLLECTOR.parse_diagnostic_capture(bad, "", lane=lane, capture=capture, workload=workload)


if __name__ == "__main__":
    unittest.main()
