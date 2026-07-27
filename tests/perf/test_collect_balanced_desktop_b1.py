#!/usr/bin/env python3
"""Synthetic fail-closed tests for the desktop B1 artifact collector."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


COLLECTOR_PATH = Path(__file__).with_name("collect-balanced-desktop-b1.py")
SPEC = importlib.util.spec_from_file_location("collect_balanced_desktop_b1", COLLECTOR_PATH)
assert SPEC is not None and SPEC.loader is not None
collector = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = collector
SPEC.loader.exec_module(collector)


SHA = "a" * 64
DATASET = {
    "id": "garden",
    "sha256": "b" * 64,
    "bytes": 123,
    "splat_count": 279_199,
    "sh_degree": 3,
}


def terminal_record(index: int, lane: object = collector.LANES[0]) -> dict[str, str]:
    trace_frame = collector.CAPTURE_TRACE_FRAMES[index]
    trace_timestamp = collector.CAPTURE_TRACE_TIMESTAMPS_NS[index]
    presentation = 201 + index
    return {
        "__stream": "stdout",
        "status": "ok",
        "capture_index": str(index),
        "path": collector.expected_capture_path(index, trace_frame).as_posix(),
        "trace_frame": str(trace_frame),
        "trace_timestamp_ns": str(trace_timestamp),
        "elapsed_ns": str((index + 1) * 10),
        "call_ms": "1.25",
        "frame_wall_ms": "2.5",
        "current_stats_ticket": str(101 + index),
        "current_stats_scene_generation": "1",
        "current_stats_camera_revision": str(index + 1),
        "current_stats_viewport_generation": "2",
        "current_stats_contract_generation": "3",
        "current_stats_plan_set_generation": "4",
        "current_stats_plan_id": "gpu_post_sort",
        "current_stats_order_generation": str(10 + index),
        "current_stats_raster_generation": str(20 + index),
        "current_stats_encode_attempt": str(30 + index),
        "current_stats_presentation_sequence": str(presentation),
        "count_semantics": "indirect_draw_equals_visible",
        "source_count": str(DATASET["splat_count"]),
        "visible_count": "100",
        "contributor_count": "90",
        "drawn_count": "100",
        "exact_contributor_compaction": "false",
        "capture_receipt_depth_precision_profile": lane.profile,
        "capture_receipt_projected_cache_precision_profile": "ExactAxes32",
        "capture_receipt_projected_axis_record_bytes": "16",
        "capture_receipt_resident_sh_codec_profile": "ExactSigned11BandScale5",
        "capture_receipt_resident_sh_mantissa_bits": "11",
        "capture_receipt_resident_sh_symmetric_max_code": "1023",
        "capture_receipt_resident_sh_point_scale_bits": "5",
        "capture_receipt_resident_sh_point_scale_max_code": "31",
        "capture_receipt_resident_sh_range_chunk_splats": "256",
        "capture_receipt_resident_sh_source_count": str(DATASET["splat_count"]),
        "capture_receipt_resident_sh_encoded_count": str(DATASET["splat_count"]),
        "capture_receipt_resident_sh_resident_count": str(DATASET["splat_count"]),
        "capture_receipt_resident_sh_addressable_count": str(DATASET["splat_count"]),
        "capture_receipt_resident_sh_source_degree": "3",
        "capture_receipt_resident_sh_resident_degree": "3",
        "capture_receipt_resident_sh_residual_coefficients_per_source": "45",
        "capture_receipt_resident_sh_plane_count": "4",
        "capture_receipt_resident_sh_bytes_per_source": "64",
        "capture_receipt_scene_generation": "1",
        "capture_receipt_camera_revision": str(index + 1),
        "capture_receipt_viewport_generation": "2",
        "capture_receipt_contract_generation": "3",
        "capture_receipt_plan_set_generation": "4",
        "capture_receipt_plan_id": "GpuPostSort",
        "capture_receipt_order_generation": str(10 + index),
        "capture_receipt_presentation_sequence": str(presentation),
        "capture_receipt_width": "1920",
        "capture_receipt_height": "1080",
        "capture_receipt_rgba8_sha256": SHA,
        "frame_presented": "true",
        "terminal_receipt": "ready",
    }


def valid_records(lane: object = collector.LANES[0]) -> list[dict[str, str]]:
    return [terminal_record(index, lane) for index in range(3)]


def capture(index: int, lane: object, root: Path) -> object:
    presentation = {
        "ticket": 101 + index,
        "outcome": "presented",
        "scene_generation": 1,
        "camera_generation": index + 1,
        "viewport_generation": 2,
        "contract_generation": 3,
        "plan_generation": 4,
        "presentation_generation": 201 + index,
    }
    receipt_identity = {
        "scene_generation": 1,
        "camera_revision": index + 1,
        "viewport_generation": 2,
        "contract_generation": 3,
        "plan_set_generation": 4,
        "plan_id": "GpuPostSort",
        "order_generation": 10 + index,
        "presentation_sequence": 201 + index,
        "width": 1920,
        "height": 1080,
        "rgba8_sha256": SHA,
    }
    return collector.Capture(
        capture_index=index,
        trace_frame_index=collector.CAPTURE_TRACE_FRAMES[index],
        trace_timestamp_ns=collector.CAPTURE_TRACE_TIMESTAMPS_NS[index],
        elapsed_ns=(index + 1) * 10,
        call_ms=1.25,
        frame_wall_ms=2.5,
        path=root / f"capture-{index}.png",
        png_sha256=SHA,
        rgba8_sha256=SHA,
        counts={
            "source": DATASET["splat_count"],
            "visible": 100,
            "contributor": 90,
            "drawn": 100,
        },
        presentation=presentation,
        depth_precision={**receipt_identity, "profile": lane.profile},
        projected_cache_precision={
            **receipt_identity,
            "profile": "ExactAxes32",
            "axis_record_bytes": 16,
        },
        resident_sh={
            **receipt_identity,
            "codec_profile": "ExactSigned11BandScale5",
            "mantissa_bits": 11,
            "symmetric_max_code": 1023,
            "point_scale_bits": 5,
            "point_scale_max_code": 31,
            "range_chunk_splats": 256,
            "source_count": DATASET["splat_count"],
            "encoded_count": DATASET["splat_count"],
            "resident_count": DATASET["splat_count"],
            "addressable_count": DATASET["splat_count"],
            "source_sh_degree": 3,
            "resident_sh_degree": 3,
            "residual_coefficients_per_source": 45,
            "plane_count": 4,
            "bytes_per_source": 64,
        },
        raster_generation=20 + index,
        encode_attempt=30 + index,
    )


def lane_session(lane: object, root: Path) -> object:
    return collector.LaneSession(
        lane=lane,
        captures=tuple(capture(index, lane, root) for index in range(3)),
        adapter={
            "backend": "metal",
            "name": "synthetic adapter",
            "device_type": "integrated_gpu",
            "driver": "synthetic",
            "driver_info": "synthetic",
        },
        started_at_utc="2026-07-27T00:00:00Z",
        ended_at_utc="2026-07-27T00:00:01Z",
    )


def write_black_rgba_png(path: Path) -> tuple[str, str]:
    width, height = collector.FORMAL_SIZE
    rgba = b"\0" * (width * height * 4)
    filtered = b"".join(b"\0" + rgba[row * width * 4 : (row + 1) * width * 4] for row in range(height))

    def chunk(kind: bytes, value: bytes) -> bytes:
        return struct.pack(">I", len(value)) + kind + value + struct.pack(">I", zlib.crc32(kind + value))

    data = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(filtered))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(data)
    return hashlib.sha256(data).hexdigest(), hashlib.sha256(rgba).hexdigest()


class CollectorRecordTests(unittest.TestCase):
    def test_candidate20_is_a_separate_single_variable_experiment(self) -> None:
        experiment = collector.EXPERIMENTS["b1-20"]
        exact, candidate = experiment.lanes
        self.assertEqual(experiment.name, "b1-depth-key-candidate20")
        self.assertEqual(experiment.changed_receipt, "depth_precision")
        self.assertEqual(exact.profile, "ExactFull32")
        self.assertEqual(candidate.profile, "CandidateStable20")
        self.assertEqual(
            candidate.cargo_feature, "diagnostic-surface-depth-key-candidate20"
        )

    def test_malformed_terminal_record_is_rejected(self) -> None:
        line = collector.PREFIXES["terminal"] + "status=ok malformed\n"
        with self.assertRaisesRegex(collector.ValidationError, "non key=value"):
            collector.parse_session_records(line, "")

    def test_missing_and_duplicate_capture_are_rejected(self) -> None:
        with self.subTest("missing"):
            with self.assertRaisesRegex(collector.ValidationError, "exactly three"):
                collector.validate_terminal_records(
                    valid_records()[:2],
                    lane=collector.LANES[0],
                    source_count=DATASET["splat_count"],
                )
        with self.subTest("duplicate"):
            records = valid_records()
            records[2]["capture_index"] = "1"
            with self.assertRaisesRegex(collector.ValidationError, "capture_index"):
                collector.validate_terminal_records(
                    records,
                    lane=collector.LANES[0],
                    source_count=DATASET["splat_count"],
                )

    def test_mismatched_lane_and_receipt_are_rejected(self) -> None:
        with self.subTest("lane"):
            records = valid_records()
            records[1]["capture_receipt_depth_precision_profile"] = "CandidateStable24"
            with self.assertRaisesRegex(
                collector.ValidationError, "capture_receipt_depth_precision_profile"
            ):
                collector.validate_terminal_records(
                    records,
                    lane=collector.LANES[0],
                    source_count=DATASET["splat_count"],
                )

    def test_projected_cache_and_resident_sh_fields_are_required_actual_receipts(self) -> None:
        for field in (
            "capture_receipt_projected_cache_precision_profile",
            "capture_receipt_projected_axis_record_bytes",
            "capture_receipt_resident_sh_codec_profile",
            "capture_receipt_resident_sh_source_count",
            "capture_receipt_resident_sh_residual_coefficients_per_source",
            "capture_receipt_resident_sh_bytes_per_source",
        ):
            with self.subTest(missing=field):
                records = valid_records()
                del records[1][field]
                with self.assertRaisesRegex(collector.ValidationError, field):
                    collector.validate_terminal_records(
                        records,
                        lane=collector.LANES[0],
                        source_count=DATASET["splat_count"],
                    )

        for field, value in (
            ("capture_receipt_projected_cache_precision_profile", "CandidateAxes16"),
            ("capture_receipt_resident_sh_codec_profile", "CandidateSigned8BandScale5"),
            ("capture_receipt_resident_sh_source_count", "1"),
        ):
            with self.subTest(mismatched=field):
                records = valid_records()
                records[1][field] = value
                with self.assertRaisesRegex(collector.ValidationError, field):
                    collector.validate_terminal_records(
                        records,
                        lane=collector.LANES[0],
                        source_count=DATASET["splat_count"],
                    )
        with self.subTest("receipt"):
            records = valid_records()
            records[1]["capture_receipt_camera_revision"] = "99"
            with self.assertRaisesRegex(collector.ValidationError, "identity mismatch"):
                collector.validate_terminal_records(
                    records,
                    lane=collector.LANES[0],
                    source_count=DATASET["splat_count"],
                )

    def test_viewport_generation_zero_is_valid_but_remains_strict(self) -> None:
        records = valid_records()
        for record in records:
            record["current_stats_viewport_generation"] = "0"
            record["capture_receipt_viewport_generation"] = "0"
        normalized = collector.validate_terminal_records(
            records,
            lane=collector.LANES[0],
            source_count=DATASET["splat_count"],
        )
        self.assertEqual(
            [value["current"]["viewport_generation"] for value in normalized],
            [0, 0, 0],
        )

        for field in (
            "current_stats_viewport_generation",
            "capture_receipt_viewport_generation",
        ):
            with self.subTest(non_integer=field):
                malformed = valid_records()
                malformed[1][field] = "not-an-integer"
                with self.assertRaisesRegex(collector.ValidationError, "must be an integer"):
                    collector.validate_terminal_records(
                        malformed,
                        lane=collector.LANES[0],
                        source_count=DATASET["splat_count"],
                    )

        mismatch = valid_records()
        mismatch[1]["current_stats_viewport_generation"] = "0"
        mismatch[1]["capture_receipt_viewport_generation"] = "1"
        with self.assertRaisesRegex(collector.ValidationError, "identity mismatch"):
            collector.validate_terminal_records(
                mismatch,
                lane=collector.LANES[0],
                source_count=DATASET["splat_count"],
            )

    def test_invalid_timing_is_rejected(self) -> None:
        with self.subTest("non-finite"):
            records = valid_records()
            records[0]["call_ms"] = "nan"
            with self.assertRaisesRegex(collector.ValidationError, "finite"):
                collector.validate_terminal_records(
                    records,
                    lane=collector.LANES[0],
                    source_count=DATASET["splat_count"],
                )
        with self.subTest("out-of-order"):
            records = valid_records()
            records[1]["elapsed_ns"] = records[0]["elapsed_ns"]
            with self.assertRaisesRegex(collector.ValidationError, "strictly increasing"):
                collector.validate_terminal_records(
                    records,
                    lane=collector.LANES[0],
                    source_count=DATASET["splat_count"],
                )


class CollectorProcessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.stage = self.root / "stage"
        self.stage.mkdir()
        self.binaries: dict[str, Path] = {}
        self.build_receipts: dict[str, dict[str, str]] = {}
        for lane in collector.LANES:
            binary = self.root / f"desktop-{lane.name}"
            binary.write_bytes(lane.name.encode("ascii"))
            binary.chmod(0o755)
            self.binaries[lane.name] = binary
            self.build_receipts[lane.name] = {
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()
            }

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def call_sessions(self, invoke: object) -> dict[str, object]:
        def validate_session(_stdout: str, _stderr: str, **kwargs: object) -> object:
            return lane_session(kwargs["lane"], self.root)

        return collector.run_host_sessions(
            stage=self.stage,
            binaries=self.binaries,
            build_receipts=self.build_receipts,
            dataset=DATASET,
            dataset_path=self.root / "garden.ply",
            trace={"trace_id": "moving", "content_sha256": "c" * 64},
            trace_path=self.root / "trace.json",
            warmup=20,
            measured=80,
            invoke=invoke,
            validate_session=validate_session,
        )

    def test_exactly_two_continuous_multi_capture_host_invocations(self) -> None:
        calls: list[list[str]] = []

        def invoke(command: object, _cwd: Path) -> subprocess.CompletedProcess[str]:
            calls.append(list(command))
            return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

        sessions = self.call_sessions(invoke)
        self.assertEqual(set(sessions), {"exact", "candidate"})
        self.assertEqual(len(calls), 2)
        for command in calls:
            self.assertEqual(command.count("--surface-diagnostic-multi-capture"), 1)
            self.assertEqual(command.count("--surface-diagnostic-capture-receipt"), 1)
            self.assertEqual(command.count("--surface-evidence-plan"), 1)
            self.assertIn("gpu-post-sort", command)

    def test_partial_host_failure_is_fail_closed(self) -> None:
        calls: list[list[str]] = []

        def invoke(command: object, _cwd: Path) -> subprocess.CompletedProcess[str]:
            calls.append(list(command))
            returncode = 0 if len(calls) == 1 else 9
            return subprocess.CompletedProcess(command, returncode, stdout="", stderr="failed")

        with self.assertRaisesRegex(collector.ValidationError, "candidate host exited with 9"):
            self.call_sessions(invoke)
        self.assertEqual(len(calls), 2)
        self.assertFalse((self.stage / "runs").exists())
        self.assertFalse((self.stage / "suite.json").exists())

    def test_publish_rejects_incomplete_stage_without_output(self) -> None:
        output = self.root / "formal-suite"
        with self.assertRaisesRegex(collector.ValidationError, "suite manifest"):
            collector.publish_suite(self.stage, output)
        self.assertFalse(output.exists())


class CollectorOrchestrationTests(unittest.TestCase):
    def test_materialized_run_passes_standard_validator(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.png"
            png_sha256, rgba_sha256 = write_black_rgba_png(source)
            lane = collector.LANES[0]
            value = capture(0, lane, root)
            value = collector.Capture(
                **{
                    **value.__dict__,
                    "path": source,
                    "png_sha256": png_sha256,
                    "rgba8_sha256": rgba_sha256,
                }
            )
            directory = root / "artifact"
            collector.build_run_artifact(
                directory,
                lane_session=lane_session(lane, root),
                capture=value,
                pair_id="synthetic-pair",
                dataset=DATASET,
                trace={"trace_id": "moving", "content_sha256": "c" * 64, "file_sha256": "e" * 64},
                build={
                    "git": {
                        "commit": "d" * 40,
                        "dirty": False,
                        "status_porcelain_sha256": hashlib.sha256(b"").hexdigest(),
                    },
                    "lanes": {lane.name: {"binary_sha256": "f" * 64}},
                },
                package="0.1.0",
                refresh_hz=60.0,
                device=None,
                camera={
                    "trace_id": "moving",
                    "trace_content_sha256": "c" * 64,
                    "pose_intrinsics_sha256": "9" * 64,
                },
                warmup=20,
            )
            completed = subprocess.run(
                [sys.executable, str(collector.BENCHMARK_VALIDATOR_PATH), str(directory)],
                cwd=collector.REPO_ROOT,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_complete_synthetic_suite_passes_balanced_validator(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / "stage"
            stage.mkdir()
            source = root / "source.png"
            png_sha256, rgba_sha256 = write_black_rgba_png(source)
            dataset_manifest = collector.REPO_ROOT / "tests/perf/datasets/kitsune.json"
            dataset = json.loads(dataset_manifest.read_text(encoding="utf-8"))
            trace_file = (
                collector.REPO_ROOT
                / "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-1920x1080-v1.json"
            )
            trace_value = json.loads(trace_file.read_text(encoding="utf-8"))
            sessions: dict[str, object] = {}
            for lane in collector.LANES:
                captures = []
                for index in range(3):
                    presentation = {
                        "ticket": 101 + index,
                        "outcome": "presented",
                        "scene_generation": 1,
                        "camera_generation": index + 1,
                        "viewport_generation": 2,
                        "contract_generation": 3,
                        "plan_generation": 4,
                        "presentation_generation": 201 + index,
                    }
                    depth_precision = {
                        "profile": lane.profile,
                        "scene_generation": 1,
                        "camera_revision": index + 1,
                        "viewport_generation": 2,
                        "contract_generation": 3,
                        "plan_set_generation": 4,
                        "plan_id": "GpuPostSort",
                        "order_generation": 10 + index,
                        "presentation_sequence": 201 + index,
                        "width": 1920,
                        "height": 1080,
                        "rgba8_sha256": rgba_sha256,
                    }
                    receipt_identity = {
                        key: value
                        for key, value in depth_precision.items()
                        if key != "profile"
                    }
                    projected_cache_precision = {
                        **receipt_identity,
                        "profile": "ExactAxes32",
                        "axis_record_bytes": 16,
                    }
                    resident_sh = {
                        **receipt_identity,
                        "codec_profile": "ExactSigned11BandScale5",
                        "mantissa_bits": 11,
                        "symmetric_max_code": 1023,
                        "point_scale_bits": 5,
                        "point_scale_max_code": 31,
                        "range_chunk_splats": 256,
                        "source_count": dataset["splat_count"],
                        "encoded_count": dataset["splat_count"],
                        "resident_count": dataset["splat_count"],
                        "addressable_count": dataset["splat_count"],
                        "source_sh_degree": dataset["sh_degree"],
                        "resident_sh_degree": dataset["sh_degree"],
                        "residual_coefficients_per_source": 45,
                        "plane_count": 4,
                        "bytes_per_source": 64,
                    }
                    captures.append(
                        collector.Capture(
                            capture_index=index,
                            trace_frame_index=collector.CAPTURE_TRACE_FRAMES[index],
                            trace_timestamp_ns=collector.CAPTURE_TRACE_TIMESTAMPS_NS[index],
                            elapsed_ns=(index + 1) * 10,
                            call_ms=1.25,
                            frame_wall_ms=2.5,
                            path=source,
                            png_sha256=png_sha256,
                            rgba8_sha256=rgba_sha256,
                            counts={
                                "source": dataset["splat_count"],
                                "visible": 100,
                                "contributor": 90,
                                "drawn": 100,
                            },
                            presentation=presentation,
                            depth_precision=depth_precision,
                            projected_cache_precision=projected_cache_precision,
                            resident_sh=resident_sh,
                            raster_generation=20 + index,
                            encode_attempt=30 + index,
                        )
                    )
                sessions[lane.name] = collector.LaneSession(
                    lane=lane,
                    captures=tuple(captures),
                    adapter={
                        "backend": "metal",
                        "name": "synthetic adapter",
                        "device_type": "integrated_gpu",
                        "driver": "synthetic",
                        "driver_info": "synthetic",
                    },
                    started_at_utc="2026-07-27T00:00:00Z",
                    ended_at_utc="2026-07-27T00:00:01Z",
                )
            trace = {
                "trace_id": trace_value["trace_id"],
                "content_sha256": trace_value["content_sha256"],
                "file_sha256": hashlib.sha256(trace_file.read_bytes()).hexdigest(),
                "value": trace_value,
            }
            build = {
                "git": {
                    "commit": "d" * 40,
                    "dirty": False,
                    "status_porcelain_sha256": hashlib.sha256(b"").hexdigest(),
                },
                "lanes": {
                    lane.name: {"binary_sha256": ("f" if lane.name == "exact" else "8") * 64}
                    for lane in collector.LANES
                },
                "package_version": "0.1.0",
                "dataset_manifest_path": "tests/perf/datasets/kitsune.json",
                "dataset_manifest_sha256": hashlib.sha256(dataset_manifest.read_bytes()).hexdigest(),
                "trace_path": trace_file.relative_to(collector.REPO_ROOT).as_posix(),
            }
            with mock.patch.object(collector, "host_device_name", return_value=None):
                suite = collector.materialize_suite(
                    stage=stage,
                    repo=collector.REPO_ROOT,
                    sessions=sessions,
                    dataset=dataset,
                    trace=trace,
                    build=build,
                    refresh_hz=60.0,
                    warmup=20,
                )
            self.assertEqual(suite["schema"], collector.SUITE_SCHEMA)
            self.assertEqual(len(suite["frames"]), 3)
            self.assertTrue((stage / "validators/balanced.stdout.log").is_file())

    def test_collection_failure_does_not_publish_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "formal-suite"
            dataset_manifest = root / "dataset.json"
            dataset_path = root / "garden.ply"
            trace_path = root / "trace.json"
            dataset_manifest.write_text("{}\n", encoding="utf-8")
            dataset_path.write_bytes(b"dataset")
            trace_value = {"frames": [{"pose": {}, "intrinsics": {}}]}
            trace_path.write_text(json.dumps(trace_value), encoding="utf-8")
            trace = {
                "path": str(trace_path),
                "trace_id": "moving",
                "content_sha256": "c" * 64,
                "file_sha256": hashlib.sha256(trace_path.read_bytes()).hexdigest(),
            }
            m2b = SimpleNamespace(
                read_dataset_manifest=lambda _repo, _path: (DATASET, dataset_path),
                read_trace=lambda _repo, _path: trace,
            )
            args = SimpleNamespace(
                output=output,
                dataset_manifest=dataset_manifest,
                trace=trace_path,
                warmup=20,
                measured=80,
                refresh_hz=60.0,
            )
            clean_git = {
                "commit": "d" * 40,
                "dirty": False,
                "status_porcelain_sha256": hashlib.sha256(b"").hexdigest(),
            }

            def fail_hosts(**_kwargs: object) -> object:
                raise collector.ValidationError("synthetic partial host failure")

            with (
                mock.patch.object(collector, "validate_ignored_output"),
                mock.patch.object(collector, "git_receipt", return_value=clean_git),
                mock.patch.object(collector, "M2B", m2b),
                mock.patch.object(collector, "package_version", return_value="0.1.0"),
                mock.patch.object(
                    collector,
                    "build_desktop_binaries",
                    return_value=({}, {}),
                ),
                mock.patch.object(collector, "run_host_sessions", side_effect=fail_hosts),
            ):
                with self.assertRaisesRegex(collector.ValidationError, "synthetic partial host failure"):
                    collector.collect(args, root)

            self.assertFalse(output.exists())
            failures = list(root.glob("formal-suite.failed-*"))
            self.assertEqual(len(failures), 1)
            failure = json.loads((failures[0] / "failure.json").read_text(encoding="utf-8"))
            self.assertFalse(failure["output_published"])
            self.assertFalse((failures[0] / "runs").exists())


if __name__ == "__main__":
    unittest.main()
