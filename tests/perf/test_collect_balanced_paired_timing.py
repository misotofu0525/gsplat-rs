from __future__ import annotations

import importlib.util
import hashlib
import json
import pathlib
import sys
import tempfile
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
    def test_matrix_content_hash_is_distinct_from_trace_file_hash(self) -> None:
        content_sha256 = "a" * 64
        with tempfile.TemporaryDirectory() as directory:
            trace_path = pathlib.Path(directory) / "trace.json"
            trace_path.write_text(
                json.dumps({"content_sha256": content_sha256}, indent=2) + "\n",
                encoding="utf-8",
            )
            trace, file_sha256 = COLLECTOR.load_trace_identity(
                trace_path, {"sha256": content_sha256}
            )
            self.assertEqual(trace["content_sha256"], content_sha256)
            self.assertEqual(file_sha256, hashlib.sha256(trace_path.read_bytes()).hexdigest())
            self.assertNotEqual(file_sha256, content_sha256)

            with self.assertRaisesRegex(COLLECTOR.ValidationError, "content SHA-256"):
                COLLECTOR.load_trace_identity(trace_path, {"sha256": "b" * 64})

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

    def test_b2_and_b3_use_separate_private_candidate_builds(self) -> None:
        b2 = COLLECTOR.B1.EXPERIMENTS["b2"]
        b3 = COLLECTOR.B1.EXPERIMENTS["b3"]
        self.assertEqual(b2.changed_receipt, "projected_cache_precision")
        self.assertEqual(
            b2.lanes[1].cargo_feature, "diagnostic-surface-projected-axes16"
        )
        self.assertEqual(b2.lanes[1].projected_cache_profile, "CandidateAxes16")
        self.assertEqual(b3.changed_receipt, "resident_sh")
        self.assertEqual(
            b3.lanes[1].cargo_feature, "diagnostic-resident-sh-mantissa8"
        )
        self.assertEqual(
            b3.lanes[1].resident_sh_codec_profile, "CandidateSigned8BandScale5"
        )

    def test_frozen_timing_schedule_and_matrix_fail_closed_before_collection(self) -> None:
        common = {
            "experiment": "b2",
            "pairs": 3,
            "warmup": COLLECTOR.DEFAULT_WARMUP,
            "measured": COLLECTOR.DEFAULT_MEASURED,
            "refresh_hz": 60.0,
            "matrix": COLLECTOR.MATRIX_PATH,
        }
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "warmup=20"):
            COLLECTOR.collect(
                SimpleNamespace(**{**common, "warmup": 19}), ROOT
            )
        with self.assertRaisesRegex(
            COLLECTOR.ValidationError, "committed full-quality matrix"
        ):
            COLLECTOR.collect(
                SimpleNamespace(**{**common, "matrix": pathlib.Path("alternate.json")}),
                ROOT,
            )

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
        workload = self.workload()
        capture = self.capture()
        line = self.diagnostic_line("b1-20", "candidate")
        receipt = COLLECTOR.parse_diagnostic_capture(line, "", lane=lane, capture=capture, workload=workload)
        self.assertEqual(receipt["depth_precision_profile"], "CandidateStable20")
        bad = line.replace("presentation_sequence=7", "presentation_sequence=8")
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "presentation_sequence"):
            COLLECTOR.parse_diagnostic_capture(bad, "", lane=lane, capture=capture, workload=workload)

    @staticmethod
    def workload() -> SimpleNamespace:
        return SimpleNamespace(
            dataset={"splat_count": 2_541_226, "sh_degree": 3},
            trace={"width": 1920, "height": 1080},
        )

    @staticmethod
    def capture() -> dict[str, str]:
        return {
            "scene_generation": "1", "camera_revision": "2", "viewport_generation": "3",
            "contract_generation": "4", "plan_set_generation": "5", "order_generation": "6",
            "presentation_sequence": "7",
        }

    @staticmethod
    def diagnostic_line(experiment: str, lane_name: str) -> str:
        lane = next(
            lane
            for lane in COLLECTOR.B1.EXPERIMENTS[experiment].lanes
            if lane.name == lane_name
        )
        fields = COLLECTOR.expected_precision_fields(
            lane, BalancedPairedTimingTests.workload()
        )
        fields.update(BalancedPairedTimingTests.capture())
        fields["rgba8_sha256"] = "a" * 64
        return "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT " + " ".join(
            f"{key}={value}" for key, value in fields.items()
        )

    def test_b2_axes16_requires_actual_axis_record_width(self) -> None:
        lane = COLLECTOR.B1.EXPERIMENTS["b2"].lanes[1]
        line = self.diagnostic_line("b2", "candidate")
        receipt = COLLECTOR.parse_diagnostic_capture(
            line, "", lane=lane, capture=self.capture(), workload=self.workload()
        )
        self.assertEqual(receipt["projected_axis_record_bytes"], "8")
        with self.assertRaisesRegex(
            COLLECTOR.ValidationError, "projected_axis_record_bytes"
        ):
            COLLECTOR.parse_diagnostic_capture(
                line.replace("projected_axis_record_bytes=8", "projected_axis_record_bytes=16"),
                "",
                lane=lane,
                capture=self.capture(),
                workload=self.workload(),
            )

    def test_b3_sh8_requires_complete_realized_codec_layout(self) -> None:
        lane = COLLECTOR.B1.EXPERIMENTS["b3"].lanes[1]
        line = self.diagnostic_line("b3", "candidate")
        receipt = COLLECTOR.parse_diagnostic_capture(
            line, "", lane=lane, capture=self.capture(), workload=self.workload()
        )
        self.assertEqual(receipt["resident_sh_mantissa_bits"], "8")
        self.assertEqual(receipt["resident_sh_plane_count"], "3")
        self.assertEqual(receipt["resident_sh_bytes_per_source"], "48")
        for field, wrong in (
            ("resident_sh_mantissa_bits", "11"),
            ("resident_sh_symmetric_max_code", "1023"),
            ("resident_sh_residual_coefficients_per_source", "44"),
            ("resident_sh_plane_count", "4"),
            ("resident_sh_bytes_per_source", "64"),
        ):
            with self.subTest(field=field), self.assertRaisesRegex(
                COLLECTOR.ValidationError, field
            ):
                COLLECTOR.parse_diagnostic_capture(
                    line.replace(f"{field}={receipt[field]}", f"{field}={wrong}"),
                    "",
                    lane=lane,
                    capture=self.capture(),
                    workload=self.workload(),
                )

    def test_b2_and_b3_representation_deltas_are_actual_and_finite(self) -> None:
        source_count = self.workload().dataset["splat_count"]
        for experiment_name in ("b2", "b3"):
            experiment = COLLECTOR.B1.EXPERIMENTS[experiment_name]
            receipts = {}
            for lane in experiment.lanes:
                line = self.diagnostic_line(experiment_name, lane.name)
                receipts[lane.name] = {
                    "diagnostic_capture": COLLECTOR.parse_diagnostic_capture(
                        line,
                        "",
                        lane=lane,
                        capture=self.capture(),
                        workload=self.workload(),
                    )
                }
            summary = COLLECTOR.representation_summary(
                experiment, receipts, source_count
            )
            if experiment_name == "b2":
                self.assertEqual(
                    summary["projected_axis_record_bytes"],
                    {"exact": 16, "candidate": 8, "candidate_minus_exact": -8},
                )
            else:
                self.assertEqual(
                    summary["resident_sh_logical_bytes"],
                    {
                        "source_count": source_count,
                        "exact_per_source": 64,
                        "candidate_per_source": 48,
                        "exact_total": 162_638_464,
                        "candidate_total": 121_978_848,
                        "candidate_minus_exact_total": -40_659_616,
                    },
                )


if __name__ == "__main__":
    unittest.main()
