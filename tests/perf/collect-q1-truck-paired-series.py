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
import stat
import subprocess
import sys
import tempfile
import time
from collections.abc import Mapping
from datetime import datetime, timezone
from typing import Any


SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
sys.path.insert(0, str(SCRIPT_DIR))

import q1_browser_process_owner as PROCESS_OWNER  # noqa: E402
from q1_browser_process_owner import (  # noqa: E402
    ProcessOutcome,
    ProcessOwnershipError as OrchestrationError,
    ProcessTimeoutError,
    ProcessTreeError,
    browser_ownership,
    require_process_completed,
)
from q1_pair_admission.artifacts import (  # noqa: E402
    HOST_ADMISSION_JOIN_SCHEMA,
    IMAGE_METRIC_IMPLEMENTATION_SHA256,
    IMAGE_TOOL_SHA256,
    artifact as admit_artifact,
    reference_authority as admit_reference_authority,
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
REFERENCE_AUTHORITY_DESTINATION = pathlib.Path("reference-authority")
GSPLAT_QUALIFICATION = "truck-quality-1080p-fixed-gpu-preproject-compact-v1"
TRACE_URL = "/tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json"
IMAGE_TOOL = pathlib.Path("tests/perf/compare-image-ssim.mjs")
IMAGE_METRIC_IMPLEMENTATION = pathlib.Path("tests/perf/png-image-metrics.mjs")

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
    "process_group_term_grace": 5,
    "process_group_kill_grace": 5,
}
PUPPETEER_GRAPH_SCHEMA = "gsplat-q1-puppeteer-production-modules/v1"
IGNORED_MODULE_TREE_PARTS = frozenset({
    ".cache", "coverage", "docs", "examples", "test", "tests", "tmp",
    "__pycache__", ".DS_Store",
})
LOCKED_REPOSITORY_FILES = (
    "tests/perf/collect-q1-truck-paired-series.py",
    "tests/perf/q1_browser_process_owner.py",
    "tests/perf/browser-process-ownership.mjs",
    "tests/perf/q1-host-start-gate.mjs",
    "tests/perf/validate-q1-truck-paired-comparison.py",
    "tests/perf/validate-benchmark-artifacts.py",
    "tests/perf/validate-balanced-image-gate.py",
    "tests/perf/compare-image-ssim.mjs",
    "tests/perf/png-image-metrics.mjs",
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


def run_process_group(
    argv: list[str],
    *,
    cwd: pathlib.Path,
    env: dict[str, str],
    timeout_seconds: int,
    browser_ownership: dict[str, str] | None = None,
) -> ProcessOutcome:
    """Use the shared formal browser owner with pair-configured safety bounds."""

    return PROCESS_OWNER.run_process_group(
        argv,
        cwd=cwd,
        env=env,
        timeout_seconds=timeout_seconds,
        browser_ownership=browser_ownership,
        term_grace_seconds=PROCESS_TIMEOUTS_SECONDS["process_group_term_grace"],
        kill_grace_seconds=PROCESS_TIMEOUTS_SECONDS["process_group_kill_grace"],
    )


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


def authority_content_identity(
    authority: dict[str, Any],
    *,
    root_path: str,
) -> dict[str, Any]:
    """Return the JSON-safe identity shared by source and retained copies."""

    authority_root = pathlib.Path(authority["authority_root"])
    views = []
    for trace in TRACE_INDICES:
        view = authority["views"][trace]
        views.append(
            {
                "trace_frame_index": trace,
                "path": pathlib.Path(view["path"])
                .relative_to(authority_root)
                .as_posix(),
                "sha256": view["sha256"],
                "decoded_rgba8_sha256": view["decoded_rgba8_sha256"],
                "pose_intrinsics_sha256": view["pose_intrinsics_sha256"],
            }
        )
    return {
        "root_path": root_path,
        "receipt_path": "reference.json",
        "receipt_sha256": authority["receipt_sha256"],
        "repository_commit": authority["repository_commit"],
        "release_binary_sha256": authority["release_binary_sha256"],
        "generated_at_utc": authority["generated_at_utc"],
        "tree": authority["tree"],
        "views": views,
    }


def validate_reference_authority_input(
    args: argparse.Namespace,
    predeclared_at: str,
) -> dict[str, Any]:
    """Fully admit the external authority before any series/browser action."""

    try:
        authority = admit_reference_authority(args.reference_authority)
    except (OSError, ValidationError, ValueError) as error:
        raise OrchestrationError(
            f"Direct-f32 reference authority is not admissible: {error}"
        ) from error
    require(
        authority["repository_commit"] == args.reviewed_sha,
        "Direct-f32 reference authority commit does not equal --reviewed-sha",
    )
    require(
        utc(
            authority["generated_at_utc"],
            "Direct-f32 reference authority generated_at_utc",
        )
        <= utc(predeclared_at, "Q1 schedule predeclared_at_utc"),
        "Direct-f32 reference authority was generated after schedule predeclaration",
    )
    return authority


def formal_reference_receipts(authority: dict[str, Any]) -> list[dict[str, Any]]:
    authority_root = pathlib.Path(authority["authority_root"])
    result = []
    for trace in TRACE_INDICES:
        view = authority["views"][trace]
        result.append(
            {
                "trace_frame_index": trace,
                "source_path": str(view["path"]),
                "series_path": (
                    REFERENCE_AUTHORITY_DESTINATION
                    / pathlib.Path(view["path"]).relative_to(authority_root)
                ).as_posix(),
                "sha256": view["sha256"],
                "rgba8_sha256": view["decoded_rgba8_sha256"],
                "width": WIDTH,
                "height": HEIGHT,
                "pixel_format": "rgba8unorm-srgb",
                "pose_intrinsics_sha256": view["pose_intrinsics_sha256"],
                "authority_receipt_path": (
                    REFERENCE_AUTHORITY_DESTINATION / "reference.json"
                ).as_posix(),
                "authority_receipt_sha256": authority["receipt_sha256"],
            }
        )
    return result


def copy_reference_authority(
    source: pathlib.Path,
    destination: pathlib.Path,
    tree: dict[str, Any],
) -> None:
    """Copy the frozen regular-file tree without ever following a symlink."""

    directory_flags = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0)
    )
    file_read_flags = (
        os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0)
    )
    file_write_flags = (
        os.O_WRONLY
        | os.O_CREAT
        | os.O_EXCL
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0)
    )

    def open_relative_directory(
        root_fd: int, parts: tuple[str, ...], *, create: bool
    ) -> int:
        current = os.dup(root_fd)
        try:
            for part in parts:
                require(
                    part not in {"", ".", ".."},
                    "authority tree contains an unsafe path",
                )
                if create:
                    try:
                        os.mkdir(part, mode=0o755, dir_fd=current)
                    except FileExistsError:
                        pass
                next_fd = os.open(part, directory_flags, dir_fd=current)
                os.close(current)
                current = next_fd
            return current
        except BaseException:
            os.close(current)
            raise

    # Anchor both trees once, then resolve every authority-relative component
    # with openat-style dir_fd calls. This prevents a checked intermediate
    # authority directory from being replaced by a symlink during the copy.
    source_root_fd = os.open(source, directory_flags)
    try:
        destination_parent_fd = os.open(destination.parent, directory_flags)
        try:
            os.mkdir(destination.name, mode=0o755, dir_fd=destination_parent_fd)
            destination_root_fd = os.open(
                destination.name, directory_flags, dir_fd=destination_parent_fd
            )
            try:
                for entry in tree["files"]:
                    relative = pathlib.PurePosixPath(entry["path"])
                    require(
                        not relative.is_absolute()
                        and relative.name not in {"", ".", ".."}
                        and ".." not in relative.parts,
                        "authority tree contains an unsafe relative path",
                    )
                    source_parent_fd = open_relative_directory(
                        source_root_fd, tuple(relative.parts[:-1]), create=False
                    )
                    destination_directory_fd = open_relative_directory(
                        destination_root_fd, tuple(relative.parts[:-1]), create=True
                    )
                    try:
                        source_fd = os.open(
                            relative.name,
                            file_read_flags,
                            dir_fd=source_parent_fd,
                        )
                        destination_fd = None
                        try:
                            destination_fd = os.open(
                                relative.name,
                                file_write_flags,
                                mode=0o600,
                                dir_fd=destination_directory_fd,
                            )
                            source_stat = os.fstat(source_fd)
                            require(
                                stat.S_ISREG(source_stat.st_mode),
                                f"authority source changed type before copy: {entry['path']}",
                            )
                            require(
                                source_stat.st_size == entry["bytes"],
                                f"authority source size drifted before copy: {entry['path']}",
                            )
                            copied_hash = hashlib.sha256()
                            copied_bytes = 0
                            with os.fdopen(
                                source_fd, "rb", closefd=False
                            ) as source_file:
                                with os.fdopen(
                                    destination_fd, "wb", closefd=False
                                ) as destination_file:
                                    while True:
                                        chunk = source_file.read(1024 * 1024)
                                        if not chunk:
                                            break
                                        copied_hash.update(chunk)
                                        copied_bytes += len(chunk)
                                        destination_file.write(chunk)
                                    destination_file.flush()
                                    os.fsync(destination_fd)
                            require(
                                copied_bytes == entry["bytes"]
                                and copied_hash.hexdigest() == entry["sha256"],
                                f"authority source content drifted during copy: {entry['path']}",
                            )
                            os.fchmod(
                                destination_fd, stat.S_IMODE(source_stat.st_mode)
                            )
                        finally:
                            if destination_fd is not None:
                                os.close(destination_fd)
                            os.close(source_fd)
                    finally:
                        os.close(destination_directory_fd)
                        os.close(source_parent_fd)
            finally:
                os.close(destination_root_fd)
        finally:
            os.close(destination_parent_fd)
    finally:
        os.close(source_root_fd)


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


