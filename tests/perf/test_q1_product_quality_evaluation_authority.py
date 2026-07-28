from __future__ import annotations

import binascii
import hashlib
import importlib.util
import json
import pathlib
import struct
import sys
import tempfile
import unittest
import zlib


MODULE_PATH = pathlib.Path(__file__).with_name("q1_product_quality_evaluation_authority.py")
SPEC = importlib.util.spec_from_file_location(
    "q1_product_quality_evaluation_authority", MODULE_PATH
)
assert SPEC and SPEC.loader
AUTHORITY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = AUTHORITY
SPEC.loader.exec_module(AUTHORITY)


def png(width: int = 979, height: int = 546, color_type: int = 2, interlace: int = 0) -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(
            ">I", binascii.crc32(kind + payload) & 0xFFFFFFFF
        )

    ihdr = struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, interlace)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(b"x"))
        + chunk(b"IEND", b"")
    )


def fixture(root: pathlib.Path) -> tuple[pathlib.Path, tuple]:
    source = root / "input"
    files = {
        "results.json": json.dumps(
            {"ours_30000": {"SSIM": 0.8, "PSNR": 25.0, "LPIPS": 0.1}}
        ).encode(),
        "per_view.json": json.dumps(
            {
                "ours_30000": {
                    metric: {"000001.png": value, "000009.png": value + 0.01}
                    for metric, value in (("SSIM", 0.9), ("PSNR", 26.0), ("LPIPS", 0.1))
                }
            }
        ).encode(),
        "gt/000001.png": png(),
        "gt/000009.png": png(),
        "renders/000001.png": png(),
        "renders/000009.png": png(),
    }
    specs = []
    for relative, data in files.items():
        path = source / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        specs.append(
            AUTHORITY.EntrySpec(
                f"truck/{relative}",
                relative,
                len(data),
                hashlib.sha256(data).hexdigest(),
                "image/png" if relative.endswith(".png") else "application/json",
            )
        )
    return source, tuple(specs)


class EvaluationAuthorityTests(unittest.TestCase):
    def test_build_validates_then_publishes_fresh_immutable_authority(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, specs = fixture(root)
            output = root / "authority"
            receipt = AUTHORITY.build_authority(source, output, specs)
            self.assertEqual(receipt["authority_class"], "upstream_evaluation_images")
            self.assertFalse(receipt["archive"]["archive_sha256_verified"])
            self.assertEqual(receipt["qualification"]["product_quality"], "Deferred")
            self.assertFalse(receipt["qualification"]["performance_authorized"])
            self.assertEqual(AUTHORITY.validate_authority(output, specs), receipt)
            self.assertTrue((output / "source/gt/000001.png").is_file())

    def test_missing_replaced_and_hash_drift_fail_closed(self) -> None:
        for mode in ("missing", "replaced", "hash"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                source, specs = fixture(root)
                target = source / "gt/000001.png"
                if mode == "missing":
                    target.unlink()
                elif mode == "replaced":
                    target.write_bytes(png(width=978))
                else:
                    data = bytearray(target.read_bytes())
                    data[-1] ^= 1
                    target.write_bytes(data)
                with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "missing|identity"):
                    AUTHORITY.build_authority(source, root / "output", specs)

    def test_path_escape_duplicate_and_symlink_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, specs = fixture(root)
            escaped = (AUTHORITY.EntrySpec("truck/../x", "../x", 1, "0" * 64, "image/png"),)
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "path escape"):
                AUTHORITY.validate_specs(escaped)
            duplicate = specs + (specs[0],)
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "duplicate"):
                AUTHORITY.validate_specs(duplicate)
            real = source / "gt/real.png"
            (source / "gt/000001.png").replace(real)
            (source / "gt/000001.png").symlink_to(real.name)
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "symlink"):
                AUTHORITY.build_authority(source, root / "output", specs)

    def test_nonfresh_output_never_overwritten(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, specs = fixture(root)
            output = root / "authority"
            output.mkdir()
            marker = output / "marker"
            marker.write_text("keep")
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "already exists"):
                AUTHORITY.build_authority(source, output, specs)
            self.assertEqual(marker.read_text(), "keep")

    def test_published_tree_rejects_unreceipted_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, specs = fixture(root)
            output = root / "authority"
            AUTHORITY.build_authority(source, output, specs)
            (output / "unexpected").write_text("not receipted")
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "file set"):
                AUTHORITY.validate_authority(output, specs)

    def test_png_dimension_color_type_and_interlace_are_strict(self) -> None:
        for label, data in (
            ("dimensions", png(width=978)),
            ("color", png(color_type=6)),
            ("interlace", png(interlace=1)),
        ):
            with self.subTest(label=label), tempfile.TemporaryDirectory() as directory:
                path = pathlib.Path(directory) / "bad.png"
                path.write_bytes(data)
                with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "RGB8"):
                    AUTHORITY.inspect_rgb8_png(path)

    def test_duplicate_json_keys_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "duplicate.json"
            path.write_text('{"ours_30000":{},"ours_30000":{}}')
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "repeats key"):
                AUTHORITY.load_unique_json(path)

    def test_endpoint_or_self_generated_receipt_cannot_validate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, specs = fixture(root)
            output = root / "authority"
            AUTHORITY.build_authority(source, output, specs)
            receipt_path = output / "authority.json"
            receipt = json.loads(receipt_path.read_text())
            receipt["authority_class"] = "endpoint_generated"
            receipt_path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "class"):
                AUTHORITY.validate_authority(output, specs)

    def test_oversized_receipt_fails_before_json_decode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source, specs = fixture(root)
            output = root / "authority"
            AUTHORITY.build_authority(source, output, specs)
            (output / "authority.json").write_bytes(
                b"{" + b" " * AUTHORITY.MAX_RECEIPT_BYTES + b"}"
            )
            with self.assertRaisesRegex(AUTHORITY.EvaluationAuthorityError, "bounded"):
                AUTHORITY.validate_authority(output, specs)


if __name__ == "__main__":
    unittest.main()
