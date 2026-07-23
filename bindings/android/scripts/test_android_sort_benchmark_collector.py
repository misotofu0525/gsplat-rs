#!/usr/bin/env python3
"""Unit tests for the Android sort benchmark collector's pure orchestration."""

from __future__ import annotations

import argparse
import copy
import contextlib
import importlib.util
import io
import json
import pathlib
import subprocess
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
TEST_CAMERA_TRACE = (
    COLLECTOR.REPO_ROOT
    / "tests/perf/trace/fixtures/quality/candidate-truck-quality-2412x1080-v1.json"
)


def camera_validation_fixture(backend: str = "gpu", sample_count: int = 1):
    position = [1.25, -0.5, 3.0]
    rotation = [0.0, 0.0, 0.0, 1.0]
    intrinsics = {
        "vertical_fov_radians": 1.0,
        "near_plane": 0.01,
        "far_plane": 100.0,
    }
    matrices = COLLECTOR._canonical_matrices_from_runtime_receipt(
        position, rotation, intrinsics, 2.0
    )
    trace_frame = {
        "frame_index": 0,
        "timestamp_ns": 0,
        "pose": {"position": position, "rotation_xyzw": rotation},
        "intrinsics": intrinsics,
        "view_matrix": matrices[0],
        "projection_matrix": matrices[1],
        "view_projection_matrix": matrices[2],
    }
    expected_trace = {
        "schema": "gsplat-camera-trace/v1",
        "trace_id": "unit-camera-trace",
        "content_sha256": "1" * 64,
        "coordinate_system": copy.deepcopy(COLLECTOR.CANONICAL_COORDINATE_SYSTEM),
        "matrix_convention": copy.deepcopy(COLLECTOR.CANONICAL_MATRIX_CONVENTION),
        "display": {"width": 200, "height": 100},
        "frames": [trace_frame],
    }
    expected_identity = {"sha256": "2" * 64, "bytes": 1234}
    manifest = {
        "renderer": {
            "order_backend_requested": backend,
            "path": "packed_atlas",
        },
        "dataset": {"sha256": "abc", "bytes": 123},
        "trace": {
            "schema": "gsplat-camera-trace/v1",
            "id": "unit-camera-trace",
            "sha256": "1" * 64,
            "file_sha256": "2" * 64,
            "reference_width": 200,
            "reference_height": 100,
            "require_display_match": True,
            "display_policy": "trace_display_exact",
            "quality_comparable": True,
            "playback_mode": "fixed",
            "measured_frames_per_loop": sample_count,
            "loops": 1,
            "frame_index": 0,
            "coordinate_system": copy.deepcopy(COLLECTOR.CANONICAL_COORDINATE_SYSTEM),
            "matrix_convention": copy.deepcopy(COLLECTOR.CANONICAL_MATRIX_CONVENTION),
            "runtime_camera_receipt": {
                "schema": COLLECTOR.CAMERA_RECEIPT_SCHEMA,
                "source": "native_runtime_after_present",
                "scalar_storage": "float32",
                "absolute_tolerance": COLLECTOR.CAMERA_RECEIPT_TOLERANCE,
                "relative_tolerance": COLLECTOR.CAMERA_RECEIPT_TOLERANCE,
            },
        },
    }
    frames = []
    for index in range(sample_count):
        revision = 7
        frames.append(
            {
                "frame_index": index,
                "camera_revision": revision,
                "trace_frame_index": 0,
                "trace_timestamp_ns": 0,
                "trace_loop_index": 0,
                "camera_receipt": {
                    "schema": COLLECTOR.CAMERA_RECEIPT_SCHEMA,
                    "source": "native_runtime_after_present",
                    "camera_revision": revision,
                    "presented_camera_revision": revision,
                    "surface_width": 200,
                    "surface_height": 100,
                    "flags": 3,
                    "pose": {
                        "position": [COLLECTOR._f32(value) for value in position],
                        "rotation_xyzw": [COLLECTOR._f32(value) for value in rotation],
                    },
                    "intrinsics": {
                        key: COLLECTOR._f32(value) for key, value in intrinsics.items()
                    },
                    "view_matrix": matrices[0],
                    "projection_matrix": matrices[1],
                    "view_projection_matrix": matrices[2],
                },
            }
        )
    cpu_frames = sample_count if backend == "cpu" else 0
    gpu_frames = sample_count if backend == "gpu" else 0
    summary = {
        "sample_count": sample_count,
        "sort_telemetry": {
            "cpu_frame_count": cpu_frames,
            "gpu_frame_count": gpu_frames,
            "gpu_sort_fallback_count": 0,
        },
    }
    return manifest, summary, frames, expected_trace, expected_identity


