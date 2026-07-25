#!/usr/bin/env python3
"""Focused tests for the full-quality cross-platform experiment validator."""

from __future__ import annotations

import copy
import importlib.util
import json
import pathlib
import shutil
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
VALIDATOR_PATH = ROOT / "tests/perf/validate-full-quality-experiment.py"
BENCHMARK_FIXTURE = ROOT / "tests/perf/fixtures/v1/valid"


def load_validator():
    spec = importlib.util.spec_from_file_location("full_quality_validator", VALIDATOR_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


VALIDATOR = load_validator()


def write_json(path: pathlib.Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def base_suite() -> dict:
    return {
        "schema": VALIDATOR.SCHEMA,
        "suite_id": "fixture-suite",
        "status": "complete",
        "pre_run_requirements": [],
        "renderer_path": "packed_atlas",
        "build": {
            "repository_commit": "0123456789abcdef0123456789abcdef01234567",
            "working_tree_dirty": False,
        },
        "quality_contract": copy.deepcopy(VALIDATOR.QUALITY_CONTRACT),
        "traces": [
            {
                "id": "static",
                "local_path": "tests/perf/trace/fixtures/phase-e-minimal-static-640x480-v1.json",
                "sha256": "b" * 64,
                "width": 1920,
                "height": 1080,
                "frame_count": 1,
                "evidence_class": "formal_full_quality",
                "dataset_id": "fixture",
                "camera_family": "fixture-static-v1",
                "pose_intrinsics_sha256": "c" * 64,
            }
        ],
        "datasets": [
            {
                "id": "fixture",
                "role": "full_scene",
                "local_path": "tests/datasets/minimal_ascii.ply",
                "sha256": "a" * 64,
                "bytes": 128,
                "splat_count": 2,
                "sh_degree": 0,
            }
        ],
        "endpoints": [
            {
                "id": "fixture-endpoint",
                "availability": "available",
                "artifact_platform": "fixture",
                "execution_class": "physical",
                "performance_evidence": True,
                "formal_display": {
                    "width": 1920,
                    "height": 1080,
                    "source": "test_fixture",
                },
            }
        ],
        "protocols": [
            {
                "id": "fixed",
                "evidence_class": "formal_full_quality",
                "dataset_ids": ["fixture"],
                "endpoint_ids": ["fixture-endpoint"],
                "sort_policies": ["cpu"],
                "repetitions": 1,
                "warmup_frames": 2,
                "measured_frames": 5,
                "sort_interval": 1,
                "randomization_seed": 20260722,
                "randomize_policy_order": True,
                "sort_refresh": "first_frame_then_reuse",
                "require_image": False,
                "display": {"width": 1920, "height": 1080},
                "camera": {
                    "mode": "fixed_frame",
                    "trace_id": "static",
                    "frame_indices": [0],
                    "require_display_match": True,
                    "display_policy": "trace_display_exact",
                    "quality_comparable": True,
                },
            }
        ],
        "capacity_rejections": [],
        "runs": [
            {
                "protocol_id": "fixed",
                "dataset_id": "fixture",
                "endpoint_id": "fixture-endpoint",
                "sort_policy": "cpu",
                "camera_case": "frame-000",
                "repetition": 1,
                "schedule_index": 1,
                "policy_position": 1,
                "artifact": "runs/cpu/artifact",
            }
        ],
    }


def install_artifact(root: pathlib.Path) -> pathlib.Path:
    artifact = root / "runs/cpu/artifact"
    artifact.parent.mkdir(parents=True)
    shutil.copytree(BENCHMARK_FIXTURE, artifact)
    manifest_path = artifact / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["trace"]["frame_index"] = 0
    manifest["trace"].update(
        {
            "reference_width": 1920,
            "reference_height": 1080,
            "require_display_match": True,
            "display_policy": "trace_display_exact",
            "quality_comparable": True,
        }
    )
    manifest["renderer"].update(
        {
            "path": "packed_atlas",
            "order_backend_requested": "cpu",
            "sort_interval": 1,
        }
    )
    manifest["display"]["width"] = 1920
    manifest["display"]["height"] = 1080
    manifest["exactness"] = {
        "source_splat_count": 2,
        "decoded_splat_count": 2,
        "encoded_splat_count": 2,
        "resident_splat_count": 2,
        "addressable_splat_count": 2,
        "source_sh_degree": 0,
        "resident_sh_degree": 0,
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "sh_degree_policy": "source",
        "partial_scene_published": False,
        "full_quality": True,
    }
    manifest["resolution"] = {
        "requested_width": 1920,
        "requested_height": 1080,
        "surface_width": 1920,
        "surface_height": 1080,
        "internal_render_width": 1920,
        "internal_render_height": 1080,
        "presented_width": 1920,
        "presented_height": 1080,
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": True,
    }
    write_json(manifest_path, manifest)
    summary_path = artifact / "summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    summary["sort_telemetry"] = {
        "cpu_frame_count": 5,
        "gpu_frame_count": 0,
        "gpu_sort_fallback_count": 0,
    }
    write_json(summary_path, summary)
    return artifact


class FullQualityExperimentTests(unittest.TestCase):
    def test_committed_plan_is_structurally_valid(self) -> None:
        result = VALIDATOR.validate(
            ROOT / "tests/perf/full-quality-matrix-plan-v1.json",
            allow_incomplete=True,
        )
        self.assertEqual(result.expected_cells, 339)
        self.assertEqual(result.rendered_cells, 0)
        self.assertEqual(len(result.missing_cells), 339)

    def test_complete_exact_suite_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            install_artifact(root)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            result = VALIDATOR.validate(suite_path)
            self.assertEqual(result.expected_cells, 1)
            self.assertEqual(result.rendered_cells, 1)
            self.assertEqual(result.capacity_rejected_cells, 0)
            self.assertEqual(result.missing_cells, ())

    def test_partial_residency_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["exactness"]["resident_splat_count"] = 1
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "resident_splat_count"):
                VALIDATOR.validate(suite_path)

    def test_false_full_quality_receipt_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["exactness"]["full_quality"] = False
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "full_quality"):
                VALIDATOR.validate(suite_path)

    def test_missing_sort_telemetry_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            summary_path = artifact / "summary.json"
            summary = json.loads(summary_path.read_text(encoding="utf-8"))
            del summary["sort_telemetry"]
            write_json(summary_path, summary)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "sort_telemetry"):
                VALIDATOR.validate(suite_path)

    def test_internal_resolution_downscale_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["resolution"]["internal_render_width"] = 320
            manifest["resolution"]["internal_render_height"] = 240
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "internal_render"):
                VALIDATOR.validate(suite_path)

    def test_upscaling_cannot_be_reported_as_full_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["resolution"]["upscaling"] = "bilinear"
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "upscaling"):
                VALIDATOR.validate(suite_path)

    def test_native_aspect_reprojection_is_not_formal_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["trace"].update(
                {
                    "require_display_match": False,
                    "display_policy": "native_aspect_reprojection",
                    "quality_comparable": False,
                }
            )
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(
                VALIDATOR.ValidationError, "require_display_match"
            ):
                VALIDATOR.validate(suite_path)

    def test_formal_trace_policy_name_is_required(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["trace"]["display_policy"] = "native_aspect_reprojection"
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "display_policy"):
                VALIDATOR.validate(suite_path)

    def test_formal_trace_must_be_quality_comparable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["trace"]["quality_comparable"] = False
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "quality_comparable"):
                VALIDATOR.validate(suite_path)

    def test_trace_reference_must_match_protocol_display(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["trace"]["reference_width"] = 320
            write_json(manifest_path, manifest)
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(
                VALIDATOR.ValidationError, "reference dimensions"
            ):
                VALIDATOR.validate(suite_path)

    def test_draw_budget_cannot_hide_visible_points(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            frames_path = artifact / "frames.jsonl"
            frames = [
                json.loads(line)
                for line in frames_path.read_text(encoding="utf-8").splitlines()
                if line
            ]
            frames[0]["drawn"] = 1
            frames_path.write_text(
                "\n".join(json.dumps(frame) for frame in frames) + "\n",
                encoding="utf-8",
            )
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "draw budget"):
                VALIDATOR.validate(suite_path)

    def test_exact_contributor_contract_allows_c_less_than_v(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["renderer"]["count_semantics"] = VALIDATOR.COUNT_SEMANTICS
            write_json(manifest_path, manifest)
            frames_path = artifact / "frames.jsonl"
            frames = [
                json.loads(line)
                for line in frames_path.read_text(encoding="utf-8").splitlines()
                if line
            ]
            for frame in frames:
                frame["visible"] = 2
                frame["contributor"] = 1
                frame["drawn"] = 1
                frame["exact_contributor_compaction"] = True
            frames_path.write_text(
                "\n".join(json.dumps(frame) for frame in frames) + "\n",
                encoding="utf-8",
            )
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            self.assertEqual(VALIDATOR.validate(suite_path).rendered_cells, 1)

    def test_contributor_contract_rejects_missing_flag(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["renderer"]["count_semantics"] = VALIDATOR.COUNT_SEMANTICS
            write_json(manifest_path, manifest)
            frames_path = artifact / "frames.jsonl"
            frames = [
                json.loads(line)
                for line in frames_path.read_text(encoding="utf-8").splitlines()
                if line
            ]
            for frame in frames:
                frame["contributor"] = frame["visible"]
            frames_path.write_text(
                "\n".join(json.dumps(frame) for frame in frames) + "\n",
                encoding="utf-8",
            )
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(
                VALIDATOR.ValidationError, "exact_contributor_compaction"
            ):
                VALIDATOR.validate(suite_path)

    def test_exact_contributor_contract_rejects_different_drawn_count(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            artifact = install_artifact(root)
            manifest_path = artifact / "manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["renderer"]["count_semantics"] = VALIDATOR.COUNT_SEMANTICS
            write_json(manifest_path, manifest)
            frames_path = artifact / "frames.jsonl"
            frames = [
                json.loads(line)
                for line in frames_path.read_text(encoding="utf-8").splitlines()
                if line
            ]
            for frame in frames:
                frame["visible"] = 2
                frame["contributor"] = 1
                frame["drawn"] = 2
                frame["exact_contributor_compaction"] = True
            frames_path.write_text(
                "\n".join(json.dumps(frame) for frame in frames) + "\n",
                encoding="utf-8",
            )
            suite_path = root / "suite.json"
            write_json(suite_path, base_suite())
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "drawn.*contributor"):
                VALIDATOR.validate(suite_path)

    def test_complete_suite_rejects_missing_backend_cell(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            install_artifact(root)
            suite = base_suite()
            suite["protocols"][0]["sort_policies"] = ["cpu", "gpu"]
            suite_path = root / "suite.json"
            write_json(suite_path, suite)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "missing 1 matrix cells"):
                VALIDATOR.validate(suite_path)

    def test_capacity_rejection_is_explicit_and_never_a_render_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            suite = base_suite()
            suite["runs"] = []
            suite["capacity_rejections"] = [
                {
                    "endpoint_id": "fixture-endpoint",
                    "dataset_id": "fixture",
                    "source_splat_count": 2,
                    "source_sh_degree": 0,
                    "scene_published": False,
                    "stage": "resource_preflight",
                    "error_code": "buffer_binding_limit",
                    "error_message": "complete scene exceeds one storage binding",
                    "resource": {
                        "kind": "position_alpha",
                        "required_bytes": 32,
                        "limit_bytes": 16,
                    },
                }
            ]
            suite_path = root / "suite.json"
            write_json(suite_path, suite)
            result = VALIDATOR.validate(suite_path)
            self.assertEqual(result.rendered_cells, 0)
            self.assertEqual(result.capacity_rejected_cells, 1)
            self.assertEqual(result.missing_cells, ())

    def test_low_resolution_cannot_enter_formal_matrix(self) -> None:
        suite = base_suite()
        suite["status"] = "planned"
        suite["runs"] = []
        suite["traces"][0].update({"width": 640, "height": 360})
        suite["endpoints"][0]["formal_display"].update(
            {"width": 640, "height": 360}
        )
        suite["protocols"][0]["display"] = {"width": 640, "height": 360}
        with tempfile.TemporaryDirectory() as directory:
            suite_path = pathlib.Path(directory) / "suite.json"
            write_json(suite_path, suite)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "diagnostic only"):
                VALIDATOR.validate(suite_path, allow_incomplete=True)

    def test_protocol_must_match_endpoint_formal_display(self) -> None:
        suite = base_suite()
        suite["status"] = "planned"
        suite["runs"] = []
        suite["endpoints"][0]["formal_display"].update(
            {"width": 2412, "height": 1080}
        )
        with tempfile.TemporaryDirectory() as directory:
            suite_path = pathlib.Path(directory) / "suite.json"
            write_json(suite_path, suite)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "formal_display"):
                VALIDATOR.validate(suite_path, allow_incomplete=True)

    def test_endpoint_variants_must_keep_one_pose_fov_family(self) -> None:
        suite = base_suite()
        suite["status"] = "planned"
        suite["runs"] = []
        android = copy.deepcopy(suite["endpoints"][0])
        android["id"] = "android"
        android["formal_display"] = {
            "width": 2412,
            "height": 1080,
            "source": "test_fixture",
        }
        suite["endpoints"].append(android)
        mobile_trace = copy.deepcopy(suite["traces"][0])
        mobile_trace.update(
            {
                "id": "static-mobile",
                "width": 2412,
                "height": 1080,
                "camera_family": "different-camera-v1",
            }
        )
        suite["traces"].append(mobile_trace)
        mobile_protocol = copy.deepcopy(suite["protocols"][0])
        mobile_protocol["id"] = "fixed-mobile"
        mobile_protocol["endpoint_ids"] = ["android"]
        mobile_protocol["display"] = {"width": 2412, "height": 1080}
        mobile_protocol["camera"]["trace_id"] = "static-mobile"
        suite["protocols"].append(mobile_protocol)
        with tempfile.TemporaryDirectory() as directory:
            suite_path = pathlib.Path(directory) / "suite.json"
            write_json(suite_path, suite)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "pose/FOV"):
                VALIDATOR.validate(suite_path, allow_incomplete=True)

    def test_motion_protocol_requires_interval_one(self) -> None:
        suite = base_suite()
        protocol = suite["protocols"][0]
        protocol["camera"] = {
            "mode": "trace_sequence",
            "trace_id": "static",
            "frame_indices": [0, 1],
            "require_display_match": True,
            "display_policy": "trace_display_exact",
            "quality_comparable": True,
        }
        protocol["sort_refresh"] = "every_camera_revision"
        protocol["sort_interval"] = 2
        suite["status"] = "planned"
        suite["endpoints"][0]["availability"] = "probe_required"
        suite["runs"] = []
        with tempfile.TemporaryDirectory() as directory:
            suite_path = pathlib.Path(directory) / "suite.json"
            write_json(suite_path, suite)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "sort_interval=1"):
                VALIDATOR.validate(suite_path, allow_incomplete=True)


if __name__ == "__main__":
    unittest.main()
