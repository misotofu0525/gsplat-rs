#!/usr/bin/env python3

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


ROOT = pathlib.Path(__file__).resolve().parents[2]
COLLECTOR_PATH = ROOT / "tests/perf/collect-scalable-s1-m4.py"


def load_module(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


COLLECTOR = load_module("collect_scalable_s1_m4_test", COLLECTOR_PATH)


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    return sha256_bytes(path.read_bytes())


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", binascii.crc32(kind + payload) & 0xFFFFFFFF)
    )


def rgba_png(width: int, height: int, rgba: bytes) -> bytes:
    rows = b"".join(
        b"\x00" + rgba[offset : offset + width * 4]
        for offset in range(0, len(rgba), width * 4)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows))
        + png_chunk(b"IEND", b"")
    )


def ply_header(count: int) -> bytes:
    lines = [
        "ply",
        "format binary_little_endian 1.0",
        f"element vertex {count}",
        "property float x",
        "property float y",
        "property float z",
    ]
    lines.extend(f"property float f_rest_{index}" for index in range(45))
    lines.extend(("end_header", ""))
    return "\n".join(lines).encode("ascii")


def coverage(
    name: str,
    *,
    source_sha256: str,
    hierarchy_sha256: str,
    source_count: int,
    active: int,
) -> dict:
    node_ids = [f"{name}-node-{index}" for index in range(max(active, 1))]
    page_hashes = [sha256_bytes(node.encode()) for node in node_ids]
    return {
        "source_sha256": source_sha256,
        "hierarchy_manifest_sha256": hierarchy_sha256,
        "source_splat_count": source_count,
        "represented_source_leaves": source_count,
        "active_proxy_splats": active,
        "missing_leaves": 0,
        "overlap_count": 0,
        "missing_page_count": 0,
        "antichain_valid": True,
        "parent_descendant_overlap": False,
        "source_sh_degree": 3,
        "sh_representation": "source_sh3",
        "sampling": "disabled",
        "partial_child_publication": "disabled",
        "ordered_node_ids": node_ids,
        "ordered_node_list_sha256": COLLECTOR.canonical_sha256(node_ids),
        "page_sha256": page_hashes,
        "page_list_sha256": COLLECTOR.canonical_sha256(page_hashes),
        "replacement_count": 2 if name == "mixed_depth_two_replacements" else 0,
        "depth_count": 2 if name == "mixed_depth_two_replacements" else 1,
        "payload_bit_exact_to_source": name == "complete_leaf_exact",
    }


def render_input(
    name: str,
    receipt: dict,
    *,
    path: str,
    sha256: str,
    byte_count: int,
) -> dict:
    value = {
        "schema": "gsplat-formal-s1-cut-render-input/v1",
        "cut_name": name,
        "source_sha256": receipt["source_sha256"],
        "hierarchy_manifest_sha256": receipt["hierarchy_manifest_sha256"],
        "ordered_node_ids": receipt["ordered_node_ids"],
        "ordered_node_list_sha256": receipt["ordered_node_list_sha256"],
        "page_sha256": receipt["page_sha256"],
        "page_list_sha256": receipt["page_list_sha256"],
        "P": receipt["active_proxy_splats"],
        "sh_degree": 3,
        "sampling": "disabled",
        "sha256": sha256,
        "bytes": byte_count,
    }
    if name == "complete_leaf_exact":
        value.update(
            {
                "kind": "content_addressed_source_ply_alias",
                "logical_path": path,
                "copied_into_package": False,
            }
        )
    else:
        value.update(
            {
                "kind": "materialized_binary_little_endian_sh3_ply",
                "path": path,
            }
        )
    return value


