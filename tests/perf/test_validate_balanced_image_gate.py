#!/usr/bin/env python3
"""Focused tests for the fail-closed Balanced v1 image-gate validator."""

from __future__ import annotations

import binascii
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
import pathlib
import shutil
import struct
import sys
import tempfile
import unittest
import zlib


ROOT = pathlib.Path(__file__).resolve().parents[2]
VALIDATOR_PATH = ROOT / "tests/perf/validate-balanced-image-gate.py"


def load_validator():
    spec = importlib.util.spec_from_file_location("balanced_image_gate", VALIDATOR_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


VALIDATOR = load_validator()


def write_json(path: pathlib.Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", binascii.crc32(kind + payload) & 0xFFFFFFFF)
    )


def rgba_png(width: int, height: int, rgba: bytes) -> bytes:
    assert len(rgba) == width * height * 4
    row_bytes = width * 4
    filtered = b"".join(
        b"\x00" + rgba[offset : offset + row_bytes]
        for offset in range(0, len(rgba), row_bytes)
    )
    return png_with_filtered_bytes(width, height, filtered)


def png_with_filtered_bytes(width: int, height: int, filtered: bytes) -> bytes:
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        VALIDATOR.PNG_SIGNATURE
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(filtered, level=9))
        + png_chunk(b"IEND", b"")
    )


def solid_pixels(width: int, height: int, value: int, *, alpha: int = 255) -> bytes:
    return bytes([value, value, value, alpha] * (width * height))


def write_image(
    root: pathlib.Path, name: str, width: int, height: int, rgba: bytes
) -> dict:
    path = root / name
    data = rgba_png(width, height, rgba)
    path.write_bytes(data)
    return {
        "path": name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "width": width,
        "height": height,
    }


def authority_receipt(
    dataset_path: str = "tests/perf/datasets/minimal_binary.json",
    trace_path: str = "tests/perf/trace/fixtures/camera-trace-v1.json",
) -> tuple[dict, dict, dict]:
    dataset_relative = pathlib.Path(dataset_path)
    trace_relative = pathlib.Path(trace_path)
    dataset_path = ROOT / dataset_relative
    trace_path = ROOT / trace_relative
    dataset = json.loads(dataset_path.read_text(encoding="utf-8"))
    trace = json.loads(trace_path.read_text(encoding="utf-8"))
    authority = {
        "dataset_manifest": {
            "path": dataset_relative.as_posix(),
            "sha256": hashlib.sha256(dataset_path.read_bytes()).hexdigest(),
            "dataset_id": dataset["id"],
            "asset_sha256": dataset["sha256"],
        },
        "trace": {
            "path": trace_relative.as_posix(),
            "sha256": hashlib.sha256(trace_path.read_bytes()).hexdigest(),
            "trace_id": trace["trace_id"],
            "content_sha256": trace["content_sha256"],
        },
    }
    return authority, dataset, trace


def decoded(root: pathlib.Path, receipt: dict):
    return VALIDATOR.decode_rgba8_png((root / receipt["path"]).read_bytes(), receipt["path"])


def refresh_frame_metrics(root: pathlib.Path, frame: dict) -> None:
    frame["metrics"] = VALIDATOR.compute_frame_metrics(
        decoded(root, frame["exact"]), decoded(root, frame["candidate"])
    )


def refresh_transition_metrics(root: pathlib.Path, manifest: dict) -> None:
    pixels = [
        VALIDATOR.FramePixels(
            capture_index=frame["capture_index"],
            trace_frame_index=frame["trace_frame_index"],
            exact=decoded(root, frame["exact"]),
            candidate=decoded(root, frame["candidate"]),
        )
        for frame in manifest["frames"]
    ]
    for index, transition in enumerate(manifest["transitions"]):
        transition["metrics"] = {
            VALIDATOR.TEMPORAL_METRIC: VALIDATOR.compute_temporal_metric(
                pixels[index], pixels[index + 1]
            )
        }


