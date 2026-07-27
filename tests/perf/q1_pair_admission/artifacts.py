"""Benchmark, environment, terminal, and image receipt admission."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import math
import pathlib
import struct
import sys
import zlib
from datetime import datetime
from typing import Any

from .common import array, canonical_sha256, fail, file_sha256, inside, integer, load_json, load_jsonl, number, obj, sha256, string, utc
from .contract import (
    COMMON_ENVIRONMENT_FIELDS,
    HEIGHT,
    IMAGE_SCHEMA,
    MEASURED,
    PLAYCANVAS,
    TRACE,
    TRACE_FRAME_POSE_INTRINSICS_SHA256,
    TRUCK,
    WARMUP,
    WIDTH,
    validate_terminal,
)


def _load_benchmark_validator() -> Any:
    path = pathlib.Path(__file__).parents[1] / "validate-benchmark-artifacts.py"
    spec = importlib.util.spec_from_file_location("q1_canonical_benchmark_validator", path)
    if spec is None or spec.loader is None:
        fail("cannot load canonical benchmark validator")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


BENCHMARK = _load_benchmark_validator()


def _load_image_validator() -> Any:
    path = pathlib.Path(__file__).parents[1] / "validate-balanced-image-gate.py"
    spec = importlib.util.spec_from_file_location("q1_canonical_image_validator", path)
    if spec is None or spec.loader is None:
        fail("cannot load canonical image validator")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


IMAGE = _load_image_validator()
IMAGE_TOOL = pathlib.Path(__file__).parents[1] / "compare-image-ssim.mjs"
IMAGE_TOOL_SHA256 = file_sha256(IMAGE_TOOL)
_IMAGE_SCORE_CACHE: dict[tuple[str, str], float] = {}

BUILD_ARTIFACT_KEYS = {
    "playcanvas": frozenset({"runtime_js", "package_lock"}),
    "gsplat_rs": frozenset({"runtime_js", "runtime_wasm", "package_manifest"}),
}
FORBIDDEN_THERMAL_STATES = frozenset(
    {"severe", "serious", "critical", "emergency", "shutdown"}
)
ADMISSIBLE_THERMAL_STATES = frozenset({"nominal", "fair", "moderate"})
HOST_ADMISSION_JOIN_SCHEMA = "gsplat-q1-host-admission-join/v1"
HOST_ADMISSION_JOIN_FIELDS = frozenset(
    {
        "schema",
        "source",
        "pixel_format",
        "width",
        "height",
        "producer_artifact_path",
        "producer_manifest_sha256",
        "source_rgba8_sha256",
        "png_sha256",
    }
)
GSPLAT_RGBA_RECEIPT_FIELDS = frozenset(
    {
        "scene_generation",
        "camera_revision",
        "viewport_generation",
        "contract_generation",
        "plan_set_generation",
        "plan_id",
        "order_generation",
        "presentation_sequence",
        "width",
        "height",
        "rgba8_sha256",
        "profile",
    }
)
PLAYCANVAS_RGBA_UNAVAILABLE = "playcanvas_renderer_same_present_rgba_receipt_unavailable"
PLAYCANVAS_CAPTURE_SCHEMA = "gsplat-playcanvas-webgpu-renderer-capture/v1"
PLAYCANVAS_CAPTURE_PRODUCER = "playcanvas_webgpu_copy_texture_to_buffer"
PLAYCANVAS_CAMERA_SCHEMA = "gsplat-playcanvas-runtime-camera-receipt/v1"
PLAYCANVAS_PRESENTATION_SCHEMA = "gsplat-playcanvas-presentation-capture/v1"
PLAYCANVAS_MATERIALIZATION_SCHEMA = (
    "gsplat-playcanvas-renderer-capture-materialization/v1"
)
PLAYCANVAS_CAPTURE_FIELDS = frozenset(
    {
        "schema",
        "producer",
        "status",
        "renderer_frame_sequence",
        "renderer_submit_version",
        "copy_submit_version_before",
        "copy_submit_version_after",
        "texture_format",
        "render_view_format",
        "canvas_color_space",
        "canvas_alpha_mode",
        "pixel_format",
        "row_origin",
        "width",
        "height",
        "row_bytes",
        "byte_length",
        "rgba8_sha256",
        "camera_receipt_sha256",
        "camera_receipt_json",
        "camera_receipt",
        "resolution",
        "source",
        "copy_map_complete",
        "queue_terminal_complete",
        "terminal_queue_drain",
    }
)
PLAYCANVAS_MATERIALIZATION_FIELDS = frozenset(
    {
        "schema",
        "source",
        "source_capture_schema",
        "source_capture_producer",
        "source_rgba8_sha256",
        "rgba8_file",
        "rgba8_byte_length",
        "png_file",
        "png_byte_length",
        "png_sha256",
        "width",
        "height",
    }
)


def _decode_image(path: pathlib.Path, context: str) -> Any:
    data = path.read_bytes()
    chunks = list(IMAGE.png_chunks(data))
    chunk_types = [kind for kind, _ in chunks]
    if (
        not chunk_types
        or chunk_types[0] != b"IHDR"
        or chunk_types[-1] != b"IEND"
        or chunk_types.count(b"IHDR") != 1
        or chunk_types.count(b"IEND") != 1
    ):
        fail(f"{context} has an invalid PNG chunk sequence")
    unsupported_critical = [
        kind
        for kind in chunk_types
        if kind[:1].isupper() and kind not in {b"IHDR", b"IDAT", b"IEND"}
    ]
    if unsupported_critical:
        fail(f"{context} uses unsupported critical PNG chunks")
    if any(kind in {b"cHRM", b"gAMA", b"iCCP", b"sRGB"} for kind in chunk_types):
        fail(f"{context} uses color-management chunks outside the raw sRGB byte contract")
    ihdr = next((payload for kind, payload in chunks if kind == b"IHDR"), None)
    idat = b"".join(payload for kind, payload in chunks if kind == b"IDAT")
    if ihdr is None or len(ihdr) != 13 or not idat:
        fail(f"{context} is missing required PNG chunks")
    width, height, bit_depth, color_type, compression, filter_method, interlace = (
        struct.unpack(">IIBBBBB", ihdr)
    )
    if (width, height) != (WIDTH, HEIGHT):
        fail(f"{context} PNG dimensions do not match the frozen 1920x1080 receipt")
    if (bit_depth, color_type, compression, filter_method, interlace) != (8, 6, 0, 0, 0):
        fail(f"{context} must be non-interlaced RGBA8 PNG")
    row_bytes = width * 4
    expected_bytes = height * (row_bytes + 1)
    try:
        decompressor = zlib.decompressobj()
        filtered = bytearray()
        pending = idat
        while pending:
            remaining = expected_bytes + 1 - len(filtered)
            if remaining <= 0:
                fail(f"{context} decompressed byte length exceeds its RGBA receipt")
            before = len(pending)
            filtered.extend(decompressor.decompress(pending, remaining))
            pending = decompressor.unconsumed_tail
            if pending and len(pending) == before:
                fail(f"{context} compressed pixel stream made no progress")
    except zlib.error as error:
        fail(f"{context} has invalid compressed pixel data: {error}")
    if (
        len(filtered) != expected_bytes
        or not decompressor.eof
        or decompressor.unused_data
        or decompressor.unconsumed_tail
    ):
        fail(f"{context} decompressed byte length is invalid")
    offsets = range(0, expected_bytes, row_bytes + 1)
    if all(filtered[offset] == 0 for offset in offsets):
        rgba = b"".join(
            filtered[offset + 1 : offset + 1 + row_bytes] for offset in offsets
        )
        return IMAGE.DecodedImage(width=width, height=height, rgba=rgba)
    return IMAGE.decode_rgba8_png(data, context, (WIDTH, HEIGHT))


def recompute_image_score(reference: pathlib.Path, candidate: pathlib.Path, context: str) -> float:
    """Decode real RGBA8 PNGs and run the repository's locked SSIM algorithm."""
    key = (file_sha256(reference), file_sha256(candidate))
    if key in _IMAGE_SCORE_CACHE:
        return _IMAGE_SCORE_CACHE[key]
    try:
        exact = _decode_image(reference, f"{context}.reference")
        observed = _decode_image(candidate, f"{context}.candidate")
        if exact.rgba == observed.rgba:
            score = 1.0
        else:
            score = float(
                IMAGE.compute_frame_metrics(exact, observed)["ssim_luma_srgb_window8"]
            )
        _IMAGE_SCORE_CACHE[key] = score
        return score
    except (OSError, IMAGE.ValidationError) as error:
        fail(f"{context} cannot recompute {IMAGE_TOOL.relative_to(pathlib.Path(__file__).parents[2])}: {error}")


