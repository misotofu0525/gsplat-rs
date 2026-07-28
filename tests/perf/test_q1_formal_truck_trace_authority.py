#!/usr/bin/env python3
"""Focused tests for the formal Truck Product Quality trace authority."""

from __future__ import annotations

import dataclasses
import hashlib
import json
import pathlib
import struct
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import q1_formal_truck_trace_authority as FORMAL


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def jpeg(width: int = FORMAL.WIDTH, height: int = FORMAL.HEIGHT) -> bytes:
    return (
        b"\xff\xd8\xff\xc0\x00\x08\x08"
        + struct.pack(">HHB", height, width, 3)
        + b"\xff\xd9"
    )


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def rgb_png() -> bytes:
    row = b"\x00" + b"\x00\x00\x00" * FORMAL.WIDTH
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(
            b"IHDR",
            struct.pack(">IIBBBBB", FORMAL.WIDTH, FORMAL.HEIGHT, 8, 2, 0, 0, 0),
        )
        + png_chunk(b"IDAT", zlib.compress(row * FORMAL.HEIGHT))
        + png_chunk(b"IEND", b"")
    )


def images_bin(names: tuple[str, str]) -> bytes:
    records = []
    poses = (
        ((1.0, 0.0, 0.0, 0.0), (1.0, 2.0, 3.0)),
        ((2.0**-0.5, 0.0, 2.0**-0.5, 0.0), (-1.0, 0.5, 4.0)),
    )
    for image_id, (name, (qvec, tvec)) in enumerate(zip(names, poses, strict=True), 1):
        records.append(
            struct.pack("<i4d3di", image_id, *qvec, *tvec, 1)
            + name.encode("utf-8")
            + b"\x00"
            + struct.pack("<Q", 0)
        )
    return struct.pack("<Q", len(records)) + b"".join(records)


@dataclasses.dataclass(frozen=True)
class Fixture:
    source_root: pathlib.Path
    evaluation_root: pathlib.Path
    source_spec: FORMAL.CAMERA_AUTHORITY.AuthoritySpec
    evaluation_entries: tuple[FORMAL.EVALUATION_AUTHORITY.EntrySpec, ...]


def make_fixture(
    root: pathlib.Path,
    *,
    view_names: tuple[str, str] = ("000001.jpg", "000009.jpg"),
    cx: float = FORMAL.WIDTH / 2.0,
) -> Fixture:
    source_input = root / "source-input"
    source_input.mkdir()
    archive = root / "camera.zip"
    archive.write_bytes(b"camera archive")
    camera_bytes = struct.pack(
        "<QiiQQ4d",
        1,
        1,
        1,
        FORMAL.WIDTH,
        FORMAL.HEIGHT,
        600.0,
        590.0,
        cx,
        FORMAL.HEIGHT / 2.0,
    )
    file_data = {
        view_names[0]: jpeg(),
        view_names[1]: jpeg(),
        "cameras.bin": camera_bytes,
        "images.bin": images_bin(view_names),
    }
    for name, data in file_data.items():
        (source_input / name).write_bytes(data)
    source_spec = FORMAL.CAMERA_AUTHORITY.AuthoritySpec(
        archive_url="https://example.invalid/camera.zip",
        archive=FORMAL.CAMERA_AUTHORITY.FileSpec(
            archive.name, archive.stat().st_size, FORMAL.CAMERA_AUTHORITY.sha256(archive)
        ),
        files=tuple(
            FORMAL.CAMERA_AUTHORITY.FileSpec(name, len(data), digest(data))
            for name, data in file_data.items()
        ),
        image_names=view_names,
        scene_sha256="1" * 64,
        scene_splat_count=2_541_226,
        scene_sh_degree=3,
    )
    source_root = root / "source-authority"
    FORMAL.CAMERA_AUTHORITY.build_authority(
        archive, source_input, source_root, spec=source_spec
    )

    evaluation_input = root / "evaluation-input"
    evaluation_input.mkdir()
    results = json.dumps(
        {"ours_30000": {"SSIM": 0.9, "PSNR": 25.0, "LPIPS": 0.1}}
    ).encode()
    per_view = json.dumps(
        {
            "ours_30000": {
                metric: {"000001.png": value, "000009.png": value}
                for metric, value in (("SSIM", 0.9), ("PSNR", 25.0), ("LPIPS", 0.1))
            }
        }
    ).encode()
    png = rgb_png()
    evaluation_data = {
        "results.json": (results, "application/json"),
        "per_view.json": (per_view, "application/json"),
        "gt/000001.png": (png, "image/png"),
        "gt/000009.png": (png, "image/png"),
        "renders/000001.png": (png, "image/png"),
        "renders/000009.png": (png, "image/png"),
    }
    evaluation_entries = []
    for local_path, (data, media_type) in evaluation_data.items():
        path = evaluation_input / local_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        evaluation_entries.append(
            FORMAL.EVALUATION_AUTHORITY.EntrySpec(
                f"truck/{local_path}", local_path, len(data), digest(data), media_type
            )
        )
    evaluation_root = root / "evaluation-authority"
    FORMAL.EVALUATION_AUTHORITY.build_authority(
        evaluation_input, evaluation_root, tuple(evaluation_entries)
    )
    return Fixture(source_root, evaluation_root, source_spec, tuple(evaluation_entries))