def module_tree_receipt(
    root: pathlib.Path, lock_path: str, node_modules_root: pathlib.Path
) -> dict[str, Any]:
    """Hash installed production module bytes without caches/tests/temp files."""

    require(not root.is_symlink(), f"locked production module is a symlink: {lock_path}")
    require(root.is_dir(), f"locked production module is unavailable: {lock_path}")
    node_modules = node_modules_root.resolve()
    require(
        root.resolve().is_relative_to(node_modules),
        f"locked production module escapes node_modules: {lock_path}",
    )
    entries = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        require(not path.is_symlink(), f"locked production module contains a symlink: {lock_path}/{relative}")
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


def dependency_candidates(parent: str, name: str) -> list[str]:
    result = []
    prefix: str | None = parent
    while prefix is not None:
        result.append(f"{prefix}/node_modules/{name}")
        marker = prefix.rfind("/node_modules/")
        prefix = prefix[:marker] if marker >= 0 else None
    result.append(f"node_modules/{name}")
    return list(dict.fromkeys(result))


def dependency_object(source: dict[str, Any], field: str, package_key: str) -> dict[str, Any]:
    value = source.get(field, {})
    require(isinstance(value, dict), f"{package_key} {field} must be an object")
    return value


def locked_runtime_dependency_requirements(
    metadata: dict[str, Any], package_key: str
) -> dict[str, bool]:
    """Return lock-authoritative runtime dependency requiredness."""

    requirements: dict[str, bool] = {}
    dependencies = dependency_object(metadata, "dependencies", package_key)
    optional = dependency_object(metadata, "optionalDependencies", package_key)
    peers = dependency_object(metadata, "peerDependencies", package_key)
    peer_meta = dependency_object(metadata, "peerDependenciesMeta", package_key)
    for name in dependencies:
        requirements[name] = True
    for name in optional:
        requirements[name] = False
    for name in peers:
        optional_peer = isinstance(peer_meta.get(name), dict) and peer_meta[name].get("optional") is True
        requirements.setdefault(name, not optional_peer)
    return requirements