def runtime_receipt(trace_frame, width: int, height: int, revision: int):
    position = [COLLECTOR._f32(value) for value in trace_frame["pose"]["position"]]
    rotation = [
        COLLECTOR._f32(value) for value in trace_frame["pose"]["rotation_xyzw"]
    ]
    intrinsics = {
        key: COLLECTOR._f32(value)
        for key, value in trace_frame["intrinsics"].items()
    }
    matrices = COLLECTOR._canonical_matrices_from_runtime_receipt(
        position, rotation, intrinsics, width / height
    )
    return {
        "schema": COLLECTOR.CAMERA_RECEIPT_SCHEMA,
        "source": "native_runtime_after_present",
        "camera_revision": revision,
        "presented_camera_revision": revision,
        "surface_width": width,
        "surface_height": height,
        "flags": 3,
        "pose": {"position": position, "rotation_xyzw": rotation},
        "intrinsics": intrinsics,
        "view_matrix": matrices[0],
        "projection_matrix": matrices[1],
        "view_projection_matrix": matrices[2],
    }


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
            geometry_path="direct",
            camera_trace=TEST_CAMERA_TRACE,
            camera_frame=None,
            camera_frame_indices="0,1",
        )
        launch = COLLECTOR.benchmark_launch_args(args, "gpu")
        self.assertIn("gsplat_surface_order_backend", launch)
        self.assertEqual(launch[launch.index("gsplat_surface_order_backend") + 1], "gpu")
        self.assertIn("gsplat_geometry_path", launch)
        self.assertIn("direct", launch)
        self.assertEqual(launch[launch.index("gsplat_benchmark_frames") + 1], "240")

    def test_launch_arguments_support_static_view_one(self) -> None:
        args = COLLECTOR.parser().parse_args(
            [
                "--serial",
                "serial",
                "--ply",
                __file__,
                "--camera-trace",
                str(TEST_CAMERA_TRACE),
                "--camera-frame",
                "1",
            ]
        )
        COLLECTOR.validate_args(args)
        launch = COLLECTOR.benchmark_launch_args(args, "cpu")
        self.assertEqual(launch[launch.index("gsplat_camera_trace_frame") + 1], "1")
        self.assertNotIn("gsplat_camera_trace_sequence", launch)

    def test_forced_backend_and_dataset_identity_are_validated(self) -> None:
        manifest, summary, frames, trace, trace_identity = camera_validation_fixture(
            "gpu", 8
        )
        COLLECTOR.validate_run_artifact(
            manifest,
            summary,
            frames,
            "gpu",
            "packed",
            {"sha256": "abc", "bytes": 123},
            trace,
            trace_identity,
        )

        summary["sort_telemetry"]["cpu_frame_count"] = 1
        summary["sort_telemetry"]["gpu_frame_count"] = 7
        with self.assertRaisesRegex(RuntimeError, "contains cpu or fallback"):
            COLLECTOR.validate_run_artifact(
                manifest,
                summary,
                frames,
                "gpu",
                "packed",
                {"sha256": "abc", "bytes": 123},
                trace,
                trace_identity,
            )

    def test_artifact_rejects_wrong_packaged_dataset(self) -> None:
        manifest, summary, frames, trace, trace_identity = camera_validation_fixture(
            "cpu"
        )
        manifest["dataset"]["sha256"] = "wrong"
        with self.assertRaisesRegex(RuntimeError, "dataset sha256"):
            COLLECTOR.validate_run_artifact(
                manifest,
                summary,
                frames,
                "cpu",
                "packed",
                {"sha256": "abc", "bytes": 123},
                trace,
                trace_identity,
            )

    def test_camera_receipt_mutations_fail_closed(self) -> None:
        fixture = camera_validation_fixture("gpu")
        COLLECTOR.validate_run_artifact(
            fixture[0],
            fixture[1],
            fixture[2],
            "gpu",
            "packed",
            {"sha256": "abc", "bytes": 123},
            fixture[3],
            fixture[4],
        )

        mutations = {
            "missing receipt": lambda manifest, frames: frames[0].pop("camera_receipt"),
            "wrong trace index": lambda manifest, frames: frames[0].__setitem__(
                "trace_frame_index", 1
            ),
            "wrong revision": lambda manifest, frames: frames[0][
                "camera_receipt"
            ].__setitem__("camera_revision", 8),
            "mutated pose": lambda manifest, frames: frames[0]["camera_receipt"][
                "pose"
            ]["position"].__setitem__(0, 9.0),
            "mutated matrix": lambda manifest, frames: frames[0]["camera_receipt"][
                "view_matrix"
            ].__setitem__(0, 0.25),
            "wrong trace file hash": lambda manifest, frames: manifest["trace"].__setitem__(
                "file_sha256", "f" * 64
            ),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                manifest = copy.deepcopy(fixture[0])
                frames = copy.deepcopy(fixture[2])
                mutate(manifest, frames)
                with self.assertRaises(RuntimeError):
                    COLLECTOR.validate_run_artifact(
                        manifest,
                        fixture[1],
                        frames,
                        "gpu",
                        "packed",
                        {"sha256": "abc", "bytes": 123},
                        fixture[3],
                        fixture[4],
                    )

    def test_fixed_view_one_and_two_view_sequence_are_auditable(self) -> None:
        trace_path = (
            COLLECTOR.REPO_ROOT
            / "tests/perf/trace/fixtures/quality/"
            "candidate-truck-quality-2412x1080-v1.json"
        )
        trace = __import__("json").loads(trace_path.read_text())
        identity = COLLECTOR.local_file_identity(trace_path)
        width = trace["display"]["width"]
        height = trace["display"]["height"]
        manifest, _, _, _, _ = camera_validation_fixture("gpu")
        manifest["trace"].update(
            {
                "id": trace["trace_id"],
                "sha256": trace["content_sha256"],
                "file_sha256": identity["sha256"],
                "reference_width": width,
                "reference_height": height,
            }
        )

        fixed_manifest = copy.deepcopy(manifest)
        fixed_manifest["trace"].update(
            {
                "playback_mode": "fixed",
                "measured_frames_per_loop": 1,
                "loops": 1,
                "frame_index": 1,
            }
        )
        fixed_frame = {
            "frame_index": 0,
            "camera_revision": 11,
            "trace_frame_index": 1,
            "trace_timestamp_ns": trace["frames"][1]["timestamp_ns"],
            "trace_loop_index": 0,
            "camera_receipt": runtime_receipt(
                trace["frames"][1], width, height, 11
            ),
        }
        COLLECTOR.validate_camera_receipts(
            fixed_manifest, [fixed_frame], trace, identity
        )

        sequence_manifest = copy.deepcopy(manifest)
        sequence_manifest["trace"].pop("frame_index", None)
        sequence_manifest["trace"].update(
            {
                "playback_mode": "sequence",
                "measured_frames_per_loop": 2,
                "loops": 2,
                "frame_indices": [0, 1],
            }
        )
        sequence_frames = []
        for sample_index, trace_index in enumerate((0, 1, 0, 1)):
            revision = 20 + sample_index
            sequence_frames.append(
                {
                    "frame_index": sample_index,
                    "camera_revision": revision,
                    "trace_frame_index": trace_index,
                    "trace_timestamp_ns": trace["frames"][trace_index]["timestamp_ns"],
                    "trace_loop_index": sample_index // 2,
                    "camera_receipt": runtime_receipt(
                        trace["frames"][trace_index], width, height, revision
                    ),
                }
            )
        COLLECTOR.validate_camera_receipts(
            sequence_manifest, sequence_frames, trace, identity
        )

    def test_artifact_frame_extraction_is_strict(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "frames.jsonl"
            path.write_text('{"frame_index":0}\n{"frame_index":1}\n')
            self.assertEqual(
                [frame["frame_index"] for frame in COLLECTOR.read_artifact_frames(path)],
                [0, 1],
            )
            path.write_text('{"frame_index":0}\nnot-json\n')
            with self.assertRaisesRegex(RuntimeError, "invalid JSON"):
                COLLECTOR.read_artifact_frames(path)

    def test_camera_validator_cli_rejects_mutated_extracted_frame(self) -> None:
        manifest, _, frames, trace, _ = camera_validation_fixture("gpu")
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = root / "artifact"
            artifact.mkdir()
            trace_path = root / "trace.json"
            trace_path.write_text(json.dumps(trace, sort_keys=True))
            manifest["trace"]["file_sha256"] = COLLECTOR.sha256_file(trace_path)
            (artifact / "manifest.json").write_text(json.dumps(manifest))
            frames_path = artifact / "frames.jsonl"
            frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")

            validator = COLLECTOR.CAMERA_RECEIPT_VALIDATOR
            subprocess.run(
                [sys.executable, str(validator), str(artifact), str(trace_path)],
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            )
            frames[0]["camera_receipt"]["projection_matrix"][0] += 1.0
            frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
            rejected = subprocess.run(
                [sys.executable, str(validator), str(artifact), str(trace_path)],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("projection_matrix", rejected.stdout)


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
                "--camera-trace",
                str(TEST_CAMERA_TRACE),
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
                        "--camera-trace",
                        str(TEST_CAMERA_TRACE),
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
                        "--camera-trace",
                        str(TEST_CAMERA_TRACE),
                        "--output",
                        str(output),
                        "--dry-run",
                    ]
                )
        plan = stdout.getvalue()
        self.assertEqual(result, 0)
        self.assertIn("apk_mode=reuse-exact-installed", plan)
        self.assertNotIn(" install -r ", plan)
        self.assertEqual(plan.count(" push "), 2)
        self.assertEqual(plan.count(" cp "), 4)
        self.assertEqual(plan.count("files/imported_scene.ply"), 6)
        self.assertEqual(plan.count(" rm -f "), 2)

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
                        "--camera-trace",
                        str(TEST_CAMERA_TRACE),
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
        self.assertEqual(plan.count(" push "), 2)

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
