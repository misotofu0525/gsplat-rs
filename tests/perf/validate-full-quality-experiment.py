#!/usr/bin/env python3
"""Validate a cross-platform, full-quality Gaussian-splat experiment suite.

The suite wraps existing ``gsplat-benchmark/v1`` run artifacts with an exact
scene-residency receipt and a declarative coverage matrix.  Correctness and
coverage fail closed; frame-rate numbers are recorded but deliberately have no
global pass/fail threshold.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import pathlib
import re
import struct
import sys
from dataclasses import dataclass
from typing import Any


SCHEMA = "gsplat-full-quality-experiment/v1"
BENCHMARK_SCHEMA = "gsplat-benchmark/v1"
TRACE_SCHEMA = "gsplat-camera-trace/v1"
REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
BENCHMARK_VALIDATOR = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
TRACE_VALIDATOR = REPO_ROOT / "tests/perf/trace/validate_trace_v1.py"
ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
GIT_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
SORT_POLICIES = {"cpu", "gpu", "adaptive"}
DATASET_ROLES = {"smoke", "scaling_tier", "full_scene"}
ENDPOINT_AVAILABILITY = {"available", "unavailable", "probe_required"}
CAMERA_MODES = {"fixed_frame", "trace_sequence"}
TRACE_EVIDENCE_CLASSES = {"formal_full_quality", "diagnostic"}
FORMAL_EVIDENCE_CLASS = "formal_full_quality"
FORMAL_MIN_WIDTH = 1920
FORMAL_MIN_HEIGHT = 1080
QUALITY_CONTRACT = {
    "blend_mode": "sorted_alpha",
    "source_membership": "all",
    "sampling": "disabled",
    "lod": "disabled",
    "sh_degree": "source",
    "resolution_scale": 1.0,
    "capacity_failure": "reject_before_publish",
}
EXACT_COUNT_FIELDS = (
    "source_splat_count",
    "decoded_splat_count",
    "encoded_splat_count",
    "resident_splat_count",
    "addressable_splat_count",
)
RESOLUTION_STAGES = ("requested", "surface", "internal_render", "presented")
COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1"


class ValidationError(ValueError):
    pass


def fail(message: str) -> None:
    raise ValidationError(message)


def reject_constant(value: str) -> None:
    fail(f"non-finite JSON number is forbidden: {value}")


def load_json(path: pathlib.Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle, parse_constant=reject_constant)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{path} must contain a JSON object")
    return value


def require_object(parent: dict[str, Any], key: str, context: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        fail(f"{context}.{key} must be an object")
    return value


def require_array(parent: dict[str, Any], key: str, context: str) -> list[Any]:
    value = parent.get(key)
    if not isinstance(value, list):
        fail(f"{context}.{key} must be an array")
    return value


def require_string(parent: dict[str, Any], key: str, context: str) -> str:
    value = parent.get(key)
    if not isinstance(value, str) or not value:
        fail(f"{context}.{key} must be a non-empty string")
    return value


def require_id(parent: dict[str, Any], key: str, context: str) -> str:
    value = require_string(parent, key, context)
    if ID_RE.fullmatch(value) is None:
        fail(f"{context}.{key} must match {ID_RE.pattern}")
    return value


def require_bool(parent: dict[str, Any], key: str, context: str) -> bool:
    value = parent.get(key)
    if not isinstance(value, bool):
        fail(f"{context}.{key} must be boolean")
    return value


def require_int(
    parent: dict[str, Any], key: str, context: str, *, positive: bool = False
) -> int:
    value = parent.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        fail(f"{context}.{key} must be a non-negative integer")
    if positive and value == 0:
        fail(f"{context}.{key} must be positive")
    return value


def require_sha256(parent: dict[str, Any], key: str, context: str) -> str:
    value = require_string(parent, key, context)
    if SHA256_RE.fullmatch(value) is None:
        fail(f"{context}.{key} must be a lowercase SHA-256")
    return value


def unique_index(values: list[Any], kind: str) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for index, value in enumerate(values):
        context = f"{kind}[{index}]"
        if not isinstance(value, dict):
            fail(f"{context} must be an object")
        item_id = require_id(value, "id", context)
        if item_id in result:
            fail(f"duplicate {kind} id: {item_id}")
        result[item_id] = value
    return result


def resolve_relative(root: pathlib.Path, value: str, context: str) -> pathlib.Path:
    relative = pathlib.PurePosixPath(value)
    if relative.is_absolute() or ".." in relative.parts:
        fail(f"{context} must be a relative path without '..'")
    return root.joinpath(*relative.parts)


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        fail(f"cannot hash {path}: {error}")
    return digest.hexdigest()


def load_module(path: pathlib.Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        fail(f"cannot load validator module {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def verify_trace_file(trace: dict[str, Any], context: str) -> None:
    path = resolve_relative(
        REPO_ROOT, require_string(trace, "local_path", context), f"{context}.local_path"
    )
    trace_dir = str(TRACE_VALIDATOR.parent)
    inserted = trace_dir not in sys.path
    if inserted:
        sys.path.insert(0, trace_dir)
    try:
        validator = load_module(TRACE_VALIDATOR, "gsplat_trace_validator")
        value = validator.load(path)
        validator.validate(value)
    except (OSError, ValueError) as error:
        fail(f"{context} failed camera-trace validation: {error}")
    finally:
        if inserted:
            sys.path.remove(trace_dir)
    if value.get("trace_id") != trace["id"]:
        fail(f"{context} trace_id does not match the declared id")
    if value.get("content_sha256") != trace["sha256"]:
        fail(f"{context} content hash does not match the declared SHA-256")
    display = value.get("display", {})
    if (display.get("width"), display.get("height")) != (
        trace["width"],
        trace["height"],
    ):
        fail(f"{context} display does not match the declared dimensions")

    frames = value.get("frames")
    if not isinstance(frames, list) or len(frames) != trace["frame_count"]:
        fail(f"{context} frame count does not match the declared frame_count")
    camera_receipt = [
        {
            "pose": frame.get("pose"),
            "intrinsics": frame.get("intrinsics"),
            "view_matrix": frame.get("view_matrix"),
        }
        for frame in frames
    ]
    camera_sha256 = hashlib.sha256(
        json.dumps(camera_receipt, sort_keys=True, separators=(",", ":")).encode(
            "utf-8"
        )
    ).hexdigest()
    if camera_sha256 != trace["pose_intrinsics_sha256"]:
        fail(
            f"{context} pose/intrinsics receipt does not match "
            "pose_intrinsics_sha256"
        )


def validate_trace(
    trace: dict[str, Any],
    context: str,
    dataset_ids: set[str],
    verify_inputs: bool,
) -> None:
    require_id(trace, "id", context)
    require_sha256(trace, "sha256", context)
    require_string(trace, "local_path", context)
    width = require_int(trace, "width", context, positive=True)
    height = require_int(trace, "height", context, positive=True)
    require_int(trace, "frame_count", context, positive=True)
    evidence_class = require_string(trace, "evidence_class", context)
    if evidence_class not in TRACE_EVIDENCE_CLASSES:
        fail(
            f"{context}.evidence_class must be one of "
            f"{sorted(TRACE_EVIDENCE_CLASSES)}"
        )
    if evidence_class == FORMAL_EVIDENCE_CLASS:
        dataset_id = require_id(trace, "dataset_id", context)
        if dataset_id not in dataset_ids:
            fail(f"{context}.dataset_id references unknown dataset {dataset_id}")
        require_id(trace, "camera_family", context)
        require_sha256(trace, "pose_intrinsics_sha256", context)
        if width < FORMAL_MIN_WIDTH or height < FORMAL_MIN_HEIGHT:
            fail(
                f"{context} formal trace must be at least "
                f"{FORMAL_MIN_WIDTH}x{FORMAL_MIN_HEIGHT}; "
                f"{width}x{height} is diagnostic only"
            )
    if verify_inputs:
        verify_trace_file(trace, context)


def validate_dataset(
    dataset: dict[str, Any],
    context: str,
    dataset_ids: set[str],
    verify_inputs: bool,
) -> None:
    require_id(dataset, "id", context)
    role = require_string(dataset, "role", context)
    if role not in DATASET_ROLES:
        fail(f"{context}.role must be one of {sorted(DATASET_ROLES)}")
    require_string(dataset, "local_path", context)
    require_sha256(dataset, "sha256", context)
    byte_count = require_int(dataset, "bytes", context, positive=True)
    require_int(dataset, "splat_count", context, positive=True)
    degree = require_int(dataset, "sh_degree", context)
    if degree > 3:
        fail(f"{context}.sh_degree must be in 0..3")
    if role == "scaling_tier":
        source_id = require_id(dataset, "source_dataset_id", context)
        if source_id not in dataset_ids:
            fail(f"{context}.source_dataset_id references unknown dataset {source_id}")
        require_string(dataset, "selection_algorithm", context)
        if dataset.get("claim_scope") != "performance_scaling_only":
            fail(f"{context}.claim_scope must equal performance_scaling_only")
    if verify_inputs:
        path = resolve_relative(
            REPO_ROOT,
            dataset["local_path"],
            f"{context}.local_path",
        )
        try:
            actual_bytes = path.stat().st_size
        except OSError as error:
            fail(f"cannot stat {path}: {error}")
        if actual_bytes != byte_count:
            fail(f"{context} byte count mismatch: expected {byte_count}, got {actual_bytes}")
        actual_sha256 = sha256_file(path)
        if actual_sha256 != dataset["sha256"]:
            fail(
                f"{context} SHA-256 mismatch: expected {dataset['sha256']}, "
                f"got {actual_sha256}"
            )


def validate_endpoint(endpoint: dict[str, Any], context: str, status: str) -> None:
    require_id(endpoint, "id", context)
    availability = require_string(endpoint, "availability", context)
    if availability not in ENDPOINT_AVAILABILITY:
        fail(
            f"{context}.availability must be one of "
            f"{sorted(ENDPOINT_AVAILABILITY)}"
        )
    require_string(endpoint, "artifact_platform", context)
    execution = require_string(endpoint, "execution_class", context)
    if execution not in {"physical", "browser", "simulator"}:
        fail(f"{context}.execution_class is invalid")
    require_bool(endpoint, "performance_evidence", context)
    formal_display = endpoint.get("formal_display")
    if availability == "available":
        if not isinstance(formal_display, dict):
            fail(f"{context}.formal_display must be an object for an available endpoint")
        width = require_int(
            formal_display, "width", f"{context}.formal_display", positive=True
        )
        height = require_int(
            formal_display, "height", f"{context}.formal_display", positive=True
        )
        require_string(formal_display, "source", f"{context}.formal_display")
        if width < FORMAL_MIN_WIDTH or height < FORMAL_MIN_HEIGHT:
            fail(
                f"{context}.formal_display {width}x{height} is below the formal "
                "qualification floor; low resolution is diagnostic only"
            )
    if availability == "unavailable":
        require_string(endpoint, "probe", context)
        require_string(endpoint, "reason", context)
    if availability == "probe_required" and status != "planned":
        fail(f"{context} must resolve probe_required before the suite starts")


def validate_build_contract(value: dict[str, Any], status: str) -> None:
    commit = value.get("repository_commit")
    dirty = value.get("working_tree_dirty")
    if status == "planned" and commit is None and dirty is None:
        return
    if not isinstance(commit, str) or GIT_COMMIT_RE.fullmatch(commit) is None:
        fail("suite.build.repository_commit must be a lowercase 40-digit Git commit")
    if not isinstance(dirty, bool):
        fail("suite.build.working_tree_dirty must be boolean")


def require_id_list(
    value: Any, context: str, known: set[str], *, nonempty: bool = True
) -> list[str]:
    if not isinstance(value, list) or (nonempty and not value):
        fail(f"{context} must be a non-empty array")
    result: list[str] = []
    for index, item in enumerate(value):
        if not isinstance(item, str) or item not in known:
            fail(f"{context}[{index}] references unknown id {item!r}")
        if item in result:
            fail(f"{context} contains duplicate id {item}")
        result.append(item)
    return result


def trace_id_for_dataset(
    protocol: dict[str, Any], dataset_id: str, context: str
) -> str:
    camera = protocol["camera"]
    trace_id = camera.get("trace_id")
    trace_by_dataset = camera.get("trace_by_dataset")
    if isinstance(trace_id, str):
        return trace_id
    if isinstance(trace_by_dataset, dict):
        value = trace_by_dataset.get(dataset_id)
        if isinstance(value, str):
            return value
    fail(f"{context} does not select a camera trace for dataset {dataset_id}")


def validate_protocol(
    protocol: dict[str, Any],
    context: str,
    datasets: dict[str, dict[str, Any]],
    endpoints: dict[str, dict[str, Any]],
    traces: dict[str, dict[str, Any]],
) -> None:
    require_id(protocol, "id", context)
    if protocol.get("evidence_class") != FORMAL_EVIDENCE_CLASS:
        fail(
            f"{context}.evidence_class must equal {FORMAL_EVIDENCE_CLASS!r}; "
            "diagnostic resolutions do not belong in the formal matrix"
        )
    selected_dataset_ids = require_id_list(
        protocol.get("dataset_ids"), f"{context}.dataset_ids", set(datasets)
    )
    selected_endpoint_ids = require_id_list(
        protocol.get("endpoint_ids"), f"{context}.endpoint_ids", set(endpoints)
    )
    policies = protocol.get("sort_policies")
    if not isinstance(policies, list) or not policies:
        fail(f"{context}.sort_policies must be a non-empty array")
    if any(not isinstance(item, str) or item not in SORT_POLICIES for item in policies):
        fail(f"{context}.sort_policies must contain only cpu/gpu/adaptive values")
    if len(set(policies)) != len(policies):
        fail(f"{context}.sort_policies must contain unique cpu/gpu/adaptive values")
    require_int(protocol, "repetitions", context, positive=True)
    require_int(protocol, "warmup_frames", context)
    require_int(protocol, "measured_frames", context, positive=True)
    sort_interval = require_int(protocol, "sort_interval", context, positive=True)
    require_int(protocol, "randomization_seed", context)
    randomize = require_bool(protocol, "randomize_policy_order", context)
    if len(policies) > 1 and not randomize:
        fail(f"{context} must randomize runs when comparing multiple sort policies")
    require_bool(protocol, "require_image", context)

    display = require_object(protocol, "display", context)
    width = require_int(display, "width", f"{context}.display", positive=True)
    height = require_int(display, "height", f"{context}.display", positive=True)
    camera = require_object(protocol, "camera", context)
    if camera.get("require_display_match") is not True:
        fail(f"{context}.camera.require_display_match must be true")
    if camera.get("display_policy") != "trace_display_exact":
        fail(
            f"{context}.camera.display_policy must equal 'trace_display_exact'"
        )
    if camera.get("quality_comparable") is not True:
        fail(f"{context}.camera.quality_comparable must be true")
    mode = require_string(camera, "mode", f"{context}.camera")
    if mode not in CAMERA_MODES:
        fail(f"{context}.camera.mode must be one of {sorted(CAMERA_MODES)}")
    raw_trace_id = camera.get("trace_id")
    raw_trace_map = camera.get("trace_by_dataset")
    if (raw_trace_id is None) == (raw_trace_map is None):
        fail(
            f"{context}.camera must declare exactly one of trace_id or "
            "trace_by_dataset"
        )
    if raw_trace_id is not None:
        trace_id = require_id(camera, "trace_id", f"{context}.camera")
        if trace_id not in traces:
            fail(f"{context}.camera.trace_id references unknown trace {trace_id}")
    else:
        if not isinstance(raw_trace_map, dict):
            fail(f"{context}.camera.trace_by_dataset must be an object")
        if set(raw_trace_map) != set(selected_dataset_ids):
            fail(
                f"{context}.camera.trace_by_dataset keys must exactly equal "
                "protocol dataset_ids"
            )
        for dataset_id, trace_id in raw_trace_map.items():
            if not isinstance(trace_id, str) or trace_id not in traces:
                fail(
                    f"{context}.camera.trace_by_dataset[{dataset_id!r}] "
                    f"references unknown trace {trace_id!r}"
                )
    frame_indices = require_array(camera, "frame_indices", f"{context}.camera")
    if not frame_indices:
        fail(f"{context}.camera.frame_indices must not be empty")
    normalized: list[int] = []
    for index, value in enumerate(frame_indices):
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            fail(f"{context}.camera.frame_indices[{index}] must be non-negative")
        if value in normalized:
            fail(f"{context}.camera.frame_indices contains duplicate {value}")
        normalized.append(value)
    if mode == "fixed_frame" and protocol.get("sort_refresh") != "first_frame_then_reuse":
        fail(f"{context}.sort_refresh must equal first_frame_then_reuse")
    if mode == "trace_sequence":
        if len(normalized) < 2:
            fail(f"{context}.camera.trace_sequence requires at least two frames")
        if protocol.get("sort_refresh") != "every_camera_revision":
            fail(f"{context}.sort_refresh must equal every_camera_revision")
        if sort_interval != 1:
            fail(f"{context}.trace_sequence requires sort_interval=1")
    for endpoint_id in selected_endpoint_ids:
        formal_display = endpoints[endpoint_id].get("formal_display")
        if not isinstance(formal_display, dict):
            fail(
                f"{context} references endpoint {endpoint_id} without a resolved "
                "formal_display"
            )
        if (width, height) != (
            formal_display.get("width"),
            formal_display.get("height"),
        ):
            fail(
                f"{context}.display must exactly match endpoint {endpoint_id} "
                "formal_display"
            )

    for dataset_id in selected_dataset_ids:
        trace_id = trace_id_for_dataset(protocol, dataset_id, context)
        trace = traces[trace_id]
        if trace.get("evidence_class") != FORMAL_EVIDENCE_CLASS:
            fail(f"{context} selects diagnostic trace {trace_id} for formal evidence")
        source_dataset_id = datasets[dataset_id].get("source_dataset_id", dataset_id)
        if trace.get("dataset_id") != source_dataset_id:
            fail(
                f"{context} trace {trace_id} belongs to {trace.get('dataset_id')!r}, "
                f"not dataset/source {source_dataset_id!r}"
            )
        if (width, height) != (trace["width"], trace["height"]):
            fail(f"{context}.display must exactly match trace {trace_id} dimensions")
        if any(index >= trace["frame_count"] for index in normalized):
            fail(f"{context}.camera.frame_indices exceed trace {trace_id} frame count")


@dataclass(frozen=True, order=True)
class Cell:
    protocol_id: str
    dataset_id: str
    endpoint_id: str
    sort_policy: str
    camera_case: str
    repetition: int

    def label(self) -> str:
        return (
            f"{self.protocol_id}/{self.dataset_id}/{self.endpoint_id}/"
            f"{self.sort_policy}/{self.camera_case}/r{self.repetition:02d}"
        )


def protocol_camera_cases(protocol: dict[str, Any]) -> list[str]:
    camera = protocol["camera"]
    if camera["mode"] == "trace_sequence":
        return ["sequence"]
    return [f"frame-{index:03d}" for index in camera["frame_indices"]]


def expected_cells(
    protocols: dict[str, dict[str, Any]], endpoints: dict[str, dict[str, Any]]
) -> set[Cell]:
    cells: set[Cell] = set()
    for protocol_id, protocol in protocols.items():
        for endpoint_id in protocol["endpoint_ids"]:
            if endpoints[endpoint_id]["availability"] != "available":
                continue
            for dataset_id in protocol["dataset_ids"]:
                for policy in protocol["sort_policies"]:
                    for camera_case in protocol_camera_cases(protocol):
                        for repetition in range(1, protocol["repetitions"] + 1):
                            cells.add(
                                Cell(
                                    protocol_id,
                                    dataset_id,
                                    endpoint_id,
                                    policy,
                                    camera_case,
                                    repetition,
                                )
                            )
    return cells


def run_cell(run: dict[str, Any], context: str) -> Cell:
    protocol_id = require_id(run, "protocol_id", context)
    dataset_id = require_id(run, "dataset_id", context)
    endpoint_id = require_id(run, "endpoint_id", context)
    policy = require_string(run, "sort_policy", context)
    if policy not in SORT_POLICIES:
        fail(f"{context}.sort_policy must be cpu, gpu, or adaptive")
    camera_case = require_string(run, "camera_case", context)
    repetition = require_int(run, "repetition", context, positive=True)
    return Cell(protocol_id, dataset_id, endpoint_id, policy, camera_case, repetition)


def validate_exactness(
    manifest: dict[str, Any], dataset: dict[str, Any], context: str
) -> None:
    exactness = require_object(manifest, "exactness", context)
    count = dataset["splat_count"]
    for field in EXACT_COUNT_FIELDS:
        actual = require_int(exactness, field, f"{context}.exactness")
        if actual != count:
            fail(f"{context}.exactness.{field} must equal dataset count {count}, got {actual}")
    for field in ("source_sh_degree", "resident_sh_degree"):
        actual = require_int(exactness, field, f"{context}.exactness")
        if actual != dataset["sh_degree"]:
            fail(
                f"{context}.exactness.{field} must equal source SH degree "
                f"{dataset['sh_degree']}, got {actual}"
            )
    required_literals = {
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "sh_degree_policy": "source",
        "partial_scene_published": False,
        "full_quality": True,
    }
    for field, expected in required_literals.items():
        if exactness.get(field) != expected:
            fail(f"{context}.exactness.{field} must equal {expected!r}")


def validate_resolution(
    manifest: dict[str, Any], protocol: dict[str, Any], context: str
) -> None:
    """Prove that a retained frame was rendered at the declared pixel size."""

    resolution = require_object(manifest, "resolution", context)
    expected = (protocol["display"]["width"], protocol["display"]["height"])
    for stage in RESOLUTION_STAGES:
        width = require_int(
            resolution, f"{stage}_width", f"{context}.resolution", positive=True
        )
        height = require_int(
            resolution, f"{stage}_height", f"{context}.resolution", positive=True
        )
        if (width, height) != expected:
            fail(
                f"{context}.resolution.{stage} dimensions must equal the "
                f"protocol display {expected[0]}x{expected[1]}, got {width}x{height}"
            )
    if resolution.get("dynamic_resolution") != "disabled":
        fail(f"{context}.resolution.dynamic_resolution must equal 'disabled'")
    if resolution.get("upscaling") != "disabled":
        fail(f"{context}.resolution.upscaling must equal 'disabled'")
    if resolution.get("full_resolution") is not True:
        fail(f"{context}.resolution.full_resolution must be true")


def validate_trace_display_policy(
    manifest: dict[str, Any], protocol: dict[str, Any], context: str
) -> None:
    """Reject endpoint-aspect reprojection from formal quality evidence."""

    trace = require_object(manifest, "trace", context)
    if trace.get("require_display_match") is not True:
        fail(f"{context}.trace.require_display_match must be true")
    if trace.get("display_policy") != "trace_display_exact":
        fail(f"{context}.trace.display_policy must equal 'trace_display_exact'")
    if trace.get("quality_comparable") is not True:
        fail(f"{context}.trace.quality_comparable must be true")

    expected = (protocol["display"]["width"], protocol["display"]["height"])
    reference = (
        require_int(trace, "reference_width", f"{context}.trace", positive=True),
        require_int(trace, "reference_height", f"{context}.trace", positive=True),
    )
    if reference != expected:
        fail(
            f"{context}.trace reference dimensions must equal the protocol display "
            f"{expected[0]}x{expected[1]}, got {reference[0]}x{reference[1]}"
        )

    resolution = require_object(manifest, "resolution", context)
    requested = (
        require_int(
            resolution, "requested_width", f"{context}.resolution", positive=True
        ),
        require_int(
            resolution, "requested_height", f"{context}.resolution", positive=True
        ),
    )
    if reference != requested:
        fail(
            f"{context}.trace reference dimensions must equal requested resolution "
            f"{requested[0]}x{requested[1]}"
        )


def validate_sort_telemetry(
    frames: list[dict[str, Any]],
    summary: dict[str, Any],
    policy: str,
    protocol: dict[str, Any],
    context: str,
) -> None:
    telemetry = require_object(summary, "sort_telemetry", context)
    sample_count = summary["sample_count"]
    cpu = require_int(telemetry, "cpu_frame_count", f"{context}.sort_telemetry")
    gpu = require_int(telemetry, "gpu_frame_count", f"{context}.sort_telemetry")
    fallbacks = require_int(
        telemetry, "gpu_sort_fallback_count", f"{context}.sort_telemetry"
    )
    if cpu + gpu != sample_count:
        fail(f"{context} CPU/GPU frame counts must sum to sample_count")
    if policy == "cpu" and (cpu != sample_count or gpu != 0 or fallbacks != 0):
        fail(f"{context} forced CPU run contains GPU or fallback frames")
    if policy == "gpu" and (gpu != sample_count or cpu != 0 or fallbacks != 0):
        fail(f"{context} forced GPU run contains CPU or fallback frames")
    if protocol["camera"]["mode"] == "trace_sequence":
        for index, frame in enumerate(frames):
            if frame.get("sort_refreshed") is not True:
                fail(f"{context} frame {index} must refresh sort for the motion trace")


def validate_frame_counts(
    frames: list[dict[str, Any]],
    dataset: dict[str, Any],
    renderer: dict[str, Any],
    context: str,
) -> None:
    source_count = dataset["splat_count"]
    uses_contributor_contract = renderer.get("count_semantics") == COUNT_SEMANTICS
    for index, frame in enumerate(frames):
        visible = frame.get("visible")
        drawn = frame.get("drawn")
        if visible > source_count or drawn > source_count:
            fail(f"{context} frame {index} count exceeds the complete source count")
        contributor_present = "contributor" in frame
        compaction_present = "exact_contributor_compaction" in frame
        if uses_contributor_contract:
            if not contributor_present or not compaction_present:
                fail(
                    f"{context} frame {index} contributor contract requires C and "
                    "exact_contributor_compaction"
                )
            contributor = frame["contributor"]
            if isinstance(contributor, bool) or not isinstance(contributor, int) or contributor < 0:
                fail(f"{context} frame {index} contributor must be a non-negative integer")
            exact_compaction = frame["exact_contributor_compaction"]
            if not isinstance(exact_compaction, bool):
                fail(
                    f"{context} frame {index} exact_contributor_compaction must be boolean"
                )
            if contributor > visible:
                fail(
                    f"{context} frame {index} violates 0 <= contributor <= visible <= source"
                )
            if exact_compaction and drawn != contributor:
                fail(
                    f"{context} frame {index} exact contributor draw requires "
                    f"drawn=contributor, got D={drawn}, C={contributor}"
                )
            if not exact_compaction and drawn != visible:
                fail(
                    f"{context} frame {index} non-compacted/legacy execution requires "
                    f"drawn=visible, got D={drawn}, V={visible}"
                )
        elif contributor_present or compaction_present:
            fail(
                f"{context} frame {index} emits contributor fields without the explicit "
                f"renderer count_semantics={COUNT_SEMANTICS} contract"
            )
        elif drawn != visible:
            fail(
                f"{context} frame {index} reports drawn={drawn}, visible={visible}; "
                "legacy full-quality artifacts require D=V and may not apply a draw budget"
            )


def png_dimensions(path: pathlib.Path) -> tuple[int, int]:
    try:
        with path.open("rb") as handle:
            header = handle.read(24)
    except OSError as error:
        fail(f"cannot read image {path}: {error}")
    if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        fail(f"image is not a PNG with an IHDR header: {path}")
    return struct.unpack(">II", header[16:24])


def validate_image(
    run: dict[str, Any], protocol: dict[str, Any], suite_root: pathlib.Path, context: str
) -> None:
    image = require_object(run, "image", context)
    path_text = require_string(image, "path", f"{context}.image")
    expected_sha256 = require_sha256(image, "sha256", f"{context}.image")
    expected_width = require_int(image, "width", f"{context}.image", positive=True)
    expected_height = require_int(image, "height", f"{context}.image", positive=True)
    display = protocol["display"]
    if (expected_width, expected_height) != (display["width"], display["height"]):
        fail(f"{context}.image dimensions must equal the protocol display")
    path = resolve_relative(suite_root, path_text, f"{context}.image.path")
    if png_dimensions(path) != (expected_width, expected_height):
        fail(f"{context}.image PNG dimensions do not match its receipt")
    if sha256_file(path) != expected_sha256:
        fail(f"{context}.image SHA-256 mismatch")


def benchmark_parts(
    artifact_dir: pathlib.Path,
) -> tuple[dict[str, Any], list[dict[str, Any]], dict[str, Any]]:
    validator = load_module(BENCHMARK_VALIDATOR, "gsplat_benchmark_validator")
    try:
        validator.validate(artifact_dir)
        manifest = validator.load_json(artifact_dir / "manifest.json")
        frames = validator.load_frames(
            artifact_dir / "frames.jsonl",
            manifest["run_id"],
            set(manifest["unavailable_fields"]),
            count_semantics=manifest["renderer"].get("count_semantics"),
            source_count=manifest["dataset"]["splat_count"],
        )
        summary = validator.load_json(artifact_dir / "summary.json")
    except ValueError as error:
        fail(f"benchmark artifact {artifact_dir} is invalid: {error}")
    return manifest, frames, summary


def validate_run(
    run: dict[str, Any],
    cell: Cell,
    suite_root: pathlib.Path,
    datasets: dict[str, dict[str, Any]],
    endpoints: dict[str, dict[str, Any]],
    protocols: dict[str, dict[str, Any]],
    traces: dict[str, dict[str, Any]],
    render_path: str,
    build_contract: dict[str, Any],
    context: str,
) -> None:
    require_int(run, "schedule_index", context, positive=True)
    policy_position = require_int(run, "policy_position", context, positive=True)
    if policy_position > len(protocols[cell.protocol_id]["sort_policies"]):
        fail(f"{context}.policy_position exceeds the protocol policy count")
    artifact_text = require_string(run, "artifact", context)
    artifact_dir = resolve_relative(suite_root, artifact_text, f"{context}.artifact")
    manifest, frames, summary = benchmark_parts(artifact_dir)
    dataset = datasets[cell.dataset_id]
    endpoint = endpoints[cell.endpoint_id]
    protocol = protocols[cell.protocol_id]
    trace = traces[
        trace_id_for_dataset(protocol, cell.dataset_id, f"protocol {cell.protocol_id}")
    ]

    artifact_dataset = require_object(manifest, "dataset", f"{context}.manifest")
    for field in ("sha256", "bytes", "splat_count", "sh_degree"):
        if artifact_dataset.get(field) != dataset[field]:
            fail(
                f"{context} artifact dataset.{field} mismatch: "
                f"expected {dataset[field]!r}, got {artifact_dataset.get(field)!r}"
            )
    artifact_build = require_object(manifest, "build", f"{context}.manifest")
    if artifact_build.get("repository_commit") != build_contract["repository_commit"]:
        fail(f"{context} artifact repository commit does not match the suite")
    if artifact_build.get("dirty") != build_contract["working_tree_dirty"]:
        fail(f"{context} artifact dirty state does not match the suite")
    artifact_trace = require_object(manifest, "trace", f"{context}.manifest")
    if artifact_trace.get("id") != trace["id"] or artifact_trace.get("sha256") != trace["sha256"]:
        fail(f"{context} artifact trace identity does not match the protocol")
    if protocol["camera"]["mode"] == "fixed_frame":
        expected_frame = int(cell.camera_case.removeprefix("frame-"))
        if artifact_trace.get("frame_index") != expected_frame:
            fail(f"{context} artifact trace.frame_index mismatch")
    else:
        if artifact_trace.get("frame_indices") != protocol["camera"]["frame_indices"]:
            fail(f"{context} artifact trace.frame_indices mismatch")

    renderer = require_object(manifest, "renderer", f"{context}.manifest")
    if renderer.get("path") != render_path:
        fail(f"{context} artifact renderer.path must equal {render_path}")
    if renderer.get("order_backend_requested") != cell.sort_policy:
        fail(f"{context} artifact order_backend_requested mismatch")
    if renderer.get("sort_interval") != protocol["sort_interval"]:
        fail(f"{context} artifact sort_interval mismatch")
    environment = require_object(manifest, "environment", f"{context}.manifest")
    if environment.get("platform") != endpoint["artifact_platform"]:
        fail(f"{context} artifact environment.platform mismatch")
    display = require_object(manifest, "display", f"{context}.manifest")
    if (display.get("width"), display.get("height")) != (
        protocol["display"]["width"],
        protocol["display"]["height"],
    ):
        fail(f"{context} artifact display dimensions mismatch")
    if summary.get("sample_count") != protocol["measured_frames"]:
        fail(f"{context} sample_count does not match the protocol")
    if summary.get("warmup_count") != protocol["warmup_frames"]:
        fail(f"{context} warmup_count does not match the protocol")
    validate_exactness(manifest, dataset, f"{context}.manifest")
    validate_trace_display_policy(manifest, protocol, f"{context}.manifest")
    validate_resolution(manifest, protocol, f"{context}.manifest")
    validate_frame_counts(frames, dataset, renderer, context)
    validate_sort_telemetry(frames, summary, cell.sort_policy, protocol, context)
    if protocol["require_image"]:
        validate_image(run, protocol, suite_root, context)
    elif run.get("image") is not None:
        validate_image(run, protocol, suite_root, context)


def validate_capacity_rejections(
    values: list[Any],
    datasets: dict[str, dict[str, Any]],
    endpoints: dict[str, dict[str, Any]],
) -> set[tuple[str, str]]:
    result: set[tuple[str, str]] = set()
    for index, value in enumerate(values):
        context = f"capacity_rejections[{index}]"
        if not isinstance(value, dict):
            fail(f"{context} must be an object")
        endpoint_id = require_id(value, "endpoint_id", context)
        dataset_id = require_id(value, "dataset_id", context)
        if endpoint_id not in endpoints or dataset_id not in datasets:
            fail(f"{context} references an unknown endpoint or dataset")
        if endpoints[endpoint_id]["availability"] != "available":
            fail(f"{context} may only describe an available endpoint")
        pair = (endpoint_id, dataset_id)
        if pair in result:
            fail(f"duplicate capacity rejection for {endpoint_id}/{dataset_id}")
        result.add(pair)
        dataset = datasets[dataset_id]
        if require_int(value, "source_splat_count", context) != dataset["splat_count"]:
            fail(f"{context}.source_splat_count mismatch")
        if require_int(value, "source_sh_degree", context) != dataset["sh_degree"]:
            fail(f"{context}.source_sh_degree mismatch")
        if value.get("scene_published") is not False:
            fail(f"{context}.scene_published must be false")
        require_string(value, "stage", context)
        require_string(value, "error_code", context)
        require_string(value, "error_message", context)
        resource = require_object(value, "resource", context)
        require_string(resource, "kind", f"{context}.resource")
        require_int(resource, "required_bytes", f"{context}.resource", positive=True)
        limit = resource.get("limit_bytes")
        if limit is not None and (
            isinstance(limit, bool) or not isinstance(limit, int) or limit < 0
        ):
            fail(f"{context}.resource.limit_bytes must be non-negative or null")
    return result


@dataclass(frozen=True)
class Result:
    expected_cells: int
    rendered_cells: int
    capacity_rejected_cells: int
    missing_cells: tuple[str, ...]


def validate(
    suite_path: pathlib.Path,
    *,
    allow_incomplete: bool = False,
    verify_inputs: bool = False,
) -> Result:
    suite_path = suite_path.resolve()
    suite_root = suite_path.parent
    suite = load_json(suite_path)
    if suite.get("schema") != SCHEMA:
        fail(f"schema must equal {SCHEMA}")
    require_id(suite, "suite_id", "suite")
    status = require_string(suite, "status", "suite")
    if status not in {"planned", "running", "complete"}:
        fail("suite.status must be planned, running, or complete")
    if not allow_incomplete and status != "complete":
        fail("suite.status must be complete (use --allow-incomplete for a plan/in-progress suite)")
    pre_run = require_array(suite, "pre_run_requirements", "suite")
    if any(not isinstance(item, str) or not item for item in pre_run):
        fail("suite.pre_run_requirements must contain non-empty strings")
    if status != "planned" and pre_run:
        fail("suite.pre_run_requirements must be empty before a suite starts")
    quality = require_object(suite, "quality_contract", "suite")
    if quality != QUALITY_CONTRACT:
        fail(f"suite.quality_contract must exactly equal {QUALITY_CONTRACT!r}")
    render_path = require_string(suite, "renderer_path", "suite")
    build_contract = require_object(suite, "build", "suite")
    validate_build_contract(build_contract, status)

    trace_values = require_array(suite, "traces", "suite")
    dataset_values = require_array(suite, "datasets", "suite")
    endpoint_values = require_array(suite, "endpoints", "suite")
    protocol_values = require_array(suite, "protocols", "suite")
    traces = unique_index(trace_values, "traces")
    datasets = unique_index(dataset_values, "datasets")
    endpoints = unique_index(endpoint_values, "endpoints")
    protocols = unique_index(protocol_values, "protocols")
    if not traces or not datasets or not endpoints or not protocols:
        fail("suite axes must all be non-empty")

    dataset_ids = set(datasets)
    for index, dataset in enumerate(dataset_values):
        validate_dataset(
            dataset,
            f"datasets[{index}]",
            dataset_ids,
            verify_inputs,
        )
    for index, trace in enumerate(trace_values):
        validate_trace(trace, f"traces[{index}]", dataset_ids, verify_inputs)
    for dataset in dataset_values:
        if dataset["role"] == "scaling_tier":
            source = datasets[dataset["source_dataset_id"]]
            if source["role"] != "full_scene":
                fail(f"scaling dataset {dataset['id']} must reference a full_scene source")
            if dataset["splat_count"] >= source["splat_count"]:
                fail(f"scaling dataset {dataset['id']} must be smaller than its source")
            if dataset["sh_degree"] != source["sh_degree"]:
                fail(f"scaling dataset {dataset['id']} must retain source SH degree")
    for index, endpoint in enumerate(endpoint_values):
        validate_endpoint(endpoint, f"endpoints[{index}]", status)
    for index, protocol in enumerate(protocol_values):
        validate_protocol(
            protocol,
            f"protocols[{index}]",
            datasets,
            endpoints,
            traces,
        )

    # A dataset's endpoint-sized traces must be the same camera, not merely
    # similarly named views.  The only permitted cross-display difference is
    # the aspect-dependent projection matrix.
    camera_receipts_by_dataset: dict[str, tuple[str, str]] = {}
    for protocol in protocol_values:
        for dataset_id in protocol["dataset_ids"]:
            source_dataset_id = datasets[dataset_id].get("source_dataset_id", dataset_id)
            trace = traces[
                trace_id_for_dataset(protocol, dataset_id, f"protocol {protocol['id']}")
            ]
            receipt = (trace["camera_family"], trace["pose_intrinsics_sha256"])
            prior = camera_receipts_by_dataset.setdefault(source_dataset_id, receipt)
            if receipt != prior:
                fail(
                    f"dataset {source_dataset_id} uses multiple pose/FOV camera "
                    "families across formal endpoint traces"
                )

    expected = expected_cells(protocols, endpoints)
    rejected_pairs = validate_capacity_rejections(
        require_array(suite, "capacity_rejections", "suite"), datasets, endpoints
    )
    rejected_cells = {
        cell for cell in expected if (cell.endpoint_id, cell.dataset_id) in rejected_pairs
    }
    runnable = expected - rejected_cells
    seen: set[Cell] = set()
    schedule_indices: set[int] = set()
    policy_positions: dict[tuple[str, str, str, str, int], set[int]] = {}
    for index, value in enumerate(require_array(suite, "runs", "suite")):
        context = f"runs[{index}]"
        if not isinstance(value, dict):
            fail(f"{context} must be an object")
        cell = run_cell(value, context)
        if cell not in expected:
            fail(f"{context} is not a declared available matrix cell: {cell.label()}")
        if (cell.endpoint_id, cell.dataset_id) in rejected_pairs:
            fail(f"{context} conflicts with a capacity rejection: {cell.label()}")
        if cell in seen:
            fail(f"duplicate run cell: {cell.label()}")
        seen.add(cell)
        schedule_index = require_int(value, "schedule_index", context, positive=True)
        if schedule_index in schedule_indices:
            fail(f"duplicate runs[{index}].schedule_index {schedule_index}")
        schedule_indices.add(schedule_index)
        group = (
            cell.protocol_id,
            cell.dataset_id,
            cell.endpoint_id,
            cell.camera_case,
            cell.repetition,
        )
        position = require_int(value, "policy_position", context, positive=True)
        if position in policy_positions.setdefault(group, set()):
            fail(f"duplicate policy_position {position} in run group {group}")
        policy_positions[group].add(position)
        validate_run(
            value,
            cell,
            suite_root,
            datasets,
            endpoints,
            protocols,
            traces,
            render_path,
            build_contract,
            context,
        )

    missing = tuple(cell.label() for cell in sorted(runnable - seen))
    if missing and not allow_incomplete:
        preview = ", ".join(missing[:5])
        suffix = " ..." if len(missing) > 5 else ""
        fail(f"suite is missing {len(missing)} matrix cells: {preview}{suffix}")
    if status == "complete" and missing:
        fail("a complete suite cannot have missing matrix cells")
    if not missing:
        for group, positions in policy_positions.items():
            protocol = protocols[group[0]]
            expected_positions = set(range(1, len(protocol["sort_policies"]) + 1))
            if positions != expected_positions:
                fail(
                    f"run group {group} policy positions mismatch: "
                    f"expected {sorted(expected_positions)}, got {sorted(positions)}"
                )
    return Result(len(expected), len(seen), len(rejected_cells), missing)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite", type=pathlib.Path, help="path to suite.json")
    parser.add_argument(
        "--allow-incomplete",
        action="store_true",
        help="validate a planned/running suite without requiring every matrix cell",
    )
    parser.add_argument(
        "--verify-inputs",
        action="store_true",
        help="hash every local dataset and run the canonical camera-trace validator",
    )
    args = parser.parse_args()
    try:
        result = validate(
            args.suite,
            allow_incomplete=args.allow_incomplete,
            verify_inputs=args.verify_inputs,
        )
    except ValidationError as error:
        print(f"full-quality experiment validation failed: {error}", file=sys.stderr)
        return 1
    print(
        "full-quality experiment valid: "
        f"expected={result.expected_cells} rendered={result.rendered_cells} "
        f"capacity_rejected={result.capacity_rejected_cells} "
        f"missing={len(result.missing_cells)} suite={args.suite}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