def _exactness(manifest: dict[str, Any], context: str) -> None:
    receipt = obj(manifest, "exactness", context)
    for field in ("source", "decoded", "encoded", "resident", "addressable"):
        if receipt.get(f"{field}_splat_count") != TRUCK["splat_count"]:
            fail(f"{context}.exactness.{field}_splat_count mismatch")
    if receipt.get("source_sh_degree") != 3 or receipt.get("resident_sh_degree") != 3:
        fail(f"{context}.exactness does not preserve SH3")
    for key, value in {
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "partial_scene_published": False,
        "full_quality": True,
    }.items():
        if receipt.get(key) != value:
            fail(f"{context}.exactness.{key} must equal {value!r}")


def _resolution(manifest: dict[str, Any], context: str) -> None:
    receipt = obj(manifest, "resolution", context)
    for stage in ("requested", "surface", "internal_render", "presented"):
        if receipt.get(f"{stage}_width") != WIDTH or receipt.get(f"{stage}_height") != HEIGHT:
            fail(f"{context}.resolution.{stage} mismatch")
    if receipt.get("dynamic_resolution") != "disabled" or receipt.get("upscaling") != "disabled" or receipt.get("full_resolution") is not True:
        fail(f"{context}.resolution does not prove native 1920x1080 presentation")


