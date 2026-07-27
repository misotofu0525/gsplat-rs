#!/usr/bin/env python3
"""Focused tests for the A065 S1 materialized-cut collection mechanism."""

from __future__ import annotations

import argparse
import copy
import dataclasses
import hashlib
import importlib.util
import json
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from unittest import mock


SCRIPT = pathlib.Path(__file__).with_name("collect-scalable-s1-a065.py")
SPEC = importlib.util.spec_from_file_location("collect_scalable_s1_a065", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def png_fixture(width: int = 2, height: int = 1, value: int = 64) -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    rows = b"".join(
        b"\x00" + bytes((value, value, value, 255)) * width for _ in range(height)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def coverage(name: str, source: dict[str, object], manifest_hash: str) -> dict[str, object]:
    active = {
        "complete_leaf_exact": source["splat_count"],
        "bootstrap_roots": 1,
        "mixed_depth_two_replacements": 3,
    }[name]
    replacement_count = 2 if name == "mixed_depth_two_replacements" else 0
    depth_count = 2 if name == "mixed_depth_two_replacements" else 1
    node_ids = {
        "complete_leaf_exact": ["node:0", "node:1"],
        "bootstrap_roots": ["node:4"],
        "mixed_depth_two_replacements": ["node:2", "node:3", "node:1"],
    }[name]
    page_hashes = [hashlib.sha256(item.encode()).hexdigest() for item in node_ids]
    return {
        "source_sha256": source["sha256"],
        "hierarchy_manifest_sha256": manifest_hash,
        "source_splat_count": source["splat_count"],
        "represented_source_leaves": source["splat_count"],
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
        "ordered_node_list_sha256": COLLECTOR.S1.canonical_sha256(node_ids),
        "page_sha256": page_hashes,
        "page_list_sha256": COLLECTOR.S1.canonical_sha256(page_hashes),
        "replacement_count": replacement_count,
        "depth_count": depth_count,
        "payload_bit_exact_to_source": name == "complete_leaf_exact",
    }


def author_fixture(root: pathlib.Path) -> tuple[pathlib.Path, dict[str, object]]:
    source_path = root / "source.ply"
    source_path.write_bytes(b"fixture-source")
    source = {
        "dataset_id": "bonsai",
        "local_path": str(source_path),
        "sha256": hashlib.sha256(source_path.read_bytes()).hexdigest(),
        "bytes": source_path.stat().st_size,
        "splat_count": 5,
        "sh_degree": 3,
    }
    package = root / "package"
    package.mkdir()
    (package / "cuts").mkdir()
    manifest_path = package / "manifest.bin"
    manifest_path.write_bytes(b"manifest")
    manifest_hash = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    cuts = []
    for name in COLLECTOR.REQUIRED_CUTS:
        cut_coverage = coverage(name, source, manifest_hash)
        common = {
            "schema": COLLECTOR.CUT_RENDER_INPUT_SCHEMA,
            "cut_name": name,
            "source_sha256": source["sha256"],
            "hierarchy_manifest_sha256": manifest_hash,
            "ordered_node_ids": cut_coverage["ordered_node_ids"],
            "ordered_node_list_sha256": cut_coverage["ordered_node_list_sha256"],
            "page_sha256": cut_coverage["page_sha256"],
            "page_list_sha256": cut_coverage["page_list_sha256"],
            "P": cut_coverage["active_proxy_splats"],
            "sh_degree": 3,
            "sampling": "disabled",
        }
        if name == "complete_leaf_exact":
            render_input = {
                **common,
                "kind": "content_addressed_source_ply_alias",
                "logical_path": str(source_path),
                "sha256": source["sha256"],
                "bytes": source["bytes"],
                "copied_into_package": False,
            }
        else:
            cut_path = package / "cuts" / f"{name}.ply"
            cut_path.write_bytes(f"fixture-{name}".encode())
            render_input = {
                **common,
                "kind": "materialized_binary_little_endian_sh3_ply",
                "path": f"cuts/{name}.ply",
                "sha256": hashlib.sha256(cut_path.read_bytes()).hexdigest(),
                "bytes": cut_path.stat().st_size,
            }
        cuts.append(
            {
                "name": name,
                "coverage_sha256": COLLECTOR.S1.canonical_sha256(cut_coverage),
                "coverage": cut_coverage,
                "render_input": render_input,
            }
        )
    receipt = {
        "schema": COLLECTOR.AUTHOR_SCHEMA,
        "authoring_status": "complete",
        "scope": "offline_hierarchy_authoring_with_s1_cut_render_inputs",
        "s1_promotion_status": "Active",
        "endpoint_image_gate": "not_run",
        "s2_s5_unlocked": False,
        "authority": {
            "source": {
                "dataset_id": source["dataset_id"],
                "logical_path": source["local_path"],
                "sha256": source["sha256"],
                "bytes": source["bytes"],
                "splat_count": source["splat_count"],
                "sh_degree": 3,
                "sampling": "disabled",
            },
            "builder": {
                "repository_commit": "a" * 40,
                "configuration": {},
                "configuration_sha256": COLLECTOR.S1.canonical_sha256({}),
            },
        },
        "hierarchy": {
            "manifest": {
                "path": "manifest.bin",
                "sha256": manifest_hash,
                "bytes": manifest_path.stat().st_size,
            }
        },
        "cuts": cuts,
    }
    receipt_path = package / "cut-receipt.json"
    receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
    return receipt_path, source


def args_fixture(root: pathlib.Path, receipt: pathlib.Path) -> argparse.Namespace:
    return COLLECTOR.parser().parse_args(
        [
            "--serial",
            "fixture-serial",
            "--author-receipt",
            str(receipt),
            "--output",
            str(root / "output"),
            "--dry-run",
        ]
    )


def raw_capture_fixture(
    root: pathlib.Path,
    spec: COLLECTOR.CaptureSpec,
    input_path: pathlib.Path,
    active: int,
    *,
    lane: str,
) -> pathlib.Path:
    raw_root = root / f"raw-{lane}"
    artifact = raw_root / "run/artifact"
    artifact.mkdir(parents=True)
    input_identity = {
        "bytes": input_path.stat().st_size,
        "sha256": hashlib.sha256(input_path.read_bytes()).hexdigest(),
    }
    trace = {
        "trace_id": "fixture-trace",
        "content_sha256": "1" * 64,
        "display": {"width": 2, "height": 1},
        "frames": [
            {"timestamp_ns": 0, "pose": {}, "intrinsics": {}},
            {"timestamp_ns": 1, "pose": {}, "intrinsics": {}},
        ],
    }
    trace_path = root / "trace.json"
    trace_path.write_text(json.dumps(trace), encoding="utf-8")
    trace_identity = {
        "path": str(trace_path),
        "bytes": trace_path.stat().st_size,
        "sha256": hashlib.sha256(trace_path.read_bytes()).hexdigest(),
        "trace_id": trace["trace_id"],
        "content_sha256": trace["content_sha256"],
    }
    environment_receipt = {
        "manufacturer": COLLECTOR.EXPECTED_MANUFACTURER,
        "model": COLLECTOR.EXPECTED_MODEL,
        "device_properties": {
            "soc_model_property": {"value": COLLECTOR.EXPECTED_SOC_MODEL},
            "vulkan_hal_property": {"value": COLLECTOR.EXPECTED_VULKAN_HAL},
        },
    }
    (raw_root / "android-environment-receipt.json").write_text(
        json.dumps(environment_receipt), encoding="utf-8"
    )
    frames = []
    ledger = []
    schedule = COLLECTOR.expected_trace_schedule(spec)
    plan = "cpu_post_sort" if spec.order_backend == "cpu" else "gpu_post_sort"
    for index, trace_index in enumerate(schedule):
        ticket = 100 + index
        presentation = 200 + index
        camera_revision = 300 + index
        frames.append(
            {
                "frame_index": index,
                "camera_revision": camera_revision,
                "trace_frame_index": trace_index,
                "current_stats_ticket": ticket,
                "current_stats_presentation_sequence": presentation,
                "current_stats_executed_plan": plan,
                "visible": min(active, 3),
                "contributor": min(active, 2),
                "drawn": min(active, 3),
                "exact_contributor_compaction": False,
                "camera_receipt": {
                    "camera_revision": camera_revision,
                    "presented_camera_revision": camera_revision,
                    "surface_width": 2,
                    "surface_height": 1,
                },
            }
        )
        ledger.append(
            {
                "ticket": ticket,
                "source": active,
                "visible": min(active, 3),
                "contributor": min(active, 2),
                "drawn": min(active, 3),
                "identity": {
                    "scene_generation": 1,
                    "camera_revision": camera_revision,
                    "viewport_generation": 2,
                    "contract_generation": 3,
                    "plan_set_generation": 4,
                    "order_generation": 5 + index,
                    "raster_generation": 6,
                    "encode_attempt": 7 + index,
                    "presentation_sequence": presentation,
                    "executed_plan": plan,
                },
            }
        )
    image = png_fixture()
    image_path = artifact / "final-frame.png"
    image_path.write_bytes(image)
    run_id = f"run-{lane}"
    exactness = {
        field: active
        for field in (
            "source_splat_count",
            "decoded_splat_count",
            "encoded_splat_count",
            "resident_splat_count",
            "addressable_splat_count",
        )
    }
    exactness.update(
        {
            "source_sh_degree": 3,
            "resident_sh_degree": 3,
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "sh_degree_policy": "source",
            "partial_scene_published": False,
            "full_quality": True,
        }
    )
    manifest = {
        "run_id": run_id,
        "dataset": {**input_identity, "splat_count": active, "sh_degree": 3},
        "exactness": exactness,
        "renderer": {
            "backend": COLLECTOR.ENDPOINT_BACKEND,
            "path": "packed_atlas",
            "order_backend_requested": spec.order_backend,
        },
        "environment": {"android_device_receipt": environment_receipt},
        "build": {
            "dirty": False,
            "repository_commit": "b" * 40,
            "profile": "fixture",
            "package_version": "0.1.3",
        },
        "image": {
            "path": image_path.name,
            "sha256": hashlib.sha256(image).hexdigest(),
            "width": 2,
            "height": 1,
        },
        "android_device_png_pull": {
            "schema": COLLECTOR.ANDROID.DEVICE_PNG_PULL_RECEIPT_SCHEMA,
            "source": "adb-exec-out-run-as-after-benchmark-complete",
            "device_path": COLLECTOR.ANDROID.INTERNAL_FINAL_PNG,
            "device_path_absent_after_package_clear": True,
            "benchmark_run_id": run_id,
            "benchmark_completed": True,
            "pulled_after_completed_log": True,
            "local_identity": {
                "bytes": len(image),
                "sha256": hashlib.sha256(image).hexdigest(),
                "width": 2,
                "height": 1,
            },
        },
    }
    summary = {"current_stats_terminal_ledger": ledger}
    (artifact / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    (artifact / "summary.json").write_text(json.dumps(summary), encoding="utf-8")
    (artifact / "frames.jsonl").write_text(
        "".join(json.dumps(frame) + "\n" for frame in frames), encoding="utf-8"
    )
    experiment = {
        "schema": "gsplat-android-sort-experiment/v1",
        "status": "complete",
        "repository": {"commit": "b" * 40, "dirty": False},
        "configuration": {
            "backends": [spec.order_backend],
            "repetitions": 1,
            "frames": spec.measured_frames,
            "warmup": COLLECTOR.WARMUP_FRAMES,
            "sort_interval": 1,
            "async_sort": False,
            "frame_latency": 2,
            "geometry_path": "packed",
            "gpu_producer": None,
            "capture_final_png": True,
            "formal_artifact": False,
            "max_thermal_status": 0,
            "apk_mode": "reuse-exact-installed",
        },
        "dataset": {"path": str(input_path), **input_identity},
        "trace": trace_identity,
        "apk": {"bytes": 1, "sha256": "2" * 64},
        "native_library": {"bytes": 1, "sha256": "3" * 64},
        "installed_apk": {"bytes": 1, "sha256": "2" * 64},
        "runs": [
            {
                "status": "complete",
                "backend": spec.order_backend,
                "thermal_status_before": 0,
                "artifact": "run/artifact",
            }
        ],
    }
    (raw_root / "experiment.json").write_text(json.dumps(experiment), encoding="utf-8")
    return raw_root


class ScalableS1A065CollectorTests(unittest.TestCase):
    def test_schedule_freezes_both_orders_views_moving_and_replacement(self) -> None:
        specs = COLLECTOR.capture_specs()
        self.assertEqual(len(specs), 36)
        keys = {
            (
                item.order_backend,
                item.cut_name,
                item.sequence,
                item.capture_index,
                item.trace_frame_index,
                item.measured_frames,
            )
            for item in specs
        }
        self.assertEqual(len(keys), 36)
        for order in COLLECTOR.REQUIRED_ORDERS:
            for cut in COLLECTOR.REQUIRED_CUTS:
                moving = [
                    item
                    for item in specs
                    if item.order_backend == order
                    and item.cut_name == cut
                    and item.sequence == "moving_sequence"
                ]
                self.assertEqual(
                    [(item.trace_frame_index, item.measured_frames) for item in moving],
                    [(0, 1), (1, 2), (0, 3)],
                )
            replacement = [
                item.cut_name
                for item in specs
                if item.order_backend == order
                and item.sequence == "replacement_sequence"
            ]
            self.assertEqual(replacement, list(COLLECTOR.REPLACEMENT_CUTS))

    def test_author_receipt_preserves_complete_S_R_and_materialized_P(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            receipt_path, source = author_fixture(root)
            with mock.patch.object(COLLECTOR, "FORMAL_SOURCE", source):
                package = COLLECTOR.validate_author_package(receipt_path)
        self.assertEqual(package.cuts["complete_leaf_exact"].active_splats, 5)
        self.assertEqual(package.cuts["bootstrap_roots"].active_splats, 1)
        self.assertEqual(package.cuts["mixed_depth_two_replacements"].active_splats, 3)
        for cut in package.cuts.values():
            self.assertEqual(cut.coverage["source_splat_count"], 5)
            self.assertEqual(cut.coverage["represented_source_leaves"], 5)

    def test_child_commands_use_capture_only_and_never_build_or_install(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            receipt_path, source = author_fixture(root)
            args = args_fixture(root, receipt_path)
            with mock.patch.object(COLLECTOR, "FORMAL_SOURCE", source):
                package = COLLECTOR.validate_author_package(receipt_path)
            moving = next(
                item
                for item in COLLECTOR.capture_specs()
                if item.order_backend == "gpu"
                and item.cut_name == "mixed_depth_two_replacements"
                and item.sequence == "moving_sequence"
                and item.capture_index == 2
            )
            command = COLLECTOR.collector_command(
                args, moving, "proxy", package, root / "raw"
            )
        self.assertIn("--capture-final-png", command)
        self.assertNotIn("--formal-artifact", command)
        self.assertNotIn("--prepare-apk", command)
        self.assertEqual(command[command.index("--geometry-path") + 1], "packed")
        self.assertEqual(command[command.index("--backend") + 1], "gpu")
        self.assertEqual(command[command.index("--camera-frame-indices") + 1], "0,1")
        self.assertEqual(command[command.index("--frames") + 1], "3")
        self.assertEqual(command[command.index("--max-thermal-status") + 1], "0")

    def test_raw_proxy_authority_is_actual_input_P_and_join_is_read_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            input_path = root / "proxy.ply"
            input_path.write_bytes(b"proxy")
            trace_path = root / "trace.json"
            spec = COLLECTOR.CaptureSpec(
                "cpu", "bootstrap_roots", "authored_views", 0, 0, 1, "fixed"
            )
            raw_root = raw_capture_fixture(root, spec, input_path, 1, lane="proxy")
            before = {
                path.relative_to(raw_root): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in raw_root.rglob("*")
                if path.is_file()
            }
            with (
                mock.patch.object(COLLECTOR, "FORMAL_TRACE", trace_path),
                mock.patch.object(COLLECTOR, "FORMAL_SIZE", (2, 1)),
                mock.patch.object(COLLECTOR.ANDROID, "validate_run_artifact"),
            ):
                evidence = COLLECTOR.read_lane_evidence(
                    raw_root, spec, "proxy", input_path, 1
                )
            after = {
                path.relative_to(raw_root): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in raw_root.rglob("*")
                if path.is_file()
            }
        self.assertEqual(evidence.dataset["splat_count"], 1)
        self.assertEqual(evidence.counts["source"], 1)
        self.assertEqual(before, after)

    def test_raw_proxy_rejects_S_substituted_for_actual_input_P(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            input_path = root / "proxy.ply"
            input_path.write_bytes(b"proxy")
            trace_path = root / "trace.json"
            spec = COLLECTOR.CaptureSpec(
                "cpu", "bootstrap_roots", "authored_views", 0, 0, 1, "fixed"
            )
            raw_root = raw_capture_fixture(root, spec, input_path, 5, lane="proxy")
            with (
                mock.patch.object(COLLECTOR, "FORMAL_TRACE", trace_path),
                mock.patch.object(COLLECTOR, "FORMAL_SIZE", (2, 1)),
                mock.patch.object(COLLECTOR.ANDROID, "validate_run_artifact"),
                self.assertRaisesRegex(COLLECTOR.RejectedCollection, "actual input P"),
            ):
                COLLECTOR.read_lane_evidence(raw_root, spec, "proxy", input_path, 1)

    def test_stale_png_current_stats_presentation_join_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            input_path = root / "proxy.ply"
            input_path.write_bytes(b"proxy")
            trace_path = root / "trace.json"
            spec = COLLECTOR.CaptureSpec(
                "cpu", "bootstrap_roots", "authored_views", 0, 0, 1, "fixed"
            )
            raw_root = raw_capture_fixture(root, spec, input_path, 1, lane="proxy")
            frames_path = raw_root / "run/artifact/frames.jsonl"
            frame = json.loads(frames_path.read_text())
            frame["current_stats_presentation_sequence"] += 1
            frames_path.write_text(json.dumps(frame) + "\n", encoding="utf-8")
            with (
                mock.patch.object(COLLECTOR, "FORMAL_TRACE", trace_path),
                mock.patch.object(COLLECTOR, "FORMAL_SIZE", (2, 1)),
                mock.patch.object(COLLECTOR.ANDROID, "validate_run_artifact"),
                self.assertRaisesRegex(COLLECTOR.RejectedCollection, "stale"),
            ):
                COLLECTOR.read_lane_evidence(raw_root, spec, "proxy", input_path, 1)

    def test_pair_join_preserves_raw_images_and_requires_matching_present_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            trace = {
                "trace_id": "fixture-trace",
                "content_sha256": "1" * 64,
                "frames": [{"pose": {}, "intrinsics": {}}],
            }
            trace_path = root / "trace.json"
            trace_path.write_text(json.dumps(trace), encoding="utf-8")
            exact_path = root / "exact.png"
            proxy_path = root / "proxy.png"
            exact_path.write_bytes(png_fixture())
            proxy_path.write_bytes(png_fixture())
            artifact_exact = root / "raw/exact"
            artifact_proxy = root / "raw/proxy"
            artifact_exact.mkdir(parents=True)
            artifact_proxy.mkdir(parents=True)
            build = {"repository_commit": "b" * 40, "dirty": False}
            package_identity = {"apk": {}, "native_library": {}, "installed_apk": {}}
            environment = {"model": "A065"}
            presentation = {
                "outcome": "presented",
                "primitive_presented": True,
                "ticket": 1,
                "scene_generation": 1,
                "camera_generation": 2,
                "viewport_generation": 3,
                "contract_generation": 4,
                "plan_generation": 5,
                "presentation_generation": 6,
                "order_generation": 7,
                "raster_generation": 8,
                "encode_attempt": 9,
                "image_join": {},
            }

            def lane(name: str, image: pathlib.Path, active: int) -> COLLECTOR.LaneEvidence:
                return COLLECTOR.LaneEvidence(
                    lane=name,
                    raw_root=image.parent,
                    artifact_path=artifact_exact if name == "exact" else artifact_proxy,
                    artifact_sha256=("a" if name == "exact" else "c") * 64,
                    run_id=f"run-{name}",
                    frame_index=0,
                    dataset={
                        "sha256": hashlib.sha256(name.encode()).hexdigest(),
                        "bytes": len(name),
                        "splat_count": active,
                        "sh_degree": 3,
                    },
                    exactness={},
                    image={
                        "path": image.name,
                        "absolute_path": image,
                        "sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
                        "width": 2,
                        "height": 1,
                    },
                    presentation={**presentation, "ticket": 1 if name == "exact" else 2},
                    camera_revision=2,
                    trace_frame_index=0,
                    counts={
                        "source": active,
                        "visible": active,
                        "contributor": active,
                        "drawn": active,
                        "exact_contributor_compaction": False,
                    },
                    executed_plan="cpu_post_sort",
                    build=build,
                    package_identity=package_identity,
                    environment_receipt=environment,
                )

            exact = lane("exact", exact_path, 5)
            proxy = lane("proxy", proxy_path, 1)
            cut_coverage = {
                "source_splat_count": 5,
                "represented_source_leaves": 5,
                "hierarchy_manifest_sha256": "d" * 64,
            }
            cut = COLLECTOR.CutInput(
                "bootstrap_roots",
                cut_coverage,
                COLLECTOR.S1.canonical_sha256(cut_coverage),
                {"P": 1},
                root / "proxy.ply",
            )
            spec = COLLECTOR.CaptureSpec(
                "cpu", "bootstrap_roots", "authored_views", 0, 0, 1, "fixed"
            )
            before = (exact_path.read_bytes(), proxy_path.read_bytes())
            with (
                mock.patch.object(COLLECTOR, "FORMAL_TRACE", trace_path),
                mock.patch.object(COLLECTOR, "FORMAL_SIZE", (2, 1)),
                mock.patch.dict(
                    COLLECTOR.FORMAL_SOURCE,
                    {"splat_count": 5, "sha256": "e" * 64},
                    clear=False,
                ),
            ):
                comparison, _ = COLLECTOR.compare_lanes(
                    spec, cut, exact, proxy, root
                )
                mismatched = dataclasses.replace(
                    proxy,
                    presentation={**proxy.presentation, "presentation_generation": 99},
                )
                with self.assertRaisesRegex(
                    COLLECTOR.RejectedCollection, "presentation_generation"
                ):
                    COLLECTOR.compare_lanes(spec, cut, exact, mismatched, root)
            after = (exact_path.read_bytes(), proxy_path.read_bytes())
        self.assertTrue(comparison["frame_gate_pass"])
        self.assertEqual(
            comparison["raw_dataset_authority"]["exact"],
            {
                "sha256": hashlib.sha256(b"exact").hexdigest(),
                "bytes": 5,
                "authority_symbol": "S",
                "active_splats": 5,
            },
        )
        self.assertEqual(
            comparison["raw_dataset_authority"]["proxy"]["authority_symbol"],
            "P",
        )
        self.assertEqual(
            comparison["raw_dataset_authority"]["proxy"]["active_splats"], 1
        )
        self.assertEqual(
            comparison["presented_cut"]["coverage_evidence"]["represented_source_leaves"],
            5,
        )
        self.assertIn("not_runtime_coverage_generation", comparison["presented_cut"]["coverage_evidence"]["identity_semantics"])
        self.assertEqual(before, after)

    def test_failed_child_command_is_not_retried(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            receipt_path, source = author_fixture(root)
            args = args_fixture(root, receipt_path)
            args.dry_run = False
            with mock.patch.object(COLLECTOR, "FORMAL_SOURCE", source):
                package = COLLECTOR.validate_author_package(receipt_path)
            output = root / "output"
            output.mkdir()
            one_spec = COLLECTOR.CaptureSpec(
                "cpu", "bootstrap_roots", "authored_views", 0, 0, 1, "fixed"
            )
            failed = COLLECTOR.subprocess.CompletedProcess([], 1)
            with (
                mock.patch.object(COLLECTOR, "validate_formal_preflight", return_value={}),
                mock.patch.object(
                    COLLECTOR,
                    "repository_identity",
                    return_value={"commit": "b" * 40, "dirty": False},
                ),
                mock.patch.object(COLLECTOR, "admit_device", return_value={}),
                mock.patch.object(COLLECTOR, "capture_specs", return_value=[one_spec]),
                mock.patch.object(COLLECTOR.subprocess, "run", return_value=failed) as run,
                self.assertRaisesRegex(COLLECTOR.RejectedCollection, "no retry"),
            ):
                COLLECTOR.run_collection(args, package, output)
        run.assert_called_once()

    def test_unavailable_doctor_is_deferred_once_before_collection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            receipt_path, source = author_fixture(root)
            args = args_fixture(root, receipt_path)
            args.dry_run = False
            with mock.patch.object(COLLECTOR, "FORMAL_SOURCE", source):
                package = COLLECTOR.validate_author_package(receipt_path)
            with (
                mock.patch.object(
                    COLLECTOR.subprocess,
                    "run",
                    side_effect=subprocess.TimeoutExpired("doctor", 1),
                ) as run,
                self.assertRaisesRegex(
                    COLLECTOR.DeferredCollection, "doctor is unavailable"
                ),
            ):
                COLLECTOR.admit_device(args, package, root / "output")
        run.assert_called_once()

    def test_transition_gate_reloads_only_the_needed_retained_images(self) -> None:
        comparisons = []
        for spec in COLLECTOR.capture_specs():
            comparisons.append(
                {
                    "pair_id": spec.pair_id,
                    "order_backend": spec.order_backend,
                    "cut_name": spec.cut_name,
                    "sequence": spec.sequence,
                    "capture_index": spec.capture_index,
                    "trace_frame_index": spec.trace_frame_index,
                    "images": {
                        "exact": {"path": f"{spec.slug}/exact.png"},
                        "proxy": {"path": f"{spec.slug}/proxy.png"},
                    },
                }
            )
        with (
            mock.patch.object(COLLECTOR.BALANCED, "load_image", return_value=object()) as load,
            mock.patch.object(
                COLLECTOR.BALANCED, "compute_temporal_metric", return_value=0.0
            ) as temporal,
        ):
            transitions = COLLECTOR.compute_transitions(
                comparisons, pathlib.Path("/unused")
            )
        self.assertEqual(len(transitions), 16)
        self.assertEqual(load.call_count, 64)
        self.assertEqual(temporal.call_count, 16)
        self.assertTrue(all(item["transition_gate_pass"] for item in transitions))

    def test_missing_author_is_one_finite_deferred_attempt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            output = root / "fresh-output"
            with mock.patch("builtins.print"):
                result = COLLECTOR.main(
                    [
                        "--serial",
                        "fixture-serial",
                        "--author-receipt",
                        str(root / "missing-receipt.json"),
                        "--output",
                        str(output),
                    ]
                )
            decision = json.loads((output / "decision.json").read_text())
        self.assertEqual(result, 2)
        self.assertEqual(decision["decision"], "Deferred")
        self.assertEqual(decision["attempt"], 1)
        self.assertEqual(decision["retry_policy"], "none")
        self.assertFalse(decision["collection_started"])

    def test_existing_output_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "existing"
            output.mkdir()
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                COLLECTOR.claim_output(output)


if __name__ == "__main__":
    unittest.main()
