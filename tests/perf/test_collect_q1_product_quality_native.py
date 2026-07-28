from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import pathlib
import struct
import sys
import tempfile
import types
import unittest
import zlib
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("collect-q1-product-quality-native.py")
SPEC = importlib.util.spec_from_file_location("collect_q1_product_quality_native", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
NATIVE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = NATIVE
SPEC.loader.exec_module(NATIVE)


def chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(
        ">I", zlib.crc32(kind + payload) & 0xFFFFFFFF
    )


def rgba_png(rgba: bytes) -> bytes:
    row_bytes = NATIVE.WIDTH * 4
    filtered = b"".join(
        b"\0" + rgba[offset : offset + row_bytes]
        for offset in range(0, len(rgba), row_bytes)
    )
    ihdr = struct.pack(">IIBBBBB", NATIVE.WIDTH, NATIVE.HEIGHT, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(
        b"IDAT", zlib.compress(filtered)
    ) + chunk(b"IEND", b"")


def line(prefix: str, values: dict[str, object]) -> str:
    return prefix + " ".join(f"{key}={value}" for key, value in values.items())


def synthetic_host(trace: dict[str, object], rgba_sha: str) -> str:
    frame = trace["frame"]
    pose = frame["pose"]
    intrinsics = frame["intrinsics"]
    count = NATIVE.TRUCK_SPLAT_COUNT
    begin = {
        "trace_id": trace["value"]["trace_id"],
        "trace_sha256": trace["value"]["content_sha256"],
        "exact_plan_requested": NATIVE.PLAN_REQUESTED,
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "blend_mode": "sorted_alpha",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "adapter_backend": "metal",
        "adapter_name": "Apple_M4",
        "adapter_device_type": "integrated_gpu",
        "adapter_driver": "Metal",
        "adapter_driver_info": "unavailable",
        "source_count": count,
        "decoded_count": count,
        "encoded_count": count,
        "resident_count": count,
        "addressable_count": count,
        "sh_degree": 3,
        "requested_width": NATIVE.WIDTH,
        "requested_height": NATIVE.HEIGHT,
        "surface_width": NATIVE.WIDTH,
        "surface_height": NATIVE.HEIGHT,
        "internal_render_width": NATIVE.WIDTH,
        "internal_render_height": NATIVE.HEIGHT,
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
        "trace_frames": 1,
    }
    measured = {
        "trace_frame": 0,
        "phase": "measure",
        "measured_sample": 0,
        "exact_plan_requested": NATIVE.PLAN_REQUESTED,
        "exact_plan_actual": NATIVE.PLAN_REQUESTED,
        "count_semantics": "direct_draw_equals_visible",
        "source_count": count,
        "visible_count": 2_000_000,
        "contributor_count": 1_900_000,
        "drawn_count": 2_000_000,
        "exact_contributor_compaction": "false",
        "actual_backend": "cpu",
        "requested_width": NATIVE.WIDTH,
        "requested_height": NATIVE.HEIGHT,
        "presented_width": NATIVE.WIDTH,
        "presented_height": NATIVE.HEIGHT,
        "frame_presented": "true",
        "terminal_receipt": "ready",
    }
    capture = {
        "status": "ok",
        "path": "capture.png",
        "trace_frame": 0,
        "exact_plan_requested": NATIVE.PLAN_REQUESTED,
        "exact_plan_actual": NATIVE.PLAN_REQUESTED,
        "count_semantics": "direct_draw_equals_visible",
        "source_count": count,
        "visible_count": 2_000_000,
        "contributor_count": 1_900_000,
        "drawn_count": 2_000_000,
        "exact_contributor_compaction": "false",
        "actual_backend": "cpu",
        "scene_generation": 1,
        "camera_revision": 9,
        "viewport_generation": 0,
        "contract_generation": 2,
        "plan_set_generation": 3,
        "order_generation": 4,
        "presentation_sequence": 11,
        "requested_width": NATIVE.WIDTH,
        "requested_height": NATIVE.HEIGHT,
        "captured_width": NATIVE.WIDTH,
        "captured_height": NATIVE.HEIGHT,
        "frame_presented": "true",
        "terminal_receipt": "ready",
    }
    diagnostic = {
        "depth_precision_profile": "ExactFull32",
        "projected_cache_precision_profile": "ExactAxes32",
        "resident_sh_codec_profile": "ExactSigned11BandScale5",
        "resident_sh_source_count": count,
        "resident_sh_encoded_count": count,
        "resident_sh_resident_count": count,
        "resident_sh_addressable_count": count,
        "resident_sh_source_degree": 3,
        "resident_sh_resident_degree": 3,
        "scene_generation": 1,
        "camera_revision": 9,
        "viewport_generation": 0,
        "contract_generation": 2,
        "plan_set_generation": 3,
        "plan_id": NATIVE.PLAN_RECEIPT,
        "order_generation": 4,
        "presentation_sequence": 11,
        "width": NATIVE.WIDTH,
        "height": NATIVE.HEIGHT,
        "rgba8_sha256": rgba_sha,
    }
    camera = {
        "trace_frame": 0,
        "camera_revision": 9,
        "presentation_sequence": 11,
        "position_x": pose["position"][0],
        "position_y": pose["position"][1],
        "position_z": pose["position"][2],
        "rotation_x": pose["rotation_xyzw"][0],
        "rotation_y": pose["rotation_xyzw"][1],
        "rotation_z": pose["rotation_xyzw"][2],
        "rotation_w": pose["rotation_xyzw"][3],
        "vertical_fov_radians": intrinsics["vertical_fov_radians"],
        "near_plane": intrinsics["near_plane"],
        "far_plane": intrinsics["far_plane"],
        "focal_length_x_over_y": intrinsics["focal_length_x_over_y"],
    }
    summary = {
        "status": "ok",
        "exact_plan_requested": NATIVE.PLAN_REQUESTED,
        "actual_plan_set": NATIVE.PLAN_REQUESTED,
        "trace_frames": 1,
        "measured_frames": 1,
        "terminal_receipts": 2,
        "final_capture": "available",
    }
    return "\n".join(
        (
            line(NATIVE.PREFIXES["begin"], begin),
            line(NATIVE.PREFIXES["frame"], measured),
            line(NATIVE.PREFIXES["capture"], capture),
            line(NATIVE.PREFIXES["diagnostic"], diagnostic),
            line(NATIVE.PREFIXES["camera"], camera),
            line(NATIVE.PREFIXES["summary"], summary),
        )
    )


class NativeQualityProducerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.trace = NATIVE.load_formal_trace(NATIVE.FORMAL_TRACE_ROOT)
        self.rgba = bytes((64, 96, 128, 255)) * (NATIVE.WIDTH * NATIVE.HEIGHT)
        self.capture_path = self.root / "capture.png"
        self.capture_path.write_bytes(rgba_png(self.rgba))
        self.stdout = synthetic_host(self.trace, hashlib.sha256(self.rgba).hexdigest())

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_command_is_one_fixed_quality_capture_without_throughput_mode(self) -> None:
        command = NATIVE.make_command(
            pathlib.Path("/locked/desktop-example"),
            pathlib.Path("/inputs/truck.ply"),
            pathlib.Path("/inputs/camera-trace.json"),
        )
        self.assertEqual(command.count("--surface-diagnostic-capture-receipt"), 1)
        self.assertIn("cpu-post-sort", command)
        self.assertIn("--camera-frame", command)
        self.assertNotIn("--camera-sequence", command)
        self.assertNotIn("throughput", command)
        self.assertEqual(command[command.index("--camera-measured-frames") + 1], "1")

    def test_terminal_receipts_materialize_gate_compatible_minimal_artifact(self) -> None:
        camera, capture = NATIVE.validate_host(
            self.stdout, "", trace=self.trace, capture_path=self.capture_path
        )
        artifact = self.root / "artifact"
        manifest, frame = NATIVE.materialize_artifact(
            artifact,
            trace=self.trace,
            native_camera=camera,
            capture=capture,
            capture_path=self.capture_path,
            commit="a" * 40,
            binary_sha256="b" * 64,
        )
        self.assertEqual(
            {path.name for path in artifact.iterdir()},
            {"manifest.json", "frames.jsonl", "final-frame.png"},
        )
        self.assertFalse(manifest["q1_comparison"]["performance_evidence"])
        self.assertFalse(manifest["q1_comparison"]["performance_eligible"])
        self.assertEqual(manifest["q1_comparison"]["product_quality"], "Deferred")
        NATIVE.reject_performance_fields(manifest)
        NATIVE.reject_performance_fields(frame)
        NATIVE.Q1._native_capture(artifact, NATIVE.trace_for_gate(self.trace))

    def test_missing_renderer_owned_camera_terminal_fails_closed(self) -> None:
        without_camera = "\n".join(
            line for line in self.stdout.splitlines()
            if not line.startswith(NATIVE.PREFIXES["camera"])
        )
        with self.assertRaisesRegex(NATIVE.ValidationError, "exactly one camera"):
            NATIVE.validate_host(
                without_camera, "", trace=self.trace, capture_path=self.capture_path
            )

    def test_camera_revision_must_match_same_present_capture(self) -> None:
        changed = self.stdout.replace("camera_revision=9 presentation_sequence=11 position_x", "camera_revision=10 presentation_sequence=11 position_x")
        with self.assertRaisesRegex(NATIVE.ValidationError, "camera receipt.camera_revision"):
            NATIVE.validate_host(changed, "", trace=self.trace, capture_path=self.capture_path)

    def test_performance_measurements_cannot_enter_published_payload(self) -> None:
        for key in ("frame_wall_ms", "fps", "pairing", "winner"):
            with self.subTest(key=key):
                with self.assertRaisesRegex(NATIVE.ValidationError, "forbidden"):
                    NATIVE.reject_performance_fields({key: 1})

    def test_output_cannot_overlap_an_immutable_input_tree(self) -> None:
        authority = self.root / "authority"
        authority.mkdir()
        with self.assertRaisesRegex(NATIVE.ValidationError, "overlaps immutable input"):
            NATIVE.require_disjoint_output(authority / "capture", (authority,))
        output = self.root / "new-output"
        NATIVE.require_disjoint_output(output, (authority,))

    def test_artifact_terminal_hash_rejects_mutation(self) -> None:
        camera, capture = NATIVE.validate_host(
            self.stdout, "", trace=self.trace, capture_path=self.capture_path
        )
        artifact = self.root / "mutated"
        NATIVE.materialize_artifact(
            artifact,
            trace=self.trace,
            native_camera=camera,
            capture=capture,
            capture_path=self.capture_path,
            commit="a" * 40,
            binary_sha256="b" * 64,
        )
        frame_path = artifact / "frames.jsonl"
        frame = json.loads(frame_path.read_text())
        frame["presentation_sequence"] += 1
        frame_path.write_text(json.dumps(frame) + "\n", encoding="utf-8")
        with self.assertRaisesRegex(NATIVE.Q1.OneViewQualityError, "terminal frame hash"):
            NATIVE.Q1._native_capture(artifact, NATIVE.trace_for_gate(self.trace))

    def test_host_failure_retains_logs_without_formal_artifact_or_private_target(self) -> None:
        stage = self.root / ".native-stage"
        host = stage / "host"
        build = stage / "build"
        private_target = stage / "cargo-target/release"
        unpublished_artifact = stage / "artifact"
        for directory in (host, build, private_target, unpublished_artifact):
            directory.mkdir(parents=True)
        (host / "command.json").write_text(
            '{"argv":["desktop-example"],"cwd":"host"}\n', encoding="utf-8"
        )
        (host / "stdout.log").write_text("renderer stdout\n", encoding="utf-8")
        (host / "stderr.log").write_text("renderer rejected the frame\n", encoding="utf-8")
        (build / "stderr.log").write_text("build diagnostic\n", encoding="utf-8")
        (private_target / "desktop-example").write_bytes(b"rebuildable")
        (unpublished_artifact / "manifest.json").write_text(
            '{"looks_complete":true}\n', encoding="utf-8"
        )
        NATIVE.make_tree_immutable(unpublished_artifact)

        output = self.root / "native-view000001"
        failure = NATIVE.publish_failure(
            stage,
            output,
            "a" * 40,
            NATIVE.ValidationError("native quality host exited with 1"),
        )

        self.assertFalse(output.exists())
        self.assertFalse(stage.exists())
        self.assertTrue(failure.is_dir())
        self.assertFalse((failure / "cargo-target").exists())
        self.assertFalse((failure / "artifact").exists())
        self.assertEqual(
            (failure / "host/stdout.log").read_text(encoding="utf-8"),
            "renderer stdout\n",
        )
        self.assertEqual(
            (failure / "host/stderr.log").read_text(encoding="utf-8"),
            "renderer rejected the frame\n",
        )
        blocker = json.loads((failure / "blocker.json").read_text(encoding="utf-8"))
        self.assertEqual(blocker["status"], "failed_attempt")
        self.assertFalse(blocker["automatic_retry"])
        self.assertFalse(blocker["formal_output_published"])
        self.assertEqual(blocker["product_quality"], "Deferred")
        self.assertFalse(blocker["performance_authorized"])
        self.assertNotIn("environment", (failure / "host/command.json").read_text())
        for path in failure.rglob("*"):
            self.assertFalse(os.access(path, os.W_OK), path)

    def test_collect_host_failure_publishes_diagnostics_once_and_keeps_output_absent(self) -> None:
        output = self.root / "native-view000001"
        dataset = self.root / "truck.ply"
        dataset.write_bytes(b"truck")
        trace_root = self.root / "trace"
        trace_root.mkdir()
        trace_path = trace_root / "camera-trace.json"
        trace_path.write_text("{}\n", encoding="utf-8")
        commit = "a" * 40
        args = types.SimpleNamespace(
            output=output,
            expected_commit=commit,
            formal_trace_authority=trace_root,
            dataset=dataset,
        )
        trace = {"root": trace_root, "path": trace_path}
        invocations = []

        def fake_build(repo, stage, expected_git, *, feature, build_jobs):
            binary = stage / "cargo-target/release/desktop-example"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"binary")
            build = stage / "build"
            build.mkdir()
            (build / "stderr.log").write_text("build ok\n", encoding="utf-8")
            return {"path": binary, "sha256": "b" * 64}

        def invoke(command, cwd):
            invocations.append((tuple(command), cwd))
            return NATIVE.subprocess.CompletedProcess(
                command,
                1,
                stdout="native stdout before failure\n",
                stderr="fatal renderer detail\n",
            )

        with (
            mock.patch.object(NATIVE, "validate_ignored_output"),
            mock.patch.object(
                NATIVE,
                "git_receipt",
                return_value={"dirty": False, "commit": commit},
            ),
            mock.patch.object(NATIVE, "load_formal_trace", return_value=trace),
            mock.patch.object(NATIVE, "load_truck", return_value=dataset),
            mock.patch.object(NATIVE, "require_disjoint_output"),
            mock.patch.object(
                NATIVE.SHARED,
                "build_locked_desktop_binary",
                side_effect=fake_build,
            ),
        ):
            with self.assertRaisesRegex(
                NATIVE.ValidationError, "retained native failure"
            ):
                NATIVE.collect(args, repo=self.root, invoke=invoke)

        self.assertEqual(len(invocations), 1)
        self.assertFalse(output.exists())
        failures = list(self.root.glob("native-view000001.failed-q1-native-*"))
        self.assertEqual(len(failures), 1)
        failure = failures[0]
        self.assertEqual(
            (failure / "host/stdout.log").read_text(encoding="utf-8"),
            "native stdout before failure\n",
        )
        self.assertEqual(
            (failure / "host/stderr.log").read_text(encoding="utf-8"),
            "fatal renderer detail\n",
        )
        self.assertFalse((failure / "cargo-target").exists())
        blocker = json.loads((failure / "blocker.json").read_text(encoding="utf-8"))
        self.assertEqual(
            blocker["retained_host_logs"],
            {
                "command": "host/command.json",
                "stderr": "host/stderr.log",
                "stdout": "host/stdout.log",
            },
        )


if __name__ == "__main__":
    unittest.main()