def base_manifest(root: pathlib.Path, *, moving: bool = False) -> dict:
    authority, dataset, trace = authority_receipt()
    width = trace["display"]["width"]
    height = trace["display"]["height"]
    trace_indices = [0, 1, 0] if moving else [0, 1]
    frames = []
    for capture_index, trace_frame_index in enumerate(trace_indices):
        exact = write_image(
            root,
            f"exact-{capture_index}.png",
            width,
            height,
            solid_pixels(width, height, 64 + capture_index),
        )
        candidate = write_image(
            root,
            f"candidate-{capture_index}.png",
            width,
            height,
            solid_pixels(width, height, 64 + capture_index),
        )
        exact["depth_precision"] = {
            "profile": "ExactFull32",
            "presentation_sequence": capture_index + 1,
        }
        candidate["depth_precision"] = {
            "profile": "CandidateStable24",
            "presentation_sequence": capture_index + 1,
        }
        frame = {
            "capture_index": capture_index,
            "trace_frame_index": trace_frame_index,
            "presented": True,
            "camera": {
                "trace_id": trace["trace_id"],
                "trace_content_sha256": trace["content_sha256"],
                "pose_intrinsics_sha256": VALIDATOR.canonical_sha256(
                    {
                        "pose": trace["frames"][trace_frame_index]["pose"],
                        "intrinsics": trace["frames"][trace_frame_index]["intrinsics"],
                    }
                ),
            },
            "presentation": {
                lane: {
                    "ticket": capture_index + 1,
                    "outcome": "presented",
                    "scene_generation": 1,
                    "camera_generation": capture_index + 1,
                    "viewport_generation": 1,
                    "contract_generation": 1,
                    "plan_generation": 1,
                    "presentation_generation": capture_index + 1,
                }
                for lane in ("exact", "candidate")
            },
            "exact": exact,
            "candidate": candidate,
            "metrics": {
                "ssim_luma_srgb_window8": 1.0,
                "rgb_mae_normalized": 0.0,
                "rgb_bad_pixel_fraction_over_3": 0.0,
                "alpha_mae_normalized": 0.0,
                "alpha_bad_pixel_fraction_over_1": 0.0,
            },
        }
        frames.append(frame)
    transitions = []
    if moving:
        for index in range(2):
            transitions.append(
                {
                    "from_capture_index": index,
                    "to_capture_index": index + 1,
                    "from_trace_frame_index": trace_indices[index],
                    "to_trace_frame_index": trace_indices[index + 1],
                    "metrics": {VALIDATOR.TEMPORAL_METRIC: 0.0},
                }
            )
    manifest = {
        "schema": VALIDATOR.SCHEMA,
        "evidence_class": "contract_fixture",
        "authority": authority,
        "exactness": {
            "source_splat_count": dataset["splat_count"],
            "decoded_splat_count": dataset["splat_count"],
            "encoded_splat_count": dataset["splat_count"],
            "resident_splat_count": dataset["splat_count"],
            "addressable_splat_count": dataset["splat_count"],
            "source_sh_degree": dataset["sh_degree"],
            "resident_sh_degree": dataset["sh_degree"],
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "sh_degree_policy": "source",
            "render_mode": "sorted_alpha",
            "partial_scene_published": False,
            "full_quality": True,
        },
        "resolution": {
            "requested_width": width,
            "requested_height": height,
            "surface_width": width,
            "surface_height": height,
            "internal_render_width": width,
            "internal_render_height": height,
            "presented_width": width,
            "presented_height": height,
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": True,
        },
        "camera": {
            "mode": "moving_sequence" if moving else "authored_views",
            "trace_frame_indices": trace_indices,
        },
        "frames": frames,
        "transitions": transitions,
    }
    return manifest


def benchmark_distribution(value: float, count: int) -> dict:
    return {
        "count": count,
        "mean": value,
        "p50": value,
        "p90": value,
        "p95": value,
        "p99": value,
        "max": value,
    }