def _build(
    root: pathlib.Path, manifest: dict[str, Any], endpoint: str, context: str
) -> tuple[str, dict[str, Any]]:
    build = obj(manifest, "build", context)
    commit = string(build, "repository_commit", f"{context}.build")
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        fail(f"{context}.build.repository_commit must be a full Git SHA")
    if build.get("dirty") is not False:
        fail(f"{context}.build.dirty must be false")
    artifacts = obj(build, "artifacts", f"{context}.build")
    required = BUILD_ARTIFACT_KEYS[endpoint]
    if set(artifacts) != required:
        fail(
            f"{context}.build.artifacts keys must equal {sorted(required)!r}"
        )
    normalized: dict[str, str] = {}
    artifact_paths: set[pathlib.Path] = set()
    for name in sorted(required):
        receipt = obj(artifacts, name, f"{context}.build.artifacts")
        path = inside(root, receipt.get("path"), f"{context}.build.artifacts.{name}.path")
        if path in artifact_paths:
            fail(f"{context}.build.artifacts reuses one file for multiple required keys")
        artifact_paths.add(path)
        digest = sha256(
            receipt.get("sha256"), f"{context}.build.artifacts.{name}.sha256"
        )
        if file_sha256(path) != digest:
            fail(f"{context}.build.artifacts.{name} content SHA-256 mismatch")
        normalized[name] = digest
    if endpoint == "playcanvas":
        expected = {
            "package_version": PLAYCANVAS["version"],
            "upstream_revision": PLAYCANVAS["revision"],
            "runtime_revision": PLAYCANVAS["runtime_revision"],
            "package_integrity": PLAYCANVAS["integrity"],
        }
        for key, value in expected.items():
            if build.get(key) != value:
                fail(f"{context}.build.{key} does not match pinned PlayCanvas")
    return commit, {
        "profile": string(build, "profile", f"{context}.build"),
        "package_version": string(build, "package_version", f"{context}.build"),
        "artifacts": normalized,
    }


def _environment(manifest: dict[str, Any], context: str) -> dict[str, Any]:
    source = obj(manifest, "environment", context)
    identity: dict[str, Any] = {}
    for field in COMMON_ENVIRONMENT_FIELDS:
        identity[field] = string(source, field, f"{context}.environment")
        if field.endswith("sha256"):
            sha256(identity[field], f"{context}.environment.{field}")
    thermal = obj(source, "thermal", f"{context}.environment")
    thermal_identity: dict[str, Any] = {}
    for field in ("source", "pre", "post"):
        thermal_identity[field] = string(
            thermal, field, f"{context}.environment.thermal"
        )
    for field in ("pre", "post"):
        state = thermal_identity[field].strip().lower()
        if state in FORBIDDEN_THERMAL_STATES:
            fail(f"{context}.environment.thermal.{field} is too hot for admission")
        if state not in ADMISSIBLE_THERMAL_STATES:
            fail(f"{context}.environment.thermal.{field} is not a recognized state")
    if thermal.get("admitted") is not True:
        fail(f"{context}.environment.thermal.admitted must be true")
    thermal_identity["admitted"] = True
    identity["thermal_source"] = thermal_identity["source"]
    return {"identity": identity, "thermal": thermal_identity}


def _renderer(manifest: dict[str, Any], endpoint: str, context: str) -> None:
    renderer = obj(manifest, "renderer", context)
    if renderer.get("backend") != "webgpu":
        fail(f"{context}.renderer.backend must be actual WebGPU")
    expected = (
        {"implementation": "playcanvas-d5fe888", "path": "GSplatHybridRenderer", "sort_policy": "raster_gpu_sort", "uses_gpu_sort": True}
        if endpoint == "playcanvas"
        else {"implementation": "gsplat-rs", "path": "wasm_packed_atlas", "order_backend_requested": "gpu", "gpu_order_producer_actual": "preproject", "projected_policy_requested": "compact", "raster_execution_plan": "projected_quads_exact", "sort_interval": 1}
    )
    for key, value in expected.items():
        if renderer.get(key) != value:
            fail(f"{context}.renderer.{key} must equal {value!r}")


def _common(manifest: dict[str, Any], context: str) -> None:
    for key, value in TRUCK.items():
        if obj(manifest, "dataset", context).get(key) != value:
            fail(f"{context}.dataset.{key} mismatch")
    trace = obj(manifest, "trace", context)
    for key, value in TRACE.items():
        if trace.get(key) != value:
            fail(f"{context}.trace.{key} mismatch")
    if trace.get("camera_mode") != "trace_sequence":
        fail(f"{context}.trace.camera_mode mismatch")
    display = obj(manifest, "display", context)
    if (display.get("width"), display.get("height"), display.get("dpr")) != (WIDTH, HEIGHT, 1):
        fail(f"{context}.display mismatch")
    _exactness(manifest, context)
    _resolution(manifest, context)


def _display(manifest: dict[str, Any], context: str) -> dict[str, Any]:
    display = obj(manifest, "display", context)
    return {
        "refresh_hz": number(display, "refresh_hz", f"{context}.display"),
        "frame_budget_ms": number(display, "frame_budget_ms", f"{context}.display"),
        "refresh_hz_source": string(display, "refresh_hz_source", f"{context}.display"),
        "frame_budget_source": string(display, "frame_budget_source", f"{context}.display"),
    }


