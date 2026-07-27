#!/usr/bin/env python3
"""Collect static Nothing A065 materialized-cut capture prerequisites for S1.

This is an orchestration and evidence-join layer over the existing Android
collector.  It never installs or builds an APK.  Every raw Android artifact
keeps the dataset identity of the PLY it actually loaded: ``S`` for the Exact
source lane and ``P`` for a materialized proxy cut.  The outer receipt joins
that renderer evidence read-only with the offline author's complete ``S/R``
coverage receipt; it never rewrites a raw manifest or claims an S4 runtime cut.

The current Android app exposes one terminal PixelCopy PNG per process.  It has
neither renderer-owned multi-capture nor a renderer capture identity that can
be atomically joined to the successful present.  This collector therefore
retains only static authored views 0/1 as diagnostic prerequisites.  The
moving/replacement sequences and formal image gate always remain Deferred; no
frame or temporal quality metric is published from PixelCopy.

The command owns one attempt.  It performs one doctor/ADB/thermal admission,
uses fresh child outputs, and never retries a failed child collector command.
Missing device prerequisites are Deferred before collection; missing or
malformed child artifacts after launch are Rejected and retained in the fresh
output root.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import subprocess
import sys
from collections.abc import Sequence
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
ANDROID_COLLECTOR_PATH = (
    REPO_ROOT / "bindings/android/scripts/collect-android-sort-benchmarks.py"
)
S1_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-scalable-proxy-image-gate.py"
VERIFICATION_BOOTSTRAP = REPO_ROOT / "tests/verification_bootstrap.py"

SCHEMA = "gsplat-scalable-s1-a065-materialized-cut-capture/v1"
AUTHOR_SCHEMA = "gsplat-formal-s1-proxy-authoring/v1"
CUT_RENDER_INPUT_SCHEMA = "gsplat-formal-s1-cut-render-input/v1"
ENDPOINT_ID = "nothing_a065_vulkan"
ENDPOINT_BACKEND = "vulkan"
EXPECTED_MANUFACTURER = "Nothing"
EXPECTED_MODEL = "A065"
EXPECTED_SOC_MODEL = "SM8475"
EXPECTED_VULKAN_HAL = "adreno"
REQUIRED_ORDERS = ("cpu", "gpu")
REQUIRED_CUTS = (
    "complete_leaf_exact",
    "bootstrap_roots",
    "mixed_depth_two_replacements",
)
MOVING_TRACE = (0, 1, 0)
REPLACEMENT_CUTS = (
    "bootstrap_roots",
    "mixed_depth_two_replacements",
    "complete_leaf_exact",
)
WARMUP_FRAMES = 20
MAX_THERMAL_STATUS = 0
PIXELCOPY_IMAGE_GATE_REASON = (
    "Android PixelCopy has no renderer-owned capture identity joined atomically "
    "to the successful present"
)
MOVING_CAPTURE_REASON = (
    "the Android collector has no renderer-owned multi-capture receipt for one "
    "continuous 0->1->0 renderer session"
)
REPLACEMENT_CAPTURE_REASON = (
    "the Android collector has no renderer-owned multi-capture receipt for one "
    "continuous replacement-cut renderer session"
)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")


def load_module(name: str, source: pathlib.Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, source)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load repository helper {source}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ANDROID = load_module("scalable_s1_a065_android_collector", ANDROID_COLLECTOR_PATH)
S1 = load_module("scalable_s1_a065_gate_validator", S1_VALIDATOR_PATH)
BALANCED = S1.load_module(
    "scalable_s1_a065_balanced_validator", S1.BALANCED_VALIDATOR_PATH
)

FORMAL_SOURCE = S1.FORMAL_SOURCE
FORMAL_ENDPOINT = S1.FORMAL_ENDPOINTS[ENDPOINT_ID]
FORMAL_TRACE = REPO_ROOT / FORMAL_ENDPOINT["trace_path"]
FORMAL_SIZE = (FORMAL_ENDPOINT["width"], FORMAL_ENDPOINT["height"])


class CollectionError(RuntimeError):
    """Base class for one finite collection outcome."""


class DeferredCollection(CollectionError):
    """A prerequisite was unavailable before a device collection began."""


class RejectedCollection(CollectionError):
    """Available evidence violated the frozen collection contract."""


@dataclasses.dataclass(frozen=True)
class CutInput:
    name: str
    coverage: dict[str, Any]
    coverage_sha256: str
    render_input: dict[str, Any]
    input_path: pathlib.Path

    @property
    def active_splats(self) -> int:
        return int(self.render_input["P"])


@dataclasses.dataclass(frozen=True)
class AuthorPackage:
    receipt_path: pathlib.Path
    receipt_sha256: str
    receipt: dict[str, Any]
    source_path: pathlib.Path
    hierarchy_manifest_path: pathlib.Path
    cuts: dict[str, CutInput]


@dataclasses.dataclass(frozen=True)
class CaptureSpec:
    order_backend: str
    cut_name: str
    sequence: str
    capture_index: int
    trace_frame_index: int
    measured_frames: int
    playback: str

    @property
    def pair_id(self) -> str:
        return (
            f"s1-a065-{self.order_backend}-{self.cut_name}-"
            f"{self.sequence}-{self.capture_index}"
        )

    @property
    def slug(self) -> str:
        return self.pair_id


@dataclasses.dataclass(frozen=True)
class LaneEvidence:
    lane: str
    raw_root: pathlib.Path
    artifact_path: pathlib.Path
    artifact_sha256: str
    run_id: str
    frame_index: int
    dataset: dict[str, Any]
    exactness: dict[str, Any]
    image: dict[str, Any]
    presentation: dict[str, Any]
    camera_revision: int
    trace_frame_index: int
    counts: dict[str, Any]
    executed_plan: str
    build: dict[str, Any]
    package_identity: dict[str, Any]
    environment_receipt: dict[str, Any]
    thermal_status_before: int
    thermal_status_after: int


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise RejectedCollection(f"{context} must be an object")
    return value


def require_array(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise RejectedCollection(f"{context} must be an array")
    return value


def require_string(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise RejectedCollection(f"{context} must be a non-empty string")
    return value


def require_int(value: Any, context: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise RejectedCollection(f"{context} must be an available non-negative integer")
    if positive and value == 0:
        raise RejectedCollection(f"{context} must be positive")
    return value


def require_sha256(value: Any, context: str) -> str:
    text = require_string(value, context)
    if SHA256_RE.fullmatch(text) is None:
        raise RejectedCollection(f"{context} must be lowercase SHA-256")
    return text


def load_json(path: pathlib.Path, context: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise DeferredCollection(f"{context} is unavailable: {path}") from error
    except (OSError, json.JSONDecodeError) as error:
        raise RejectedCollection(f"cannot read {context}: {error}") from error
    return require_object(value, context)


def resolve_confined_file(root: pathlib.Path, relative: str, context: str) -> pathlib.Path:
    candidate = (root / relative).resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise RejectedCollection(f"{context} escapes the authored package") from error
    if not candidate.is_file():
        raise DeferredCollection(f"{context} is unavailable: {candidate}")
    return candidate


def verify_file_identity(
    path: pathlib.Path, expected_bytes: Any, expected_sha256: Any, context: str
) -> None:
    byte_count = require_int(expected_bytes, f"{context}.bytes", positive=True)
    digest = require_sha256(expected_sha256, f"{context}.sha256")
    if path.stat().st_size != byte_count:
        raise RejectedCollection(f"{context} byte count mismatch")
    if sha256_file(path) != digest:
        raise RejectedCollection(f"{context} SHA-256 mismatch")


def validate_author_package(receipt_path: pathlib.Path) -> AuthorPackage:
    selected = receipt_path.expanduser().resolve()
    receipt = load_json(selected, "S1a author receipt")
    if receipt.get("schema") != AUTHOR_SCHEMA:
        raise RejectedCollection(f"author receipt schema must equal {AUTHOR_SCHEMA!r}")
    expected_top = {
        "authoring_status": "complete",
        "scope": "offline_hierarchy_authoring_with_s1_cut_render_inputs",
        "s1_promotion_status": "Active",
        "endpoint_image_gate": "not_run",
        "s2_s5_unlocked": False,
    }
    for field, expected in expected_top.items():
        if receipt.get(field) != expected:
            raise RejectedCollection(f"author receipt {field} must equal {expected!r}")

    authority = require_object(receipt.get("authority"), "author receipt.authority")
    source = require_object(authority.get("source"), "author receipt.authority.source")
    source_identity = {
        "dataset_id": source.get("dataset_id"),
        "local_path": source.get("logical_path"),
        "sha256": source.get("sha256"),
        "bytes": source.get("bytes"),
        "splat_count": source.get("splat_count"),
        "sh_degree": source.get("sh_degree"),
    }
    if source_identity != FORMAL_SOURCE or source.get("sampling") != "disabled":
        raise RejectedCollection("author receipt does not bind the frozen complete-SH3 Bonsai source")
    source_path = (REPO_ROOT / FORMAL_SOURCE["local_path"]).resolve()
    if not source_path.is_file():
        raise DeferredCollection(f"formal Bonsai source is unavailable: {source_path}")
    verify_file_identity(
        source_path,
        source["bytes"],
        source["sha256"],
        "author receipt source",
    )

    builder = require_object(authority.get("builder"), "author receipt.authority.builder")
    if COMMIT_RE.fullmatch(str(builder.get("repository_commit"))) is None:
        raise RejectedCollection("author receipt builder commit must be full 40-hex")
    require_sha256(
        builder.get("configuration_sha256"),
        "author receipt.authority.builder.configuration_sha256",
    )

    hierarchy = require_object(receipt.get("hierarchy"), "author receipt.hierarchy")
    manifest = require_object(hierarchy.get("manifest"), "author receipt.hierarchy.manifest")
    manifest_path = resolve_confined_file(
        selected.parent,
        require_string(manifest.get("path"), "author receipt hierarchy manifest path"),
        "author receipt hierarchy manifest",
    )
    verify_file_identity(
        manifest_path,
        manifest.get("bytes"),
        manifest.get("sha256"),
        "author receipt hierarchy manifest",
    )
    manifest_sha256 = require_sha256(
        manifest.get("sha256"), "author receipt hierarchy manifest SHA-256"
    )

    raw_cuts = require_array(receipt.get("cuts"), "author receipt.cuts")
    indexed: dict[str, CutInput] = {}
    for index, raw in enumerate(raw_cuts):
        context = f"author receipt.cuts[{index}]"
        cut = require_object(raw, context)
        name = require_string(cut.get("name"), f"{context}.name")
        if name in indexed:
            raise RejectedCollection(f"duplicate authored cut {name!r}")
        coverage = require_object(cut.get("coverage"), f"{context}.coverage")
        coverage_sha256 = require_sha256(
            cut.get("coverage_sha256"), f"{context}.coverage_sha256"
        )
        if S1.canonical_sha256(coverage) != coverage_sha256:
            raise RejectedCollection(f"{context}.coverage_sha256 mismatch")
        if coverage.get("hierarchy_manifest_sha256") != manifest_sha256:
            raise RejectedCollection(f"{context} hierarchy manifest hash mismatch")
        render_input = require_object(cut.get("render_input"), f"{context}.render_input")
        if render_input.get("schema") != CUT_RENDER_INPUT_SCHEMA:
            raise RejectedCollection(f"{context} render-input schema mismatch")
        if render_input.get("cut_name") != name:
            raise RejectedCollection(f"{context} render-input cut name mismatch")
        if render_input.get("source_sha256") != FORMAL_SOURCE["sha256"]:
            raise RejectedCollection(f"{context} render-input source hash mismatch")
        if render_input.get("hierarchy_manifest_sha256") != manifest_sha256:
            raise RejectedCollection(f"{context} render-input hierarchy hash mismatch")
        active = require_int(render_input.get("P"), f"{context}.render_input.P", positive=True)
        if active != coverage.get("active_proxy_splats"):
            raise RejectedCollection(f"{context} render-input P differs from coverage")
        if render_input.get("sh_degree") != 3 or render_input.get("sampling") != "disabled":
            raise RejectedCollection(f"{context} render input must preserve SH3 without sampling")
        if name == "complete_leaf_exact":
            if (
                render_input.get("kind") != "content_addressed_source_ply_alias"
                or render_input.get("logical_path") != FORMAL_SOURCE["local_path"]
                or render_input.get("sha256") != FORMAL_SOURCE["sha256"]
                or render_input.get("bytes") != FORMAL_SOURCE["bytes"]
                or render_input.get("copied_into_package") is not False
                or active != FORMAL_SOURCE["splat_count"]
            ):
                raise RejectedCollection("complete_leaf_exact render input is not the frozen source alias")
            input_path = source_path
        else:
            if render_input.get("kind") != "materialized_binary_little_endian_sh3_ply":
                raise RejectedCollection(f"{context} is not a materialized SH3 PLY")
            input_path = resolve_confined_file(
                selected.parent,
                require_string(render_input.get("path"), f"{context}.render_input.path"),
                f"{context} materialized PLY",
            )
            verify_file_identity(
                input_path,
                render_input.get("bytes"),
                render_input.get("sha256"),
                f"{context} materialized PLY",
            )
        indexed[name] = CutInput(
            name=name,
            coverage=coverage,
            coverage_sha256=coverage_sha256,
            render_input=render_input,
            input_path=input_path,
        )

    if set(indexed) != set(REQUIRED_CUTS):
        raise RejectedCollection("author receipt must contain exactly the three frozen S1 cuts")

    s1_authority = S1.Authority(
        dataset_id=FORMAL_SOURCE["dataset_id"],
        source_sha256=FORMAL_SOURCE["sha256"],
        source_bytes=FORMAL_SOURCE["bytes"],
        source_splats=FORMAL_SOURCE["splat_count"],
        source_sh_degree=FORMAL_SOURCE["sh_degree"],
        camera_metadata_sha256=S1.FORMAL_CAMERA["sha256"],
        camera_review_traces={
            endpoint: (values["trace_file_sha256"], values["trace_content_sha256"])
            for endpoint, values in S1.FORMAL_ENDPOINTS.items()
        },
        hierarchy_manifest_sha256=manifest_sha256,
        builder_commit=builder["repository_commit"],
        builder_configuration_sha256=builder["configuration_sha256"],
    )
    try:
        S1.validate_cuts({"cuts": raw_cuts}, s1_authority)
    except S1.ValidationError as error:
        raise RejectedCollection(f"author receipt fails S1 coverage validation: {error}") from error

    return AuthorPackage(
        receipt_path=selected,
        receipt_sha256=sha256_file(selected),
        receipt=receipt,
        source_path=source_path,
        hierarchy_manifest_path=manifest_path,
        cuts=indexed,
    )


def capture_specs() -> list[CaptureSpec]:
    result: list[CaptureSpec] = []
    for order_backend in REQUIRED_ORDERS:
        for cut_name in REQUIRED_CUTS:
            for capture_index, trace_frame_index in enumerate((0, 1)):
                result.append(
                    CaptureSpec(
                        order_backend,
                        cut_name,
                        "authored_views",
                        capture_index,
                        trace_frame_index,
                        1,
                        "fixed",
                    )
                )
    return result


def collector_command(
    args: argparse.Namespace,
    spec: CaptureSpec,
    lane: str,
    package: AuthorPackage,
    raw_output: pathlib.Path,
) -> list[str]:
    if lane not in {"exact", "proxy"}:
        raise ValueError(f"unknown lane {lane!r}")
    if (
        spec.sequence != "authored_views"
        or spec.playback != "fixed"
        or spec.measured_frames != 1
        or spec.trace_frame_index not in {0, 1}
    ):
        raise ValueError("A065 collector supports static authored views 0/1 only")
    input_path = package.source_path if lane == "exact" else package.cuts[spec.cut_name].input_path
    command = [
        sys.executable,
        os.fspath(ANDROID_COLLECTOR_PATH),
        "--serial",
        args.serial,
        "--ply",
        os.fspath(input_path),
        "--backend",
        spec.order_backend,
        "--repetitions",
        "1",
        "--sort-interval",
        "1",
        "--frames",
        str(spec.measured_frames),
        "--warmup",
        str(WARMUP_FRAMES),
        "--yaw",
        "0",
        "--frame-latency",
        "2",
        "--camera-trace",
        os.fspath(FORMAL_TRACE),
        "--geometry-path",
        "packed",
        "--max-thermal-status",
        str(MAX_THERMAL_STATUS),
        "--thermal-timeout-seconds",
        str(args.thermal_timeout_seconds),
        "--thermal-poll-seconds",
        str(args.thermal_poll_seconds),
        "--run-timeout-seconds",
        str(args.run_timeout_seconds),
        "--capture-final-png",
        "--output",
        os.fspath(raw_output),
    ]
    command.extend(["--camera-frame", str(spec.trace_frame_index)])
    if args.apk is not None:
        command.extend(["--apk", os.fspath(args.apk)])
    if args.adb is not None:
        command.extend(["--adb", os.fspath(args.adb)])
    return command


def expected_trace_schedule(spec: CaptureSpec) -> list[int]:
    if (
        spec.sequence != "authored_views"
        or spec.playback != "fixed"
        or spec.measured_frames != 1
        or spec.trace_frame_index not in {0, 1}
    ):
        raise RejectedCollection("A065 raw evidence is not one static authored view")
    return [spec.trace_frame_index]


def validate_a065_environment(receipt: dict[str, Any]) -> None:
    expected = {
        "manufacturer": EXPECTED_MANUFACTURER,
        "model": EXPECTED_MODEL,
    }
    for field, value in expected.items():
        if receipt.get(field) != value:
            raise RejectedCollection(f"Android endpoint {field} is not frozen A065 identity")
    properties = require_object(
        receipt.get("device_properties"), "Android environment device_properties"
    )
    soc = require_object(properties.get("soc_model_property"), "A065 SoC receipt")
    vulkan = require_object(properties.get("vulkan_hal_property"), "A065 Vulkan receipt")
    if soc.get("value") != EXPECTED_SOC_MODEL:
        raise RejectedCollection("Android endpoint SoC is not the frozen A065 SM8475")
    if vulkan.get("value") != EXPECTED_VULKAN_HAL:
        raise RejectedCollection("Android endpoint Vulkan HAL is not the frozen Adreno path")


def validate_exactness(exactness: dict[str, Any], active: int, context: str) -> None:
    count_fields = (
        "source_splat_count",
        "decoded_splat_count",
        "encoded_splat_count",
        "resident_splat_count",
        "addressable_splat_count",
    )
    if any(exactness.get(field) != active for field in count_fields):
        raise RejectedCollection(f"{context} must prove source=decoded=encoded=resident=addressable=P")
    expected = {
        "source_sh_degree": 3,
        "resident_sh_degree": 3,
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "sh_degree_policy": "source",
        "partial_scene_published": False,
        "full_quality": True,
    }
    for field, value in expected.items():
        if exactness.get(field) != value:
            raise RejectedCollection(f"{context}.{field} must equal {value!r}")


def read_lane_evidence(
    raw_root: pathlib.Path,
    spec: CaptureSpec,
    lane: str,
    expected_input: pathlib.Path,
    expected_active: int,
) -> LaneEvidence:
    experiment = load_json(raw_root / "experiment.json", f"{lane} raw experiment")
    if experiment.get("schema") != "gsplat-android-sort-experiment/v1" or experiment.get("status") != "complete":
        raise RejectedCollection(f"{lane} raw experiment is not complete")
    repository = require_object(experiment.get("repository"), f"{lane} raw repository")
    if repository.get("dirty") is not False or COMMIT_RE.fullmatch(str(repository.get("commit"))) is None:
        raise RejectedCollection(f"{lane} raw experiment must bind a clean repository commit")
    configuration = require_object(experiment.get("configuration"), f"{lane} raw configuration")
    expected_configuration = {
        "backends": [spec.order_backend],
        "repetitions": 1,
        "frames": spec.measured_frames,
        "warmup": WARMUP_FRAMES,
        "sort_interval": 1,
        "async_sort": False,
        "frame_latency": 2,
        "geometry_path": "packed",
        "gpu_producer": None,
        "capture_final_png": True,
        "formal_artifact": False,
        "max_thermal_status": MAX_THERMAL_STATUS,
        "apk_mode": "reuse-exact-installed",
    }
    for field, value in expected_configuration.items():
        if configuration.get(field) != value:
            raise RejectedCollection(f"{lane} raw configuration.{field} must equal {value!r}")

    dataset_identity = require_object(experiment.get("dataset"), f"{lane} raw dataset")
    expected_file_identity = {
        "bytes": expected_input.stat().st_size,
        "sha256": sha256_file(expected_input),
    }
    for field, value in expected_file_identity.items():
        if dataset_identity.get(field) != value:
            raise RejectedCollection(f"{lane} raw experiment dataset {field} mismatch")
    trace_identity = require_object(experiment.get("trace"), f"{lane} raw trace")
    expected_trace = load_json(FORMAL_TRACE, "frozen A065 Bonsai trace")
    runs = require_array(experiment.get("runs"), f"{lane} raw runs")
    if len(runs) != 1 or not isinstance(runs[0], dict) or runs[0].get("status") != "complete":
        raise RejectedCollection(f"{lane} raw experiment must contain one complete run")
    run = runs[0]
    if run.get("backend") != spec.order_backend:
        raise RejectedCollection(f"{lane} raw run backend mismatch")
    thermal_status_before = require_int(
        run.get("thermal_status_before"), f"{lane} raw thermal_status_before"
    )
    thermal_status_after = require_int(
        run.get("thermal_status_after"), f"{lane} raw thermal_status_after"
    )
    if thermal_status_before != MAX_THERMAL_STATUS:
        raise RejectedCollection(f"{lane} raw run was not admitted at thermal status zero")
    if thermal_status_after != MAX_THERMAL_STATUS:
        raise RejectedCollection(f"{lane} raw run ended above thermal status zero")
    artifact_relative = require_string(run.get("artifact"), f"{lane} raw artifact path")
    try:
        artifact = BALANCED.resolve_artifact_directory(
            raw_root, artifact_relative, f"{lane} raw artifact path"
        )
    except BALANCED.ValidationError as error:
        raise RejectedCollection(str(error)) from error
    manifest = load_json(artifact / "manifest.json", f"{lane} benchmark manifest")
    summary = load_json(artifact / "summary.json", f"{lane} benchmark summary")
    try:
        frames = ANDROID.read_artifact_frames(artifact / "frames.jsonl")
    except (OSError, RuntimeError) as error:
        raise RejectedCollection(f"{lane} benchmark frames are invalid: {error}") from error
    try:
        ANDROID.validate_run_artifact(
            manifest,
            summary,
            frames,
            spec.order_backend,
            "packed",
            dataset_identity,
            expected_trace,
            trace_identity,
            None,
            load_json(
                raw_root / "android-environment-receipt.json",
                f"{lane} Android environment receipt",
            ),
        )
    except RuntimeError as error:
        raise RejectedCollection(f"{lane} raw Android validation failed: {error}") from error

    dataset = require_object(manifest.get("dataset"), f"{lane} benchmark dataset")
    expected_dataset = {
        "sha256": expected_file_identity["sha256"],
        "bytes": expected_file_identity["bytes"],
        "splat_count": expected_active,
        "sh_degree": 3,
    }
    for field, value in expected_dataset.items():
        if dataset.get(field) != value:
            raise RejectedCollection(
                f"{lane} raw dataset must report the actual input P; {field} mismatch"
            )
    exactness = require_object(manifest.get("exactness"), f"{lane} benchmark exactness")
    validate_exactness(exactness, expected_active, f"{lane} benchmark exactness")
    renderer = require_object(manifest.get("renderer"), f"{lane} benchmark renderer")
    if renderer.get("backend") != ENDPOINT_BACKEND or renderer.get("path") != "packed_atlas":
        raise RejectedCollection(f"{lane} benchmark is not A065 Vulkan Packed Exact")
    if renderer.get("order_backend_requested") != spec.order_backend:
        raise RejectedCollection(f"{lane} benchmark did not force {spec.order_backend}")

    if len(frames) != spec.measured_frames:
        raise RejectedCollection(f"{lane} benchmark measured-frame count mismatch")
    if [frame.get("trace_frame_index") for frame in frames] != expected_trace_schedule(spec):
        raise RejectedCollection(f"{lane} benchmark did not execute the frozen camera prefix")
    terminal_index = len(frames) - 1
    terminal = frames[terminal_index]
    if terminal.get("frame_index") != terminal_index:
        raise RejectedCollection(f"{lane} terminal frame index mismatch")
    ledger = require_array(
        summary.get("current_stats_terminal_ledger"),
        f"{lane} current-stats terminal ledger",
    )
    if len(ledger) != len(frames):
        raise RejectedCollection(f"{lane} current-stats ledger is incomplete")
    terminal_stats = require_object(ledger[terminal_index], f"{lane} terminal current-stats")
    identity = require_object(terminal_stats.get("identity"), f"{lane} terminal identity")
    ticket = require_int(terminal_stats.get("ticket"), f"{lane} current-stats ticket", positive=True)
    camera_revision = require_int(
        terminal.get("camera_revision"), f"{lane} terminal camera revision", positive=True
    )
    presentation_sequence = require_int(
        identity.get("presentation_sequence"),
        f"{lane} terminal presentation sequence",
        positive=True,
    )
    if (
        terminal.get("current_stats_ticket") != ticket
        or terminal.get("current_stats_presentation_sequence") != presentation_sequence
        or identity.get("camera_revision") != camera_revision
    ):
        raise RejectedCollection(f"{lane} current-stats terminal join is stale")
    camera_receipt = require_object(terminal.get("camera_receipt"), f"{lane} camera receipt")
    if (
        camera_receipt.get("camera_revision") != camera_revision
        or camera_receipt.get("presented_camera_revision") != camera_revision
        or (camera_receipt.get("surface_width"), camera_receipt.get("surface_height"))
        != FORMAL_SIZE
    ):
        raise RejectedCollection(f"{lane} camera receipt is not the terminal successful present")

    plan = require_string(identity.get("executed_plan"), f"{lane} executed plan")
    expected_plan = "cpu_post_sort" if spec.order_backend == "cpu" else "gpu_post_sort"
    if plan != expected_plan or terminal.get("current_stats_executed_plan") != plan:
        raise RejectedCollection(f"{lane} terminal plan is not forced {expected_plan}")
    counts = {
        "source": require_int(terminal_stats.get("source"), f"{lane} S", positive=True),
        "visible": require_int(terminal_stats.get("visible"), f"{lane} V"),
        "contributor": require_int(terminal_stats.get("contributor"), f"{lane} C"),
        "drawn": require_int(terminal_stats.get("drawn"), f"{lane} D"),
        "exact_contributor_compaction": terminal.get("exact_contributor_compaction"),
    }
    if counts["source"] != expected_active:
        raise RejectedCollection(f"{lane} current-stats source must equal actual input P")
    if not 0 <= counts["contributor"] <= counts["visible"] <= expected_active:
        raise RejectedCollection(f"{lane} terminal counts must prove 0 <= C <= V <= P")
    compacted = counts["exact_contributor_compaction"]
    if not isinstance(compacted, bool) or counts["drawn"] != (
        counts["contributor"] if compacted else counts["visible"]
    ):
        raise RejectedCollection(f"{lane} terminal D does not match its compaction semantics")

    image = require_object(manifest.get("image"), f"{lane} benchmark image")
    if (image.get("width"), image.get("height")) != FORMAL_SIZE:
        raise RejectedCollection(f"{lane} final PNG is not the real A065 Surface size")
    image_path = artifact / require_string(image.get("path"), f"{lane} benchmark image path")
    verify_file_identity(
        image_path,
        image_path.stat().st_size,
        image.get("sha256"),
        f"{lane} benchmark image",
    )
    pull_receipt = require_object(
        manifest.get("android_device_png_pull"), f"{lane} device PNG pull receipt"
    )
    run_id = require_string(manifest.get("run_id"), f"{lane} benchmark run_id")
    if (
        pull_receipt.get("schema") != ANDROID.DEVICE_PNG_PULL_RECEIPT_SCHEMA
        or pull_receipt.get("source")
        != "adb-exec-out-run-as-after-benchmark-complete"
        or pull_receipt.get("device_path") != ANDROID.INTERNAL_FINAL_PNG
        or pull_receipt.get("device_path_absent_after_package_clear") is not True
        or pull_receipt.get("benchmark_run_id") != run_id
        or pull_receipt.get("benchmark_completed") is not True
        or pull_receipt.get("pulled_after_completed_log") is not True
    ):
        raise RejectedCollection(f"{lane} final PNG is not bound to the completed benchmark")
    local_png = require_object(pull_receipt.get("local_identity"), f"{lane} local PNG identity")
    if (
        local_png.get("sha256") != image.get("sha256")
        or (local_png.get("width"), local_png.get("height")) != FORMAL_SIZE
    ):
        raise RejectedCollection(f"{lane} final PNG identity drifted from its pull receipt")

    environment = require_object(manifest.get("environment"), f"{lane} environment")
    environment_receipt = require_object(
        environment.get("android_device_receipt"), f"{lane} Android environment receipt"
    )
    validate_a065_environment(environment_receipt)
    build = require_object(manifest.get("build"), f"{lane} build")
    if build.get("dirty") is not False or build.get("repository_commit") != repository["commit"]:
        raise RejectedCollection(f"{lane} build does not bind the clean collector commit")
    package_identity = {
        field: experiment.get(field)
        for field in ("apk", "native_library", "installed_apk")
    }
    if any(not isinstance(value, dict) for value in package_identity.values()):
        raise RejectedCollection(f"{lane} package identity is incomplete")

    presentation = {
        "outcome": "presented",
        "primitive_presented": True,
        "ticket": ticket,
        "scene_generation": require_int(
            identity.get("scene_generation"), f"{lane} scene generation", positive=True
        ),
        "camera_generation": camera_revision,
        "viewport_generation": require_int(
            identity.get("viewport_generation"), f"{lane} viewport generation", positive=True
        ),
        "contract_generation": require_int(
            identity.get("contract_generation"), f"{lane} contract generation", positive=True
        ),
        "plan_generation": require_int(
            identity.get("plan_set_generation"), f"{lane} plan generation", positive=True
        ),
        "presentation_generation": presentation_sequence,
        "order_generation": require_int(
            identity.get("order_generation"), f"{lane} order generation", positive=True
        ),
        "raster_generation": require_int(
            identity.get("raster_generation"), f"{lane} raster generation", positive=True
        ),
        "encode_attempt": require_int(
            identity.get("encode_attempt"), f"{lane} encode attempt", positive=True
        ),
    }
    return LaneEvidence(
        lane=lane,
        raw_root=raw_root,
        artifact_path=artifact,
        artifact_sha256=BALANCED.artifact_directory_sha256(artifact),
        run_id=run_id,
        frame_index=terminal_index,
        dataset=dataset,
        exactness=exactness,
        image={**image, "absolute_path": image_path},
        presentation=presentation,
        camera_revision=camera_revision,
        trace_frame_index=spec.trace_frame_index,
        counts=counts,
        executed_plan=plan,
        build=build,
        package_identity=package_identity,
        environment_receipt=environment_receipt,
        thermal_status_before=thermal_status_before,
        thermal_status_after=thermal_status_after,
    )


def scoped_path(path: pathlib.Path, root: pathlib.Path, context: str) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as error:
        raise RejectedCollection(f"{context} is outside the S1 output root") from error


def image_receipt(lane: LaneEvidence, output: pathlib.Path) -> dict[str, Any]:
    return {
        "path": scoped_path(lane.image["absolute_path"], output, f"{lane.lane} image"),
        "sha256": lane.image["sha256"],
        "width": lane.image["width"],
        "height": lane.image["height"],
    }


def retain_static_pair(
    spec: CaptureSpec,
    cut: CutInput,
    exact: LaneEvidence,
    proxy: LaneEvidence,
    output: pathlib.Path,
) -> dict[str, Any]:
    if spec.sequence != "authored_views":
        raise RejectedCollection("only static authored views may be retained")
    if exact.dataset.get("splat_count") != FORMAL_SOURCE["splat_count"]:
        raise RejectedCollection("Exact raw artifact active count must equal S")
    if proxy.dataset.get("splat_count") != cut.active_splats:
        raise RejectedCollection("proxy raw artifact dataset authority must equal input P")
    if exact.build != proxy.build:
        raise RejectedCollection(f"{spec.pair_id} Exact/proxy build identities differ")
    if exact.package_identity != proxy.package_identity:
        raise RejectedCollection(f"{spec.pair_id} Exact/proxy APK/native identities differ")
    if exact.environment_receipt != proxy.environment_receipt:
        raise RejectedCollection(f"{spec.pair_id} Exact/proxy A065 identities differ")
    if exact.executed_plan != proxy.executed_plan:
        raise RejectedCollection(f"{spec.pair_id} Exact/proxy executed plans differ")

    exact_image_receipt = image_receipt(exact, output)
    proxy_image_receipt = image_receipt(proxy, output)

    trace = load_json(FORMAL_TRACE, "frozen A065 Bonsai trace")
    trace_frame = trace["frames"][spec.trace_frame_index]
    camera = {
        "trace_id": trace["trace_id"],
        "trace_content_sha256": trace["content_sha256"],
        "pose_intrinsics_sha256": S1.canonical_sha256(
            {"pose": trace_frame["pose"], "intrinsics": trace_frame["intrinsics"]}
        ),
    }
    coverage_evidence = {
        **cut.coverage,
        "coverage_sha256": cut.coverage_sha256,
        "identity_semantics": "offline_author_receipt_join_not_runtime_coverage_generation",
    }
    return {
        "pair_id": spec.pair_id,
        "endpoint_id": ENDPOINT_ID,
        "order_backend": spec.order_backend,
        "cut_name": spec.cut_name,
        "sequence": spec.sequence,
        "capture_index": spec.capture_index,
        "trace_frame_index": spec.trace_frame_index,
        "camera": camera,
        "raw_dataset_authority": {
            "exact": {
                "sha256": exact.dataset["sha256"],
                "bytes": exact.dataset["bytes"],
                "authority_symbol": "S",
                "active_splats": exact.dataset["splat_count"],
            },
            "proxy": {
                "sha256": proxy.dataset["sha256"],
                "bytes": proxy.dataset["bytes"],
                "authority_symbol": "P",
                "active_splats": proxy.dataset["splat_count"],
            },
        },
        "renderer_present_evidence": {
            "exact": exact.presentation,
            "proxy": proxy.presentation,
        },
        "presented_cut": {
            "source_sha256": FORMAL_SOURCE["sha256"],
            "hierarchy_manifest_sha256": cut.coverage["hierarchy_manifest_sha256"],
            "cut_name": spec.cut_name,
            "coverage_evidence": coverage_evidence,
            "source_splat_count": cut.coverage["source_splat_count"],
            "represented_source_leaves": cut.coverage["represented_source_leaves"],
            "active_proxy_splats": cut.active_splats,
            "visible": proxy.counts["visible"],
            "contributor": proxy.counts["contributor"],
            "drawn": proxy.counts["drawn"],
            "exact_contributor_compaction": proxy.counts["exact_contributor_compaction"],
            "global_plan": proxy.executed_plan,
            "outcome": "presented",
            "presentation": proxy.presentation,
        },
        "pixelcopy_images": {
            "exact": exact_image_receipt,
            "proxy": proxy_image_receipt,
        },
        "image_gate": {
            "decision": "Deferred",
            "pass": False,
            "reason": PIXELCOPY_IMAGE_GATE_REASON,
            "capture_source": "android_surface_pixelcopy",
            "association_to_renderer_present": "same_benchmark_process_only",
            "renderer_same_present_capture_identity": "unavailable",
            "formal_metrics_published": False,
        },
        "benchmark_artifacts": {
            "pair_id": spec.pair_id,
            "exact": {
                "path": scoped_path(exact.artifact_path, output, "Exact raw artifact"),
                "sha256": exact.artifact_sha256,
                "run_id": exact.run_id,
                "frame_index": exact.frame_index,
                "thermal_status_before": exact.thermal_status_before,
                "thermal_status_after": exact.thermal_status_after,
            },
            "proxy": {
                "path": scoped_path(proxy.artifact_path, output, "proxy raw artifact"),
                "sha256": proxy.artifact_sha256,
                "run_id": proxy.run_id,
                "frame_index": proxy.frame_index,
                "thermal_status_before": proxy.thermal_status_before,
                "thermal_status_after": proxy.thermal_status_after,
            },
        },
    }


def atomic_write_json(path: pathlib.Path, value: dict[str, Any]) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    os.replace(temporary, path)


def claim_output(path: pathlib.Path) -> None:
    if path.exists():
        raise ValueError(f"output already exists; refusing to overwrite: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.mkdir()


def repository_identity() -> dict[str, Any]:
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=REPO_ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout.strip()
    dirty = bool(
        subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=normal"],
            cwd=REPO_ROOT,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout.strip()
    )
    if COMMIT_RE.fullmatch(commit) is None or dirty:
        raise RejectedCollection("formal A065 collection requires one clean named commit")
    return {"commit": commit, "dirty": False}


def doctor_environment(
    args: argparse.Namespace, package: AuthorPackage, output: pathlib.Path
) -> tuple[list[str], dict[str, str]]:
    command = [
        sys.executable,
        os.fspath(VERIFICATION_BOOTSTRAP),
        "doctor",
        "--profile",
        "android-a065",
    ]
    environment = os.environ.copy()
    environment["GSPLAT_ANDROID_SERIAL"] = args.serial
    environment["GSPLAT_ANDROID_DATASET"] = os.fspath(package.source_path)
    environment["GSPLAT_ANDROID_OUTPUT"] = os.fspath(output / "doctor-probe")
    return command, environment


def admit_device(
    args: argparse.Namespace, package: AuthorPackage, output: pathlib.Path
) -> dict[str, Any]:
    command, environment = doctor_environment(args, package, output)
    try:
        doctor = subprocess.run(
            command,
            cwd=REPO_ROOT,
            env=environment,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=args.doctor_timeout_seconds,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise DeferredCollection(f"android-a065 doctor is unavailable: {error}") from error
    if doctor.returncode != 0:
        raise DeferredCollection(
            "android-a065 doctor did not reach READY: " + doctor.stdout.strip()
        )
    try:
        adb = ANDROID.resolve_adb(args.adb, dry_run=False)
        state = ANDROID.run_command(
            ANDROID.adb_args(adb, args.serial, "get-state"), capture=True
        ).stdout.strip()
        observed_serial = ANDROID.run_command(
            ANDROID.adb_args(adb, args.serial, "get-serialno"), capture=True
        ).stdout.strip()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise DeferredCollection(f"ADB identity is unavailable: {error}") from error
    if state != "device" or observed_serial != args.serial:
        raise DeferredCollection(
            f"ADB identity is unavailable: state={state!r} serial={observed_serial!r}"
        )
    try:
        android_device_receipt = ANDROID.build_android_environment_receipt(
            ANDROID.device_info(adb, args.serial)
        )
        validate_a065_environment(android_device_receipt)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        raise DeferredCollection(f"A065 device identity is unavailable: {error}") from error
    except RejectedCollection as error:
        raise DeferredCollection(f"ADB target is not the frozen A065: {error}") from error
    try:
        thermal = ANDROID.wait_for_thermal_status(
            adb,
            args.serial,
            MAX_THERMAL_STATUS,
            args.thermal_timeout_seconds,
            args.thermal_poll_seconds,
        )
    except (RuntimeError, TimeoutError) as error:
        raise DeferredCollection(f"A065 thermal admission is unavailable: {error}") from error
    return {
        "doctor_command": ANDROID.command_text(command),
        "doctor_status": "READY",
        "adb_state": state,
        "adb_serial": observed_serial,
        "android_device_receipt": android_device_receipt,
        "thermal_status": thermal,
        "max_thermal_status": MAX_THERMAL_STATUS,
        "checked_at_utc": utc_now(),
    }


def validate_formal_preflight() -> dict[str, Any]:
    try:
        receipt = S1.formal_collection_preflight()
    except S1.DeferredEvidence as error:
        raise DeferredCollection(str(error)) from error
    except S1.ValidationError as error:
        raise RejectedCollection(f"formal S1 preflight rejected: {error}") from error
    missing = require_array(
        receipt.get("missing_prerequisites"), "formal preflight missing_prerequisites"
    )
    unavailable_authority = [
        item
        for item in missing
        if isinstance(item, dict) and not str(item.get("name", "")).startswith("endpoint:")
    ]
    if unavailable_authority:
        reasons = "; ".join(str(item.get("reason")) for item in unavailable_authority)
        raise DeferredCollection(f"formal S1 authority is unavailable: {reasons}")
    return receipt


def endpoint_artifact_header(
    package: AuthorPackage,
    repository: dict[str, Any],
    admission: dict[str, Any] | None,
) -> dict[str, Any]:
    authority = package.receipt["authority"]
    hierarchy = package.receipt["hierarchy"]["manifest"]
    return {
        "schema": SCHEMA,
        "status": "running",
        "decision": None,
        "scope": "a065_vulkan_static_authored_capture_prerequisite_only",
        "attempt": 1,
        "retry_policy": "none",
        "s1_promotion": False,
        "s2_s5_unlocked": False,
        "started_at_utc": utc_now(),
        "repository": repository,
        "admission": admission,
        "contract": {
            "endpoint_id": ENDPOINT_ID,
            "backend": ENDPOINT_BACKEND,
            "resolution": {"width": FORMAL_SIZE[0], "height": FORMAL_SIZE[1]},
            "required_order_backends": list(REQUIRED_ORDERS),
            "required_cuts": list(REQUIRED_CUTS),
            "authored_views": [0, 1],
            "warmup_frames": WARMUP_FRAMES,
            "max_thermal_status": MAX_THERMAL_STATUS,
            "implemented_capture": "one_static_authored_view_per_process",
            "formal_image_qualification": False,
        },
        "deferred_gates": {
            "formal_image_gate": {
                "decision": "Deferred",
                "reason": PIXELCOPY_IMAGE_GATE_REASON,
            },
            "moving_same_session_capture": {
                "decision": "Deferred",
                "required_trace": list(MOVING_TRACE),
                "reason": MOVING_CAPTURE_REASON,
            },
            "replacement_same_session_capture": {
                "decision": "Deferred",
                "required_cuts": list(REPLACEMENT_CUTS),
                "reason": REPLACEMENT_CAPTURE_REASON,
            },
        },
        "authority": {
            "author_receipt": {
                "path": os.fspath(package.receipt_path),
                "sha256": package.receipt_sha256,
                "schema": AUTHOR_SCHEMA,
            },
            "source": authority["source"],
            "builder": authority["builder"],
            "hierarchy_manifest": hierarchy,
            "coverage_semantics": "offline_author_receipt_join_not_runtime_coverage_generation",
        },
        "cuts": [
            {
                "name": name,
                "coverage_sha256": package.cuts[name].coverage_sha256,
                "coverage": package.cuts[name].coverage,
                "render_input": package.cuts[name].render_input,
            }
            for name in REQUIRED_CUTS
        ],
        "static_captures": [],
        "completed_child_commands": 0,
        "planned_child_commands": len(capture_specs()) * 2,
    }


def run_collection(args: argparse.Namespace, package: AuthorPackage, output: pathlib.Path) -> dict[str, Any]:
    formal_preflight = validate_formal_preflight()
    repository = repository_identity()
    admission = admit_device(args, package, output)
    artifact = endpoint_artifact_header(package, repository, admission)
    artifact["formal_preflight"] = formal_preflight
    atomic_write_json(output / "capture.json", artifact)

    for spec in capture_specs():
        lane_evidence: dict[str, LaneEvidence] = {}
        for lane in ("exact", "proxy"):
            raw_output = output / "raw" / spec.slug / lane
            raw_output.parent.mkdir(parents=True, exist_ok=True)
            command = collector_command(args, spec, lane, package, raw_output)
            completed = subprocess.run(command, cwd=REPO_ROOT, check=False)
            if completed.returncode != 0:
                raise RejectedCollection(
                    f"child Android collector failed once for {spec.pair_id}/{lane} "
                    f"with exit {completed.returncode}; no retry was attempted"
                )
            expected_path = package.source_path if lane == "exact" else package.cuts[spec.cut_name].input_path
            expected_active = (
                FORMAL_SOURCE["splat_count"]
                if lane == "exact"
                else package.cuts[spec.cut_name].active_splats
            )
            try:
                lane_evidence[lane] = read_lane_evidence(
                    raw_output, spec, lane, expected_path, expected_active
                )
            except DeferredCollection as error:
                raise RejectedCollection(
                    f"{spec.pair_id}/{lane} evidence is unavailable after its "
                    f"single child launch: {error}"
                ) from error
            if (
                lane_evidence[lane].environment_receipt
                != admission["android_device_receipt"]
            ):
                raise RejectedCollection(
                    f"{spec.pair_id}/{lane} A065 identity drifted after admission"
                )
            artifact["completed_child_commands"] += 1
            atomic_write_json(output / "capture.json", artifact)

        static_capture = retain_static_pair(
            spec,
            package.cuts[spec.cut_name],
            lane_evidence["exact"],
            lane_evidence["proxy"],
            output,
        )
        artifact["static_captures"].append(static_capture)
        atomic_write_json(output / "capture.json", artifact)
    artifact.update(
        {
            "status": "complete",
            "decision": "Deferred",
            "pass": False,
            "reason": (
                "static authored PixelCopy captures are retained, but renderer-owned "
                "same-present image identity and same-session moving capture are unavailable"
            ),
            "ended_at_utc": utc_now(),
            "limitations": [
                "This artifact covers only Nothing A065 Vulkan; Apple M4 Metal remains separate.",
                "This artifact cannot pass the formal image or temporal gates.",
                "PixelCopy PNGs are diagnostic captures without renderer same-present identity.",
                "No moving or replacement sequence is synthesized from separate processes.",
                "Coverage is joined from the offline author receipt and is not an S4 runtime generation.",
                "Every proxy raw benchmark retains its actual materialized PLY P as dataset authority.",
                "Deferred does not unlock S2-S5.",
            ],
        }
    )
    atomic_write_json(output / "capture.json", artifact)
    return artifact


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--serial", required=True, help="exact A065 adb serial")
    result.add_argument(
        "--author-receipt",
        required=True,
        type=pathlib.Path,
        help="retained S1a cut-receipt.json",
    )
    result.add_argument("--output", required=True, type=pathlib.Path, help="fresh output root")
    result.add_argument("--apk", type=pathlib.Path, help="exact preinstalled debuggable APK")
    result.add_argument("--adb", type=pathlib.Path, help="adb executable")
    result.add_argument("--doctor-timeout-seconds", type=float, default=120.0)
    result.add_argument("--thermal-timeout-seconds", type=float, default=300.0)
    result.add_argument("--thermal-poll-seconds", type=float, default=5.0)
    result.add_argument("--run-timeout-seconds", type=float, default=180.0)
    result.add_argument(
        "--dry-run",
        action="store_true",
        help="validate local inputs and print the immutable child schedule only",
    )
    return result


def validate_args(args: argparse.Namespace) -> pathlib.Path:
    args.serial = args.serial.strip()
    if not args.serial or any(character.isspace() for character in args.serial):
        raise ValueError("--serial must be one non-empty adb serial")
    args.author_receipt = args.author_receipt.expanduser().resolve()
    args.output = args.output.expanduser().resolve()
    if args.output.exists():
        raise ValueError(f"output already exists; refusing to overwrite: {args.output}")
    if args.apk is not None:
        args.apk = args.apk.expanduser().resolve()
        if not args.dry_run and not args.apk.is_file():
            raise ValueError(f"APK does not exist: {args.apk}")
    if args.adb is not None:
        args.adb = args.adb.expanduser().resolve()
        if not args.dry_run and not args.adb.is_file():
            raise ValueError(f"adb does not exist: {args.adb}")
    for field in (
        "doctor_timeout_seconds",
        "thermal_timeout_seconds",
        "thermal_poll_seconds",
        "run_timeout_seconds",
    ):
        value = getattr(args, field)
        if not isinstance(value, (int, float)) or value <= 0:
            raise ValueError(f"--{field.replace('_', '-')} must be positive")
    return args.output


def dry_run(args: argparse.Namespace, package: AuthorPackage, output: pathlib.Path) -> None:
    doctor, _ = doctor_environment(args, package, output)
    print(f"output_root={output}")
    print(f"attempt=1 retry_policy=none")
    print(f"formal_image_gate=Deferred reason={PIXELCOPY_IMAGE_GATE_REASON}")
    print(f"moving_same_session_capture=Deferred reason={MOVING_CAPTURE_REASON}")
    print(f"doctor={ANDROID.command_text(doctor)}")
    print(f"adb_serial={args.serial} thermal_max={MAX_THERMAL_STATUS}")
    for spec in capture_specs():
        for lane in ("exact", "proxy"):
            raw_output = output / "raw" / spec.slug / lane
            print(
                f"pair={spec.pair_id} lane={lane} trace_frame={spec.trace_frame_index} "
                f"measured_frames={spec.measured_frames}"
            )
            print(ANDROID.command_text(collector_command(args, spec, lane, package, raw_output)))


def terminal_receipt(
    output: pathlib.Path,
    decision: str,
    reason: str,
    *,
    collection_started: bool,
) -> dict[str, Any]:
    value = {
        "schema": SCHEMA,
        "status": "terminal",
        "decision": decision,
        "pass": False,
        "reason": reason,
        "attempt": 1,
        "retry_policy": "none",
        "collection_started": collection_started,
        "s1_promotion": False,
        "s2_s5_unlocked": False,
        "ended_at_utc": utc_now(),
    }
    atomic_write_json(output / "decision.json", value)
    return value


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        output = validate_args(args)
    except (OSError, ValueError) as error:
        parser().error(str(error))

    if args.dry_run:
        try:
            package = validate_author_package(args.author_receipt)
            dry_run(args, package, output)
        except (CollectionError, OSError, ValueError) as error:
            parser().error(str(error))
        return 0

    try:
        claim_output(output)
    except (OSError, ValueError) as error:
        parser().error(str(error))

    try:
        package = validate_author_package(args.author_receipt)
        artifact = run_collection(args, package, output)
        atomic_write_json(
            output / "decision.json",
            {
                "schema": SCHEMA,
                "status": "terminal",
                "decision": artifact["decision"],
                "pass": False,
                "reason": artifact["reason"],
                "attempt": 1,
                "retry_policy": "none",
                "collection_started": True,
                "s1_promotion": False,
                "s2_s5_unlocked": False,
                "ended_at_utc": artifact["ended_at_utc"],
            },
        )
        print(f"artifact={output / 'capture.json'}")
        return 2
    except DeferredCollection as error:
        print(
            json.dumps(
                terminal_receipt(
                    output,
                    "Deferred",
                    str(error),
                    collection_started=(output / "raw").exists(),
                ),
                sort_keys=True,
            )
        )
        return 2
    except (
        RejectedCollection,
        OSError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
    ) as error:
        print(
            json.dumps(
                terminal_receipt(
                    output,
                    "Rejected",
                    str(error),
                    collection_started=(output / "raw").exists(),
                ),
                sort_keys=True,
            )
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