def write_canonical_benchmark_artifact(
    root: pathlib.Path,
    *,
    lane: str,
    capture_index: int,
    pair_id: str,
    dataset: dict,
    trace: dict,
    presentation: dict,
    camera: dict,
    image_receipt: dict,
) -> dict:
    run_id = f"formal-{lane}-{capture_index}"
    artifact = root / "benchmark" / run_id
    artifact.mkdir(parents=True)
    image_data = (root / image_receipt["path"]).read_bytes()
    image_path = artifact / "final-frame.png"
    image_path.write_bytes(image_data)
    benchmark_image = {
        "path": image_path.name,
        "sha256": hashlib.sha256(image_data).hexdigest(),
        "width": image_receipt["width"],
        "height": image_receipt["height"],
    }
    unavailable = [
        "frames[*].preprocess_ms",
        "frames[*].sort_ms",
        "frames[*].geometry_submit_ms",
        "frames[*].gpu_wait_ms",
        "frames[*].gpu_complete_ms",
    ]
    manifest = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {
            "series_id": f"balanced-formal-{capture_index}",
            "started_at_utc": "2026-07-27T00:00:00Z",
            "ended_at_utc": "2026-07-27T00:00:01Z",
            "measurement_started_at_utc": "2026-07-27T00:00:00Z",
            "measurement_ended_at_utc": "2026-07-27T00:00:01Z",
        },
        "build": {
            "repository_commit": "1" * 40,
            "dirty": False,
            "profile": "release",
            "package_version": "0.1.3",
        },
        "dataset": {
            "id": dataset["id"],
            "sha256": dataset["sha256"],
            "bytes": dataset["bytes"],
            "splat_count": dataset["splat_count"],
            "sh_degree": dataset["sh_degree"],
        },
        "trace": {"id": trace["trace_id"], "sha256": trace["content_sha256"]},
        "renderer": {
            "implementation": f"balanced-{lane}",
            "path": "sorted_index_direct",
            "backend": "test",
            "sort_policy": "sync",
        },
        "display": {
            "width": image_receipt["width"],
            "height": image_receipt["height"],
            "dpr": 1.0,
            "refresh_hz": 60.0,
            "frame_budget_ms": 16.6666666667,
            "refresh_hz_source": "formal fixture",
            "frame_budget_source": "formal fixture",
        },
        "environment": {
            "platform": "fixture",
            "os": "fixture",
            "device": None,
            "browser": None,
            "adapter": None,
            "driver": None,
        },
        "image": benchmark_image,
        "unavailable_fields": unavailable,
    }
    write_json(artifact / "manifest.json", manifest)

    frames = []
    for frame_index in range(capture_index + 1):
        selected = frame_index == capture_index
        frame = {
            "schema": "gsplat-benchmark/v1",
            "record_type": "frame",
            "run_id": run_id,
            "frame_index": frame_index,
            "elapsed_ns": frame_index * 1_000_000,
            "call_ms": 1.0,
            "frame_wall_ms": 1.0,
            "preprocess_ms": None,
            "sort_ms": None,
            "geometry_submit_ms": None,
            "gpu_wait_ms": None,
            "gpu_complete_ms": None,
            "visible": dataset["splat_count"],
            "drawn": dataset["splat_count"],
            "active_splats": dataset["splat_count"],
            "sort_refreshed": True,
        }
        if selected:
            frame.update(
                {
                    "pair_id": pair_id,
                    "capture_index": capture_index,
                    "trace_frame_index": capture_index,
                    "camera": copy.deepcopy(camera),
                    "terminal_outcome": "presented",
                    "presentation": copy.deepcopy(presentation),
                    "capture_depth_precision": copy.deepcopy(
                        image_receipt["depth_precision"]
                    ),
                }
            )
        frames.append(frame)
    (artifact / "frames.jsonl").write_text(
        "".join(json.dumps(frame, sort_keys=True) + "\n" for frame in frames),
        encoding="utf-8",
    )
    count = len(frames)
    distributions = {
        metric: benchmark_distribution(1.0, count)
        if metric in {"call_ms", "frame_wall_ms"}
        else None
        for metric in (
            "call_ms",
            "frame_wall_ms",
            "preprocess_ms",
            "sort_ms",
            "geometry_submit_ms",
            "gpu_wait_ms",
            "gpu_complete_ms",
        )
    }
    summary = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": count,
        "warmup_count": 0,
        "frame_budget_ms": 16.6666666667,
        "missed_frame_count": 0,
        "distributions": distributions,
    }
    write_json(artifact / "summary.json", summary)
    return {
        "path": artifact.relative_to(root).as_posix(),
        "sha256": VALIDATOR.artifact_directory_sha256(artifact),
        "run_id": run_id,
        "frame_index": capture_index,
        "depth_precision": copy.deepcopy(image_receipt["depth_precision"]),
    }