def validate_installed_manifest(
    metadata: dict[str, Any], installed: dict[str, Any], package_key: str
) -> None:
    installed_name = package_key.rsplit("node_modules/", 1)[-1]
    require(installed.get("name") == installed_name, f"installed package name mismatch: {package_key}")
    require(
        isinstance(metadata.get("version"), str)
        and installed.get("version") == metadata["version"],
        f"installed package version mismatch: {package_key}",
    )
    for field in (
        "dependencies",
        "optionalDependencies",
        "peerDependencies",
        "peerDependenciesMeta",
    ):
        require(
            dependency_object(installed, field, package_key)
            == dependency_object(metadata, field, package_key),
            f"installed {package_key} {field} differs from package-lock semantics",
        )


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
        package_root = playcanvas_root / package_key
        require(not package_root.is_symlink(), f"installed package directory is a symlink: {package_key}")
        package_json = package_root / "package.json"
        require(not package_json.is_symlink(), f"installed package.json is a symlink: {package_key}")
        require(
            package_root.resolve().is_relative_to((playcanvas_root / "node_modules").resolve()),
            f"installed package escapes node_modules: {package_key}",
        )
        installed = load_object(package_json, f"installed {package_key} package.json")
        validate_installed_manifest(metadata, installed, package_key)
        resolved_dependencies = []
        for name, required in sorted(
            locked_runtime_dependency_requirements(metadata, package_key).items()
        ):
            installed_candidates = [
                candidate
                for candidate in dependency_candidates(package_key, name)
                if (playcanvas_root / candidate).is_dir()
            ]
            if not installed_candidates:
                require(
                    not required,
                    f"required runtime dependency {name!r} from {package_key!r} is absent",
                )
                continue
            dependency_key = installed_candidates[0]
            require(
                dependency_key in packages,
                f"installed runtime dependency {dependency_key!r} is absent from package-lock",
            )
            pending.append(dependency_key)
            resolved_dependencies.append({
                "name": name,
                "lock_path": dependency_key,
                "required": required,
            })
        receipt = module_tree_receipt(
            package_root,
            package_key,
            playcanvas_root / "node_modules",
        )
        receipts.append({
            **receipt,
            "version": metadata.get("version"),
            "integrity": metadata.get("integrity"),
            "runtime_dependencies": resolved_dependencies,
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


def capture_formal_inputs(
    args: argparse.Namespace,
    *,
    authority: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if authority is None:
        authority = validate_reference_authority_input(
            args, args.predeclared_at_utc
        )
    reference_receipts = formal_reference_receipts(authority)
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
        "reference_authority": authority_content_identity(
            authority,
            root_path=str(args.reference_authority),
        ),
        "references": reference_receipts,
    }


def verify_formal_inputs(
    args: argparse.Namespace, expected: dict[str, Any]
) -> dict[str, Any]:
    require(not git_output("status", "--porcelain"), "repository became dirty during Q1 execution")
    observed = capture_formal_inputs(args)
    require(
        observed == expected,
        "reviewed commit, browser, WASM, producer, validator, runtime, dataset, trace, or reference input drifted",
    )
    return observed


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
    ownership = browser_ownership(root, invocation_id, chrome)
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
            **ownership["environment"],
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
            **ownership["environment"],
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
        "browser_ownership": {
            key: ownership[key]
            for key in (
                "marker",
                "marker_argument",
                "user_data_dir",
                "handshake_path",
            )
        },
        "dynamic_inputs": dynamic_inputs,
        "automatic_retry": False,
        "timeout_seconds": PROCESS_TIMEOUTS_SECONDS["producer"],
    }


def build_plan(args: argparse.Namespace, *, predeclared_at: str) -> dict[str, Any]:
    root = args.series_root.resolve()
    protocol_value = protocol()
    protocol_sha = canonical_sha256(protocol_value)
    authority = args.reference_authority_admission
    authority_root = pathlib.Path(authority["authority_root"])
    references = [
        {
            "trace_frame_index": trace,
            "path": (
                REFERENCE_AUTHORITY_DESTINATION
                / pathlib.Path(authority["views"][trace]["path"]).relative_to(
                    authority_root
                )
            ).as_posix(),
            "sha256": authority["views"][trace]["sha256"],
            "decoded_rgba8_sha256": authority["views"][trace][
                "decoded_rgba8_sha256"
            ],
            "pose_intrinsics_sha256": authority["views"][trace][
                "pose_intrinsics_sha256"
            ],
            "authority_receipt_path": (
                REFERENCE_AUTHORITY_DESTINATION / "reference.json"
            ).as_posix(),
            "authority_receipt_sha256": authority["receipt_sha256"],
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
        "reference_authority": {
            "source": authority_content_identity(
                authority,
                root_path=str(args.reference_authority),
            ),
            "series_root": REFERENCE_AUTHORITY_DESTINATION.as_posix(),
            "series_receipt_path": (
                REFERENCE_AUTHORITY_DESTINATION / "reference.json"
            ).as_posix(),
            "series_files": [
                {
                    **entry,
                    "destination": (
                        REFERENCE_AUTHORITY_DESTINATION / entry["path"]
                    ).as_posix(),
                }
                for entry in authority["tree"]["files"]
            ],
        },
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
                "metric_implementation": IMAGE_METRIC_IMPLEMENTATION.as_posix(),
                "metric_implementation_sha256": IMAGE_METRIC_IMPLEMENTATION_SHA256,
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
        "reference_authority": plan["reference_authority"],
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
                    "browser_ownership",
                    "dynamic_inputs",
                    "automatic_retry",
                    "timeout_seconds",
                )
            }
            for invocation in plan["invocations"]
        ],
    }


