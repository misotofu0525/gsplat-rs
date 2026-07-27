#!/usr/bin/env python3
"""Focused fixtures for Q1 Truck schedule, admission, and finite verdicts."""

from __future__ import annotations

import hashlib
import json
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from datetime import datetime, timedelta, timezone

from unittest import mock

from q1_pair_admission.artifacts import (
    BUILD_ARTIFACT_KEYS,
    HOST_ADMISSION_JOIN_SCHEMA,
    IMAGE_TOOL_SHA256,
    PLAYCANVAS_RGBA_UNAVAILABLE,
    REFERENCE_RUST_TOOLCHAIN_SHA256,
    REFERENCE_SOURCE_PATHS,
    REFERENCE_TRACE_FILE_SHA256,
    reference_authority,
)
from q1_pair_admission.common import ValidationError, canonical_sha256, file_sha256
from q1_pair_admission.contract import (
    ADAPTER_IDENTITY_STATUS,
    ADAPTER_SELECTION_CLASS,
    CANONICAL_ADAPTER_SCHEMA,
    CANONICAL_SUPPORTED_LIMIT_NAMES,
    CANONICAL_SUPPORTED_LIMITS_SCHEMA,
    HEIGHT,
    IMAGE_SCHEMA,
    MEASURED,
    PLAYCANVAS,
    SCHEMA,
    TERMINAL_SCHEMA,
    TRACE,
    TRACE_FRAME_POSE_INTRINSICS_SHA256,
    TRUCK,
    WARMUP,
    WIDTH,
    WEBGPU_ENVIRONMENT_SCHEMA,
)
from q1_pair_admission.evaluate import admission_rejection, evaluate


SHA_A = "a" * 64
SHA_B = "b" * 64
COMMIT = "c" * 40
PREDECLARED = "2026-07-28T00:00:00Z"
NORMALIZED_BROWSER_ARGS = [
    "<browser-executable>",
    "--enable-unsafe-webgpu",
    "--enable-gpu",
    "--ignore-gpu-blocklist",
    "--remote-debugging-port=<ephemeral-port>",
    "--user-data-dir=<ephemeral-profile>",
]
NORMALIZED_BROWSER_ARGS_SHA = hashlib.sha256(
    json.dumps(NORMALIZED_BROWSER_ARGS, separators=(",", ":")).encode()
).hexdigest()
PROCESS_ARGS_RECEIPT = {
    "schema": "gsplat-q1-browser-process-args/v1",
    "source": "node_child_process_spawnargs",
    "normalized_args": NORMALIZED_BROWSER_ARGS,
    "redactions": [
        {"index": 4, "kind": "ephemeral_remote_debugging_port"},
        {"index": 5, "kind": "ephemeral_user_data_dir"},
    ],
    "normalized_sha256": NORMALIZED_BROWSER_ARGS_SHA,
}
ORDERS = ["playcanvas-first", "gsplat-rs-first", "playcanvas-first", "gsplat-rs-first", "playcanvas-first"]
CLI = pathlib.Path(__file__).with_name("validate-q1-truck-paired-comparison.py")
PLAYCANVAS_CAMERA_AUTHORITY_GENERATOR = pathlib.Path(__file__).parent / (
    "q1_pair_admission/generate_playcanvas_camera_authority.mjs"
)
PLAYCANVAS_CAMERA_AUTHORITY_FIXTURE = pathlib.Path(__file__).parent / (
    "q1_pair_admission/fixtures/playcanvas-truck-camera-receipts-v1.json"
)
PLAYCANVAS_CAMERA_RECEIPTS = json.loads(
    PLAYCANVAS_CAMERA_AUTHORITY_FIXTURE.read_text(encoding="utf-8")
)["receipts"]
REPO = pathlib.Path(__file__).parents[2]


def webgpu_limits(offset: int = 0, *, extra: bool = False) -> dict[str, int]:
    limits = {
        name: 1000 + index + offset
        for index, name in enumerate(CANONICAL_SUPPORTED_LIMIT_NAMES)
    }
    if extra:
        limits["futureEndpointOnlyLimit"] = 9999 + offset
    return limits


def webgpu_environment_receipt(endpoint: str) -> dict[str, object]:
    canonical = webgpu_limits()
    if endpoint == "playcanvas":
        adapter_provenance = "playcanvas_graphicsDevice.gpuAdapter"
        device_provenance = "playcanvas_graphicsDevice.wgpu"
        info_status = "browser_exposed"
        info: dict[str, object] = {
            "vendor": "Apple",
            "architecture": "apple8",
            "device": "M4",
            "description": "Metal",
            "subgroupMinSize": 4,
            "subgroupMaxSize": 32,
        }
        adapter_limits = webgpu_limits(extra=True)
        device_limits = webgpu_limits(-100)
    else:
        adapter_provenance = "gsplat_surface_session.wgpu_adapter"
        device_provenance = "gsplat_surface_session.wgpu_device"
        info_status = "unavailable_wgpu28_browser_backend"
        info = {
            "name": "",
            "vendor_id": 0,
            "device_id": 0,
            "device_type": "Other",
            "driver": "",
            "driver_info": "",
            "backend": "BrowserWebGpu",
        }
        adapter_limits = webgpu_limits()
        device_limits = webgpu_limits(-200, extra=True)
    return {
        "schema": WEBGPU_ENVIRONMENT_SCHEMA,
        "endpoint": endpoint,
        "selected_adapter": {
            "provenance": adapter_provenance,
            "info_status": info_status,
            "info": info,
            "supported_limits": adapter_limits,
        },
        "selected_device": {
            "provenance": device_provenance,
            "effective_limits": device_limits,
        },
        "canonical_adapter": {
            "schema": CANONICAL_ADAPTER_SCHEMA,
            "selection_class": ADAPTER_SELECTION_CLASS,
            "backend_class": "browser_webgpu",
            "hardware_identity_status": ADAPTER_IDENTITY_STATUS,
            "supported_limits_schema": CANONICAL_SUPPORTED_LIMITS_SCHEMA,
            "supported_limits": canonical,
        },
    }


def webgpu_environment_fields(endpoint: str) -> dict[str, object]:
    receipt = webgpu_environment_receipt(endpoint)
    adapter_limits = receipt["selected_adapter"]["supported_limits"]
    device_limits = receipt["selected_device"]["effective_limits"]
    canonical_limits = receipt["canonical_adapter"]["supported_limits"]
    return {
        "adapter": ADAPTER_SELECTION_CLASS,
        "adapter_identity_status": ADAPTER_IDENTITY_STATUS,
        "canonical_adapter_supported_limits_sha256": canonical_sha256(canonical_limits),
        "adapter_supported_limits_sha256": canonical_sha256(adapter_limits),
        "device_effective_limits_sha256": canonical_sha256(device_limits),
        "webgpu_device_environment_receipt": receipt,
    }


def write_json(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"{json.dumps(value, indent=2)}\n", encoding="utf-8")


def rgba_png_bytes(value: int = 0, *, ancillary: bytes | None = None) -> bytes:
    pixel = bytes((value, value, value, 255))
    row = pixel * WIDTH
    filtered = (b"\0" + row) * HEIGHT

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 6, 0, 0, 0))
        + (chunk(b"tEXt", ancillary) if ancillary is not None else b"")
        + chunk(b"IDAT", zlib.compress(filtered, 9))
        + chunk(b"IEND", b"")
    )


def write_rgba_png(path: pathlib.Path, value: int = 0) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(rgba_png_bytes(value))
    return file_sha256(path)


def write_reencoded_rgba_png(path: pathlib.Path, value: int = 0) -> str:
    path.write_bytes(rgba_png_bytes(value, ancillary=b"noncanonical=reencode"))
    return file_sha256(path)


def local_identity(relative: str) -> dict[str, object]:
    path = REPO / relative
    return {
        "path": relative,
        "bytes": path.stat().st_size,
        "sha256": file_sha256(path),
    }


