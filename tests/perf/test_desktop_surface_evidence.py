#!/usr/bin/env python3
"""Focused fail-closed tests for the M2b desktop Surface collector."""

from __future__ import annotations

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
from types import SimpleNamespace
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tests/perf/collect-desktop-surface-evidence.py"
SPEC = importlib.util.spec_from_file_location("desktop_surface_evidence", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)

DATASET = {
    "id": "kitsune",
    "sha256": "a" * 64,
    "bytes": 123,
    "splat_count": 100,
    "sh_degree": 3,
}
TRACE = {
    "trace_id": "kitsune-1080p",
    "content_sha256": "b" * 64,
    "file_sha256": "c" * 64,
    "width": 1920,
    "height": 1080,
    "frame_indices": (0, 1),
    "frame_timestamps_ns": (0, 100),
}
WARMUP = 1
MEASURED = 2


def line(prefix: str, fields: dict[str, str]) -> str:
    return prefix + " ".join(f"{key}={value}" for key, value in fields.items())


def actuals(arm: str) -> list[str]:
    if arm == "adaptive":
        return ["cpu_post_sort", "gpu_post_sort", "gpu_preproject", "gpu_post_sort"]
    return [arm] * 4


def semantics(plan: str) -> tuple[str, bool, int, int, str]:
    if plan == "cpu_post_sort":
        return "direct_draw_equals_visible", False, 80, 80, "cpu"
    if plan == "gpu_post_sort":
        return "indirect_draw_equals_visible", False, 80, 80, "gpu"
    return "indirect_draw_equals_contributor", True, 70, 70, "gpu"


def synthetic_records(arm: str) -> dict[str, list[dict[str, str]]]:
    plans = actuals(arm)
    begin = {
        "trace_id": TRACE["trace_id"],
        "trace_sha256": TRACE["content_sha256"],
        "exact_plan_requested": arm,
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "blend_mode": "sorted_alpha",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "source_count": "100",
        "decoded_count": "100",
        "encoded_count": "100",
        "resident_count": "100",
        "addressable_count": "100",
        "sh_degree": "3",
        "requested_width": "1920",
        "requested_height": "1080",
        "surface_width": "1920",
        "surface_height": "1080",
        "internal_render_width": "1920",
        "internal_render_height": "1080",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
        "trace_frames": "3",
        "adapter_backend": "metal",
        "adapter_name": "Apple-M4",
        "adapter_device_type": "integrated_gpu",
        "adapter_driver": "unavailable",
        "adapter_driver_info": "unavailable",
    }
    frames: list[dict[str, str]] = []
    for index in range(3):
        plan = plans[index]
        count_semantics, compacted, contributor, drawn, backend = semantics(plan)
        phase_frame_index = index if index == 0 else index - 1
        frames.append(
            {
                "trace_id": TRACE["trace_id"],
                "trace_sha256": TRACE["content_sha256"],
                "playback_index": str(index),
                "phase": "warmup" if index == 0 else "measure",
                "measured_sample": "none" if index == 0 else str(index - 1),
                "trace_frame": str(phase_frame_index % 2),
                "trace_timestamp_ns": str((phase_frame_index % 2) * 100),
                "elapsed_ns": str(1_000 + index),
                "exact_plan_requested": arm,
                "exact_plan_actual": plan,
                "current_stats_ticket": str(10 + index),
                "scene_generation": "1",
                "camera_revision": str(index),
                "viewport_generation": "1",
                "contract_generation": "1",
                "plan_set_generation": "1",
                "order_generation": str(20 + index),
                "raster_generation": "1",
                "encode_attempt": str(30 + index),
                "presentation_sequence": str(40 + index),
                "count_semantics": count_semantics,
                "source_count": "100",
                "visible_count": "80",
                "contributor_count": str(contributor),
                "drawn_count": str(drawn),
                "exact_contributor_compaction": str(compacted).lower(),
                "sort_refreshed": "true",
                "actual_backend": backend,
                "call_ms": f"{1.0 + index:.6f}",
                "frame_wall_ms": f"{1.5 + index:.6f}",
                "requested_width": "1920",
                "requested_height": "1080",
                "presented_width": "1920",
                "presented_height": "1080",
                "frame_presented": "true",
                "terminal_receipt": "ready",
            }
        )
    capture_plan = plans[3]
    count_semantics, compacted, contributor, drawn, backend = semantics(capture_plan)
    capture = {
        "status": "ok",
        "path": COLLECTOR.FINAL_CAPTURE_IDENTITY,
        "trace_frame": frames[-1]["trace_frame"],
        "exact_plan_requested": arm,
        "exact_plan_actual": capture_plan,
        "current_stats_ticket": "99",
        "scene_generation": "1",
        "camera_revision": "3",
        "viewport_generation": "0",
        "contract_generation": "1",
        "plan_set_generation": "2",
        "order_generation": "99",
        "raster_generation": "99",
        "encode_attempt": "99",
        "presentation_sequence": "99",
        "count_semantics": count_semantics,
        "source_count": "100",
        "visible_count": "80",
        "contributor_count": str(contributor),
        "drawn_count": str(drawn),
        "exact_contributor_compaction": str(compacted).lower(),
        "actual_backend": backend,
        "requested_width": "1920",
        "requested_height": "1080",
        "captured_width": "1920",
        "captured_height": "1080",
        "frame_presented": "true",
        "terminal_receipt": "ready",
    }
    plan_set = sorted(set(plans))
    summary = {
        "status": "ok",
        "exact_plan_requested": arm,
        "actual_plan_set": ",".join(plan_set),
        "trace_frames": "3",
        "measured_frames": "2",
        "eligibility_retries": "0",
        "capture_retries": "0",
        "terminal_receipts": "4",
        "final_capture": "available",
    }
    return {"begin": [begin], "frame": frames, "capture": [capture], "summary": [summary]}


