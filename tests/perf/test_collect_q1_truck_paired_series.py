from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import unittest
import struct
import zlib
from types import SimpleNamespace
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tests/perf/collect-q1-truck-paired-series.py"
SPEC = importlib.util.spec_from_file_location("collect_q1_truck_paired_series", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)
import q1_pair_admission.artifacts as ADMISSION_ARTIFACTS  # noqa: E402
from q1_pair_admission.contract import (  # noqa: E402
    TRACE_FRAME_POSE_INTRINSICS_SHA256,
    validate_orchestration,
)


def rgba_png_bytes(value: int) -> bytes:
    pixel = bytes((value, value, value, 255))
    filtered = (b"\0" + pixel * COLLECTOR.WIDTH) * COLLECTOR.HEIGHT

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", COLLECTOR.WIDTH, COLLECTOR.HEIGHT, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(filtered, 9))
        + chunk(b"IEND", b"")
    )


class Q1TruckPairedSeriesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.authority_root = self.root / "reference-authority-source"
        self.authority_root.mkdir()
        (self.authority_root / "producer").mkdir()
        (self.authority_root / "producer/desktop-example").write_bytes(
            b"retained-direct-reference-binary"
        )
        (self.authority_root / "reference.json").write_text(
            json.dumps({"schema": "unit-authority"})
        )
        (self.authority_root / "build.stdout.log").write_text("build ok\n")
        (self.authority_root / "build.stderr.log").write_text("")
        self.reference = {}
        for trace in (0, 1):
            path = self.authority_root / f"reference-trace-{trace}.png"
            path.write_bytes(rgba_png_bytes(trace))
            (self.authority_root / f"trace-{trace}.stdout.log").write_text(
                f"trace {trace} ok\n"
            )
            (self.authority_root / f"trace-{trace}.stderr.log").write_text("")
            self.reference[trace] = path
        self.series = self.root / "series"
        self.args = SimpleNamespace(
            series_root=self.series,
            series_id="q1-truck-unit-series",
            collection_session_id="q1-unit-session",
            seed=20260728,
            chrome=self.root / "Chrome",
            gsplat_wasm_package=self.root / "quality-exact",
            reference_authority=self.authority_root,
            reviewed_sha="a" * 40,
            gsplat_port_base=43000,
            predeclared_at_utc="2026-07-28T00:00:00Z",
        )
        self.args.reference_authority_admission = self.admit_authority(
            self.authority_root
        )
        self.args.formal_inputs = {
            "reference_authority": COLLECTOR.authority_content_identity(
                self.args.reference_authority_admission,
                root_path=str(self.authority_root),
            ),
            "references": COLLECTOR.formal_reference_receipts(
                self.args.reference_authority_admission
            ),
        }
        self.authority_patch = mock.patch.object(
            COLLECTOR,
            "admit_reference_authority",
            side_effect=self.admit_authority,
        )
        self.authority_patch.start()
        self.contract_authority_patch = mock.patch.object(
            ADMISSION_ARTIFACTS,
            "reference_authority",
            side_effect=self.admit_authority,
        )
        self.contract_authority_patch.start()

    def tearDown(self) -> None:
        self.contract_authority_patch.stop()
        self.authority_patch.stop()
        self.temp.cleanup()

    def admit_authority(self, root: pathlib.Path) -> dict[str, object]:
        root = pathlib.Path(root)
        if (root / "blocker.json").exists():
            raise ValueError("authority contains blocker.json")
        files = []
        for path in sorted(value for value in root.rglob("*") if value.is_file()):
            relative = path.relative_to(root).as_posix()
            data = path.read_bytes()
            files.append(
                {
                    "path": relative,
                    "bytes": len(data),
                    "sha256": hashlib.sha256(data).hexdigest(),
                }
            )
        views = {}
        for trace in (0, 1):
            path = root / f"reference-trace-{trace}.png"
            pixel = bytes((trace, trace, trace, 255))
            views[trace] = {
                "path": path,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "decoded_rgba8_sha256": hashlib.sha256(
                    pixel * COLLECTOR.WIDTH * COLLECTOR.HEIGHT
                ).hexdigest(),
                "pose_intrinsics_sha256": TRACE_FRAME_POSE_INTRINSICS_SHA256[trace],
            }
        retained = root / "producer/desktop-example"
        return {
            "authority_root": root,
            "receipt_path": root / "reference.json",
            "receipt_sha256": hashlib.sha256(
                (root / "reference.json").read_bytes()
            ).hexdigest(),
            "repository_commit": self.args.reviewed_sha
            if hasattr(self, "args")
            else "a" * 40,
            "release_binary_sha256": hashlib.sha256(retained.read_bytes()).hexdigest(),
            "generated_at_utc": "2026-01-01T00:00:00Z",
            "tree": {
                "file_count": len(files),
                "bytes": sum(value["bytes"] for value in files),
                "sha256": COLLECTOR.canonical_sha256(files),
                "files": files,
            },
            "views": views,
        }

    def plan(self):
        return COLLECTOR.build_plan(
            self.args, predeclared_at="2026-07-28T00:00:00Z"
        )

    def ownership(self, name: str) -> dict[str, str]:
        value = COLLECTOR.browser_ownership(self.root, name, self.args.chrome)
        return {
            key: value[key]
            for key in (
                "marker",
                "marker_argument",
                "user_data_dir",
                "handshake_path",
                "expected_executable",
            )
        }

    def ownership_environment(self, ownership: dict[str, str]) -> dict[str, str]:
        return {
            **COLLECTOR.safe_host_environment(),
            "GSPLAT_Q1_BROWSER_OWNER_MARKER": ownership["marker"],
            "GSPLAT_Q1_BROWSER_USER_DATA_DIR": ownership["user_data_dir"],
            "GSPLAT_Q1_BROWSER_HANDSHAKE_PATH": ownership["handshake_path"],
        }

    def formal_inputs(self) -> dict[str, object]:
        required = set(COLLECTOR.LOCKED_REPOSITORY_FILES)
        return {
            "schema": COLLECTOR.FORMAL_INPUTS_SCHEMA,
            "reviewed_commit": self.args.reviewed_sha,
            "git": {"head": self.args.reviewed_sha, "clean": True},
            "browser": {"path": str(self.args.chrome.resolve()), "bytes": 1, "sha256": "1" * 64},
            "toolchains": [
                {"name": name, "path": f"/usr/bin/{name}", "bytes": 1, "sha256": "7" * 64}
                for name in ("node", "python")
            ],
            "wasm_package": {
                "path": "target/quality-exact",
                "files": [
                    {"name": name, "path": f"target/quality-exact/{name}", "bytes": 1, "sha256": str(index) * 64}
                    for index, name in enumerate(
                        ("gsplat_web.js", "gsplat_web_bg.wasm", "gsplat_web_build_receipt.json"),
                        2,
                    )
                ],
            },
            "repository_files": [
                {"path": path, "bytes": 1, "sha256": "5" * 64}
                for path in sorted(required)
            ],
            "repository_trees": [
                {"path": path, "file_count": 1, "bytes": 1, "sha256": "6" * 64}
                for path in COLLECTOR.LOCKED_REPOSITORY_TREES
            ],
            "puppeteer_production_modules": {
                "schema": COLLECTOR.PUPPETEER_GRAPH_SCHEMA,
                "package_lock_sha256": "5" * 64,
                "root": "node_modules/puppeteer-core",
                "package_count": 1,
                "packages": [{
                    "path": "node_modules/puppeteer-core",
                    "lock_path": "node_modules/puppeteer-core",
                    "version": "24.15.0",
                    "integrity": "sha512-test",
                    "file_count": 1,
                    "bytes": 1,
                    "sha256": "8" * 64,
                    "runtime_dependencies": [],
                }],
                "sha256": COLLECTOR.canonical_sha256([{
                    "path": "node_modules/puppeteer-core",
                    "lock_path": "node_modules/puppeteer-core",
                    "version": "24.15.0",
                    "integrity": "sha512-test",
                    "file_count": 1,
                    "bytes": 1,
                    "sha256": "8" * 64,
                    "runtime_dependencies": [],
                }]),
            },
            "reference_authority": self.args.formal_inputs[
                "reference_authority"
            ],
            "references": self.args.formal_inputs["references"],
        }

    def post_authority(self, locked: dict[str, object]) -> dict[str, object]:
        authority = locked["reference_authority"]
        claimed = authority["claimed_pre"]
        return {
            "source_pre_sha256": authority["source_pre_sha256"],
            "source_post_sha256": authority["source_pre_sha256"],
            "claimed_pre_sha256": authority["claimed_pre_sha256"],
            "claimed_post_sha256": authority["claimed_pre_sha256"],
            "receipt_sha256": claimed["receipt_sha256"],
            "tree_sha256": claimed["tree"]["sha256"],
        }

    def formal_orchestration_bundle(self, name: str):
        self.args.series_root = self.root / name
        self.args.formal_inputs = self.formal_inputs()
        plan = self.plan()
        locked = COLLECTOR.claim_series(self.args, plan)
        post = {
            "schema": COLLECTOR.POST_RUN_SCHEMA,
            "verified_at_utc": "2026-07-28T01:00:00Z",
            "reviewed_commit": self.args.reviewed_sha,
            "git": {"head": self.args.reviewed_sha, "clean": True},
            "formal_inputs_sha256": locked["formal_inputs_sha256"],
            "command_receipt_sha256": locked["command_receipt"]["sha256"],
            "formal_lock_sha256": COLLECTOR.sha256_path(
                self.args.series_root / "formal-execution-lock.json"
            ),
            "reference_authority": self.post_authority(locked),
        }
        document = {
            "schedule": plan["schedule"],
            "orchestration": {
                "formal_execution_lock": locked,
                "post_run_verification": post,
            },
        }
        commands = json.loads(
            (self.args.series_root / "commands.json").read_text()
        )
        return plan, locked, post, document, commands

    def rewrite_formal_orchestration_bundle(
        self, locked, post, commands
    ) -> None:
        root = self.args.series_root
        locked["formal_inputs_sha256"] = COLLECTOR.canonical_sha256(
            locked["formal_inputs"]
        )
        commands["formal_inputs_sha256"] = locked["formal_inputs_sha256"]
        (root / "commands.json").write_bytes(COLLECTOR.json_bytes(commands))
        locked["command_receipt"]["sha256"] = COLLECTOR.sha256_path(
            root / "commands.json"
        )
        post["formal_inputs_sha256"] = locked["formal_inputs_sha256"]
        post["command_receipt_sha256"] = locked["command_receipt"]["sha256"]
        post["reference_authority"] = self.post_authority(locked)
        (root / "formal-execution-lock.json").write_bytes(
            COLLECTOR.json_bytes(locked)
        )
        post["formal_lock_sha256"] = COLLECTOR.sha256_path(
            root / "formal-execution-lock.json"
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
            "--reference-authority", str(self.authority_root),
            "--reviewed-sha", self.args.reviewed_sha,
            "--predeclared-at-utc", self.args.predeclared_at_utc,
        ]
        frozen_plans = []
        frozen_commands = []
        for mode in ("--dry-run", "--print-only"):
            with (
                self.subTest(mode=mode),
                contextlib.redirect_stdout(io.StringIO()) as output,
                mock.patch.object(
                    COLLECTOR,
                    "validate_reference_authority_input",
                    wraps=COLLECTOR.validate_reference_authority_input,
                ) as admitted,
                mock.patch.object(
                    COLLECTOR,
                    "preflight_execute",
                    return_value=self.formal_inputs(),
                ) as preflight,
            ):
                self.assertEqual(COLLECTOR.main([mode, *common]), 0)
                parsed = json.loads(output.getvalue())
                self.assertFalse(parsed["side_effects"])
                self.assertEqual(len(parsed["commands"]["invocations"]), 30)
                self.assertEqual(admitted.call_count, 1)
                self.assertEqual(preflight.call_count, 1)
                self.assertIsNotNone(parsed["plan"]["formal_inputs"])
                references = parsed["plan"]["schedule"]["reference_images"]
                self.assertEqual(
                    {value["authority_receipt_path"] for value in references},
                    {"reference-authority/reference.json"},
                )
                self.assertTrue(
                    all(value["decoded_rgba8_sha256"] for value in references)
                )
                frozen_plans.append(COLLECTOR.json_bytes(parsed["plan"]))
                frozen_commands.append(COLLECTOR.json_bytes(parsed["commands"]))
            self.assertFalse(self.series.exists())
        captured_execute = []
        with (
            mock.patch.object(
                COLLECTOR, "preflight_execute", return_value=self.formal_inputs()
            ),
            mock.patch.object(
                COLLECTOR,
                "execute",
                side_effect=lambda _args, plan: captured_execute.append(plan) or 0,
            ),
        ):
            self.assertEqual(COLLECTOR.main(["--execute", *common]), 0)
        self.assertEqual(len(set(frozen_plans)), 1)
        self.assertEqual(len(set(frozen_commands)), 1)
        self.assertEqual(frozen_plans[0], COLLECTOR.json_bytes(captured_execute[0]))
        self.assertEqual(
            frozen_commands[0],
            COLLECTOR.json_bytes(COLLECTOR.command_receipt(captured_execute[0])),
        )
        self.assertFalse(self.series.exists())

    def test_dry_run_preflight_failure_is_side_effect_free(self) -> None:
        arguments = [
            "--dry-run",
            "--series-root", str(self.series),
            "--series-id", self.args.series_id,
            "--collection-session-id", self.args.collection_session_id,
            "--seed", str(self.args.seed),
            "--chrome", str(self.args.chrome),
            "--gsplat-wasm-package", str(self.args.gsplat_wasm_package),
            "--reference-authority", str(self.authority_root),
            "--reviewed-sha", self.args.reviewed_sha,
            "--predeclared-at-utc", self.args.predeclared_at_utc,
        ]
        with (
            mock.patch.object(
                COLLECTOR,
                "preflight_execute",
                side_effect=COLLECTOR.OrchestrationError(
                    "quality-exact WASM package is unavailable"
                ),
            ),
            contextlib.redirect_stderr(io.StringIO()) as stderr,
        ):
            self.assertEqual(COLLECTOR.main(arguments), 2)
        self.assertIn("quality-exact", stderr.getvalue())
        self.assertFalse(self.series.exists())

    def test_future_predeclared_timestamp_is_rejected_before_admission(self) -> None:
        arguments = [
            "--dry-run",
            "--series-root", str(self.series),
            "--series-id", self.args.series_id,
            "--collection-session-id", self.args.collection_session_id,
            "--seed", str(self.args.seed),
            "--chrome", str(self.args.chrome),
            "--gsplat-wasm-package", str(self.args.gsplat_wasm_package),
            "--reference-authority", str(self.authority_root),
            "--reviewed-sha", self.args.reviewed_sha,
            "--predeclared-at-utc", "2999-01-01T00:00:00Z",
        ]
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(COLLECTOR.main(arguments), 2)
        self.assertIn("must not be in the future", stderr.getvalue())
        self.assertFalse(self.series.exists())

    def test_authority_commit_or_time_mismatch_rejects_before_series_claim(self) -> None:
        common = [
            "--dry-run",
            "--series-root", str(self.series),
            "--series-id", self.args.series_id,
            "--collection-session-id", self.args.collection_session_id,
            "--seed", str(self.args.seed),
            "--chrome", str(self.args.chrome),
            "--gsplat-wasm-package", str(self.args.gsplat_wasm_package),
            "--reference-authority", str(self.authority_root),
            "--reviewed-sha", self.args.reviewed_sha,
            "--predeclared-at-utc", self.args.predeclared_at_utc,
        ]
        for field, value, message in (
            ("repository_commit", "b" * 40, "commit"),
            ("generated_at_utc", "2999-01-01T00:00:00Z", "after schedule"),
        ):
            with self.subTest(field=field):
                admitted = self.admit_authority(self.authority_root)
                admitted[field] = value
                with (
                    mock.patch.object(
                        COLLECTOR,
                        "admit_reference_authority",
                        return_value=admitted,
                    ),
                    contextlib.redirect_stderr(io.StringIO()) as stderr,
                ):
                    self.assertEqual(COLLECTOR.main(common), 2)
                self.assertIn(message, stderr.getvalue())
                self.assertFalse(self.series.exists())

    def test_authority_copy_checks_each_open_source_fd_identity(self) -> None:
        authority = self.admit_authority(self.authority_root)
        drifted_tree = json.loads(json.dumps(authority["tree"]))
        drifted_tree["files"][0]["sha256"] = "f" * 64
        with self.assertRaisesRegex(
            COLLECTOR.OrchestrationError, "content drifted during copy"
        ):
            COLLECTOR.copy_reference_authority(
                self.authority_root,
                self.root / "bad-authority-copy",
                drifted_tree,
            )

    def test_authority_copy_rejects_replaced_intermediate_symlink(self) -> None:
        authority = self.admit_authority(self.authority_root)
        original = self.authority_root / "producer"
        moved = self.root / "original-producer"
        original.rename(moved)
        attacker = self.root / "attacker-producer"
        attacker.mkdir()
        (attacker / "desktop-example").write_bytes(
            (moved / "desktop-example").read_bytes()
        )
        original.symlink_to(attacker, target_is_directory=True)
        with self.assertRaises(OSError):
            COLLECTOR.copy_reference_authority(
                self.authority_root,
                self.root / "symlink-authority-copy",
                authority["tree"],
            )

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
            hashlib.sha256(COLLECTOR.json_bytes(commands)).hexdigest(),
        )
        formal_lock = json.loads((self.series / "formal-execution-lock.json").read_text())
        self.assertEqual(
            formal_lock["command_receipt"]["sha256"],
            hashlib.sha256((self.series / "commands.json").read_bytes()).hexdigest(),
        )
        self.assertEqual(len(list((self.series / "requests").glob("*.template.json"))), 5)
        self.assertEqual(len(list((self.series / "run-contexts").glob("*.json"))), 15)
        first_artifact = self.series / plan["invocations"][0]["artifact"]
        self.assertTrue(first_artifact.parent.is_dir())
        self.assertFalse(first_artifact.exists())
        self.assertEqual(
            (
                self.series
                / "reference-authority/reference-trace-0.png"
            ).read_bytes(),
            self.reference[0].read_bytes(),
        )
        self.assertEqual(
            (
                self.series / "reference-authority/build.stdout.log"
            ).read_bytes(),
            (self.authority_root / "build.stdout.log").read_bytes(),
        )
        self.assertEqual(
            formal_lock["reference_authority"]["claimed_pre"]["tree"]["sha256"],
            plan["reference_authority"]["source"]["tree"]["sha256"],
        )

    def test_playcanvas_throughput_request_binds_real_control_hashes(self) -> None:
        plan = self.plan()
        COLLECTOR.claim_series(self.args, plan)
        invocation = next(
            item for item in plan["invocations"]
            if item["endpoint"] == "playcanvas" and item["artifact_role"] == "throughput"
        )
        admitted = []
        for trace in (0, 1):
            admitted.append({
                "run_id": f"pc-control-{trace}",
                "commit": self.args.reviewed_sha,
                "manifest_sha256": str(trace + 1) * 64,
                "configuration": plan["configuration_sha256"],
                "environment": {
                    "identity": {
                        "collection_session_id": self.args.collection_session_id,
                    },
                },
            })
        with mock.patch.object(
            COLLECTOR, "admit_artifact", side_effect=admitted
        ) as admit:
            COLLECTOR.resolve_throughput(invocation, self.series)
        self.assertEqual(admit.call_count, 2)
        for trace, call in enumerate(admit.call_args_list):
            self.assertEqual(call.kwargs["endpoint"], "playcanvas")
            self.assertEqual(call.kwargs["role"], "control")
            self.assertEqual(call.kwargs["expected_trace"], trace)
            self.assertEqual(call.kwargs["schedule_sha"], plan["schedule_sha256"])
            self.assertEqual(call.kwargs["protocol_sha"], plan["protocol_sha256"])
            self.assertEqual(call.kwargs["position"], invocation["position"])
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
        response = COLLECTOR.ProcessOutcome(
            argv=invocation["argv"],
            returncode=9,
            stdout="",
            stderr="failed once",
            timed_out=False,
            timeout_seconds=invocation["timeout_seconds"],
            cleanup={
                "isolated_process_group": True,
                "lineage_complete": True,
                "group_gone": True,
                "orphan_descendants_detected": False,
            },
        )
        with mock.patch.object(COLLECTOR, "run_process_group", return_value=response) as run:
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "exited 9"):
                COLLECTOR.run_once(invocation, self.root)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.kwargs["env"], invocation["environment"])
        self.assertEqual(
            run.call_args.kwargs["timeout_seconds"],
            COLLECTOR.PROCESS_TIMEOUTS_SECONDS["producer"],
        )

    def test_hung_producer_times_out_once_and_publishes_blocker(self) -> None:
        plan = self.plan()
        counter = self.root / "producer-count.txt"
        pids = self.root / "producer-pids.json"
        script = self.root / "hung-process-tree.py"
        script.write_text(
            "import json, os, pathlib, signal, subprocess, sys, time\n"
            "counter = pathlib.Path(sys.argv[1])\n"
            "pids = pathlib.Path(sys.argv[2])\n"
            "count = int(counter.read_text()) + 1 if counter.exists() else 1\n"
            "counter.write_text(str(count))\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "child = subprocess.Popen([sys.executable, '-c', "
            "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)'], "
            "start_new_session=True)\n"
            "pids.write_text(json.dumps({'leader': os.getpid(), 'grandchild': child.pid}))\n"
            "print('tree-ready', flush=True)\n"
            "time.sleep(300)\n"
        )
        first = plan["invocations"][0]
        first["argv"] = [sys.executable, str(script), str(counter), str(pids)]
        first["timeout_seconds"] = 1
        with mock.patch.dict(
            COLLECTOR.PROCESS_TIMEOUTS_SECONDS,
            {"process_group_term_grace": 1, "process_group_kill_grace": 2},
        ):
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "safety timeout"):
                COLLECTOR.execute(self.args, plan)
        self.assertEqual(counter.read_text(), "1")
        blocker = json.loads((self.series / "blocker.json").read_text())
        self.assertIn("safety timeout", blocker["reason"])
        self.assertFalse(blocker["automatic_retry"])
        self.assertFalse(blocker["retry_authorized"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["isolated_process_group"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["term"]["groups"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["kill"]["groups"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["leader_reaped"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["group_gone"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["detached_process_groups"])
        self.assertIn("tree-ready", blocker["process_timeout"]["stdout_tail"])
        receipt = json.loads(
            (self.series / "logs" / f"{first['invocation_id']}.process.json").read_text()
        )
        self.assertTrue(receipt["timed_out"])
        self.assertTrue(receipt["cleanup"]["group_gone"])
        for pid in json.loads(pids.read_text()).values():
            with self.assertRaises(ProcessLookupError):
                os.kill(pid, 0)

    def test_normal_exit_with_detached_descendant_is_cleaned_and_fails_closed(self) -> None:
        plan = self.plan()
        counter = self.root / "normal-counter.txt"
        child_pid = self.root / "detached-child.pid"
        script = self.root / "normal-detached-tree.py"
        script.write_text(
            "import os, pathlib, signal, subprocess, sys, time\n"
            "counter = pathlib.Path(sys.argv[1])\n"
            "child_pid = pathlib.Path(sys.argv[2])\n"
            "counter.write_text(str((int(counter.read_text()) if counter.exists() else 0) + 1))\n"
            "child = subprocess.Popen([sys.executable, '-c', "
            "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)'], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True)\n"
            "child_pid.write_text(str(child.pid))\n"
            "time.sleep(0.4)\n"
        )
        first = plan["invocations"][0]
        first["argv"] = [sys.executable, str(script), str(counter), str(child_pid)]
        first["timeout_seconds"] = 5
        with mock.patch.dict(
            COLLECTOR.PROCESS_TIMEOUTS_SECONDS,
            {"process_group_term_grace": 1, "process_group_kill_grace": 2},
        ):
            with self.assertRaisesRegex(COLLECTOR.ProcessTreeError, "descendant process tree"):
                COLLECTOR.execute(self.args, plan)
        self.assertEqual(counter.read_text(), "1")
        blocker = json.loads((self.series / "blocker.json").read_text())
        self.assertIsNone(blocker["process_timeout"])
        self.assertFalse(blocker["process_failure"]["timed_out"])
        cleanup = blocker["process_failure"]["cleanup"]
        self.assertTrue(cleanup["orphan_descendants_detected"])
        self.assertTrue(cleanup["detached_process_groups"])
        self.assertTrue(cleanup["kill"]["groups"])
        self.assertTrue(cleanup["group_gone"])
        self.assertFalse(blocker["automatic_retry"])
        self.assertFalse(blocker["retry_authorized"])
        with self.assertRaises(ProcessLookupError):
            os.kill(int(child_pid.read_text()), 0)

    def test_child_environment_ignores_all_undeclared_host_controls(self) -> None:
        injected = {
            "NODE_OPTIONS": "--require=/tmp/injected.js",
            "PYTHONPATH": "/tmp/injected-python",
            "PLAYCANVAS_UNDECLARED": "1",
            "GSPLAT_UNDECLARED": "1",
            "CHROME_EXTRA_FLAGS": "--disable-gpu",
            "NPM_CONFIG_USERCONFIG": "/tmp/injected-npmrc",
        }
        with mock.patch.dict(COLLECTOR.os.environ, injected, clear=False):
            plan = self.plan()
        for invocation in plan["invocations"]:
            for name in injected:
                self.assertNotIn(name, invocation["environment"])
        for name in injected:
            self.assertNotIn(name, plan["postprocess"]["environment"])
        self.assertEqual(
            plan["postprocess"]["environment"]["CHROME_PATH"],
            str(self.args.chrome.resolve()),
        )
        receipt = COLLECTOR.command_receipt(plan)
        self.assertEqual(
            receipt["invocations"][0]["environment"],
            plan["invocations"][0]["environment"],
        )

    def test_immediate_zero_marker_handshake_cannot_escape_cleanup(self) -> None:
        ownership = self.ownership("immediate-zero")
        ownership["expected_executable"] = sys.executable
        script = self.root / "immediate-zero.py"
        script.write_text(
            "import json, os, pathlib, subprocess, sys\n"
            "marker_arg = '--user-data-dir=' + os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR']\n"
            "child = subprocess.Popen([sys.executable, '-c', "
            "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)', marker_arg], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)\n"
            "receipt = {'schema':'gsplat-q1-browser-process-ownership/v1', "
            "'marker':os.environ['GSPLAT_Q1_BROWSER_OWNER_MARKER'], 'marker_arg':marker_arg, "
            "'user_data_dir':os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR'], "
            "'producer_pid':os.getpid(), 'producer_ppid':os.getppid(), "
            "'browser_pid':child.pid, 'browser_spawnfile':sys.executable, "
            "'browser_spawnargs':[sys.executable, marker_arg]}\n"
            "path = pathlib.Path(os.environ['GSPLAT_Q1_BROWSER_HANDSHAKE_PATH'])\n"
            "path.parent.mkdir(parents=True, exist_ok=True)\n"
            "temporary = path.with_suffix('.tmp')\n"
            "temporary.write_text(json.dumps(receipt))\n"
            "temporary.replace(path)\n"
        )
        outcome = COLLECTOR.run_process_group(
            [sys.executable, str(script)], cwd=self.root,
            env=self.ownership_environment(ownership), timeout_seconds=5,
            browser_ownership=ownership,
        )
        with self.assertRaisesRegex(COLLECTOR.ProcessTreeError, "descendant process tree"):
            COLLECTOR.require_process_completed(outcome, "immediate zero producer")
        self.assertTrue(outcome.cleanup["browser_ownership"]["handshake_verified"])
        self.assertTrue(outcome.cleanup["orphan_descendants_detected"])
        self.assertTrue(outcome.cleanup["group_gone"])

    def test_missing_and_wrong_browser_handshakes_fail_closed(self) -> None:
        for case in ("missing", "wrong-pid"):
            with self.subTest(case=case):
                ownership = self.ownership(case)
                script = self.root / f"{case}.py"
                if case == "missing":
                    script.write_text("pass\n")
                else:
                    script.write_text(
                        "import json, os, pathlib\n"
                        "path=pathlib.Path(os.environ['GSPLAT_Q1_BROWSER_HANDSHAKE_PATH'])\n"
                        "path.parent.mkdir(parents=True, exist_ok=True)\n"
                        "path.write_text(json.dumps({'schema':'gsplat-q1-browser-process-ownership/v1',"
                        "'marker':os.environ['GSPLAT_Q1_BROWSER_OWNER_MARKER'],"
                        "'marker_arg':'--user-data-dir='+os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR'],"
                        "'user_data_dir':os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR'],"
                        "'producer_pid':os.getpid(),'browser_pid':999999}))\n"
                    )
                outcome = COLLECTOR.run_process_group(
                    [sys.executable, str(script)], cwd=self.root,
                    env=self.ownership_environment(ownership), timeout_seconds=5,
                    browser_ownership=ownership,
                )
                with self.assertRaises(COLLECTOR.ProcessTreeError):
                    COLLECTOR.require_process_completed(outcome, case)
                self.assertFalse(outcome.cleanup["browser_ownership"]["handshake_verified"])

    def test_macos_chrome_hidden_argv_requires_related_exact_marker_process(self) -> None:
        ownership = self.ownership("macos-hidden-argv")
        browser_pid = 41001
        browser_pgid = browser_pid
        marker_argument = ownership["marker_argument"]
        handshake = {
            "schema": "gsplat-q1-browser-process-ownership/v1",
            "marker": ownership["marker"],
            "marker_arg": marker_argument,
            "user_data_dir": ownership["user_data_dir"],
            "producer_pid": 40001,
            "browser_pid": browser_pid,
            "browser_spawnfile": ownership["expected_executable"],
            "browser_spawnargs": [
                ownership["expected_executable"],
                marker_argument,
            ],
        }
        handshake_path = pathlib.Path(ownership["handshake_path"])
        handshake_path.parent.mkdir(parents=True)
        handshake_path.write_text(json.dumps(handshake))
        browser = COLLECTOR.ProcessIdentity(
            browser_pid, 40001, browser_pgid, "Tue Jul 28 09:44:50 2026", "(Google Chrome)"
        )
        helper = COLLECTOR.ProcessIdentity(
            41002,
            browser_pid,
            browser_pgid,
            "Tue Jul 28 09:44:51 2026",
            f"/Applications/Google Chrome Helper --type=gpu-process {marker_argument}",
        )

        receipt = COLLECTOR.validate_browser_handshake(
            ownership,
            40001,
            {browser.pid: browser, helper.pid: helper},
            {browser.pid: browser, helper.pid: helper},
        )
        self.assertEqual(receipt["browser_pid"], browser_pid)

        unrelated = COLLECTOR.ProcessIdentity(
            helper.pid,
            42000,
            42000,
            helper.started,
            helper.command,
        )
        with self.assertRaisesRegex(
            COLLECTOR.OrchestrationError, "related exact-marker process"
        ):
            COLLECTOR.validate_browser_handshake(
                ownership,
                40001,
                {browser.pid: browser, unrelated.pid: unrelated},
                {browser.pid: browser, unrelated.pid: unrelated},
            )

        with self.assertRaisesRegex(
            COLLECTOR.OrchestrationError, "related exact-marker process"
        ):
            COLLECTOR.validate_browser_handshake(
                ownership,
                40001,
                {browser.pid: browser, helper.pid: helper},
                {helper.pid: helper},
            )

        invalid_spawn_values = [
            ("missing marker", [handshake["browser_spawnfile"]]),
            (
                "duplicate marker",
                [handshake["browser_spawnfile"], marker_argument, marker_argument],
            ),
            (
                "second user-data-dir",
                [
                    handshake["browser_spawnfile"],
                    marker_argument,
                    "--user-data-dir=/tmp/unrelated",
                ],
            ),
        ]
        for name, spawnargs in invalid_spawn_values:
            with self.subTest(name=name):
                handshake["browser_spawnargs"] = spawnargs
                handshake_path.write_text(json.dumps(handshake))
                with self.assertRaisesRegex(
                    COLLECTOR.OrchestrationError, "spawn identity"
                ):
                    COLLECTOR.validate_browser_handshake(
                        ownership,
                        40001,
                        {browser.pid: browser, helper.pid: helper},
                        {browser.pid: browser, helper.pid: helper},
                    )

        handshake["browser_spawnfile"] = "/tmp/not-the-declared-chrome"
        handshake["browser_spawnargs"] = [
            handshake["browser_spawnfile"],
            marker_argument,
        ]
        handshake_path.write_text(json.dumps(handshake))
        with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "spawn identity"):
            COLLECTOR.validate_browser_handshake(
                ownership,
                40001,
                {browser.pid: browser, helper.pid: helper},
                {browser.pid: browser, helper.pid: helper},
            )

    def test_snapshot_failure_after_immediate_marker_spawn_still_converges(self) -> None:
        ownership = self.ownership("snapshot-race")
        ownership["expected_executable"] = sys.executable
        counter = self.root / "snapshot-race-count"
        script = self.root / "snapshot-race.py"
        script.write_text(
            "import json, os, pathlib, subprocess, sys\n"
            "counter=pathlib.Path(sys.argv[1]); counter.write_text('1')\n"
            "marker_arg='--user-data-dir='+os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR']\n"
            "child=subprocess.Popen([sys.executable,'-c',"
            "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)',marker_arg],"
            "start_new_session=True)\n"
            "receipt={'schema':'gsplat-q1-browser-process-ownership/v1',"
            "'marker':os.environ['GSPLAT_Q1_BROWSER_OWNER_MARKER'],'marker_arg':marker_arg,"
            "'user_data_dir':os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR'],"
            "'producer_pid':os.getpid(),'producer_ppid':os.getppid(),"
            "'browser_pid':child.pid,'browser_spawnfile':sys.executable,"
            "'browser_spawnargs':[sys.executable,marker_arg]}\n"
            "path=pathlib.Path(os.environ['GSPLAT_Q1_BROWSER_HANDSHAKE_PATH']);"
            "path.parent.mkdir(parents=True,exist_ok=True)\n"
            "temporary=path.with_suffix('.tmp'); temporary.write_text(json.dumps(receipt));"
            "temporary.replace(path)\n"
        )
        original_snapshot = COLLECTOR.process_table_snapshot
        injected = False

        def fail_first_cleanup_snapshot():
            nonlocal injected
            if pathlib.Path(ownership["handshake_path"]).exists() and not injected:
                injected = True
                raise COLLECTOR.OrchestrationError("synthetic first cleanup snapshot failure")
            return original_snapshot()

        def idle_tracker(self):
            self._stop.wait(30)

        with (
            mock.patch.object(COLLECTOR.ProcessTreeTracker, "_loop", idle_tracker),
            mock.patch.object(
                COLLECTOR, "process_table_snapshot", side_effect=fail_first_cleanup_snapshot
            ),
        ):
            outcome = COLLECTOR.run_process_group(
                [sys.executable, str(script), str(counter)], cwd=self.root,
                env=self.ownership_environment(ownership), timeout_seconds=5,
                browser_ownership=ownership,
            )
        self.assertTrue(injected)
        self.assertEqual(counter.read_text(), "1")
        self.assertGreaterEqual(outcome.cleanup["cleanup_snapshot_failures"], 1)
        self.assertTrue(outcome.cleanup["browser_ownership"]["handshake_verified"])
        self.assertTrue(outcome.cleanup["group_gone"])
        self.assertEqual(outcome.cleanup["survivors"], [])
        self.assertTrue(outcome.cleanup["kill"]["groups"])
        browser_pid = json.loads(
            pathlib.Path(ownership["handshake_path"]).read_text()
        )["browser_pid"]
        with self.assertRaises(ProcessLookupError):
            os.kill(browser_pid, 0)
        with self.assertRaises(COLLECTOR.ProcessTreeError):
            COLLECTOR.require_process_completed(outcome, "snapshot race")

    def test_marker_first_discovered_after_tracker_stop_is_still_cleaned(self) -> None:
        ownership = self.ownership("post-tracker-discovery")
        ownership["expected_executable"] = sys.executable
        script = self.root / "post-tracker-discovery.py"
        script.write_text(
            "import json, os, pathlib, subprocess, sys\n"
            "marker_arg='--user-data-dir='+os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR']\n"
            "child=subprocess.Popen([sys.executable,'-c',"
            "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)',marker_arg],"
            "start_new_session=True)\n"
            "receipt={'schema':'gsplat-q1-browser-process-ownership/v1',"
            "'marker':os.environ['GSPLAT_Q1_BROWSER_OWNER_MARKER'],'marker_arg':marker_arg,"
            "'user_data_dir':os.environ['GSPLAT_Q1_BROWSER_USER_DATA_DIR'],"
            "'producer_pid':os.getpid(),'producer_ppid':os.getppid(),"
            "'browser_pid':child.pid,'browser_spawnfile':sys.executable,"
            "'browser_spawnargs':[sys.executable,marker_arg]}\n"
            "path=pathlib.Path(os.environ['GSPLAT_Q1_BROWSER_HANDSHAKE_PATH']);"
            "path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(receipt))\n"
        )
        original_snapshot = COLLECTOR.process_table_snapshot
        original_stop = COLLECTOR.ProcessTreeTracker.stop
        tracker_stopped = False

        def idle_tracker(self):
            self._stop.wait(30)

        def mark_stopped(self):
            nonlocal tracker_stopped
            result = original_stop(self)
            tracker_stopped = True
            return result

        def hide_marker_until_post_stop():
            snapshot = original_snapshot()
            if not tracker_stopped:
                for identity in COLLECTOR.marker_processes(
                    snapshot, ownership["marker_argument"]
                ):
                    snapshot.pop(identity.pid, None)
            return snapshot

        with (
            mock.patch.object(COLLECTOR.ProcessTreeTracker, "_loop", idle_tracker),
            mock.patch.object(COLLECTOR.ProcessTreeTracker, "stop", mark_stopped),
            mock.patch.object(
                COLLECTOR,
                "process_table_snapshot",
                side_effect=hide_marker_until_post_stop,
            ),
        ):
            outcome = COLLECTOR.run_process_group(
                [sys.executable, str(script)],
                cwd=self.root,
                env=self.ownership_environment(ownership),
                timeout_seconds=5,
                browser_ownership=ownership,
            )
        self.assertTrue(tracker_stopped)
        self.assertTrue(outcome.cleanup["group_gone"])
        self.assertEqual(outcome.cleanup["survivors"], [])
        self.assertTrue(outcome.cleanup["detached_process_groups"])
        self.assertTrue(outcome.cleanup["kill"]["groups"])
        browser_pid = json.loads(
            pathlib.Path(ownership["handshake_path"]).read_text()
        )["browser_pid"]
        with self.assertRaises(ProcessLookupError):
            os.kill(browser_pid, 0)
        with self.assertRaises(COLLECTOR.ProcessTreeError):
            COLLECTOR.require_process_completed(outcome, "post tracker discovery")

    def test_persistent_cleanup_snapshot_unavailability_fails_closed(self) -> None:
        original_snapshot = COLLECTOR.process_table_snapshot
        calls = 0

        def only_initial_snapshot_available():
            nonlocal calls
            calls += 1
            if calls == 1:
                return original_snapshot()
            raise COLLECTOR.OrchestrationError(
                "synthetic persistent process-table outage"
            )

        with (
            mock.patch.object(
                COLLECTOR,
                "process_table_snapshot",
                side_effect=only_initial_snapshot_available,
            ),
            mock.patch.dict(
                COLLECTOR.PROCESS_TIMEOUTS_SECONDS,
                {"process_group_term_grace": 1, "process_group_kill_grace": 1},
            ),
        ):
            outcome = COLLECTOR.run_process_group(
                [sys.executable, "-c", "pass"],
                cwd=self.root,
                env=COLLECTOR.safe_host_environment(),
                timeout_seconds=5,
            )
        self.assertFalse(outcome.cleanup["final_snapshot_available"])
        self.assertFalse(outcome.cleanup["group_gone"])
        self.assertGreater(outcome.cleanup["cleanup_snapshot_failures"], 0)
        self.assertTrue(outcome.cleanup["leader_reaped"])
        with self.assertRaises(COLLECTOR.ProcessTreeError):
            COLLECTOR.require_process_completed(outcome, "persistent ps outage")

    def test_preexisting_exact_browser_marker_blocks_before_launch(self) -> None:
        ownership = self.ownership("preexisting")
        marker_process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(300)", ownership["marker_argument"]],
            start_new_session=True,
        )
        sentinel = self.root / "must-not-run"
        try:
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "already belongs"):
                COLLECTOR.run_process_group(
                    [sys.executable, "-c", f"open({str(sentinel)!r}, 'w').close()"],
                    cwd=self.root, env=self.ownership_environment(ownership),
                    timeout_seconds=5, browser_ownership=ownership,
                )
            self.assertFalse(sentinel.exists())
        finally:
            os.killpg(marker_process.pid, signal.SIGKILL)
            marker_process.wait()

    def test_ps_failure_after_spawn_reaps_stopped_launcher(self) -> None:
        sentinel = self.root / "continued"
        original = COLLECTOR.process_table_snapshot
        calls = 0

        def fail_first_snapshot():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise COLLECTOR.OrchestrationError("synthetic ps failure")
            return original()

        with mock.patch.object(COLLECTOR, "process_table_snapshot", side_effect=fail_first_snapshot):
            outcome = COLLECTOR.run_process_group(
                [sys.executable, "-c", f"open({str(sentinel)!r}, 'w').close()"],
                cwd=self.root, env=COLLECTOR.safe_host_environment(), timeout_seconds=5,
            )
        with self.assertRaises(COLLECTOR.ProcessTreeError):
            COLLECTOR.require_process_completed(outcome, "ps failure")
        self.assertFalse(sentinel.exists())
        self.assertTrue(outcome.cleanup["leader_reaped"])
        self.assertTrue(outcome.cleanup["group_gone"])
        with self.assertRaises(ProcessLookupError):
            os.kill(outcome.cleanup["process_group_id"], 0)

    def test_throughput_rejects_control_not_bound_to_declared_configuration(self) -> None:
        plan = self.plan()
        COLLECTOR.claim_series(self.args, plan)
        invocation = next(
            item for item in plan["invocations"]
            if item["endpoint"] == "gsplat_rs" and item["artifact_role"] == "throughput"
        )
        bad = {
            "run_id": "control-bad",
            "commit": self.args.reviewed_sha,
            "manifest_sha256": "1" * 64,
            "configuration": "2" * 64,
            "environment": {
                "identity": {
                    "collection_session_id": self.args.collection_session_id,
                },
            },
        }
        with mock.patch.object(COLLECTOR, "admit_artifact", return_value=bad):
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "configuration"):
                COLLECTOR.resolve_throughput(invocation, self.series)
        self.assertFalse((
            self.series / "requests" / f"{invocation['invocation_id']}.bindings.json"
        ).exists())

    def test_final_validator_reads_command_reviewed_build_browser_and_reference_lock(self) -> None:
        self.args.formal_inputs = self.formal_inputs()
        plan = self.plan()
        locked = COLLECTOR.claim_series(self.args, plan)
        post = {
            "schema": COLLECTOR.POST_RUN_SCHEMA,
            "verified_at_utc": "2026-07-28T01:00:00Z",
            "reviewed_commit": self.args.reviewed_sha,
            "git": {"head": self.args.reviewed_sha, "clean": True},
            "formal_inputs_sha256": locked["formal_inputs_sha256"],
            "command_receipt_sha256": locked["command_receipt"]["sha256"],
            "formal_lock_sha256": COLLECTOR.sha256_path(
                self.series / "formal-execution-lock.json"
            ),
            "reference_authority": self.post_authority(locked),
        }
        document = {
            "schedule": plan["schedule"],
            "orchestration": {
                "formal_execution_lock": locked,
                "post_run_verification": post,
            },
        }
        admitted = validate_orchestration(
            document,
            self.series,
            series_id=plan["series_id"],
            schedule_sha=plan["schedule_sha256"],
            protocol_sha=plan["protocol_sha256"],
        )
        self.assertEqual(admitted["reviewed_commit"], self.args.reviewed_sha)
        commands_path = self.series / "commands.json"
        commands_path.write_bytes(commands_path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "command receipt identity"):
            validate_orchestration(
                document,
                self.series,
                series_id=plan["series_id"],
                schedule_sha=plan["schedule_sha256"],
                protocol_sha=plan["protocol_sha256"],
            )

    def test_offline_orchestration_rejects_authority_join_tampering(self) -> None:
        for field in (
            "rgba",
            "pose",
            "receipt",
            "receipt_path",
            "commands",
            "post",
        ):
            with self.subTest(field=field):
                plan, locked, post, document, commands = (
                    self.formal_orchestration_bundle(f"tamper-{field}")
                )
                if field == "rgba":
                    locked["formal_inputs"]["references"][0][
                        "rgba8_sha256"
                    ] = "e" * 64
                elif field == "pose":
                    locked["formal_inputs"]["references"][0][
                        "pose_intrinsics_sha256"
                    ] = "e" * 64
                elif field == "receipt":
                    locked["formal_inputs"]["references"][0][
                        "authority_receipt_sha256"
                    ] = "e" * 64
                elif field == "receipt_path":
                    locked["formal_inputs"]["references"][0][
                        "authority_receipt_path"
                    ] = "other-authority/reference.json"
                self.rewrite_formal_orchestration_bundle(locked, post, commands)
                if field == "commands":
                    commands["reference_authority"]["series_root"] = "forged"
                    self.rewrite_formal_orchestration_bundle(
                        locked, post, commands
                    )
                elif field == "post":
                    post["reference_authority"]["source_post_sha256"] = "e" * 64
                with self.assertRaisesRegex(ValueError, "authority|command receipt"):
                    validate_orchestration(
                        document,
                        self.args.series_root,
                        series_id=plan["series_id"],
                        schedule_sha=plan["schedule_sha256"],
                        protocol_sha=plan["protocol_sha256"],
                    )

    def test_self_consistent_forged_formal_tree_rejects_actual_retained_tree(self) -> None:
        plan, locked, post, document, commands = self.formal_orchestration_bundle(
            "tamper-tree"
        )
        formal_authority = locked["formal_inputs"]["reference_authority"]
        forged_entry = {
            "path": "forged-support.log",
            "bytes": 6,
            "sha256": hashlib.sha256(b"forged").hexdigest(),
        }
        formal_authority["tree"]["files"].append(forged_entry)
        formal_authority["tree"]["files"].sort(key=lambda value: value["path"])
        formal_authority["tree"]["file_count"] += 1
        formal_authority["tree"]["bytes"] += forged_entry["bytes"]
        formal_authority["tree"]["sha256"] = COLLECTOR.canonical_sha256(
            formal_authority["tree"]["files"]
        )
        commands["reference_authority"] = {
            "source": formal_authority,
            "series_root": "reference-authority",
            "series_receipt_path": "reference-authority/reference.json",
            "series_files": [
                {
                    **entry,
                    "destination": f"reference-authority/{entry['path']}",
                }
                for entry in formal_authority["tree"]["files"]
            ],
        }
        claimed = json.loads(json.dumps(formal_authority))
        claimed["root_path"] = "reference-authority"
        locked["reference_authority"] = {
            "source_pre_sha256": COLLECTOR.canonical_sha256(formal_authority),
            "claimed_pre": claimed,
            "claimed_pre_sha256": COLLECTOR.canonical_sha256(claimed),
        }
        self.rewrite_formal_orchestration_bundle(locked, post, commands)
        with self.assertRaisesRegex(ValueError, "retained authority"):
            validate_orchestration(
                document,
                self.args.series_root,
                series_id=plan["series_id"],
                schedule_sha=plan["schedule_sha256"],
                protocol_sha=plan["protocol_sha256"],
            )

    def test_postprocess_chrome_must_equal_frozen_browser(self) -> None:
        self.args.formal_inputs = self.formal_inputs()
        plan = self.plan()
        plan["postprocess"]["environment"]["CHROME_PATH"] = "/tmp/drifted-chrome"
        locked = COLLECTOR.claim_series(self.args, plan)
        document = {
            "schedule": plan["schedule"],
            "orchestration": {
                "formal_execution_lock": locked,
                "post_run_verification": {
                    "schema": COLLECTOR.POST_RUN_SCHEMA,
                    "verified_at_utc": "2026-07-28T01:00:00Z",
                    "reviewed_commit": self.args.reviewed_sha,
                    "git": {"head": self.args.reviewed_sha, "clean": True},
                    "formal_inputs_sha256": locked["formal_inputs_sha256"],
                    "command_receipt_sha256": locked["command_receipt"]["sha256"],
                    "formal_lock_sha256": COLLECTOR.sha256_path(
                        self.series / "formal-execution-lock.json"
                    ),
                    "reference_authority": self.post_authority(locked),
                },
            },
        }
        with self.assertRaisesRegex(ValueError, "postprocess environment"):
            validate_orchestration(
                document,
                self.series,
                series_id=plan["series_id"],
                schedule_sha=plan["schedule_sha256"],
                protocol_sha=plan["protocol_sha256"],
            )

    def test_post_run_rehash_rejects_browser_executable_drift(self) -> None:
        expected = self.formal_inputs()
        observed = json.loads(json.dumps(expected))
        observed["browser"]["sha256"] = "9" * 64
        with (
            mock.patch.object(COLLECTOR, "git_output", return_value=""),
            mock.patch.object(COLLECTOR, "capture_formal_inputs", return_value=observed),
        ):
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "browser.*drifted"):
                COLLECTOR.verify_formal_inputs(self.args, expected)

    def test_image_comparison_rejects_actual_chrome_hash_drift(self) -> None:
        chrome = self.args.chrome
        chrome.write_bytes(b"locked chrome")
        candidate = self.root / "pairs/pair-01/playcanvas/control-trace-0/final-frame.png"
        candidate.parent.mkdir(parents=True)
        candidate.write_bytes(b"candidate")
        reference = self.root / "reference/trace-0.png"
        reference.parent.mkdir(parents=True)
        reference.write_bytes(self.reference[0].read_bytes())

        def fake_run(argv, **kwargs):
            output = pathlib.Path(argv[argv.index("--output") + 1])
            output.write_text(json.dumps({
                "score": 1.0,
                "browser": {
                    "executablePath": str(chrome),
                    "sha256": "0" * 64,
                },
            }))
            return COLLECTOR.ProcessOutcome(
                argv=argv,
                returncode=0,
                stdout="",
                stderr="",
                timed_out=False,
                timeout_seconds=kwargs["timeout_seconds"],
                cleanup={
                    "isolated_process_group": True,
                    "lineage_complete": True,
                    "group_gone": True,
                    "orphan_descendants_detected": False,
                },
            )

        with mock.patch.object(COLLECTOR, "run_process_group", side_effect=fake_run):
            with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "locked Chrome"):
                COLLECTOR.compare_image(
                    root=self.root,
                    pair_id="pair-01",
                    endpoint="playcanvas",
                    trace=0,
                    reference={"path": "reference/trace-0.png", "sha256": "1" * 64},
                    environment={"PATH": "/usr/bin", "HOME": str(self.root), "CHROME_PATH": str(chrome)},
                )

    def test_puppeteer_production_dependency_bytes_are_frozen(self) -> None:
        root = self.root / "playcanvas"
        dependency = root / "node_modules/puppeteer-core"
        child = root / "node_modules/ws"
        dependency.mkdir(parents=True)
        child.mkdir(parents=True)
        (dependency / "index.js").write_text("import 'ws';\n")
        (dependency / "package.json").write_text(json.dumps({
            "name": "puppeteer-core",
            "version": "1.0.0",
            "dependencies": {"ws": "1.0.0"},
        }))
        (dependency / "tests").mkdir()
        (dependency / "tests/ignored.js").write_text("ignored\n")
        (child / "index.js").write_text("export {};\n")
        (child / "package.json").write_text(json.dumps({"name": "ws", "version": "1.0.0"}))
        (root / "package-lock.json").write_text(json.dumps({
            "lockfileVersion": 3,
            "packages": {
                "node_modules/puppeteer-core": {
                    "version": "1.0.0",
                    "integrity": "sha512-root",
                    "dependencies": {"ws": "1.0.0"},
                },
                "node_modules/ws": {
                    "version": "1.0.0",
                    "integrity": "sha512-child",
                },
            },
        }))
        before = COLLECTOR.puppeteer_production_modules(root)
        self.assertEqual(before["package_count"], 2)
        (child / "index.js").write_text("export const drift = true;\n")
        after = COLLECTOR.puppeteer_production_modules(root)
        self.assertNotEqual(before["sha256"], after["sha256"])
        self.assertEqual(
            before["packages"][0 if before["packages"][0]["lock_path"] == "node_modules/puppeteer-core" else 1]["file_count"],
            2,
        )

    def test_installed_optional_dependency_is_hashed_and_missing_required_fails(self) -> None:
        root = self.root / "optional-playcanvas"
        dependency = root / "node_modules/puppeteer-core"
        optional = root / "node_modules/source-map-support"
        dependency.mkdir(parents=True)
        optional.mkdir(parents=True)
        root_manifest = {
            "name": "puppeteer-core",
            "version": "1.0.0",
            "dependencies": {"required-runtime": "1.0.0"},
            "optionalDependencies": {"source-map-support": "1.0.0", "platform-only": "1.0.0"},
        }
        (dependency / "package.json").write_text(json.dumps(root_manifest))
        (dependency / "index.js").write_text("export {};\n")
        (optional / "package.json").write_text(json.dumps({
            "name": "source-map-support", "version": "1.0.0"
        }))
        (optional / "index.js").write_text("export const value = 1;\n")
        lock = {
            "lockfileVersion": 3,
            "packages": {
                "node_modules/puppeteer-core": {
                    "version": "1.0.0",
                    "integrity": "sha512-root",
                    "dependencies": {"required-runtime": "1.0.0"},
                    "optionalDependencies": {
                        "source-map-support": "1.0.0",
                        "platform-only": "1.0.0",
                    },
                },
                "node_modules/required-runtime": {
                    "version": "1.0.0", "integrity": "sha512-required"
                },
                "node_modules/source-map-support": {
                    "version": "1.0.0", "integrity": "sha512-optional"
                },
            },
        }
        (root / "package-lock.json").write_text(json.dumps(lock))
        with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "required runtime dependency"):
            COLLECTOR.puppeteer_production_modules(root)

        required = root / "node_modules/required-runtime"
        required.mkdir()
        (required / "package.json").write_text(json.dumps({
            "name": "required-runtime", "version": "1.0.0"
        }))
        (required / "index.js").write_text("export {};\n")
        before = COLLECTOR.puppeteer_production_modules(root)
        locked_paths = {value["lock_path"] for value in before["packages"]}
        self.assertIn("node_modules/source-map-support", locked_paths)
        self.assertNotIn("node_modules/platform-only", locked_paths)
        (optional / "index.js").write_text("export const value = 2;\n")
        after = COLLECTOR.puppeteer_production_modules(root)
        self.assertNotEqual(before["sha256"], after["sha256"])
        platform = root / "node_modules/platform-only"
        platform.mkdir()
        (platform / "package.json").write_text(json.dumps({
            "name": "platform-only", "version": "1.0.0"
        }))
        (platform / "index.js").write_text("export {};\n")
        with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "absent from package-lock"):
            COLLECTOR.puppeteer_production_modules(root)

    def test_lock_required_dependency_cannot_be_reclassified_optional(self) -> None:
        root = self.root / "reclassified-playcanvas"
        package = root / "node_modules/puppeteer-core"
        package.mkdir(parents=True)
        (package / "index.js").write_text("export {};\n")
        (package / "package.json").write_text(json.dumps({
            "name": "puppeteer-core",
            "version": "1.0.0",
            "optionalDependencies": {"required-runtime": "1.0.0"},
        }))
        (root / "package-lock.json").write_text(json.dumps({
            "lockfileVersion": 3,
            "packages": {
                "node_modules/puppeteer-core": {
                    "version": "1.0.0",
                    "integrity": "sha512-root",
                    "dependencies": {"required-runtime": "1.0.0"},
                },
                "node_modules/required-runtime": {
                    "version": "1.0.0",
                    "integrity": "sha512-required",
                },
            },
        }))
        with self.assertRaisesRegex(COLLECTOR.OrchestrationError, "differs from package-lock"):
            COLLECTOR.puppeteer_production_modules(root)

    def test_module_symlinks_and_installed_identity_mismatch_fail_closed(self) -> None:
        for case in ("directory-symlink", "manifest-symlink", "identity-mismatch"):
            with self.subTest(case=case):
                root = self.root / case
                node_modules = root / "node_modules"
                node_modules.mkdir(parents=True)
                outside = self.root / f"{case}-outside"
                outside.mkdir()
                package = node_modules / "puppeteer-core"
                if case == "directory-symlink":
                    (outside / "package.json").write_text(json.dumps({
                        "name": "puppeteer-core", "version": "1.0.0"
                    }))
                    package.symlink_to(outside, target_is_directory=True)
                else:
                    package.mkdir()
                    manifest = {
                        "name": "wrong-name" if case == "identity-mismatch" else "puppeteer-core",
                        "version": "2.0.0" if case == "identity-mismatch" else "1.0.0",
                    }
                    if case == "manifest-symlink":
                        target = outside / "package.json"
                        target.write_text(json.dumps(manifest))
                        (package / "package.json").symlink_to(target)
                    else:
                        (package / "package.json").write_text(json.dumps(manifest))
                (root / "package-lock.json").write_text(json.dumps({
                    "lockfileVersion": 3,
                    "packages": {
                        "node_modules/puppeteer-core": {
                            "version": "1.0.0", "integrity": "sha512-root"
                        },
                    },
                }))
                with self.assertRaisesRegex(
                    COLLECTOR.OrchestrationError,
                    "symlink|name mismatch|version mismatch",
                ):
                    COLLECTOR.puppeteer_production_modules(root)


if __name__ == "__main__":
    unittest.main()