def install_reference_authority(
    root: pathlib.Path, *, repository_commit: str = COMMIT
) -> list[dict[str, object]]:
    authority = root / "reference-authority"
    retained_binary = authority / "producer/desktop-example"
    retained_binary.parent.mkdir(parents=True)
    retained_binary.write_bytes(b"locked Direct-f32 reference producer\n")
    binary_sha = file_sha256(retained_binary)
    trace_path = REPO / (
        "tests/perf/trace/fixtures/quality/"
        "candidate-truck-quality-1920x1080-v1.json"
    )
    trace_document = json.loads(trace_path.read_text(encoding="utf-8"))
    trace_frames = []
    captures = []
    references = []
    for frame in trace_document["frames"]:
        trace = int(frame["frame_index"])
        semantic = {"pose": frame["pose"], "intrinsics": frame["intrinsics"]}
        pose_sha = canonical_sha256(semantic)
        trace_frames.append(
            {
                "frame_index": trace,
                **semantic,
                "pose_intrinsics_sha256": pose_sha,
            }
        )
        image_name = f"reference-trace-{trace}.png"
        image_path = authority / image_name
        image_sha = write_rgba_png(image_path)
        decoded_sha = solid_rgba_sha256()
        visible = 1_800_000 + trace
        renderer_receipt = {
            "schema": "gsplat-direct-f32-offscreen-receipt/v1",
            "geometry_path": "sorted_index_direct",
            "representation": "wide_f32",
            "render_mode": "sorted_alpha",
            "order_backend": "cpu",
            "depth_key_precision": "exact_full32",
            "stable_source_id_order": "true",
            "raster_execution_plan": "wgpu_direct_global_quads",
            "gpu_rasterizer": "true",
            "source_count": str(TRUCK["splat_count"]),
            "decoded_count": str(TRUCK["splat_count"]),
            "encoded_count": str(TRUCK["splat_count"]),
            "resident_count": str(TRUCK["splat_count"]),
            "addressable_count": str(TRUCK["splat_count"]),
            "source_sh_degree": "3",
            "resident_sh_degree": "3",
            "requested_width": str(WIDTH),
            "requested_height": str(HEIGHT),
            "internal_render_width": str(WIDTH),
            "internal_render_height": str(HEIGHT),
            "readback_width": str(WIDTH),
            "readback_height": str(HEIGHT),
            "readback_format": "rgba8_unorm",
            "readback_row_origin": "top_left",
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "partial_scene_published": "false",
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "adapter_backend": "Metal",
            "adapter_device_type": "IntegratedGpu",
            "adapter_vendor": "0",
            "adapter_device": "0",
            "visible_count": str(visible),
            "drawn_count": str(visible),
        }
        captures.append(
            {
                "frame_index": trace,
                "pose_intrinsics_sha256": pose_sha,
                "renderer_receipt": renderer_receipt,
                "visible_count": visible,
                "drawn_count": visible,
                "image": {
                    "path": image_name,
                    "bytes": image_path.stat().st_size,
                    "sha256": image_sha,
                    "width": WIDTH,
                    "height": HEIGHT,
                    "format": "rgba8",
                    "decoded_rgba8_sha256": decoded_sha,
                },
            }
        )
        references.append(
            {
                "trace_frame_index": trace,
                "path": image_path.relative_to(root).as_posix(),
                "sha256": image_sha,
                "decoded_rgba8_sha256": decoded_sha,
                "pose_intrinsics_sha256": pose_sha,
            }
        )
    exactness = {
        "source_count": TRUCK["splat_count"],
        "decoded_count": TRUCK["splat_count"],
        "encoded_count": TRUCK["splat_count"],
        "resident_count": TRUCK["splat_count"],
        "addressable_count": TRUCK["splat_count"],
        "source_sh_degree": 3,
        "resident_sh_degree": 3,
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "partial_scene_published": False,
    }
    binary_identity = {
        "path": "/discarded/build/desktop-example",
        "bytes": retained_binary.stat().st_size,
        "sha256": binary_sha,
    }
    repository = {"commit": repository_commit, "clean": True}
    source_identities = [local_identity(path) for path in REFERENCE_SOURCE_PATHS]
    receipt = {
        "schema": "gsplat-q1-direct-f32-reference/v1",
        "status": "accepted",
        "generated_at_utc": "2026-07-28T00:00:00Z",
        "repository": repository,
        "cargo_lock": local_identity("Cargo.lock"),
        "rust_toolchain": {
            **local_identity("rust-toolchain.toml"),
            "channel": "1.93.0",
            "profile": "default",
            "components": ["rustfmt", "clippy"],
        },
        "toolchain": {
            "rustc": {"path": "/toolchain/rustc", "version": "rustc 1.93.0"},
            "cargo": {"path": "/toolchain/cargo", "version": "cargo 1.93.0"},
        },
        "release_binary": {
            **binary_identity,
            "retained": {
                "path": "producer/desktop-example",
                "bytes": retained_binary.stat().st_size,
                "sha256": binary_sha,
            },
        },
        "producer_sources": source_identities,
        "dataset": {
            "path": "tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
            "bytes": TRUCK["bytes"],
            "sha256": TRUCK["sha256"],
            "id": "truck-full",
            "splat_count": TRUCK["splat_count"],
            "sh_degree": 3,
        },
        "trace": {
            "path": trace_path.relative_to(REPO).as_posix(),
            "bytes": trace_path.stat().st_size,
            "sha256": REFERENCE_TRACE_FILE_SHA256,
            "trace_id": TRACE["id"],
            "semantic_sha256": TRACE["sha256"],
            "frames": trace_frames,
        },
        "exactness": exactness,
        "execution": {
            "geometry_path": "sorted_index_direct",
            "representation": "wide_f32",
            "order_backend": "cpu",
            "depth_key_precision": "exact_full32",
            "stable_source_id_order": True,
            "render_mode": "sorted_alpha",
            "raster_execution_plan": "wgpu_direct_global_quads",
            "gpu_rasterizer": True,
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "requested": {"width": WIDTH, "height": HEIGHT},
            "internal_render": {"width": WIDTH, "height": HEIGHT, "format": "rgba8_unorm"},
            "readback": {"width": WIDTH, "height": HEIGHT, "format": "rgba8", "row_origin": "top_left"},
        },
        "environment": {
            "adapter_backend": "Metal",
            "adapter_device_type": "IntegratedGpu",
            "adapter_vendor": "0",
            "adapter_device": "0",
        },
        "captures": captures,
        "integrity": {
            "repository_pre": repository,
            "repository_post": repository,
            "binary_pre": binary_identity,
            "binary_post": binary_identity,
            "producer_sources_pre_sha256": canonical_sha256(source_identities),
            "producer_sources_post_sha256": canonical_sha256(source_identities),
            "inputs_rechecked_after_render": True,
        },
    }
    receipt_path = authority / "reference.json"
    write_json(receipt_path, receipt)
    receipt_sha = file_sha256(receipt_path)
    for reference in references:
        reference.update(
            {
                "authority_receipt_path": receipt_path.relative_to(root).as_posix(),
                "authority_receipt_sha256": receipt_sha,
            }
        )
    assert file_sha256(REPO / "rust-toolchain.toml") == REFERENCE_RUST_TOOLCHAIN_SHA256
    return references


def solid_rgba_sha256(value: int = 0) -> str:
    return hashlib.sha256(bytes((value, value, value, 255)) * (WIDTH * HEIGHT)).hexdigest()


def gsplat_renderer_rgba(
    camera_revision: int,
    presentation_sequence: int,
) -> dict[str, object]:
    return {
        "scene_generation": 1,
        "camera_revision": camera_revision,
        "viewport_generation": 0,
        "contract_generation": 1,
        "plan_set_generation": 2,
        "plan_id": "GpuPreproject",
        "order_generation": camera_revision,
        "presentation_sequence": presentation_sequence,
        "width": WIDTH,
        "height": HEIGHT,
        "rgba8_sha256": solid_rgba_sha256(),
        "profile": "ExactFull32",
    }


def host_admission_join(
    png_sha256: str, producer_path: pathlib.Path, producer_sha256: str
) -> dict[str, object]:
    return {
        "schema": HOST_ADMISSION_JOIN_SCHEMA,
        "source": "decoded_png_rgba8_to_renderer_receipt",
        "pixel_format": "rgba8unorm-srgb",
        "width": WIDTH,
        "height": HEIGHT,
        "producer_artifact_path": producer_path.as_posix(),
        "producer_manifest_sha256": producer_sha256,
        "source_rgba8_sha256": solid_rgba_sha256(),
        "png_sha256": png_sha256,
    }


def build_artifact_receipts(root: pathlib.Path, endpoint: str) -> dict[str, object]:
    receipts: dict[str, object] = {}
    for name in sorted(BUILD_ARTIFACT_KEYS[endpoint]):
        path = pathlib.Path("build") / endpoint / name
        absolute = root / path
        absolute.parent.mkdir(parents=True, exist_ok=True)
        if not absolute.exists():
            absolute.write_bytes(f"{endpoint}:{name}:locked\n".encode())
        receipts[name] = {"path": path.as_posix(), "sha256": file_sha256(absolute)}
    return receipts


def distribution(value: float | None) -> dict[str, float | int] | None:
    if value is None:
        return None
    return {"count": MEASURED, "mean": value, "p50": value, "p90": value, "p95": value, "p99": value, "max": value}


def frame(run_id: str, index: int, endpoint: str, role: str) -> dict[str, object]:
    value: dict[str, object] = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "frame",
        "run_id": run_id,
        "frame_index": index,
        "elapsed_ns": index * 1_000_000,
        "call_ms": 1.0,
        "frame_wall_ms": 10.0,
        "preprocess_ms": None,
        "sort_ms": None,
        "geometry_submit_ms": None,
        "gpu_wait_ms": None,
        "gpu_complete_ms": None,
        "visible": None,
        "drawn": None,
        "sort_refreshed": True,
        "trace_frame_index": index % 2,
        "camera_revision": index + 1,
        "presentation_sequence": index + 1,
    }
    if endpoint == "playcanvas":
        value["active_splats"] = TRUCK["splat_count"]
    else:
        value.update({
            "order_backend": "gpu",
            "gpu_order_producer": "preproject",
            "projected_execution": "compact",
            "raster_execution_plan": "projected_quads_exact",
            "gpu_sort_fallback": False,
        })
        if role == "control":
            value.update({"visible": 1_500_000, "contributor": 900_000, "drawn": 900_000, "exact_contributor_compaction": True})
    return value


