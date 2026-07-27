"""Benchmark, environment, terminal, and image receipt admission."""

from __future__ import annotations

import importlib.util
import pathlib
from datetime import datetime
from typing import Any

from .common import array, fail, file_sha256, inside, integer, load_json, load_jsonl, number, obj, png_dimensions, sha256, string, utc
from .contract import (
    COMMON_ENVIRONMENT_FIELDS,
    HEIGHT,
    IMAGE_SCHEMA,
    MEASURED,
    PLAYCANVAS,
    TRACE,
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


def _build(manifest: dict[str, Any], endpoint: str, context: str) -> tuple[str, dict[str, str]]:
    build = obj(manifest, "build", context)
    commit = string(build, "repository_commit", f"{context}.build")
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        fail(f"{context}.build.repository_commit must be a full Git SHA")
    if build.get("dirty") is not False:
        fail(f"{context}.build.dirty must be false")
    artifacts = obj(build, "artifacts", f"{context}.build")
    if not artifacts:
        fail(f"{context}.build.artifacts must not be empty")
    normalized = {string({"name": name}, "name", f"{context}.build.artifacts"): sha256(digest, f"{context}.build.artifacts.{name}") for name, digest in artifacts.items()}
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
    return commit, normalized


def _environment(manifest: dict[str, Any], context: str) -> dict[str, Any]:
    source = obj(manifest, "environment", context)
    identity: dict[str, Any] = {}
    for field in COMMON_ENVIRONMENT_FIELDS:
        identity[field] = string(source, field, f"{context}.environment")
        if field.endswith("sha256"):
            sha256(identity[field], f"{context}.environment.{field}")
    thermal = obj(source, "thermal", f"{context}.environment")
    for field in ("source", "pre", "post"):
        string(thermal, field, f"{context}.environment.thermal")
    if thermal.get("admitted") is not True:
        fail(f"{context}.environment.thermal.admitted must be true")
    identity["thermal_source"] = thermal["source"]
    return identity


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


def _frames(frames: list[dict[str, Any]], endpoint: str, role: str, run_id: str, context: str) -> None:
    if len(frames) != MEASURED:
        fail(f"{context} must contain exactly 80 frames")
    for index, frame in enumerate(frames):
        if frame.get("run_id") != run_id or frame.get("frame_index") != index or frame.get("trace_frame_index") != index % 2:
            fail(f"{context}[{index}] run/frame/trace identity mismatch")
        if frame.get("sort_refreshed") is not True:
            fail(f"{context}[{index}].sort_refreshed must be true")
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


def presentation_receipts(q1: dict[str, Any], context: str) -> dict[int, dict[str, Any]]:
    receipts = array(q1, "presentation_receipts", context)
    if len(receipts) != 2:
        fail(f"{context}.presentation_receipts must cover both views")
    indexed: dict[int, dict[str, Any]] = {}
    for index, receipt in enumerate(receipts):
        if not isinstance(receipt, dict):
            fail(f"{context}.presentation_receipts[{index}] must be an object")
        trace = integer(receipt, "trace_frame_index", f"{context}.presentation_receipts[{index}]")
        if trace not in {0, 1} or trace in indexed:
            fail(f"{context}.presentation_receipts must contain views 0 and 1 once")
        for field in ("camera_receipt_sha256", "presentation_receipt_sha256"):
            sha256(receipt.get(field), f"{context}.presentation_receipts[{index}].{field}")
        if any(receipt.get(field) is not True for field in ("successful_present", "queue_terminal_complete", "captured_after_terminal")):
            fail(f"{context}.presentation_receipts[{index}] lacks successful terminal presentation")
        dimensions = obj(receipt, "dimensions", f"{context}.presentation_receipts[{index}]")
        for stage in ("requested", "surface", "internal_render", "presented"):
            if (dimensions.get(f"{stage}_width"), dimensions.get(f"{stage}_height")) != (WIDTH, HEIGHT):
                fail(f"{context}.presentation_receipts[{index}] {stage} dimensions mismatch")
        indexed[trace] = receipt
    return indexed


def artifact(
    root: pathlib.Path, relative: Any, *, endpoint: str, role: str, series_id: str,
    schedule_sha: str, protocol_sha: str, pair_id: str, order: str, position: int,
    predeclared: datetime, seen_paths: set[pathlib.Path], seen_runs: set[str],
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
    commit, build_artifacts = _build(manifest, endpoint, context)
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
    _frames(frames, endpoint, role, run_id, f"{pair_id}.{endpoint}.{role}.frames")
    return {
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
        "presentations": presentation_receipts(q1, f"{context}.q1_comparison") if role == "control" else None,
        "terminal_ms": validate_terminal(q1, summary, f"{context}.q1_comparison") if role == "throughput" else None,
    }


def bind_control(throughput: dict[str, Any], control: dict[str, Any], context: str) -> None:
    binding = obj(throughput["manifest"]["q1_comparison"], "control_binding", context)
    expected = {"run_id": control["run_id"], "manifest_sha256": control["manifest_sha256"], "configuration_sha256": control["configuration"]}
    if any(binding.get(key) != value for key, value in expected.items()) or throughput["configuration"] != control["configuration"]:
        fail(f"{context} does not bind the exact same-configuration control artifact")


def reference_images(root: pathlib.Path, document: dict[str, Any]) -> dict[int, dict[str, Any]]:
    values = array(document, "reference_images", "schedule")
    if len(values) != 2:
        fail("schedule.reference_images must cover both views")
    result: dict[int, dict[str, Any]] = {}
    for index, value in enumerate(values):
        if not isinstance(value, dict):
            fail(f"schedule.reference_images[{index}] must be an object")
        trace = integer(value, "trace_frame_index", f"schedule.reference_images[{index}]")
        path = inside(root, value.get("path"), f"schedule.reference_images[{index}].path")
        digest = sha256(value.get("sha256"), f"schedule.reference_images[{index}].sha256")
        if trace not in {0, 1} or trace in result or file_sha256(path) != digest or png_dimensions(path) != (WIDTH, HEIGHT):
            fail(f"schedule.reference_images[{index}] identity mismatch")
        result[trace] = value
    return result


def endpoint_images(root: pathlib.Path, values: Any, *, endpoint: str, pair_id: str, control: dict[str, Any], references: dict[int, dict[str, Any]], minimum: float) -> list[dict[str, Any]]:
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
        if trace not in {0, 1} or trace in seen or file_sha256(path) != digest or png_dimensions(path) != (WIDTH, HEIGHT):
            fail(f"{context} image identity mismatch")
        seen.add(trace)
        presentation = control["presentations"][trace]
        if any(value.get(field) != presentation.get(field) for field in ("camera_receipt_sha256", "presentation_receipt_sha256")):
            fail(f"{context} is not bound to the control presentation")
        receipt = load_json(inside(root, value.get("comparison"), f"{context}.comparison"), f"{context}.comparison")
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
        sha256(receipt.get("tool_sha256"), f"{context}.comparison.tool_sha256")
        score = receipt.get("score")
        if not isinstance(score, (int, float)) or isinstance(score, bool) or not 0 <= score <= 1:
            fail(f"{context}.comparison.score must be in [0,1]")
        result.append({"trace_frame_index": trace, "score": float(score)})
    return sorted(result, key=lambda item: item["trace_frame_index"])
