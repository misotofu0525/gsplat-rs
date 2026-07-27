#!/usr/bin/env python3
"""Collect one fail-closed Apple M4 S1 materialized-cut endpoint bundle.

This collector is an evidence bridge, not a Scalable runtime.  It loads the
complete Bonsai PLY for every Exact lane and loads only the render input named
by the retained S1a cut receipt for the compared lane.  Raw desktop host
outputs are immutable and retain their input-local point count.  The outer S1
bundle joins those renderer-local P/V/C/D receipts to the independently
authored S/R coverage receipt without rewriting a raw artifact.

The command never retries a build or host invocation.  A successful result is
an endpoint-scoped ``Captured`` bundle; it is not aggregate S1 acceptance.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import re
import shutil
import subprocess
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Sequence


REPO_ROOT = Path(__file__).resolve().parents[2]
BALANCED_COLLECTOR_PATH = REPO_ROOT / "tests/perf/collect-balanced-desktop-b1.py"
S1_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-scalable-proxy-image-gate.py"
BENCHMARK_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"


def load_module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


DESKTOP = load_module("collect_scalable_s1_m4_desktop", BALANCED_COLLECTOR_PATH)
S1 = load_module("collect_scalable_s1_m4_validator", S1_VALIDATOR_PATH)
M2B = DESKTOP.M2B
IMAGE = DESKTOP.BALANCED

SCHEMA = "gsplat-scalable-s1-endpoint-capture/v1"
ENDPOINT_ID = "apple_m4_metal"
FORMAL_SIZE = (1920, 1080)
FORMAL_TRACE_PATH = Path(S1.FORMAL_ENDPOINTS[ENDPOINT_ID]["trace_path"])
FORMAL_DATASET_MANIFEST = Path("tests/perf/datasets/bonsai.local-candidate.json")
FORMAL_CAMERA_REVIEW = Path(
    "docs/plans/active/2026-07-23-native-render-scalable/"
    "s1-bonsai-camera-review.json"
)
CANONICAL_WARMUP = 20
CANONICAL_MEASURED = 80
CAPTURE_TRACE_FRAMES = (0, 1, 0)
CAPTURE_TRACE_TIMESTAMPS_NS = (0, 16_666_667, 0)
RUN_TIMEOUT_SECONDS = 30 * 60
COUNT_SEMANTICS = "candidate_visible_contributor_issued_v1"
RAW_COUNT_SEMANTICS = {
    "cpu": "direct_draw_equals_visible",
    "gpu": "indirect_draw_equals_visible",
}
ORDER_SPEC = {
    "cpu": {
        "requested": "cpu_post_sort",
        "cli": "cpu-post-sort",
        "current": "cpu_post_sort",
        "capture": "CpuPostSort",
    },
    "gpu": {
        "requested": "gpu_post_sort",
        "cli": "gpu-post-sort",
        "current": "gpu_post_sort",
        "capture": "GpuPostSort",
    },
}
SESSION_SELECTIONS = {
    "authored_views": ((0, 0, 0), (1, 1, 1)),
    "moving_sequence": ((0, 0, 0), (1, 1, 1), (2, 2, 0)),
    # Each cut owns a separate post-warmup view-0 capture for the replacement
    # sequence.  Its formal capture index is assigned from the frozen cut order.
    "replacement_sequence": ((0, 0, 0),),
}
REPLACEMENT_CUTS = (
    "bootstrap_roots",
    "mixed_depth_two_replacements",
    "complete_leaf_exact",
)
PREFIXES = DESKTOP.PREFIXES
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
M4_RE = re.compile(r"(?:^|[^A-Za-z0-9])M4(?:[^A-Za-z0-9]|$)", re.IGNORECASE)


class ValidationError(ValueError):
    """Evidence exists but is invalid or incomplete: finite Rejected."""


class DeferredEvidence(ValidationError):
    """A named prerequisite or the required M4 endpoint is unavailable."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def defer(condition: bool, message: str) -> None:
    if not condition:
        raise DeferredEvidence(message)


def only(records: Sequence[dict[str, str]], context: str) -> dict[str, str]:
    require(len(records) == 1, f"expected exactly one {context}, got {len(records)}")
    return records[0]


def require_keys(record: dict[str, str], keys: Sequence[str], context: str) -> None:
    missing = sorted(set(keys) - record.keys())
    require(not missing, f"{context} is missing fields: {', '.join(missing)}")


def assert_fields(record: dict[str, str], expected: dict[str, str], context: str) -> None:
    require_keys(record, tuple(expected), context)
    for key, value in expected.items():
        require(
            record[key] == value,
            f"{context}.{key}: expected {value!r}, got {record[key]!r}",
        )


def read_json(path: Path, context: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"cannot read {context}: {error}") from error
    require(isinstance(value, dict), f"{context} must be an object")
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        raise ValidationError(f"cannot hash {path}: {error}") from error
    return digest.hexdigest()


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode(
            "utf-8"
        )
    ).hexdigest()


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def confined_file(root: Path, value: str, context: str) -> Path:
    relative = PurePosixPath(value)
    require(
        not relative.is_absolute() and relative.parts and ".." not in relative.parts,
        f"{context} must stay inside {root}",
    )
    try:
        resolved = root.joinpath(*relative.parts).resolve(strict=True)
        resolved.relative_to(root.resolve())
    except (OSError, ValueError) as error:
        raise ValidationError(f"{context} escapes or is unavailable below {root}") from error
    require(resolved.is_file() and not resolved.is_symlink(), f"{context} must be a regular file")
    return resolved


@dataclass(frozen=True)
class CutInput:
    name: str
    coverage: dict[str, Any]
    coverage_sha256: str
    render_input: dict[str, Any]
    path: Path
    active_splats: int


@dataclass(frozen=True)
class CutAuthority:
    receipt_path: Path
    receipt_sha256: str
    receipt: dict[str, Any]
    source: dict[str, Any]
    source_path: Path
    hierarchy_manifest_path: Path
    hierarchy_manifest_sha256: str
    cuts: dict[str, CutInput]