def formal_manifest(root: pathlib.Path) -> dict:
    manifest = base_manifest(root)
    authority, dataset, trace = authority_receipt(
        "tests/perf/datasets/flowers.json",
        "tests/perf/trace/fixtures/quality/candidate-flowers-quality-1920x1080-v1.json",
    )
    manifest["evidence_class"] = "formal_quality"
    manifest["authority"] = authority
    for field in VALIDATOR.EXACT_COUNT_FIELDS:
        manifest["exactness"][field] = dataset["splat_count"]
    manifest["exactness"]["source_sh_degree"] = dataset["sh_degree"]
    manifest["exactness"]["resident_sh_degree"] = dataset["sh_degree"]
    width = trace["display"]["width"]
    height = trace["display"]["height"]
    for stage in VALIDATOR.RESOLUTION_STAGES:
        manifest["resolution"][f"{stage}_width"] = width
        manifest["resolution"][f"{stage}_height"] = height

    for frame in manifest["frames"]:
        capture_index = frame["capture_index"]
        frame["exact"] = write_image(
            root,
            f"formal-exact-{capture_index}.png",
            width,
            height,
            solid_pixels(width, height, 64 + capture_index),
        )
        frame["candidate"] = write_image(
            root,
            f"formal-candidate-{capture_index}.png",
            width,
            height,
            solid_pixels(width, height, 64 + capture_index),
        )
        frame["exact"]["depth_precision"] = {
            "profile": "ExactFull32",
            "presentation_sequence": frame["presentation"]["exact"][
                "presentation_generation"
            ],
        }
        frame["candidate"]["depth_precision"] = {
            "profile": "CandidateStable24",
            "presentation_sequence": frame["presentation"]["candidate"][
                "presentation_generation"
            ],
        }
        trace_frame = trace["frames"][frame["trace_frame_index"]]
        pose_intrinsics_sha256 = VALIDATOR.canonical_sha256(
            {"pose": trace_frame["pose"], "intrinsics": trace_frame["intrinsics"]}
        )
        frame["camera"] = {
            "trace_id": trace["trace_id"],
            "trace_content_sha256": trace["content_sha256"],
            "pose_intrinsics_sha256": pose_intrinsics_sha256,
        }
        pair_id = f"balanced-formal-pair-{capture_index}"
        frame["benchmark_artifacts"] = {
            "pair_id": pair_id,
            "exact": {
                **write_canonical_benchmark_artifact(
                    root,
                    lane="exact",
                    capture_index=capture_index,
                    pair_id=pair_id,
                    dataset=dataset,
                    trace=trace,
                    presentation=frame["presentation"]["exact"],
                    camera=frame["camera"],
                    image_receipt=frame["exact"],
                )
            },
            "candidate": {
                **write_canonical_benchmark_artifact(
                    root,
                    lane="candidate",
                    capture_index=capture_index,
                    pair_id=pair_id,
                    dataset=dataset,
                    trace=trace,
                    presentation=frame["presentation"]["candidate"],
                    camera=frame["camera"],
                    image_receipt=frame["candidate"],
                )
            },
        }
    return manifest


def validate_manifest(root: pathlib.Path, manifest: dict):
    path = root / "gate.json"
    write_json(path, manifest)
    return VALIDATOR.validate(path)