def fixture_cut_authority(root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path, dict]:
    repo = root / "repo"
    receipt_root = root / "s1a"
    (repo / "assets").mkdir(parents=True)
    (receipt_root / "cuts").mkdir(parents=True)
    source_path = repo / "assets/source.ply"
    source_path.write_bytes(ply_header(8))
    proxy_paths = {
        "bootstrap_roots": receipt_root / "cuts/bootstrap_roots.ply",
        "mixed_depth_two_replacements": receipt_root
        / "cuts/mixed_depth_two_replacements.ply",
    }
    proxy_paths["bootstrap_roots"].write_bytes(ply_header(2))
    proxy_paths["mixed_depth_two_replacements"].write_bytes(ply_header(4))
    hierarchy_path = receipt_root / "manifest.bin"
    hierarchy_path.write_bytes(b"fixture hierarchy manifest")
    hierarchy_sha = sha256_file(hierarchy_path)
    source = {
        "dataset_id": "bonsai-fixture",
        "logical_path": "assets/source.ply",
        "sha256": sha256_file(source_path),
        "bytes": source_path.stat().st_size,
        "splat_count": 8,
        "sh_degree": 3,
        "sampling": "disabled",
    }
    active = {
        "complete_leaf_exact": 8,
        "bootstrap_roots": 2,
        "mixed_depth_two_replacements": 4,
    }
    cuts = []
    for name in COLLECTOR.S1.REQUIRED_CUTS:
        item = coverage(
            name,
            source_sha256=source["sha256"],
            hierarchy_sha256=hierarchy_sha,
            source_count=8,
            active=active[name],
        )
        if name == "complete_leaf_exact":
            input_path = source["logical_path"]
            input_file = source_path
        else:
            input_file = proxy_paths[name]
            input_path = input_file.relative_to(receipt_root).as_posix()
        cuts.append(
            {
                "name": name,
                "coverage": item,
                "coverage_sha256": COLLECTOR.canonical_sha256(item),
                "render_input": render_input(
                    name,
                    item,
                    path=input_path,
                    sha256=sha256_file(input_file),
                    byte_count=input_file.stat().st_size,
                ),
            }
        )
    receipt = {
        "schema": "gsplat-formal-s1-proxy-authoring/v1",
        "authoring_status": "complete",
        "scope": "offline_hierarchy_authoring_with_s1_cut_render_inputs",
        "s1_promotion_status": "Active",
        "endpoint_image_gate": "not_run",
        "s2_s5_unlocked": False,
        "authority": {
            "source": source,
            "builder": {
                "repository_commit": "a" * 40,
                "configuration_sha256": "b" * 64,
            },
        },
        "hierarchy": {
            "manifest": {
                "path": "manifest.bin",
                "sha256": hierarchy_sha,
                "bytes": hierarchy_path.stat().st_size,
            }
        },
        "cuts": cuts,
    }
    receipt_path = receipt_root / "cut-receipt.json"
    receipt_path.write_text(json.dumps(receipt, sort_keys=True), encoding="utf-8")
    return repo, receipt_path, source


def fixture_trace() -> dict:
    frames = []
    for index in range(2):
        frames.append(
            {
                "frame_index": index,
                "timestamp_ns": COLLECTOR.CAPTURE_TRACE_TIMESTAMPS_NS[index],
                "pose": {
                    "position": [float(index), 0.0, -3.0],
                    "rotation_xyzw": [0.0, 0.0, 0.0, 1.0],
                },
                "intrinsics": {
                    "vertical_fov_radians": 1.0,
                    "near_plane": 0.01,
                    "far_plane": 100.0,
                },
            }
        )
    return {"trace_id": "fixture-trace", "content_sha256": "c" * 64, "frames": frames}