def validate_render_input(
    *,
    repo: Path,
    receipt_root: Path,
    source: dict[str, Any],
    cut_document: dict[str, Any],
) -> CutInput:
    name = cut_document.get("name")
    require(name in S1.REQUIRED_CUTS, f"unknown S1 cut {name!r}")
    coverage = cut_document.get("coverage")
    render_input = cut_document.get("render_input")
    require(isinstance(coverage, dict), f"cut {name} coverage must be an object")
    require(isinstance(render_input, dict), f"cut {name} render_input must be an object")
    coverage_sha256 = cut_document.get("coverage_sha256")
    require(
        isinstance(coverage_sha256, str)
        and SHA256_RE.fullmatch(coverage_sha256) is not None
        and coverage_sha256 == canonical_sha256(coverage),
        f"cut {name} coverage SHA-256 mismatch",
    )
    expected_bindings = {
        "schema": "gsplat-formal-s1-cut-render-input/v1",
        "cut_name": name,
        "source_sha256": source["sha256"],
        "hierarchy_manifest_sha256": coverage["hierarchy_manifest_sha256"],
        "ordered_node_ids": coverage["ordered_node_ids"],
        "ordered_node_list_sha256": coverage["ordered_node_list_sha256"],
        "page_sha256": coverage["page_sha256"],
        "page_list_sha256": coverage["page_list_sha256"],
        "P": coverage["active_proxy_splats"],
        "sh_degree": 3,
        "sampling": "disabled",
    }
    for key, expected in expected_bindings.items():
        require(render_input.get(key) == expected, f"cut {name} render_input.{key} mismatch")
    active = coverage["active_proxy_splats"]
    require(isinstance(active, int) and not isinstance(active, bool) and active > 0, f"cut {name} P invalid")

    kind = render_input.get("kind")
    if name == "complete_leaf_exact":
        require(kind == "content_addressed_source_ply_alias", "complete leaf must alias source")
        require(render_input.get("copied_into_package") is False, "complete source must not be copied")
        require(render_input.get("logical_path") == source["logical_path"], "source alias path drift")
        path = (repo / source["logical_path"]).resolve()
    else:
        require(
            kind == "materialized_binary_little_endian_sh3_ply",
            f"proxy cut {name} must use the S1a materialized PLY",
        )
        path = confined_file(receipt_root, str(render_input.get("path", "")), f"cut {name} PLY")

    defer(path.is_file(), f"render input is unavailable for cut {name}: {path}")
    require(path.stat().st_size == render_input.get("bytes"), f"cut {name} PLY byte count mismatch")
    require(sha256_file(path) == render_input.get("sha256"), f"cut {name} PLY SHA-256 mismatch")
    count, degree = S1.read_ply_header_identity(path)
    require((count, degree) == (active, 3), f"cut {name} PLY must contain P complete-SH3 splats")
    return CutInput(name, coverage, coverage_sha256, render_input, path, active)


def read_cut_authority(
    repo: Path,
    receipt_path: Path,
    *,
    formal_source: dict[str, Any] | None = None,
) -> CutAuthority:
    expected_source = formal_source or {
        "dataset_id": S1.FORMAL_SOURCE["dataset_id"],
        "logical_path": S1.FORMAL_SOURCE["local_path"],
        "sha256": S1.FORMAL_SOURCE["sha256"],
        "bytes": S1.FORMAL_SOURCE["bytes"],
        "splat_count": S1.FORMAL_SOURCE["splat_count"],
        "sh_degree": S1.FORMAL_SOURCE["sh_degree"],
        "sampling": "disabled",
    }
    receipt_path = receipt_path.resolve()
    defer(receipt_path.is_file(), f"S1a cut receipt is unavailable: {receipt_path}")
    receipt = read_json(receipt_path, "S1a cut receipt")
    require(receipt.get("schema") == "gsplat-formal-s1-proxy-authoring/v1", "S1a receipt schema mismatch")
    require(receipt.get("authoring_status") == "complete", "S1a authoring is incomplete")
    require(receipt.get("scope") == "offline_hierarchy_authoring_with_s1_cut_render_inputs", "S1a scope mismatch")
    require(receipt.get("endpoint_image_gate") == "not_run", "S1a receipt already claims an endpoint gate")
    require(receipt.get("s2_s5_unlocked") is False, "S1a receipt illegally unlocks S2-S5")
    authority = receipt.get("authority")
    require(isinstance(authority, dict) and authority.get("source") == expected_source, "S1a source authority mismatch")
    source = authority["source"]
    source_path = (repo / source["logical_path"]).resolve()
    defer(source_path.is_file(), f"complete Bonsai source is unavailable: {source_path}")
    require(source_path.stat().st_size == source["bytes"], "complete Bonsai source byte count mismatch")
    require(sha256_file(source_path) == source["sha256"], "complete Bonsai source SHA-256 mismatch")
    source_count, source_degree = S1.read_ply_header_identity(source_path)
    require((source_count, source_degree) == (source["splat_count"], 3), "complete Bonsai source identity mismatch")

    hierarchy = receipt.get("hierarchy")
    require(isinstance(hierarchy, dict) and isinstance(hierarchy.get("manifest"), dict), "S1a hierarchy manifest receipt missing")
    hierarchy_receipt = hierarchy["manifest"]
    hierarchy_path = confined_file(receipt_path.parent, str(hierarchy_receipt.get("path", "")), "hierarchy manifest")
    hierarchy_sha = hierarchy_receipt.get("sha256")
    require(isinstance(hierarchy_sha, str) and SHA256_RE.fullmatch(hierarchy_sha) is not None, "hierarchy SHA-256 invalid")
    require(sha256_file(hierarchy_path) == hierarchy_sha, "hierarchy manifest SHA-256 mismatch")
    require(hierarchy_path.stat().st_size == hierarchy_receipt.get("bytes"), "hierarchy manifest byte count mismatch")

    builder = authority.get("builder")
    require(isinstance(builder, dict), "S1a builder authority missing")
    builder_commit = builder.get("repository_commit")
    require(isinstance(builder_commit, str) and re.fullmatch(r"[0-9a-f]{40}", builder_commit) is not None, "S1a builder commit invalid")
    builder_config = builder.get("configuration_sha256")
    require(isinstance(builder_config, str) and SHA256_RE.fullmatch(builder_config) is not None, "S1a builder configuration hash invalid")

    s1_authority = S1.Authority(
        dataset_id=source["dataset_id"],
        source_sha256=source["sha256"],
        source_bytes=source["bytes"],
        source_splats=source["splat_count"],
        source_sh_degree=source["sh_degree"],
        camera_metadata_sha256="0" * 64,
        camera_review_traces={},
        hierarchy_manifest_sha256=hierarchy_sha,
        builder_commit=builder_commit,
        builder_configuration_sha256=builder_config,
    )
    S1.validate_cuts(receipt, s1_authority)
    raw_cuts = receipt["cuts"]
    cuts = {
        str(raw["name"]): validate_render_input(
            repo=repo,
            receipt_root=receipt_path.parent,
            source=source,
            cut_document=raw,
        )
        for raw in raw_cuts
    }
    require(set(cuts) == set(S1.REQUIRED_CUTS), "S1a receipt does not contain all frozen cuts")
    return CutAuthority(
        receipt_path=receipt_path,
        receipt_sha256=sha256_file(receipt_path),
        receipt=receipt,
        source=source,
        source_path=source_path,
        hierarchy_manifest_path=hierarchy_path,
        hierarchy_manifest_sha256=hierarchy_sha,
        cuts=cuts,
    )


@dataclass(frozen=True)
class RawCapture:
    raw_capture_index: int
    trace_frame_index: int
    trace_timestamp_ns: int
    elapsed_ns: int
    call_ms: float
    frame_wall_ms: float
    path: Path
    png_sha256: str
    rgba8_sha256: str
    source_count: int
    visible: int
    contributor: int
    drawn: int
    renderer_ticket: int
    identity: dict[str, int]
    raster_generation: int
    encode_attempt: int


@dataclass(frozen=True)
class RawSession:
    order: str
    input_count: int
    captures: tuple[RawCapture, ...]
    adapter: dict[str, str]
    raw_directory: Path
    raw_sha256: str
    started_at_utc: str
    ended_at_utc: str


def parse_uint(record: dict[str, str], key: str, context: str, *, positive: bool = False) -> int:
    value = record.get(key, "")
    require(re.fullmatch(r"[0-9]+", value) is not None, f"{context}.{key} must be an integer")
    result = int(value)
    if positive:
        require(result > 0, f"{context}.{key} must be positive")
    return result


