from __future__ import annotations

import dataclasses
import hashlib
import json
import pathlib
import struct
import sys
import tempfile
import unittest
from unittest import mock


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from q1_product_quality_authority import (  # noqa: E402
    AUTHORITY_CLASS,
    AuthorityError,
    AuthoritySpec,
    FileSpec,
    build_authority,
    validate_authority,
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def jpeg(width: int, height: int) -> bytes:
    sof = bytes([8]) + struct.pack(">HHB", height, width, 3) + bytes(
        [1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]
    )
    return b"\xff\xd8\xff\xc0" + struct.pack(">H", len(sof) + 2) + sof + b"\xff\xd9"


def cameras(*, model_id: int = 1, fx: float = 12.0) -> bytes:
    return (
        struct.pack("<QiiQQ", 1, 7, model_id, 20, 10)
        + struct.pack("<4d", fx, 13.0, 10.0, 5.0)
    )


def images(
    *,
    duplicate: bool = False,
    duplicate_id: bool = False,
    qvec: tuple[float, float, float, float] = (1.0, 0.0, 0.0, 0.0),
) -> bytes:
    records = []
    names = ["000001.jpg", "000001.jpg" if duplicate else "000108.jpg"]
    for image_id, name in enumerate(names, 1):
        stored_image_id = 1 if duplicate_id else image_id
        records.append(
            struct.pack(
                "<i4d3di",
                stored_image_id,
                *qvec,
                0.0,
                0.0,
                float(image_id),
                7,
            )
            + name.encode()
            + b"\0"
            + struct.pack("<Q", 0)
        )
    return struct.pack("<Q", len(records)) + b"".join(records)


class AuthorityFixture:
    def __init__(
        self,
        root: pathlib.Path,
        *,
        model_id: int = 1,
        duplicate: bool = False,
        duplicate_id: bool = False,
        fx: float = 12.0,
        qvec: tuple[float, float, float, float] = (1.0, 0.0, 0.0, 0.0),
    ):
        self.archive = root / "archive.zip"
        self.source = root / "input"
        self.output = root / "authority"
        self.source.mkdir()
        values = {
            "000001.jpg": jpeg(10, 5),
            "000108.jpg": jpeg(10, 5),
            "cameras.bin": cameras(model_id=model_id, fx=fx),
            "images.bin": images(
                duplicate=duplicate, duplicate_id=duplicate_id, qvec=qvec
            ),
        }
        self.archive.write_bytes(b"archive")
        for name, data in values.items():
            (self.source / name).write_bytes(data)
        self.spec = AuthoritySpec(
            archive_url="https://example.invalid/official.zip",
            archive=FileSpec("archive.zip", self.archive.stat().st_size, digest(b"archive")),
            files=tuple(FileSpec(name, len(data), digest(data)) for name, data in values.items()),
            image_names=("000001.jpg", "000108.jpg"),
            scene_sha256="a" * 64,
            scene_splat_count=2,
            scene_sh_degree=3,
        )


class ProductAuthorityTests(unittest.TestCase):
    def test_builds_and_revalidates_two_calibrated_views(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            receipt = build_authority(
                fixture.archive, fixture.source, fixture.output, spec=fixture.spec
            )
            self.assertEqual(receipt["authority_class"], AUTHORITY_CLASS)
            self.assertEqual(
                [view["name"] for view in receipt["views"]],
                ["000001.jpg", "000108.jpg"],
            )
            scale = receipt["views"][0]["intrinsic_scale"]
            self.assertEqual(scale["sx"], {"numerator": 1, "denominator": 2})
            self.assertEqual(scale["sy"], {"numerator": 1, "denominator": 2})
            self.assertEqual(scale["fx"], 6.0)
            self.assertEqual(scale["fy"], 6.5)
            self.assertEqual(
                validate_authority(fixture.output, spec=fixture.spec), receipt
            )

    def test_existing_output_is_never_overwritten(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            fixture.output.mkdir()
            marker = fixture.output / "keep"
            marker.write_text("unchanged")
            with self.assertRaisesRegex(AuthorityError, "already exists"):
                build_authority(
                    fixture.archive,
                    fixture.source,
                    fixture.output,
                    spec=fixture.spec,
                )
            self.assertEqual(marker.read_text(), "unchanged")

    def test_archive_or_source_hash_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            fixture.archive.write_bytes(b"changed")
            with self.assertRaisesRegex(AuthorityError, "byte identity mismatch"):
                build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)

        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            (fixture.source / "000001.jpg").write_bytes(b"changed")
            with self.assertRaisesRegex(AuthorityError, "byte identity mismatch"):
                build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)

    def test_symlink_input_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            source = fixture.source / "000001.jpg"
            real = fixture.source / "real.jpg"
            source.rename(real)
            source.symlink_to(real.name)
            with self.assertRaisesRegex(AuthorityError, "missing regular file"):
                build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)

    def test_symlink_source_directory_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fixture = AuthorityFixture(root)
            real_source = fixture.source.with_name("real-input")
            fixture.source.rename(real_source)
            fixture.source.symlink_to(real_source.name, target_is_directory=True)
            with self.assertRaisesRegex(AuthorityError, "regular directory"):
                build_authority(
                    fixture.archive,
                    fixture.source,
                    fixture.output,
                    spec=fixture.spec,
                )

    def test_wrong_camera_model_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory), model_id=0)
            with self.assertRaisesRegex(AuthorityError, "requires COLMAP PINHOLE"):
                build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)

    def test_duplicate_image_name_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory), duplicate=True)
            with self.assertRaisesRegex(AuthorityError, "duplicate image identity"):
                build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)

    def test_duplicate_image_id_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory), duplicate_id=True)
            with self.assertRaisesRegex(AuthorityError, "duplicate image identity"):
                build_authority(
                    fixture.archive,
                    fixture.source,
                    fixture.output,
                    spec=fixture.spec,
                )

    def test_non_finite_camera_parameter_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory), fx=float("nan"))
            with self.assertRaisesRegex(AuthorityError, "invalid or duplicate camera"):
                build_authority(
                    fixture.archive,
                    fixture.source,
                    fixture.output,
                    spec=fixture.spec,
                )

    def test_non_finite_or_non_unit_pose_is_rejected(self) -> None:
        for qvec in (
            (float("inf"), 0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0, 0.0),
            (2.0, 0.0, 0.0, 0.0),
        ):
            with self.subTest(qvec=qvec), tempfile.TemporaryDirectory() as directory:
                fixture = AuthorityFixture(pathlib.Path(directory), qvec=qvec)
                with self.assertRaisesRegex(AuthorityError, "invalid pose"):
                    build_authority(
                        fixture.archive,
                        fixture.source,
                        fixture.output,
                        spec=fixture.spec,
                    )

    def test_publish_race_never_replaces_existing_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            original_validate = validate_authority

            def validate_then_race(root: pathlib.Path, *, spec: AuthoritySpec):
                receipt = original_validate(root, spec=spec)
                fixture.output.mkdir()
                (fixture.output / "keep").write_text("unchanged")
                return receipt

            module = sys.modules[build_authority.__module__]
            with mock.patch.object(
                module, "validate_authority", side_effect=validate_then_race
            ):
                with self.assertRaisesRegex(AuthorityError, "already exists"):
                    build_authority(
                        fixture.archive,
                        fixture.source,
                        fixture.output,
                        spec=fixture.spec,
                    )
            self.assertEqual((fixture.output / "keep").read_text(), "unchanged")

    def test_mutated_authority_class_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            receipt_path = fixture.output / "authority.json"
            receipt = json.loads(receipt_path.read_text())
            receipt["authority_class"] = "gsplat_rs_endpoint_output"
            receipt_path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(AuthorityError, "schema or class mismatch"):
                validate_authority(fixture.output, spec=fixture.spec)

    def test_mutated_camera_receipt_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            receipt_path = fixture.output / "authority.json"
            receipt = json.loads(receipt_path.read_text())
            receipt["views"][0]["colmap"]["tvec"][0] = 99.0
            receipt_path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(AuthorityError, "retained source"):
                validate_authority(fixture.output, spec=fixture.spec)

    def test_equal_numeric_value_with_wrong_json_type_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            receipt_path = fixture.output / "authority.json"
            receipt = json.loads(receipt_path.read_text())
            receipt["views"][0]["colmap"]["camera"]["width"] = 20.0
            receipt_path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(AuthorityError, "retained source"):
                validate_authority(fixture.output, spec=fixture.spec)

    def test_oversized_or_invalid_json_receipt_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            receipt_path = fixture.output / "authority.json"
            receipt_path.write_bytes(b"{" + b" " * (64 * 1024) + b"}")
            with self.assertRaisesRegex(AuthorityError, "bounded receipt size"):
                validate_authority(fixture.output, spec=fixture.spec)

    def test_duplicate_nested_json_key_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            receipt_path = fixture.output / "authority.json"
            encoded = receipt_path.read_text()
            encoded = encoded.replace(
                '"splat_count": 2', '"splat_count": 999, "splat_count": 2', 1
            )
            receipt_path.write_text(encoded)
            with self.assertRaisesRegex(AuthorityError, "repeats key 'splat_count'"):
                validate_authority(fixture.output, spec=fixture.spec)

    def test_copied_source_drift_and_extra_files_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            copied = fixture.output / "source" / "000108.jpg"
            copied.write_bytes(b"changed")
            with self.assertRaisesRegex(AuthorityError, "byte identity mismatch"):
                validate_authority(fixture.output, spec=fixture.spec)

        with tempfile.TemporaryDirectory() as directory:
            fixture = AuthorityFixture(pathlib.Path(directory))
            build_authority(fixture.archive, fixture.source, fixture.output, spec=fixture.spec)
            (fixture.output / "unexpected").write_text("no")
            with self.assertRaisesRegex(AuthorityError, "file set mismatch"):
                validate_authority(fixture.output, spec=fixture.spec)


if __name__ == "__main__":
    unittest.main()