def _frames(
    frames: list[dict[str, Any]], endpoint: str, role: str, run_id: str,
    capture_trace: int | None, context: str,
) -> None:
    if len(frames) != MEASURED:
        fail(f"{context} must contain exactly 80 frames")
    for index, frame in enumerate(frames):
        if frame.get("run_id") != run_id or frame.get("frame_index") != index or frame.get("trace_frame_index") != index % 2:
            fail(f"{context}[{index}] run/frame/trace identity mismatch")
        if frame.get("sort_refreshed") is not True:
            fail(f"{context}[{index}].sort_refreshed must be true")
        has_capture = isinstance(frame.get("capture_depth_precision"), dict)
        expected_capture = endpoint == "gsplat_rs" and role == "control" and (
            index == MEASURED - 2 + capture_trace
        )
        if has_capture != expected_capture:
            fail(f"{context}[{index}] renderer RGBA terminal placement mismatch")
        if role == "throughput" and "capture_depth_precision" in frame:
            fail(f"{context}[{index}] timed throughput must not carry capture evidence")
        counts = (frame.get("visible"), frame.get("contributor"), frame.get("drawn"))
        if role == "throughput":
            if counts != (None, None, None):
                fail(f"{context}[{index}] copied V/C/D into timed throughput")
        elif endpoint == "playcanvas":
            if frame.get("active_splats") != TRUCK["splat_count"] or counts != (None, None, None):
                fail(f"{context}[{index}] must preserve S and leave PlayCanvas V/C/D unavailable")
        else:
            if not all(isinstance(value, int) and not isinstance(value, bool) for value in counts):
                fail(f"{context}[{index}] lacks exact gsplat-rs V/C/D")
            visible, contributor, drawn = counts
            if not 0 <= contributor <= visible <= TRUCK["splat_count"] or drawn != contributor:
                fail(f"{context}[{index}] violates C<=V<=S and D=C")
        if endpoint == "gsplat_rs":
            expected = {"order_backend": "gpu", "gpu_order_producer": "preproject", "projected_execution": "compact", "raster_execution_plan": "projected_quads_exact", "gpu_sort_fallback": False}
            for key, value in expected.items():
                if frame.get(key) != value:
                    fail(f"{context}[{index}].{key} must equal {value!r}")


def _gsplat_presentation_receipt(
    q1: dict[str, Any], frames: list[dict[str, Any]], run_id: str,
    trace: int, context: str,
) -> dict[str, Any]:
    receipt = obj(q1, "presentation_identity", context)
    if receipt.get("trace_frame_index") != trace:
        fail(f"{context}.presentation_identity trace mismatch")
    camera = obj(receipt, "camera", f"{context}.presentation_identity")
    for field, expected in {
        "trace_id": TRACE["id"],
        "trace_content_sha256": TRACE["sha256"],
        "trace_frame_index": trace,
        "pose_intrinsics_sha256": TRACE_FRAME_POSE_INTRINSICS_SHA256[trace],
    }.items():
        if camera.get(field) != expected:
            fail(f"{context}.presentation_identity.camera.{field} does not bind the frozen trace frame")
    camera_revision = integer(camera, "camera_revision", f"{context}.presentation_identity.camera")
    terminal = obj(receipt, "terminal_identity", f"{context}.presentation_identity")
    terminal_index = integer(terminal, "frame_index", f"{context}.presentation_identity.terminal_identity")
    expected_index = MEASURED - 2 + trace
    if terminal.get("run_id") != run_id or terminal_index != expected_index:
        fail(f"{context}.presentation_identity does not bind the artifact terminal")
    terminal_frame = frames[terminal_index]
    if terminal.get("frame_sha256") != canonical_sha256(terminal_frame):
        fail(f"{context}.presentation_identity terminal frame hash mismatch")
    presentation_sequence = integer(
        terminal, "presentation_sequence", f"{context}.presentation_identity.terminal_identity"
    )
    if (
        camera_revision <= 0
        or terminal_frame.get("trace_frame_index") != trace
        or terminal_frame.get("camera_revision") != camera_revision
        or terminal_frame.get("presentation_sequence") != presentation_sequence
        or presentation_sequence <= 0
    ):
        fail(f"{context}.presentation_identity is not same-present")
    capture = obj(terminal_frame, "capture_depth_precision", f"{context}.terminal_frame")
    if set(capture) != GSPLAT_RGBA_RECEIPT_FIELDS:
        fail(f"{context} gsplat-rs renderer RGBA receipt fields are not frozen")
    for field, expected in {
        "camera_revision": camera_revision,
        "presentation_sequence": presentation_sequence,
        "plan_id": "GpuPreproject",
        "width": WIDTH,
        "height": HEIGHT,
        "profile": "ExactFull32",
    }.items():
        if capture.get(field) != expected:
            fail(f"{context}.renderer_rgba_receipt.{field} mismatch")
    for field in ("scene_generation", "contract_generation", "plan_set_generation", "order_generation"):
        if integer(capture, field, f"{context}.renderer_rgba_receipt") <= 0:
            fail(f"{context}.renderer_rgba_receipt.{field} must be positive")
    integer(capture, "viewport_generation", f"{context}.renderer_rgba_receipt")
    sha256(capture.get("rgba8_sha256"), f"{context}.renderer_rgba_receipt.rgba8_sha256")
    if any(receipt.get(field) is not True for field in (
        "successful_present", "queue_terminal_complete", "captured_after_terminal"
    )):
        fail(f"{context}.presentation_identity lacks a successful terminal presentation")
    dimensions = obj(receipt, "dimensions", f"{context}.presentation_identity")
    for stage in ("requested", "surface", "internal_render", "presented"):
        if (dimensions.get(f"{stage}_width"), dimensions.get(f"{stage}_height")) != (WIDTH, HEIGHT):
            fail(f"{context}.presentation_identity {stage} dimensions mismatch")
    return {
        "trace_frame_index": trace,
        "renderer_rgba_status": "ready",
        "renderer_rgba_receipt": capture,
        "native_materialization": None,
        "native_identity": receipt,
    }