def parse_float(record: dict[str, str], key: str, context: str) -> float:
    try:
        value = float(record.get(key, ""))
    except ValueError as error:
        raise ValidationError(f"{context}.{key} must be numeric") from error
    require(math.isfinite(value) and value >= 0.0, f"{context}.{key} must be finite and non-negative")
    return value


def validate_raw_begin(
    record: dict[str, str],
    *,
    order: str,
    input_count: int,
    trace: dict[str, Any],
    size: tuple[int, int],
    warmup: int,
    measured: int,
    require_m4: bool,
) -> dict[str, str]:
    spec = ORDER_SPEC[order]
    width, height = size
    expected = {
        "trace_id": trace["trace_id"],
        "trace_sha256": trace["content_sha256"],
        "exact_plan_requested": spec["requested"],
        "geometry_path": "packed_atlas",
        "raster_execution_plan": "projected_quads_exact",
        "blend_mode": "sorted_alpha",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "adapter_backend": "metal",
        "source_count": str(input_count),
        "decoded_count": str(input_count),
        "encoded_count": str(input_count),
        "resident_count": str(input_count),
        "addressable_count": str(input_count),
        "sh_degree": "3",
        "requested_width": str(width),
        "requested_height": str(height),
        "surface_width": str(width),
        "surface_height": str(height),
        "internal_render_width": str(width),
        "internal_render_height": str(height),
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
        "full_resolution": "true",
        "trace_frames": str(warmup + measured),
    }
    assert_fields(record, expected, "begin")
    require(record.get("__stream") == "stdout", "begin receipt must be emitted on stdout")
    adapter_name = record.get("adapter_name", "")
    defer(adapter_name not in ("", "unavailable"), "Metal adapter name is unavailable")
    if require_m4:
        defer(M4_RE.search(adapter_name) is not None, f"required Apple M4 adapter is unavailable: {adapter_name!r}")
    device_type = record.get("adapter_device_type", "")
    defer(device_type not in ("", "other"), "Metal adapter device type is unavailable")
    require_keys(record, ("adapter_driver", "adapter_driver_info"), "begin")
    return {
        "backend": "metal",
        "name": adapter_name,
        "device_type": device_type,
        "driver": record["adapter_driver"],
        "driver_info": record["adapter_driver_info"],
    }


def validate_raw_terminals(
    records: Sequence[dict[str, str]],
    *,
    raw_directory: Path,
    order: str,
    input_count: int,
    size: tuple[int, int],
) -> tuple[RawCapture, ...]:
    require(len(records) == 3, f"expected three 0-1-0 terminal captures, got {len(records)}")
    spec = ORDER_SPEC[order]
    width, height = size
    captures: list[RawCapture] = []
    previous_elapsed = -1
    previous_ticket = 0
    previous_presentation = 0
    for index, (record, trace_frame, timestamp) in enumerate(
        zip(records, CAPTURE_TRACE_FRAMES, CAPTURE_TRACE_TIMESTAMPS_NS, strict=True)
    ):
        context = f"terminal[{index}]"
        expected_path = PurePosixPath(
            "capture.captures", f"capture-{index}-trace-{trace_frame}.png"
        ).as_posix()
        expected = {
            "status": "ok",
            "capture_index": str(index),
            "path": expected_path,
            "trace_frame": str(trace_frame),
            "trace_timestamp_ns": str(timestamp),
            "current_stats_plan_id": spec["current"],
            "count_semantics": RAW_COUNT_SEMANTICS[order],
            "source_count": str(input_count),
            "exact_contributor_compaction": "false",
            "capture_receipt_plan_id": spec["capture"],
            "capture_receipt_width": str(width),
            "capture_receipt_height": str(height),
            "capture_receipt_resident_sh_source_count": str(input_count),
            "capture_receipt_resident_sh_encoded_count": str(input_count),
            "capture_receipt_resident_sh_resident_count": str(input_count),
            "capture_receipt_resident_sh_addressable_count": str(input_count),
            "capture_receipt_resident_sh_source_degree": "3",
            "capture_receipt_resident_sh_resident_degree": "3",
            "frame_presented": "true",
            "terminal_receipt": "ready",
        }
        assert_fields(record, expected, context)
        require(record.get("__stream") == "stdout", f"{context} must be emitted on stdout")
        elapsed = parse_uint(record, "elapsed_ns", context, positive=True)
        require(elapsed > previous_elapsed, f"{context} elapsed time is stale")
        previous_elapsed = elapsed
        ticket = parse_uint(record, "current_stats_ticket", context, positive=True)
        presentation = parse_uint(
            record, "current_stats_presentation_sequence", context, positive=True
        )
        require(ticket > previous_ticket, f"{context} current-stats ticket is stale or reused")
        require(presentation > previous_presentation, f"{context} presentation sequence is stale or reused")
        previous_ticket = ticket
        previous_presentation = presentation
        current = {
            "scene_generation": parse_uint(record, "current_stats_scene_generation", context, positive=True),
            "camera_revision": parse_uint(record, "current_stats_camera_revision", context, positive=True),
            "viewport_generation": parse_uint(record, "current_stats_viewport_generation", context),
            "contract_generation": parse_uint(record, "current_stats_contract_generation", context, positive=True),
            "plan_set_generation": parse_uint(record, "current_stats_plan_set_generation", context, positive=True),
            "order_generation": parse_uint(record, "current_stats_order_generation", context, positive=True),
            "presentation_sequence": presentation,
        }
        captured = {
            "scene_generation": parse_uint(record, "capture_receipt_scene_generation", context, positive=True),
            "camera_revision": parse_uint(record, "capture_receipt_camera_revision", context, positive=True),
            "viewport_generation": parse_uint(record, "capture_receipt_viewport_generation", context),
            "contract_generation": parse_uint(record, "capture_receipt_contract_generation", context, positive=True),
            "plan_set_generation": parse_uint(record, "capture_receipt_plan_set_generation", context, positive=True),
            "order_generation": parse_uint(record, "capture_receipt_order_generation", context, positive=True),
            "presentation_sequence": parse_uint(record, "capture_receipt_presentation_sequence", context, positive=True),
        }
        require(current == captured, f"{context} PNG/current-stats/capture receipt identity mismatch")
        visible = parse_uint(record, "visible_count", context)
        contributor = parse_uint(record, "contributor_count", context)
        drawn = parse_uint(record, "drawn_count", context)
        require(0 <= contributor <= visible <= input_count, f"{context} violates 0<=C<=V<=P")
        require(drawn == visible, f"{context} PostSort requires D=V")
        rgba8_sha = record.get("capture_receipt_rgba8_sha256", "")
        require(SHA256_RE.fullmatch(rgba8_sha) is not None, f"{context} RGBA8 SHA-256 invalid")
        path = DESKTOP.resolve_capture_file(raw_directory, expected_path, context)
        data = path.read_bytes()
        decoded = IMAGE.decode_rgba8_png(data, context, size)
        require(
            hashlib.sha256(decoded.rgba).hexdigest() == rgba8_sha,
            f"{context} PNG bytes do not match the atomic capture receipt",
        )
        captures.append(
            RawCapture(
                raw_capture_index=index,
                trace_frame_index=trace_frame,
                trace_timestamp_ns=timestamp,
                elapsed_ns=elapsed,
                call_ms=parse_float(record, "call_ms", context),
                frame_wall_ms=parse_float(record, "frame_wall_ms", context),
                path=path,
                png_sha256=hashlib.sha256(data).hexdigest(),
                rgba8_sha256=rgba8_sha,
                source_count=input_count,
                visible=visible,
                contributor=contributor,
                drawn=drawn,
                renderer_ticket=ticket,
                identity=current,
                raster_generation=parse_uint(record, "current_stats_raster_generation", context, positive=True),
                encode_attempt=parse_uint(record, "current_stats_encode_attempt", context, positive=True),
            )
        )
    return tuple(captures)