def summary(run_id: str, terminal_ms: float | None) -> dict[str, object]:
    value: dict[str, object] = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": MEASURED,
        "warmup_count": WARMUP,
        "frame_budget_ms": 16.666666666666668,
        "missed_frame_count": 0,
        "distributions": {
            "call_ms": distribution(1.0),
            "frame_wall_ms": distribution(10.0),
            "preprocess_ms": None,
            "sort_ms": None,
            "geometry_submit_ms": None,
            "gpu_wait_ms": None,
            "gpu_complete_ms": None,
        },
    }
    if terminal_ms is not None:
        value["sustained_throughput"] = {
            "measured_frame_count": MEASURED,
            "terminal_window_ms": terminal_ms,
            "mean_frame_ms": terminal_ms / MEASURED,
            "mean_fps": 1000 * MEASURED / terminal_ms,
        }
    return value


def terminal(duration: float) -> dict[str, object]:
    return {
        "schema": TERMINAL_SCHEMA,
        "clock": "performance_now_monotonic",
        "start_boundary": "first_measured_camera_input_accepted",
        "end_boundary": "final_measured_gpu_queue_completion",
        "completion_primitive": "gpu_queue_on_submitted_work_done",
        "frame_loop_policy": "controlled_presented_raf",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_queue_drained": True,
        "continuous_submissions": True,
        "per_frame_observer_reads": 0,
        "extra_submissions_during_terminal_drain": 0,
        "measured_camera_input_count": MEASURED,
        "measured_submission_count": MEASURED,
        "dropped_frame_count": 0,
        "submission_counter_stable_during_drain": True,
        "submission_counter_before_first": 40,
        "submission_counter_after_last": 120,
        "started_at_monotonic_ms": 100.0,
        "completed_at_monotonic_ms": 100.0 + duration,
        "duration_ms": duration,
    }


def presentation(
    trace: int, run_id: str, frames: list[dict[str, object]]
) -> dict[str, object]:
    terminal_index = MEASURED - 2 + trace
    terminal_frame = frames[terminal_index]
    result = {
        "trace_frame_index": trace,
        "camera": {
            "trace_id": TRACE["id"],
            "trace_content_sha256": TRACE["sha256"],
            "trace_frame_index": trace,
            "pose_intrinsics_sha256": TRACE_FRAME_POSE_INTRINSICS_SHA256[trace],
            "camera_revision": terminal_frame["camera_revision"],
        },
        "terminal_identity": {
            "run_id": run_id,
            "frame_index": terminal_index,
            "frame_sha256": canonical_sha256(terminal_frame),
            "presentation_sequence": terminal_frame["presentation_sequence"],
        },
        "successful_present": True,
        "queue_terminal_complete": True,
        "captured_after_terminal": True,
        "dimensions": {
            **{f"{stage}_width": WIDTH for stage in ("requested", "surface", "internal_render", "presented")},
            **{f"{stage}_height": HEIGHT for stage in ("requested", "surface", "internal_render", "presented")},
        },
    }
    return result


def playcanvas_camera_receipt(trace: int, phase: str) -> dict[str, object]:
    receipt = json.loads(json.dumps(PLAYCANVAS_CAMERA_RECEIPTS[str(trace)]))
    receipt["phase"] = phase
    return receipt


def legacy_playcanvas_presentation(trace: int) -> tuple[dict[str, object], dict[str, object]]:
    frames: list[dict[str, object]] = []
    submit = 100
    for index in range(3):
        camera = playcanvas_camera_receipt(trace, f"presentation_frame_{index}")
        frames.append(
            {
                "trace_frame_index": trace,
                "camera_receipt": camera,
                "submit_version_before": submit,
                "submit_version_after": submit + 1,
                "queue_submit_call_count": 1,
            }
        )
        submit += 1
    terminal_camera = playcanvas_camera_receipt(trace, "external_capture_terminal")
    return (
        {
            "schema": "gsplat-playcanvas-presentation-capture/v1",
            "ready_for_external_capture": True,
            "excluded_from_performance": True,
            "capture_trace_frame_index": trace,
            "capture_trace_frame_source": "explicit_capture_trace_frame",
            "stable_frame_count": 3,
            "minimum_stable_frame_count": 3,
            "measurement_terminal_submit_version": 100,
            "frames": frames,
            "terminal_camera_receipt": terminal_camera,
        },
        terminal_camera,
    )


def protocol() -> dict[str, object]:
    return {
        "dataset": TRUCK,
        "trace": TRACE,
        "display": {"width": WIDTH, "height": HEIGHT, "dpr": 1},
        "camera_mode": "trace_sequence",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_frames": WARMUP,
        "measured_frames": MEASURED,
        "terminal_boundary": "first_measured_camera_input_to_final_gpu_queue_completion",
        "claim_scope": "near-contract",
        "quality_gate": {"metric": "ssim-luma-srgb-window8", "minimum_ssim": 0.99},
    }


def manifest(
    *, endpoint: str, role: str, run_id: str, series_id: str, schedule_sha: str,
    protocol_sha: str, pair_id: str, order: str, position: int, configuration: str,
    terminal_ms: float | None, started_at: str, ended_at: str,
    build_artifacts: dict[str, object],
    capture_trace: int | None = None,
) -> dict[str, object]:
    renderer = (
        {"implementation": "playcanvas-d5fe888", "path": "GSplatHybridRenderer", "backend": "webgpu", "sort_policy": "raster_gpu_sort", "uses_gpu_sort": True}
        if endpoint == "playcanvas"
        else {"implementation": "gsplat-rs", "path": "wasm_packed_atlas", "backend": "webgpu", "sort_policy": "gpu_every_frame", "order_backend_requested": "gpu", "gpu_order_producer_actual": "preproject", "projected_policy_requested": "compact", "raster_execution_plan": "projected_quads_exact", "sort_interval": 1}
    )
    if endpoint == "gsplat_rs" and role == "control":
        renderer["count_semantics"] = "candidate_visible_contributor_issued_v1"
    build: dict[str, object] = {"repository_commit": COMMIT, "dirty": False, "profile": "browser", "package_version": "0.1.3", "artifacts": build_artifacts}
    if endpoint == "playcanvas":
        build.update({"package_version": PLAYCANVAS["version"], "upstream_revision": PLAYCANVAS["revision"], "runtime_revision": PLAYCANVAS["runtime_revision"], "package_integrity": PLAYCANVAS["integrity"]})
    unavailable = ["frames[*].preprocess_ms", "frames[*].sort_ms", "frames[*].geometry_submit_ms", "frames[*].gpu_wait_ms", "frames[*].gpu_complete_ms"]
    if role == "throughput" or endpoint == "playcanvas":
        unavailable += ["frames[*].visible", "frames[*].contributor", "frames[*].drawn"]
    q1: dict[str, object] = {
        "artifact_role": role,
        "protocol_sha256": protocol_sha,
        "configuration_sha256": configuration,
        "performance_evidence": role == "throughput",
        "count_scope": {
            ("playcanvas", "control"): "full_membership_v_c_d_unavailable",
            ("playcanvas", "throughput"): "full_membership_v_c_d_unavailable",
            ("gsplat_rs", "control"): "exact_v_c_d_control_only",
            ("gsplat_rs", "throughput"): "control_bound_v_c_d_unavailable",
        }[(endpoint, role)],
    }
    if role != "control":
        q1["terminal_window"] = terminal(terminal_ms or 1.0)
    else:
        q1["capture_trace_frame_index"] = capture_trace
        if endpoint == "playcanvas":
            q1["renderer_rgba_unavailable_reason"] = PLAYCANVAS_RGBA_UNAVAILABLE
    return {
        "schema": "gsplat-benchmark/v1",
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {"series_id": series_id, "started_at_utc": started_at, "ended_at_utc": ended_at, "measurement_started_at_utc": started_at, "measurement_ended_at_utc": ended_at},
        "build": build,
        "dataset": TRUCK,
        "trace": {**TRACE, "camera_mode": "trace_sequence"},
        "renderer": renderer,
        "display": {"width": WIDTH, "height": HEIGHT, "dpr": 1, "refresh_hz": 60, "frame_budget_ms": 16.666666666666668, "refresh_hz_source": "configured", "frame_budget_source": "configured"},
        "environment": {"platform": "web", "os": "Darwin-test", "device": "M4-test", "browser": "Chrome-test", **webgpu_environment_fields(endpoint), "driver": "apple_metal_os_build:25A1", "driver_source": "macos_sw_vers_buildVersion", "browser_executable_sha256": SHA_A, "browser_launch_args_sha256": NORMALIZED_BROWSER_ARGS_SHA, "browser_launch_args_receipt": PROCESS_ARGS_RECEIPT, "power_source": "ac", "collection_session_id": "session-1", "thermal": {"source": "host-probe", "pre": "nominal", "post": "nominal", "admitted": True}},
        "unavailable_fields": unavailable,
        "exactness": {"source_splat_count": TRUCK["splat_count"], "decoded_splat_count": TRUCK["splat_count"], "encoded_splat_count": TRUCK["splat_count"], "resident_splat_count": TRUCK["splat_count"], "addressable_splat_count": TRUCK["splat_count"], "source_sh_degree": 3, "resident_sh_degree": 3, "source_membership": "all", "sampling": "disabled", "lod": "disabled", "partial_scene_published": False, "full_quality": True},
        "resolution": {**{f"{stage}_width": WIDTH for stage in ("requested", "surface", "internal_render", "presented")}, **{f"{stage}_height": HEIGHT for stage in ("requested", "surface", "internal_render", "presented")}, "dynamic_resolution": "disabled", "upscaling": "disabled", "full_resolution": True},
        "pairing": {"series_id": series_id, "schedule_sha256": schedule_sha, "pair_id": pair_id, "run_order": order, "position": position, "fresh_output": True, "automatic_retry": False},
        "q1_comparison": q1,
    }