def _playcanvas_presentation_receipt(
    manifest: dict[str, Any], q1: dict[str, Any], trace: int, context: str,
) -> dict[str, Any]:
    presentation = manifest.get("presentation_capture")
    if not isinstance(presentation, dict):
        if (
            q1.get("renderer_rgba_unavailable_reason") != PLAYCANVAS_RGBA_UNAVAILABLE
            or manifest.get("renderer_capture") is not None
            or manifest.get("renderer_capture_materialization") is not None
        ):
            fail(f"{context} must truthfully declare unavailable PlayCanvas renderer RGBA")
        return {
            "trace_frame_index": trace,
            "renderer_rgba_status": "unavailable",
            "renderer_rgba_reason": PLAYCANVAS_RGBA_UNAVAILABLE,
            "renderer_rgba_receipt": None,
            "native_materialization": None,
            "native_identity": None,
        }
    if q1.get("renderer_rgba_unavailable_reason") is not None:
        fail(f"{context} cannot mark an available PlayCanvas producer unavailable")
    if (
        presentation.get("schema") != PLAYCANVAS_PRESENTATION_SCHEMA
        or presentation.get("ready_for_external_capture") is not True
        or presentation.get("excluded_from_performance") is not True
        or presentation.get("capture_trace_frame_index") != trace
        or presentation.get("minimum_stable_frame_count") != 3
    ):
        fail(f"{context}.presentation_capture is not a frozen untimed producer terminal")
    stable_count = integer(presentation, "stable_frame_count", f"{context}.presentation_capture")
    presentation_frames = array(presentation, "frames", f"{context}.presentation_capture")
    if stable_count < 3 or len(presentation_frames) != stable_count:
        fail(f"{context}.presentation_capture lacks stable renderer frames")
    prior_submit = integer(
        presentation, "measurement_terminal_submit_version", f"{context}.presentation_capture"
    )
    for index, frame in enumerate(presentation_frames):
        if not isinstance(frame, dict) or frame.get("trace_frame_index") != trace:
            fail(f"{context}.presentation_capture.frames[{index}] trace mismatch")
        camera = obj(frame, "camera_receipt", f"{context}.presentation_capture.frames[{index}]")
        if camera.get("schema") != PLAYCANVAS_CAMERA_SCHEMA or camera.get("trace_frame_index") != trace:
            fail(f"{context}.presentation_capture.frames[{index}] camera mismatch")
        before = integer(frame, "submit_version_before", f"{context}.presentation_capture.frames[{index}]")
        after = integer(frame, "submit_version_after", f"{context}.presentation_capture.frames[{index}]")
        calls = integer(frame, "queue_submit_call_count", f"{context}.presentation_capture.frames[{index}]")
        if before != prior_submit or after <= before or calls != after - before:
            fail(f"{context}.presentation_capture.frames[{index}] submit chain mismatch")
        prior_submit = after
    final_frame = presentation_frames[-1]
    capture = obj(presentation, "renderer_capture", f"{context}.presentation_capture")
    if manifest.get("renderer_capture") != capture or set(capture) != PLAYCANVAS_CAPTURE_FIELDS:
        fail(f"{context} PlayCanvas renderer capture fields/owner location are not frozen")
    for field, expected in {
        "schema": PLAYCANVAS_CAPTURE_SCHEMA,
        "producer": PLAYCANVAS_CAPTURE_PRODUCER,
        "status": "terminal",
        "renderer_submit_version": prior_submit,
        "copy_submit_version_before": prior_submit,
        "copy_submit_version_after": prior_submit + 1,
        "pixel_format": "rgba8unorm",
        "row_origin": "top_left",
        "width": WIDTH,
        "height": HEIGHT,
        "row_bytes": WIDTH * 4,
        "byte_length": WIDTH * HEIGHT * 4,
        "copy_map_complete": True,
        "queue_terminal_complete": True,
    }.items():
        if capture.get(field) != expected:
            fail(f"{context}.renderer_capture.{field} mismatch")
    integer(capture, "renderer_frame_sequence", f"{context}.renderer_capture")
    if capture.get("texture_format") not in {"bgra8unorm", "rgba8unorm"}:
        fail(f"{context}.renderer_capture.texture_format is unsupported")
    for field in ("render_view_format", "canvas_color_space", "canvas_alpha_mode"):
        string(capture, field, f"{context}.renderer_capture")
    rgba_sha = sha256(capture.get("rgba8_sha256"), f"{context}.renderer_capture.rgba8_sha256")
    camera_json = string(capture, "camera_receipt_json", f"{context}.renderer_capture")
    camera_sha = sha256(capture.get("camera_receipt_sha256"), f"{context}.renderer_capture.camera_receipt_sha256")
    if hashlib.sha256(camera_json.encode("utf-8")).hexdigest() != camera_sha:
        fail(f"{context}.renderer_capture camera JSON hash mismatch")
    try:
        parsed_camera = json.loads(camera_json)
    except json.JSONDecodeError as error:
        fail(f"{context}.renderer_capture.camera_receipt_json is invalid: {error}")
    camera = obj(capture, "camera_receipt", f"{context}.renderer_capture")
    if (
        parsed_camera != camera
        or camera.get("schema") != PLAYCANVAS_CAMERA_SCHEMA
        or camera.get("trace_frame_index") != trace
        or camera != final_frame.get("camera_receipt")
    ):
        fail(f"{context}.renderer_capture camera does not bind the final presentation frame")
    copy = obj(final_frame, "renderer_capture_copy", f"{context}.presentation_capture.frames[-1]")
    if copy.get("submit_version_after") != capture["copy_submit_version_after"]:
        fail(f"{context}.renderer_capture copy submission is not same-frame")
    terminal_drain = obj(capture, "terminal_queue_drain", f"{context}.renderer_capture")
    if terminal_drain != {
        "phase": "post_capture_presentation",
        "submit_version_before": capture["copy_submit_version_after"],
        "submit_version_after": capture["copy_submit_version_after"],
        "submit_version_stable": True,
    }:
        fail(f"{context}.renderer_capture terminal queue drain mismatch")
    outer_drain = obj(presentation, "queue_drain", f"{context}.presentation_capture")
    expected_outer_drain = {
        "phase": "post_capture_presentation",
        "frameLoopStopped": True,
        "submitVersionBefore": capture["copy_submit_version_after"],
        "submitVersionAfter": capture["copy_submit_version_after"],
        "submitVersionStable": True,
    }
    if any(outer_drain.get(field) != expected for field, expected in expected_outer_drain.items()):
        fail(f"{context}.presentation_capture queue drain mismatch")
    if presentation.get("terminal_camera_receipt") != manifest.get("camera_receipt"):
        fail(f"{context}.presentation_capture terminal camera owner mismatch")
    source = obj(capture, "source", f"{context}.renderer_capture")
    for field, expected in {
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
    }.items():
        if source.get(field) != expected:
            fail(f"{context}.renderer_capture.source.{field} mismatch")
    resolution = obj(capture, "resolution", f"{context}.renderer_capture")
    for stage in ("requested", "surface", "internal_render"):
        if (resolution.get(f"{stage}_width"), resolution.get(f"{stage}_height")) != (WIDTH, HEIGHT):
            fail(f"{context}.renderer_capture.resolution.{stage} mismatch")
    if (
        resolution.get("dynamic_resolution") != "disabled"
        or resolution.get("upscaling") != "disabled"
        or resolution.get("internal_full_resolution") is not True
    ):
        fail(f"{context}.renderer_capture resolution policy mismatch")
    materialization = obj(manifest, "renderer_capture_materialization", context)
    if set(materialization) != PLAYCANVAS_MATERIALIZATION_FIELDS:
        fail(f"{context}.renderer_capture_materialization fields are not frozen")
    return {
        "trace_frame_index": trace,
        "renderer_rgba_status": "ready",
        "renderer_rgba_receipt": capture,
        "native_materialization": materialization,
        "native_identity": presentation,
        "rgba8_sha256": rgba_sha,
    }