def validate_raw_summary(record: dict[str, str], *, order: str, warmup: int, measured: int) -> None:
    assert_fields(
        record,
        {
            "status": "ok",
            "exact_plan_requested": ORDER_SPEC[order]["requested"],
            "actual_plan_set": ORDER_SPEC[order]["current"],
            "trace_frames": str(warmup + measured),
            "measured_frames": str(measured),
            "terminal_receipts": str(warmup + measured + 3),
            "final_capture": "available",
        },
        "summary",
    )
    require(record.get("__stream") == "stdout", "summary receipt must be emitted on stdout")


def validate_raw_session(
    stdout: str,
    stderr: str,
    *,
    raw_directory: Path,
    order: str,
    input_count: int,
    trace: dict[str, Any],
    size: tuple[int, int] = FORMAL_SIZE,
    warmup: int = CANONICAL_WARMUP,
    measured: int = CANONICAL_MEASURED,
    require_m4: bool = True,
    started_at_utc: str = "unknown",
    ended_at_utc: str = "unknown",
) -> RawSession:
    require(order in ORDER_SPEC, f"unsupported order lane {order!r}")
    records = DESKTOP.parse_session_records(stdout, stderr)
    begin = only(records["begin"], "begin receipt")
    summary = only(records["summary"], "summary receipt")
    adapter = validate_raw_begin(
        begin,
        order=order,
        input_count=input_count,
        trace=trace,
        size=size,
        warmup=warmup,
        measured=measured,
        require_m4=require_m4,
    )
    captures = validate_raw_terminals(
        records["terminal"],
        raw_directory=raw_directory,
        order=order,
        input_count=input_count,
        size=size,
    )
    validate_raw_summary(summary, order=order, warmup=warmup, measured=measured)
    return RawSession(
        order=order,
        input_count=input_count,
        captures=captures,
        adapter=adapter,
        raw_directory=raw_directory,
        raw_sha256=IMAGE.artifact_directory_sha256(raw_directory),
        started_at_utc=started_at_utc,
        ended_at_utc=ended_at_utc,
    )


def make_command(binary: Path, input_path: Path, trace_path: Path, order: str) -> list[str]:
    return [
        str(binary),
        str(input_path),
        "--geometry-path",
        "packed",
        "--interactive",
        "--camera-trace",
        str(trace_path),
        "--camera-sequence",
        "--camera-warmup-frames",
        str(CANONICAL_WARMUP),
        "--camera-measured-frames",
        str(CANONICAL_MEASURED),
        "--camera-loops",
        "1",
        "--surface-benchmark-mode",
        "isolated",
        "--surface-sort-policy",
        "every-frame",
        "--surface-evidence-plan",
        ORDER_SPEC[order]["cli"],
        "--surface-diagnostic-capture-receipt",
        "--surface-diagnostic-multi-capture",
        "--png",
        "capture.png",
    ]


