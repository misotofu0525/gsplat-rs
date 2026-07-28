from __future__ import annotations

import copy
import hashlib
import json
import math
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


def filtered_row(raw: bytes, previous: bytes, bytes_per_pixel: int, filter_type: int) -> bytes:
    encoded = bytearray(len(raw))
    for index, value in enumerate(raw):
        left = raw[index - bytes_per_pixel] if index >= bytes_per_pixel else 0
        above = previous[index] if previous else 0
        upper_left = (
            previous[index - bytes_per_pixel]
            if previous and index >= bytes_per_pixel
            else 0
        )
        if filter_type == 0:
            predictor = 0
        elif filter_type == 1:
            predictor = left
        elif filter_type == 4:
            predictor = SMOKE.PNG.paeth_predictor(left, above, upper_left)
        else:
            raise ValueError(filter_type)
        encoded[index] = (value - predictor) & 0xFF
    return bytes(encoded)


def rgb_png(rgb: bytes, *, filter_type: int = 0) -> bytes:
    rows = []
    previous = b""
    for y in range(HEIGHT):
        raw = rgb[y * WIDTH * 3 : (y + 1) * WIDTH * 3]
        rows.append(bytes((filter_type,)) + filtered_row(raw, previous, 3, filter_type))
        previous = raw
    header = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + b"".join(
        (
            png_chunk(b"IHDR", header),
            png_chunk(b"IDAT", zlib.compress(b"".join(rows), 1)),
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


def presentation_dimensions() -> dict[str, int]:
    return {
        f"{stage}_{axis}": value
        for stage in ("requested", "surface", "internal_render", "presented")
        for axis, value in (("width", WIDTH), ("height", HEIGHT))
    }


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
    q = pose["rotation_xyzw"]
    right = SMOKE._rotate_vector(q, [1.0, 0.0, 0.0])
    up = SMOKE._rotate_vector(q, [0.0, 1.0, 0.0])
    forward = SMOKE._rotate_vector(q, [0.0, 0.0, 1.0])
    position = pose["position"]
    view = [
        *right, -sum(a * b for a, b in zip(right, position)),
        *up, -sum(a * b for a, b in zip(up, position)),
        *forward, -sum(a * b for a, b in zip(forward, position)),
        0.0, 0.0, 0.0, 1.0,
    ]
    focal = 1.0 / math.tan(intrinsics["vertical_fov_radians"] * 0.5)
    depth = intrinsics["far_plane"] / (
        intrinsics["far_plane"] - intrinsics["near_plane"]
    )
    projection = [
        focal * intrinsics["focal_length_x_over_y"] / (WIDTH / HEIGHT),
        0.0, 0.0, 0.0,
        0.0, focal, 0.0, 0.0,
        0.0, 0.0, depth, -intrinsics["near_plane"] * depth,
        0.0, 0.0, 1.0, 0.0,
    ]
    return {
        "trace": {
            "trace_id": SMOKE.TRACE_AUTHORITY.TRACE_ID,
            "content_sha256": "1" * 64,
            "frames": [
                {
                    "frame_index": 0,
                    "pose": pose,
                    "intrinsics": intrinsics,
                    "view_matrix": view,
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
        "camera_revision": 0,
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
        "camera_revision": 0,
        "presentation_sequence": 11,
        "capture_depth_precision": capture,
    }
    manifest = common_manifest(formal_value)
    formal_frame = formal_value["trace"]["frames"][0]
    runtime_camera = {
        "position": formal_frame["pose"]["position"],
        "rotationXyzw": formal_frame["pose"]["rotation_xyzw"],
        "intrinsics": {
            "verticalFovRadians": formal_frame["intrinsics"]["vertical_fov_radians"],
            "nearPlane": formal_frame["intrinsics"]["near_plane"],
            "farPlane": formal_frame["intrinsics"]["far_plane"],
            "focalLengthXOverY": formal_frame["intrinsics"]["focal_length_x_over_y"],
        },
    }
    manifest["camera_receipt"] = runtime_camera
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
                "camera_revision": 0,
                "runtime_camera_receipt_sha256": SMOKE.canonical_sha256(runtime_camera),
            },
            "terminal_identity": {
                "frame_index": 0,
                "frame_sha256": SMOKE.canonical_sha256(frame),
                "presentation_sequence": 11,
            },
            "successful_present": True,
            "queue_terminal_complete": True,
            "captured_after_terminal": True,
            "dimensions": presentation_dimensions(),
        },
    }
    (root / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    (root / "frames.jsonl").write_text(json.dumps(frame) + "\n", encoding="utf-8")
    (root / "final-frame.png").write_bytes(rgba_png(rgba))


def playcanvas_camera(formal_value: dict[str, object]) -> dict[str, object]:
    frame = formal_value["trace"]["frames"][0]
    oracle = SMOKE._playcanvas_camera_oracle(formal_value)
    return {
        "schema": SMOKE.PLAYCANVAS_CAMERA_SCHEMA,
        "trace_frame_index": 0,
        "phase": "presentation_frame_2",
        "runtime_source": "live PlayCanvas Entity and Camera matrices after trace application",
        "position": oracle["position"],
        "forward": oracle["forward"],
        "up": oracle["up"],
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
            "configured_projection_matrix_opengl_column_major": oracle[
                "projection_matrix_opengl_column_major"
            ],
        },
        "view_matrix_column_major": oracle["view_matrix_column_major"],
        "projection_matrix_opengl_column_major": oracle[
            "projection_matrix_opengl_column_major"
        ],
        "view_projection_matrix_opengl_column_major": oracle[
            "view_projection_matrix_opengl_column_major"
        ],
        "shader_projection_matrix_webgpu_column_major": oracle[
            "shader_projection_matrix_webgpu_column_major"
        ],
        "shader_view_projection_matrix_webgpu_column_major": oracle[
            "shader_view_projection_matrix_webgpu_column_major"
        ],
        "conversion": {
            "world": "canonical RUF +Z-forward to PlayCanvas RUB -Z-forward by diag(1,1,-1)",
            "projection": (
                "centered f*x/y/aspect canonical row-major +Z/[0,1] -> PlayCanvas "
                "column-major -Z/OpenGL[-1,1] -> WebGPU shader [0,1]"
            ),
            "shader_flip_y": False,
        },
        "validation": {
            "oracle": (
                "pose/intrinsics-recomputed canonical trace oracle; trace matrices "
                "verified independently"
            ),
            "absolute_tolerance": 0.0002,
            "relative_tolerance": 0.00002,
            "passed": True,
        },
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
    presentation_frames = []
    for index in range(3):
        frame_camera = copy.deepcopy(camera)
        frame_camera["phase"] = f"presentation_frame_{index}"
        frame = {
            "trace_frame_index": 0,
            "camera_receipt": frame_camera,
            "submit_version_before": 6 + index,
            "submit_version_after": 7 + index,
            "queue_submit_call_count": 1,
        }
        if index == 2:
            frame["renderer_capture_copy"] = {"submit_version_after": 10}
        presentation_frames.append(frame)
    terminal_camera = copy.deepcopy(camera)
    terminal_camera["phase"] = "external_capture_terminal"
    manifest["camera_receipt"] = terminal_camera
    manifest["presentation_capture"] = {
        "schema": SMOKE.PLAYCANVAS_PRESENTATION_SCHEMA,
        "ready_for_external_capture": True,
        "excluded_from_performance": True,
        "capture_trace_frame_index": 0,
        "minimum_stable_frame_count": 3,
        "stable_frame_count": 3,
        "measurement_terminal_submit_version": 6,
        "frames": presentation_frames,
        "terminal_camera_receipt": terminal_camera,
        "renderer_capture": capture,
        "queue_drain": {
            "phase": "post_capture_presentation",
            "api": SMOKE.PLAYCANVAS_QUEUE_DRAIN_API,
            "semanticsSource": SMOKE.PLAYCANVAS_QUEUE_DRAIN_SEMANTICS,
            "specificationUrl": SMOKE.PLAYCANVAS_QUEUE_DRAIN_SPECIFICATION,
            "frameLoopStopped": True,
            "submitVersionBefore": 10,
            "submitVersionAfter": 10,
            "submitVersionStable": True,
            "startedAtUtc": "2026-07-27T08:00:00.000Z",
            "endedAtUtc": "2026-07-27T08:00:00.015Z",
            "drainMs": 15.0,
            "endedAtMs": 1234.5,
        },
    }
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

    def test_camera_revision_is_nonnegative_but_not_boolean(self) -> None:
        self.assertEqual(SMOKE._nonnegative_integer(0, "camera revision"), 0)
        for invalid in (-1, True):
            with self.subTest(invalid=invalid):
                with self.assertRaisesRegex(
                    SMOKE.OneViewQualityError, "non-negative integer"
                ):
                    SMOKE._nonnegative_integer(invalid, "camera revision")

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

    def test_ground_truth_decoder_admits_only_opaque_rgb8(self) -> None:
        source = self.root / "source.png"
        source.write_bytes(rgb_png(self.source_rgb))
        decoded = SMOKE._decode_png(
            source,
            "source",
            allowed_color_types=frozenset({2}),
        )
        self.assertEqual(decoded, self.good_rgba)
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "RGBA8"):
            SMOKE._decode_png(source, "endpoint")

        endpoint = self.root / "endpoint.png"
        endpoint.write_bytes(rgba_png(self.good_rgba))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "RGB8"):
            SMOKE._decode_png(
                endpoint,
                "source",
                allowed_color_types=frozenset({2}),
            )

    def test_ground_truth_decoder_uses_rgb_stride_for_sub_and_paeth(self) -> None:
        for filter_type in (1, 4):
            source = self.root / f"source-filter-{filter_type}.png"
            source.write_bytes(rgb_png(self.source_rgb, filter_type=filter_type))
            decoded = SMOKE._decode_png(
                source,
                f"source filter {filter_type}",
                allowed_color_types=frozenset({2}),
            )
            self.assertEqual(decoded, self.good_rgba)

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

    def test_native_actual_camera_ratio_drift_fails_even_with_rehashed_owner(self) -> None:
        native = self.root / "native"
        write_native(native, self.good_rgba, self.formal)
        manifest_path = native / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        receipt = manifest["camera_receipt"]
        receipt["intrinsics"]["focalLengthXOverY"] = 1.0
        manifest["q1_comparison"]["presentation_identity"]["camera"][
            "runtime_camera_receipt_sha256"
        ] = SMOKE.canonical_sha256(receipt)
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "focalLengthXOverY"):
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
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "owner location"):
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
        manifest["presentation_capture"]["renderer_capture"] = copy.deepcopy(
            manifest["renderer_capture"]
        )
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "focal_length_x_over_y"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_playcanvas_full_runtime_camera_oracle_rejects_rehashed_matrix_drift(self) -> None:
        camera = playcanvas_camera(self.formal)
        camera["shader_view_projection_matrix_webgpu_column_major"][7] += 0.25
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "shader_view_projection"):
            SMOKE._playcanvas_camera(camera, self.formal, "camera")

    def test_playcanvas_capture_requires_same_presentation_owner(self) -> None:
        playcanvas = self.root / "playcanvas"
        write_playcanvas(playcanvas, self.good_rgba, self.formal)
        manifest_path = playcanvas / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["presentation_capture"]["renderer_capture"] = copy.deepcopy(
            manifest["renderer_capture"]
        )
        manifest["presentation_capture"]["renderer_capture"]["renderer_frame_sequence"] += 1
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "owner location"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_playcanvas_capture_requires_final_frame_copy_and_outer_drain(self) -> None:
        playcanvas = self.root / "playcanvas"
        write_playcanvas(playcanvas, self.good_rgba, self.formal)
        manifest_path = playcanvas / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["presentation_capture"]["frames"][-1]["renderer_capture_copy"][
            "submit_version_after"
        ] = 11
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "same-frame"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

        manifest = json.loads(manifest_path.read_text())
        manifest["presentation_capture"]["frames"][-1]["renderer_capture_copy"][
            "submit_version_after"
        ] = 10
        manifest["presentation_capture"]["queue_drain"]["frameLoopStopped"] = False
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "queue drain"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_playcanvas_capture_requires_complete_exact_outer_queue_drain(self) -> None:
        mutations = {
            "extra field": lambda receipt: receipt.__setitem__("extra", True),
            "wrong API": lambda receipt: receipt.__setitem__("api", "queue.finish()"),
            "wrong semantics": lambda receipt: receipt.__setitem__(
                "semanticsSource", "invented"
            ),
            "wrong specification": lambda receipt: receipt.__setitem__(
                "specificationUrl", "https://example.invalid/"
            ),
            "invalid start UTC": lambda receipt: receipt.__setitem__(
                "startedAtUtc", "not-a-date"
            ),
            "reverse UTC": lambda receipt: receipt.__setitem__(
                "endedAtUtc", "2026-07-27T07:59:59.000Z"
            ),
            "negative drain": lambda receipt: receipt.__setitem__("drainMs", -1.0),
            "invalid monotonic end": lambda receipt: receipt.__setitem__(
                "endedAtMs", -1.0
            ),
            "unstable version": lambda receipt: receipt.__setitem__(
                "submitVersionAfter", 11
            ),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                playcanvas = self.root / f"playcanvas-{label.replace(' ', '-')}"
                write_playcanvas(playcanvas, self.good_rgba, self.formal)
                manifest_path = playcanvas / "manifest.json"
                manifest = json.loads(manifest_path.read_text())
                mutate(manifest["presentation_capture"]["queue_drain"])
                manifest_path.write_text(json.dumps(manifest))
                with self.assertRaisesRegex(
                    SMOKE.OneViewQualityError, "queue drain"
                ):
                    SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_playcanvas_blocker_fails_closed(self) -> None:
        playcanvas = self.root / "playcanvas"
        write_playcanvas(playcanvas, self.good_rgba, self.formal)
        (playcanvas / "blocker.json").write_text("{}")
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "blocker.json"):
            SMOKE._playcanvas_capture(playcanvas, self.formal)

    def test_publication_is_fresh_atomic_and_rejects_performance_fields(self) -> None:
        result = self.evaluate()
        input_root = self.root / "immutable-input"
        input_root.mkdir()
        output = self.root / "published"
        SMOKE.publish_one_view(result, output, input_roots=(input_root,))
        self.assertEqual(
            json.loads((output / "result.json").read_text())["status"], "Accepted"
        )
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "already exists"):
            SMOKE.publish_one_view(result, output, input_roots=(input_root,))
        mutated = copy.deepcopy(result)
        mutated["timing"] = {"frame_ms": 1.0}
        with self.assertRaisesRegex(SMOKE.OneViewQualityError, "forbidden"):
            SMOKE.validate_result(mutated)

    def test_publication_rejects_every_overlapping_input_tree_before_staging(self) -> None:
        result = self.evaluate()
        for index in range(4):
            input_root = self.root / f"input-{index}"
            input_root.mkdir()
            output = input_root / "forbidden-output"
            roots = tuple(
                input_root if item == index else self.root / f"peer-{index}-{item}"
                for item in range(4)
            )
            for root in roots:
                root.mkdir(exist_ok=True)
            with self.assertRaisesRegex(SMOKE.OneViewQualityError, "overlaps"):
                SMOKE.publish_one_view(result, output, input_roots=roots)
            self.assertFalse(output.exists())
            self.assertFalse(any(input_root.glob(".forbidden-output.staging-*")))


if __name__ == "__main__":
    unittest.main()
