from __future__ import annotations

import copy
import hashlib
import json
import pathlib
import struct
import sys
import tempfile
import unittest
import zlib
from unittest import mock


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import q1_product_quality_smoke as SMOKE  # noqa: E402


WIDTH = SMOKE.FORMAL_WIDTH
HEIGHT = SMOKE.FORMAL_HEIGHT
PIXELS = WIDTH * HEIGHT


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def rgba_png(rgba: bytes) -> bytes:
    rows = b"".join(
        b"\x00" + rgba[y * WIDTH * 4 : (y + 1) * WIDTH * 4]
        for y in range(HEIGHT)
    )
    header = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + b"".join(
        (
            png_chunk(b"IHDR", header),
            png_chunk(b"IDAT", zlib.compress(rows, 1)),
            png_chunk(b"IEND", b""),
        )
    )


def exactness() -> dict[str, object]:
    return {
        "source_splat_count": SMOKE.TRUCK_SPLAT_COUNT,
        "decoded_splat_count": SMOKE.TRUCK_SPLAT_COUNT,
        "resident_splat_count": SMOKE.TRUCK_SPLAT_COUNT,
        "source_sh_degree": SMOKE.TRUCK_SH_DEGREE,
        "resident_sh_degree": SMOKE.TRUCK_SH_DEGREE,
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "partial_scene_published": False,
        "full_quality": True,
    }


def resolution(*, presented: bool) -> dict[str, object]:
    result: dict[str, object] = {
        "requested_width": WIDTH,
        "requested_height": HEIGHT,
        "surface_width": WIDTH,
        "surface_height": HEIGHT,
        "internal_render_width": WIDTH,
        "internal_render_height": HEIGHT,
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "internal_full_resolution": True,
    }
    if presented:
        result.update(
            {
                "presented_width": WIDTH,
                "presented_height": HEIGHT,
                "full_resolution": True,
            }
        )
    return result


def formal() -> dict[str, object]:
    intrinsics = {
        "vertical_fov_radians": 0.881621552836618,
        "near_plane": 0.01,
        "far_plane": 100.0,
        "focal_length_x_over_y": 1.005624011459175,
    }
    pose = {
        "position": [3.398962270047007, -0.6835340434263679, -2.29915441055627],
        "rotation_xyzw": [
            -0.034582275508909785,
            -0.3269484852594975,
            0.0032672478788591,
            0.9444035574363557,
        ],
    }
    projection = [0.0] * 16
    projection[0] = 1.188814234062581
    projection[5] = 2.119670769914365
    return {
        "trace": {
            "trace_id": SMOKE.TRACE_AUTHORITY.TRACE_ID,
            "content_sha256": "1" * 64,
            "frames": [
                {
                    "frame_index": 0,
                    "pose": pose,
                    "intrinsics": intrinsics,
                    "projection_matrix": projection,
                }
            ],
        },
        "trace_file_sha256": "2" * 64,
        "trace_receipt_sha256": "3" * 64,
        "evaluation_receipt_sha256": "4" * 64,
        "pose_intrinsics_sha256": SMOKE.canonical_sha256(
            {"pose": pose, "intrinsics": intrinsics}
        ),
        "focal_length_x_over_y": intrinsics["focal_length_x_over_y"],
    }


def common_manifest(formal_value: dict[str, object]) -> dict[str, object]:
    return {
        "schema": SMOKE.NATIVE_ARTIFACT_SCHEMA,
        "dataset": {
            "id": SMOKE.TRUCK_ID,
            "sha256": SMOKE.TRUCK_SHA256,
            "splat_count": SMOKE.TRUCK_SPLAT_COUNT,
            "sh_degree": SMOKE.TRUCK_SH_DEGREE,
        },
        "exactness": exactness(),
        "resolution": resolution(presented=True),
        "trace": {
            "id": formal_value["trace"]["trace_id"],
            "sha256": formal_value["trace"]["content_sha256"],
            "capture_frame_index": 0,
        },
    }


