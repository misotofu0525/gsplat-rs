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
import pathlib
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
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        VALIDATOR.PNG_SIGNATURE
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(filtered))
        + png_chunk(b"IEND", b"")
    )


def solid_pixels(value: int, *, alpha: int = 255) -> bytes:
    return bytes([value, value, value, alpha] * 64)


def write_image(root: pathlib.Path, name: str, rgba: bytes) -> dict:
    path = root / name
    data = rgba_png(8, 8, rgba)
    path.write_bytes(data)
    return {
        "path": name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "width": 8,
        "height": 8,
    }


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
    trace_indices = [0, 1, 0] if moving else [0, 1]
    frames = []
    for capture_index, trace_frame_index in enumerate(trace_indices):
        exact = write_image(root, f"exact-{capture_index}.png", solid_pixels(64 + capture_index))
        candidate = write_image(
            root, f"candidate-{capture_index}.png", solid_pixels(64 + capture_index)
        )
        frame = {
            "capture_index": capture_index,
            "trace_frame_index": trace_frame_index,
            "presented": True,
            "exact": exact,
            "candidate": candidate,
        }
        refresh_frame_metrics(root, frame)
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
                }
            )
    manifest = {
        "schema": VALIDATOR.SCHEMA,
        "exactness": {
            "source_splat_count": 100,
            "decoded_splat_count": 100,
            "encoded_splat_count": 100,
            "resident_splat_count": 100,
            "addressable_splat_count": 100,
            "source_sh_degree": 3,
            "resident_sh_degree": 3,
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "sh_degree_policy": "source",
            "render_mode": "sorted_alpha",
            "partial_scene_published": False,
            "full_quality": True,
        },
        "resolution": {
            "requested_width": 8,
            "requested_height": 8,
            "surface_width": 8,
            "surface_height": 8,
            "internal_render_width": 8,
            "internal_render_height": 8,
            "presented_width": 8,
            "presented_height": 8,
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
    refresh_transition_metrics(root, manifest)
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

    def test_unsuccessful_presentation_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            manifest = base_manifest(root)
            manifest["frames"][0]["presented"] = False
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "presented must be true"):
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
            frame["candidate"] = write_image(root, "candidate-1-failed.png", solid_pixels(255))
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
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "separate artifacts"):
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
                frame["exact"] = write_image(root, f"exact-temporal-{index}.png", solid_pixels(128))
                frame["candidate"] = write_image(
                    root, f"candidate-temporal-{index}.png", solid_pixels(value)
                )
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
