from __future__ import annotations

import argparse
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("collect-q1-product-quality-view000001.py")
SPEC = importlib.util.spec_from_file_location("collect_q1_product_quality_view000001", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


class Q1OneShotCoordinatorTests(unittest.TestCase):
    def fixture(self, root: pathlib.Path) -> tuple[argparse.Namespace, dict[str, object]]:
        formal = root / "formal"
        evaluation = root / "evaluation"
        truck = root / "truck.ply"
        formal.mkdir()
        evaluation.mkdir()
        truck.write_bytes(b"truck")
        output = root / "published"
        args = argparse.Namespace(
            expected_commit="a" * 40,
            formal_trace_authority=formal,
            evaluation_authority=evaluation,
            dataset=truck,
            output=output,
        )
        inputs = {
            "formal": formal,
            "evaluation": evaluation,
            "truck": truck,
            "output": output,
            "protocol_sha256": "b" * 64,
            "trace_content_sha256": "c" * 64,
            "trace_receipt_sha256": "d" * 64,
            "evaluation_receipt_sha256": "e" * 64,
        }
        return args, inputs

    @staticmethod
    def successful_invoke(calls: list[tuple[str, ...]]):
        def invoke(argv, cwd, env):
            calls.append(tuple(argv))
            if "collect-q1-product-quality-native.py" in " ".join(argv):
                output = pathlib.Path(argv[argv.index("--output") + 1])
                output.mkdir()
                (output / "manifest.json").write_text("{}\n", encoding="utf-8")
            elif argv[:3] == ("npm", "run", "quality:truck-view000001"):
                output = pathlib.Path(env["PLAYCANVAS_ARTIFACT_DIR"])
                output.mkdir()
                (output / "manifest.json").write_text("{}\n", encoding="utf-8")
            else:
                output = pathlib.Path(argv[argv.index("--output") + 1])
                output.mkdir()
                (output / "result.json").write_text(
                    json.dumps({"status": "Accepted"}) + "\n", encoding="utf-8"
                )
            return subprocess.CompletedProcess(argv, 0, "", "")

        return invoke

    def test_orders_each_step_once_and_atomically_publishes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            args, inputs = self.fixture(root)
            calls: list[tuple[str, ...]] = []
            with (
                mock.patch.object(COLLECTOR, "require_clean_exact"),
                mock.patch.object(COLLECTOR.Q1, "validate_result"),
            ):
                result = COLLECTOR.collect(
                    args,
                    invoke=self.successful_invoke(calls),
                    preflight=lambda _: inputs,
                )
            self.assertEqual(result, args.output)
            self.assertEqual(len(calls), 3)
            self.assertIn("collect-q1-product-quality-native.py", " ".join(calls[0]))
            self.assertEqual(calls[1][:3], ("npm", "run", "quality:truck-view000001"))
            self.assertIn("validate-q1-product-quality-smoke.py", " ".join(calls[2]))
            self.assertEqual(calls[2][0], sys.executable)
            self.assertEqual(calls[2][1], str(COLLECTOR.OFFLINE_GATE))
            self.assertEqual(calls[2].count(str(COLLECTOR.OFFLINE_GATE)), 1)
            self.assertEqual(
                calls[2][2::2],
                (
                    "--formal-trace-authority",
                    "--evaluation-authority",
                    "--gsplat-capture",
                    "--playcanvas-capture",
                    "--output",
                ),
            )
            receipt = json.loads((result / "receipt.json").read_text(encoding="utf-8"))
            self.assertEqual(receipt["status"], "complete")
            self.assertEqual(receipt["product_quality"], "Deferred")
            self.assertFalse(receipt["performance_authorized"])
            self.assertTrue(all(not step["automatic_retry"] for step in receipt["steps"]))

    def test_first_failure_stops_without_retry_and_preserves_diagnostic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            args, inputs = self.fixture(root)
            calls = []

            def invoke(argv, cwd, env):
                calls.append(tuple(argv))
                return subprocess.CompletedProcess(argv, 23, "", "failed")

            with self.assertRaisesRegex(COLLECTOR.CollectionError, "native_quality_only exited"):
                COLLECTOR.collect(args, invoke=invoke, preflight=lambda _: inputs)
            self.assertEqual(len(calls), 1)
            self.assertFalse(args.output.exists())
            failures = list(root.glob("published.failed-*"))
            self.assertEqual(len(failures), 1)
            blocker = json.loads((failures[0] / "blocker.json").read_text(encoding="utf-8"))
            self.assertEqual(blocker["failed_step"], "native_quality_only")
            self.assertEqual(blocker["status"], "failed_attempt")
            self.assertEqual(blocker["product_quality"], "Deferred")
            self.assertFalse(blocker["automatic_retry"])

    def test_second_failure_never_runs_offline_gate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            args, inputs = self.fixture(root)
            calls = []

            def invoke(argv, cwd, env):
                calls.append(tuple(argv))
                if len(calls) == 1:
                    output = pathlib.Path(argv[argv.index("--output") + 1])
                    output.mkdir()
                    immutable = output / "manifest.json"
                    immutable.write_text("{}\n", encoding="utf-8")
                    os.chmod(immutable, 0o444)
                    os.chmod(output, 0o555)
                    return subprocess.CompletedProcess(argv, 0, "", "")
                return subprocess.CompletedProcess(argv, 17, "", "")

            with (
                mock.patch.object(COLLECTOR, "require_clean_exact"),
                self.assertRaisesRegex(COLLECTOR.CollectionError, "playcanvas_quality_only exited"),
            ):
                COLLECTOR.collect(args, invoke=invoke, preflight=lambda _: inputs)
            self.assertEqual(len(calls), 2)
            self.assertFalse(args.output.exists())
            failures = list(root.glob("published.failed-*"))
            self.assertEqual(len(failures), 1)
            self.assertTrue((failures[0] / "native-view000001/manifest.json").is_file())

    def test_offline_gate_failure_follows_two_producers_without_retry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            args, inputs = self.fixture(root)
            calls = []

            def invoke(argv, cwd, env):
                calls.append(tuple(argv))
                if len(calls) == 1:
                    output = pathlib.Path(argv[argv.index("--output") + 1])
                    output.mkdir()
                elif len(calls) == 2:
                    pathlib.Path(env["PLAYCANVAS_ARTIFACT_DIR"]).mkdir()
                return subprocess.CompletedProcess(
                    argv, 31 if len(calls) == 3 else 0, "", ""
                )

            with (
                mock.patch.object(COLLECTOR, "require_clean_exact"),
                self.assertRaisesRegex(COLLECTOR.CollectionError, "offline_quality_gate exited"),
            ):
                COLLECTOR.collect(args, invoke=invoke, preflight=lambda _: inputs)
            self.assertEqual(len(calls), 3)
            self.assertFalse(args.output.exists())

    def test_exception_is_one_terminal_attempt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            args, inputs = self.fixture(root)
            calls = []

            def invoke(argv, cwd, env):
                calls.append(tuple(argv))
                raise RuntimeError("launch failed")

            with self.assertRaisesRegex(COLLECTOR.CollectionError, "raised before completion"):
                COLLECTOR.collect(args, invoke=invoke, preflight=lambda _: inputs)
            self.assertEqual(len(calls), 1)
            self.assertFalse(args.output.exists())

    def test_preflight_rejects_existing_or_overlapping_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            authority = root / "authority"
            authority.mkdir()
            occupied = root / "occupied"
            occupied.mkdir()
            with self.assertRaisesRegex(COLLECTOR.CollectionError, "already exists"):
                COLLECTOR.require_disjoint_output(occupied, (authority,))
            with self.assertRaisesRegex(COLLECTOR.CollectionError, "overlaps"):
                COLLECTOR.require_disjoint_output(authority / "child", (authority,))

    def test_playcanvas_invocation_is_frozen_quality_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            args, inputs = self.fixture(root)
            environments = []
            requests = []
            calls = []
            invoke = self.successful_invoke(calls)

            def observing(argv, cwd, env):
                environments.append(dict(env))
                request_path = env.get("PLAYCANVAS_Q1_PRODUCER_REQUEST")
                if request_path:
                    requests.append(
                        json.loads(pathlib.Path(request_path).read_text(encoding="utf-8"))
                    )
                return invoke(argv, cwd, env)

            with (
                mock.patch.object(COLLECTOR, "require_clean_exact"),
                mock.patch.object(COLLECTOR.Q1, "validate_result"),
            ):
                COLLECTOR.collect(args, invoke=observing, preflight=lambda _: inputs)
            playcanvas = environments[1]
            self.assertEqual(playcanvas["HEADLESS"], "0")
            self.assertEqual(len(requests), 1)
            request = requests[0]
            self.assertEqual(request["formal_view_id"], "000001")
            self.assertEqual(request["trace_frame_index"], 0)
            self.assertFalse(request["performance_authorized"])


if __name__ == "__main__":
    unittest.main()