def build_binary(repo: Path, stage: Path, expected_git: dict[str, Any]) -> tuple[Path, dict[str, Any]]:
    target = stage / "cargo-target"
    target.mkdir()
    build = stage / "build"
    build.mkdir()
    command = [
        "cargo",
        "build",
        "--locked",
        "--release",
        "-p",
        "desktop-example",
        "--features",
        "diagnostic-surface-capture-receipt",
        "--message-format=json-render-diagnostics",
    ]
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target.resolve())
    write_json(build / "command.json", {"argv": command, "environment": {"CARGO_TARGET_DIR": environment["CARGO_TARGET_DIR"]}})
    completed = subprocess.run(
        command,
        cwd=repo,
        env=environment,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    (build / "stdout.log").write_text(completed.stdout, encoding="utf-8")
    (build / "stderr.log").write_text(completed.stderr, encoding="utf-8")
    require(completed.returncode == 0, f"desktop diagnostic build exited with {completed.returncode}")
    binary = DESKTOP.cargo_executable(completed.stdout, target)
    retained = build / "desktop-example-bin"
    shutil.copy2(binary, retained)
    retained.chmod(retained.stat().st_mode | 0o100)
    require(M2B.git_receipt(repo) == expected_git, "Git receipt changed during build")
    return retained, {
        "binary_sha256": sha256_file(retained),
        "feature": "diagnostic-surface-capture-receipt",
        "command": "build/command.json",
        "stdout": "build/stdout.log",
        "stderr": "build/stderr.log",
    }


HostInvoker = Callable[[Sequence[str], Path], subprocess.CompletedProcess[str]]


def default_host_invoker(command: Sequence[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(command),
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=RUN_TIMEOUT_SECONDS,
    )


def run_raw_session(
    *,
    stage: Path,
    binary: Path,
    binary_sha256: str,
    input_path: Path,
    input_count: int,
    trace_path: Path,
    trace: dict[str, Any],
    order: str,
    cut_name: str,
    lane: str,
    session_name: str,
    invoke: HostInvoker = default_host_invoker,
) -> RawSession:
    directory = stage / "raw" / order / cut_name / lane / session_name
    directory.mkdir(parents=True)
    command = make_command(binary, input_path, trace_path, order)
    write_json(directory / "command.json", {"argv": command, "cwd": directory.relative_to(stage).as_posix(), "attempt": 1, "auto_retry": False})
    require(sha256_file(binary) == binary_sha256, "collector-built binary changed before invocation")
    started = M2B.utc_now()
    completed = invoke(command, directory)
    ended = M2B.utc_now()
    (directory / "stdout.log").write_text(completed.stdout, encoding="utf-8")
    (directory / "stderr.log").write_text(completed.stderr, encoding="utf-8")
    require(completed.returncode == 0, f"{order}/{cut_name}/{lane}/{session_name} host exited with {completed.returncode}")
    require(sha256_file(binary) == binary_sha256, "collector-built binary changed after invocation")
    return validate_raw_session(
        completed.stdout,
        completed.stderr,
        raw_directory=directory,
        order=order,
        input_count=input_count,
        trace=trace,
        started_at_utc=started,
        ended_at_utc=ended,
    )


def camera_receipt(trace: dict[str, Any], trace_frame_index: int) -> dict[str, str]:
    frame = trace["frames"][trace_frame_index]
    return {
        "trace_id": trace["trace_id"],
        "trace_content_sha256": trace["content_sha256"],
        "pose_intrinsics_sha256": canonical_sha256(
            {"pose": frame["pose"], "intrinsics": frame["intrinsics"]}
        ),
    }


def canonical_presentation(capture: RawCapture, session_ordinal: int) -> dict[str, Any]:
    # Renderer tickets are session-local.  Preserve them verbatim and namespace
    # only the endpoint-bundle ticket used to keep cross-process evidence
    # identities unique.  This never rewrites the raw renderer receipt.
    evidence_ticket = (session_ordinal << 32) | capture.renderer_ticket
    identity = capture.identity
    return {
        "ticket": evidence_ticket,
        "renderer_ticket": capture.renderer_ticket,
        "ticket_semantics": "collector_namespaced_renderer_ticket",
        "outcome": "presented",
        "primitive_presented": True,
        "scene_generation": identity["scene_generation"],
        "camera_generation": identity["camera_revision"],
        "viewport_generation": identity["viewport_generation"],
        "contract_generation": identity["contract_generation"],
        "plan_generation": identity["plan_set_generation"],
        "presentation_generation": identity["presentation_sequence"],
    }


def validate_pair(exact: RawCapture, proxy: RawCapture, context: str) -> None:
    require(exact.trace_frame_index == proxy.trace_frame_index, f"{context} trace frame mismatch")
    require(exact.trace_timestamp_ns == proxy.trace_timestamp_ns, f"{context} trace timestamp mismatch")
    require(exact.identity == proxy.identity, f"{context} Exact/proxy lifecycle identity mismatch")


def distribution(value: float) -> dict[str, Any]:
    return {"count": 1, "mean": value, "p50": value, "p90": value, "p95": value, "p99": value, "max": value}


def write_benchmark_artifact(
    directory: Path,
    *,
    lane: str,
    capture: RawCapture,
    presentation: dict[str, Any],
    pair_id: str,
    order: str,
    cut_name: str,
    sequence: str,
    capture_index: int,
    camera: dict[str, str],
    authority: CutAuthority,
    trace: dict[str, Any],
    adapter: dict[str, str],
    build: dict[str, Any],
    raw_session: RawSession,
    stage: Path,
    size: tuple[int, int] = FORMAL_SIZE,
) -> dict[str, Any]:
    directory.mkdir(parents=True)
    image_path = directory / "final-frame.png"
    shutil.copyfile(capture.path, image_path)
    require(sha256_file(image_path) == capture.png_sha256, "raw capture changed during materialization")
    run_id = f"s1-m4-{build['git']['commit'][:12]}-{order}-{cut_name}-{lane}-{sequence}-{capture_index}-{uuid.uuid4().hex[:8]}"
    width, height = size
    frame_budget_ms = 1000.0 / 60.0
    unavailable = [
        "frames[*].preprocess_ms",
        "frames[*].sort_ms",
        "frames[*].geometry_submit_ms",
        "frames[*].gpu_wait_ms",
        "frames[*].gpu_complete_ms",
        "frames[*].sort_refreshed",
        "environment.browser",
    ]
    if adapter["driver"] == "unavailable":
        unavailable.append("environment.driver")
    manifest = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {
            "series_id": "scalable-s1-m4-materialized-cut",
            "started_at_utc": raw_session.started_at_utc,
            "ended_at_utc": raw_session.ended_at_utc,
            "measurement_started_at_utc": raw_session.started_at_utc,
            "measurement_ended_at_utc": raw_session.ended_at_utc,
        },
        "build": {
            "repository_commit": build["git"]["commit"],
            "dirty": False,
            "profile": "release",
            "package_version": build["package_version"],
            "executable_sha256": build["binary_sha256"],
        },
        # S remains the external coverage authority.  The raw renderer input
        # count is retained separately and the frame reports active_splats=P.
        "dataset": {
            "id": authority.source["dataset_id"],
            "sha256": authority.source["sha256"],
            "bytes": authority.source["bytes"],
            "splat_count": authority.source["splat_count"],
            "sh_degree": 3,
        },
        "trace": {"id": trace["trace_id"], "sha256": trace["content_sha256"]},
        "renderer": {
            "implementation": f"gsplat-rs S1 M4 {lane}",
            "path": "packed_atlas",
            "backend": "metal",
            "sort_policy": f"forced_{order}",
            "exact_plan_requested": ORDER_SPEC[order]["requested"],
            "exact_plan_actual": ORDER_SPEC[order]["current"],
            "count_semantics": COUNT_SEMANTICS,
            "raster_execution_plan": "projected_quads_exact",
            "blend_mode": "sorted_alpha",
            "raw_input_splat_count": capture.source_count,
        },
        "display": {
            "width": width,
            "height": height,
            "dpr": 1.0,
            "refresh_hz": 60.0,
            "frame_budget_ms": frame_budget_ms,
            "refresh_hz_source": "collector_configured_budget",
            "frame_budget_source": "collector_configured_budget",
        },
        "environment": {
            "platform": "macOS",
            "os": platform.platform(),
            "device": adapter["name"],
            "browser": None,
            "adapter": adapter["name"],
            "driver": None if adapter["driver"] == "unavailable" else adapter["driver"],
        },
        "image": {"path": "final-frame.png", "sha256": capture.png_sha256, "width": width, "height": height},
        "raw_renderer_artifact": {
            "scope": "suite",
            "path": raw_session.raw_directory.relative_to(stage).as_posix(),
            "sha256": raw_session.raw_sha256,
            "immutable": True,
            "input_splat_count": capture.source_count,
        },
        "unavailable_fields": unavailable,
    }
    frame = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "frame",
        "run_id": run_id,
        "frame_index": 0,
        "elapsed_ns": capture.elapsed_ns,
        "call_ms": capture.call_ms,
        "frame_wall_ms": capture.frame_wall_ms,
        "preprocess_ms": None,
        "sort_ms": None,
        "geometry_submit_ms": None,
        "gpu_wait_ms": None,
        "gpu_complete_ms": None,
        "visible": capture.visible,
        "contributor": capture.contributor,
        "drawn": capture.drawn,
        "active_splats": capture.source_count,
        "exact_contributor_compaction": False,
        "sort_refreshed": None,
        "pair_id": pair_id,
        "endpoint_id": ENDPOINT_ID,
        "order_backend": order,
        "cut_name": cut_name,
        "sequence": sequence,
        "capture_index": capture_index,
        "trace_frame_index": capture.trace_frame_index,
        "camera": camera,
        "terminal_outcome": "presented",
        "presentation": presentation,
        "global_plan": ORDER_SPEC[order]["current"],
        "raw_renderer_ticket": capture.renderer_ticket,
        "raw_capture_index": capture.raw_capture_index,
        "capture_join": {
            "status": "verified",
            "current_stats_ticket": capture.renderer_ticket,
            "camera_revision": capture.identity["camera_revision"],
            "presentation_sequence": capture.identity["presentation_sequence"],
            "rgba8_sha256": capture.rgba8_sha256,
            "raw_artifact_sha256": raw_session.raw_sha256,
        },
        "raster_generation": capture.raster_generation,
        "encode_attempt": capture.encode_attempt,
    }
    summary = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": 1,
        "warmup_count": CANONICAL_WARMUP,
        "frame_budget_ms": frame_budget_ms,
        "missed_frame_count": int(capture.frame_wall_ms > frame_budget_ms),
        "distributions": {
            "call_ms": distribution(capture.call_ms),
            "frame_wall_ms": distribution(capture.frame_wall_ms),
            "preprocess_ms": None,
            "sort_ms": None,
            "geometry_submit_ms": None,
            "gpu_wait_ms": None,
            "gpu_complete_ms": None,
        },
    }
    write_json(directory / "manifest.json", manifest)
    (directory / "frames.jsonl").write_text(json.dumps(frame, sort_keys=True) + "\n", encoding="utf-8")
    write_json(directory / "summary.json", summary)
    validator = load_module(f"s1_m4_benchmark_{uuid.uuid4().hex}", BENCHMARK_VALIDATOR_PATH)
    validator.validate(directory)
    return {
        "path": directory.relative_to(stage).as_posix(),
        "sha256": IMAGE.artifact_directory_sha256(directory),
        "run_id": run_id,
        "frame_index": 0,
        "image": manifest["image"],
    }