def artifact(
    root: pathlib.Path, relative: Any, *, endpoint: str, role: str, series_id: str,
    schedule_sha: str, protocol_sha: str, pair_id: str, order: str, position: int,
    predeclared: datetime, seen_paths: set[pathlib.Path], seen_runs: set[str],
    expected_trace: int | None = None,
) -> dict[str, Any]:
    directory = inside(root, relative, f"{pair_id}.{endpoint}.{role}", directory=True)
    if directory in seen_paths:
        fail(f"artifact directory is reused: {directory}")
    seen_paths.add(directory)
    try:
        BENCHMARK.validate(directory)
    except BENCHMARK.ValidationError as error:
        fail(f"{pair_id}.{endpoint}.{role} fails canonical benchmark validation: {error}")
    manifest_path = directory / "manifest.json"
    manifest = load_json(manifest_path, f"{pair_id}.{endpoint}.{role}.manifest")
    summary = load_json(directory / "summary.json", f"{pair_id}.{endpoint}.{role}.summary")
    frames = load_jsonl(directory / "frames.jsonl", f"{pair_id}.{endpoint}.{role}.frames")
    context = f"{pair_id}.{endpoint}.{role}.manifest"
    run_id = string(manifest, "run_id", context)
    if run_id in seen_runs:
        fail(f"run_id is reused: {run_id}")
    seen_runs.add(run_id)
    if summary.get("sample_count") != MEASURED or summary.get("warmup_count") != WARMUP:
        fail(f"{context} must use 20 warmup and 80 measured frames")
    identity = obj(manifest, "identity", context)
    started = utc(identity.get("started_at_utc"), f"{context}.identity.started_at_utc")
    ended = utc(identity.get("ended_at_utc"), f"{context}.identity.ended_at_utc")
    if started <= predeclared:
        fail(f"{context} started before schedule declaration")
    if ended <= started:
        fail(f"{context} ended before it started")
    _common(manifest, context)
    _renderer(manifest, endpoint, context)
    commit, build_artifacts = _build(root, manifest, endpoint, context)
    environment = _environment(manifest, context)
    pairing = obj(manifest, "pairing", context)
    expected_pairing = {"series_id": series_id, "schedule_sha256": schedule_sha, "pair_id": pair_id, "run_order": order, "position": position, "fresh_output": True, "automatic_retry": False}
    for key, value in expected_pairing.items():
        if pairing.get(key) != value:
            fail(f"{context}.pairing.{key} must equal {value!r}")
    q1 = obj(manifest, "q1_comparison", context)
    if q1.get("artifact_role") != role or q1.get("protocol_sha256") != protocol_sha:
        fail(f"{context}.q1_comparison role/protocol mismatch")
    expected_performance = role == "throughput"
    if q1.get("performance_evidence") is not expected_performance:
        fail(f"{context}.q1_comparison.performance_evidence must equal {expected_performance!r}")
    capture_trace = None
    if role == "control":
        capture_trace = integer(
            q1, "capture_trace_frame_index", f"{context}.q1_comparison"
        )
        if capture_trace not in {0, 1} or capture_trace != expected_trace:
            fail(f"{context}.q1_comparison capture trace mismatch")
    elif expected_trace is not None:
        fail(f"{context} throughput cannot claim a capture trace")
    expected_count_scope = {
        ("playcanvas", "control"): "full_membership_v_c_d_unavailable",
        ("playcanvas", "throughput"): "full_membership_v_c_d_unavailable",
        ("gsplat_rs", "control"): "exact_v_c_d_control_only",
        ("gsplat_rs", "throughput"): "control_bound_v_c_d_unavailable",
    }[(endpoint, role)]
    if q1.get("count_scope") != expected_count_scope:
        fail(f"{context}.q1_comparison.count_scope must equal {expected_count_scope!r}")
    unavailable = set(manifest.get("unavailable_fields", []))
    if (role == "throughput" or endpoint == "playcanvas") and not {"frames[*].visible", "frames[*].contributor", "frames[*].drawn"}.issubset(unavailable):
        fail(f"{context} does not declare unavailable V/C/D")
    configuration = sha256(q1.get("configuration_sha256"), f"{context}.q1_comparison.configuration_sha256")
    _frames(
        frames, endpoint, role, run_id, capture_trace,
        f"{pair_id}.{endpoint}.{role}.frames",
    )
    presentation = None
    if role == "control":
        presentation = (
            _gsplat_presentation_receipt(
                q1, frames, run_id, capture_trace, f"{context}.q1_comparison"
            )
            if endpoint == "gsplat_rs"
            else _playcanvas_presentation_receipt(
                manifest, q1, capture_trace, context
            )
        )
    return {
        "directory": directory,
        "relative_path": directory.relative_to(root).as_posix(),
        "manifest": manifest,
        "manifest_sha256": file_sha256(manifest_path),
        "run_id": run_id,
        "commit": commit,
        "build_artifacts": build_artifacts,
        "environment": environment,
        "display": _display(manifest, context),
        "configuration": configuration,
        "started": started,
        "ended": ended,
        "presentation": presentation,
        "terminal_ms": validate_terminal(q1, summary, f"{context}.q1_comparison") if role == "throughput" else None,
    }


