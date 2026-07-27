#!/usr/bin/env python3
"""One-shot five-pair Q1 same-Chrome WebGPU Truck series orchestrator.

The default-safe surface is ``--dry-run``.  ``--execute`` is admitted only for
an exact reviewed clean commit and never retries an endpoint invocation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import random
import shutil
import subprocess
import sys
from collections.abc import Mapping
from datetime import datetime, timezone
from typing import Any


SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
sys.path.insert(0, str(SCRIPT_DIR))

from q1_pair_admission.artifacts import (  # noqa: E402
    HOST_ADMISSION_JOIN_SCHEMA,
    IMAGE_TOOL_SHA256,
    artifact as admit_artifact,
    frozen_rgba8_png_receipt,
)
from q1_pair_admission.common import (  # noqa: E402
    ValidationError,
    canonical_sha256,
    file_sha256,
    utc,
)
from q1_pair_admission.contract import (  # noqa: E402
    HEIGHT,
    IMAGE_SCHEMA,
    MEASURED,
    SCHEMA,
    TRACE,
    TRUCK,
    WARMUP,
    WIDTH,
)


PLAN_SCHEMA = "gsplat-q1-truck-paired-series-plan/v1"
COMMANDS_SCHEMA = "gsplat-q1-truck-paired-command-receipt/v1"
BLOCKER_SCHEMA = "gsplat-q1-truck-paired-orchestrator-blocker/v1"
FORMAL_INPUTS_SCHEMA = "gsplat-q1-truck-formal-inputs/v1"
FORMAL_LOCK_SCHEMA = "gsplat-q1-truck-formal-execution-lock/v1"
POST_RUN_SCHEMA = "gsplat-q1-truck-post-run-verification/v1"
PLAYCANVAS_REQUEST_SCHEMA = "gsplat-q1-playcanvas-producer-request/v1"
MINIMUM_SSIM = 0.99
ENDPOINTS = ("playcanvas", "gsplat_rs")
TRACE_INDICES = (0, 1)
GSPLAT_QUALIFICATION = "truck-quality-1080p-fixed-gpu-preproject-compact-v1"
TRACE_URL = "/tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json"
IMAGE_TOOL = pathlib.Path("tests/perf/compare-image-ssim.mjs")

SAFE_HOST_ENVIRONMENT = ("PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE")
PROCESS_TIMEOUTS_SECONDS = {
    # These are safety bounds, not performance gates. They are intentionally
    # wider than the expected duration and a timeout always terminates the
    # one-shot attempt without retrying it.
    "git_helper": 120,
    "producer": 1800,
    "canonical_validator": 300,
    "image_comparison": 600,
    "final_validator": 600,
}
PUPPETEER_GRAPH_SCHEMA = "gsplat-q1-puppeteer-production-modules/v1"
IGNORED_MODULE_TREE_PARTS = frozenset({
    ".cache", "coverage", "docs", "examples", "test", "tests", "tmp",
    "__pycache__", ".DS_Store",
})
LOCKED_REPOSITORY_FILES = (
    "tests/perf/collect-q1-truck-paired-series.py",
    "tests/perf/validate-q1-truck-paired-comparison.py",
    "tests/perf/validate-benchmark-artifacts.py",
    "tests/perf/validate-balanced-image-gate.py",
    "tests/perf/compare-image-ssim.mjs",
    "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
    "tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
    "tests/competitive/playcanvas/package.json",
    "tests/competitive/playcanvas/package-lock.json",
    "tests/competitive/playcanvas/expected-engine.json",
)
LOCKED_REPOSITORY_TREES = (
    "tests/perf/q1_pair_admission",
    "tests/competitive/playcanvas/scripts",
    "tests/competitive/playcanvas/public",
    "tests/competitive/playcanvas/node_modules/playcanvas/build/playcanvas",
    "tests/competitive/playcanvas/node_modules/puppeteer-core",
    "examples/web/scripts",
    "examples/web/src",
)


class OrchestrationError(RuntimeError):
    """A finite preflight, producer, materialization, or admission failure."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise OrchestrationError(message)


def json_bytes(value: Any) -> bytes:
    return f"{json.dumps(value, indent=2, sort_keys=True)}\n".encode()


