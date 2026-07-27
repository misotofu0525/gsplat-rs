from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
import pathlib
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
from q1_pair_admission.contract import validate_orchestration  # noqa: E402


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
        self.reference = {}
        for trace in (0, 1):
            path = self.root / f"reference-source-{trace}.png"
            path.write_bytes(rgba_png_bytes(trace))
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
        self.args.formal_inputs = {
            "references": [
                {
                    "trace_frame_index": trace,
                    "source_path": str(self.reference[trace]),
                    "series_path": f"reference/trace-{trace}.png",
                    **COLLECTOR.frozen_rgba8_png_receipt(
                        self.reference[trace], f"test reference {trace}"
                    ),
                }
                for trace in (0, 1)
            ]
        }

    def tearDown(self) -> None:
        self.temp.cleanup()

    def plan(self):
        return COLLECTOR.build_plan(
            self.args, predeclared_at="2026-07-28T00:00:00Z"
        )

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
            "references": self.args.formal_inputs["references"],
        }

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
        admitted = []
        for trace in (0, 1):
            admitted.append({
                "run_id": f"pc-control-{trace}",
                "commit": self.args.reviewed_sha,
                "manifest_sha256": str(trace + 1) * 64,
                "configuration": plan["configuration_sha256"],
                "environment": {
                    "collection_session_id": self.args.collection_session_id,
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
            cleanup={"group_gone": True},
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
            "'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)'])\n"
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
        self.assertTrue(blocker["process_timeout"]["cleanup"]["term_sent"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["kill_sent"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["leader_reaped"])
        self.assertTrue(blocker["process_timeout"]["cleanup"]["group_gone"])
        self.assertIn("tree-ready", blocker["process_timeout"]["stdout_tail"])
        receipt = json.loads(
            (self.series / "logs" / f"{first['invocation_id']}.process.json").read_text()
        )
        self.assertTrue(receipt["timed_out"])
        self.assertTrue(receipt["cleanup"]["group_gone"])

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
            "environment": {"collection_session_id": self.args.collection_session_id},
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
                cleanup={"group_gone": True},
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


if __name__ == "__main__":
    unittest.main()