def presented_cut_receipt(
    *,
    authority: CutAuthority,
    cut: CutInput,
    order: str,
    proxy: RawCapture,
    presentation: dict[str, Any],
    coverage_generation: int,
) -> dict[str, Any]:
    return {
        "source_sha256": authority.source["sha256"],
        "hierarchy_manifest_sha256": authority.hierarchy_manifest_sha256,
        "cut_name": cut.name,
        "coverage_sha256": cut.coverage_sha256,
        "order_backend": order,
        "outcome": "presented",
        "presentation": presentation,
        "coverage_generation": coverage_generation,
        "coverage_generation_semantics": "collector_evidence_identity_not_s4_runtime_generation",
        "source_splat_count": authority.source["splat_count"],
        "represented_source_leaves": cut.coverage["represented_source_leaves"],
        "active_proxy_splats": cut.active_splats,
        "visible": proxy.visible,
        "contributor": proxy.contributor,
        "drawn": proxy.drawn,
        "exact_contributor_compaction": False,
        "global_plan": ORDER_SPEC[order]["current"],
    }


def exactness_receipt(source_count: int) -> dict[str, Any]:
    return {
        "source_splat_count": source_count,
        "decoded_splat_count": source_count,
        "encoded_splat_count": source_count,
        "resident_splat_count": source_count,
        "addressable_splat_count": source_count,
        "source_sh_degree": 3,
        "resident_sh_degree": 3,
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "sh_degree_policy": "source",
        "render_mode": "sorted_alpha",
        "partial_scene_published": False,
        "full_quality": True,
    }


