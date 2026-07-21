#!/usr/bin/env python3
"""Unit tests for the Android sort benchmark collector's pure orchestration."""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import pathlib
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = pathlib.Path(__file__).with_name("collect-android-sort-benchmarks.py")
SPEC = importlib.util.spec_from_file_location("android_sort_collector", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


class ScheduleTests(unittest.TestCase):
    def test_schedule_is_paired_and_balanced(self) -> None:
        schedule = COLLECTOR.build_schedule(["cpu", "gpu"], 3, False, 99)
        self.assertEqual(
            [(run.repetition, run.position, run.backend) for run in schedule],
            [
                (1, 1, "cpu"),
                (1, 2, "gpu"),
                (2, 1, "cpu"),
                (2, 2, "gpu"),
                (3, 1, "cpu"),
                (3, 2, "gpu"),
            ],
        )
        self.assertEqual([run.index for run in schedule], list(range(1, 7)))

    def test_random_schedule_is_seeded_and_keeps_each_pair_complete(self) -> None:
        first = COLLECTOR.build_schedule(
            ["cpu", "gpu", "adaptive"], 8, True, 20260722
        )
        second = COLLECTOR.build_schedule(
            ["cpu", "gpu", "adaptive"], 8, True, 20260722
        )
        self.assertEqual(first, second)
        for repetition in range(1, 9):
            backends = {
                run.backend for run in first if run.repetition == repetition
            }
            self.assertEqual(backends, {"cpu", "gpu", "adaptive"})


class ParsingTests(unittest.TestCase):
    def test_default_frame_count_bounds_android_log_artifact_burst(self) -> None:
        args = COLLECTOR.parser().parse_args(
            ["--serial", "serial", "--ply", __file__]
        )
        self.assertEqual(args.frames, 80)

    def test_thermal_status_variants(self) -> None:
        self.assertEqual(COLLECTOR.parse_thermal_status("Thermal Status: 0\n"), 0)
        self.assertEqual(COLLECTOR.parse_thermal_status("status: 3\n"), 3)
        self.assertEqual(COLLECTOR.parse_thermal_status("  2\n"), 2)
        self.assertIsNone(COLLECTOR.parse_thermal_status("no status available"))

    def test_thermal_status_falls_back_to_dumpsys(self) -> None:
        unsupported = COLLECTOR.subprocess.CompletedProcess(
            args=[], returncode=255, stdout="Unknown command: get-status\n"
        )
        dumpsys = COLLECTOR.subprocess.CompletedProcess(
            args=[], returncode=0, stdout="Thermal Status: 0\n"
        )
        with mock.patch.object(
            COLLECTOR.subprocess, "run", side_effect=[unsupported, dumpsys]
        ) as run:
            self.assertEqual(COLLECTOR.read_thermal_status("adb", "device"), 0)
        self.assertEqual(run.call_count, 2)

    def test_launch_arguments_are_typed_and_select_backend(self) -> None:
        args = argparse.Namespace(
            frames=240,
            warmup=30,
            yaw=0.002,
            sort_interval=2,
            async_sort=False,
            frame_latency=3,
        )
        launch = COLLECTOR.benchmark_launch_args(args, "gpu")
        self.assertIn("gsplat_surface_order_backend", launch)
        self.assertEqual(launch[-1], "gpu")
        self.assertIn("gsplat_geometry_path", launch)
        self.assertIn("direct", launch)
        self.assertEqual(launch[launch.index("gsplat_benchmark_frames") + 1], "240")

    def test_forced_backend_and_dataset_identity_are_validated(self) -> None:
        manifest = {
            "renderer": {"order_backend_requested": "gpu"},
            "dataset": {"sha256": "abc", "bytes": 123},
        }
        summary = {
            "sample_count": 8,
            "sort_telemetry": {
                "cpu_frame_count": 0,
                "gpu_frame_count": 8,
                "gpu_sort_fallback_count": 0,
            },
        }
        COLLECTOR.validate_run_artifact(
            manifest, summary, "gpu", {"sha256": "abc", "bytes": 123}
        )

        summary["sort_telemetry"]["cpu_frame_count"] = 1
        summary["sort_telemetry"]["gpu_frame_count"] = 7
        with self.assertRaisesRegex(RuntimeError, "contains cpu or fallback"):
            COLLECTOR.validate_run_artifact(
                manifest, summary, "gpu", {"sha256": "abc", "bytes": 123}
            )

    def test_artifact_rejects_wrong_packaged_dataset(self) -> None:
        manifest = {
            "renderer": {"order_backend_requested": "cpu"},
            "dataset": {"sha256": "wrong", "bytes": 123},
        }
        summary = {
            "sample_count": 1,
            "sort_telemetry": {
                "cpu_frame_count": 1,
                "gpu_frame_count": 0,
                "gpu_sort_fallback_count": 0,
            },
        }
        with self.assertRaisesRegex(RuntimeError, "dataset sha256"):
            COLLECTOR.validate_run_artifact(
                manifest, summary, "cpu", {"sha256": "abc", "bytes": 123}
            )


class SafetyTests(unittest.TestCase):
    def test_async_sort_rejects_gpu_or_adaptive_backend(self) -> None:
        args = COLLECTOR.parser().parse_args(
            [
                "--serial",
                "serial",
                "--ply",
                __file__,
                "--backend",
                "cpu",
                "--backend",
                "gpu",
                "--async-sort",
            ]
        )
        with self.assertRaisesRegex(ValueError, "only compatible with the cpu"):
            COLLECTOR.validate_args(args)

    def test_existing_output_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                COLLECTOR.fresh_output_root(pathlib.Path(directory), dry_run=False)

    def test_only_fixed_sample_package_is_used_for_clear(self) -> None:
        self.assertEqual(COLLECTOR.PACKAGE, "com.gsplat.example")
        self.assertEqual(
            COLLECTOR.INTERNAL_DATASET, "files/imported_scene.ply"
        )
        args = COLLECTOR.parser().parse_args(
            ["--serial", "serial", "--ply", __file__, "--dry-run"]
        )
        launch = COLLECTOR.benchmark_launch_args(args, "cpu")
        self.assertEqual(launch[launch.index("-n") + 1], "com.gsplat.example/.MainActivity")

    def test_dry_run_does_not_create_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "planned"
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = COLLECTOR.main(
                    [
                        "--serial",
                        "test-device",
                        "--ply",
                        __file__,
                        "--backend",
                        "cpu",
                        "--output",
                        str(output),
                        "--dry-run",
                    ]
                )
            self.assertEqual(result, 0)
            self.assertFalse(output.exists())
            self.assertIn("backend=cpu", stdout.getvalue())

    def test_default_dry_run_reuses_apk_and_pushes_dataset_once(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "planned"
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = COLLECTOR.main(
                    [
                        "--serial",
                        "test-device",
                        "--ply",
                        __file__,
                        "--backend",
                        "cpu",
                        "--backend",
                        "gpu",
                        "--output",
                        str(output),
                        "--dry-run",
                    ]
                )
        plan = stdout.getvalue()
        self.assertEqual(result, 0)
        self.assertIn("apk_mode=reuse-exact-installed", plan)
        self.assertNotIn(" install -r ", plan)
        self.assertEqual(plan.count(" push "), 1)
        self.assertEqual(plan.count(" cp "), 2)
        self.assertEqual(plan.count("files/imported_scene.ply"), 6)
        self.assertEqual(plan.count(" rm -f "), 1)

    def test_prepare_mode_builds_only_tiny_bootstrap_asset(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "planned"
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                result = COLLECTOR.main(
                    [
                        "--serial",
                        "test-device",
                        "--ply",
                        __file__,
                        "--backend",
                        "cpu",
                        "--prepare-apk",
                        "--output",
                        str(output),
                        "--dry-run",
                    ]
                )
        plan = stdout.getvalue()
        self.assertEqual(result, 0)
        self.assertIn("apk_mode=prepare-once", plan)
        self.assertIn("tests/datasets/minimal_ascii.ply", plan)
        self.assertEqual(plan.count(" install -r "), 1)
        self.assertEqual(plan.count(" push "), 1)

    def test_installed_apk_hash_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            apk = pathlib.Path(directory) / "sample.apk"
            apk.write_bytes(b"local apk")
            actual = {"sha256": "0" * 64, "bytes": apk.stat().st_size}
            with (
                mock.patch.object(
                    COLLECTOR,
                    "installed_base_apk_path",
                    return_value="/data/app/pkg/base.apk",
                ),
                mock.patch.object(
                    COLLECTOR, "read_device_file_identity", return_value=actual
                ),
                mock.patch.object(COLLECTOR, "run_command") as run,
            ):
                with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                    COLLECTOR.verify_installed_apk("adb", "serial", apk)
            run.assert_not_called()

    def test_staged_dataset_is_cleaned_after_run_failure(self) -> None:
        expected = {"sha256": "a" * 64, "bytes": 123}
        temporary_path = COLLECTOR.device_dataset_path(expected["sha256"])
        with (
            mock.patch.object(
                COLLECTOR,
                "run_command",
                return_value=COLLECTOR.subprocess.CompletedProcess([], 0, ""),
            ),
            mock.patch.object(
                COLLECTOR,
                "read_device_file_identity",
                return_value=expected,
            ),
            mock.patch.object(COLLECTOR, "cleanup_device_dataset") as cleanup,
        ):
            with self.assertRaisesRegex(RuntimeError, "synthetic run failure"):
                with COLLECTOR.staged_device_dataset(
                    "adb", "serial", pathlib.Path(__file__), expected
                ) as staged:
                    self.assertEqual(staged, temporary_path)
                    raise RuntimeError("synthetic run failure")
        cleanup.assert_called_once_with("adb", "serial", temporary_path)

    def test_cleanup_refuses_any_path_outside_exact_benchmark_prefix(self) -> None:
        with mock.patch.object(COLLECTOR, "run_command") as run:
            with self.assertRaisesRegex(ValueError, "refusing to clean"):
                COLLECTOR.cleanup_device_dataset(
                    "adb", "serial", "/data/local/tmp/unrelated.ply"
                )
        run.assert_not_called()

    def test_cleanup_removes_only_the_exact_hash_addressed_file(self) -> None:
        temporary_path = COLLECTOR.device_dataset_path("b" * 64)
        completed = COLLECTOR.subprocess.CompletedProcess([], 0, "")
        with mock.patch.object(
            COLLECTOR, "run_command", return_value=completed
        ) as run:
            COLLECTOR.cleanup_device_dataset("adb", "serial", temporary_path)
        run.assert_called_once_with(
            [
                "adb",
                "-s",
                "serial",
                "shell",
                "rm",
                "-f",
                temporary_path,
            ],
            capture=True,
        )


if __name__ == "__main__":
    unittest.main()