def write_artifact(
    root: pathlib.Path,
    relative: pathlib.Path,
    doc: dict[str, object],
    endpoint: str,
    role: str,
    terminal_ms: float | None,
    capture_trace: int | None = None,
) -> pathlib.Path:
    directory = root / relative
    directory.mkdir(parents=True)
    records = [frame(str(doc["run_id"]), index, endpoint, role) for index in range(MEASURED)]
    if role == "control":
        if endpoint == "gsplat_rs":
            assert capture_trace in {0, 1}
            terminal_frame = records[MEASURED - 2 + capture_trace]
            terminal_frame["capture_depth_precision"] = gsplat_renderer_rgba(
                int(terminal_frame["camera_revision"]),
                int(terminal_frame["presentation_sequence"]),
            )
            doc["q1_comparison"]["presentation_identity"] = presentation(
                capture_trace, str(doc["run_id"]), records
            )
        else:
            assert capture_trace in {0, 1}
            native_presentation, terminal_camera = legacy_playcanvas_presentation(
                capture_trace
            )
            doc["presentation_capture"] = native_presentation
            doc["camera_receipt"] = terminal_camera
    write_json(directory / "manifest.json", doc)
    (directory / "frames.jsonl").write_text("".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8")
    write_json(directory / "summary.json", summary(str(doc["run_id"]), terminal_ms))
    return directory


def install_complete_playcanvas_producers(root: pathlib.Path, schedule_path: pathlib.Path) -> None:
    document = json.loads(schedule_path.read_text(encoding="utf-8"))
    rgba = bytes((0, 0, 0, 255)) * (WIDTH * HEIGHT)
    rgba_sha = hashlib.sha256(rgba).hexdigest()
    for pair in document["pairs"]:
        endpoint = pair["playcanvas"]
        binding_digests: dict[int, str] = {}
        for control in endpoint["controls"]:
            trace = int(control["trace_frame_index"])
            directory = root / control["artifact"]
            manifest_path = directory / "manifest.json"
            manifest_value = json.loads(manifest_path.read_text(encoding="utf-8"))
            native_presentation = manifest_value["presentation_capture"]
            final_frame = native_presentation["frames"][-1]
            renderer_submit = int(final_frame["submit_version_after"])
            copy_submit = renderer_submit + 1
            camera = final_frame["camera_receipt"]
            camera_json = json.dumps(camera, separators=(",", ":"))
            capture = {
                "schema": "gsplat-playcanvas-webgpu-renderer-capture/v1",
                "producer": "playcanvas_webgpu_copy_texture_to_buffer",
                "status": "terminal",
                "renderer_frame_sequence": 103,
                "renderer_submit_version": renderer_submit,
                "copy_submit_version_before": renderer_submit,
                "copy_submit_version_after": copy_submit,
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
                "resolution": {
                    "requested_width": WIDTH,
                    "requested_height": HEIGHT,
                    "surface_width": WIDTH,
                    "surface_height": HEIGHT,
                    "internal_render_width": WIDTH,
                    "internal_render_height": HEIGHT,
                    "presented_width": WIDTH,
                    "presented_height": HEIGHT,
                    "dynamic_resolution": "disabled",
                    "upscaling": "disabled",
                    "internal_full_resolution": True,
                    "full_resolution": True,
                },
                "source": {
                    "dataset_id": TRUCK["id"],
                    "dataset_sha256": TRUCK["sha256"],
                    "source_splat_count": TRUCK["splat_count"],
                    "decoded_splat_count": TRUCK["splat_count"],
                    "resident_splat_count": TRUCK["splat_count"],
                    "source_sh_degree": 3,
                    "resident_sh_degree": 3,
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
                    "submit_version_before": copy_submit,
                    "submit_version_after": copy_submit,
                    "submit_version_stable": True,
                },
            }
            source_image = root / next(
                image["path"] for image in endpoint["images"]
                if image["trace_frame_index"] == trace
            )
            png = source_image.read_bytes()
            png_path = directory / "final-frame.png"
            rgba_path = directory / "final-frame.rgba8"
            png_path.write_bytes(png)
            rgba_path.write_bytes(rgba)
            png_sha = hashlib.sha256(png).hexdigest()
            materialization = {
                "schema": "gsplat-playcanvas-renderer-capture-materialization/v1",
                "source": "host_png_from_renderer_owned_webgpu_rgba8",
                "source_capture_schema": capture["schema"],
                "source_capture_producer": capture["producer"],
                "source_rgba8_sha256": rgba_sha,
                "rgba8_file": rgba_path.name,
                "rgba8_byte_length": len(rgba),
                "png_file": png_path.name,
                "png_byte_length": len(png),
                "png_sha256": png_sha,
                "width": WIDTH,
                "height": HEIGHT,
            }
            final_frame["renderer_capture_copy"] = {
                "submit_version_after": copy_submit
            }
            native_presentation["renderer_capture"] = capture
            native_presentation["queue_drain"] = {
                "phase": "post_capture_presentation",
                "frameLoopStopped": True,
                "submitVersionBefore": copy_submit,
                "submitVersionAfter": copy_submit,
                "submitVersionStable": True,
            }
            manifest_value["renderer_capture"] = capture
            manifest_value["renderer_capture_materialization"] = materialization
            manifest_value["q1_comparison"].pop(
                "renderer_rgba_unavailable_reason", None
            )
            write_json(manifest_path, manifest_value)
            manifest_sha = file_sha256(manifest_path)
            control["manifest_sha256"] = manifest_sha
            binding_digests[trace] = manifest_sha
            image = next(
                image for image in endpoint["images"]
                if image["trace_frame_index"] == trace
            )
            image["path"] = png_path.relative_to(root).as_posix()
            image["sha256"] = png_sha
            image["producer_artifact"]["manifest_sha256"] = manifest_sha
            image["host_admission_join"].update(
                {
                    "producer_manifest_sha256": manifest_sha,
                    "source_rgba8_sha256": rgba_sha,
                    "png_sha256": png_sha,
                }
            )
        throughput_path = root / endpoint["throughput"] / "manifest.json"
        throughput = json.loads(throughput_path.read_text(encoding="utf-8"))
        for binding in throughput["q1_comparison"]["control_bindings"]:
            binding["manifest_sha256"] = binding_digests[
                int(binding["trace_frame_index"])
            ]
        write_json(throughput_path, throughput)
    write_json(schedule_path, document)


def build_series(
    root: pathlib.Path, *, gs_terminal_ms: float = 800.0, score: float = 1.0,
    playcanvas_producer: bool = False, authority_commit: str = COMMIT,
) -> pathlib.Path:
    series_id = "q1-truck-test"
    references = install_reference_authority(
        root, repository_commit=authority_commit
    )
    schedule_block = {
        "seed": 20260728,
        "predeclared_at_utc": PREDECLARED,
        "reference_images": references,
        "pairs": [
            {"pair_id": f"pair-{index + 1:02d}", "run_order": order}
            for index, order in enumerate(ORDERS)
        ],
    }
    schedule_sha = canonical_sha256(schedule_block)
    protocol_value = protocol()
    protocol_sha = canonical_sha256(protocol_value)
    builds = {
        endpoint: build_artifact_receipts(root, endpoint)
        for endpoint in ("playcanvas", "gsplat_rs")
    }
    pairs = []
    for pair_index, order in enumerate(ORDERS, 1):
        pair_id = f"pair-{pair_index:02d}"
        pair: dict[str, object] = {"pair_id": pair_id, "run_order": order}
        for endpoint in ("playcanvas", "gsplat_rs"):
            position = 1 if (order == "playcanvas-first") == (endpoint == "playcanvas") else 2
            base = pathlib.Path("pairs") / pair_id / endpoint
            configuration = hashlib.sha256(f"config-{endpoint}".encode()).hexdigest()
            slot = (pair_index - 1) * 20 + (position - 1) * 8
            epoch = datetime(2026, 7, 28, tzinfo=timezone.utc)
            throughput_started = epoch + timedelta(seconds=slot + 7)
            throughput_ended = throughput_started + timedelta(seconds=2)
            controls: list[dict[str, object]] = []
            controls_by_trace: dict[int, tuple[pathlib.Path, str]] = {}
            for trace in (0, 1):
                control_relative = base / f"control-trace-{trace}"
                control_started = epoch + timedelta(seconds=slot + trace * 2 + 1)
                control_ended = control_started + timedelta(seconds=1)
                control_doc = manifest(
                    endpoint=endpoint,
                    role="control",
                    run_id=f"{pair_id}-{endpoint}-control-{trace}",
                    series_id=series_id,
                    schedule_sha=schedule_sha,
                    protocol_sha=protocol_sha,
                    pair_id=pair_id,
                    order=order,
                    position=position,
                    configuration=configuration,
                    terminal_ms=None,
                    started_at=control_started.isoformat(),
                    ended_at=control_ended.isoformat(),
                    build_artifacts=builds[endpoint],
                    capture_trace=trace,
                )
                control_dir = write_artifact(
                    root,
                    control_relative,
                    control_doc,
                    endpoint,
                    "control",
                    None,
                    capture_trace=trace,
                )
                control_sha = file_sha256(control_dir / "manifest.json")
                controls.append(
                    {
                        "trace_frame_index": trace,
                        "artifact": control_relative.as_posix(),
                        "manifest_sha256": control_sha,
                    }
                )
                controls_by_trace[trace] = (control_relative, control_sha)
            images: list[dict[str, object]] = []
            for trace in (0, 1):
                image_path = base / f"view-{trace}.png"
                image_sha = write_rgba_png(root / image_path)
                comparison_path = base / f"view-{trace}-comparison.json"
                write_json(root / comparison_path, {"schema": IMAGE_SCHEMA, "metric": "ssim-luma-srgb-window8", "tool": "tests/perf/compare-image-ssim.mjs", "tool_sha256": IMAGE_TOOL_SHA256, "trace_frame_index": trace, "reference_sha256": references[trace]["sha256"], "candidate_sha256": image_sha, "width": WIDTH, "height": HEIGHT, "minimum_ssim": 0.99, "score": score})
                control_relative, control_sha = controls_by_trace[trace]
                images.append(
                    {
                        "trace_frame_index": trace,
                        "path": str(image_path),
                        "sha256": image_sha,
                        "comparison": str(comparison_path),
                        "producer_artifact": {
                            "path": control_relative.as_posix(),
                            "manifest_sha256": control_sha,
                        },
                        "host_admission_join": host_admission_join(
                            image_sha, control_relative, control_sha
                        ),
                    }
                )
            throughput_ms = 1000.0 if endpoint == "playcanvas" else gs_terminal_ms
            throughput_doc = manifest(endpoint=endpoint, role="throughput", run_id=f"{pair_id}-{endpoint}-throughput", series_id=series_id, schedule_sha=schedule_sha, protocol_sha=protocol_sha, pair_id=pair_id, order=order, position=position, configuration=configuration, terminal_ms=throughput_ms, started_at=throughput_started.isoformat(), ended_at=throughput_ended.isoformat(), build_artifacts=builds[endpoint])
            throughput_doc["q1_comparison"]["control_bindings"] = [
                {
                    "trace_frame_index": control["trace_frame_index"],
                    "run_id": f"{pair_id}-{endpoint}-control-{control['trace_frame_index']}",
                    "manifest_sha256": control["manifest_sha256"],
                    "configuration_sha256": configuration,
                }
                for control in controls
            ]
            write_artifact(root, base / "throughput", throughput_doc, endpoint, "throughput", throughput_ms)
            pair[endpoint] = {
                "position": position,
                "controls": controls,
                "throughput": str(base / "throughput"),
                "images": images,
            }
        pairs.append(pair)
    schedule_path = root / "schedule.json"
    write_json(schedule_path, {"schema": SCHEMA, "series_id": series_id, "schedule": schedule_block, "protocol": protocol_value, "pairs": pairs})
    if playcanvas_producer:
        install_complete_playcanvas_producers(root, schedule_path)
    return schedule_path


class ScheduleAndAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.schedule = build_series(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def mutate_manifest(self, relative: str, callback) -> None:
        path = self.root / relative / "manifest.json"
        value = json.loads(path.read_text(encoding="utf-8"))
        callback(value)
        write_json(path, value)
        parts = pathlib.Path(relative).parts
        if len(parts) == 4 and parts[0] == "pairs" and parts[3].startswith("control-trace-"):
            pair_id, endpoint = parts[1], parts[2]
            trace = int(parts[3].removeprefix("control-trace-"))
            digest = file_sha256(path)
            document = json.loads(self.schedule.read_text(encoding="utf-8"))
            pair = next(item for item in document["pairs"] if item["pair_id"] == pair_id)
            control = next(
                item for item in pair[endpoint]["controls"]
                if item["trace_frame_index"] == trace
            )
            control["manifest_sha256"] = digest
            image = next(
                item for item in pair[endpoint]["images"]
                if item["trace_frame_index"] == trace
            )
            image["producer_artifact"]["manifest_sha256"] = digest
            image["host_admission_join"]["producer_manifest_sha256"] = digest
            write_json(self.schedule, document)
            throughput_path = self.root / "pairs" / pair_id / endpoint / "throughput" / "manifest.json"
            throughput = json.loads(throughput_path.read_text(encoding="utf-8"))
            binding = next(
                item for item in throughput["q1_comparison"]["control_bindings"]
                if item["trace_frame_index"] == trace
            )
            binding["manifest_sha256"] = digest
            write_json(throughput_path, throughput)

    def mutate_reference_authority(self, callback) -> None:
        receipt_path = self.root / "reference-authority/reference.json"
        value = json.loads(receipt_path.read_text(encoding="utf-8"))
        callback(value)
        write_json(receipt_path, value)
        digest = file_sha256(receipt_path)
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        for reference in document["schedule"]["reference_images"]:
            reference["authority_receipt_sha256"] = digest
        write_json(self.schedule, document)

    def test_public_reference_authority_seam_validates_the_complete_tree(self) -> None:
        authority = self.root / "reference-authority"
        admitted = reference_authority(authority)
        self.assertEqual(admitted["repository_commit"], COMMIT)
        self.assertEqual(admitted["receipt_path"], authority / "reference.json")
        self.assertEqual(
            admitted["tree"]["sha256"],
            canonical_sha256(admitted["tree"]["files"]),
        )
        self.assertEqual(
            {value["path"] for value in admitted["tree"]["files"]},
            {
                "producer/desktop-example",
                "reference-trace-0.png",
                "reference-trace-1.png",
                "reference.json",
            },
        )

        outside = self.root / "outside-support.log"
        outside.write_text("outside\n")
        (authority / "support.log").symlink_to(outside)
        with self.assertRaisesRegex(ValidationError, "symlink"):
            reference_authority(authority)

    def test_legacy_presentation_without_renderer_capture_is_deferred(self) -> None:
        result = evaluate(self.schedule)
        self.assertEqual(result["state"], "Deferred")
        self.assertFalse(result["evidence_admitted"])
        self.assertTrue(result["candidate_evidence_valid"])
        self.assertEqual(result["pair_count"], 5)
        self.assertIsNone(result["performance"])
        self.assertEqual(
            result["reasons"],
            ["playcanvas_renderer_same_present_rgba_receipt_unavailable"],
        )
        self.assertEqual(
            result["reference_authority"]["repository_commit"], COMMIT
        )
        self.assertEqual(
            result["reference_authority"]["receipt_path"],
            "reference-authority/reference.json",
        )

    def test_arbitrary_self_consistent_png_is_not_a_reference_authority(self) -> None:
        arbitrary = self.root / "arbitrary.png"
        digest = write_rgba_png(arbitrary, value=7)
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        reference = document["schedule"]["reference_images"][0]
        reference.update(
            {
                "path": "arbitrary.png",
                "sha256": digest,
                "decoded_rgba8_sha256": solid_rgba_sha256(7),
            }
        )
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "identity mismatch"):
            evaluate(self.schedule)

    def test_reference_views_must_share_one_authority_receipt(self) -> None:
        copied = self.root / "copied-authority/reference.json"
        copied.parent.mkdir()
        copied.write_bytes(
            (self.root / "reference-authority/reference.json").read_bytes()
        )
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["schedule"]["reference_images"][1].update(
            {
                "authority_receipt_path": "copied-authority/reference.json",
                "authority_receipt_sha256": file_sha256(copied),
            }
        )
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "one shared authority"):
            evaluate(self.schedule)

    def test_reference_authority_blocker_is_rejected(self) -> None:
        write_json(
            self.root / "reference-authority/blocker.json",
            {"status": "rejected"},
        )
        with self.assertRaisesRegex(ValidationError, "contains blocker.json"):
            evaluate(self.schedule)

    def test_reference_authority_receipt_symlink_is_rejected(self) -> None:
        alias = self.root / "reference-authority/receipt-alias.json"
        alias.symlink_to("reference.json")
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        for reference in document["schedule"]["reference_images"]:
            reference["authority_receipt_path"] = (
                "reference-authority/receipt-alias.json"
            )
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "symlink component"):
            evaluate(self.schedule)

    def test_reference_authority_directory_symlink_is_rejected(self) -> None:
        (self.root / "authority-alias").symlink_to(
            "reference-authority", target_is_directory=True
        )
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        for reference in document["schedule"]["reference_images"]:
            reference["authority_receipt_path"] = "authority-alias/reference.json"
            reference["path"] = (
                f"authority-alias/reference-trace-{reference['trace_frame_index']}.png"
            )
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "symlink component"):
            evaluate(self.schedule)

    def test_reference_authority_retained_binary_symlink_is_rejected(self) -> None:
        retained = self.root / "reference-authority/producer/desktop-example"
        real = retained.with_name("desktop-example-real")
        retained.rename(real)
        retained.symlink_to(real.name)
        with self.assertRaisesRegex(ValidationError, "symlink component"):
            evaluate(self.schedule)

    def test_reference_authority_png_symlink_is_rejected(self) -> None:
        image = self.root / "reference-authority/reference-trace-0.png"
        real = image.with_name("reference-trace-0-real.png")
        image.rename(real)
        image.symlink_to(real.name)
        with self.assertRaisesRegex(ValidationError, "symlink component"):
            evaluate(self.schedule)

    def test_schedule_reference_mixed_symlink_alias_is_rejected(self) -> None:
        (self.root / "image-alias").symlink_to(
            "reference-authority", target_is_directory=True
        )
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["schedule"]["reference_images"][0]["path"] = (
            "image-alias/reference-trace-0.png"
        )
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "symlink component"):
            evaluate(self.schedule)

    def test_reference_authority_retained_binary_drift_is_rejected(self) -> None:
        (self.root / "reference-authority/producer/desktop-example").write_bytes(
            b"drift"
        )
        with self.assertRaisesRegex(ValidationError, "retained identity mismatch"):
            evaluate(self.schedule)

    def test_reference_authority_exactness_mutation_is_rejected(self) -> None:
        self.mutate_reference_authority(
            lambda value: value["exactness"].__setitem__(
                "resident_count", TRUCK["splat_count"] - 1
            )
        )
        with self.assertRaisesRegex(ValidationError, "not complete Truck SH3"):
            evaluate(self.schedule)

    def test_reference_authority_execution_mutation_is_rejected(self) -> None:
        self.mutate_reference_authority(
            lambda value: value["execution"].__setitem__(
                "depth_key_precision", "candidate_stable20"
            )
        )
        with self.assertRaisesRegex(ValidationError, "not the frozen Direct-f32 oracle"):
            evaluate(self.schedule)

    def test_reference_authority_view_hash_mutation_is_rejected(self) -> None:
        self.mutate_reference_authority(
            lambda value: value["captures"][0].__setitem__(
                "pose_intrinsics_sha256", SHA_A
            )
        )
        with self.assertRaisesRegex(ValidationError, "view identity mismatch"):
            evaluate(self.schedule)

    def test_reference_authority_zero_visible_oracle_is_rejected(self) -> None:
        def zero_view(value: dict) -> None:
            capture = value["captures"][0]
            capture["visible_count"] = 0
            capture["drawn_count"] = 0
            capture["renderer_receipt"]["visible_count"] = "0"
            capture["renderer_receipt"]["drawn_count"] = "0"

        self.mutate_reference_authority(zero_view)
        with self.assertRaisesRegex(ValidationError, "0<drawn=visible<=source"):
            evaluate(self.schedule)

    def test_reference_authority_must_precede_schedule(self) -> None:
        self.mutate_reference_authority(
            lambda value: value.__setitem__(
                "generated_at_utc", "2026-07-28T00:00:01Z"
            )
        )
        with self.assertRaisesRegex(
            ValidationError, "generated after schedule predeclaration"
        ):
            evaluate(self.schedule)

    def test_reference_authority_commit_must_equal_endpoint_commit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            schedule = build_series(
                pathlib.Path(directory), authority_commit="d" * 40
            )
            with self.assertRaisesRegex(
                ValidationError, "commit does not match the Direct-f32 authority"
            ):
                evaluate(schedule)

    def test_partial_playcanvas_renderer_producer_is_rejected(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/playcanvas/control-trace-0",
            lambda value: value["presentation_capture"]["frames"][-1].__setitem__(
                "renderer_capture_copy", {"submit_version_after": 104}
            ),
        )
        with self.assertRaisesRegex(ValidationError, "partial PlayCanvas renderer producer"):
            evaluate(self.schedule)

    def test_complete_playcanvas_native_producer_enters_admitted_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            schedule = build_series(root, playcanvas_producer=True)
            playcanvas_environment = json.loads(
                (root / "pairs/pair-01/playcanvas/throughput/manifest.json").read_text()
            )["environment"]
            gsplat_environment = json.loads(
                (root / "pairs/pair-01/gsplat_rs/throughput/manifest.json").read_text()
            )["environment"]
            self.assertNotEqual(
                playcanvas_environment["device_effective_limits_sha256"],
                gsplat_environment["device_effective_limits_sha256"],
            )
            self.assertEqual(
                playcanvas_environment["canonical_adapter_supported_limits_sha256"],
                gsplat_environment["canonical_adapter_supported_limits_sha256"],
            )
            result = evaluate(schedule)
        self.assertEqual(result["state"], "Accepted")
        self.assertTrue(result["evidence_admitted"])
        self.assertTrue(result["quality_passed"])
        self.assertIsNotNone(result["performance"])
        self.assertEqual(
            result["reference_authority"]["repository_commit"], COMMIT
        )

    def test_device_effective_limit_mutation_is_rejected_against_actual_receipt(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/playcanvas/throughput",
            lambda value: value["environment"]["webgpu_device_environment_receipt"][
                "selected_device"
            ]["effective_limits"].__setitem__("maxBufferSize", 4242),
        )
        with self.assertRaisesRegex(
            ValidationError, "device_effective_limits_sha256 does not match"
        ):
            evaluate(self.schedule)

    def test_canonical_limit_must_come_from_actual_selected_adapter(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/playcanvas/throughput",
            lambda value: value["environment"]["webgpu_device_environment_receipt"][
                "canonical_adapter"
            ]["supported_limits"].__setitem__("maxBufferSize", 4242),
        )
        with self.assertRaisesRegex(
            ValidationError, "canonical limits are not selected-adapter data"
        ):
            evaluate(self.schedule)

    def test_playcanvas_camera_authority_fixture_is_current(self) -> None:
        completed = subprocess.run(
            ["node", str(PLAYCANVAS_CAMERA_AUTHORITY_GENERATOR), "--check"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_self_consistent_playcanvas_camera_mutations_are_rejected(self) -> None:
        mutations = {
            "position": lambda camera: camera["position"].__setitem__(
                0, camera["position"][0] + 0.25
            ),
            "fov": lambda camera: camera.__setitem__(
                "vertical_fov_radians", camera["vertical_fov_radians"] + 0.01
            ),
            "matrix": lambda camera: camera[
                "shader_view_projection_matrix_webgpu_column_major"
            ].__setitem__(7, camera["shader_view_projection_matrix_webgpu_column_major"][7] + 0.25),
        }
        for label, mutate_camera in mutations.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                schedule = build_series(root, playcanvas_producer=True)
                manifest_path = root / "pairs/pair-01/playcanvas/control-trace-0/manifest.json"
                manifest_value = json.loads(manifest_path.read_text(encoding="utf-8"))
                presentation_value = manifest_value["presentation_capture"]
                for frame_value in presentation_value["frames"]:
                    mutate_camera(frame_value["camera_receipt"])
                mutate_camera(presentation_value["terminal_camera_receipt"])
                mutate_camera(manifest_value["camera_receipt"])
                capture = manifest_value["renderer_capture"]
                mutate_camera(capture["camera_receipt"])
                camera_json = json.dumps(capture["camera_receipt"], separators=(",", ":"))
                capture["camera_receipt_json"] = camera_json
                capture["camera_receipt_sha256"] = hashlib.sha256(
                    camera_json.encode()
                ).hexdigest()
                write_json(manifest_path, manifest_value)

                manifest_sha = file_sha256(manifest_path)
                schedule_value = json.loads(schedule.read_text(encoding="utf-8"))
                endpoint = schedule_value["pairs"][0]["playcanvas"]
                endpoint["controls"][0]["manifest_sha256"] = manifest_sha
                endpoint["images"][0]["producer_artifact"]["manifest_sha256"] = manifest_sha
                endpoint["images"][0]["host_admission_join"][
                    "producer_manifest_sha256"
                ] = manifest_sha
                throughput_path = root / endpoint["throughput"] / "manifest.json"
                throughput = json.loads(throughput_path.read_text(encoding="utf-8"))
                throughput["q1_comparison"]["control_bindings"][0][
                    "manifest_sha256"
                ] = manifest_sha
                write_json(throughput_path, throughput)
                write_json(schedule, schedule_value)
                with self.assertRaisesRegex(
                    ValidationError, "authoritative PlayCanvas camera receipt"
                ):
                    evaluate(schedule)

    def test_null_pairing_from_historical_artifact_is_rejected(self) -> None:
        self.mutate_manifest("pairs/pair-01/playcanvas/control-trace-0", lambda value: value.__setitem__("pairing", {"pair_id": None, "run_order": None, "position": None}))
        with self.assertRaisesRegex(ValidationError, "pairing.series_id"):
            evaluate(self.schedule)

    def test_non_common_terminal_primitive_is_rejected(self) -> None:
        self.mutate_manifest("pairs/pair-01/gsplat_rs/throughput", lambda value: value["q1_comparison"]["terminal_window"].__setitem__("completion_primitive", "renderer_current_stats_map"))
        with self.assertRaisesRegex(ValidationError, "completion_primitive"):
            evaluate(self.schedule)

    def test_missing_build_hash_is_rejected(self) -> None:
        self.mutate_manifest("pairs/pair-01/playcanvas/control-trace-0", lambda value: value["build"].__setitem__("artifacts", {}))
        with self.assertRaisesRegex(ValidationError, "build.artifacts keys"):
            evaluate(self.schedule)

    def test_artifact_with_blocker_is_rejected(self) -> None:
        directory = self.root / "pairs/pair-01/playcanvas/throughput"
        write_json(directory / "blocker.json", {"status": "blocked"})
        with self.assertRaisesRegex(ValidationError, "contains blocker.json"):
            evaluate(self.schedule)

    def test_artifact_with_dangling_blocker_symlink_is_rejected(self) -> None:
        directory = self.root / "pairs/pair-01/playcanvas/throughput"
        (directory / "blocker.json").symlink_to("missing-blocker-target.json")
        with self.assertRaisesRegex(ValidationError, "contains blocker.json"):
            evaluate(self.schedule)

    def test_artifact_with_cleanup_blocker_is_rejected(self) -> None:
        directory = self.root / "pairs/pair-01/playcanvas/throughput"
        write_json(directory / "cleanup-blocker.json", {"status": "blocked"})
        with self.assertRaisesRegex(ValidationError, "contains cleanup-blocker.json"):
            evaluate(self.schedule)

    def test_artifact_with_dangling_cleanup_blocker_symlink_is_rejected(self) -> None:
        directory = self.root / "pairs/pair-01/playcanvas/throughput"
        (directory / "cleanup-blocker.json").symlink_to(
            "missing-cleanup-blocker-target.json"
        )
        with self.assertRaisesRegex(ValidationError, "contains cleanup-blocker.json"):
            evaluate(self.schedule)

    def test_schedule_root_with_blocker_is_rejected(self) -> None:
        write_json(self.root / "blocker.json", {"status": "blocked"})
        with self.assertRaisesRegex(ValidationError, "schedule root contains blocker.json"):
            evaluate(self.schedule)

    def test_schedule_root_with_dangling_blocker_symlink_is_rejected(self) -> None:
        (self.root / "blocker.json").symlink_to("missing-blocker-target.json")
        with self.assertRaisesRegex(ValidationError, "schedule root contains blocker.json"):
            evaluate(self.schedule)

    def test_schedule_root_with_cleanup_blocker_is_rejected(self) -> None:
        write_json(self.root / "cleanup-blocker.json", {"status": "blocked"})
        with self.assertRaisesRegex(
            ValidationError,
            "schedule root contains cleanup-blocker.json",
        ):
            evaluate(self.schedule)

    def test_schedule_root_with_dangling_cleanup_blocker_symlink_is_rejected(self) -> None:
        (self.root / "cleanup-blocker.json").symlink_to(
            "missing-cleanup-blocker-target.json"
        )
        with self.assertRaisesRegex(
            ValidationError,
            "schedule root contains cleanup-blocker.json",
        ):
            evaluate(self.schedule)

    def test_browser_process_argument_receipt_hash_is_recomputed(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/playcanvas/throughput",
            lambda value: value["environment"]["browser_launch_args_receipt"][
                "normalized_args"
            ].append("--unbound-mutation"),
        )
        with self.assertRaisesRegex(ValidationError, "browser_launch_args_receipt hash mismatch"):
            evaluate(self.schedule)

    def test_browser_process_argument_receipt_content_is_admitted(self) -> None:
        def add_headless(value: dict) -> None:
            environment = value["environment"]
            receipt = environment["browser_launch_args_receipt"]
            receipt["normalized_args"].append("--headless=new")
            digest = hashlib.sha256(
                json.dumps(receipt["normalized_args"], separators=(",", ":")).encode()
            ).hexdigest()
            receipt["normalized_sha256"] = digest
            environment["browser_launch_args_sha256"] = digest

        self.mutate_manifest("pairs/pair-01/playcanvas/throughput", add_headless)
        with self.assertRaisesRegex(
            ValidationError,
            "browser_launch_args_receipt content is inadmissible",
        ):
            evaluate(self.schedule)

    def test_missing_common_image_receipt_is_rejected(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["pairs"][0]["playcanvas"]["images"][0]["comparison"] = "missing.json"
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "comparison does not name a file"):
            evaluate(self.schedule)

    def test_image_cannot_reference_the_other_trace_control(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        endpoint = document["pairs"][0]["gsplat_rs"]
        other = endpoint["controls"][1]
        image = endpoint["images"][0]
        image["producer_artifact"] = {
            "path": other["artifact"],
            "manifest_sha256": other["manifest_sha256"],
        }
        image["host_admission_join"]["producer_artifact_path"] = other["artifact"]
        image["host_admission_join"]["producer_manifest_sha256"] = other[
            "manifest_sha256"
        ]
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "native control manifest"):
            evaluate(self.schedule)

    def test_untimed_controls_do_not_define_pair_order(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/playcanvas/control-trace-0",
            lambda value: value["identity"].update(
                {
                    "started_at_utc": "2026-07-28T00:00:40+00:00",
                    "ended_at_utc": "2026-07-28T00:00:41+00:00",
                }
            ),
        )
        self.assertEqual(evaluate(self.schedule)["state"], "Deferred")

    def test_image_receipt_without_comparator_hash_is_rejected(self) -> None:
        receipt = self.root / "pairs/pair-01/playcanvas/view-0-comparison.json"
        value = json.loads(receipt.read_text(encoding="utf-8"))
        del value["tool_sha256"]
        write_json(receipt, value)
        with self.assertRaisesRegex(ValidationError, "tool_sha256"):
            evaluate(self.schedule)

    def test_fake_comparator_hash_is_rejected_even_when_well_formed(self) -> None:
        receipt = self.root / "pairs/pair-01/playcanvas/view-0-comparison.json"
        value = json.loads(receipt.read_text(encoding="utf-8"))
        value["tool_sha256"] = SHA_A
        write_json(receipt, value)
        with self.assertRaisesRegex(ValidationError, "locked tool"):
            evaluate(self.schedule)

    def test_self_reported_score_is_rejected_when_decoded_pixels_disagree(self) -> None:
        receipt = self.root / "pairs/pair-01/playcanvas/view-0-comparison.json"
        value = json.loads(receipt.read_text(encoding="utf-8"))
        value["score"] = 0.98
        write_json(receipt, value)
        with self.assertRaisesRegex(ValidationError, "decoded PNG bytes"):
            evaluate(self.schedule)

    def test_twenty_four_byte_png_header_is_not_a_decodable_image(self) -> None:
        reference = self.root / "reference-authority/reference-trace-0.png"
        reference.write_bytes(
            b"\x89PNG\r\n\x1a\n"
            + (13).to_bytes(4, "big")
            + b"IHDR"
            + WIDTH.to_bytes(4, "big")
            + HEIGHT.to_bytes(4, "big")
        )
        receipt_path = self.root / "reference-authority/reference.json"
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        receipt["captures"][0]["image"]["bytes"] = reference.stat().st_size
        receipt["captures"][0]["image"]["sha256"] = file_sha256(reference)
        write_json(receipt_path, receipt)
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["schedule"]["reference_images"][0]["sha256"] = file_sha256(reference)
        for value in document["schedule"]["reference_images"]:
            value["authority_receipt_sha256"] = file_sha256(receipt_path)
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "image is not RGBA8"):
            evaluate(self.schedule)

    def test_copied_schedule_receipt_cannot_replace_renderer_terminal_evidence(self) -> None:
        frames = self.root / "pairs/pair-01/gsplat_rs/control-trace-0/frames.jsonl"
        records = [json.loads(line) for line in frames.read_text().splitlines()]
        del records[MEASURED - 2]["capture_depth_precision"]
        frames.write_text(
            "".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8"
        )
        with self.assertRaisesRegex(ValidationError, "RGBA terminal placement"):
            evaluate(self.schedule)

    def test_gsplat_renderer_receipt_uses_real_depth_precision_fields(self) -> None:
        frames = self.root / "pairs/pair-01/gsplat_rs/control-trace-0/frames.jsonl"
        records = [json.loads(line) for line in frames.read_text().splitlines()]
        del records[MEASURED - 2]["capture_depth_precision"]["plan_set_generation"]
        frames.write_text(
            "".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8"
        )
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/control-trace-0",
            lambda value: value["q1_comparison"]["presentation_identity"][
                "terminal_identity"
            ].__setitem__("frame_sha256", canonical_sha256(records[MEASURED - 2])),
        )
        with self.assertRaisesRegex(ValidationError, "RGBA receipt fields are not frozen"):
            evaluate(self.schedule)

    def test_playcanvas_renderer_producer_cannot_be_fabricated(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/playcanvas/control-trace-0",
            lambda value: value.__setitem__(
                "renderer_capture", gsplat_renderer_rgba(79, 79)
            ),
        )
        with self.assertRaisesRegex(ValidationError, "partial PlayCanvas renderer producer"):
            evaluate(self.schedule)

    def test_host_png_source_must_match_renderer_owned_rgba(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["pairs"][0]["gsplat_rs"]["images"][0]["host_admission_join"][
            "source_rgba8_sha256"
        ] = SHA_A
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "declared RGBA8 source"):
            evaluate(self.schedule)

    def test_reencoded_png_is_accepted_when_pixels_and_receipts_match(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        image = document["pairs"][0]["gsplat_rs"]["images"][0]
        image_path = self.root / image["path"]
        replacement_sha = write_reencoded_rgba_png(image_path)
        self.assertNotEqual(replacement_sha, image["sha256"])
        self.assertEqual(solid_rgba_sha256(), image["host_admission_join"]["source_rgba8_sha256"])
        image["sha256"] = replacement_sha
        image["host_admission_join"]["png_sha256"] = replacement_sha
        comparison_path = self.root / image["comparison"]
        comparison = json.loads(comparison_path.read_text(encoding="utf-8"))
        comparison["candidate_sha256"] = replacement_sha
        comparison["score"] = 1.0
        write_json(comparison_path, comparison)
        write_json(self.schedule, document)
        self.assertEqual(evaluate(self.schedule)["state"], "Deferred")

    def test_host_materialization_may_repeat_identical_rgba_content(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        first = document["pairs"][0]["playcanvas"]["images"][0]
        second = document["pairs"][1]["playcanvas"]["images"][0]
        self.assertEqual(first["sha256"], second["sha256"])
        self.assertNotEqual(first["path"], second["path"])
        self.assertEqual(evaluate(self.schedule)["state"], "Deferred")

    def test_pair_two_cannot_reuse_pair_one_comparison_path(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["pairs"][1]["playcanvas"]["images"][0]["comparison"] = document[
            "pairs"
        ][0]["playcanvas"]["images"][0]["comparison"]
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "reuses endpoint evidence path"):
            evaluate(self.schedule)

    def test_schedule_mutation_changes_predeclared_schedule_hash(self) -> None:
        document = json.loads(self.schedule.read_text(encoding="utf-8"))
        document["schedule"]["seed"] += 1
        write_json(self.schedule, document)
        with self.assertRaisesRegex(ValidationError, "pairing.schedule_sha256"):
            evaluate(self.schedule)

    def test_build_artifact_file_content_drift_is_rejected(self) -> None:
        path = self.root / "build/gsplat_rs/runtime_wasm"
        path.write_bytes(b"drifted wasm\n")
        with self.assertRaisesRegex(ValidationError, "content SHA-256 mismatch"):
            evaluate(self.schedule)

    def test_severe_post_thermal_state_is_rejected(self) -> None:
        self.mutate_manifest(
            "pairs/pair-02/playcanvas/throughput",
            lambda value: value["environment"]["thermal"].__setitem__("post", "severe"),
        )
        with self.assertRaisesRegex(ValidationError, "too hot for admission"):
            evaluate(self.schedule)

    def test_endpoint_effective_device_limits_are_not_misreported_as_common_hardware(self) -> None:
        for endpoint, value in (("playcanvas", 111), ("gsplat_rs", 222)):
            for role in ("control-trace-0", "control-trace-1", "throughput"):
                self.mutate_manifest(
                    f"pairs/pair-01/{endpoint}/{role}",
                    lambda manifest, observed=value: manifest["environment"].__setitem__(
                        "endpoint_device_receipt",
                        {"effective_device_limits": {"maxBufferSize": observed}},
                    ),
                )
        self.assertEqual(evaluate(self.schedule)["state"], "Deferred")

    def test_common_selected_adapter_supported_limits_hash_drift_is_rejected(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/throughput",
            lambda value: value["environment"].__setitem__(
                "canonical_adapter_supported_limits_sha256", SHA_B
            ),
        )
        with self.assertRaisesRegex(ValidationError, "does not match"):
            evaluate(self.schedule)

    def test_pair_order_label_without_timestamp_proof_is_rejected(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/throughput",
            lambda value: value["identity"].__setitem__(
                "started_at_utc", "2026-07-28T00:00:08+00:00"
            ),
        )
        with self.assertRaisesRegex(ValidationError, "timestamps do not prove"):
            evaluate(self.schedule)

    def test_camera_receipt_must_bind_frozen_trace_frame(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/control-trace-0",
            lambda value: value["q1_comparison"]["presentation_identity"]["camera"].__setitem__(
                "pose_intrinsics_sha256", SHA_B
            ),
        )
        with self.assertRaisesRegex(ValidationError, "frozen trace frame"):
            evaluate(self.schedule)

    def test_presentation_receipt_must_bind_artifact_terminal_frame(self) -> None:
        self.mutate_manifest(
            "pairs/pair-01/gsplat_rs/control-trace-0",
            lambda value: value["q1_comparison"]["presentation_identity"][
                "terminal_identity"
            ].__setitem__("frame_sha256", SHA_B),
        )
        with self.assertRaisesRegex(ValidationError, "terminal frame hash"):
            evaluate(self.schedule)

    def test_playcanvas_vcd_must_remain_unavailable(self) -> None:
        path = self.root / "pairs/pair-01/playcanvas/control-trace-0/frames.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()]
        records[0]["visible"] = TRUCK["splat_count"]
        records[0]["drawn"] = TRUCK["splat_count"]
        path.write_text("".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8")
        with self.assertRaisesRegex(ValidationError, "PlayCanvas V/C/D"):
            evaluate(self.schedule)

    def test_gsplat_throughput_cannot_copy_control_counts(self) -> None:
        path = self.root / "pairs/pair-01/gsplat_rs/throughput/frames.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()]
        records[0]["visible"] = 10
        records[0]["drawn"] = 10
        path.write_text("".join(f"{json.dumps(value)}\n" for value in records), encoding="utf-8")
        with self.assertRaises(ValidationError):
            evaluate(self.schedule)


class FiniteVerdictTests(unittest.TestCase):
    def test_cli_publishes_deferred_candidate_without_performance(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            schedule = build_series(root)
            output = root / "result.json"
            completed = subprocess.run(
                [sys.executable, str(CLI), str(schedule), "--output", str(output)],
                check=False,
                capture_output=True,
                text=True,
            )
            result = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(result["state"], "Deferred")
        self.assertFalse(result["evidence_admitted"])
        self.assertTrue(result["candidate_evidence_valid"])
        self.assertIsNone(result["performance"])

    def test_cli_admission_failure_is_exit_two_and_no_performance_claim(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            schedule = build_series(root)
            value = json.loads(schedule.read_text(encoding="utf-8"))
            value["pairs"][0]["pair_id"] = "not-predeclared"
            write_json(schedule, value)
            output = root / "result.json"
            completed = subprocess.run(
                [sys.executable, str(CLI), str(schedule), "--output", str(output)],
                check=False,
                capture_output=True,
                text=True,
            )
            result = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(completed.returncode, 2)
        self.assertFalse(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertFalse(result["retry_authorized"])

    def test_slower_candidate_cannot_publish_comparison_without_both_producers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = evaluate(build_series(pathlib.Path(directory), gs_terminal_ms=1200.0))
        self.assertEqual(result["state"], "Deferred")
        self.assertFalse(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertEqual(
            result["reasons"],
            ["playcanvas_renderer_same_present_rgba_receipt_unavailable"],
        )
        self.assertFalse(result["retry_authorized"])

    def test_diagnostic_quality_miss_remains_deferred_without_both_producers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with mock.patch(
                "q1_pair_admission.artifacts.recompute_image_score", return_value=0.98
            ):
                result = evaluate(build_series(pathlib.Path(directory), score=0.98))
        self.assertEqual(result["state"], "Deferred")
        self.assertFalse(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertIsNone(result["quality_passed"])
        self.assertEqual(
            result["reasons"],
            ["playcanvas_renderer_same_present_rgba_receipt_unavailable"],
        )
        encoded = json.dumps(result, sort_keys=True)
        for forbidden in (
            "playcanvas_terminal_mean_ms",
            "gsplat_rs_terminal_mean_ms",
            "gsplat_rs_minus_playcanvas_ms",
            "gsplat_rs_over_playcanvas_ratio",
        ):
            self.assertNotIn(forbidden, encoded)

    def test_admission_rejection_has_no_performance_or_retry(self) -> None:
        result = admission_rejection("series", "missing common terminal")
        self.assertFalse(result["evidence_admitted"])
        self.assertIsNone(result["performance"])
        self.assertFalse(result["retry_authorized"])


if __name__ == "__main__":
    unittest.main()