class BalancedImageGateTests(unittest.TestCase):
    def test_valid_authored_views_recompute_rgba_metrics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            result = validate_manifest(root, base_manifest(root))
            self.assertEqual(result.frame_count, 2)
            self.assertEqual(result.transition_count, 0)
            self.assertEqual(len(result.validator_sha256), 64)

    def test_valid_moving_sequence_recomputes_both_transitions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            result = validate_manifest(root, base_manifest(root, moving=True))
            self.assertEqual(result.frame_count, 3)
            self.assertEqual(result.transition_count, 2)

    def test_cli_records_validator_version_and_hash(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest_path = root / "gate.json"
            write_json(manifest_path, base_manifest(root))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(VALIDATOR.main([str(manifest_path)]), 0)
            receipt = json.loads(output.getvalue())
            self.assertEqual(receipt["schema"], VALIDATOR.SCHEMA)
            self.assertEqual(receipt["validator"]["version"], VALIDATOR.VALIDATOR_VERSION)
            self.assertEqual(
                receipt["validator"]["sha256"],
                hashlib.sha256(VALIDATOR_PATH.read_bytes()).hexdigest(),
            )
            self.assertIs(receipt["pass"], True)

    def test_missing_membership_field_is_not_defaulted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            del manifest["exactness"]["encoded_splat_count"]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "encoded_splat_count"):
                validate_manifest(root, manifest)

    def test_partial_membership_and_sh_reduction_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)
            for field, value, message in [
                ("resident_splat_count", 99, "source=decoded=encoded=resident=addressable"),
                ("resident_sh_degree", 2, "resident SH degree"),
            ]:
                with self.subTest(field=field):
                    manifest = copy.deepcopy(baseline)
                    manifest["exactness"][field] = value
                    with self.assertRaisesRegex(VALIDATOR.ValidationError, message):
                        validate_manifest(root, manifest)

    def test_self_consistent_count_one_and_sh_99_cannot_override_dataset(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)

            count_one = copy.deepcopy(baseline)
            for field in VALIDATOR.EXACT_COUNT_FIELDS:
                count_one["exactness"][field] = 1
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "authoritative dataset"):
                validate_manifest(root, count_one)

            sh_99 = copy.deepcopy(baseline)
            sh_99["exactness"]["source_sh_degree"] = 99
            sh_99["exactness"]["resident_sh_degree"] = 99
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "authoritative dataset"):
                validate_manifest(root, sh_99)

    def test_every_resolution_stage_must_equal_requested_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)
            for stage in ("surface", "internal_render", "presented"):
                with self.subTest(stage=stage):
                    manifest = copy.deepcopy(baseline)
                    manifest["resolution"][f"{stage}_width"] = 4
                    with self.assertRaisesRegex(VALIDATOR.ValidationError, "dimensions must match"):
                        validate_manifest(root, manifest)

    def test_dynamic_resolution_and_upscaling_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)
            for field, value in [("dynamic_resolution", "enabled"), ("upscaling", "bilinear")]:
                with self.subTest(field=field):
                    manifest = copy.deepcopy(baseline)
                    manifest["resolution"][field] = value
                    with self.assertRaisesRegex(VALIDATOR.ValidationError, field):
                        validate_manifest(root, manifest)

    def test_formal_quality_cannot_use_an_8x8_trace_and_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = formal_manifest(root)
            for stage in VALIDATOR.RESOLUTION_STAGES:
                manifest["resolution"][f"{stage}_width"] = 8
                manifest["resolution"][f"{stage}_height"] = 8
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "trace display"):
                validate_manifest(root, manifest)

    def test_formal_quality_forbids_minimal_contract_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["evidence_class"] = "formal_quality"
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "minimal contract fixture"):
                validate_manifest(root, manifest)

    def test_formal_trace_derivation_cannot_be_spliced_across_datasets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            authority, _, _ = authority_receipt(
                "tests/perf/datasets/flowers.json",
                "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
            )
            manifest["evidence_class"] = "formal_quality"
            manifest["authority"] = authority
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "derivation.source_path"):
                validate_manifest(root, manifest)

    def test_exact_and_candidate_lifecycle_generations_must_match(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = formal_manifest(root)
            manifest["frames"][0]["presentation"]["candidate"]["plan_generation"] = 2
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "plan_generation must match"):
                validate_manifest(root, manifest)

    def test_presentation_generation_must_match_even_when_both_lanes_are_monotonic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["presentation"]["candidate"][
                "presentation_generation"
            ] = 10
            manifest["frames"][1]["presentation"]["candidate"][
                "presentation_generation"
            ] = 11
            with self.assertRaisesRegex(
                VALIDATOR.ValidationError, "presentation_generation must match"
            ):
                validate_manifest(root, manifest)

    def test_capture_depth_precision_profiles_are_lane_specific_and_required(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)

            wrong_exact = copy.deepcopy(baseline)
            wrong_exact["frames"][0]["exact"]["depth_precision"]["profile"] = (
                "CandidateStable24"
            )
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "ExactFull32"):
                validate_manifest(root, wrong_exact)

            wrong_candidate = copy.deepcopy(baseline)
            wrong_candidate["frames"][0]["candidate"]["depth_precision"]["profile"] = (
                "ExactFull32"
            )
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "CandidateStable24"):
                validate_manifest(root, wrong_candidate)

            missing_profile = copy.deepcopy(baseline)
            del missing_profile["frames"][0]["candidate"]["depth_precision"]["profile"]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "profile"):
                validate_manifest(root, missing_profile)

    def test_capture_depth_precision_sequence_must_match_successful_present(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["candidate"]["depth_precision"][
                "presentation_sequence"
            ] = 99
            with self.assertRaisesRegex(
                VALIDATOR.ValidationError, "presentation_sequence must match"
            ):
                validate_manifest(root, manifest)

    def test_valid_1920x1080_formal_quality_binds_canonical_runs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            result = validate_manifest(root, formal_manifest(root))
            self.assertEqual(result.evidence_class, "formal_quality")
            self.assertEqual(result.frame_count, 2)

    def test_candidate_benchmark_frame_must_match_capture_and_trace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = formal_manifest(root)
            receipt = manifest["frames"][0]["benchmark_artifacts"]["candidate"]
            artifact = root / receipt["path"]
            frames_path = artifact / "frames.jsonl"
            benchmark_frame = json.loads(frames_path.read_text(encoding="utf-8"))
            benchmark_frame["capture_index"] = 1
            benchmark_frame["trace_frame_index"] = 1
            frames_path.write_text(
                json.dumps(benchmark_frame, sort_keys=True) + "\n", encoding="utf-8"
            )
            receipt["sha256"] = VALIDATOR.artifact_directory_sha256(artifact)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "capture_index mismatch"):
                validate_manifest(root, manifest)

    def test_canonical_benchmark_must_retain_capture_depth_precision_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = formal_manifest(root)
            receipt = manifest["frames"][0]["benchmark_artifacts"]["candidate"]
            artifact = root / receipt["path"]
            frames_path = artifact / "frames.jsonl"
            benchmark_frame = json.loads(frames_path.read_text(encoding="utf-8"))
            del benchmark_frame["capture_depth_precision"]
            frames_path.write_text(
                json.dumps(benchmark_frame, sort_keys=True) + "\n", encoding="utf-8"
            )
            receipt["sha256"] = VALIDATOR.artifact_directory_sha256(artifact)
            with self.assertRaisesRegex(
                VALIDATOR.ValidationError, "benchmark capture depth-precision receipt"
            ):
                validate_manifest(root, manifest)

    def test_external_benchmark_artifact_is_rejected_before_validation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            container = pathlib.Path(directory)
            root = container / "gate"
            root.mkdir()
            manifest = formal_manifest(root)
            receipt = manifest["frames"][0]["benchmark_artifacts"]["candidate"]
            external = container / "forged-canonical-run"
            shutil.copytree(root / receipt["path"], external)
            receipt["path"] = "../forged-canonical-run"
            receipt["sha256"] = VALIDATOR.artifact_directory_sha256(external)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "relative path"):
                validate_manifest(root, manifest)

    def test_canonical_benchmark_active_count_must_bind_full_membership(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = formal_manifest(root)
            receipt = manifest["frames"][0]["benchmark_artifacts"]["candidate"]
            artifact = root / receipt["path"]
            frames_path = artifact / "frames.jsonl"
            benchmark_frame = json.loads(frames_path.read_text(encoding="utf-8"))
            benchmark_frame["active_splats"] -= 1
            frames_path.write_text(
                json.dumps(benchmark_frame, sort_keys=True) + "\n", encoding="utf-8"
            )
            receipt["sha256"] = VALIDATOR.artifact_directory_sha256(artifact)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "active_splats"):
                validate_manifest(root, manifest)

    def test_missing_authority_or_lifecycle_receipt_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)

            missing_authority = copy.deepcopy(baseline)
            del missing_authority["authority"]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "authority"):
                validate_manifest(root, missing_authority)

            missing_ticket = copy.deepcopy(baseline)
            del missing_ticket["frames"][0]["presentation"]["candidate"]["ticket"]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "ticket"):
                validate_manifest(root, missing_ticket)

            missing_generation = copy.deepcopy(baseline)
            del missing_generation["frames"][0]["presentation"]["exact"]["plan_generation"]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "plan_generation"):
                validate_manifest(root, missing_generation)

    def test_camera_receipt_must_bind_authoritative_pose_and_intrinsics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["camera"]["pose_intrinsics_sha256"] = "0" * 64
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "pose/intrinsics"):
                validate_manifest(root, manifest)

    def test_unsuccessful_presentation_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["presented"] = False
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "presented must be true"):
                validate_manifest(root, manifest)

            manifest = base_manifest(root)
            manifest["frames"][0]["presentation"]["candidate"]["outcome"] = "failed"
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "outcome"):
                validate_manifest(root, manifest)

    def test_image_hash_and_rgba8_encoding_are_verified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root)
            bad_hash = copy.deepcopy(baseline)
            bad_hash["frames"][0]["candidate"]["sha256"] = "0" * 64
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "SHA-256 mismatch"):
                validate_manifest(root, bad_hash)

            broken = copy.deepcopy(baseline)
            path = root / broken["frames"][0]["candidate"]["path"]
            data = bytearray(path.read_bytes())
            data[24] = 2  # IHDR color type: RGB instead of RGBA.
            path.write_bytes(data)
            broken["frames"][0]["candidate"]["sha256"] = hashlib.sha256(data).hexdigest()
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "CRC mismatch|RGBA8"):
                validate_manifest(root, broken)

    def test_image_path_may_not_escape_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["candidate"]["path"] = "../outside.png"
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "relative path"):
                validate_manifest(root, manifest)

    def test_declared_frame_metric_must_match_recomputed_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["metrics"]["rgb_mae_normalized"] = 0.001
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "recomputed RGBA8 bytes"):
                validate_manifest(root, manifest)

    def test_frame_threshold_is_applied_per_frame_not_as_an_average(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            frame = manifest["frames"][1]
            width = frame["candidate"]["width"]
            height = frame["candidate"]["height"]
            depth_precision = frame["candidate"]["depth_precision"]
            frame["candidate"] = write_image(
                root,
                "candidate-1-failed.png",
                width,
                height,
                solid_pixels(width, height, 255),
            )
            frame["candidate"]["depth_precision"] = depth_precision
            refresh_frame_metrics(root, frame)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "Balanced v1 gate"):
                validate_manifest(root, manifest)

    def test_moving_sequence_requires_exact_0_1_0_camera_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root, moving=True)
            manifest["camera"]["trace_frame_indices"] = [0, 1, 1]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "0 -> 1 -> 0"):
                validate_manifest(root, manifest)

    def test_static_mode_cannot_disguise_a_0_1_0_sequence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root, moving=True)
            manifest["camera"]["mode"] = "static"
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "camera.mode"):
                validate_manifest(root, manifest)

    def test_authored_views_require_both_frozen_trace_frames(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["camera"]["trace_frame_indices"] = [0]
            manifest["frames"] = manifest["frames"][:1]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "trace frames 0 and 1"):
                validate_manifest(root, manifest)

    def test_exact_and_candidate_must_be_separate_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["candidate"] = copy.deepcopy(
                manifest["frames"][0]["exact"]
            )
            manifest["frames"][0]["candidate"]["depth_precision"]["profile"] = (
                "CandidateStable24"
            )
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "separate artifacts"):
                validate_manifest(root, manifest)

    def test_symlink_and_hardlink_aliases_are_the_same_image_artifact(self) -> None:
        for alias_kind in ("symlink", "hardlink"):
            with self.subTest(alias_kind=alias_kind), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                manifest = base_manifest(root)
                frame = manifest["frames"][0]
                exact_path = root / frame["exact"]["path"]
                candidate_path = root / frame["candidate"]["path"]
                candidate_path.unlink()
                if alias_kind == "symlink":
                    os.symlink(exact_path.name, candidate_path)
                else:
                    os.link(exact_path, candidate_path)
                with self.assertRaisesRegex(VALIDATOR.ValidationError, "separate artifacts"):
                    validate_manifest(root, manifest)

    def test_png_decompression_is_bounded_to_expected_rgba_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            frame = manifest["frames"][0]
            candidate_path = root / frame["candidate"]["path"]
            width = frame["candidate"]["width"]
            height = frame["candidate"]["height"]
            expected_filtered_bytes = height * (width * 4 + 1)
            bomb = png_with_filtered_bytes(
                width, height, b"\x00" * (expected_filtered_bytes + 1024 * 1024)
            )
            candidate_path.write_bytes(bomb)
            frame["candidate"]["sha256"] = hashlib.sha256(bomb).hexdigest()
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "exceeds its RGBA receipt"):
                validate_manifest(root, manifest)

    def test_missing_or_misjoined_transition_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            baseline = base_manifest(root, moving=True)
            missing = copy.deepcopy(baseline)
            missing["transitions"].pop()
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "every and only adjacent"):
                validate_manifest(root, missing)

            misjoined = copy.deepcopy(baseline)
            misjoined["transitions"][0]["to_capture_index"] = 2
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "adjacent capture"):
                validate_manifest(root, misjoined)

    def test_temporal_gate_catches_opposite_small_per_frame_errors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root, moving=True)
            for index, value in enumerate([129, 127, 129]):
                frame = manifest["frames"][index]
                width = frame["exact"]["width"]
                height = frame["exact"]["height"]
                frame["exact"] = write_image(
                    root,
                    f"exact-temporal-{index}.png",
                    width,
                    height,
                    solid_pixels(width, height, 128),
                )
                frame["candidate"] = write_image(
                    root,
                    f"candidate-temporal-{index}.png",
                    width,
                    height,
                    solid_pixels(width, height, value),
                )
                frame["exact"]["depth_precision"] = {
                    "profile": "ExactFull32",
                    "presentation_sequence": index + 1,
                }
                frame["candidate"]["depth_precision"] = {
                    "profile": "CandidateStable24",
                    "presentation_sequence": index + 1,
                }
                refresh_frame_metrics(root, frame)
            refresh_transition_metrics(root, manifest)
            with self.assertRaisesRegex(VALIDATOR.ValidationError, VALIDATOR.TEMPORAL_METRIC):
                validate_manifest(root, manifest)

    def test_missing_metric_is_not_defaulted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root, moving=True)
            del manifest["transitions"][0]["metrics"][VALIDATOR.TEMPORAL_METRIC]
            with self.assertRaisesRegex(VALIDATOR.ValidationError, VALIDATOR.TEMPORAL_METRIC):
                validate_manifest(root, manifest)


if __name__ == "__main__":
    unittest.main()
