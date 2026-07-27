#!/usr/bin/env python3

from __future__ import annotations

import binascii
import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zlib


SCRIPT = Path(__file__).with_name("collect-q1-truck-direct-reference.py")
SPEC = importlib.util.spec_from_file_location("q1_direct_reference", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", binascii.crc32(kind + payload) & 0xFFFFFFFF)
    )


def rgba_png(width: int, height: int, rgba: bytes) -> bytes:
    rows = b"".join(
        b"\0" + rgba[row * width * 4 : (row + 1) * width * 4]
        for row in range(height)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows))
        + png_chunk(b"IEND", b"")
    )


def valid_receipt() -> str:
    values = {
        "schema": COLLECTOR.RECEIPT_SCHEMA,
        "geometry_path": "sorted_index_direct",
        "representation": "wide_f32",
        "render_mode": "sorted_alpha",
        "order_backend": "cpu",
        "depth_key_precision": "exact_full32",
        "stable_source_id_order": "true",
        "raster_execution_plan": "wgpu_direct_global_quads",
        "gpu_rasterizer": "true",
        "adapter_backend": "Metal",
        "adapter_device_type": "IntegratedGpu",
        "adapter_vendor": "0",
        "adapter_device": "0",
        "source_count": str(COLLECTOR.EXPECTED_SPLATS),
        "decoded_count": str(COLLECTOR.EXPECTED_SPLATS),
        "encoded_count": str(COLLECTOR.EXPECTED_SPLATS),
        "resident_count": str(COLLECTOR.EXPECTED_SPLATS),
        "addressable_count": str(COLLECTOR.EXPECTED_SPLATS),
        "source_sh_degree": "3",
        "resident_sh_degree": "3",
        "requested_width": "1920",
        "requested_height": "1080",
        "internal_render_width": "1920",
        "internal_render_height": "1080",
        "readback_width": "1920",
        "readback_height": "1080",
        "readback_format": "rgba8_unorm",
        "readback_row_origin": "top_left",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "partial_scene_published": "false",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "visible_count": "123",
        "drawn_count": "123",
    }
    return COLLECTOR.RECEIPT_PREFIX + " ".join(f"{key}={value}" for key, value in values.items())


class CollectorTests(unittest.TestCase):
    def test_renderer_receipt_accepts_exact_direct_and_fails_closed(self) -> None:
        receipt = COLLECTOR.parse_receipt(valid_receipt())
        self.assertEqual(receipt["visible_count"], "123")
        with self.assertRaisesRegex(COLLECTOR.ReferenceError, "depth_key_precision"):
            COLLECTOR.parse_receipt(valid_receipt().replace("exact_full32", "candidate24"))
        with self.assertRaisesRegex(COLLECTOR.ReferenceError, "drawn=visible"):
            COLLECTOR.parse_receipt(valid_receipt().replace("drawn_count=123", "drawn_count=122"))

    def test_trace_freezes_both_pose_and_intrinsics(self) -> None:
        trace, receipt = COLLECTOR.frozen_trace()
        COLLECTOR.validate_matrix_authority(
            {
                "sha256": COLLECTOR.EXPECTED_TRUCK_SHA256,
                "bytes": COLLECTOR.EXPECTED_TRUCK_BYTES,
            },
            trace,
        )
        self.assertEqual([frame["frame_index"] for frame in trace["frames"]], [0, 1])
        self.assertEqual(len(receipt["frames"]), 2)
        for frame in receipt["frames"]:
            self.assertEqual(len(frame["pose_intrinsics_sha256"]), 64)

    def test_rgba_decoder_rejects_wrong_formal_size(self) -> None:
        data = rgba_png(1, 1, bytes([1, 2, 3, 4]))
        with self.assertRaisesRegex(COLLECTOR.ReferenceError, "1920x1080"):
            COLLECTOR.decode_rgba8_png(data)

    def test_output_transaction_publishes_success_or_blocker_atomically(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "success"
            transaction = COLLECTOR.OutputTransaction(output)
            COLLECTOR.write_json(transaction.stage / "reference.json", {"schema": COLLECTOR.SIDECAR_SCHEMA})
            transaction.publish()
            self.assertTrue((output / "reference.json").is_file())
            self.assertFalse(transaction.stage.exists())

            rejected = root / "rejected"
            failed = COLLECTOR.OutputTransaction(rejected)
            COLLECTOR.write_json(failed.stage / "reference.json", {"status": "incomplete"})
            failed.reject(COLLECTOR.ReferenceError("intentional"))
            self.assertTrue((rejected / "blocker.json").is_file())
            self.assertFalse((rejected / "reference.json").exists())
            self.assertEqual(json.loads((rejected / "blocker.json").read_text())["status"], "rejected")

    def test_output_root_must_be_fresh(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            existing = Path(directory) / "existing"
            existing.mkdir()
            with self.assertRaisesRegex(COLLECTOR.ReferenceError, "already exists"):
                COLLECTOR.OutputTransaction(existing)


if __name__ == "__main__":
    unittest.main()