def render(records: dict[str, list[dict[str, str]]]) -> str:
    output: list[str] = []
    for name in ("begin", "frame", "capture", "summary"):
        prefix = COLLECTOR.PREFIXES[name]
        output.extend(line(prefix, record) for record in records[name])
    return "\n".join(output) + "\n"


def validate(records: dict[str, list[dict[str, str]]], arm: str):
    return COLLECTOR.validate_run_log(
        render(records), "", arm=arm, dataset=DATASET, trace=TRACE,
        warmup=WARMUP, measured=MEASURED,
    )


def minimal_png(path: pathlib.Path, width: int = 1920, height: int = 1080) -> None:
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + (13).to_bytes(4, "big")
        + b"IHDR"
        + width.to_bytes(4, "big")
        + height.to_bytes(4, "big")
    )


class ReceiptValidationTests(unittest.TestCase):
    def test_all_four_plan_arms_validate(self) -> None:
        for arm in COLLECTOR.ARMS:
            with self.subTest(arm=arm):
                result = validate(synthetic_records(arm), arm)
                self.assertEqual(len(result["frames"]), MEASURED)
                self.assertIn(result["final_actual_plan"], COLLECTOR.PLAN_SEMANTICS)

    def test_missing_capture_or_terminal_never_validates(self) -> None:
        records = synthetic_records("cpu_post_sort")
        records["capture"].clear()
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "exactly one"):
            validate(records, "cpu_post_sort")

        records = synthetic_records("cpu_post_sort")
        records["frame"][1]["terminal_receipt"] = "map_failure"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "terminal_receipt"):
            validate(records, "cpu_post_sort")

    def test_forced_plan_drift_and_adaptive_unknown_plan_fail(self) -> None:
        records = synthetic_records("gpu_post_sort")
        records["frame"][1]["exact_plan_actual"] = "cpu_post_sort"
        records["frame"][1]["count_semantics"] = "direct_draw_equals_visible"
        records["frame"][1]["actual_backend"] = "cpu"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "forced plan drift"):
            validate(records, "gpu_post_sort")

        records = synthetic_records("adaptive")
        records["frame"][1]["exact_plan_actual"] = "adaptive"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "invalid actual plan"):
            validate(records, "adaptive")

    def test_counts_resolution_and_membership_fail_closed(self) -> None:
        mutations = [
            ("frame", 1, "drawn_count", "79", "D=C or D=V"),
            ("frame", 1, "visible_count", "101", "C<=V<=S"),
            ("frame", 1, "actual_backend", "cpu", "actual_backend"),
            ("frame", 1, "sort_refreshed", "false", "sort_refreshed"),
            ("frame", 1, "presented_width", "960", "presented_width"),
            ("begin", 0, "resident_count", "99", "resident_count"),
            ("begin", 0, "internal_render_width", "960", "internal_render_width"),
            ("capture", 0, "captured_width", "960", "captured_width"),
        ]
        for group, index, key, value, message in mutations:
            with self.subTest(group=group, key=key):
                records = synthetic_records("gpu_post_sort")
                records[group][index][key] = value
                with self.assertRaisesRegex(COLLECTOR.ValidationError, message):
                    validate(records, "gpu_post_sort")

    def test_duplicate_ticket_or_presentation_fails(self) -> None:
        records = synthetic_records("gpu_preproject")
        records["frame"][1]["current_stats_ticket"] = records["frame"][0]["current_stats_ticket"]
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "duplicate current-stats"):
            validate(records, "gpu_preproject")

        records = synthetic_records("gpu_preproject")
        records["capture"][0]["presentation_sequence"] = records["frame"][0]["presentation_sequence"]
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "reused.*presentation"):
            validate(records, "gpu_preproject")

    def test_phase_view_and_timestamp_schedule_mutations_fail(self) -> None:
        mutations = [
            ("phase", "measure", "phase violates"),
            ("trace_frame", "1", "trace_frame schedule"),
            ("trace_timestamp_ns", "1", "trace_timestamp_ns schedule"),
        ]
        for key, value, message in mutations:
            with self.subTest(key=key):
                records = synthetic_records("cpu_post_sort")
                records["frame"][0][key] = value
                with self.assertRaisesRegex(COLLECTOR.ValidationError, message):
                    validate(records, "cpu_post_sort")

    def test_capture_identity_must_be_artifact_relative(self) -> None:
        records = synthetic_records("cpu_post_sort")
        records["capture"][0]["path"] = "/tmp/stale-stage/final-frame.png"
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "artifact-relative"):
            validate(records, "cpu_post_sort")