def materialize_endpoint_suite(
    *,
    stage: Path,
    repo: Path,
    authority: CutAuthority,
    trace: dict[str, Any],
    trace_path: Path,
    sessions: dict[tuple[str, str, str, str], RawSession],
    build: dict[str, Any],
) -> dict[str, Any]:
    authority_dir = stage / "authority"
    authority_dir.mkdir()
    dataset_source = repo / FORMAL_DATASET_MANIFEST
    review_source = repo / FORMAL_CAMERA_REVIEW
    copied = {
        "dataset": authority_dir / "bonsai.local-candidate.json",
        "trace": authority_dir / FORMAL_TRACE_PATH.name,
        "review": authority_dir / FORMAL_CAMERA_REVIEW.name,
        "hierarchy": authority_dir / "manifest.bin",
        "cut_receipt": authority_dir / "s1a-cut-receipt.json",
    }
    for source, destination in (
        (dataset_source, copied["dataset"]),
        (trace_path, copied["trace"]),
        (review_source, copied["review"]),
        (authority.hierarchy_manifest_path, copied["hierarchy"]),
        (authority.receipt_path, copied["cut_receipt"]),
    ):
        shutil.copyfile(source, destination)
        require(sha256_file(source) == sha256_file(destination), f"authority copy changed: {source}")
    for cut_name in ("bootstrap_roots", "mixed_depth_two_replacements"):
        destination = authority_dir / authority.cuts[cut_name].render_input["path"]
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(authority.cuts[cut_name].path, destination)
        require(
            sha256_file(destination) == authority.cuts[cut_name].render_input["sha256"],
            f"authority copy changed for {cut_name}",
        )

    review = read_json(review_source, "authored-camera review")
    embedded_review = {
        "status": "approved",
        "source_sha256": authority.source["sha256"],
        "camera_metadata_sha256": S1.FORMAL_CAMERA["sha256"],
        "selected_camera_ids": list(S1.FORMAL_CAMERA["selected_camera_ids"]),
        "trace_receipts": [
            {
                "endpoint_id": endpoint_id,
                "trace_file_sha256": expected["trace_file_sha256"],
                "trace_content_sha256": expected["trace_content_sha256"],
            }
            for endpoint_id, expected in S1.FORMAL_ENDPOINTS.items()
        ],
        "receipt_sha256": sha256_file(review_source),
        "receipt_decision": review["decision"],
    }
    exactness = exactness_receipt(authority.source["splat_count"])
    exactness_sha = canonical_sha256(exactness)
    comparisons: list[dict[str, Any]] = []
    frame_pixels: dict[tuple[str, str, str, int], Any] = {}
    coverage_generation = 0
    session_ordinals: dict[tuple[str, str, str], int] = {}
    next_ordinal = 1
    for order in S1.REQUIRED_ORDERS:
        for cut_name in S1.REQUIRED_CUTS:
            for session_name in SESSION_SELECTIONS:
                session_ordinals[(order, cut_name, session_name)] = next_ordinal
                next_ordinal += 1
                exact_session = sessions[(order, cut_name, "exact", session_name)]
                proxy_session = sessions[(order, cut_name, "proxy", session_name)]
                require(exact_session.adapter == proxy_session.adapter, f"{order}/{cut_name}/{session_name} adapter drift")
                selections = SESSION_SELECTIONS[session_name]
                for formal_index, raw_index, expected_trace in selections:
                    capture_index = (
                        REPLACEMENT_CUTS.index(cut_name)
                        if session_name == "replacement_sequence"
                        else formal_index
                    )
                    exact_capture = exact_session.captures[raw_index]
                    proxy_capture = proxy_session.captures[raw_index]
                    context = f"{order}/{cut_name}/{session_name}/{capture_index}"
                    require(exact_capture.source_count == authority.source["splat_count"], f"{context} Exact active count is not S")
                    require(proxy_capture.source_count == authority.cuts[cut_name].active_splats, f"{context} proxy raw active count is not P")
                    require(exact_capture.trace_frame_index == expected_trace, f"{context} Exact trace drift")
                    validate_pair(exact_capture, proxy_capture, context)
                    camera = camera_receipt(trace, expected_trace)
                    ordinal = session_ordinals[(order, cut_name, session_name)]
                    exact_presentation = canonical_presentation(exact_capture, ordinal)
                    proxy_presentation = canonical_presentation(proxy_capture, ordinal)
                    pair_id = f"s1-m4-{build['git']['commit'][:12]}-{order}-{cut_name}-{session_name}-{capture_index}"
                    receipts = {}
                    for lane, capture, presentation, session in (
                        ("exact", exact_capture, exact_presentation, exact_session),
                        ("proxy", proxy_capture, proxy_presentation, proxy_session),
                    ):
                        artifact = stage / "runs" / order / cut_name / session_name / f"capture-{capture_index}" / lane
                        receipts[lane] = write_benchmark_artifact(
                            artifact,
                            lane=lane,
                            capture=capture,
                            presentation=presentation,
                            pair_id=pair_id,
                            order=order,
                            cut_name=cut_name,
                            sequence=session_name,
                            capture_index=capture_index,
                            camera=camera,
                            authority=authority,
                            trace=trace,
                            adapter=session.adapter,
                            build=build,
                            raw_session=session,
                            stage=stage,
                        )
                    exact_image = {
                        "path": f"{receipts['exact']['path']}/final-frame.png",
                        "sha256": exact_capture.png_sha256,
                        "width": FORMAL_SIZE[0],
                        "height": FORMAL_SIZE[1],
                    }
                    proxy_image = {
                        "path": f"{receipts['proxy']['path']}/final-frame.png",
                        "sha256": proxy_capture.png_sha256,
                        "width": FORMAL_SIZE[0],
                        "height": FORMAL_SIZE[1],
                    }
                    exact_decoded = IMAGE.decode_rgba8_png(exact_capture.path.read_bytes(), f"{context} Exact", FORMAL_SIZE)
                    proxy_decoded = IMAGE.decode_rgba8_png(proxy_capture.path.read_bytes(), f"{context} proxy", FORMAL_SIZE)
                    metrics = IMAGE.compute_frame_metrics(exact_decoded, proxy_decoded)
                    coverage_generation += 1
                    comparison = {
                        "endpoint_id": ENDPOINT_ID,
                        "order_backend": order,
                        "cut_name": cut_name,
                        "sequence": session_name,
                        "capture_index": capture_index,
                        "trace_frame_index": expected_trace,
                        "pair_id": pair_id,
                        "exact_reference_sha256": exactness_sha,
                        "camera": camera,
                        "presentation": {"exact": exact_presentation, "proxy": proxy_presentation},
                        "presented_cut": presented_cut_receipt(
                            authority=authority,
                            cut=authority.cuts[cut_name],
                            order=order,
                            proxy=proxy_capture,
                            presentation=proxy_presentation,
                            coverage_generation=coverage_generation,
                        ),
                        "images": {"exact": exact_image, "proxy": proxy_image},
                        "benchmark_artifacts": {
                            "pair_id": pair_id,
                            "exact": {key: receipts["exact"][key] for key in ("path", "sha256", "run_id", "frame_index")},
                            "proxy": {key: receipts["proxy"][key] for key in ("path", "sha256", "run_id", "frame_index")},
                        },
                        "metrics": metrics,
                    }
                    comparisons.append(comparison)
                    frame_pixels[(order, cut_name, session_name, capture_index)] = IMAGE.FramePixels(
                        capture_index=capture_index,
                        trace_frame_index=expected_trace,
                        exact=exact_decoded,
                        candidate=proxy_decoded,
                    )

    transitions: list[dict[str, Any]] = []
    replacement_cuts = REPLACEMENT_CUTS
    for order in S1.REQUIRED_ORDERS:
        for cut_name in S1.REQUIRED_CUTS:
            for from_index, to_index in ((0, 1), (1, 2)):
                previous = frame_pixels[(order, cut_name, "moving_sequence", from_index)]
                current = frame_pixels[(order, cut_name, "moving_sequence", to_index)]
                transitions.append(
                    {
                        "endpoint_id": ENDPOINT_ID,
                        "order_backend": order,
                        "cut_name": cut_name,
                        "sequence": "moving_sequence",
                        "from_capture_index": from_index,
                        "to_capture_index": to_index,
                        "metrics": {S1.TEMPORAL_METRIC: IMAGE.compute_temporal_metric(previous, current)},
                    }
                )
        for from_index, to_index in ((0, 1), (1, 2)):
            previous_cut = replacement_cuts[from_index]
            current_cut = replacement_cuts[to_index]
            previous = frame_pixels[(order, previous_cut, "replacement_sequence", from_index)]
            current = frame_pixels[(order, current_cut, "replacement_sequence", to_index)]
            transitions.append(
                {
                    "endpoint_id": ENDPOINT_ID,
                    "order_backend": order,
                    "cut_name": previous_cut,
                    "sequence": "replacement_sequence",
                    "from_capture_index": from_index,
                    "to_capture_index": to_index,
                    "metrics": {S1.TEMPORAL_METRIC: IMAGE.compute_temporal_metric(previous, current)},
                }
            )

    adapters = {json.dumps(session.adapter, sort_keys=True) for session in sessions.values()}
    require(len(adapters) == 1, "M4 adapter identity changed across host invocations")
    adapter = next(iter(sessions.values())).adapter
    dataset_manifest = read_json(dataset_source, "Bonsai dataset authority")
    suite = {
        "schema": SCHEMA,
        "decision": "Captured",
        "pass": True,
        "scope": "apple_m4_materialized_cut_endpoint_only",
        "s1_acceptance": False,
        "s2_s5_unlocked": False,
        "contract": {
            "name": S1.SCHEMA,
            "aggregation": "logical_all",
            "required_cuts": list(S1.REQUIRED_CUTS),
            "required_order_backends": list(S1.REQUIRED_ORDERS),
            "frame_metric_limits": {key: list(value) for key, value in S1.FRAME_METRIC_LIMITS.items()},
            "temporal_metric": {"name": S1.TEMPORAL_METRIC, "maximum": S1.TEMPORAL_LIMIT},
        },
        "validator_dependencies": {
            "balanced_image_gate_sha256": S1.BALANCED_VALIDATOR_SHA256,
            "benchmark_artifact_validator_sha256": S1.BENCHMARK_VALIDATOR_SHA256,
        },
        "authority": {
            "dataset_manifest": {
                "scope": "repository",
                "path": FORMAL_DATASET_MANIFEST.as_posix(),
                "sha256": sha256_file(copied["dataset"]),
                "retained_copy": copied["dataset"].relative_to(stage).as_posix(),
            },
            "camera": {
                "path": S1.FORMAL_CAMERA["path"],
                "sha256": S1.FORMAL_CAMERA["sha256"],
                "bytes": S1.FORMAL_CAMERA["bytes"],
                "entry_count": S1.FORMAL_CAMERA["entry_count"],
                "selected_camera_ids": list(S1.FORMAL_CAMERA["selected_camera_ids"]),
                "review": embedded_review,
                "review_sha256": canonical_sha256(embedded_review),
                "review_receipt": {
                    "scope": "artifact",
                    "path": copied["review"].relative_to(stage).as_posix(),
                    "sha256": sha256_file(copied["review"]),
                },
            },
            "hierarchy_manifest": {
                "scope": "artifact",
                "path": copied["hierarchy"].relative_to(stage).as_posix(),
                "sha256": authority.hierarchy_manifest_sha256,
            },
            "builder": {
                "repository_commit": authority.receipt["authority"]["builder"]["repository_commit"],
                "configuration_sha256": authority.receipt["authority"]["builder"]["configuration_sha256"],
            },
            "s1a_cut_receipt": {
                "scope": "artifact",
                "path": copied["cut_receipt"].relative_to(stage).as_posix(),
                "sha256": authority.receipt_sha256,
            },
        },
        "exact_reference": {"exactness": exactness, "exactness_sha256": exactness_sha},
        "cuts": [
            {
                "name": name,
                "coverage": authority.cuts[name].coverage,
                "coverage_sha256": authority.cuts[name].coverage_sha256,
                "render_input": authority.cuts[name].render_input,
            }
            for name in S1.REQUIRED_CUTS
        ],
        "endpoint": {
            "id": ENDPOINT_ID,
            "status": "complete",
            "backend": "metal",
            "resolution": {
                **{f"{stage_name}_{axis}": value for stage_name in S1.RESOLUTION_STAGES for axis, value in (("width", FORMAL_SIZE[0]), ("height", FORMAL_SIZE[1]))},
                "dynamic_resolution": "disabled",
                "upscaling": "disabled",
                "full_resolution": True,
            },
            "surface_probe": {"status": "observed", "width": FORMAL_SIZE[0], "height": FORMAL_SIZE[1]},
            "trace": {
                "scope": "repository",
                "path": FORMAL_TRACE_PATH.as_posix(),
                "sha256": sha256_file(copied["trace"]),
                "trace_id": trace["trace_id"],
                "content_sha256": trace["content_sha256"],
                "retained_copy": copied["trace"].relative_to(stage).as_posix(),
            },
            "adapter": adapter,
        },
        "comparisons": comparisons,
        "transitions": transitions,
        "collector": {
            "repository_commit": build["git"]["commit"],
            "dirty": False,
            "host_invocations": len(sessions),
            "host_invocation_attempts_each": 1,
            "auto_retry": False,
            "raw_artifacts_immutable": True,
            "proxy_raw_count_authority": "input_P_only",
            "coverage_join": "S_and_R_from_s1a_receipt_P_V_C_D_from_renderer",
            "coverage_generation_semantics": "collector_evidence_identity_not_s4_runtime_generation",
            "formal_m4_endpoint_matrix_executed": True,
            "aggregate_two_endpoint_s1_matrix_executed": False,
            "benchmark_validator_sha256": sha256_file(BENCHMARK_VALIDATOR_PATH),
            "s1_validator_sha256": sha256_file(S1_VALIDATOR_PATH),
            "dataset_manifest_identity": dataset_manifest["sha256"],
        },
    }
    require(len(comparisons) == 36, "M4 endpoint comparison matrix is incomplete")
    require(len(transitions) == 16, "M4 endpoint transition matrix is incomplete")
    write_json(stage / "endpoint.json", suite)
    return suite


