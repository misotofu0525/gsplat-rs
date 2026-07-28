from __future__ import annotations

import ast
import importlib.util
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("q1_browser_process_owner.py")
SPEC = importlib.util.spec_from_file_location("q1_browser_process_owner_tests", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
OWNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = OWNER
SPEC.loader.exec_module(OWNER)


class Q1BrowserProcessOwnerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.chrome = self.root / "Chrome"
        self.chrome.write_text("#!/bin/sh\n", encoding="utf-8")
        self.chrome.chmod(0o755)
        self.args = SimpleNamespace(chrome=self.chrome)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def ownership(self, name: str) -> dict[str, str]:
        value = OWNER.browser_ownership(self.root, name, self.chrome)
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

    def safe_environment(self) -> dict[str, str]:
        return {
            key: os.environ[key]
            for key in ("PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE")
            if key in os.environ
        }

    def ownership_environment(self, ownership: dict[str, str]) -> dict[str, str]:
        return {
            **self.safe_environment(),
            "GSPLAT_Q1_BROWSER_OWNER_MARKER": ownership["marker"],
            "GSPLAT_Q1_BROWSER_USER_DATA_DIR": ownership["user_data_dir"],
            "GSPLAT_Q1_BROWSER_HANDSHAKE_PATH": ownership["handshake_path"],
        }

    def test_paired_collector_delegates_to_shared_owner_without_duplicate(self) -> None:
        paired_path = MODULE_PATH.with_name("collect-q1-truck-paired-series.py")
        source = paired_path.read_text(encoding="utf-8")
        tree = ast.parse(source)
        functions = {
            node.name: node for node in tree.body if isinstance(node, ast.FunctionDef)
        }
        self.assertIn("run_process_group", functions)
        self.assertNotIn("browser_ownership", functions)
        wrapper = ast.get_source_segment(source, functions["run_process_group"])
        self.assertIsNotNone(wrapper)
        self.assertIn("PROCESS_OWNER.run_process_group", wrapper)
        self.assertIn('"tests/perf/q1_browser_process_owner.py"', source)

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
        outcome = OWNER.run_process_group(
            [sys.executable, str(script)], cwd=self.root,
            env=self.ownership_environment(ownership), timeout_seconds=5,
            browser_ownership=ownership,
        )
        with self.assertRaisesRegex(OWNER.ProcessTreeError, "descendant process tree"):
            OWNER.require_process_completed(outcome, "immediate zero producer")
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
                outcome = OWNER.run_process_group(
                    [sys.executable, str(script)], cwd=self.root,
                    env=self.ownership_environment(ownership), timeout_seconds=5,
                    browser_ownership=ownership,
                )
                with self.assertRaises(OWNER.ProcessTreeError):
                    OWNER.require_process_completed(outcome, case)
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
        browser = OWNER.ProcessIdentity(
            browser_pid, 40001, browser_pgid, "Tue Jul 28 09:44:50 2026", "(Google Chrome)"
        )
        helper = OWNER.ProcessIdentity(
            41002,
            browser_pid,
            browser_pgid,
            "Tue Jul 28 09:44:51 2026",
            f"/Applications/Google Chrome Helper --type=gpu-process {marker_argument}",
        )

        receipt = OWNER.validate_browser_handshake(
            ownership,
            40001,
            {browser.pid: browser, helper.pid: helper},
            {browser.pid: browser, helper.pid: helper},
        )
        self.assertEqual(receipt["browser_pid"], browser_pid)

        unrelated = OWNER.ProcessIdentity(
            helper.pid,
            42000,
            42000,
            helper.started,
            helper.command,
        )
        with self.assertRaisesRegex(
            OWNER.ProcessOwnershipError, "related exact-marker process"
        ):
            OWNER.validate_browser_handshake(
                ownership,
                40001,
                {browser.pid: browser, unrelated.pid: unrelated},
                {browser.pid: browser, unrelated.pid: unrelated},
            )

        with self.assertRaisesRegex(
            OWNER.ProcessOwnershipError, "related exact-marker process"
        ):
            OWNER.validate_browser_handshake(
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
                    OWNER.ProcessOwnershipError, "spawn identity"
                ):
                    OWNER.validate_browser_handshake(
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
        with self.assertRaisesRegex(OWNER.ProcessOwnershipError, "spawn identity"):
            OWNER.validate_browser_handshake(
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
        original_snapshot = OWNER.process_table_snapshot
        injected = False

        def fail_first_cleanup_snapshot():
            nonlocal injected
            if pathlib.Path(ownership["handshake_path"]).exists() and not injected:
                injected = True
                raise OWNER.ProcessOwnershipError("synthetic first cleanup snapshot failure")
            return original_snapshot()

        def idle_tracker(self):
            self._stop.wait(30)

        with (
            mock.patch.object(OWNER.ProcessTreeTracker, "_loop", idle_tracker),
            mock.patch.object(
                OWNER, "process_table_snapshot", side_effect=fail_first_cleanup_snapshot
            ),
        ):
            outcome = OWNER.run_process_group(
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
        with self.assertRaises(OWNER.ProcessTreeError):
            OWNER.require_process_completed(outcome, "snapshot race")

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
        original_snapshot = OWNER.process_table_snapshot
        original_stop = OWNER.ProcessTreeTracker.stop
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
                for identity in OWNER.marker_processes(
                    snapshot, ownership["marker_argument"]
                ):
                    snapshot.pop(identity.pid, None)
            return snapshot

        with (
            mock.patch.object(OWNER.ProcessTreeTracker, "_loop", idle_tracker),
            mock.patch.object(OWNER.ProcessTreeTracker, "stop", mark_stopped),
            mock.patch.object(
                OWNER,
                "process_table_snapshot",
                side_effect=hide_marker_until_post_stop,
            ),
        ):
            outcome = OWNER.run_process_group(
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
        with self.assertRaises(OWNER.ProcessTreeError):
            OWNER.require_process_completed(outcome, "post tracker discovery")

    def test_persistent_cleanup_snapshot_unavailability_fails_closed(self) -> None:
        original_snapshot = OWNER.process_table_snapshot
        calls = 0

        def only_initial_snapshot_available():
            nonlocal calls
            calls += 1
            if calls == 1:
                return original_snapshot()
            raise OWNER.ProcessOwnershipError(
                "synthetic persistent process-table outage"
            )

        with (
            mock.patch.object(
                OWNER,
                "process_table_snapshot",
                side_effect=only_initial_snapshot_available,
            ),
            mock.patch.object(OWNER, "PROCESS_GROUP_TERM_GRACE_SECONDS", 1),
            mock.patch.object(OWNER, "PROCESS_GROUP_KILL_GRACE_SECONDS", 1),
        ):
            outcome = OWNER.run_process_group(
                [sys.executable, "-c", "pass"],
                cwd=self.root,
                env=self.safe_environment(),
                timeout_seconds=5,
            )
        self.assertFalse(outcome.cleanup["final_snapshot_available"])
        self.assertFalse(outcome.cleanup["group_gone"])
        self.assertGreater(outcome.cleanup["cleanup_snapshot_failures"], 0)
        self.assertTrue(outcome.cleanup["leader_reaped"])
        with self.assertRaises(OWNER.ProcessTreeError):
            OWNER.require_process_completed(outcome, "persistent ps outage")

    def test_preexisting_exact_browser_marker_blocks_before_launch(self) -> None:
        ownership = self.ownership("preexisting")
        marker_process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(300)", ownership["marker_argument"]],
            start_new_session=True,
        )
        sentinel = self.root / "must-not-run"
        try:
            with self.assertRaisesRegex(OWNER.ProcessOwnershipError, "already belongs"):
                OWNER.run_process_group(
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
        original = OWNER.process_table_snapshot
        calls = 0

        def fail_first_snapshot():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise OWNER.ProcessOwnershipError("synthetic ps failure")
            return original()

        with mock.patch.object(OWNER, "process_table_snapshot", side_effect=fail_first_snapshot):
            outcome = OWNER.run_process_group(
                [sys.executable, "-c", f"open({str(sentinel)!r}, 'w').close()"],
                cwd=self.root, env=self.safe_environment(), timeout_seconds=5,
            )
        with self.assertRaises(OWNER.ProcessTreeError):
            OWNER.require_process_completed(outcome, "ps failure")
        self.assertFalse(sentinel.exists())
        self.assertTrue(outcome.cleanup["leader_reaped"])
        self.assertTrue(outcome.cleanup["group_gone"])
        with self.assertRaises(ProcessLookupError):
            os.kill(outcome.cleanup["process_group_id"], 0)



if __name__ == "__main__":
    unittest.main()