def bind_controls(
    throughput: dict[str, Any], controls: dict[int, dict[str, Any]], context: str
) -> None:
    bindings = array(
        throughput["manifest"]["q1_comparison"], "control_bindings", context
    )
    expected = [
        {
            "trace_frame_index": trace,
            "run_id": control["run_id"],
            "manifest_sha256": control["manifest_sha256"],
            "configuration_sha256": control["configuration"],
        }
        for trace, control in sorted(controls.items())
    ]
    if bindings != expected or any(
        throughput["configuration"] != control["configuration"]
        for control in controls.values()
    ):
        fail(f"{context} does not bind both exact same-configuration control artifacts")


def reference_images(root: pathlib.Path, document: dict[str, Any]) -> dict[int, dict[str, Any]]:
    values = array(obj(document, "schedule", "schedule"), "reference_images", "schedule.schedule")
    if len(values) != 2:
        fail("schedule.reference_images must cover both views")
    result: dict[int, dict[str, Any]] = {}
    for index, value in enumerate(values):
        if not isinstance(value, dict):
            fail(f"schedule.reference_images[{index}] must be an object")
        trace = integer(value, "trace_frame_index", f"schedule.reference_images[{index}]")
        path = inside(root, value.get("path"), f"schedule.reference_images[{index}].path")
        digest = sha256(value.get("sha256"), f"schedule.reference_images[{index}].sha256")
        try:
            _decode_image(path, f"schedule.reference_images[{index}]")
        except (OSError, IMAGE.ValidationError) as error:
            fail(f"schedule.reference_images[{index}] is not a decodable RGBA8 PNG: {error}")
        if trace not in {0, 1} or trace in result or file_sha256(path) != digest:
            fail(f"schedule.reference_images[{index}] identity mismatch")
        result[trace] = {**value, "absolute_path": path}
    return result