class FormalTruckTraceAuthorityTests(unittest.TestCase):
    def build(self, root: pathlib.Path, fixture: Fixture) -> pathlib.Path:
        output = root / "formal-trace"
        FORMAL.build_output(
            fixture.source_root,
            fixture.evaluation_root,
            output,
            source_spec=fixture.source_spec,
            evaluation_entries=fixture.evaluation_entries,
        )
        return output

    def test_checked_in_fixture_is_v1_and_receipted(self) -> None:
        fixture_root = (
            pathlib.Path(__file__).resolve().parent
            / "trace/fixtures/quality/formal-truck-product-quality-979x546-v1"
        )
        trace = FORMAL.load_json(fixture_root / "camera-trace.json")
        receipt = FORMAL.load_json(fixture_root / "receipt.json")
        FORMAL.validate_trace_v1(trace)
        self.assertEqual(
            trace["content_sha256"],
            "46819f71d5025bb61f6583392448d977051a0b4c67a0c05db860232033c0676c",
        )
        self.assertEqual(receipt["trace"]["content_sha256"], trace["content_sha256"])
        self.assertEqual(
            receipt["trace"]["sha256"],
            FORMAL.CAMERA_AUTHORITY.sha256(fixture_root / "camera-trace.json"),
        )
        self.assertEqual(receipt["qualification"]["product_quality"], "Deferred")

    def test_builds_exact_two_view_calibrated_trace_and_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            output = self.build(root, fixture)
            trace, receipt = FORMAL.validate_output(
                output,
                fixture.source_root,
                fixture.evaluation_root,
                source_spec=fixture.source_spec,
                evaluation_entries=fixture.evaluation_entries,
            )
            self.assertEqual(trace["display"], {"width": 979, "height": 546})
            self.assertEqual(
                [item["view_id"] for item in trace["derivation"]["view_bindings"]],
                ["000001", "000009"],
            )
            for frame in trace["frames"]:
                intrinsics = frame["intrinsics"]
                self.assertAlmostEqual(
                    intrinsics["focal_length_x_over_y"], 600.0 / 590.0
                )
                self.assertEqual(intrinsics["near_plane"], 0.01)
                self.assertEqual(intrinsics["far_plane"], 100.0)
                self.assertAlmostEqual(
                    frame["projection_matrix"][0], 2.0 * 600.0 / 979.0
                )
                self.assertAlmostEqual(
                    frame["projection_matrix"][5], 2.0 * 590.0 / 546.0
                )
            self.assertEqual(
                trace["frames"][0]["pose"]["position"], [-1.0, 2.0, -3.0]
            )
            self.assertEqual(
                trace["derivation"]["clip_policy"]["source"],
                "upstream_3dgs_camera_znear_zfar",
            )
            self.assertEqual(receipt["qualification"]["product_quality"], "Deferred")
            self.assertFalse(receipt["qualification"]["performance_authorized"])
            self.assertEqual(
                receipt["trace"]["sha256"],
                FORMAL.CAMERA_AUTHORITY.sha256(output / "camera-trace.json"),
            )

    def test_trace_binds_every_file_from_both_input_authorities(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            output = self.build(root, fixture)
            trace = FORMAL.load_json(output / "camera-trace.json")
            bindings = trace["derivation"]["input_authorities"]
            self.assertEqual(
                {item["path"] for item in bindings["source_camera"]["files"]},
                {
                    path.relative_to(fixture.source_root).as_posix()
                    for path in fixture.source_root.rglob("*")
                    if path.is_file()
                },
            )
            self.assertEqual(
                {item["path"] for item in bindings["evaluation_images"]["files"]},
                {
                    path.relative_to(fixture.evaluation_root).as_posix()
                    for path in fixture.evaluation_root.rglob("*")
                    if path.is_file()
                },
            )

    def test_existing_output_fails_closed_without_replacement(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            output = self.build(root, fixture)
            before = (output / "camera-trace.json").read_bytes()
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "already exists"):
                self.build(root, fixture)
            self.assertEqual((output / "camera-trace.json").read_bytes(), before)

    def test_output_inside_either_input_authority_fails_before_staging(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            for authority_root in (fixture.source_root, fixture.evaluation_root):
                with self.subTest(authority_root=authority_root.name):
                    before = {
                        path.relative_to(authority_root).as_posix()
                        for path in authority_root.rglob("*")
                    }
                    output = authority_root / "nested-output"
                    with self.assertRaisesRegex(
                        FORMAL.FormalTraceError, "authority root"
                    ):
                        FORMAL.build_output(
                            fixture.source_root,
                            fixture.evaluation_root,
                            output,
                            source_spec=fixture.source_spec,
                            evaluation_entries=fixture.evaluation_entries,
                        )
                    self.assertFalse(output.exists())
                    self.assertEqual(
                        {
                            path.relative_to(authority_root).as_posix()
                            for path in authority_root.rglob("*")
                        },
                        before,
                    )

    def test_wrong_view_set_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root, view_names=("000001.jpg", "000108.jpg"))
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "formal views"):
                self.build(root, fixture)

    def test_off_center_principal_point_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root, cx=489.0)
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "centered"):
                self.build(root, fixture)

    def test_input_drift_invalidates_published_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            output = self.build(root, fixture)
            image = fixture.source_root / "source/000001.jpg"
            image.write_bytes(image.read_bytes() + b"drift")
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "input authority rejected"):
                FORMAL.validate_output(
                    output,
                    fixture.source_root,
                    fixture.evaluation_root,
                    source_spec=fixture.source_spec,
                    evaluation_entries=fixture.evaluation_entries,
                )

    def test_missing_input_file_fails_closed_before_publication(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            missing = fixture.evaluation_root / "source/gt/000009.png"
            missing.rename(root / "removed-000009.png")
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "input authority rejected"):
                self.build(root, fixture)
            self.assertFalse((root / "formal-trace").exists())

    def test_trace_mutation_is_rejected_even_with_recomputed_content_hash(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            output = self.build(root, fixture)
            trace_path = output / "camera-trace.json"
            trace = FORMAL.load_json(trace_path)
            trace["frames"][0]["pose"]["position"][0] += 1.0
            trace = FORMAL.with_content_hash(trace)
            trace_path.write_text(FORMAL.canonical_json(trace) + "\n")
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "bound authority inputs"):
                FORMAL.validate_output(
                    output,
                    fixture.source_root,
                    fixture.evaluation_root,
                    source_spec=fixture.source_spec,
                    evaluation_entries=fixture.evaluation_entries,
                )

    def test_unreceipted_output_file_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = make_fixture(root)
            output = self.build(root, fixture)
            (output / "extra").write_text("not receipted")
            with self.assertRaisesRegex(FORMAL.FormalTraceError, "file set"):
                FORMAL.validate_output(
                    output,
                    fixture.source_root,
                    fixture.evaluation_root,
                    source_spec=fixture.source_spec,
                    evaluation_entries=fixture.evaluation_entries,
                )


if __name__ == "__main__":
    unittest.main()