def write_new_json(path: pathlib.Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as handle:
        handle.write(json_bytes(value))
        handle.flush()
        os.fsync(handle.fileno())


def load_object(path: pathlib.Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise OrchestrationError(f"cannot read {label}: {error}") from error
    require(isinstance(value, dict), f"{label} must contain an object")
    return value


def sha256_path(path: pathlib.Path) -> str:
    try:
        return file_sha256(path)
    except Exception as error:  # q1 helper raises its own validation type
        raise OrchestrationError(str(error)) from error


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def safe_host_environment(source: Mapping[str, str] | None = None) -> dict[str, str]:
    """Return the complete inherited environment allowed in producer children."""

    values = os.environ if source is None else source
    result = {
        key: values[key]
        for key in SAFE_HOST_ENVIRONMENT
        if key in values and values[key]
    }
    require("PATH" in result and "HOME" in result, "formal producer environment requires PATH and HOME")
    return result


def child_base_environment(root: pathlib.Path) -> dict[str, str]:
    result = safe_host_environment()
    result["HOME"] = str(root / "process-home")
    return result


def repository_file_receipt(relative_path: str) -> dict[str, Any]:
    path = REPO_ROOT / relative_path
    require(path.is_file(), f"locked repository input is unavailable: {relative_path}")
    return {
        "path": relative_path,
        "bytes": path.stat().st_size,
        "sha256": sha256_path(path),
    }


def repository_tree_receipt(relative_path: str) -> dict[str, Any]:
    root = REPO_ROOT / relative_path
    require(root.is_dir(), f"locked repository input tree is unavailable: {relative_path}")
    entries = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or "__pycache__" in path.parts or path.suffix == ".pyc":
            continue
        entries.append({
            "path": path.relative_to(root).as_posix(),
            "bytes": path.stat().st_size,
            "sha256": sha256_path(path),
        })
    require(entries, f"locked repository input tree is empty: {relative_path}")
    return {
        "path": relative_path,
        "file_count": len(entries),
        "bytes": sum(entry["bytes"] for entry in entries),
        "sha256": canonical_sha256(entries),
    }


def module_tree_receipt(root: pathlib.Path, lock_path: str) -> dict[str, Any]:
    """Hash installed production module bytes without caches/tests/temp files."""

    require(root.is_dir(), f"locked production module is unavailable: {lock_path}")
    entries = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if (
            not path.is_file()
            or any(part in IGNORED_MODULE_TREE_PARTS for part in relative.parts)
            or path.suffix == ".pyc"
        ):
            continue
        entries.append({
            "path": relative.as_posix(),
            "bytes": path.stat().st_size,
            "sha256": sha256_path(path),
        })
    require(entries, f"locked production module is empty: {lock_path}")
    return {
        "path": lock_path,
        "lock_path": lock_path,
        "file_count": len(entries),
        "bytes": sum(entry["bytes"] for entry in entries),
        "sha256": canonical_sha256(entries),
    }


def resolve_lock_dependency(packages: dict[str, Any], parent: str, name: str) -> str:
    """Resolve one npm lockfile dependency with Node's ancestor lookup order."""

    prefix: str | None = parent
    while prefix is not None:
        candidate = f"{prefix}/node_modules/{name}"
        if candidate in packages:
            return candidate
        marker = prefix.rfind("/node_modules/")
        prefix = prefix[:marker] if marker >= 0 else None
    root_candidate = f"node_modules/{name}"
    if root_candidate in packages:
        return root_candidate
    raise OrchestrationError(f"package-lock cannot resolve production dependency {name!r} from {parent!r}")


def puppeteer_production_modules(playcanvas_root: pathlib.Path) -> dict[str, Any]:
    """Lock the installed production dependency closure rooted at puppeteer-core."""

    lock_path = playcanvas_root / "package-lock.json"
    lock = load_object(lock_path, "PlayCanvas package-lock")
    packages = lock.get("packages")
    require(isinstance(packages, dict), "PlayCanvas package-lock lacks packages")
    root_key = "node_modules/puppeteer-core"
    require(isinstance(packages.get(root_key), dict), "package-lock lacks puppeteer-core")
    pending = [root_key]
    visited: set[str] = set()
    receipts = []
    while pending:
        package_key = pending.pop()
        if package_key in visited:
            continue
        visited.add(package_key)
        metadata = packages.get(package_key)
        require(isinstance(metadata, dict), f"package-lock entry is invalid: {package_key}")
        dependencies = metadata.get("dependencies", {})
        require(isinstance(dependencies, dict), f"package-lock dependencies are invalid: {package_key}")
        for name in sorted(dependencies):
            pending.append(resolve_lock_dependency(packages, package_key, name))
        receipt = module_tree_receipt(playcanvas_root / package_key, package_key)
        receipts.append({
            **receipt,
            "version": metadata.get("version"),
            "integrity": metadata.get("integrity"),
        })
    receipts.sort(key=lambda value: value["lock_path"])
    return {
        "schema": PUPPETEER_GRAPH_SCHEMA,
        "package_lock_sha256": sha256_path(lock_path),
        "root": root_key,
        "package_count": len(receipts),
        "packages": receipts,
        "sha256": canonical_sha256(receipts),
    }


def capture_formal_inputs(args: argparse.Namespace) -> dict[str, Any]:
    reference_receipts = []
    for trace in TRACE_INDICES:
        path = args.reference_images[trace]
        try:
            receipt = frozen_rgba8_png_receipt(path, f"reference trace {trace}")
        except (OSError, ValidationError) as error:
            raise OrchestrationError(f"reference trace {trace} is not frozen RGBA8 1920x1080: {error}") from error
        reference_receipts.append({
            "trace_frame_index": trace,
            "source_path": str(path),
            "series_path": f"reference/trace-{trace}.png",
            **receipt,
        })
    wasm_files = []
    for name in ("gsplat_web.js", "gsplat_web_bg.wasm", "gsplat_web_build_receipt.json"):
        path = args.gsplat_wasm_package / name
        require(path.is_file(), f"WASM package lacks {name}")
        wasm_files.append({
            "name": name,
            "path": (args.gsplat_wasm_package.relative_to(REPO_ROOT) / name).as_posix(),
            "bytes": path.stat().st_size,
            "sha256": sha256_path(path),
        })
    host = safe_host_environment()
    toolchains = []
    for name, executable in (
        ("node", shutil.which("node", path=host["PATH"])),
        ("python", sys.executable),
    ):
        require(executable is not None, f"formal producer toolchain lacks {name}")
        path = pathlib.Path(executable).resolve()
        require(path.is_file() and os.access(path, os.X_OK), f"formal {name} is not executable")
        toolchains.append({
            "name": name,
            "path": str(path),
            "bytes": path.stat().st_size,
            "sha256": sha256_path(path),
        })
    return {
        "schema": FORMAL_INPUTS_SCHEMA,
        "reviewed_commit": args.reviewed_sha,
        "git": {"head": git_output("rev-parse", "HEAD"), "clean": True},
        "browser": {
            "path": str(args.chrome),
            "bytes": args.chrome.stat().st_size,
            "sha256": sha256_path(args.chrome),
        },
        "toolchains": toolchains,
        "wasm_package": {
            "path": args.gsplat_wasm_package.relative_to(REPO_ROOT).as_posix(),
            "files": wasm_files,
        },
        "repository_files": [
            repository_file_receipt(path) for path in LOCKED_REPOSITORY_FILES
        ],
        "repository_trees": [
            repository_tree_receipt(path) for path in LOCKED_REPOSITORY_TREES
        ],
        "puppeteer_production_modules": puppeteer_production_modules(
            REPO_ROOT / "tests/competitive/playcanvas"
        ),
        "references": reference_receipts,
    }


def verify_formal_inputs(args: argparse.Namespace, expected: dict[str, Any]) -> None:
    require(not git_output("status", "--porcelain"), "repository became dirty during Q1 execution")
    observed = capture_formal_inputs(args)
    require(
        observed == expected,
        "reviewed commit, browser, WASM, producer, validator, runtime, dataset, trace, or reference input drifted",
    )


def schedule_orders(seed: int) -> list[str]:
    """Return a deterministic, exactly-five, counterbalanced AB/BA schedule."""

    playcanvas_first = "playcanvas-first"
    gsplat_first = "gsplat-rs-first"
    orders = [playcanvas_first, playcanvas_first, gsplat_first, gsplat_first]
    odd = hashlib.sha256(f"gsplat-q1-truck-pairs/{seed}".encode()).digest()[0] & 1
    orders.append(playcanvas_first if odd == 0 else gsplat_first)
    random.Random(seed).shuffle(orders)
    require(
        set(orders) == {playcanvas_first, gsplat_first}
        and abs(orders.count(playcanvas_first) - orders.count(gsplat_first)) <= 1,
        "generated schedule is not counterbalanced",
    )
    return orders


def protocol() -> dict[str, Any]:
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
        "quality_gate": {
            "metric": "ssim-luma-srgb-window8",
            "minimum_ssim": MINIMUM_SSIM,
        },
    }


def endpoint_position(order: str, endpoint: str) -> int:
    return 1 if (order == "playcanvas-first") == (endpoint == "playcanvas") else 2


def pairing(
    *, series_id: str, schedule_sha: str, pair_id: str, order: str, endpoint: str
) -> dict[str, Any]:
    return {
        "series_id": series_id,
        "schedule_sha256": schedule_sha,
        "pair_id": pair_id,
        "run_order": order,
        "position": endpoint_position(order, endpoint),
        "fresh_output": True,
        "automatic_retry": False,
    }


def relative(root: pathlib.Path, path: pathlib.Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as error:
        raise OrchestrationError(f"path escapes series root: {path}") from error


def common_configuration() -> dict[str, Any]:
    return {
        "dataset": TRUCK,
        "trace": TRACE,
        "display": {"width": WIDTH, "height": HEIGHT, "dpr": 1},
        "warmup_frames": WARMUP,
        "measured_frames": MEASURED,
        "camera_mode": "trace_sequence",
        "terminal_window": "gpu_queue_on_submitted_work_done",
        "source_membership": "all",
        "source_sh_degree": 3,
        "lod": "disabled",
        "sampling": "disabled",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
    }


def playcanvas_request(
    *, role: str, trace: int | None, context: dict[str, Any], bindings: Any = None
) -> dict[str, Any]:
    value = {
        "schema": PLAYCANVAS_REQUEST_SCHEMA,
        "artifact_role": role,
        "series_id": context["series_id"],
        "schedule_sha256": context["schedule_sha256"],
        "protocol_sha256": context["protocol_sha256"],
        "configuration_sha256": context["configuration_sha256"],
        "pair_id": context["pair_id"],
        "run_order": context["run_order"],
        "position": context["position"],
        "collection_session_id": context["collection_session_id"],
    }
    if role == "control":
        value["capture_trace_frame_index"] = trace
    else:
        value["control_bindings"] = bindings
    return value


def make_invocation(
    *,
    sequence: int,
    root: pathlib.Path,
    repo_root: pathlib.Path,
    pair_id: str,
    order: str,
    endpoint: str,
    role: str,
    trace: int | None,
    protocol_sha: str,
    schedule_sha: str,
    configuration_sha: str,
    series_id: str,
    collection_session_id: str,
    chrome: pathlib.Path,
    wasm_package: pathlib.Path,
    gsplat_port: int,
) -> dict[str, Any]:
    role_name = f"control-trace-{trace}" if role == "control" else "throughput"
    invocation_id = f"{sequence:02d}-{pair_id}-{endpoint}-{role_name}"
    artifact = root / "pairs" / pair_id / endpoint / role_name
    pairing_value = pairing(
        series_id=series_id,
        schedule_sha=schedule_sha,
        pair_id=pair_id,
        order=order,
        endpoint=endpoint,
    )
    context = {
        **pairing_value,
        "protocol_sha256": protocol_sha,
        "configuration_sha256": configuration_sha,
        "collection_session_id": collection_session_id,
    }
    admission = {
        "series_id": series_id,
        "schedule_sha256": schedule_sha,
        "protocol_sha256": protocol_sha,
        "configuration_sha256": configuration_sha,
        "pair_id": pair_id,
        "run_order": order,
        "position": pairing_value["position"],
        "collection_session_id": collection_session_id,
        "reviewed_commit": None,
        "predeclared_at_utc": None,
    }
    dynamic_inputs: dict[str, Any] | None = None
    if endpoint == "playcanvas":
        request = root / "requests" / f"{invocation_id}.json"
        if role == "throughput":
            dynamic_inputs = {
                "resolved_before_invocation": True,
                "request_path": relative(root, request),
                "control_bindings": [
                    {
                        "trace_frame_index": control_trace,
                        "manifest": (
                            f"pairs/{pair_id}/playcanvas/control-trace-{control_trace}/manifest.json"
                        ),
                        "fields": [
                            "run_id",
                            "manifest_sha256",
                            "configuration_sha256",
                        ],
                    }
                    for control_trace in TRACE_INDICES
                ],
            }
        environment = {
            **child_base_environment(root),
            "CHROME_PATH": str(chrome),
            "HEADLESS": "0",
            "PHASE_E_QUALIFICATION": "truck-quality-1080p-v1",
            "PLAYCANVAS_CAMERA_MODE": "sequence",
            "PLAYCANVAS_WARMUP_FRAMES": str(WARMUP),
            "PLAYCANVAS_MEASURED_FRAMES": str(MEASURED),
            "PLAYCANVAS_VIEWPORT_WIDTH": str(WIDTH),
            "PLAYCANVAS_VIEWPORT_HEIGHT": str(HEIGHT),
            "PLAYCANVAS_Q1_SERIES_ROOT": str(root),
            "PLAYCANVAS_Q1_PRODUCER_REQUEST": str(request),
            "PLAYCANVAS_ARTIFACT_DIR": str(artifact),
            **(
                {"PLAYCANVAS_CAPTURE_TRACE_FRAME": str(trace)}
                if role == "control"
                else {}
            ),
        }
        argv = ["node", "tests/competitive/playcanvas/scripts/run-timed-benchmark.mjs"]
        producer_request = playcanvas_request(
            role=role,
            trace=trace,
            context=context,
            bindings=None if role == "throughput" else None,
        )
    else:
        run_context = root / "run-contexts" / f"{invocation_id}.json"
        environment = {
            **child_base_environment(root),
            "CHROME_PATH": str(chrome),
            "HEADLESS": "0",
            "GSPLAT_PHASE_E_QUALIFICATION": GSPLAT_QUALIFICATION,
            "GSPLAT_TRUCK_QUALIFICATION_STAGE": role,
            "GSPLAT_DATASET": "truck",
            "GSPLAT_ARTIFACT_DIR": str(artifact),
            "GSPLAT_GEOMETRY_PATH": "packed",
            "GSPLAT_ORDER_BACKEND": "gpu",
            "GSPLAT_PROJECTED_POLICY": "compact",
            "GSPLAT_SORT_INTERVAL": "1",
            "GSPLAT_BENCHMARK_WARMUP_FRAMES": str(WARMUP),
            "GSPLAT_BENCHMARK_FRAMES": str(MEASURED),
            "GSPLAT_CAMERA_TRACE_URL": TRACE_URL,
            "GSPLAT_CAMERA_TRACE_SEQUENCE": "1",
            "GSPLAT_CAMERA_TRACE_LOOPS": "1",
            "GSPLAT_CAMERA_FRAME_INDICES": "0,1",
            "GSPLAT_Q1_ARTIFACT_ROLE": role,
            "GSPLAT_Q1_PROTOCOL_SHA256": protocol_sha,
            "GSPLAT_Q1_WASM_PACKAGE_DIR": str(wasm_package),
            "GSPLAT_Q1_RUN_CONTEXT": str(run_context),
            "GSPLAT_Q1_COLLECTION_SESSION_ID": collection_session_id,
            "GSPLAT_Q1_SERIES_ROOT": str(root),
            "GSPLAT_HTTP_PORT": str(gsplat_port),
            "GSPLAT_ORDER_COMPLETION_PROTOCOL": (
                "isolated_terminal" if role == "control" else "sustained_window"
            ),
            "GSPLAT_BENCHMARK_WINDOW_MODE": (
                "current_stats_evidence_window"
                if role == "control"
                else "terminal_queue_throughput_window"
            ),
            **(
                {"GSPLAT_Q1_CAPTURE_TRACE_FRAME": str(trace)}
                if role == "control"
                else {
                    "GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT": str(
                        root / "pairs" / pair_id / endpoint / "control-trace-0"
                    ),
                    "GSPLAT_Q1_CONTROL_ARTIFACT_0": str(
                        root / "pairs" / pair_id / endpoint / "control-trace-0"
                    ),
                    "GSPLAT_Q1_CONTROL_ARTIFACT_1": str(
                        root / "pairs" / pair_id / endpoint / "control-trace-1"
                    ),
                }
            ),
        }
        argv = ["node", "examples/web/scripts/collect-web-benchmark-artifact.mjs"]
        producer_request = {
            "pairing": pairing_value,
            "configuration_sha256": configuration_sha,
        }
        request = run_context
        if role == "throughput":
            dynamic_inputs = {
                "resolved_before_invocation": True,
                "control_bindings": [
                    {
                        "trace_frame_index": control_trace,
                        "artifact": (
                            f"pairs/{pair_id}/gsplat_rs/control-trace-{control_trace}"
                        ),
                        "producer_reads_manifest_hash": True,
                    }
                    for control_trace in TRACE_INDICES
                ],
            }
    return {
        "sequence": sequence,
        "invocation_id": invocation_id,
        "pair_id": pair_id,
        "run_order": order,
        "endpoint": endpoint,
        "position": pairing_value["position"],
        "artifact_role": role,
        "trace_frame_index": trace,
        "artifact": relative(root, artifact),
        "request": relative(root, request),
        "request_value": producer_request,
        "admission": admission,
        "cwd": str(repo_root),
        "argv": argv,
        "environment": environment,
        "dynamic_inputs": dynamic_inputs,
        "automatic_retry": False,
        "timeout_seconds": PROCESS_TIMEOUTS_SECONDS["producer"],
    }


def build_plan(args: argparse.Namespace, *, predeclared_at: str) -> dict[str, Any]:
    root = args.series_root.resolve()
    protocol_value = protocol()
    protocol_sha = canonical_sha256(protocol_value)
    references = [
        {
            "trace_frame_index": trace,
            "path": f"reference/trace-{trace}.png",
            "sha256": sha256_path(args.reference_images[trace]),
        }
        for trace in TRACE_INDICES
    ]
    orders = schedule_orders(args.seed)
    schedule = {
        "seed": args.seed,
        "predeclared_at_utc": predeclared_at,
        "reference_images": references,
        "pairs": [
            {"pair_id": f"pair-{index:02d}", "run_order": order}
            for index, order in enumerate(orders, 1)
        ],
    }
    schedule_sha = canonical_sha256(schedule)
    configuration_sha = canonical_sha256(common_configuration())
    invocations = []
    sequence = 0
    gsplat_index = 0
    for index, order in enumerate(orders, 1):
        pair_id = f"pair-{index:02d}"
        ordered_endpoints = (
            ("playcanvas", "gsplat_rs")
            if order == "playcanvas-first"
            else ("gsplat_rs", "playcanvas")
        )
        for endpoint in ordered_endpoints:
            for role, trace in (("control", 0), ("control", 1), ("throughput", None)):
                sequence += 1
                if endpoint == "gsplat_rs":
                    gsplat_port = args.gsplat_port_base + gsplat_index
                    gsplat_index += 1
                else:
                    gsplat_port = args.gsplat_port_base
                invocations.append(
                    make_invocation(
                        sequence=sequence,
                        root=root,
                        repo_root=REPO_ROOT,
                        pair_id=pair_id,
                        order=order,
                        endpoint=endpoint,
                        role=role,
                        trace=trace,
                        protocol_sha=protocol_sha,
                        schedule_sha=schedule_sha,
                        configuration_sha=configuration_sha,
                        series_id=args.series_id,
                        collection_session_id=args.collection_session_id,
                        chrome=args.chrome.resolve(),
                        wasm_package=args.gsplat_wasm_package.resolve(),
                        gsplat_port=gsplat_port,
                    )
                )
                invocations[-1]["admission"]["predeclared_at_utc"] = predeclared_at
                invocations[-1]["admission"]["reviewed_commit"] = args.reviewed_sha
    require(len(invocations) == 30, "Q1 plan must contain exactly 30 producer invocations")
    return {
        "schema": PLAN_SCHEMA,
        "series_id": args.series_id,
        "reviewed_commit": args.reviewed_sha,
        "formal_inputs": getattr(args, "formal_inputs", None),
        "collection_session_id": args.collection_session_id,
        "protocol": protocol_value,
        "protocol_sha256": protocol_sha,
        "schedule": schedule,
        "schedule_sha256": schedule_sha,
        "configuration": common_configuration(),
        "configuration_sha256": configuration_sha,
        "invocations": invocations,
        "postprocess": {
            "environment": {
                **child_base_environment(root),
                "CHROME_PATH": str(args.chrome.resolve()),
            },
            "timeout_seconds": {
                "canonical_validator": PROCESS_TIMEOUTS_SECONDS["canonical_validator"],
                "image_comparison": PROCESS_TIMEOUTS_SECONDS["image_comparison"],
                "final_validator": PROCESS_TIMEOUTS_SECONDS["final_validator"],
            },
            "image_comparisons": {
                "count": 20,
                "tool": IMAGE_TOOL.as_posix(),
                "tool_sha256": IMAGE_TOOL_SHA256,
                "threshold_is_decided_only_by_final_validator": True,
            },
            "final_validator": {
                "argv": [
                    sys.executable,
                    "tests/perf/validate-q1-truck-paired-comparison.py",
                    str(root / "schedule.json"),
                    "--output",
                    str(root / "result.json"),
                ],
                "performance_inference_owner": "validator_only",
            },
        },
        "execution_policy": {
            "one_shot": True,
            "automatic_retry": False,
            "stop_on_first_failed_command": True,
            "fresh_series_root": True,
            "formal_execution_requires_reviewed_exact_sha": True,
            "child_environment_is_exact_allowlist": True,
            "inherited_environment_allowlist": list(SAFE_HOST_ENVIRONMENT),
            "timeouts_seconds": PROCESS_TIMEOUTS_SECONDS,
            "timeouts_are_safety_bounds_not_performance_gates": True,
        },
    }


def command_receipt(plan: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": COMMANDS_SCHEMA,
        "series_id": plan["series_id"],
        "reviewed_commit": plan["reviewed_commit"],
        "formal_inputs_sha256": (
            canonical_sha256(plan["formal_inputs"])
            if plan["formal_inputs"] is not None
            else None
        ),
        "schedule_sha256": plan["schedule_sha256"],
        "protocol_sha256": plan["protocol_sha256"],
        "invocation_count": len(plan["invocations"]),
        "postprocess": plan["postprocess"],
        "invocations": [
            {
                key: invocation[key]
                for key in (
                    "sequence",
                    "invocation_id",
                    "pair_id",
                    "run_order",
                    "endpoint",
                    "position",
                    "artifact_role",
                    "trace_frame_index",
                    "artifact",
                    "request",
                    "request_value",
                    "admission",
                    "cwd",
                    "argv",
                    "environment",
                    "dynamic_inputs",
                    "automatic_retry",
                    "timeout_seconds",
                )
            }
            for invocation in plan["invocations"]
        ],
    }


def execution_lock(plan: dict[str, Any], commands: dict[str, Any]) -> dict[str, Any]:
    formal_inputs = plan.get("formal_inputs")
    require(isinstance(formal_inputs, dict), "formal execution lacks its immutable input lock")
    return {
        "schema": FORMAL_LOCK_SCHEMA,
        "series_id": plan["series_id"],
        "reviewed_commit": plan["reviewed_commit"],
        "schedule_sha256": plan["schedule_sha256"],
        "protocol_sha256": plan["protocol_sha256"],
        "formal_inputs": formal_inputs,
        "formal_inputs_sha256": canonical_sha256(formal_inputs),
        "command_receipt": {
            "path": "commands.json",
            "sha256": hashlib.sha256(json_bytes(commands)).hexdigest(),
            "canonical_sha256": canonical_sha256(commands),
            "invocation_count": len(plan["invocations"]),
        },
        "timeouts_seconds": plan["execution_policy"]["timeouts_seconds"],
    }


def git_output(*arguments: str) -> str:
    try:
        completed = subprocess.run(
            ["git", *arguments],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
            env=safe_host_environment(),
            timeout=PROCESS_TIMEOUTS_SECONDS["git_helper"],
        )
    except subprocess.TimeoutExpired as error:
        raise OrchestrationError(
            f"git helper exceeded {PROCESS_TIMEOUTS_SECONDS['git_helper']} second safety timeout"
        ) from error
    require(completed.returncode == 0, completed.stderr.strip() or "git command failed")
    return completed.stdout.strip()


def preflight_execute(args: argparse.Namespace) -> dict[str, Any]:
    require(len(args.reviewed_sha or "") == 40, "--execute requires a full --reviewed-sha")
    require(
        git_output("rev-parse", "HEAD") == args.reviewed_sha,
        "reviewed SHA does not equal the current exact commit",
    )
    require(not git_output("status", "--porcelain"), "formal Q1 execution requires a clean tree")
    require(args.chrome.is_file() and os.access(args.chrome, os.X_OK), "Chrome is not executable")
    require(
        (REPO_ROOT / "examples/web/src/q1-gsplat-producer.mjs").is_file(),
        "gsplat-rs Q1 producer is not integrated at this commit",
    )
    receipt_path = args.gsplat_wasm_package / "gsplat_web_build_receipt.json"
    receipt = load_object(receipt_path, "gsplat-rs WASM build receipt")
    require(
        receipt.get("profile") == "quality-exact"
        and receipt.get("repository_commit") == args.reviewed_sha
        and receipt.get("dirty") is False,
        "WASM package is not a clean quality-exact build of the reviewed commit",
    )
    for name in ("gsplat_web.js", "gsplat_web_bg.wasm"):
        require((args.gsplat_wasm_package / name).is_file(), f"WASM package lacks {name}")
    require(
        (REPO_ROOT / "tests/competitive/playcanvas/node_modules/puppeteer-core").is_dir(),
        "pinned PlayCanvas puppeteer-core is unavailable; run its documented npm ci first",
    )
    require(not args.series_root.exists(), "series root is already claimed")
    require(args.series_root.parent.is_dir(), "series root parent must already exist")
    try:
        in_repository = args.series_root.relative_to(REPO_ROOT)
    except ValueError:
        in_repository = None
    require(
        in_repository is None or in_repository.parts[:1] == ("target",),
        "a repository-local series root must stay below ignored target/",
    )
    try:
        args.gsplat_wasm_package.relative_to(REPO_ROOT)
    except ValueError as error:
        raise OrchestrationError("gsplat-rs WASM package must stay inside the repository") from error
    formal_inputs = capture_formal_inputs(args)
    require(
        formal_inputs["git"] == {"head": args.reviewed_sha, "clean": True},
        "formal input lock does not match the reviewed clean commit",
    )
    return formal_inputs


def claim_series(args: argparse.Namespace, plan: dict[str, Any]) -> dict[str, Any]:
    root = args.series_root
    commands = command_receipt(plan)
    commands_sha = hashlib.sha256(json_bytes(commands)).hexdigest()
    locked = execution_lock(plan, commands)
    root.mkdir()
    for directory in ("reference", "requests", "run-contexts", "logs", "process-home"):
        (root / directory).mkdir()
    for invocation in plan["invocations"]:
        # Producers atomically claim their final artifact directory; only the
        # shared parents may exist before invocation.
        (root / invocation["artifact"]).parent.mkdir(parents=True, exist_ok=True)
    for trace in TRACE_INDICES:
        shutil.copyfile(args.reference_images[trace], root / f"reference/trace-{trace}.png")
        copied = frozen_rgba8_png_receipt(
            root / f"reference/trace-{trace}.png", f"claimed reference trace {trace}"
        )
        expected = next(
            value for value in locked["formal_inputs"]["references"]
            if value["trace_frame_index"] == trace
        )
        require(
            copied == {
                key: expected[key]
                for key in ("sha256", "rgba8_sha256", "width", "height", "pixel_format")
            },
            f"claimed reference trace {trace} drifted before browser work",
        )
    write_new_json(root / "schedule-declaration.json", {
        "schema": SCHEMA,
        "series_id": plan["series_id"],
        "schedule": plan["schedule"],
        "protocol": plan["protocol"],
        "schedule_sha256": plan["schedule_sha256"],
        "protocol_sha256": plan["protocol_sha256"],
        "command_receipt_sha256": commands_sha,
        "evidence_state": "predeclared_before_browser",
    })
    write_new_json(root / "commands.json", commands)
    write_new_json(root / "formal-execution-lock.json", locked)
    write_new_json(root / "series-plan.json", {
        **{key: value for key, value in plan.items() if key != "invocations"},
        "command_receipt_sha256": commands_sha,
    })
    for invocation in plan["invocations"]:
        if invocation["endpoint"] == "playcanvas" and invocation["artifact_role"] == "throughput":
            write_new_json(
                root / "requests" / f"{invocation['invocation_id']}.template.json",
                {
                    **invocation["request_value"],
                    "control_bindings": invocation["dynamic_inputs"]["control_bindings"],
                    "template_only": True,
                },
            )
        else:
            write_new_json(root / invocation["request"], invocation["request_value"])
    # Durable declaration barrier: all immutable receipts exist before browser work.
    directory_fd = os.open(root, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    return locked


def run_once(invocation: dict[str, Any], root: pathlib.Path) -> None:
    try:
        completed = subprocess.run(
            invocation["argv"],
            cwd=REPO_ROOT,
            env=invocation["environment"],
            check=False,
            capture_output=True,
            text=True,
            timeout=invocation["timeout_seconds"],
        )
    except subprocess.TimeoutExpired as error:
        raise OrchestrationError(
            f"{invocation['invocation_id']} exceeded its {invocation['timeout_seconds']} "
            "second producer safety timeout"
        ) from error
    log_root = root / "logs"
    (log_root / f"{invocation['invocation_id']}.stdout.log").write_text(
        completed.stdout, encoding="utf-8"
    )
    (log_root / f"{invocation['invocation_id']}.stderr.log").write_text(
        completed.stderr, encoding="utf-8"
    )
    require(
        completed.returncode == 0,
        f"{invocation['invocation_id']} exited {completed.returncode}",
    )
    validation_environment = {
        key: invocation["environment"][key]
        for key in SAFE_HOST_ENVIRONMENT
        if key in invocation["environment"]
    }
    validate_canonical_artifact(
        root / invocation["artifact"],
        invocation["invocation_id"],
        environment=validation_environment,
    )


def validate_canonical_artifact(
    directory: pathlib.Path, context: str, *, environment: dict[str, str]
) -> None:
    try:
        validate = subprocess.run(
            [sys.executable, "tests/perf/validate-benchmark-artifacts.py", str(directory)],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
            env=environment,
            timeout=PROCESS_TIMEOUTS_SECONDS["canonical_validator"],
        )
    except subprocess.TimeoutExpired as error:
        raise OrchestrationError(
            f"{context} canonical validator exceeded "
            f"{PROCESS_TIMEOUTS_SECONDS['canonical_validator']} second safety timeout"
        ) from error
    require(
        validate.returncode == 0,
        f"{context} canonical validation failed: "
        f"{validate.stderr.strip() or validate.stdout.strip()}",
    )


def resolve_throughput(invocation: dict[str, Any], root: pathlib.Path) -> None:
    pair_id = invocation["pair_id"]
    endpoint = invocation["endpoint"]
    expected = invocation["admission"]
    controls = []
    seen_paths: set[pathlib.Path] = set()
    seen_runs: set[str] = set()
    for trace in TRACE_INDICES:
        relative_directory = pathlib.Path("pairs") / pair_id / endpoint / f"control-trace-{trace}"
        try:
            control = admit_artifact(
                root,
                relative_directory.as_posix(),
                endpoint=endpoint,
                role="control",
                series_id=expected["series_id"],
                schedule_sha=expected["schedule_sha256"],
                protocol_sha=expected["protocol_sha256"],
                pair_id=expected["pair_id"],
                order=expected["run_order"],
                position=expected["position"],
                predeclared=utc(
                    expected["predeclared_at_utc"],
                    f"{invocation['invocation_id']}.predeclared_at_utc",
                ),
                seen_paths=seen_paths,
                seen_runs=seen_runs,
                expected_trace=trace,
            )
        except ValidationError as error:
            raise OrchestrationError(
                f"{invocation['invocation_id']} control {trace} admission failed: {error}"
            ) from error
        require(
            control["configuration"] == expected["configuration_sha256"],
            f"control trace {trace} configuration differs from the declared command",
        )
        require(
            control["commit"] == expected["reviewed_commit"],
            f"control trace {trace} build commit differs from the reviewed command",
        )
        require(
            control["environment"].get("collection_session_id")
            == expected["collection_session_id"],
            f"control trace {trace} collection session differs from the declared command",
        )
        controls.append({
            "trace_frame_index": trace,
            "run_id": control["run_id"],
            "manifest_sha256": control["manifest_sha256"],
            "configuration_sha256": control["configuration"],
        })
    require(
        [control["trace_frame_index"] for control in controls] == list(TRACE_INDICES)
        and all(
            control["configuration_sha256"] == expected["configuration_sha256"]
            for control in controls
        ),
        "controls do not bind the exact declared trace/configuration pair",
    )
    resolution = {
        "schema": "gsplat-q1-control-binding-resolution/v1",
        "invocation_id": invocation["invocation_id"],
        "resolved_at_utc": utc_now(),
        "control_bindings": controls,
    }
    write_new_json(root / "requests" / f"{invocation['invocation_id']}.bindings.json", resolution)
    if endpoint == "playcanvas":
        request = playcanvas_request(
            role="throughput",
            trace=None,
            context=invocation["request_value"],
            bindings=controls,
        )
        write_new_json(root / invocation["request"], request)


def image_source_rgba(endpoint: str, control: pathlib.Path, trace: int) -> str:
    manifest = load_object(control / "manifest.json", f"{endpoint} trace-{trace} manifest")
    if endpoint == "playcanvas":
        capture = manifest.get("renderer_capture")
        require(isinstance(capture, dict), "PlayCanvas control lacks renderer_capture")
        digest = capture.get("rgba8_sha256")
    else:
        frames = []
        for line in (control / "frames.jsonl").read_text(encoding="utf-8").splitlines():
            if line.strip():
                frames.append(json.loads(line))
        captures = [
            frame["capture_depth_precision"]
            for frame in frames
            if frame.get("trace_frame_index") == trace
            and isinstance(frame.get("capture_depth_precision"), dict)
        ]
        require(len(captures) == 1, "gsplat-rs control lacks one renderer capture receipt")
        digest = captures[0].get("rgba8_sha256")
    require(
        isinstance(digest, str) and len(digest) == 64,
        f"{endpoint} trace-{trace} has an invalid renderer RGBA digest",
    )
    return digest


def compare_image(
    *,
    root: pathlib.Path,
    pair_id: str,
    endpoint: str,
    trace: int,
    reference: dict[str, Any],
    environment: dict[str, str],
) -> tuple[pathlib.Path, float]:
    control_relative = pathlib.Path("pairs") / pair_id / endpoint / f"control-trace-{trace}"
    candidate_relative = control_relative / "final-frame.png"
    candidate = root / candidate_relative
    comparison_relative = pathlib.Path("comparisons") / pair_id / endpoint / f"trace-{trace}.json"
    raw_relative = pathlib.Path("comparisons") / pair_id / endpoint / f"trace-{trace}.raw.json"
    raw = root / raw_relative
    raw.parent.mkdir(parents=True, exist_ok=True)
    try:
        completed = subprocess.run(
            [
                "node",
                str(IMAGE_TOOL),
                str(root / reference["path"]),
                str(candidate),
                "--output",
                str(raw),
            ],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
            env=environment,
            timeout=PROCESS_TIMEOUTS_SECONDS["image_comparison"],
        )
    except subprocess.TimeoutExpired as error:
        raise OrchestrationError(
            f"image comparison for {pair_id}.{endpoint}.trace-{trace} exceeded "
            f"{PROCESS_TIMEOUTS_SECONDS['image_comparison']} second safety timeout"
        ) from error
    require(
        completed.returncode == 0,
        f"image comparison failed for {pair_id}.{endpoint}.trace-{trace}: "
        f"{completed.stderr.strip() or completed.stdout.strip()}",
    )
    raw_value = load_object(raw, "raw image comparison")
    browser = raw_value.get("browser")
    require(
        browser == {
            "executablePath": environment.get("CHROME_PATH"),
            "sha256": sha256_path(pathlib.Path(environment["CHROME_PATH"])),
        },
        "image comparison did not use the locked Chrome executable",
    )
    score = raw_value.get("score")
    require(isinstance(score, (int, float)) and 0 <= score <= 1, "image comparison score is invalid")
    receipt = {
        "schema": IMAGE_SCHEMA,
        "metric": "ssim-luma-srgb-window8",
        "tool": IMAGE_TOOL.as_posix(),
        "tool_sha256": IMAGE_TOOL_SHA256,
        "browser_executable_path": browser["executablePath"],
        "browser_executable_sha256": browser["sha256"],
        "trace_frame_index": trace,
        "reference_sha256": reference["sha256"],
        "candidate_sha256": sha256_path(candidate),
        "width": WIDTH,
        "height": HEIGHT,
        "minimum_ssim": MINIMUM_SSIM,
        "score": score,
    }
    comparison = root / comparison_relative
    write_new_json(comparison, receipt)
    return comparison_relative, float(score)


def post_run_verification(
    args: argparse.Namespace,
    plan: dict[str, Any],
    root: pathlib.Path,
    locked: dict[str, Any],
) -> dict[str, Any]:
    verify_formal_inputs(args, locked["formal_inputs"])
    command_path = root / locked["command_receipt"]["path"]
    require(
        sha256_path(command_path) == locked["command_receipt"]["sha256"],
        "immutable command receipt drifted during Q1 execution",
    )
    lock_path = root / "formal-execution-lock.json"
    require(load_object(lock_path, "formal execution lock") == locked, "formal lock file drifted")
    return {
        "schema": POST_RUN_SCHEMA,
        "verified_at_utc": utc_now(),
        "reviewed_commit": plan["reviewed_commit"],
        "git": {"head": git_output("rev-parse", "HEAD"), "clean": True},
        "formal_inputs_sha256": locked["formal_inputs_sha256"],
        "command_receipt_sha256": locked["command_receipt"]["sha256"],
        "formal_lock_sha256": sha256_path(lock_path),
    }


def finalize_schedule(
    args: argparse.Namespace,
    plan: dict[str, Any],
    root: pathlib.Path,
    locked: dict[str, Any],
) -> pathlib.Path:
    references = {
        value["trace_frame_index"]: value for value in plan["schedule"]["reference_images"]
    }
    pairs = []
    for declaration in plan["schedule"]["pairs"]:
        pair_id = declaration["pair_id"]
        order = declaration["run_order"]
        pair: dict[str, Any] = {"pair_id": pair_id, "run_order": order}
        for endpoint in ENDPOINTS:
            controls = []
            images = []
            for trace in TRACE_INDICES:
                control_relative = pathlib.Path("pairs") / pair_id / endpoint / f"control-trace-{trace}"
                control = root / control_relative
                manifest_sha = sha256_path(control / "manifest.json")
                controls.append({
                    "trace_frame_index": trace,
                    "artifact": control_relative.as_posix(),
                    "manifest_sha256": manifest_sha,
                })
                comparison, _ = compare_image(
                    root=root,
                    pair_id=pair_id,
                    endpoint=endpoint,
                    trace=trace,
                    reference=references[trace],
                    environment=plan["postprocess"]["environment"],
                )
                image_relative = control_relative / "final-frame.png"
                image_sha = sha256_path(root / image_relative)
                images.append({
                    "trace_frame_index": trace,
                    "path": image_relative.as_posix(),
                    "sha256": image_sha,
                    "comparison": comparison.as_posix(),
                    "producer_artifact": {
                        "path": control_relative.as_posix(),
                        "manifest_sha256": manifest_sha,
                    },
                    "host_admission_join": {
                        "schema": HOST_ADMISSION_JOIN_SCHEMA,
                        "source": "decoded_png_rgba8_to_renderer_receipt",
                        "pixel_format": "rgba8unorm-srgb",
                        "width": WIDTH,
                        "height": HEIGHT,
                        "producer_artifact_path": control_relative.as_posix(),
                        "producer_manifest_sha256": manifest_sha,
                        "source_rgba8_sha256": image_source_rgba(endpoint, control, trace),
                        "png_sha256": image_sha,
                    },
                })
            pair[endpoint] = {
                "position": endpoint_position(order, endpoint),
                "controls": controls,
                "throughput": f"pairs/{pair_id}/{endpoint}/throughput",
                "images": images,
            }
        pairs.append(pair)
    # All producers and image comparisons are complete. Recheck every frozen
    # executable/module/reference input before publishing the evidence schedule.
    post_run = post_run_verification(args, plan, root, locked)
    schedule_path = root / "schedule.json"
    write_new_json(schedule_path, {
        "schema": SCHEMA,
        "series_id": plan["series_id"],
        "schedule": plan["schedule"],
        "protocol": plan["protocol"],
        "orchestration": {
            "formal_execution_lock": locked,
            "post_run_verification": post_run,
        },
        "pairs": pairs,
    })
    return schedule_path


def publish_blocker(root: pathlib.Path, plan: dict[str, Any] | None, error: BaseException) -> None:
    blocker = root / "blocker.json"
    if not root.is_dir() or blocker.exists():
        return
    try:
        write_new_json(blocker, {
            "schema": BLOCKER_SCHEMA,
            "series_id": None if plan is None else plan["series_id"],
            "failed_at_utc": utc_now(),
            "reason": str(error),
            "automatic_retry": False,
            "retry_authorized": False,
        })
    except OSError:
        pass


def execute(args: argparse.Namespace, plan: dict[str, Any]) -> int:
    root = args.series_root
    try:
        locked = claim_series(args, plan)
        for invocation in plan["invocations"]:
            if invocation["artifact_role"] == "throughput":
                resolve_throughput(invocation, root)
            run_once(invocation, root)
        schedule_path = finalize_schedule(args, plan, root, locked)
        try:
            result = subprocess.run(
                [
                    sys.executable,
                    "tests/perf/validate-q1-truck-paired-comparison.py",
                    str(schedule_path),
                    "--output",
                    str(root / "result.json"),
                ],
                cwd=REPO_ROOT,
                check=False,
                env=plan["postprocess"]["environment"],
                timeout=PROCESS_TIMEOUTS_SECONDS["final_validator"],
            )
        except subprocess.TimeoutExpired as error:
            raise OrchestrationError(
                "final Q1 validator exceeded "
                f"{PROCESS_TIMEOUTS_SECONDS['final_validator']} second safety timeout"
            ) from error
        require(result.returncode == 0, f"final Q1 validator exited {result.returncode}")
        return 0
    except BaseException as error:
        publish_blocker(root, plan, error)
        raise


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--dry-run",
        "--print-only",
        dest="dry_run",
        action="store_true",
        help="print the immutable plan only",
    )
    mode.add_argument("--execute", action="store_true", help="execute one reviewed finite attempt")
    parser.add_argument("--series-root", type=pathlib.Path, required=True)
    parser.add_argument("--series-id", required=True)
    parser.add_argument("--collection-session-id", required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--chrome", type=pathlib.Path, required=True)
    parser.add_argument("--gsplat-wasm-package", type=pathlib.Path, required=True)
    parser.add_argument("--reference-trace-0", type=pathlib.Path, required=True)
    parser.add_argument("--reference-trace-1", type=pathlib.Path, required=True)
    parser.add_argument("--reviewed-sha")
    parser.add_argument("--gsplat-port-base", type=int, default=43000)
    args = parser.parse_args(argv)
    args.series_root = args.series_root.resolve()
    args.chrome = args.chrome.resolve()
    args.gsplat_wasm_package = args.gsplat_wasm_package.resolve()
    args.reference_images = {
        0: args.reference_trace_0.resolve(),
        1: args.reference_trace_1.resolve(),
    }
    require(all(path.is_file() for path in args.reference_images.values()), "reference images must exist")
    require(
        1024 <= args.gsplat_port_base <= 65520,
        "--gsplat-port-base must leave room for 15 predeclared ports",
    )
    return args


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        if args.execute:
            args.formal_inputs = preflight_execute(args)
        else:
            args.formal_inputs = None
        plan = build_plan(args, predeclared_at=utc_now())
        if args.dry_run:
            print(json.dumps({
                "mode": "dry-run",
                "side_effects": False,
                "plan": plan,
                "commands": command_receipt(plan),
            }, indent=2, sort_keys=True))
            return 0
        return execute(args, plan)
    except OrchestrationError as error:
        print(f"Q1 orchestration rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