class CanonicalAdmissionTests(unittest.TestCase):
    def test_caller_supplied_binary_is_not_an_option(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                COLLECTOR.parse_args(
                    [
                        "--binary", "/tmp/foreign-desktop-example",
                        "--dataset-manifest", str(COLLECTOR.CANONICAL_DATASET_MANIFEST),
                        "--trace", str(COLLECTOR.CANONICAL_TRACE_PATH),
                        "--output", "target/benchmarks/m2b/test",
                    ]
                )

    def test_collector_built_binary_content_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            binary = pathlib.Path(temporary) / "desktop-example"
            binary.write_bytes(b"collector-built")
            expected = COLLECTOR.sha256_file(binary)
            COLLECTOR.require_binary_sha256(binary, expected)
            binary.write_bytes(b"foreign-or-stale")
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "collector-built binary changed"):
                COLLECTOR.require_binary_sha256(binary, expected)

    def test_qualified_kitsune_manifest_tuple_is_frozen(self) -> None:
        COLLECTOR.validate_canonical_dataset_manifest(dict(COLLECTOR.CANONICAL_DATASET))
        for key in (
            "id", "qualification_status", "local_path", "sha256", "bytes", "splat_count", "sh_degree"
        ):
            with self.subTest(key=key):
                value = dict(COLLECTOR.CANONICAL_DATASET)
                value[key] = "wrong" if isinstance(value[key], str) else value[key] + 1
                with self.assertRaisesRegex(COLLECTOR.ValidationError, key):
                    COLLECTOR.validate_canonical_dataset_manifest(value)

    def test_alternate_dataset_and_trace_paths_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            alternate = pathlib.Path(temporary) / "alternate.json"
            alternate.write_text("{}", encoding="utf-8")
            for canonical, context in (
                (COLLECTOR.CANONICAL_DATASET_MANIFEST, "dataset manifest"),
                (COLLECTOR.CANONICAL_TRACE_PATH, "camera trace"),
            ):
                with self.subTest(context=context):
                    with self.assertRaisesRegex(COLLECTOR.ValidationError, context):
                        COLLECTOR.require_canonical_path(ROOT, alternate, canonical, context)

    def test_exact_trace_identity_content_file_and_views_are_frozen(self) -> None:
        path = ROOT / COLLECTOR.CANONICAL_TRACE_PATH
        canonical = json.loads(path.read_text(encoding="utf-8"))
        COLLECTOR.validate_canonical_trace(canonical, COLLECTOR.CANONICAL_TRACE["file_sha256"])
        mutations = [
            (lambda value: value.__setitem__("trace_id", "alternate"), "alternate trace identity"),
            (lambda value: value.__setitem__("content_sha256", "0" * 64), "content hash"),
            (lambda value: value["display"].__setitem__("width", 960), "1920x1080"),
            (lambda value: value["frames"].pop(), "views 0,1"),
            (lambda value: value["frames"][1].__setitem__("timestamp_ns", 1), "timestamps"),
        ]
        for mutate, message in mutations:
            with self.subTest(message=message):
                value = copy.deepcopy(canonical)
                mutate(value)
                with self.assertRaisesRegex(COLLECTOR.ValidationError, message):
                    COLLECTOR.validate_canonical_trace(
                        value, COLLECTOR.CANONICAL_TRACE["file_sha256"]
                    )
        with self.assertRaisesRegex(COLLECTOR.ValidationError, "file SHA-256"):
            COLLECTOR.validate_canonical_trace(canonical, "0" * 64)

    def test_repository_trace_validator_is_an_admission_gate(self) -> None:
        path = ROOT / COLLECTOR.CANONICAL_TRACE_PATH
        COLLECTOR.run_trace_validator(ROOT, path)
        rejected = subprocess.CompletedProcess([], 1, stdout="", stderr="invalid canonical trace")
        with mock.patch.object(COLLECTOR.subprocess, "run", return_value=rejected):
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "repository trace validator"):
                COLLECTOR.run_trace_validator(ROOT, path)

    def test_schedule_weakening_is_rejected(self) -> None:
        COLLECTOR.validate_canonical_schedule(
            COLLECTOR.CANONICAL_WARMUP, COLLECTOR.CANONICAL_MEASURED
        )
        for warmup, measured in ((19, 80), (20, 79), (0, 1), (21, 80), (20, 81)):
            with self.subTest(warmup=warmup, measured=measured):
                with self.assertRaisesRegex(COLLECTOR.ValidationError, "warmup=20 and measured=80"):
                    COLLECTOR.validate_canonical_schedule(warmup, measured)