def terminal_line(
    *,
    index: int,
    order: str,
    source_count: int,
    rgba8_sha256: str,
    visible: int | None = None,
    captured_camera_revision: int | None = None,
) -> str:
    trace_frame = COLLECTOR.CAPTURE_TRACE_FRAMES[index]
    timestamp = COLLECTOR.CAPTURE_TRACE_TIMESTAMPS_NS[index]
    spec = COLLECTOR.ORDER_SPEC[order]
    camera_revision = index + 1
    captured_camera = camera_revision if captured_camera_revision is None else captured_camera_revision
    visible_count = source_count if visible is None else visible
    contributor = max(visible_count - 1, 0)
    fields = {
        "status": "ok",
        "capture_index": index,
        "path": f"capture.captures/capture-{index}-trace-{trace_frame}.png",
        "trace_frame": trace_frame,
        "trace_timestamp_ns": timestamp,
        "elapsed_ns": 1000 + index,
        "call_ms": "1.0",
        "frame_wall_ms": "2.0",
        "current_stats_ticket": 10 + index,
        "current_stats_scene_generation": 1,
        "current_stats_camera_revision": camera_revision,
        "current_stats_viewport_generation": 0,
        "current_stats_contract_generation": 1,
        "current_stats_plan_set_generation": 1,
        "current_stats_plan_id": spec["current"],
        "current_stats_order_generation": index + 1,
        "current_stats_raster_generation": 1,
        "current_stats_encode_attempt": 1,
        "current_stats_presentation_sequence": 20 + index,
        "count_semantics": COLLECTOR.RAW_COUNT_SEMANTICS[order],
        "source_count": source_count,
        "visible_count": visible_count,
        "contributor_count": contributor,
        "drawn_count": visible_count,
        "exact_contributor_compaction": "false",
        "capture_receipt_depth_precision_profile": "ExactFull32",
        "capture_receipt_projected_cache_precision_profile": "ExactAxes32",
        "capture_receipt_projected_axis_record_bytes": 16,
        "capture_receipt_resident_sh_codec_profile": "ExactSigned11BandScale5",
        "capture_receipt_resident_sh_source_count": source_count,
        "capture_receipt_resident_sh_encoded_count": source_count,
        "capture_receipt_resident_sh_resident_count": source_count,
        "capture_receipt_resident_sh_addressable_count": source_count,
        "capture_receipt_resident_sh_source_degree": 3,
        "capture_receipt_resident_sh_resident_degree": 3,
        "capture_receipt_scene_generation": 1,
        "capture_receipt_camera_revision": captured_camera,
        "capture_receipt_viewport_generation": 0,
        "capture_receipt_contract_generation": 1,
        "capture_receipt_plan_set_generation": 1,
        "capture_receipt_plan_id": spec["capture"],
        "capture_receipt_order_generation": index + 1,
        "capture_receipt_presentation_sequence": 20 + index,
        "capture_receipt_width": 2,
        "capture_receipt_height": 2,
        "capture_receipt_rgba8_sha256": rgba8_sha256,
        "frame_presented": "true",
        "terminal_receipt": "ready",
    }
    return "SURFACE_DIAGNOSTIC_MULTI_CAPTURE_TERMINAL " + " ".join(
        f"{key}={json.dumps(str(value)) if key == 'path' else value}"
        for key, value in fields.items()
    )


def fixture_raw_session(
    directory: pathlib.Path,
    *,
    order: str = "cpu",
    source_count: int = 3,
    begin_source_count: int | None = None,
    invalid_capture_camera: bool = False,
    visible: int | None = None,
) -> tuple[str, str]:
    capture_dir = directory / "capture.captures"
    capture_dir.mkdir(parents=True)
    rgba = bytes([64, 96, 128, 255]) * 4
    png = rgba_png(2, 2, rgba)
    for index, trace_frame in enumerate(COLLECTOR.CAPTURE_TRACE_FRAMES):
        (capture_dir / f"capture-{index}-trace-{trace_frame}.png").write_bytes(png)
    rgba_hash = sha256_bytes(rgba)
    spec = COLLECTOR.ORDER_SPEC[order]
    begin_count = source_count if begin_source_count is None else begin_source_count
    begin = {
        "trace_id": "fixture-trace",
        "trace_sha256": "c" * 64,
        "exact_plan_requested": spec["requested"],
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "blend_mode": "sorted_alpha",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "adapter_backend": "metal",
        "adapter_name": "Apple M4",
        "adapter_device_type": "integrated_gpu",
        "adapter_driver": "fixture",
        "adapter_driver_info": "fixture-driver",
        "source_count": begin_count,
        "decoded_count": begin_count,
        "encoded_count": begin_count,
        "resident_count": begin_count,
        "addressable_count": begin_count,
        "sh_degree": 3,
        "requested_width": 2,
        "requested_height": 2,
        "surface_width": 2,
        "surface_height": 2,
        "internal_render_width": 2,
        "internal_render_height": 2,
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
        "trace_frames": 100,
    }
    lines = [
        "SURFACE_EXACT_EVIDENCE_BEGIN "
        + " ".join(
            f"{key}={json.dumps(str(value)) if key in {'adapter_name', 'adapter_driver_info'} else value}"
            for key, value in begin.items()
        )
    ]
    for index in range(3):
        lines.append(
            terminal_line(
                index=index,
                order=order,
                source_count=source_count,
                rgba8_sha256=rgba_hash,
                visible=visible,
                captured_camera_revision=(99 if invalid_capture_camera and index == 0 else None),
            )
        )
    summary = {
        "status": "ok",
        "exact_plan_requested": spec["requested"],
        "actual_plan_set": spec["current"],
        "trace_frames": 100,
        "measured_frames": 80,
        "terminal_receipts": 103,
        "final_capture": "available",
    }
    lines.append(
        "SURFACE_EXACT_EVIDENCE_SUMMARY "
        + " ".join(f"{key}={value}" for key, value in summary.items())
    )
    return "\n".join(lines) + "\n", ""