def validate_formal_prerequisites(repo: Path) -> tuple[dict[str, Any], Path]:
    preflight = S1.formal_collection_preflight()
    missing = preflight.get("missing_prerequisites", [])
    allowed = {"endpoint:apple_m4_metal", "endpoint:nothing_a065_vulkan"}
    unexpected = [item for item in missing if item.get("name") not in allowed]
    if unexpected:
        reasons = "; ".join(str(item.get("reason", item.get("name"))) for item in unexpected)
        raise DeferredEvidence(f"formal S1 prerequisite unavailable: {reasons}")
    trace_path = (repo / FORMAL_TRACE_PATH).resolve()
    S1.validate_frozen_trace(trace_path, ENDPOINT_ID, S1.FORMAL_ENDPOINTS[ENDPOINT_ID])
    trace = read_json(trace_path, "frozen M4 Bonsai trace")
    return trace, trace_path


def validate_ignored_output(repo: Path, output: Path) -> None:
    M2B.validate_ignored_output(repo, output)


def remove_private_build(stage: Path) -> None:
    target = stage / "cargo-target"
    require(target.is_dir() and not target.is_symlink(), "private Cargo target missing")
    shutil.rmtree(target)
    binary = stage / "build/desktop-example-bin"
    require(binary.is_file() and not binary.is_symlink(), "retained build binary missing")
    binary.unlink()


def publish(stage: Path, output: Path) -> None:
    require((stage / "endpoint.json").is_file(), "endpoint bundle is not materialized")
    require(not (stage / "cargo-target").exists(), "private Cargo target must not be published")
    executables = [
        path.relative_to(stage)
        for path in stage.rglob("*")
        if path.is_file() and path.stat().st_mode & 0o111
    ]
    require(not executables, f"staging retains executables: {executables}")
    require(not output.exists(), f"output appeared during collection: {output}")
    os.rename(stage, output)


def collect(args: argparse.Namespace, repo: Path) -> dict[str, Any]:
    output = args.output.resolve()
    require(not output.exists(), f"fresh output already exists: {output}")
    validate_ignored_output(repo, output)
    initial_git = M2B.git_receipt(repo)
    require(not initial_git["dirty"], "formal M4 collection requires a clean repository")
    authority = read_cut_authority(repo, args.cut_receipt)
    trace, trace_path = validate_formal_prerequisites(repo)
    require(M2B.git_receipt(repo) == initial_git, "Git receipt changed during read-only admission")

    stage = output.parent / f".{output.name}.stage-{os.getpid()}-{uuid.uuid4().hex[:10]}"
    require(not stage.exists(), f"staging path exists: {stage}")
    stage.mkdir(parents=True)
    decision = "Rejected"
    failure_reason: str | None = None
    try:
        binary, build_receipt = build_binary(repo, stage, initial_git)
        build = {
            "git": initial_git,
            "binary_sha256": build_receipt["binary_sha256"],
            "package_version": M2B.package_version(repo),
            "receipt": build_receipt,
        }
        sessions: dict[tuple[str, str, str, str], RawSession] = {}
        for order in S1.REQUIRED_ORDERS:
            for cut_name in S1.REQUIRED_CUTS:
                cut = authority.cuts[cut_name]
                for session_name in SESSION_SELECTIONS:
                    for lane, path, count in (
                        ("exact", authority.source_path, authority.source["splat_count"]),
                        ("proxy", cut.path, cut.active_splats),
                    ):
                        sessions[(order, cut_name, lane, session_name)] = run_raw_session(
                            stage=stage,
                            binary=binary,
                            binary_sha256=build["binary_sha256"],
                            input_path=path,
                            input_count=count,
                            trace_path=trace_path,
                            trace=trace,
                            order=order,
                            cut_name=cut_name,
                            lane=lane,
                            session_name=session_name,
                        )
                        require(sha256_file(path) == (authority.source["sha256"] if lane == "exact" else cut.render_input["sha256"]), f"{lane} input changed during collection")
                        require(M2B.git_receipt(repo) == initial_git, "Git receipt changed during collection")
        suite = materialize_endpoint_suite(
            stage=stage,
            repo=repo,
            authority=authority,
            trace=trace,
            trace_path=trace_path,
            sessions=sessions,
            build=build,
        )
        remove_private_build(stage)
        publish(stage, output)
        return suite
    except DeferredEvidence as error:
        decision = "Deferred"
        failure_reason = str(error)
        raise
    except Exception as error:
        failure_reason = str(error)
        raise
    finally:
        if stage.exists():
            failure = {
                "schema": SCHEMA,
                "decision": decision,
                "pass": False,
                "attempt": 1,
                "auto_retry": False,
                "output_published": output.exists(),
                "reason": failure_reason or "collection did not publish",
            }
            write_json(stage / "failure.json", failure)
            failed = output.parent / f"{output.name}.failed-{os.getpid()}-{uuid.uuid4().hex[:8]}"
            os.replace(stage, failed)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cut-receipt", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        suite = collect(args, REPO_ROOT)
    except DeferredEvidence as error:
        print(json.dumps({"schema": SCHEMA, "decision": "Deferred", "pass": False, "reason": str(error), "auto_retry": False}, sort_keys=True))
        return 2
    except (OSError, subprocess.SubprocessError, ValidationError, ValueError) as error:
        print(json.dumps({"schema": SCHEMA, "decision": "Rejected", "pass": False, "reason": str(error), "auto_retry": False}, sort_keys=True))
        return 1
    print(json.dumps({"schema": SCHEMA, "decision": suite["decision"], "pass": True, "output": str(args.output), "s1_acceptance": False}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