def endpoint_images(
    root: pathlib.Path,
    values: Any,
    *,
    endpoint: str,
    pair_id: str,
    controls: dict[int, dict[str, Any]],
    references: dict[int, dict[str, Any]],
    minimum: float,
    seen_paths: set[pathlib.Path],
) -> list[dict[str, Any]]:
    if not isinstance(values, list) or len(values) != 2:
        fail(f"{pair_id}.{endpoint}.images must cover both views")
    result: list[dict[str, Any]] = []
    seen: set[int] = set()
    for index, value in enumerate(values):
        context = f"{pair_id}.{endpoint}.images[{index}]"
        if not isinstance(value, dict):
            fail(f"{context} must be an object")
        trace = integer(value, "trace_frame_index", context)
        path = inside(root, value.get("path"), f"{context}.path")
        digest = sha256(value.get("sha256"), f"{context}.sha256")
        comparison_path = inside(root, value.get("comparison"), f"{context}.comparison")
        for evidence_path in (path, comparison_path):
            if evidence_path in seen_paths:
                fail(f"{context} reuses endpoint evidence path {evidence_path}")
            seen_paths.add(evidence_path)
        if trace not in {0, 1} or trace in seen or file_sha256(path) != digest:
            fail(f"{context} image identity mismatch")
        seen.add(trace)
        control = controls.get(trace)
        if control is None:
            fail(f"{context} lacks its trace-specific producer control")
        producer = obj(value, "producer_artifact", context)
        if producer != {
            "path": control["relative_path"],
            "manifest_sha256": control["manifest_sha256"],
        }:
            fail(f"{context}.producer_artifact does not bind the native control manifest")
        presentation = control["presentation"]
        host_join = obj(value, "host_admission_join", context)
        if set(host_join) != HOST_ADMISSION_JOIN_FIELDS:
            fail(f"{context}.host_admission_join fields are not frozen")
        expected_join = {
            "schema": HOST_ADMISSION_JOIN_SCHEMA,
            "source": "decoded_png_rgba8_to_renderer_receipt",
            "pixel_format": "rgba8unorm-srgb",
            "width": WIDTH,
            "height": HEIGHT,
            "producer_artifact_path": control["relative_path"],
            "producer_manifest_sha256": control["manifest_sha256"],
            "png_sha256": digest,
        }
        for field, expected in expected_join.items():
            if host_join.get(field) != expected:
                fail(f"{context}.host_admission_join.{field} mismatch")
        source_rgba8 = sha256(
            host_join.get("source_rgba8_sha256"),
            f"{context}.host_admission_join.source_rgba8_sha256",
        )
        try:
            decoded = _decode_image(path, f"{context}.host_materialization")
        except (OSError, IMAGE.ValidationError) as error:
            fail(f"{context} host PNG materialization is invalid: {error}")
        if hashlib.sha256(decoded.rgba).hexdigest() != source_rgba8:
            fail(f"{context} host PNG does not derive from its declared RGBA8 source")
        renderer_rgba_ready = presentation.get("renderer_rgba_status") == "ready"
        if renderer_rgba_ready:
            capture = obj(
                presentation,
                "renderer_rgba_receipt",
                f"{context}.producer_artifact",
            )
            if source_rgba8 != capture.get("rgba8_sha256"):
                fail(f"{context} host PNG source does not match renderer-owned RGBA8")
        if endpoint == "playcanvas" and renderer_rgba_ready:
            materialization = obj(
                presentation, "native_materialization", f"{context}.producer_artifact"
            )
            expected_native = {
                "schema": PLAYCANVAS_MATERIALIZATION_SCHEMA,
                "source": "host_png_from_renderer_owned_webgpu_rgba8",
                "source_capture_schema": PLAYCANVAS_CAPTURE_SCHEMA,
                "source_capture_producer": PLAYCANVAS_CAPTURE_PRODUCER,
                "source_rgba8_sha256": source_rgba8,
                "rgba8_byte_length": WIDTH * HEIGHT * 4,
                "png_byte_length": path.stat().st_size,
                "png_sha256": digest,
                "width": WIDTH,
                "height": HEIGHT,
            }
            for field, expected in expected_native.items():
                if materialization.get(field) != expected:
                    fail(f"{context}.native_materialization.{field} mismatch")
            native_png = inside(
                control["directory"], materialization.get("png_file"),
                f"{context}.native_materialization.png_file",
            )
            native_rgba = inside(
                control["directory"], materialization.get("rgba8_file"),
                f"{context}.native_materialization.rgba8_file",
            )
            if native_png != path or file_sha256(native_rgba) != source_rgba8:
                fail(f"{context} does not bind PlayCanvas native materialized files")
            if native_rgba.stat().st_size != materialization["rgba8_byte_length"]:
                fail(f"{context} PlayCanvas RGBA8 byte length mismatch")
        receipt = load_json(comparison_path, f"{context}.comparison")
        expected = {
            "schema": IMAGE_SCHEMA,
            "metric": "ssim-luma-srgb-window8",
            "tool": "tests/perf/compare-image-ssim.mjs",
            "trace_frame_index": trace,
            "reference_sha256": references[trace]["sha256"],
            "candidate_sha256": digest,
            "width": WIDTH,
            "height": HEIGHT,
            "minimum_ssim": minimum,
        }
        if any(receipt.get(key) != expected_value for key, expected_value in expected.items()):
            fail(f"{context}.comparison identity mismatch")
        if receipt.get("tool_sha256") != IMAGE_TOOL_SHA256:
            fail(f"{context}.comparison.tool_sha256 does not match the locked tool")
        score = receipt.get("score")
        if not isinstance(score, (int, float)) or isinstance(score, bool) or not 0 <= score <= 1:
            fail(f"{context}.comparison.score must be in [0,1]")
        recomputed = recompute_image_score(
            references[trace]["absolute_path"], path, f"{context}.comparison"
        )
        if not math.isclose(float(score), recomputed, rel_tol=0.0, abs_tol=1.0e-9):
            fail(f"{context}.comparison.score does not match decoded PNG bytes")
        result.append(
            {
                "trace_frame_index": trace,
                "score": recomputed,
                "renderer_rgba_ready": renderer_rgba_ready,
            }
        )
    return sorted(result, key=lambda item: item["trace_frame_index"])