def execution_lock(
    plan: dict[str, Any],
    commands: dict[str, Any],
    *,
    claimed_reference_authority: dict[str, Any],
) -> dict[str, Any]:
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
        "reference_authority": {
            "source_pre_sha256": canonical_sha256(
                formal_inputs["reference_authority"]
            ),
            "claimed_pre": claimed_reference_authority,
            "claimed_pre_sha256": canonical_sha256(
                claimed_reference_authority
            ),
        },
        "command_receipt": {
            "path": "commands.json",
            "sha256": hashlib.sha256(json_bytes(commands)).hexdigest(),
            "canonical_sha256": canonical_sha256(commands),
            "invocation_count": len(plan["invocations"]),
        },
        "timeouts_seconds": plan["execution_policy"]["timeouts_seconds"],
    }


def git_output(*arguments: str) -> str:
    completed = run_process_group(
        ["git", *arguments],
        cwd=REPO_ROOT,
        env=safe_host_environment(),
        timeout_seconds=PROCESS_TIMEOUTS_SECONDS["git_helper"],
    )
    require_process_completed(completed, "git helper")
    require(completed.returncode == 0, completed.stderr.strip() or "git command failed")
    return completed.stdout.strip()


def preflight_execute(args: argparse.Namespace) -> dict[str, Any]:
    require(
        len(args.reviewed_sha or "") == 40,
        "formal Q1 planning requires a full --reviewed-sha",
    )
    require(
        git_output("rev-parse", "HEAD") == args.reviewed_sha,
        "reviewed SHA does not equal the current exact commit",
    )
    require(not git_output("status", "--porcelain"), "formal Q1 execution requires a clean tree")
    require(args.chrome.is_file() and os.access(args.chrome, os.X_OK), "Chrome is not executable")
    require_process_table_commandlines()
    require(not any(character.isspace() for character in str(args.series_root)), "formal series root cannot contain whitespace because its exact browser ownership token must be observable")
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
    authority = validate_reference_authority_input(
        args, args.predeclared_at_utc
    )
    args.reference_authority_admission = authority
    formal_inputs = capture_formal_inputs(args, authority=authority)
    require(
        formal_inputs["git"] == {"head": args.reviewed_sha, "clean": True},
        "formal input lock does not match the reviewed clean commit",
    )
    return formal_inputs