def write_native(root: pathlib.Path, rgba: bytes, formal_value: dict[str, object]) -> None:
    root.mkdir()
    capture = {
        "scene_generation": 1,
        "camera_revision": 9,
        "viewport_generation": 0,
        "contract_generation": 1,
        "plan_set_generation": 2,
        "plan_id": "GpuPreproject",
        "order_generation": 10,
        "presentation_sequence": 11,
        "width": WIDTH,
        "height": HEIGHT,
        "rgba8_sha256": hashlib.sha256(rgba).hexdigest(),
        "profile": "ExactFull32",
    }
    frame = {
        "trace_frame_index": 0,
        "camera_revision": 9,
        "presentation_sequence": 11,
        "capture_depth_precision": capture,
    }
    manifest = common_manifest(formal_value)
    manifest["q1_comparison"] = {
        "artifact_role": "control",
        "performance_evidence": False,
        "presentation_identity": {
            "trace_frame_index": 0,
            "camera": {
                "trace_id": formal_value["trace"]["trace_id"],
                "trace_content_sha256": formal_value["trace"]["content_sha256"],
                "trace_frame_index": 0,
                "pose_intrinsics_sha256": formal_value["pose_intrinsics_sha256"],
                "camera_revision": 9,
            },
            "terminal_identity": {
                "frame_index": 0,
                "frame_sha256": SMOKE.canonical_sha256(frame),
                "presentation_sequence": 11,
            },
            "successful_present": True,
            "queue_terminal_complete": True,
            "captured_after_terminal": True,
            "dimensions": resolution(presented=True),
        },
    }
    (root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    (root / "frames.jsonl").write_text(json.dumps(frame) + "\n", encoding="utf-8")
    (root / "final-frame.png").write_bytes(rgba_png(rgba))


def playcanvas_camera(formal_value: dict[str, object]) -> dict[str, object]:
    frame = formal_value["trace"]["frames"][0]
    projection = frame["projection_matrix"]
    configured = [0.0] * 16
    configured[0] = projection[0]
    configured[5] = projection[5]
    return {
        "schema": SMOKE.PLAYCANVAS_CAMERA_SCHEMA,
        "trace_frame_index": 0,
        "phase": "presentation_frame_2",
        "vertical_fov_radians": frame["intrinsics"]["vertical_fov_radians"],
        "near_plane": frame["intrinsics"]["near_plane"],
        "far_plane": frame["intrinsics"]["far_plane"],
        "aspect": WIDTH / HEIGHT,
        "focal_length_x_over_y": frame["intrinsics"]["focal_length_x_over_y"],
        "horizontal_fov": False,
        "render_target_flip_y": False,
        "webgpu_depth_range_applied": True,
        "custom_projection": {
            "active": True,
            "hook": "CameraComponent.calculateProjection",
            "configured_projection_matrix_opengl_column_major": configured,
        },
        "validation": {"passed": True},
    }


def write_playcanvas(root: pathlib.Path, rgba: bytes, formal_value: dict[str, object]) -> None:
    root.mkdir()
    camera = playcanvas_camera(formal_value)
    camera_json = json.dumps(camera, separators=(",", ":"))
    rgba_sha = hashlib.sha256(rgba).hexdigest()
    capture = {
        "schema": SMOKE.PLAYCANVAS_CAPTURE_SCHEMA,
        "producer": SMOKE.PLAYCANVAS_CAPTURE_PRODUCER,
        "status": "terminal",
        "renderer_frame_sequence": 8,
        "renderer_submit_version": 9,
        "copy_submit_version_before": 9,
        "copy_submit_version_after": 10,
        "texture_format": "bgra8unorm",
        "render_view_format": "bgra8unorm",
        "canvas_color_space": "srgb",
        "canvas_alpha_mode": "premultiplied",
        "pixel_format": "rgba8unorm",
        "row_origin": "top_left",
        "width": WIDTH,
        "height": HEIGHT,
        "row_bytes": WIDTH * 4,
        "byte_length": len(rgba),
        "rgba8_sha256": rgba_sha,
        "camera_receipt_sha256": hashlib.sha256(camera_json.encode()).hexdigest(),
        "camera_receipt_json": camera_json,
        "camera_receipt": camera,
        "resolution": resolution(presented=False),
        "source": {
            "dataset_id": SMOKE.TRUCK_ID,
            "dataset_sha256": SMOKE.TRUCK_SHA256,
            "source_splat_count": SMOKE.TRUCK_SPLAT_COUNT,
            "decoded_splat_count": SMOKE.TRUCK_SPLAT_COUNT,
            "resident_splat_count": SMOKE.TRUCK_SPLAT_COUNT,
            "source_sh_degree": SMOKE.TRUCK_SH_DEGREE,
            "resident_sh_degree": SMOKE.TRUCK_SH_DEGREE,
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "partial_scene_published": False,
            "full_quality": True,
        },
        "copy_map_complete": True,
        "queue_terminal_complete": True,
        "terminal_queue_drain": {
            "phase": "post_capture_presentation",
            "submit_version_before": 10,
            "submit_version_after": 10,
            "submit_version_stable": True,
        },
    }
    png = rgba_png(rgba)
    manifest = common_manifest(formal_value)
    manifest["renderer_capture"] = capture
    manifest["renderer_capture_materialization"] = {
        "schema": SMOKE.PLAYCANVAS_MATERIALIZATION_SCHEMA,
        "source": "host_png_from_renderer_owned_webgpu_rgba8",
        "source_capture_schema": SMOKE.PLAYCANVAS_CAPTURE_SCHEMA,
        "source_capture_producer": SMOKE.PLAYCANVAS_CAPTURE_PRODUCER,
        "source_rgba8_sha256": rgba_sha,
        "rgba8_file": "final-frame.rgba8",
        "rgba8_byte_length": len(rgba),
        "png_file": "final-frame.png",
        "png_byte_length": len(png),
        "png_sha256": hashlib.sha256(png).hexdigest(),
        "width": WIDTH,
        "height": HEIGHT,
    }
    (root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    (root / "final-frame.rgba8").write_bytes(rgba)
    (root / "final-frame.png").write_bytes(png)


class ProductQualitySmokeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.formal = formal()
        self.source_rgb = bytes((80, 120, 160)) * PIXELS
        self.good_rgba = bytes((80, 120, 160, 255)) * PIXELS

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def evaluate(self, native: bytes | None = None, playcanvas: bytes | None = None) -> dict[str, object]:
        native_root = self.root / "native"
        playcanvas_root = self.root / "playcanvas"
        write_native(native_root, native or self.good_rgba, self.formal)
        write_playcanvas(playcanvas_root, playcanvas or self.good_rgba, self.formal)
        with (
            mock.patch.object(SMOKE, "_formal_trace", return_value=self.formal),
            mock.patch.object(
                SMOKE,
                "_ground_truth",
                return_value=(self.source_rgb, {"sha256": "b" * 64}),
            ),
        ):
            return SMOKE.evaluate_one_view(
                formal_trace_authority=self.root / "unused-trace",
                evaluation_authority=self.root / "unused-evaluation",
                gsplat_capture=native_root,
                playcanvas_capture=playcanvas_root,
            )

    def test_two_renderer_owned_exact_images_accept_one_view_only(self) -> None:
        result = self.evaluate()
        self.assertEqual(result["status"], "Accepted")
        self.assertEqual(result["endpoints"]["gsplat_rs"]["state"], "Accepted")
        self.assertEqual(result["endpoints"]["playcanvas"]["state"], "Accepted")
        self.assertEqual(result["qualification"]["product_quality"], "Deferred")
        self.assertFalse(result["qualification"]["performance_eligible"])
        text = json.dumps(result, sort_keys=True)
        for forbidden in ("frame_wall", "fps", "winner", "pairing", "throughput"):
            self.assertNotIn(forbidden, text.lower())

    def test_quality_miss_is_a_finite_rejected_result(self) -> None:
        rejected = bytes((0, 0, 0, 255)) * PIXELS
        result = self.evaluate(playcanvas=rejected)
        self.assertEqual(result["status"], "Rejected")
        self.assertEqual(result["endpoints"]["gsplat_rs"]["state"], "Accepted")
        self.assertEqual(result["endpoints"]["playcanvas"]["state"], "Rejected")

    def test_nonopaque_endpoint_fails_closed_instead_of_scoring(self) -> None:
        malformed = bytearray(self.good_rgba)
        malformed[3] = 0
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "alpha"):
            self.evaluate(playcanvas=bytes(malformed))

    def test_native_camera_hash_drift_fails_closed(self) -> None:
        native = self.root / "native"
        write_native(native, self.good_rgba, self.formal)
        manifest_path = native / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["q1_comparison"]["presentation_identity"]["camera"][
            "pose_intrinsics_sha256"
        ] = "0" * 64
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "pose_intrinsics"):
            SMOKE._native_capture(native, self.formal)

    def test_native_png_must_match_same_present_capture_hash(self) -> None:
        native = self.root / "native"
        write_native(native, self.good_rgba, self.formal)
        (native / "final-frame.png").write_bytes(
            rgba_png(bytes((1, 2, 3, 255)) * PIXELS)
        )
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "renderer-owned"):
            SMOKE._native_capture(native, self.formal)

    def test_playcanvas_producer_and_materialized_bytes_are_required(self) -> None:
        playcanvas = self.root / "playcanvas"
        write_playcanvas(playcanvas, self.good_rgba, self.formal)
        manifest_path = playcanvas / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["renderer_capture"]["producer"] = "canvas_to_data_url"
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "terminal identity"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_playcanvas_calibrated_ratio_is_bound_by_camera_json_hash(self) -> None:
        playcanvas = self.root / "playcanvas"
        write_playcanvas(playcanvas, self.good_rgba, self.formal)
        manifest_path = playcanvas / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        camera = manifest["renderer_capture"]["camera_receipt"]
        camera["focal_length_x_over_y"] = 1.0
        camera_json = json.dumps(camera, separators=(",", ":"))
        manifest["renderer_capture"]["camera_receipt_json"] = camera_json
        manifest["renderer_capture"]["camera_receipt_sha256"] = hashlib.sha256(
            camera_json.encode()
        ).hexdigest()
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "focal_length_x_over_y"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_publication_is_fresh_atomic_and_rejects_performance_fields(self) -> None:
        result = self.evaluate()
        output = self.root / "published"
        SMOKE.publish_one_view(result, output)
        self.assertEqual(
            json.loads((output / "result.json").read_text())["status"], "Accepted"
        )
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "already exists"):
            SMOKE.publish_one_view(result, output)
        mutated = copy.deepcopy(result)
        mutated["timing"] = {"frame_ms": 1.0}
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "forbidden"):
            SMOKE.validate_result(mutated)


if __name__ == "__main__":
    unittest.main()
