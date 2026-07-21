#!/usr/bin/env python3
"""Focused tests for the external-scene and deterministic-ladder tools."""

from __future__ import annotations

import hashlib
import http.server
import json
import pathlib
import socketserver
import struct
import sys
import tempfile
import threading
import unittest
import zipfile


DATASET_TOOLS = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(DATASET_TOOLS))

import fetch_inria_3dgs_scenes as inria  # noqa: E402
import ply_ladder  # noqa: E402


def binary_ply(vertex_count: int = 10) -> tuple[bytes, bytes]:
    header = (
        "ply\n"
        "format binary_little_endian 1.0\n"
        "comment fixture header must be retained\n"
        f"element vertex {vertex_count}\n"
        "property int id\n"
        "property float x\n"
        "element metadata 2\n"
        "property ushort tag\n"
        "end_header\n"
    ).encode("ascii")
    vertices = b"".join(struct.pack("<if", index, index + 0.25) for index in range(vertex_count))
    suffix = struct.pack("<HH", 17, 29)
    return header + vertices + suffix, suffix


def scene_binary_ply(vertex_count: int = 3) -> bytes:
    properties = [
        "x",
        "y",
        "z",
        "opacity",
        "scale_0",
        "scale_1",
        "scale_2",
        "rot_0",
        "rot_1",
        "rot_2",
        "rot_3",
        "f_dc_0",
        "f_dc_1",
        "f_dc_2",
    ]
    header = [
        "ply",
        "format binary_little_endian 1.0",
        f"element vertex {vertex_count}",
        *[f"property float {name}" for name in properties],
        "end_header",
        "",
    ]
    row = struct.Struct(f"<{len(properties)}f")
    return "\n".join(header).encode("ascii") + b"".join(
        row.pack(*([float(index)] * len(properties))) for index in range(vertex_count)
    )


