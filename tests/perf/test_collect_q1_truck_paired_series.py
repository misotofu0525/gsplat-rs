from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tests/perf/collect-q1-truck-paired-series.py"
SPEC = importlib.util.spec_from_file_location("collect_q1_truck_paired_series", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


class Q1TruckPairedSeriesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.reference = {}
        for trace in (0, 1):
            path = self.root / f"reference-source-{trace}.png"
            path.write_bytes(f"reference-{trace}".encode())
            self.reference[trace] = path
        self.series = self.root / "series"
        self.args = SimpleNamespace(
            series_root=self.series,
            series_id="q1-truck-unit-series",
            collection_session_id="q1-unit-session",
            seed=20260728,
            chrome=self.root / "Chrome",
            gsplat_wasm_package=self.root / "quality-exact",
            reference_images=self.reference,
            reviewed_sha="a" * 40,
            gsplat_port_base=43000,
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def plan(self):
        return COLLECTOR.build_plan(
            self.args, predeclared_at="2026-07-28T00:00:00Z"
        )

    def test_seeded_schedule_is_deterministic_exactly_five_and_counterbalanced(self) -> None:
        first = COLLECTOR.schedule_orders(20260728)
        self.assertEqual(first, COLLECTOR.schedule_orders(20260728))
        self.assertEqual(len(first), 5)
        self.assertEqual(
            set(first), {"playcanvas-first", "gsplat-rs-first"}
        )
        self.assertLessEqual(
            abs(first.count("playcanvas-first") - first.count("gsplat-rs-first")),
            1,
        )

    def test_plan_predeclares_all_thirty_fresh_commands_in_pair_order(self) -> None:
        plan = self.plan()
        invocations = plan["invocations"]
        self.assertEqual(len(invocations), 30)
        self.assertEqual([item["sequence"] for item in invocations], list(range(1, 31)))
        self.assertEqual(len({item["artifact"] for item in invocations}), 30)
        self.assertTrue(all(item["automatic_retry"] is False for item in invocations))
        for declaration in plan["schedule"]["pairs"]:
            pair = [
                item for item in invocations if item["pair_id"] == declaration["pair_id"]
            ]
            expected = (
                ["playcanvas"] * 3 + ["gsplat_rs"] * 3
                if declaration["run_order"] == "playcanvas-first"
                else ["gsplat_rs"] * 3 + ["playcanvas"] * 3
            )
            self.assertEqual([item["endpoint"] for item in pair], expected)
            for offset in (0, 3):
                self.assertEqual(
                    [(item["artifact_role"], item["trace_frame_index"]) for item in pair[offset:offset + 3]],
                    [("control", 0), ("control", 1), ("throughput", None)],
                )
        receipt = COLLECTOR.command_receipt(plan)
        self.assertEqual(receipt["invocation_count"], 30)
        self.assertEqual(len(receipt["invocations"]), 30)
        self.assertTrue(all("request_value" in item for item in receipt["invocations"]))

    def test_dry_run_and_print_only_have_no_filesystem_side_effects(self) -> None:
        common = [
            "--series-root", str(self.series),
            "--series-id", self.args.series_id,
            "--collection-session-id", self.args.collection_session_id,
            "--seed", str(self.args.seed),
            "--chrome", str(self.args.chrome),
            "--gsplat-wasm-package", str(self.args.gsplat_wasm_package),
            "--reference-trace-0", str(self.reference[0]),
            "--reference-trace-1", str(self.reference[1]),
        ]
        for mode in ("--dry-run", "--print-only"):
            with self.subTest(mode=mode), contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(COLLECTOR.main([mode, *common]), 0)
                parsed = json.loads(output.getvalue())
                self.assertFalse(parsed["side_effects"])
                self.assertEqual(len(parsed["commands"]["invocations"]), 30)
            self.assertFalse(self.series.exists())

    def test_claim_materializes_declaration_commands_and_requests_before_run(self) -> None:
        plan = self.plan()
        COLLECTOR.claim_series(self.args, plan)
        self.assertTrue((self.series / "schedule-declaration.json").is_file())
        commands = json.loads((self.series / "commands.json").read_text())
        declaration = json.loads(
            (self.series / "schedule-declaration.json").read_text()
        )
        self.assertEqual(commands["invocation_count"], 30)
        self.assertEqual(
            declaration["command_receipt_sha256"],
            COLLECTOR.canonical_sha256(commands),
        )
        self.assertEqual(len(list((self.series / "requests").glob("*.template.json"))), 5)
        self.assertEqual(len(list((self.series / "run-contexts").glob("*.json"))), 15)
        first_artifact = self.series / plan["invocations"][0]["artifact"]
        self.assertTrue(first_artifact.parent.is_dir())
        self.assertFalse(first_artifact.exists())
        self.assertEqual(
            (self.series / "reference/trace-0.png").read_bytes(),
            self.reference[0].read_bytes(),
        )

    def test_playcanvas_throughput_request_binds_real_control_hashes(self) -> None:
        plan = self.plan()
        COLLECTOR.claim_series(self.args, plan)
        invocation = next(
            item for item in plan["invocations"]
            if item["endpoint"] == "playcanvas" and item["artifact_role"] == "throughput"
        )
        for trace in (0, 1):
            directory = (
                self.series / "pairs" / invocation["pair_id"] / "playcanvas"
                / f"control-trace-{trace}"
            )
            directory.mkdir(parents=True)
            (directory / "manifest.json").write_text(json.dumps({
                "run_id": f"pc-control-{trace}",
                "q1_comparison": {
                    "artifact_role": "control",
                    "capture_trace_frame_index": trace,
                    "configuration_sha256": plan["configuration_sha256"],
                },
            }))
        COLLECTOR.resolve_throughput(invocation, self.series)
        request = json.loads((self.series / invocation["request"]).read_text())
        self.assertEqual(
            [binding["trace_frame_index"] for binding in request["control_bindings"]],
            [0, 1],
        )
        self.assertTrue(all(len(binding["manifest_sha256"]) == 64 for binding in request["control_bindings"]))
        self.assertTrue(
            (self.series / "requests" / f"{invocation['invocation_id']}.bindings.json").is_file()
        )

    def test_execute_stops_after_first_failure_and_publishes_one_blocker(self) -> None:
        plan = self.plan()
        calls = []

        def fail_first(invocation, root):
            calls.append(invocation["invocation_id"])
            raise COLLECTOR.OrchestrationError("synthetic first command failure")

        with mock.patch.object(COLLECTOR, "run_once", side_effect=fail_first):
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "synthetic"):
                COLLECTOR.execute(self.args, plan)
        self.assertEqual(len(calls), 1)
        blocker = json.loads((self.series / "blocker.json").read_text())
        self.assertFalse(blocker["automatic_retry"])
        self.assertFalse(blocker["retry_authorized"])

    def test_run_once_never_retries_a_failed_producer(self) -> None:
        invocation = self.plan()["invocations"][0]
        (self.root / "logs").mkdir()
        response = SimpleNamespace(returncode=9, stdout="", stderr="failed once")
        with mock.patch.object(COLLECTOR.subprocess, "run", return_value=response) as run:
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "exited 9"):
                COLLECTOR.run_once(invocation, self.root)
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