def claim_series(args: argparse.Namespace, plan: dict[str, Any]) -> dict[str, Any]:
    root = args.series_root
    # Re-admit before the first filesystem side effect.  This catches drift
    # since preflight and preserves the authority-before-series invariant even
    # when this function is exercised directly by focused tests.
    source_authority = validate_reference_authority_input(
        args, plan["schedule"]["predeclared_at_utc"]
    )
    source_identity = authority_content_identity(
        source_authority,
        root_path=str(args.reference_authority),
    )
    require(
        source_identity == plan["reference_authority"]["source"],
        "Direct-f32 reference authority drifted before series claim",
    )
    if isinstance(plan.get("formal_inputs"), dict):
        require(
            source_identity == plan["formal_inputs"].get("reference_authority"),
            "formal input lock does not bind the admitted Direct-f32 authority",
        )
    root.mkdir()
    for directory in (
        "requests", "run-contexts", "logs", "process-home",
        "process-home/browser-profiles", "browser-handshakes",
    ):
        (root / directory).mkdir()
    for invocation in plan["invocations"]:
        # Producers atomically claim their final artifact directory; only the
        # shared parents may exist before invocation.
        (root / invocation["artifact"]).parent.mkdir(parents=True, exist_ok=True)
    claimed_root = root / REFERENCE_AUTHORITY_DESTINATION
    copy_reference_authority(
        args.reference_authority,
        claimed_root,
        source_authority["tree"],
    )
    # Re-admit both ends after the no-follow copy.  A source mutation, an
    # omitted support file, or a destination drift rejects before producer 1.
    source_after = validate_reference_authority_input(
        args, plan["schedule"]["predeclared_at_utc"]
    )
    require(
        authority_content_identity(
            source_after,
            root_path=str(args.reference_authority),
        )
        == source_identity,
        "Direct-f32 reference authority changed while being retained",
    )
    try:
        claimed_authority = admit_reference_authority(claimed_root)
    except (OSError, ValidationError, ValueError) as error:
        raise OrchestrationError(
            f"retained Direct-f32 reference authority is not admissible: {error}"
        ) from error
    claimed_identity = authority_content_identity(
        claimed_authority,
        root_path=REFERENCE_AUTHORITY_DESTINATION.as_posix(),
    )
    expected_claimed = {
        **source_identity,
        "root_path": REFERENCE_AUTHORITY_DESTINATION.as_posix(),
    }
    require(
        claimed_identity == expected_claimed,
        "retained Direct-f32 reference authority differs from its source",
    )
    commands = command_receipt(plan)
    commands_sha = hashlib.sha256(json_bytes(commands)).hexdigest()
    locked = execution_lock(
        plan,
        commands,
        claimed_reference_authority=claimed_identity,
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
    browser_owner = invocation["browser_ownership"]
    completed = run_process_group(
        invocation["argv"],
        cwd=REPO_ROOT,
        env=invocation["environment"],
        timeout_seconds=invocation["timeout_seconds"],
        browser_ownership={
            **browser_owner,
            "expected_executable": invocation["environment"]["CHROME_PATH"],
        },
    )
    log_root = root / "logs"
    (log_root / f"{invocation['invocation_id']}.stdout.log").write_text(
        completed.stdout, encoding="utf-8"
    )
    (log_root / f"{invocation['invocation_id']}.stderr.log").write_text(
        completed.stderr, encoding="utf-8"
    )
    write_new_json(log_root / f"{invocation['invocation_id']}.process.json", {
        "timed_out": completed.timed_out,
        "timeout_seconds": completed.timeout_seconds,
        "returncode": completed.returncode,
        "cleanup": completed.cleanup,
    })
    require_process_completed(completed, invocation["invocation_id"])
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
    validate = run_process_group(
        [sys.executable, "tests/perf/validate-benchmark-artifacts.py", str(directory)],
        cwd=REPO_ROOT,
        env=environment,
        timeout_seconds=PROCESS_TIMEOUTS_SECONDS["canonical_validator"],
    )
    require_process_completed(validate, f"{context} canonical validator")
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
            control["environment"]["identity"].get("collection_session_id")
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
    comparison_id = f"image-{pair_id}-{endpoint}-trace-{trace}"
    ownership = browser_ownership(
        root, comparison_id, pathlib.Path(environment["CHROME_PATH"])
    )
    comparison_environment = {**environment, **ownership["environment"]}
    completed = run_process_group(
        [
            "node",
            str(IMAGE_TOOL),
            str(root / reference["path"]),
            str(candidate),
            "--expected-width",
            str(WIDTH),
            "--expected-height",
            str(HEIGHT),
            "--raw-rgba8-contract",
            "--output",
            str(raw),
        ],
        cwd=REPO_ROOT,
        env=comparison_environment,
        timeout_seconds=PROCESS_TIMEOUTS_SECONDS["image_comparison"],
        browser_ownership={
            key: ownership[key]
            for key in (
                "marker",
                "marker_argument",
                "user_data_dir",
                "handshake_path",
                "expected_executable",
            )
        },
    )
    (raw.parent / f"{raw.stem}.stdout.log").write_text(completed.stdout, encoding="utf-8")
    (raw.parent / f"{raw.stem}.stderr.log").write_text(completed.stderr, encoding="utf-8")
    write_new_json(raw.parent / f"{raw.stem}.process.json", {
        "timed_out": completed.timed_out,
        "timeout_seconds": completed.timeout_seconds,
        "returncode": completed.returncode,
        "cleanup": completed.cleanup,
    })
    require_process_completed(
        completed,
        f"image comparison for {pair_id}.{endpoint}.trace-{trace}",
    )
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
    require(
        raw_value.get("metricImplementation")
        == {
            "path": IMAGE_METRIC_IMPLEMENTATION.as_posix(),
            "sha256": IMAGE_METRIC_IMPLEMENTATION_SHA256,
        },
        "image comparison did not use the locked raw PNG metric implementation",
    )
    require(
        raw_value.get("pixelDomain")
        == "raw_noninterlaced_rgba8_no_color_management",
        "image comparison did not use the strict Q1 raw RGBA8 pixel domain",
    )
    score = raw_value.get("score")
    require(isinstance(score, (int, float)) and 0 <= score <= 1, "image comparison score is invalid")
    receipt = {
        "schema": IMAGE_SCHEMA,
        "metric": "ssim-luma-srgb-window8",
        "tool": IMAGE_TOOL.as_posix(),
        "tool_sha256": IMAGE_TOOL_SHA256,
        "metric_implementation": IMAGE_METRIC_IMPLEMENTATION.as_posix(),
        "metric_implementation_sha256": IMAGE_METRIC_IMPLEMENTATION_SHA256,
        "pixel_domain": "raw_noninterlaced_rgba8_no_color_management",
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
    observed_inputs = verify_formal_inputs(args, locked["formal_inputs"])
    claimed_root = root / REFERENCE_AUTHORITY_DESTINATION
    try:
        claimed_authority = admit_reference_authority(claimed_root)
    except (OSError, ValidationError, ValueError) as error:
        raise OrchestrationError(
            f"retained Direct-f32 reference authority drifted: {error}"
        ) from error
    claimed_post = authority_content_identity(
        claimed_authority,
        root_path=REFERENCE_AUTHORITY_DESTINATION.as_posix(),
    )
    authority_lock = locked["reference_authority"]
    require(
        canonical_sha256(observed_inputs["reference_authority"])
        == authority_lock["source_pre_sha256"],
        "source Direct-f32 reference authority drifted during Q1 execution",
    )
    require(
        canonical_sha256(claimed_post)
        == authority_lock["claimed_pre_sha256"]
        and claimed_post == authority_lock["claimed_pre"],
        "retained Direct-f32 reference authority drifted during Q1 execution",
    )
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
        "reference_authority": {
            "source_pre_sha256": authority_lock["source_pre_sha256"],
            "source_post_sha256": canonical_sha256(
                observed_inputs["reference_authority"]
            ),
            "claimed_pre_sha256": authority_lock["claimed_pre_sha256"],
            "claimed_post_sha256": canonical_sha256(claimed_post),
            "receipt_sha256": claimed_post["receipt_sha256"],
            "tree_sha256": claimed_post["tree"]["sha256"],
        },
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
        process = error.outcome if isinstance(error, (ProcessTimeoutError, ProcessTreeError)) else None
        process_receipt = None if process is None else {
            "argv": process.argv,
            "timeout_seconds": process.timeout_seconds,
            "timed_out": process.timed_out,
            "returncode": process.returncode,
            "stdout_tail": process.stdout[-4096:],
            "stderr_tail": process.stderr[-4096:],
            "cleanup": process.cleanup,
        }
        write_new_json(blocker, {
            "schema": BLOCKER_SCHEMA,
            "series_id": None if plan is None else plan["series_id"],
            "failed_at_utc": utc_now(),
            "reason": str(error),
            "automatic_retry": False,
            "retry_authorized": False,
            "process_failure": process_receipt,
            "process_timeout": process_receipt if isinstance(error, ProcessTimeoutError) else None,
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
        result = run_process_group(
            [
                sys.executable,
                "tests/perf/validate-q1-truck-paired-comparison.py",
                str(schedule_path),
                "--output",
                str(root / "result.json"),
            ],
            cwd=REPO_ROOT,
            env=plan["postprocess"]["environment"],
            timeout_seconds=PROCESS_TIMEOUTS_SECONDS["final_validator"],
        )
        require_process_completed(result, "final Q1 validator")
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
    parser.add_argument("--reference-authority", type=pathlib.Path, required=True)
    parser.add_argument("--reviewed-sha", required=True)
    parser.add_argument("--predeclared-at-utc", required=True)
    parser.add_argument("--gsplat-port-base", type=int, default=43000)
    args = parser.parse_args(argv)
    args.series_root = args.series_root.resolve()
    args.chrome = args.chrome.resolve()
    args.gsplat_wasm_package = args.gsplat_wasm_package.resolve()
    # Do not resolve away a lexical authority-root symlink before the shared
    # admission owner has a chance to reject it.
    args.reference_authority = args.reference_authority.absolute()
    require(
        len(args.reviewed_sha) == 40
        and all(character in "0123456789abcdef" for character in args.reviewed_sha),
        "--reviewed-sha must be a full lowercase Git SHA",
    )
    try:
        predeclared_at = utc(args.predeclared_at_utc, "--predeclared-at-utc")
    except ValidationError as error:
        raise OrchestrationError(str(error)) from error
    require(
        predeclared_at <= datetime.now(timezone.utc),
        "--predeclared-at-utc must not be in the future",
    )
    require(
        1024 <= args.gsplat_port_base <= 65520,
        "--gsplat-port-base must leave room for 15 predeclared ports",
    )
    return args


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        args.reference_authority_admission = validate_reference_authority_input(
            args, args.predeclared_at_utc
        )
        # Dry-run is the same full read-only admission as execute.  It locks
        # clean Git, Chrome, WASM, Puppeteer, repository and authority inputs,
        # but still performs no mkdir/copy/build/browser work.
        args.formal_inputs = preflight_execute(args)
        plan = build_plan(args, predeclared_at=args.predeclared_at_utc)
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
