#!/usr/bin/env python3
"""Validate one fail-closed ``gsplat-scalable-proxy-image-gate/v1`` artifact.

The validator consumes retained evidence only. It validates canonical benchmark
artifacts before applying the S1-specific proxy coverage, V/C/D, image, and
temporal rules. It never fills a missing field, converts an unavailable count
to zero, or treats a contract fixture as S1 promotion evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import pathlib
import re
import sys
from dataclasses import dataclass
from typing import Any


SCHEMA = "gsplat-scalable-proxy-image-gate/v1"
PREFLIGHT_SCHEMA = "gsplat-scalable-proxy-image-gate-preflight/v1"
VALIDATOR_VERSION = 1
BALANCED_VALIDATOR_SHA256 = (
    "6c1e61edf97096ecb8dd1555cc9553353d6a12dd77373c643f5a65a4138c0dfa"
)
BENCHMARK_VALIDATOR_SHA256 = (
    "3a47dc9221e28a13985928c531d53143934f62313f9d8d257758fb1befbecc6c"
)
REQUIRED_CUTS = (
    "complete_leaf_exact",
    "bootstrap_roots",
    "mixed_depth_two_replacements",
)
REQUIRED_ENDPOINTS = ("apple_m4_metal", "nothing_a065_vulkan")
REQUIRED_ORDERS = ("cpu", "gpu")
RESOLUTION_STAGES = ("requested", "surface", "internal_render", "presented")
MATCHED_GENERATIONS = (
    "scene_generation",
    "camera_generation",
    "viewport_generation",
    "contract_generation",
    "plan_generation",
    "presentation_generation",
)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
BALANCED_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-balanced-image-gate.py"
BENCHMARK_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
FRAME_METRIC_LIMITS = {
    "ssim_luma_srgb_window8": ("minimum", 0.99),
    "rgb_mae_normalized": ("maximum", 0.005),
    "rgb_bad_pixel_fraction_over_3": ("maximum", 0.02),
    "alpha_mae_normalized": ("maximum", 0.001),
    "alpha_bad_pixel_fraction_over_1": ("maximum", 0.005),
}
TEMPORAL_METRIC = "temporal_rgb_residual_mae_normalized"
TEMPORAL_LIMIT = 0.005
METRIC_RECEIPT_TOLERANCE = 1.0e-9
FORMAL_SOURCE = {
    "dataset_id": "bonsai",
    "local_path": "tests/datasets/external/inria_3dgs/bonsai/point_cloud.ply",
    "sha256": "a16af6d8815498ffbf9eb5d5ee93f5bcc9dca34c4e3eb6f7a796ef9e97c0d273",
    "bytes": 308_716_644,
    "splat_count": 1_244_819,
    "sh_degree": 3,
}
FORMAL_DATASET_POLICY = {
    "qualification_status": "local_candidate",
    "source_url": "https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/datasets/pretrained/models.zip",
    "archive_entry": "bonsai/point_cloud/iteration_30000/point_cloud.ply",
    "source_repository": "https://github.com/graphdeco-inria/gaussian-splatting",
    "source_repository_license_url": "https://github.com/graphdeco-inria/gaussian-splatting/blob/main/LICENSE.md",
    "upstream_dataset": "Mip-NeRF 360 indoor",
    "upstream_dataset_url": "https://jonbarron.info/mipnerf360/",
    "fetch_script": "tests/datasets/fetch_inria_3dgs_scenes.py",
    "license": None,
    "license_context": "the source repository publishes a research/evaluation software license, but the pretrained archive does not state an asset-specific model license",
    "attribution": "Official pretrained 3D Gaussian Splatting model by Kerbl et al., Inria GRAPHDECO and MPII",
    "allowed_use": "local research/evaluation only",
    "redistribution": "prohibited unless model and upstream dataset rights are clarified",
    "conversion": "none",
}
FORMAL_CAMERA = {
    "path": "tests/datasets/external/inria_3dgs/bonsai/cameras.json",
    "sha256": "41e623748141d5b1a292c2bcafbf9e897a3876f90c11a14618e9ac6190b05af3",
    "bytes": 116_695,
    "entry_count": 292,
    "selected_camera_ids": [0, 146],
}
FORMAL_ENDPOINTS = {
    "apple_m4_metal": {
        "backend": "metal",
        "width": 1920,
        "height": 1080,
        "trace_path": "tests/perf/trace/fixtures/quality/candidate-bonsai-quality-1920x1080-v1.json",
        "trace_file_sha256": "ea7f09ca4cec606f153308f8c9ae707eff96efabd897752e5e462e9b76fe81a6",
        "trace_content_sha256": "8f0c419cdd090bc93bdfd46d5876f954c46b6d189e4f72d34c8a0fa475a87ef3",
    },
    "nothing_a065_vulkan": {
        "backend": "vulkan",
        "width": 2412,
        "height": 1080,
        "trace_path": "tests/perf/trace/fixtures/quality/candidate-bonsai-quality-2412x1080-v1.json",
        "trace_file_sha256": "af70f0291197ad6e13b2dc1ab7bce77588a1d497db26721d039b1e30655a4d53",
        "trace_content_sha256": "b189dc06ac35f0a4e3805b5f53d7caab5a40c3e06e042836c848cb4aa185975f",
    },
}


class ValidationError(ValueError):
    """Evidence exists but is invalid or incomplete: S1 Rejected."""


class DeferredEvidence(ValidationError):
    """A named external prerequisite or required endpoint is unavailable."""


@dataclass(frozen=True)
class Authority:
    dataset_id: str
    source_sha256: str
    source_bytes: int
    source_splats: int
    source_sh_degree: int
    camera_metadata_sha256: str
    camera_review_traces: dict[str, tuple[str, str]]
    hierarchy_manifest_sha256: str
    builder_commit: str
    builder_configuration_sha256: str


@dataclass(frozen=True)
class Cut:
    name: str
    coverage_sha256: str
    active_proxy_splats: int


@dataclass(frozen=True)
class Endpoint:
    endpoint_id: str
    backend: str
    width: int
    height: int
    trace_id: str
    trace_file_sha256: str
    trace_content_sha256: str
    trace: dict[str, Any]


@dataclass(frozen=True)
class ValidationResult:
    evidence_class: str
    endpoint_count: int
    comparison_count: int
    transition_count: int
    validator_sha256: str


def reject(message: str) -> None:
    raise ValidationError(message)


def require_object(parent: dict[str, Any], key: str, context: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        reject(f"{context}.{key} must be an object")
    return value


def require_array(parent: dict[str, Any], key: str, context: str) -> list[Any]:
    value = parent.get(key)
    if not isinstance(value, list):
        reject(f"{context}.{key} must be an array")
    return value


def require_string(parent: dict[str, Any], key: str, context: str) -> str:
    value = parent.get(key)
    if not isinstance(value, str) or not value:
        reject(f"{context}.{key} must be a non-empty string")
    return value


def require_bool(parent: dict[str, Any], key: str, context: str) -> bool:
    value = parent.get(key)
    if not isinstance(value, bool):
        reject(f"{context}.{key} must be boolean")
    return value


def require_int(
    parent: dict[str, Any], key: str, context: str, *, positive: bool = False
) -> int:
    value = parent.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        reject(f"{context}.{key} must be an available non-negative integer")
    if positive and value == 0:
        reject(f"{context}.{key} must be positive")
    return value


def require_number(parent: dict[str, Any], key: str, context: str) -> float:
    value = parent.get(key)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        reject(f"{context}.{key} must be an available number")
    result = float(value)
    if not math.isfinite(result):
        reject(f"{context}.{key} must be finite")
    return result


def require_sha256(parent: dict[str, Any], key: str, context: str) -> str:
    value = require_string(parent, key, context)
    if SHA256_RE.fullmatch(value) is None:
        reject(f"{context}.{key} must be a lowercase SHA-256")
    return value


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        reject(f"cannot hash {path}: {error}")
    return digest.hexdigest()


def canonical_sha256(value: Any) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def load_module(name: str, path: pathlib.Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        reject(f"cannot load validator dependency {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    try:
        spec.loader.exec_module(module)
    finally:
        sys.modules.pop(name, None)
    return module


def resolve_scoped_file(
    balanced: Any,
    artifact_root: pathlib.Path,
    receipt: dict[str, Any],
    context: str,
) -> pathlib.Path:
    scope = require_string(receipt, "scope", context)
    root = artifact_root if scope == "artifact" else REPO_ROOT if scope == "repository" else None
    if root is None:
        reject(f"{context}.scope must be artifact or repository")
    try:
        return balanced.resolve_artifact_file(
            root, require_string(receipt, "path", context), f"{context}.path"
        )
    except balanced.ValidationError as error:
        reject(str(error))


def validate_dependencies(manifest: dict[str, Any]) -> None:
    dependencies = require_object(manifest, "validator_dependencies", "manifest")
    expected = {
        "balanced_image_gate_sha256": (
            BALANCED_VALIDATOR_PATH,
            BALANCED_VALIDATOR_SHA256,
        ),
        "benchmark_artifact_validator_sha256": (
            BENCHMARK_VALIDATOR_PATH,
            BENCHMARK_VALIDATOR_SHA256,
        ),
    }
    for key, (path, pinned) in expected.items():
        declared = require_sha256(dependencies, key, "manifest.validator_dependencies")
        actual = sha256_file(path)
        if actual != pinned:
            reject(f"pinned dependency {path.name} has drifted from S0 section 9.4")
        if declared != actual:
            reject(f"manifest.validator_dependencies.{key} mismatch")


def validate_pinned_dependencies() -> list[dict[str, Any]]:
    checks = []
    for path, pinned in (
        (BALANCED_VALIDATOR_PATH, BALANCED_VALIDATOR_SHA256),
        (BENCHMARK_VALIDATOR_PATH, BENCHMARK_VALIDATOR_SHA256),
    ):
        actual = sha256_file(path)
        if actual != pinned:
            reject(f"pinned dependency {path.name} has drifted from S0 section 9.4")
        checks.append(
            {
                "name": f"validator:{path.name}",
                "status": "available",
                "sha256": actual,
            }
        )
    return checks


def load_json_file(path: pathlib.Path, context: str) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        reject(f"cannot read {context}: {error}")


def validate_frozen_trace(
    path: pathlib.Path, endpoint_id: str, expected: dict[str, Any]
) -> dict[str, Any]:
    actual_file_hash = sha256_file(path)
    if actual_file_hash != expected["trace_file_sha256"]:
        reject(f"frozen {endpoint_id} trace file SHA-256 mismatch")
    trace = load_json_file(path, f"frozen {endpoint_id} trace")
    if not isinstance(trace, dict) or trace.get("schema") != "gsplat-camera-trace/v1":
        reject(f"frozen {endpoint_id} trace schema mismatch")
    if trace.get("content_sha256") != expected["trace_content_sha256"]:
        reject(f"frozen {endpoint_id} trace content SHA-256 mismatch")
    if trace.get("display") != {
        "width": expected["width"],
        "height": expected["height"],
    }:
        reject(f"frozen {endpoint_id} trace resolution mismatch")
    derivation = trace.get("derivation")
    if not isinstance(derivation, dict):
        reject(f"frozen {endpoint_id} trace derivation is missing")
    if derivation.get("status") != "candidate_requires_manual_image_review":
        reject(f"frozen {endpoint_id} trace changed its manual-review boundary")
    source_identity = {
        "local_path": derivation.get("source_path"),
        "sha256": derivation.get("source_sha256"),
        "bytes": derivation.get("source_bytes"),
        "splat_count": derivation.get("source_splat_count"),
        "sh_degree": derivation.get("source_sh_degree"),
    }
    if source_identity != {
        key: FORMAL_SOURCE[key]
        for key in ("local_path", "sha256", "bytes", "splat_count", "sh_degree")
    }:
        reject(f"frozen {endpoint_id} trace source authority mismatch")
    views = derivation.get("views")
    if (
        not isinstance(views, list)
        or any(not isinstance(view, dict) for view in views)
        or [view.get("source_camera_id") for view in views] != [0, 146]
    ):
        reject(f"frozen {endpoint_id} trace must retain authored cameras 0/146")
    camera = derivation.get("official_camera_metadata")
    if not isinstance(camera, dict) or {
        "path": camera.get("local_path"),
        "sha256": camera.get("sha256"),
        "bytes": camera.get("bytes"),
        "entry_count": camera.get("entry_count"),
    } != {key: FORMAL_CAMERA[key] for key in ("path", "sha256", "bytes", "entry_count")}:
        reject(f"frozen {endpoint_id} trace camera authority mismatch")
    return {
        "name": f"trace:{endpoint_id}",
        "status": "available",
        "file_sha256": actual_file_hash,
        "content_sha256": trace["content_sha256"],
    }


def read_ply_header_identity(path: pathlib.Path) -> tuple[int, int]:
    header = bytearray()
    try:
        with path.open("rb") as handle:
            while b"end_header\n" not in header:
                chunk = handle.read(4096)
                if not chunk or len(header) + len(chunk) > 1024 * 1024:
                    reject("formal Bonsai source has no bounded PLY header")
                header.extend(chunk)
    except OSError as error:
        reject(f"cannot read formal Bonsai source header: {error}")
    try:
        text = bytes(header).split(b"end_header\n", 1)[0].decode("ascii")
    except UnicodeDecodeError as error:
        reject(f"formal Bonsai source header is not ASCII: {error}")
    vertex_count = None
    rest_count = 0
    for line in text.splitlines():
        if line.startswith("element vertex "):
            try:
                vertex_count = int(line.split()[2])
            except (IndexError, ValueError):
                reject("formal Bonsai source vertex count is malformed")
        elif line.startswith("property ") and line.split()[-1].startswith("f_rest_"):
            rest_count += 1
    if vertex_count is None:
        reject("formal Bonsai source vertex count is missing")
    degree_by_rest = {0: 0, 9: 1, 24: 2, 45: 3}
    if rest_count not in degree_by_rest:
        reject(f"formal Bonsai source SH rest count {rest_count} is invalid")
    return vertex_count, degree_by_rest[rest_count]


def formal_collection_preflight() -> dict[str, Any]:
    checks = validate_pinned_dependencies()
    missing = []

    dataset_path = REPO_ROOT / "tests/perf/datasets/bonsai.local-candidate.json"
    dataset = load_json_file(dataset_path, "frozen Bonsai dataset manifest")
    if not isinstance(dataset, dict) or dataset.get("schema") != "gsplat-dataset/v1":
        reject("frozen Bonsai dataset manifest schema mismatch")
    dataset_identity = {
        "dataset_id": dataset.get("id"),
        "local_path": dataset.get("local_path"),
        "sha256": dataset.get("sha256"),
        "bytes": dataset.get("bytes"),
        "splat_count": dataset.get("splat_count"),
        "sh_degree": dataset.get("sh_degree"),
    }
    if dataset_identity != FORMAL_SOURCE:
        reject("frozen Bonsai dataset manifest identity mismatch")
    if {key: dataset.get(key) for key in FORMAL_DATASET_POLICY} != FORMAL_DATASET_POLICY:
        reject("Bonsai authority provenance or local-only rights scope mismatch")
    checks.append(
        {
            "name": "authority:bonsai-dataset-manifest",
            "status": "available",
            "sha256": sha256_file(dataset_path),
            "rights_scope": "local_research_evaluation_only",
        }
    )

    source_path = REPO_ROOT / FORMAL_SOURCE["local_path"]
    if source_path.exists():
        if source_path.stat().st_size != FORMAL_SOURCE["bytes"]:
            reject("formal Bonsai source byte count mismatch")
        source_hash = sha256_file(source_path)
        if source_hash != FORMAL_SOURCE["sha256"]:
            reject("formal Bonsai source SHA-256 mismatch")
        splat_count, sh_degree = read_ply_header_identity(source_path)
        if (splat_count, sh_degree) != (
            FORMAL_SOURCE["splat_count"],
            FORMAL_SOURCE["sh_degree"],
        ):
            reject("formal Bonsai source count or complete SH3 identity mismatch")
        checks.append(
            {
                "name": "authority:bonsai-source",
                "status": "available",
                "sha256": source_hash,
                "splat_count": splat_count,
                "sh_degree": sh_degree,
            }
        )
    else:
        missing.append(
            {
                "name": "authority:bonsai-source",
                "reason": f"missing {FORMAL_SOURCE['local_path']}",
            }
        )

    camera_path = REPO_ROOT / FORMAL_CAMERA["path"]
    if camera_path.exists():
        if camera_path.stat().st_size != FORMAL_CAMERA["bytes"]:
            reject("formal Bonsai camera metadata byte count mismatch")
        camera_hash = sha256_file(camera_path)
        if camera_hash != FORMAL_CAMERA["sha256"]:
            reject("formal Bonsai camera metadata SHA-256 mismatch")
        camera_records = load_json_file(camera_path, "formal Bonsai camera metadata")
        if not isinstance(camera_records, list) or len(camera_records) != FORMAL_CAMERA["entry_count"]:
            reject("formal Bonsai camera metadata entry count mismatch")
        camera_ids = set()
        for index, record in enumerate(camera_records):
            if not isinstance(record, dict):
                reject(f"formal Bonsai camera metadata entry {index} must be an object")
            camera_id = record.get("id")
            if isinstance(camera_id, bool) or not isinstance(camera_id, int) or camera_id < 0:
                reject(f"formal Bonsai camera metadata entry {index} has an invalid id")
            camera_ids.add(camera_id)
        if not set(FORMAL_CAMERA["selected_camera_ids"]).issubset(camera_ids):
            reject("formal Bonsai camera metadata is missing cameras 0/146")
        checks.append(
            {
                "name": "authority:bonsai-cameras-0-146",
                "status": "available",
                "sha256": camera_hash,
                "entry_count": len(camera_records),
            }
        )
    else:
        missing.append(
            {
                "name": "authority:bonsai-cameras-0-146",
                "reason": f"missing {FORMAL_CAMERA['path']}",
            }
        )

    for endpoint_id, expected in FORMAL_ENDPOINTS.items():
        checks.append(
            validate_frozen_trace(REPO_ROOT / expected["trace_path"], endpoint_id, expected)
        )

    missing.extend(
        [
            {
                "name": "authority:authored-camera-review",
                "reason": "no approved review receipt binds both frozen trace hashes",
            },
            {
                "name": "endpoint:apple_m4_metal",
                "reason": "no retained formal proxy-image gate artifact was supplied",
            },
            {
                "name": "endpoint:nothing_a065_vulkan",
                "reason": "no retained formal proxy-image gate artifact was supplied",
            },
        ]
    )
    return {
        "schema": PREFLIGHT_SCHEMA,
        "decision": "Deferred",
        "pass": False,
        "scope": "formal_quality_collection_preflight",
        "checks": checks,
        "missing_prerequisites": missing,
        "s2_s5_unlocked": False,
        "validator": {
            "version": VALIDATOR_VERSION,
            "sha256": sha256_file(pathlib.Path(__file__).resolve()),
            "balanced_image_gate_sha256": BALANCED_VALIDATOR_SHA256,
            "benchmark_artifact_validator_sha256": BENCHMARK_VALIDATOR_SHA256,
        },
    }


def validate_authority(
    manifest: dict[str, Any],
    artifact_root: pathlib.Path,
    evidence_class: str,
    balanced: Any,
) -> Authority:
    authority = require_object(manifest, "authority", "manifest")
    dataset_receipt = require_object(authority, "dataset_manifest", "manifest.authority")
    dataset_path = resolve_scoped_file(
        balanced, artifact_root, dataset_receipt, "manifest.authority.dataset_manifest"
    )
    if sha256_file(dataset_path) != require_sha256(
        dataset_receipt, "sha256", "manifest.authority.dataset_manifest"
    ):
        reject("manifest.authority.dataset_manifest SHA-256 mismatch")
    try:
        dataset = balanced.load_json(dataset_path)
    except balanced.ValidationError as error:
        reject(f"invalid dataset authority: {error}")
    if dataset.get("schema") != "gsplat-dataset/v1":
        reject("authority dataset schema must equal 'gsplat-dataset/v1'")
    dataset_id = require_string(dataset, "id", "authority dataset")
    source_sha256 = require_sha256(dataset, "sha256", "authority dataset")
    source_bytes = require_int(dataset, "bytes", "authority dataset", positive=True)
    source_splats = require_int(dataset, "splat_count", "authority dataset", positive=True)
    source_sh_degree = require_int(dataset, "sh_degree", "authority dataset")
    if source_sh_degree != 3:
        reject("S1 proxy-image evidence requires complete source SH3")

    camera = require_object(authority, "camera", "manifest.authority")
    camera_path = require_string(camera, "path", "manifest.authority.camera")
    camera_sha256 = require_sha256(camera, "sha256", "manifest.authority.camera")
    camera_bytes = require_int(camera, "bytes", "manifest.authority.camera", positive=True)
    camera_entries = require_int(
        camera, "entry_count", "manifest.authority.camera", positive=True
    )
    selected_ids = require_array(camera, "selected_camera_ids", "manifest.authority.camera")
    if len(selected_ids) != 2 or any(
        isinstance(value, bool) or not isinstance(value, int) or value < 0
        for value in selected_ids
    ):
        reject("manifest.authority.camera.selected_camera_ids must contain two IDs")
    review = require_object(camera, "review", "manifest.authority.camera")
    if require_sha256(camera, "review_sha256", "manifest.authority.camera") != canonical_sha256(review):
        reject("manifest.authority.camera.review_sha256 mismatch")
    review_status = require_string(review, "status", "manifest.authority.camera.review")
    if review_status == "unavailable":
        reason = require_string(review, "reason", "manifest.authority.camera.review")
        raise DeferredEvidence(f"authored-camera review is unavailable: {reason}")
    if review_status != "approved":
        reject("manifest.authority.camera.review.status must be approved or unavailable")
    if require_sha256(review, "source_sha256", "manifest.authority.camera.review") != source_sha256:
        reject("authored-camera review source hash mismatch")
    if require_sha256(
        review, "camera_metadata_sha256", "manifest.authority.camera.review"
    ) != camera_sha256:
        reject("authored-camera review metadata hash mismatch")
    if require_array(
        review, "selected_camera_ids", "manifest.authority.camera.review"
    ) != selected_ids:
        reject("authored-camera review selected camera IDs mismatch")
    raw_review_traces = require_array(
        review, "trace_receipts", "manifest.authority.camera.review"
    )
    camera_review_traces: dict[str, tuple[str, str]] = {}
    for index, raw_trace in enumerate(raw_review_traces):
        context = f"manifest.authority.camera.review.trace_receipts[{index}]"
        if not isinstance(raw_trace, dict):
            reject(f"{context} must be an object")
        endpoint_id = require_string(raw_trace, "endpoint_id", context)
        if endpoint_id in camera_review_traces:
            reject(f"duplicate authored-camera review endpoint {endpoint_id!r}")
        camera_review_traces[endpoint_id] = (
            require_sha256(raw_trace, "trace_file_sha256", context),
            require_sha256(raw_trace, "trace_content_sha256", context),
        )
    if set(camera_review_traces) != set(REQUIRED_ENDPOINTS):
        reject("authored-camera review must bind both frozen endpoint traces")

    hierarchy = require_object(authority, "hierarchy_manifest", "manifest.authority")
    hierarchy_path = resolve_scoped_file(
        balanced, artifact_root, hierarchy, "manifest.authority.hierarchy_manifest"
    )
    hierarchy_sha256 = require_sha256(
        hierarchy, "sha256", "manifest.authority.hierarchy_manifest"
    )
    if sha256_file(hierarchy_path) != hierarchy_sha256:
        reject("manifest.authority.hierarchy_manifest SHA-256 mismatch")
    builder = require_object(authority, "builder", "manifest.authority")
    builder_commit = require_string(builder, "repository_commit", "manifest.authority.builder")
    if COMMIT_RE.fullmatch(builder_commit) is None:
        reject("manifest.authority.builder.repository_commit must be a 40-hex commit")
    builder_configuration_sha256 = require_sha256(
        builder, "configuration_sha256", "manifest.authority.builder"
    )

    if evidence_class == "formal_quality":
        if dataset_receipt.get("scope") != "repository" or dataset_receipt.get("path") != "tests/perf/datasets/bonsai.local-candidate.json":
            reject("formal_quality requires the frozen Bonsai authority manifest")
        actual_source = {
            "dataset_id": dataset_id,
            "local_path": dataset.get("local_path"),
            "sha256": source_sha256,
            "bytes": source_bytes,
            "splat_count": source_splats,
            "sh_degree": source_sh_degree,
        }
        if actual_source != FORMAL_SOURCE:
            reject("formal_quality source identity does not match frozen Bonsai")
        if {key: dataset.get(key) for key in FORMAL_DATASET_POLICY} != FORMAL_DATASET_POLICY:
            reject("formal_quality Bonsai authority provenance or rights scope mismatch")
        actual_camera = {
            "path": camera_path,
            "sha256": camera_sha256,
            "bytes": camera_bytes,
            "entry_count": camera_entries,
            "selected_camera_ids": selected_ids,
        }
        if actual_camera != FORMAL_CAMERA:
            reject("formal_quality camera authority does not match frozen Bonsai cameras 0/146")

    return Authority(
        dataset_id=dataset_id,
        source_sha256=source_sha256,
        source_bytes=source_bytes,
        source_splats=source_splats,
        source_sh_degree=source_sh_degree,
        camera_metadata_sha256=camera_sha256,
        camera_review_traces=camera_review_traces,
        hierarchy_manifest_sha256=hierarchy_sha256,
        builder_commit=builder_commit,
        builder_configuration_sha256=builder_configuration_sha256,
    )


def validate_exact_reference(
    manifest: dict[str, Any], authority: Authority, balanced: Any
) -> str:
    exact_reference = require_object(manifest, "exact_reference", "manifest")
    exactness = require_object(exact_reference, "exactness", "manifest.exact_reference")
    exactness_sha256 = require_sha256(
        exact_reference, "exactness_sha256", "manifest.exact_reference"
    )
    if canonical_sha256(exactness) != exactness_sha256:
        reject("manifest.exact_reference.exactness_sha256 mismatch")
    balanced_authority = balanced.Authority(
        dataset_id=authority.dataset_id,
        dataset_asset_sha256=authority.source_sha256,
        dataset_asset_bytes=authority.source_bytes,
        source_splat_count=authority.source_splats,
        source_sh_degree=authority.source_sh_degree,
        trace_id="not-used-by-exactness-validation",
        trace_content_sha256="0" * 64,
        trace={},
    )
    try:
        balanced.validate_exactness({"exactness": exactness}, balanced_authority)
    except balanced.ValidationError as error:
        reject(f"manifest.exact_reference fails unchanged B1 membership: {error}")
    return exactness_sha256


def validate_cuts(manifest: dict[str, Any], authority: Authority) -> dict[str, Cut]:
    raw_cuts = require_array(manifest, "cuts", "manifest")
    cuts: dict[str, Cut] = {}
    for index, raw_cut in enumerate(raw_cuts):
        context = f"manifest.cuts[{index}]"
        if not isinstance(raw_cut, dict):
            reject(f"{context} must be an object")
        name = require_string(raw_cut, "name", context)
        if name in cuts:
            reject(f"duplicate proxy cut {name!r}")
        coverage = require_object(raw_cut, "coverage", context)
        coverage_sha256 = require_sha256(raw_cut, "coverage_sha256", context)
        if canonical_sha256(coverage) != coverage_sha256:
            reject(f"{context}.coverage_sha256 mismatch")
        coverage_context = f"{context}.coverage"
        if require_sha256(coverage, "source_sha256", coverage_context) != authority.source_sha256:
            reject(f"{context} source hash mismatch")
        if require_sha256(
            coverage, "hierarchy_manifest_sha256", coverage_context
        ) != authority.hierarchy_manifest_sha256:
            reject(f"{context} hierarchy manifest hash mismatch")
        if require_int(coverage, "source_splat_count", coverage_context) != authority.source_splats:
            reject(f"{context} source count S mismatch")
        represented = require_int(coverage, "represented_source_leaves", coverage_context)
        active = require_int(coverage, "active_proxy_splats", coverage_context, positive=True)
        if represented != authority.source_splats:
            reject(f"{context} must prove R=S complete source-leaf coverage")
        if require_int(coverage, "missing_leaves", coverage_context) != 0:
            reject(f"{context} has missing source leaves")
        if require_int(coverage, "overlap_count", coverage_context) != 0:
            reject(f"{context} has overlapping source leaves")
        if require_int(coverage, "missing_page_count", coverage_context) != 0:
            reject(f"{context} has a missing page")
        if not require_bool(coverage, "antichain_valid", coverage_context):
            reject(f"{context} must prove a valid antichain")
        if require_bool(coverage, "parent_descendant_overlap", coverage_context):
            reject(f"{context} has parent/descendant overlap")
        if active > authority.source_splats:
            reject(f"{context} active proxy count P exceeds S")
        if require_int(coverage, "source_sh_degree", coverage_context) != authority.source_sh_degree:
            reject(f"{context} source SH degree mismatch")
        required_strings = {
            "sh_representation": "source_sh3",
            "sampling": "disabled",
            "partial_child_publication": "disabled",
        }
        for key, expected in required_strings.items():
            if require_string(coverage, key, coverage_context) != expected:
                reject(f"{coverage_context}.{key} must equal {expected!r}")
        node_ids = require_array(coverage, "ordered_node_ids", coverage_context)
        if not node_ids or any(not isinstance(value, str) or not value for value in node_ids):
            reject(f"{coverage_context}.ordered_node_ids must be non-empty strings")
        if len(set(node_ids)) != len(node_ids):
            reject(f"{coverage_context}.ordered_node_ids contains duplicates")
        if require_sha256(
            coverage, "ordered_node_list_sha256", coverage_context
        ) != canonical_sha256(node_ids):
            reject(f"{coverage_context}.ordered_node_list_sha256 mismatch")
        page_hashes = require_array(coverage, "page_sha256", coverage_context)
        if not page_hashes or any(
            not isinstance(value, str) or SHA256_RE.fullmatch(value) is None
            for value in page_hashes
        ):
            reject(f"{coverage_context}.page_sha256 must bind every cut page")
        if len(set(page_hashes)) != len(page_hashes):
            reject(f"{coverage_context}.page_sha256 contains duplicates")
        if require_sha256(
            coverage, "page_list_sha256", coverage_context
        ) != canonical_sha256(page_hashes):
            reject(f"{coverage_context}.page_list_sha256 mismatch")
        replacement_count = require_int(coverage, "replacement_count", coverage_context)
        depth_count = require_int(coverage, "depth_count", coverage_context, positive=True)
        if name == "complete_leaf_exact":
            if active != authority.source_splats or not require_bool(
                coverage, "payload_bit_exact_to_source", coverage_context
            ):
                reject("complete_leaf_exact requires P=S and bit-exact source payloads")
        elif require_bool(coverage, "payload_bit_exact_to_source", coverage_context):
            reject(f"{name} must not claim bit-exact full-source payloads")
        if name == "bootstrap_roots" and replacement_count != 0:
            reject("bootstrap_roots replacement_count must be zero")
        if name == "mixed_depth_two_replacements" and (
            replacement_count != 2 or depth_count < 2
        ):
            reject("mixed_depth_two_replacements requires two replacements and mixed depths")
        cuts[name] = Cut(name, coverage_sha256, active)
    if set(cuts) != set(REQUIRED_CUTS):
        reject("manifest.cuts must contain exactly the three frozen S1 proxy cuts")
    return cuts


def validate_endpoints(
    manifest: dict[str, Any],
    artifact_root: pathlib.Path,
    evidence_class: str,
    authority: Authority,
    balanced: Any,
) -> dict[str, Endpoint]:
    raw_endpoints = require_array(manifest, "endpoints", "manifest")
    indexed: dict[str, dict[str, Any]] = {}
    for index, raw_endpoint in enumerate(raw_endpoints):
        context = f"manifest.endpoints[{index}]"
        if not isinstance(raw_endpoint, dict):
            reject(f"{context} must be an object")
        endpoint_id = require_string(raw_endpoint, "id", context)
        if endpoint_id in indexed:
            reject(f"duplicate endpoint {endpoint_id!r}")
        indexed[endpoint_id] = raw_endpoint
    missing = [endpoint for endpoint in REQUIRED_ENDPOINTS if endpoint not in indexed]
    if missing:
        raise DeferredEvidence(f"required endpoint evidence is unavailable: {', '.join(missing)}")
    if set(indexed) != set(REQUIRED_ENDPOINTS):
        reject("manifest.endpoints contains an endpoint outside the frozen S1 v1 set")

    endpoints: dict[str, Endpoint] = {}
    trace_validator = balanced.load_trace_validator()
    for endpoint_id in REQUIRED_ENDPOINTS:
        raw_endpoint = indexed[endpoint_id]
        context = f"manifest.endpoints[{endpoint_id}]"
        status = require_string(raw_endpoint, "status", context)
        if status == "unavailable":
            reason = require_string(raw_endpoint, "reason", context)
            raise DeferredEvidence(f"required endpoint {endpoint_id} is unavailable: {reason}")
        if status != "complete":
            reject(f"{context}.status must be complete or unavailable")
        backend = require_string(raw_endpoint, "backend", context)
        resolution = require_object(raw_endpoint, "resolution", context)
        trace_receipt = require_object(raw_endpoint, "trace", context)
        trace_path = resolve_scoped_file(
            balanced, artifact_root, trace_receipt, f"{context}.trace"
        )
        trace_file_sha256 = require_sha256(trace_receipt, "sha256", f"{context}.trace")
        if sha256_file(trace_path) != trace_file_sha256:
            reject(f"{context}.trace SHA-256 mismatch")
        try:
            trace = balanced.load_json(trace_path)
            trace_validator.validate(trace)
        except (balanced.ValidationError, trace_validator.ValidationError) as error:
            reject(f"{context}.trace is invalid: {error}")
        trace_id = require_string(trace, "trace_id", f"{context}.trace document")
        trace_content_sha256 = require_sha256(
            trace, "content_sha256", f"{context}.trace document"
        )
        if require_string(trace_receipt, "trace_id", f"{context}.trace") != trace_id:
            reject(f"{context}.trace.trace_id mismatch")
        if require_sha256(
            trace_receipt, "content_sha256", f"{context}.trace"
        ) != trace_content_sha256:
            reject(f"{context}.trace.content_sha256 mismatch")
        if authority.camera_review_traces[endpoint_id] != (
            trace_file_sha256,
            trace_content_sha256,
        ):
            reject(f"{context}.trace does not match the authored-camera review")
        try:
            width, height = balanced.validate_resolution(
                {"resolution": resolution}, trace, evidence_class
            )
        except balanced.ValidationError as error:
            reject(f"{context}.resolution fails unchanged B1 authority: {error}")
        probe = require_object(raw_endpoint, "surface_probe", context)
        if require_string(probe, "status", f"{context}.surface_probe") != "observed":
            reject(f"{context}.surface_probe.status must equal 'observed'")
        if (
            require_int(probe, "width", f"{context}.surface_probe", positive=True),
            require_int(probe, "height", f"{context}.surface_probe", positive=True),
        ) != (width, height):
            reject(f"{context}.surface_probe dimensions mismatch")
        frames = require_array(trace, "frames", f"{context}.trace document")
        if len(frames) < 2:
            reject(f"{context}.trace must contain frozen authored views 0 and 1")

        if evidence_class == "formal_quality":
            expected = FORMAL_ENDPOINTS[endpoint_id]
            actual = {
                "backend": backend,
                "width": width,
                "height": height,
                "trace_path": trace_receipt.get("path"),
                "trace_file_sha256": trace_file_sha256,
                "trace_content_sha256": trace_content_sha256,
            }
            if trace_receipt.get("scope") != "repository" or actual != expected:
                reject(f"{context} does not match the frozen formal endpoint/trace")
        endpoints[endpoint_id] = Endpoint(
            endpoint_id,
            backend,
            width,
            height,
            trace_id,
            trace_file_sha256,
            trace_content_sha256,
            trace,
        )
    return endpoints


def validate_presentation(
    raw: dict[str, Any], context: str
) -> tuple[dict[str, dict[str, Any]], dict[str, dict[str, int]]]:
    presentations: dict[str, dict[str, Any]] = {}
    generations: dict[str, dict[str, int]] = {}
    for lane in ("exact", "proxy"):
        presentation = require_object(raw, lane, context)
        lane_context = f"{context}.{lane}"
        if require_string(presentation, "outcome", lane_context) != "presented":
            reject(f"{lane_context}.outcome must equal 'presented'")
        if not require_bool(presentation, "primitive_presented", lane_context):
            reject(f"{lane_context}.primitive_presented must be true")
        require_int(presentation, "ticket", lane_context, positive=True)
        presentations[lane] = presentation
        generations[lane] = {
            key: require_int(presentation, key, lane_context, positive=True)
            for key in MATCHED_GENERATIONS
        }
    for key in MATCHED_GENERATIONS:
        if generations["exact"][key] != generations["proxy"][key]:
            reject(f"{context} Exact/proxy {key} must match")
    return presentations, generations


def validate_vcd(
    raw: dict[str, Any], active: int, context: str
) -> tuple[int, int, int, bool]:
    visible = require_int(raw, "visible", context)
    contributor = require_int(raw, "contributor", context)
    drawn = require_int(raw, "drawn", context)
    compacted = require_bool(raw, "exact_contributor_compaction", context)
    if not 0 <= contributor <= visible <= active:
        reject(f"{context} must prove 0 <= C <= V <= active splats")
    expected_drawn = contributor if compacted else visible
    if drawn != expected_drawn:
        relation = "D=C" if compacted else "D=V"
        reject(f"{context} must prove {relation}")
    return visible, contributor, drawn, compacted


def validate_benchmark_lane(
    receipt: dict[str, Any],
    lane: str,
    raw_comparison: dict[str, Any],
    presentation: dict[str, Any],
    image_receipt: dict[str, Any],
    endpoint: Endpoint,
    authority: Authority,
    cut: Cut,
    artifact_root: pathlib.Path,
    balanced: Any,
    benchmark: Any,
    context: str,
) -> tuple[dict[str, Any], dict[str, Any], pathlib.Path]:
    lane_context = f"{context}.benchmark_artifacts.{lane}"
    try:
        artifact = balanced.resolve_artifact_directory(
            artifact_root,
            require_string(receipt, "path", lane_context),
            f"{lane_context}.path",
        )
    except balanced.ValidationError as error:
        reject(str(error))
    if balanced.artifact_directory_sha256(artifact) != require_sha256(
        receipt, "sha256", lane_context
    ):
        reject(f"{lane_context} benchmark artifact SHA-256 mismatch")
    try:
        benchmark.validate(artifact)
    except benchmark.ValidationError as error:
        reject(f"{lane_context} is not canonical gsplat-benchmark/v1 evidence: {error}")
    benchmark_manifest = benchmark.load_json(artifact / "manifest.json")
    run_id = require_string(receipt, "run_id", lane_context)
    if benchmark_manifest.get("run_id") != run_id:
        reject(f"{lane_context}.run_id mismatch")
    renderer = benchmark_manifest["renderer"]
    if renderer.get("count_semantics") != benchmark.COUNT_SEMANTICS:
        reject(f"{lane_context} must declare canonical V/C/D count semantics")
    unavailable = set(benchmark_manifest["unavailable_fields"])
    frames = benchmark.load_frames(
        artifact / "frames.jsonl",
        run_id,
        unavailable,
        count_semantics=renderer.get("count_semantics"),
        source_count=authority.source_splats,
    )
    frame_index = require_int(receipt, "frame_index", lane_context)
    if frame_index != len(frames) - 1:
        reject(f"{lane_context}.frame_index must identify the terminal frame")
    frame = frames[frame_index]
    expected_dataset = {
        "id": authority.dataset_id,
        "sha256": authority.source_sha256,
        "bytes": authority.source_bytes,
        "splat_count": authority.source_splats,
        "sh_degree": authority.source_sh_degree,
    }
    for key, expected in expected_dataset.items():
        if benchmark_manifest["dataset"].get(key) != expected:
            reject(f"{lane_context} benchmark dataset.{key} mismatch")
    if benchmark_manifest["trace"] != {
        "id": endpoint.trace_id,
        "sha256": endpoint.trace_content_sha256,
    }:
        reject(f"{lane_context} benchmark trace identity mismatch")
    if (
        benchmark_manifest["display"].get("width"),
        benchmark_manifest["display"].get("height"),
    ) != (endpoint.width, endpoint.height):
        reject(f"{lane_context} benchmark resolution mismatch")
    build = benchmark_manifest["build"]
    if build.get("dirty") is not False or COMMIT_RE.fullmatch(str(build.get("repository_commit"))) is None:
        reject(f"{lane_context} benchmark build must name a clean commit")
    expected_frame_identity = {
        "pair_id": raw_comparison["pair_id"],
        "endpoint_id": raw_comparison["endpoint_id"],
        "order_backend": raw_comparison["order_backend"],
        "cut_name": raw_comparison["cut_name"],
        "sequence": raw_comparison["sequence"],
        "capture_index": raw_comparison["capture_index"],
        "trace_frame_index": raw_comparison["trace_frame_index"],
        "camera": raw_comparison["camera"],
        "terminal_outcome": "presented",
        "presentation": presentation,
    }
    for key, expected in expected_frame_identity.items():
        if frame.get(key) != expected:
            reject(f"{lane_context} terminal frame {key} mismatch")
    expected_active = authority.source_splats if lane == "exact" else cut.active_proxy_splats
    if frame.get("active_splats") != expected_active:
        reject(f"{lane_context} active_splats mismatch")
    validate_vcd(frame, expected_active, f"{lane_context} terminal V/C/D")
    benchmark_image = require_object(benchmark_manifest, "image", f"{lane_context}.manifest")
    if require_sha256(benchmark_image, "sha256", f"{lane_context}.manifest.image") != require_sha256(
        image_receipt, "sha256", f"{context}.images.{lane}"
    ):
        reject(f"{lane_context} benchmark image hash mismatch")
    if (
        require_int(benchmark_image, "width", f"{lane_context}.manifest.image"),
        require_int(benchmark_image, "height", f"{lane_context}.manifest.image"),
    ) != (endpoint.width, endpoint.height):
        reject(f"{lane_context} benchmark image dimensions mismatch")
    return benchmark_manifest, frame, artifact


def expected_comparison_keys() -> set[tuple[str, str, str, str, int]]:
    expected: set[tuple[str, str, str, str, int]] = set()
    for endpoint_id in REQUIRED_ENDPOINTS:
        for order in REQUIRED_ORDERS:
            for cut in REQUIRED_CUTS:
                expected.update(
                    (endpoint_id, order, cut, "authored_views", index)
                    for index in range(2)
                )
                expected.update(
                    (endpoint_id, order, cut, "moving_sequence", index)
                    for index in range(3)
                )
            expected.update(
                (endpoint_id, order, cut, "replacement_sequence", index)
                for index, cut in enumerate(
                    ("bootstrap_roots", "mixed_depth_two_replacements", "complete_leaf_exact")
                )
            )
    return expected


def validate_comparisons(
    manifest: dict[str, Any],
    artifact_root: pathlib.Path,
    authority: Authority,
    cuts: dict[str, Cut],
    endpoints: dict[str, Endpoint],
    exact_reference_sha256: str,
    balanced: Any,
    benchmark: Any,
) -> dict[tuple[str, str, str, str, int], Any]:
    raw_comparisons = require_array(manifest, "comparisons", "manifest")
    frames: dict[tuple[str, str, str, str, int], Any] = {}
    run_ids: set[str] = set()
    pair_ids: set[str] = set()
    tickets: dict[str, set[int]] = {"exact": set(), "proxy": set()}
    coverage_generations: set[int] = set()
    for index, raw in enumerate(raw_comparisons):
        context = f"manifest.comparisons[{index}]"
        if not isinstance(raw, dict):
            reject(f"{context} must be an object")
        endpoint_id = require_string(raw, "endpoint_id", context)
        order = require_string(raw, "order_backend", context)
        cut_name = require_string(raw, "cut_name", context)
        sequence = require_string(raw, "sequence", context)
        capture_index = require_int(raw, "capture_index", context)
        trace_frame_index = require_int(raw, "trace_frame_index", context)
        pair_id = require_string(raw, "pair_id", context)
        if pair_id in pair_ids:
            reject(f"{context}.pair_id is reused")
        pair_ids.add(pair_id)
        if endpoint_id not in endpoints or order not in REQUIRED_ORDERS or cut_name not in cuts:
            reject(f"{context} names an unknown endpoint, order backend, or cut")
        if sequence == "authored_views":
            expected_trace = (0, 1)
        elif sequence == "moving_sequence":
            expected_trace = (0, 1, 0)
        elif sequence == "replacement_sequence":
            expected_trace = (0, 0, 0)
            expected_cut = (
                "bootstrap_roots",
                "mixed_depth_two_replacements",
                "complete_leaf_exact",
            )
            if capture_index >= len(expected_cut) or cut_name != expected_cut[capture_index]:
                reject(f"{context} replacement sequence must use the frozen cut order")
        else:
            reject(f"{context}.sequence is not a frozen S1 capture sequence")
        if capture_index >= len(expected_trace) or trace_frame_index != expected_trace[capture_index]:
            reject(f"{context} capture/trace index mismatch")
        key = (endpoint_id, order, cut_name, sequence, capture_index)
        if key in frames:
            reject(f"duplicate comparison identity {key}")
        endpoint = endpoints[endpoint_id]
        cut = cuts[cut_name]
        if require_sha256(raw, "exact_reference_sha256", context) != exact_reference_sha256:
            reject(f"{context}.exact_reference_sha256 mismatch")

        camera = require_object(raw, "camera", context)
        if require_string(camera, "trace_id", f"{context}.camera") != endpoint.trace_id:
            reject(f"{context}.camera.trace_id mismatch")
        if require_sha256(
            camera, "trace_content_sha256", f"{context}.camera"
        ) != endpoint.trace_content_sha256:
            reject(f"{context}.camera.trace_content_sha256 mismatch")
        trace_frame = endpoint.trace["frames"][trace_frame_index]
        pose_hash = canonical_sha256(
            {"pose": trace_frame["pose"], "intrinsics": trace_frame["intrinsics"]}
        )
        if require_sha256(camera, "pose_intrinsics_sha256", f"{context}.camera") != pose_hash:
            reject(f"{context}.camera pose/intrinsics hash mismatch")

        presentations, _ = validate_presentation(
            require_object(raw, "presentation", context), f"{context}.presentation"
        )
        for lane in ("exact", "proxy"):
            ticket = presentations[lane]["ticket"]
            if ticket in tickets[lane]:
                reject(f"{context}.presentation.{lane}.ticket is reused")
            tickets[lane].add(ticket)
        presented_cut = require_object(raw, "presented_cut", context)
        presented_context = f"{context}.presented_cut"
        expected_cut_identity = {
            "source_sha256": authority.source_sha256,
            "hierarchy_manifest_sha256": authority.hierarchy_manifest_sha256,
            "cut_name": cut_name,
            "coverage_sha256": cut.coverage_sha256,
            "order_backend": order,
            "outcome": "presented",
        }
        for field, expected in expected_cut_identity.items():
            if presented_cut.get(field) != expected:
                reject(f"{presented_context}.{field} mismatch")
        if presented_cut.get("presentation") != presentations["proxy"]:
            reject(f"{presented_context}.presentation mismatch")
        coverage_generation = require_int(
            presented_cut, "coverage_generation", presented_context, positive=True
        )
        if coverage_generation in coverage_generations:
            reject(f"{presented_context}.coverage_generation is reused")
        coverage_generations.add(coverage_generation)
        if require_int(presented_cut, "source_splat_count", presented_context) != authority.source_splats:
            reject(f"{presented_context} S mismatch")
        if require_int(presented_cut, "represented_source_leaves", presented_context) != authority.source_splats:
            reject(f"{presented_context} R must equal S")
        if require_int(presented_cut, "active_proxy_splats", presented_context) != cut.active_proxy_splats:
            reject(f"{presented_context} P mismatch")
        presented_vcd = validate_vcd(presented_cut, cut.active_proxy_splats, presented_context)
        if require_string(presented_cut, "global_plan", presented_context) == "adaptive":
            reject(f"{presented_context}.global_plan must identify a forced non-Adaptive S1 lane")

        images = require_object(raw, "images", context)
        exact_receipt = require_object(images, "exact", f"{context}.images")
        proxy_receipt = require_object(images, "proxy", f"{context}.images")
        try:
            exact_image = balanced.load_image(
                artifact_root, exact_receipt, (endpoint.width, endpoint.height), f"{context}.images.exact"
            )
            proxy_image = balanced.load_image(
                artifact_root, proxy_receipt, (endpoint.width, endpoint.height), f"{context}.images.proxy"
            )
        except balanced.ValidationError as error:
            reject(str(error))
        assert exact_image.path is not None and proxy_image.path is not None
        try:
            if exact_image.path.samefile(proxy_image.path):
                reject(f"{context} Exact/proxy images must be separate artifacts")
        except OSError as error:
            reject(f"{context} cannot compare image identity: {error}")

        benchmark_pair = require_object(raw, "benchmark_artifacts", context)
        if require_string(benchmark_pair, "pair_id", f"{context}.benchmark_artifacts") != raw["pair_id"]:
            reject(f"{context}.benchmark_artifacts.pair_id mismatch")
        benchmark_manifests: list[dict[str, Any]] = []
        benchmark_frames: dict[str, dict[str, Any]] = {}
        artifact_paths: list[pathlib.Path] = []
        for lane, image_receipt in (("exact", exact_receipt), ("proxy", proxy_receipt)):
            lane_receipt = require_object(benchmark_pair, lane, f"{context}.benchmark_artifacts")
            run_id = require_string(lane_receipt, "run_id", f"{context}.benchmark_artifacts.{lane}")
            if run_id in run_ids:
                reject(f"canonical benchmark run_id {run_id!r} is reused")
            run_ids.add(run_id)
            bench_manifest, bench_frame, artifact_path = validate_benchmark_lane(
                lane_receipt,
                lane,
                raw,
                presentations[lane],
                image_receipt,
                endpoint,
                authority,
                cut,
                artifact_root,
                balanced,
                benchmark,
                context,
            )
            benchmark_manifests.append(bench_manifest)
            benchmark_frames[lane] = bench_frame
            artifact_paths.append(artifact_path)
        if artifact_paths[0] == artifact_paths[1]:
            reject(f"{context} Exact/proxy canonical artifacts must be distinct")
        build_keys = ("repository_commit", "dirty", "profile", "package_version")
        if {
            key: benchmark_manifests[0]["build"].get(key) for key in build_keys
        } != {
            key: benchmark_manifests[1]["build"].get(key) for key in build_keys
        }:
            reject(f"{context} Exact/proxy build and profile identity mismatch")
        if benchmark_frames["proxy"].get("global_plan") != presented_cut["global_plan"]:
            reject(f"{context} proxy canonical global plan mismatch")
        if benchmark_frames["exact"].get("global_plan") != presented_cut["global_plan"]:
            reject(f"{context} Exact/proxy canonical global plan mismatch")
        if any(
            benchmark_manifest["renderer"].get("backend") != endpoint.backend
            or benchmark_manifest["renderer"].get("exact_plan_actual")
            != presented_cut["global_plan"]
            for benchmark_manifest in benchmark_manifests
        ):
            reject(f"{context} canonical backend/global plan does not match the endpoint receipt")
        benchmark_proxy_vcd = (
            benchmark_frames["proxy"]["visible"],
            benchmark_frames["proxy"]["contributor"],
            benchmark_frames["proxy"]["drawn"],
            benchmark_frames["proxy"]["exact_contributor_compaction"],
        )
        if benchmark_proxy_vcd != presented_vcd:
            reject(f"{context} proxy canonical V/C/D does not join presented-cut receipt")

        actual_metrics = balanced.compute_frame_metrics(exact_image, proxy_image)
        metrics = require_object(raw, "metrics", context)
        for metric, (kind, limit) in FRAME_METRIC_LIMITS.items():
            declared = require_number(metrics, metric, f"{context}.metrics")
            actual = actual_metrics[metric]
            if not math.isclose(declared, actual, rel_tol=0.0, abs_tol=METRIC_RECEIPT_TOLERANCE):
                reject(f"{context}.metrics.{metric} does not match recomputed RGBA8 bytes")
            if kind == "minimum" and actual < limit:
                reject(f"{context}.{metric} is below the S1 v1 image gate")
            if kind == "maximum" and actual > limit:
                reject(f"{context}.{metric} exceeds the S1 v1 image gate")
        frames[key] = balanced.FramePixels(
            capture_index=capture_index,
            trace_frame_index=trace_frame_index,
            exact=exact_image,
            candidate=proxy_image,
        )
    expected = expected_comparison_keys()
    if set(frames) != expected:
        missing = len(expected - set(frames))
        extra = len(set(frames) - expected)
        reject(f"formal capture matrix is incomplete: missing={missing}, extra={extra}")
    return frames


def expected_transition_keys() -> set[tuple[str, str, str, str, int, int]]:
    expected: set[tuple[str, str, str, str, int, int]] = set()
    for endpoint_id in REQUIRED_ENDPOINTS:
        for order in REQUIRED_ORDERS:
            for cut in REQUIRED_CUTS:
                expected.add((endpoint_id, order, cut, "moving_sequence", 0, 1))
                expected.add((endpoint_id, order, cut, "moving_sequence", 1, 2))
            expected.add((endpoint_id, order, "bootstrap_roots", "replacement_sequence", 0, 1))
            expected.add((endpoint_id, order, "mixed_depth_two_replacements", "replacement_sequence", 1, 2))
    return expected


def validate_transitions(manifest: dict[str, Any], frames: dict[Any, Any], balanced: Any) -> int:
    raw_transitions = require_array(manifest, "transitions", "manifest")
    seen: set[tuple[str, str, str, str, int, int]] = set()
    replacement_cuts = (
        "bootstrap_roots",
        "mixed_depth_two_replacements",
        "complete_leaf_exact",
    )
    for index, raw in enumerate(raw_transitions):
        context = f"manifest.transitions[{index}]"
        if not isinstance(raw, dict):
            reject(f"{context} must be an object")
        endpoint_id = require_string(raw, "endpoint_id", context)
        order = require_string(raw, "order_backend", context)
        cut_name = require_string(raw, "cut_name", context)
        sequence = require_string(raw, "sequence", context)
        from_index = require_int(raw, "from_capture_index", context)
        to_index = require_int(raw, "to_capture_index", context)
        if to_index != from_index + 1:
            reject(f"{context} must join adjacent captures")
        transition_key = (endpoint_id, order, cut_name, sequence, from_index, to_index)
        if transition_key in seen:
            reject(f"duplicate transition identity {transition_key}")
        seen.add(transition_key)
        if sequence == "moving_sequence":
            previous_key = (endpoint_id, order, cut_name, sequence, from_index)
            current_key = (endpoint_id, order, cut_name, sequence, to_index)
        elif sequence == "replacement_sequence":
            if from_index >= 2 or cut_name != replacement_cuts[from_index]:
                reject(f"{context} does not identify a frozen replacement transition")
            previous_key = (endpoint_id, order, replacement_cuts[from_index], sequence, from_index)
            current_key = (endpoint_id, order, replacement_cuts[to_index], sequence, to_index)
        else:
            reject(f"{context}.sequence must be moving_sequence or replacement_sequence")
        if previous_key not in frames or current_key not in frames:
            reject(f"{context} references an unavailable comparison")
        actual = balanced.compute_temporal_metric(frames[previous_key], frames[current_key])
        metrics = require_object(raw, "metrics", context)
        declared = require_number(metrics, TEMPORAL_METRIC, f"{context}.metrics")
        if not math.isclose(declared, actual, rel_tol=0.0, abs_tol=METRIC_RECEIPT_TOLERANCE):
            reject(f"{context}.{TEMPORAL_METRIC} does not match retained RGBA8 bytes")
        if actual > TEMPORAL_LIMIT:
            reject(f"{context}.{TEMPORAL_METRIC} exceeds the S1 v1 temporal gate")
    expected = expected_transition_keys()
    if seen != expected:
        reject(
            "formal transition matrix is incomplete: "
            f"missing={len(expected - seen)}, extra={len(seen - expected)}"
        )
    return len(seen)


def validate(path: pathlib.Path) -> ValidationResult:
    manifest_path = path.resolve()
    balanced = load_module("scalable_proxy_balanced_validator", BALANCED_VALIDATOR_PATH)
    benchmark = load_module("scalable_proxy_benchmark_validator", BENCHMARK_VALIDATOR_PATH)
    try:
        manifest = balanced.load_json(manifest_path)
    except balanced.ValidationError as error:
        reject(f"cannot read gate manifest: {error}")
    if manifest.get("schema") != SCHEMA:
        reject(f"manifest.schema must equal {SCHEMA!r}")
    evidence_class = require_string(manifest, "evidence_class", "manifest")
    if evidence_class not in {"contract_fixture", "formal_quality"}:
        reject("manifest.evidence_class must be contract_fixture or formal_quality")
    contract = require_object(manifest, "contract", "manifest")
    if contract != {
        "name": SCHEMA,
        "aggregation": "logical_all",
        "required_cuts": list(REQUIRED_CUTS),
        "required_order_backends": list(REQUIRED_ORDERS),
        "frame_metric_limits": {
            key: list(value) for key, value in FRAME_METRIC_LIMITS.items()
        },
        "temporal_metric": {"name": TEMPORAL_METRIC, "maximum": TEMPORAL_LIMIT},
    }:
        reject("manifest.contract must reproduce the frozen S1 v1 gate exactly")
    validate_dependencies(manifest)
    authority = validate_authority(
        manifest, manifest_path.parent, evidence_class, balanced
    )
    exact_reference_sha256 = validate_exact_reference(manifest, authority, balanced)
    cuts = validate_cuts(manifest, authority)
    endpoints = validate_endpoints(
        manifest, manifest_path.parent, evidence_class, authority, balanced
    )
    comparisons = validate_comparisons(
        manifest,
        manifest_path.parent,
        authority,
        cuts,
        endpoints,
        exact_reference_sha256,
        balanced,
        benchmark,
    )
    transition_count = validate_transitions(manifest, comparisons, balanced)
    return ValidationResult(
        evidence_class=evidence_class,
        endpoint_count=len(endpoints),
        comparison_count=len(comparisons),
        transition_count=transition_count,
        validator_sha256=sha256_file(pathlib.Path(__file__).resolve()),
    )


def decision_receipt(decision: str, passed: bool, **extra: Any) -> str:
    receipt = {
        "schema": SCHEMA,
        "decision": decision,
        "pass": passed,
        "validator": {
            "version": VALIDATOR_VERSION,
            "sha256": sha256_file(pathlib.Path(__file__).resolve()),
            "balanced_image_gate_sha256": BALANCED_VALIDATOR_SHA256,
            "benchmark_artifact_validator_sha256": BENCHMARK_VALIDATOR_SHA256,
        },
        **extra,
    }
    return json.dumps(receipt, sort_keys=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=pathlib.Path, nargs="?")
    parser.add_argument(
        "--preflight-formal",
        action="store_true",
        help=(
            "check the frozen local Bonsai/camera/trace prerequisites and emit "
            "a finite Deferred receipt without fabricating missing endpoint evidence"
        ),
    )
    args = parser.parse_args(argv)
    if args.preflight_formal:
        if args.manifest is not None:
            parser.error("--preflight-formal does not accept an evidence manifest")
        try:
            print(json.dumps(formal_collection_preflight(), sort_keys=True))
        except ValidationError as error:
            print(
                json.dumps(
                    {
                        "schema": PREFLIGHT_SCHEMA,
                        "decision": "Rejected",
                        "pass": False,
                        "scope": "formal_quality_collection_preflight",
                        "reason": str(error),
                        "s2_s5_unlocked": False,
                    },
                    sort_keys=True,
                )
            )
            return 1
        return 2
    if args.manifest is None:
        parser.error("manifest is required unless --preflight-formal is used")
    try:
        result = validate(args.manifest)
    except DeferredEvidence as error:
        print(decision_receipt("Deferred", False, reason=str(error)))
        return 2
    except ValidationError as error:
        print(decision_receipt("Rejected", False, reason=str(error)))
        return 1
    decision = "ValidatedFixture" if result.evidence_class == "contract_fixture" else "Accepted"
    print(
        decision_receipt(
            decision,
            True,
            evidence_class=result.evidence_class,
            endpoint_count=result.endpoint_count,
            comparison_count=result.comparison_count,
            transition_count=result.transition_count,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