class CollectScalableS1M4Tests(unittest.TestCase):
    def test_s1a_receipt_is_the_only_proxy_render_input_authority(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            repo, receipt_path, source = fixture_cut_authority(pathlib.Path(temp))
            authority = COLLECTOR.read_cut_authority(
                repo, receipt_path, formal_source=source
            )
            self.assertEqual(authority.cuts["complete_leaf_exact"].path, authority.source_path)
            self.assertEqual(authority.cuts["bootstrap_roots"].active_splats, 2)
            self.assertEqual(authority.cuts["mixed_depth_two_replacements"].active_splats, 4)
            self.assertEqual(
                authority.cuts["bootstrap_roots"].path.resolve(),
                (receipt_path.parent / "cuts/bootstrap_roots.ply").resolve(),
            )

    def test_cpu_proxy_raw_session_keeps_input_p_and_atomic_join(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(directory, order="cpu", source_count=3)
            before = COLLECTOR.IMAGE.artifact_directory_sha256(directory)
            session = COLLECTOR.validate_raw_session(
                stdout,
                stderr,
                raw_directory=directory,
                order="cpu",
                input_count=3,
                trace=fixture_trace(),
                size=(2, 2),
            )
            self.assertEqual(session.input_count, 3)
            self.assertEqual([capture.source_count for capture in session.captures], [3, 3, 3])
            self.assertEqual([capture.trace_frame_index for capture in session.captures], [0, 1, 0])
            self.assertEqual(before, COLLECTOR.IMAGE.artifact_directory_sha256(directory))

    def test_gpu_order_is_forced_without_changing_count_contract(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(directory, order="gpu", source_count=4)
            session = COLLECTOR.validate_raw_session(
                stdout,
                stderr,
                raw_directory=directory,
                order="gpu",
                input_count=4,
                trace=fixture_trace(),
                size=(2, 2),
            )
            self.assertEqual(session.order, "gpu")
            self.assertTrue(all(capture.drawn == capture.visible for capture in session.captures))

    def test_proxy_raw_artifact_cannot_be_relabelled_as_s(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(
                directory, source_count=3, begin_source_count=8
            )
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "source_count"):
                COLLECTOR.validate_raw_session(
                    stdout,
                    stderr,
                    raw_directory=directory,
                    order="cpu",
                    input_count=3,
                    trace=fixture_trace(),
                    size=(2, 2),
                )

    def test_vcd_above_p_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(
                directory, source_count=3, visible=4
            )
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "C<=V<=P"):
                COLLECTOR.validate_raw_session(
                    stdout,
                    stderr,
                    raw_directory=directory,
                    order="cpu",
                    input_count=3,
                    trace=fixture_trace(),
                    size=(2, 2),
                )

    def test_stale_capture_identity_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(
                directory, source_count=3, invalid_capture_camera=True
            )
            with self.assertRaisesRegex(
                COLLECTOR.ValidationError, "PNG/current-stats/capture receipt identity mismatch"
            ):
                COLLECTOR.validate_raw_session(
                    stdout,
                    stderr,
                    raw_directory=directory,
                    order="cpu",
                    input_count=3,
                    trace=fixture_trace(),
                    size=(2, 2),
                )

    def test_missing_terminal_receipt_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(directory, source_count=3)
            lines = stdout.splitlines()
            del lines[2]
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "expected three"):
                COLLECTOR.validate_raw_session(
                    "\n".join(lines),
                    stderr,
                    raw_directory=directory,
                    order="cpu",
                    input_count=3,
                    trace=fixture_trace(),
                    size=(2, 2),
                )

    def test_failed_present_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(directory, source_count=3)
            stdout = stdout.replace("frame_presented=true", "frame_presented=false", 1)
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "frame_presented"):
                COLLECTOR.validate_raw_session(
                    stdout,
                    stderr,
                    raw_directory=directory,
                    order="cpu",
                    input_count=3,
                    trace=fixture_trace(),
                    size=(2, 2),
                )

    def test_dimension_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = pathlib.Path(temp) / "raw"
            stdout, stderr = fixture_raw_session(directory, source_count=3)
            stdout = stdout.replace("requested_width=2", "requested_width=3", 1)
            with self.assertRaisesRegex(COLLECTOR.ValidationError, "requested_width"):
                COLLECTOR.validate_raw_session(
                    stdout,
                    stderr,
                    raw_directory=directory,
                    order="cpu",
                    input_count=3,
                    trace=fixture_trace(),
                    size=(2, 2),
                )

    def test_derived_benchmark_joins_s_to_raw_p_without_mutating_raw(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            repo, receipt_path, source = fixture_cut_authority(root)
            authority = COLLECTOR.read_cut_authority(
                repo, receipt_path, formal_source=source
            )
            stage = root / "stage"
            raw = stage / "raw/cpu/bootstrap/proxy/moving"
            stdout, stderr = fixture_raw_session(raw, source_count=2)
            session = COLLECTOR.validate_raw_session(
                stdout,
                stderr,
                raw_directory=raw,
                order="cpu",
                input_count=2,
                trace=fixture_trace(),
                size=(2, 2),
            )
            raw_hash = session.raw_sha256
            capture = session.captures[0]
            presentation = COLLECTOR.canonical_presentation(capture, 1)
            result = COLLECTOR.write_benchmark_artifact(
                stage / "runs/proxy",
                lane="proxy",
                capture=capture,
                presentation=presentation,
                pair_id="fixture-pair",
                order="cpu",
                cut_name="bootstrap_roots",
                sequence="moving_sequence",
                capture_index=0,
                camera=COLLECTOR.camera_receipt(fixture_trace(), 0),
                authority=authority,
                trace=fixture_trace(),
                adapter=session.adapter,
                build={
                    "git": {"commit": "e" * 40},
                    "binary_sha256": "f" * 64,
                    "package_version": "0.1.3",
                },
                raw_session=session,
                stage=stage,
                size=(2, 2),
            )
            manifest = json.loads(
                (stage / result["path"] / "manifest.json").read_text(encoding="utf-8")
            )
            frame = json.loads(
                (stage / result["path"] / "frames.jsonl").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["dataset"]["splat_count"], 8)
            self.assertEqual(
                manifest["trace"],
                {"id": "fixture-trace", "sha256": "c" * 64},
            )
            self.assertEqual(manifest["renderer"]["raw_input_splat_count"], 2)
            self.assertEqual(frame["active_splats"], 2)
            self.assertEqual(frame["capture_join"]["status"], "verified")
            self.assertEqual(raw_hash, COLLECTOR.IMAGE.artifact_directory_sha256(raw))

    def test_coverage_generation_is_evidence_identity_not_s4(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            repo, receipt_path, source = fixture_cut_authority(root)
            authority = COLLECTOR.read_cut_authority(
                repo, receipt_path, formal_source=source
            )
            raw = root / "raw"
            stdout, stderr = fixture_raw_session(raw, source_count=2)
            session = COLLECTOR.validate_raw_session(
                stdout,
                stderr,
                raw_directory=raw,
                order="cpu",
                input_count=2,
                trace=fixture_trace(),
                size=(2, 2),
            )
            capture = session.captures[0]
            presentation = COLLECTOR.canonical_presentation(capture, 1)
            receipt = COLLECTOR.presented_cut_receipt(
                authority=authority,
                cut=authority.cuts["bootstrap_roots"],
                order="cpu",
                proxy=capture,
                presentation=presentation,
                coverage_generation=7,
            )
            self.assertEqual(receipt["source_splat_count"], 8)
            self.assertEqual(receipt["represented_source_leaves"], 8)
            self.assertEqual(receipt["active_proxy_splats"], 2)
            self.assertEqual(
                receipt["coverage_generation_semantics"],
                "collector_evidence_identity_not_s4_runtime_generation",
            )

    def test_command_freezes_packed_multicapture_and_has_no_retry_switch(self) -> None:
        command = COLLECTOR.make_command(
            pathlib.Path("desktop"),
            pathlib.Path("cut.ply"),
            pathlib.Path("trace.json"),
            "gpu",
        )
        self.assertIn("packed", command)
        self.assertIn("gpu-post-sort", command)
        self.assertIn("--surface-diagnostic-multi-capture", command)
        self.assertEqual(command.count("--camera-sequence"), 1)
        self.assertFalse(any("retry" in value for value in command))
        self.assertEqual(
            COLLECTOR.REPLACEMENT_CUTS,
            (
                "bootstrap_roots",
                "mixed_depth_two_replacements",
                "complete_leaf_exact",
            ),
        )


if __name__ == "__main__":
    unittest.main()