class DatasetToolTests(unittest.TestCase):
    def test_official_scene_catalog_has_complete_identity_pins(self) -> None:
        self.assertEqual(set(inria.SCENES), {"bonsai", "truck", "garden", "bicycle"})
        for scene, value in inria.SCENES.items():
            self.assertRegex(value["sha256"], r"^[0-9a-f]{64}$", msg=scene)
            self.assertGreater(value["bytes"], 0, msg=scene)
            self.assertGreater(value["compressed_bytes"], 0, msg=scene)
            self.assertRegex(value["archive_crc32"], r"^[0-9a-f]{8}$", msg=scene)
            self.assertGreater(value["splat_count"], 0, msg=scene)

    def test_midpoint_ladder_is_deterministic_and_preserves_layout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source = root / "scene.ply"
            source_bytes, suffix = binary_ply()
            source.write_bytes(source_bytes)
            source_manifest = root / "source.json"
            source_manifest.write_text(
                json.dumps(
                    {
                        "schema": "fixture/v1",
                        "id": "fixture",
                        "local_path": str(source),
                        "license": "fixture-only",
                        "allowed_use": "fixture tests only",
                        "sha256": hashlib.sha256(source_bytes).hexdigest(),
                        "bytes": len(source_bytes),
                        "splat_count": 10,
                    }
                ),
                encoding="utf-8",
            )
            output = root / "ladder"
            manifest = ply_ladder.generate_ladder(
                source,
                [7, 4, 4],
                output,
                source_manifest_path=source_manifest,
            )
            four = output / "scene-n4.ply"
            layout = ply_ladder.inspect_binary_ply(four)
            self.assertEqual(layout.vertex.count, 4)
            self.assertEqual(layout.vertex.property_names, ("id", "x"))
            source_header = ply_ladder.inspect_binary_ply(source).header
            self.assertEqual(
                layout.header,
                source_header.replace(b"element vertex 10", b"element vertex 4"),
            )
            self.assertEqual(four.read_bytes()[-len(suffix) :], suffix)

            with four.open("rb") as handle:
                handle.seek(layout.vertex_data_offset)
                ids = [struct.unpack("<if", handle.read(8))[0] for _ in range(4)]
            self.assertEqual(ids, [1, 3, 6, 8])
            self.assertEqual(
                manifest["selection"]["algorithm"],
                ply_ladder.SELECTION_ALGORITHM,
            )
            self.assertEqual(
                manifest["source"]["provenance"]["license"], "fixture-only"
            )
            self.assertEqual(
                manifest["source"]["provenance"]["allowed_use"],
                "fixture tests only",
            )

            hashes_before = [tier["sha256"] for tier in manifest["tiers"]]
            manifest_bytes_before = (output / "ladder.json").read_bytes()
            second = ply_ladder.generate_ladder(
                source,
                [4, 7],
                output,
                source_manifest_path=source_manifest,
                overwrite=True,
            )
            self.assertEqual(hashes_before, [tier["sha256"] for tier in second["tiers"]])
            self.assertEqual(manifest_bytes_before, (output / "ladder.json").read_bytes())

    def test_ladder_rejects_unsupported_or_mismatched_input(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            ascii_path = root / "ascii.ply"
            ascii_path.write_text(
                "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nend_header\n0\n",
                encoding="ascii",
            )
            with self.assertRaisesRegex(ply_ladder.PlyLadderError, "input must use binary"):
                ply_ladder.inspect_binary_ply(ascii_path)

            source = root / "scene.ply"
            source_bytes, _ = binary_ply()
            source.write_bytes(source_bytes)
            bad_manifest = root / "bad.json"
            bad_manifest.write_text(
                json.dumps(
                    {
                        "local_path": str(source),
                        "sha256": "0" * 64,
                        "bytes": len(source_bytes),
                        "splat_count": 10,
                    }
                ),
                encoding="utf-8",
            )
            output = root / "bad-ladder"
            with self.assertRaisesRegex(ply_ladder.PlyLadderError, "source manifest mismatch"):
                ply_ladder.generate_ladder(
                    source,
                    [4],
                    output,
                    source_manifest_path=bad_manifest,
                )
            self.assertFalse((output / "scene-n4.ply").exists())

    def test_zip64_extra_field_resolution(self) -> None:
        extra = struct.pack("<HHQQQ", 0x0001, 24, 1000, 800, 123456)
        self.assertEqual(
            inria._zip64_values(extra, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0),
            (1000, 800, 123456),
        )

    def test_range_reader_refuses_a_full_archive_response(self) -> None:
        payload = b"an archive body that must not be accepted whole"
        with socketserver.TCPServer(("127.0.0.1", 0), no_range_handler(payload)) as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            url = f"http://127.0.0.1:{server.server_address[1]}/models.zip"
            with self.assertRaisesRegex(inria.DownloadError, "ignored range request"):
                inria._read_range(url, 0, 4)
            server.shutdown()
            thread.join(timeout=5)

    def test_range_extraction_streams_and_records_identity(self) -> None:
        scene_bytes = scene_binary_ply()
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            archive = root / "models.zip"
            archive_entry = (
                "mipnerf360/bonsai/point_cloud/iteration_30000/point_cloud.ply"
            )
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as handle:
                handle.writestr(archive_entry, scene_bytes)
                handle.writestr("unrelated/readme.txt", b"not selected")

            handler = range_handler(archive.read_bytes())
            with socketserver.TCPServer(("127.0.0.1", 0), handler) as server:
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                url = f"http://127.0.0.1:{server.server_address[1]}/models.zip"
                archive_size, entries = inria.read_zip_entries(url)
                self.assertEqual(archive_size, archive.stat().st_size)
                entry = inria._scene_entry("bonsai", entries)
                expected = {
                    "family": "fixture",
                    "upstream_dataset_url": "https://example.invalid/dataset",
                    "sha256": hashlib.sha256(scene_bytes).hexdigest(),
                    "bytes": len(scene_bytes),
                    "compressed_bytes": entry.compressed_bytes,
                    "archive_crc32": f"{entry.crc32:08x}",
                    "splat_count": 3,
                }
                inria._validate_entry("bonsai", entry, expected)
                manifest = inria.download_scene(
                    url,
                    "bonsai",
                    entry,
                    root / "output",
                    expected,
                )
                reused = inria.download_scene(
                    url,
                    "bonsai",
                    entry,
                    root / "output",
                    expected,
                )
                server.shutdown()
                thread.join(timeout=5)

            output = root / "output/bonsai/point_cloud.ply"
            self.assertEqual(output.read_bytes(), scene_bytes)
            self.assertEqual(manifest["sha256"], expected["sha256"])
            self.assertEqual(reused, manifest)
            self.assertEqual(manifest["identity_status"], "verified-pinned")
            self.assertEqual(manifest["license"], "NOASSERTION")
            self.assertEqual(
                json.loads((output.parent / "source.json").read_text(encoding="utf-8")),
                manifest,
            )


def range_handler(payload: bytes):
    class RangeHandler(http.server.BaseHTTPRequestHandler):
        def do_HEAD(self) -> None:  # noqa: N802
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()

        def do_GET(self) -> None:  # noqa: N802
            value = self.headers.get("Range", "")
            match = __import__("re").fullmatch(r"bytes=([0-9]+)-([0-9]+)", value)
            if match is None:
                self.send_response(416)
                self.end_headers()
                return
            start, end = (int(match.group(1)), int(match.group(2)))
            if start < 0 or end < start or end >= len(payload):
                self.send_response(416)
                self.end_headers()
                return
            body = payload[start : end + 1]
            self.send_response(206)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(payload)}")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *args: object) -> None:
            del args

    return RangeHandler


def no_range_handler(payload: bytes):
    class NoRangeHandler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            try:
                self.wfile.write(payload)
            except BrokenPipeError:
                pass

        def log_message(self, _format: str, *args: object) -> None:
            del args

    return NoRangeHandler


if __name__ == "__main__":
    unittest.main()