class BuildIsolationTests(unittest.TestCase):
    def test_build_uses_new_empty_private_target_and_ignores_shared_poison(self) -> None:
        expected_git = {
            "commit": "a" * 40,
            "dirty": False,
            "status_porcelain_sha256": "b" * 64,
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            repo = root / "repo"
            stage = root / ".stage"
            repo.mkdir()
            stage.mkdir()
            shared_binary = repo / "target/release/desktop-example"
            shared_binary.parent.mkdir(parents=True)
            shared_binary.write_bytes(b"timestamp-preserved-poison")
            shared_binary.chmod(0o755)

            def cargo_build(command, **kwargs):
                self.assertEqual(command[:3], ["cargo", "build", "--locked"])
                target_dir = pathlib.Path(kwargs["env"]["CARGO_TARGET_DIR"])
                self.assertEqual(target_dir, (stage / "cargo-target").resolve())
                self.assertTrue(target_dir.is_dir())
                self.assertEqual(list(target_dir.iterdir()), [])
                binary = target_dir / "release/desktop-example"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"fresh-private-build")
                binary.chmod(0o755)
                message = {
                    "reason": "compiler-artifact",
                    "target": {"name": "desktop-example", "kind": ["bin"]},
                    "executable": str(binary),
                }
                return subprocess.CompletedProcess(command, 0, json.dumps(message), "")

            with (
                mock.patch.object(COLLECTOR.subprocess, "run", side_effect=cargo_build),
                mock.patch.object(COLLECTOR, "git_receipt", return_value=expected_git),
            ):
                binary = COLLECTOR.build_desktop_binary(repo, stage, expected_git)

            self.assertEqual(binary.read_bytes(), b"fresh-private-build")
            self.assertEqual(shared_binary.read_bytes(), b"timestamp-preserved-poison")
            command_receipt = json.loads((stage / "build/command.json").read_text())
            self.assertEqual(
                command_receipt["environment"]["CARGO_TARGET_DIR"],
                str((stage / "cargo-target").resolve()),
            )

    def test_preexisting_private_target_content_fails_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            repo = root / "repo"
            stage = root / ".stage"
            repo.mkdir()
            (stage / "cargo-target").mkdir(parents=True)
            (stage / "cargo-target/poison").write_bytes(b"poison")
            cargo = mock.Mock()
            with mock.patch.object(COLLECTOR.subprocess, "run", cargo):
                with self.assertRaisesRegex(
                    COLLECTOR.ValidationError, "private Cargo target already exists"
                ):
                    COLLECTOR.build_desktop_binary(repo, stage, {})
            cargo.assert_not_called()

    def test_cargo_executable_outside_private_target_is_rejected(self) -> None:
        expected_git = {
            "commit": "a" * 40,
            "dirty": False,
            "status_porcelain_sha256": "b" * 64,
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            repo = root / "repo"
            stage = root / ".stage"
            repo.mkdir()
            stage.mkdir()
            shared_binary = repo / "target/release/desktop-example"
            shared_binary.parent.mkdir(parents=True)
            shared_binary.write_bytes(b"poison")
            shared_binary.chmod(0o755)
            message = {
                "reason": "compiler-artifact",
                "target": {"name": "desktop-example", "kind": ["bin"]},
                "executable": str(shared_binary),
            }
            completed = subprocess.CompletedProcess([], 0, json.dumps(message), "")
            with mock.patch.object(COLLECTOR.subprocess, "run", return_value=completed):
                with self.assertRaisesRegex(
                    COLLECTOR.ValidationError, "outside the private target root"
                ):
                    COLLECTOR.build_desktop_binary(repo, stage, expected_git)

    def test_build_failure_starts_no_arm_and_publishes_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            output = root / "canonical"
            dataset_path = root / "kitsune.ply"
            trace_path = root / "trace.json"
            trace = {**TRACE, "path": str(trace_path)}
            expected_git = {
                "commit": "a" * 40,
                "dirty": False,
                "status_porcelain_sha256": "b" * 64,
            }
            args = SimpleNamespace(
                output=output,
                warmup=COLLECTOR.CANONICAL_WARMUP,
                measured=COLLECTOR.CANONICAL_MEASURED,
                refresh_hz=60.0,
                dataset_manifest=root / "dataset.json",
                trace=trace_path,
            )
            with (
                mock.patch.object(COLLECTOR, "git_receipt", return_value=expected_git),
                mock.patch.object(
                    COLLECTOR, "read_dataset_manifest", return_value=(DATASET, dataset_path)
                ),
                mock.patch.object(COLLECTOR, "read_trace", return_value=trace),
                mock.patch.object(
                    COLLECTOR,
                    "build_desktop_binary",
                    side_effect=COLLECTOR.ValidationError("private target setup failed"),
                ),
                mock.patch.object(COLLECTOR, "publish_validated_suite") as publish,
            ):
                with self.assertRaisesRegex(COLLECTOR.ValidationError, "private target setup failed"):
                    COLLECTOR.collect(args, ROOT)

            self.assertFalse(output.exists())
            publish.assert_not_called()
            failed = list(root.glob("canonical.failed-*"))
            self.assertEqual(len(failed), 1)
            suite = json.loads((failed[0] / "suite.json").read_text())
            self.assertEqual(suite["runs"], [])
            self.assertFalse(any((failed[0] / arm).exists() for arm in COLLECTOR.ARMS))


class CanonicalArtifactTests(unittest.TestCase):
    def complete_suite(self, stage: pathlib.Path, binary_sha256: str) -> dict[str, object]:
        runs = []
        for arm in COLLECTOR.ARMS:
            artifact = stage / arm / "artifact"
            artifact.mkdir(parents=True)
            for name in ("manifest.json", "frames.jsonl", "summary.json", "final-frame.png"):
                (artifact / name).write_bytes(b"retained")
            runs.append(
                {
                    "arm": arm,
                    "artifact": f"{arm}/artifact",
                    "validator_exit_status": 0,
                }
            )
        return {
            "schema": COLLECTOR.SUITE_SCHEMA,
            "status": "ok",
            "build": {"binary_sha256": binary_sha256},
            "runs": runs,
        }

    def test_built_artifact_passes_repository_validator(self) -> None:
        validated = validate(synthetic_records("adaptive"), "adaptive")
        build = {
            "binary_sha256": "d" * 64,
            "package_version": "0.1.3",
            "git": {
                "commit": "e" * 40,
                "dirty": False,
                "status_porcelain_sha256": "f" * 64,
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            artifact = pathlib.Path(temporary)
            capture = artifact / "final-frame.png"
            minimal_png(capture)
            COLLECTOR.build_artifact(
                artifact, arm="adaptive", validated=validated, dataset=DATASET, trace=TRACE,
                build=build, capture_path=capture,
                started_at="2026-07-25T00:00:00Z", ended_at="2026-07-25T00:00:01Z",
                warmup=WARMUP, refresh_hz=60.0, host_device=None,
            )
            completed = subprocess.run(
                [sys.executable, str(ROOT / "tests/perf/validate-benchmark-artifacts.py"), str(artifact)],
                cwd=ROOT, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)

    def test_damaged_or_mismatched_image_is_rejected(self) -> None:
        validated = validate(synthetic_records("cpu_post_sort"), "cpu_post_sort")
        build = {
            "binary_sha256": "d" * 64,
            "package_version": "0.1.3",
            "git": {
                "commit": "e" * 40,
                "dirty": False,
                "status_porcelain_sha256": "f" * 64,
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            artifact = pathlib.Path(temporary)
            capture = artifact / "final-frame.png"
            minimal_png(capture)
            COLLECTOR.build_artifact(
                artifact, arm="cpu_post_sort", validated=validated, dataset=DATASET, trace=TRACE,
                build=build, capture_path=capture,
                started_at="2026-07-25T00:00:00Z", ended_at="2026-07-25T00:00:01Z",
                warmup=WARMUP, refresh_hz=60.0, host_device=None,
            )
            capture.write_bytes(b"not a png")
            completed = subprocess.run(
                [sys.executable, str(ROOT / "tests/perf/validate-benchmark-artifacts.py"), str(artifact)],
                cwd=ROOT, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            self.assertNotEqual(completed.returncode, 0)

    def test_incomplete_or_unvalidated_suite_is_never_published(self) -> None:
        complete_runs = []
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            stage = root / ".stage"
            output = root / "canonical"
            for arm in COLLECTOR.ARMS:
                artifact = stage / arm / "artifact"
                artifact.mkdir(parents=True)
                for name in ("manifest.json", "frames.jsonl", "summary.json", "final-frame.png"):
                    (artifact / name).write_bytes(b"retained")
                complete_runs.append(
                    {
                        "arm": arm,
                        "artifact": f"{arm}/artifact",
                        "validator_exit_status": 0,
                    }
                )

            for runs, message in (
                (complete_runs[:-1], "all four arms"),
                (
                    [*complete_runs[:-1], {**complete_runs[-1], "validator_exit_status": 1}],
                    "validator did not pass",
                ),
            ):
                with self.subTest(message=message):
                    suite = {
                        "schema": COLLECTOR.SUITE_SCHEMA,
                        "status": "ok",
                        "runs": runs,
                    }
                    with self.assertRaisesRegex(COLLECTOR.ValidationError, message):
                        COLLECTOR.publish_validated_suite(stage, output, suite)
                    self.assertFalse(output.exists())
                    self.assertTrue(stage.is_dir())

    def test_publication_rejects_any_retained_execute_mode_bit(self) -> None:
        for mode in (0o100, 0o010, 0o001):
            with self.subTest(mode=oct(mode)), tempfile.TemporaryDirectory() as temporary:
                root = pathlib.Path(temporary)
                stage = root / ".stage"
                output = root / "canonical"
                suite = self.complete_suite(stage, "d" * 64)
                retained = stage / "retained-file"
                retained.write_bytes(b"must-not-publish")
                retained.chmod(mode)

                self.assertEqual(retained.stat().st_mode & 0o111, mode)
                with self.assertRaisesRegex(COLLECTOR.ValidationError, "executable files"):
                    COLLECTOR.publish_validated_suite(stage, output, suite)

                self.assertTrue(stage.is_dir())
                self.assertTrue(retained.is_file())
                self.assertFalse((stage / "suite.json").exists())
                self.assertFalse(output.exists())

    def test_publication_allows_retained_regular_file_with_mode_0000(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            stage = root / ".stage"
            output = root / "canonical"
            suite = self.complete_suite(stage, "d" * 64)
            retained = stage / "retained-file"
            retained.write_bytes(b"safe-to-publish")
            retained.chmod(0o000)

            self.assertEqual(retained.stat().st_mode & 0o111, 0)
            COLLECTOR.publish_validated_suite(stage, output, suite)

            self.assertFalse(stage.exists())
            self.assertTrue((output / "retained-file").is_file())
            self.assertTrue((output / "suite.json").is_file())

    def test_successful_publication_excludes_private_target_and_keeps_attestation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            stage = root / ".stage"
            output = root / "canonical"
            target = stage / "cargo-target"
            binary = target / "release/desktop-example"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"fresh-private-build")
            binary.chmod(0o755)
            binary_sha256 = COLLECTOR.sha256_file(binary)
            build_dir = stage / "build"
            build_dir.mkdir()
            (build_dir / "command.json").write_text('{"argv":["cargo","build"]}\n')
            (build_dir / "stdout.log").write_text("compiler artifact attestation\n")
            (build_dir / "stderr.log").write_text("")
            suite = self.complete_suite(stage, binary_sha256)

            COLLECTOR.require_binary_sha256(binary, binary_sha256)
            COLLECTOR.remove_private_cargo_target(stage)
            COLLECTOR.publish_validated_suite(stage, output, suite)

            self.assertTrue(output.is_dir())
            self.assertFalse((output / "cargo-target").exists())
            self.assertFalse(binary.exists())
            self.assertEqual(
                (output / "build/command.json").read_text(),
                '{"argv":["cargo","build"]}\n',
            )
            self.assertEqual(
                (output / "build/stdout.log").read_text(),
                "compiler artifact attestation\n",
            )
            published_suite = json.loads((output / "suite.json").read_text())
            self.assertEqual(published_suite["build"]["binary_sha256"], binary_sha256)
            self.assertFalse(
                any(path.is_file() and path.stat().st_mode & 0o111 for path in output.rglob("*"))
            )

    def test_private_target_cleanup_failure_is_retained_and_cannot_publish(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            output = root / "canonical"
            dataset_path = root / "kitsune.ply"
            dataset_path.write_bytes(b"canonical-dataset")
            dataset = {**DATASET, "sha256": COLLECTOR.sha256_file(dataset_path)}
            trace_path = root / "trace.json"
            trace_path.write_bytes(b"canonical-trace")
            trace = {
                **TRACE,
                "file_sha256": COLLECTOR.sha256_file(trace_path),
                "path": str(trace_path),
            }
            expected_git = {
                "commit": "a" * 40,
                "dirty": False,
                "status_porcelain_sha256": "b" * 64,
            }
            args = SimpleNamespace(
                output=output,
                warmup=COLLECTOR.CANONICAL_WARMUP,
                measured=COLLECTOR.CANONICAL_MEASURED,
                refresh_hz=60.0,
                dataset_manifest=root / "dataset.json",
                trace=trace_path,
            )
            validated_arms = []

            def build_binary(_repo, stage, _expected_git):
                build_dir = stage / "build"
                build_dir.mkdir()
                (build_dir / "command.json").write_text("{}\n")
                (build_dir / "stdout.log").write_text("build attestation\n")
                (build_dir / "stderr.log").write_text("")
                binary = stage / "cargo-target/release/desktop-example"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"fresh-private-build")
                binary.chmod(0o755)
                return binary

            def run(command, **kwargs):
                stage = next(root.glob(".canonical.stage-*"))
                self.assertTrue((stage / "cargo-target/release/desktop-example").is_file())
                if command[0] == sys.executable:
                    validated_arms.append(pathlib.Path(command[-1]).parent.name)
                else:
                    minimal_png(pathlib.Path(kwargs["cwd"]) / COLLECTOR.FINAL_CAPTURE_IDENTITY)
                return subprocess.CompletedProcess(command, 0, "", "")

            def build_artifact(directory, *, arm, **_kwargs):
                for name in ("manifest.json", "frames.jsonl", "summary.json"):
                    (directory / name).write_bytes(b"retained")
                return {
                    "run_id": f"run-{arm}",
                    "renderer": {"exact_plan_actual": arm},
                    "image": {"sha256": arm},
                }

            with (
                mock.patch.object(COLLECTOR, "git_receipt", return_value=expected_git),
                mock.patch.object(
                    COLLECTOR, "read_dataset_manifest", return_value=(dataset, dataset_path)
                ),
                mock.patch.object(COLLECTOR, "read_trace", return_value=trace),
                mock.patch.object(COLLECTOR, "build_desktop_binary", side_effect=build_binary),
                mock.patch.object(COLLECTOR, "package_version", return_value="0.1.3"),
                mock.patch.object(COLLECTOR, "host_device_name", return_value=None),
                mock.patch.object(COLLECTOR, "validate_run_log", return_value={}),
                mock.patch.object(COLLECTOR, "build_artifact", side_effect=build_artifact),
                mock.patch.object(COLLECTOR.subprocess, "run", side_effect=run),
                mock.patch.object(
                    COLLECTOR.shutil,
                    "rmtree",
                    side_effect=PermissionError("simulated cleanup denial"),
                ),
                mock.patch.object(COLLECTOR, "publish_validated_suite") as publish,
            ):
                with self.assertRaisesRegex(
                    COLLECTOR.ValidationError, "retained_failed_evidence"
                ):
                    COLLECTOR.collect(args, ROOT)

            self.assertFalse(output.exists())
            publish.assert_not_called()
            self.assertEqual(validated_arms, list(COLLECTOR.ARMS))
            failed = list(root.glob("canonical.failed-*"))
            self.assertEqual(len(failed), 1)
            self.assertTrue((failed[0] / "cargo-target/release/desktop-example").is_file())
            suite = json.loads((failed[0] / "suite.json").read_text())
            self.assertEqual(suite["status"], "failed")
            self.assertIn("private Cargo target cleanup failed", suite["error"])

    def test_raw_log_revalidates_after_atomic_publication(self) -> None:
        records = synthetic_records("cpu_post_sort")
        stdout = render(records)
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            stage = root / ".stage"
            artifact = stage / "cpu_post_sort" / "artifact"
            artifact.mkdir(parents=True)
            minimal_png(artifact / COLLECTOR.FINAL_CAPTURE_IDENTITY)
            (stage / "cpu_post_sort" / "stdout.log").write_text(stdout, encoding="utf-8")
            output = root / "published"
            stage.rename(output)
            published_arm = output / "cpu_post_sort"
            result = COLLECTOR.validate_run_log(
                (published_arm / "stdout.log").read_text(encoding="utf-8"),
                "",
                arm="cpu_post_sort",
                dataset=DATASET,
                trace=TRACE,
                warmup=WARMUP,
                measured=MEASURED,
                capture_path=published_arm / "artifact" / COLLECTOR.FINAL_CAPTURE_IDENTITY,
            )
            self.assertEqual(result["final_actual_plan"], "cpu_post_sort")


if __name__ == "__main__":
    unittest.main()
