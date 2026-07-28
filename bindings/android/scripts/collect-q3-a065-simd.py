#!/usr/bin/env python3
"""Collect the finite Q3 A065 Scalar/Neon whole-plan matrix.

This qualification-only orchestrator builds one private runtime-selection APK,
installs and hash-verifies it once, prepares each workload once, and delegates
all device capture plus strict current-stats/artifact validation to
collect-android-sort-benchmarks.py. It never changes the product default and
never retries a failed command.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import hashlib
import importlib.util
import json
import math
import os
import pathlib
import random
import re
import shutil
import statistics
import subprocess
import sys
import uuid
from collections.abc import Sequence
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
BASE_PATH = pathlib.Path(__file__).with_name("collect-android-sort-benchmarks.py")
MATRIX_PATH = REPO_ROOT / "tests/perf/full-quality-matrix-plan-v1.json"
TRACE_VALIDATOR = REPO_ROOT / "tests/perf/trace/validate_trace_v1.py"
SCHEMA = "gsplat-q3-a065-simd/v1"
CELL = "Q3.A065.PackedCpuExact.ScalarVsNeon.WholePlan"
DECISIONS = {"Accepted", "Rejected", "Deferred"}
FORMAL_SIZE = (2412, 1080)
DEFAULT_PAIRS = 5
DEFAULT_WARMUP = 20
DEFAULT_MEASURED = 80
DEFAULT_CORRECTNESS_FRAMES = 2
DEFAULT_SEED = 0x5133413036355349
Q3_APK_INSTALL_TIMEOUT_SECONDS = 60.0
Q3_LANE_ENV = "GSPLAT_ANDROID_Q3_CPU_LANE"
Q3_RUNTIME_BUILD = "runtime"
PHASE_SCHEMA = "gsplat-q3-android-protocol-phase/v1"
PARITY_SCHEMA = "gsplat-q3-a065-element-parity/v1"
RANGE_REJECTION_SCHEMA = "gsplat-renderer-scene-admission-range-rejection/v1"
PARITY_TEST = "cpu::q3_a065::q3_a065_forced_scalar_neon_element_parity"
ANDROID_TARGET = "aarch64-linux-android"
ANDROID_NDK_VERSION = "29.0.14206865"
ANDROID_API_LEVEL = 24
REQUIRED_WORKLOAD_IDS = (
    "truck-050k",
    "truck-100k",
    "truck-200k",
    "truck-300k",
    "truck-500k",
    "truck-1m",
    "truck-1p5m",
    "truck-2m",
    "truck-full",
)
TRACE_ID = "candidate-truck-quality-2view-2412x1080-v1"


def load_module(name: str, path: pathlib.Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


BASE = load_module("android_sort_collector_for_q3_a065", BASE_PATH)
PREPARED_INPUTS_SCHEMA = BASE.Q3_PREPARED_INPUTS_SCHEMA
INSTALLED_APK_SCHEMA = BASE.Q3_INSTALLED_APK_SCHEMA


class QualificationError(RuntimeError):
    pass


class EnvironmentPrerequisiteError(QualificationError):
    pass


class IntegrityRejectedError(QualificationError):
    pass


class RangeAdmissionRejectedError(IntegrityRejectedError):
    pass


class MatrixInfrastructureError(IntegrityRejectedError):
    pass


@dataclasses.dataclass(frozen=True)
class Lane:
    name: str


LANES = (
    Lane("scalar"),
    Lane("neon"),
)
LANE_BY_NAME = {lane.name: lane for lane in LANES}


@dataclasses.dataclass(frozen=True)
class Workload:
    id: str
    path: pathlib.Path
    identity: dict[str, Any]
    role: str


@dataclasses.dataclass(frozen=True)
class PreparedWorkload:
    path: pathlib.Path
    receipt: dict[str, Any]


@dataclasses.dataclass
class DeviceMatrixSession:
    adb: pathlib.Path | str
    serial: str
    stage: pathlib.Path
    lane_receipts: dict[str, dict[str, Any]]
    installed_receipt_path: pathlib.Path | None = None
    install_count: int = 0
    dataset_push_count: int = 0
    dataset_copy_count: int = 0
    trace_push_count: int = 0
    trace_copy_count: int = 0
    package_clear_count: int = 0
    measurement_attempt_count: int = 0
    preparation_id: str = dataclasses.field(default_factory=lambda: uuid.uuid4().hex)
    installation_events: list[dict[str, Any]] = dataclasses.field(default_factory=list)
    workload_events: list[dict[str, Any]] = dataclasses.field(default_factory=list)

    def ensure_lane(self, lane_name: str, timeout_seconds: float) -> tuple[pathlib.Path, bool]:
        if lane_name not in LANE_BY_NAME:
            raise IntegrityRejectedError(f"unknown Q3 lane {lane_name!r}")
        if self.installed_receipt_path is not None:
            return self.installed_receipt_path, False
        runtime_receipt = self.lane_receipts["scalar"]
        apk = self.stage / runtime_receipt["apk_path"]
        BASE.install_apk(
            self.adb,
            self.serial,
            apk,
            min(timeout_seconds, Q3_APK_INSTALL_TIMEOUT_SECONDS),
        )
        installed = BASE.verify_installed_apk(self.adb, self.serial, apk)
        self.install_count += 1
        event = {
            "schema": INSTALLED_APK_SCHEMA,
            "serial": self.serial,
            "package": BASE.PACKAGE,
            "lane": Q3_RUNTIME_BUILD,
            "install_sequence": self.install_count,
            "installed_at_utc": BASE.utc_now(),
            "local_apk": runtime_receipt["apk"],
            "installed_apk": installed,
            "native_library": runtime_receipt["native_library"],
        }
        directory = self.stage / "installations"
        directory.mkdir(exist_ok=True)
        path = directory / f"{self.install_count:03d}-{Q3_RUNTIME_BUILD}.json"
        write_json(path, event)
        self.installation_events.append(
            {
                **event,
                "receipt": str(path.relative_to(self.stage)),
            }
        )
        self.installed_receipt_path = path
        return path, True

    def snapshot(self) -> dict[str, Any]:
        return {
            "preparation_id": self.preparation_id,
            "install_count": self.install_count,
            "dataset_push_count": self.dataset_push_count,
            "dataset_copy_count": self.dataset_copy_count,
            "trace_push_count": self.trace_push_count,
            "trace_copy_count": self.trace_copy_count,
            "package_clear_count": self.package_clear_count,
            "measurement_attempt_count": self.measurement_attempt_count,
            "installation_events": self.installation_events,
            "workload_events": self.workload_events,
        }


def ensure_matrix_lane(
    session: DeviceMatrixSession,
    lane_name: str,
    timeout_seconds: float,
) -> tuple[pathlib.Path, bool]:
    try:
        return session.ensure_lane(lane_name, timeout_seconds)
    except MatrixInfrastructureError:
        raise
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise MatrixInfrastructureError(
            f"Q3 cannot maintain installed {lane_name} lane identity: {error}"
        ) from error


@dataclasses.dataclass
class ProtocolPhase:
    path: pathlib.Path
    label: str
    phase: str = "preflight"

    def __post_init__(self) -> None:
        self._publish([{"phase": self.phase, "at_utc": BASE.utc_now()}])

    def _publish(self, history: list[dict[str, Any]]) -> None:
        write_json(
            self.path,
            {
                "schema": PHASE_SCHEMA,
                "label": self.label,
                "phase": self.phase,
                "history": history,
            },
        )

    def transition(self, phase: str) -> None:
        allowed = {
            "preflight": "build",
            "build": "install",
            "install": "launched",
            "launched": "evidence",
        }
        require(allowed.get(self.phase) == phase, f"invalid Q3 phase {self.phase} -> {phase}")
        payload = json.loads(self.path.read_text(encoding="utf-8"))
        history = payload.get("history")
        require(isinstance(history, list), "Q3 phase history is missing")
        self.phase = phase
        history.append({"phase": phase, "at_utc": BASE.utc_now()})
        self._publish(history)

    def snapshot(self) -> dict[str, Any]:
        return json.loads(self.path.read_text(encoding="utf-8"))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise IntegrityRejectedError(message)


def require_matrix_identity(condition: bool, message: str) -> None:
    if not condition:
        raise MatrixInfrastructureError(message)


def write_json(path: pathlib.Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def clear_package_once_for_matrix(session: DeviceMatrixSession) -> None:
    require_matrix_identity(
        session.package_clear_count == 0,
        "Q3 package clear must happen exactly once",
    )
    result = BASE.run_command(
        BASE.adb_args(
            session.adb, session.serial, "shell", "pm", "clear", BASE.PACKAGE
        ),
        capture=True,
    ).stdout.strip()
    require_matrix_identity(
        result == "Success", f"failed to clear Q3 package once: {result}"
    )
    BASE.run_command(
        BASE.adb_args(
            session.adb,
            session.serial,
            "shell",
            "cmd",
            "package",
            "wait-for-handler",
            "--timeout",
            "10000",
        ),
        timeout=15.0,
    )
    session.package_clear_count = 1


@contextlib.contextmanager
def staged_matrix_trace(
    session: DeviceMatrixSession,
    trace_path: pathlib.Path,
    trace_identity: dict[str, Any],
):
    with BASE.staged_device_trace(
        session.adb, session.serial, trace_path, trace_identity
    ) as temporary_trace_path:
        session.trace_push_count += 1
        yield temporary_trace_path


@contextlib.contextmanager
def prepared_workload_inputs(
    session: DeviceMatrixSession,
    workload: Workload,
    trace_identity: dict[str, Any],
    temporary_trace_path: str,
):
    dataset_identity = {
        field: workload.identity[field] for field in ("bytes", "sha256")
    }
    staging = contextlib.ExitStack()
    try:
        temporary_dataset_path = staging.enter_context(
            BASE.staged_device_dataset(
                session.adb,
                session.serial,
                workload.path,
                dataset_identity,
            )
        )
        session.dataset_push_count += 1
        if session.package_clear_count == 0:
            clear_package_once_for_matrix(session)
        internal_dataset = BASE.inject_device_dataset(
            session.adb,
            session.serial,
            temporary_dataset_path,
            dataset_identity,
        )
        session.dataset_copy_count += 1
        if session.trace_copy_count == 0:
            internal_trace = BASE.inject_device_trace(
                session.adb,
                session.serial,
                temporary_trace_path,
                trace_identity,
            )
            session.trace_copy_count = 1
        else:
            internal_trace = BASE.read_device_file_identity(
                session.adb,
                session.serial,
                BASE.INTERNAL_TRACE,
                run_as_package=BASE.PACKAGE,
            )
            BASE.require_matching_identity(
                trace_identity, internal_trace, "reused Q3 camera trace"
            )
        receipt = {
            "schema": PREPARED_INPUTS_SCHEMA,
            "preparation_id": f"{session.preparation_id}:{workload.id}",
            "matrix_preparation_id": session.preparation_id,
            "prepared_at_utc": BASE.utc_now(),
            "serial": session.serial,
            "package": BASE.PACKAGE,
            "workload": workload.id,
            "dataset": {
                "local": dataset_identity,
                "device_staged": {
                    "path": temporary_dataset_path,
                    **dataset_identity,
                },
                "package_internal": {
                    "path": BASE.INTERNAL_DATASET,
                    **internal_dataset,
                },
            },
            "trace": {
                "local": trace_identity,
                "device_staged": {
                    "path": temporary_trace_path,
                    **trace_identity,
                },
                "package_internal": {
                    "path": BASE.INTERNAL_TRACE,
                    **internal_trace,
                },
            },
            "preparation_counts_at_publish": {
                "package_clear": session.package_clear_count,
                "dataset_push": session.dataset_push_count,
                "dataset_copy": session.dataset_copy_count,
                "trace_push": session.trace_push_count,
                "trace_copy": session.trace_copy_count,
            },
        }
        directory = session.stage / "prepared-inputs"
        directory.mkdir(exist_ok=True)
        path = directory / f"{workload.id}.json"
        write_json(path, receipt)
        event = {
            "workload": workload.id,
            "receipt": str(path.relative_to(session.stage)),
            "preparation_id": receipt["preparation_id"],
            "dataset_sha256": dataset_identity["sha256"],
            "trace_sha256": trace_identity["sha256"],
        }
        session.workload_events.append(event)
    except MatrixInfrastructureError:
        staging.close()
        raise
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        staging.close()
        raise MatrixInfrastructureError(
            f"Q3 cannot maintain prepared input identity for {workload.id}: {error}"
        ) from error
    try:
        yield PreparedWorkload(path, receipt)
    finally:
        try:
            staging.close()
        except (OSError, RuntimeError, subprocess.SubprocessError) as error:
            raise MatrixInfrastructureError(
                f"Q3 cannot close staged input identity for {workload.id}: {error}"
            ) from error


def git_receipt(repo: pathlib.Path = REPO_ROOT) -> dict[str, Any]:
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout.strip()
    dirty = bool(
        subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=normal"],
            cwd=repo,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout.strip()
    )
    return {"commit": commit, "dirty": dirty}


def require_ignored_or_external_output(output: pathlib.Path) -> None:
    try:
        output.relative_to(REPO_ROOT)
    except ValueError:
        return
    checked = subprocess.run(
        ["git", "check-ignore", "-q", str(output)],
        cwd=REPO_ROOT,
        check=False,
    )
    if checked.returncode != 0:
        raise ValueError(
            "Q3 output inside the repository must be ignored so evidence cannot "
            "dirty the frozen commit"
        )


def android_build_environment(
    lane: str | None, base: dict[str, str] | None = None
) -> dict[str, str]:
    """Return the only supported build selector; None is the product default."""
    environment = dict(os.environ if base is None else base)
    environment.pop(Q3_LANE_ENV, None)
    environment["ANDROID_RUST_PROFILE"] = "release"
    if lane is None:
        return environment
    if lane not in LANE_BY_NAME and lane != Q3_RUNTIME_BUILD:
        raise ValueError(f"unsupported Q3 Android CPU lane: {lane!r}")
    environment[Q3_LANE_ENV] = lane
    return environment


def schedule_pairs(pairs: int, seed: int, workload_id: str) -> list[tuple[str, str]]:
    require(pairs == DEFAULT_PAIRS, f"formal Q3 matrix requires exactly {DEFAULT_PAIRS} pairs")
    forward = ("scalar", "neon")
    reverse = ("neon", "scalar")
    schedule = [forward] * (pairs // 2) + [reverse] * (pairs // 2)
    if pairs % 2:
        selector = hashlib.sha256(
            f"gsplat-q3-a065-simd/{seed}/{workload_id}".encode("ascii")
        ).digest()[0]
        schedule.append(forward if selector & 1 == 0 else reverse)
    random.Random(f"{seed}:{workload_id}").shuffle(schedule)
    require(
        abs(schedule.count(forward) - schedule.count(reverse)) <= 1,
        f"Q3 schedule for {workload_id} is not counterbalanced",
    )
    return schedule


def diagnostic_schedule(seed: int) -> dict[str, tuple[str, str]]:
    ladder = REQUIRED_WORKLOAD_IDS[:-1]
    orders = [("scalar", "neon")] * (len(ladder) // 2)
    orders += [("neon", "scalar")] * (len(ladder) - len(orders))
    random.Random(f"{seed}:point-ladder").shuffle(orders)
    return dict(zip(ladder, orders, strict=True))


def staged_plan(seed: int) -> list[dict[str, Any]]:
    diagnostics = diagnostic_schedule(seed)
    result = [
        {
            "workload": workload_id,
            "evidence_class": "diagnostic",
            "correctness_runs": 2,
            "timing_pairs": 1,
            "schedule": [list(diagnostics[workload_id])],
            "promotion_eligible": False,
        }
        for workload_id in REQUIRED_WORKLOAD_IDS[:-1]
    ]
    result.append(
        {
            "workload": "truck-full",
            "evidence_class": "terminal",
            "correctness_runs": 2,
            "timing_pairs": DEFAULT_PAIRS,
            "schedule": [
                list(pair)
                for pair in schedule_pairs(DEFAULT_PAIRS, seed, "truck-full")
            ],
            "promotion_eligible": True,
        }
    )
    return result


def load_workloads(
    selected_ids: Sequence[str], matrix_path: pathlib.Path = MATRIX_PATH
) -> tuple[list[Workload], dict[str, Any], pathlib.Path]:
    require(
        matrix_path.resolve() == MATRIX_PATH.resolve(),
        "Q3 A065 qualification requires the committed full-quality matrix",
    )
    matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
    datasets = {
        item.get("id"): item
        for item in matrix.get("datasets", [])
        if isinstance(item, dict) and isinstance(item.get("id"), str)
    }
    requested = list(selected_ids) if selected_ids else list(REQUIRED_WORKLOAD_IDS)
    require(len(requested) == len(set(requested)), "Q3 workload selection contains duplicates")
    require(
        set(requested).issubset(REQUIRED_WORKLOAD_IDS),
        "Q3 workload selection contains a non-canonical dataset",
    )
    workloads: list[Workload] = []
    for workload_id in requested:
        entry = datasets.get(workload_id)
        require(isinstance(entry, dict), f"matrix lacks Q3 workload {workload_id}")
        expected_role = "full_scene" if workload_id == "truck-full" else "scaling_tier"
        require(entry.get("role") == expected_role, f"{workload_id} role drifted")
        if expected_role == "scaling_tier":
            require(
                entry.get("source_dataset_id") == "truck-full",
                f"{workload_id} is not derived from complete Truck",
            )
        path = (REPO_ROOT / str(entry.get("local_path"))).resolve()
        if not path.is_file():
            raise EnvironmentPrerequisiteError(f"Q3 workload is unavailable: {path}")
        actual = BASE.local_file_identity(path)
        require(actual.get("sha256") == entry.get("sha256"), f"{workload_id} SHA-256 mismatch")
        require(actual.get("bytes") == entry.get("bytes"), f"{workload_id} byte count mismatch")
        identity = {
            "id": workload_id,
            "sha256": entry.get("sha256"),
            "bytes": entry.get("bytes"),
            "splat_count": entry.get("splat_count"),
            "sh_degree": entry.get("sh_degree"),
        }
        require(
            type(identity["splat_count"]) is int and identity["splat_count"] > 0,
            f"{workload_id} splat count is invalid",
        )
        workloads.append(Workload(workload_id, path, identity, expected_role))

    trace_entry = next(
        (
            item
            for item in matrix.get("traces", [])
            if isinstance(item, dict) and item.get("id") == TRACE_ID
        ),
        None,
    )
    require(isinstance(trace_entry, dict), "matrix lacks the A065 Truck trace")
    require(trace_entry.get("dataset_id") == "truck-full", "A065 trace source drifted")
    require(
        (trace_entry.get("width"), trace_entry.get("height")) == FORMAL_SIZE,
        "A065 trace resolution drifted",
    )
    trace_path = (REPO_ROOT / str(trace_entry.get("local_path"))).resolve()
    require(trace_path.is_file(), f"A065 trace is unavailable: {trace_path}")
    checked = subprocess.run(
        [sys.executable, str(TRACE_VALIDATOR), str(trace_path)],
        cwd=REPO_ROOT,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    require(
        checked.returncode == 0,
        f"A065 trace validator rejected input: {checked.stderr.strip()}",
    )
    trace = json.loads(trace_path.read_text(encoding="utf-8"))
    require(trace.get("trace_id") == TRACE_ID, "A065 trace id mismatch")
    require(
        trace.get("content_sha256") == trace_entry.get("sha256"),
        "A065 trace content hash mismatch",
    )
    trace["file_sha256"] = BASE.sha256_file(trace_path)
    return workloads, trace, trace_path


def validate_a065_receipt(receipt: dict[str, Any]) -> None:
    require_matrix_identity(
        receipt.get("manufacturer") == "Nothing",
        "device manufacturer is not Nothing",
    )
    require_matrix_identity(receipt.get("model") == "A065", "device model is not A065")
    device = receipt.get("device")
    require_matrix_identity(
        isinstance(device, str) and device.lower() == "pong",
        "device codename is not Pong",
    )
    properties = receipt.get("device_properties")
    require_matrix_identity(
        isinstance(properties, dict), "A065 device properties are missing"
    )
    soc = properties.get("soc_model_property")
    require_matrix_identity(
        isinstance(soc, dict) and soc.get("value") == "SM8475",
        "A065 SoC identity is not SM8475",
    )


def validate_a065_artifact_renderer(manifest: dict[str, Any]) -> None:
    """Validate Vulkan/Adreno identity without inventing an adapter string.

    Android currently cannot always surface wgpu's adapter name through the
    benchmark ABI.  When that field is unavailable, retain the null and bind
    the endpoint to the independently captured A065 device receipt instead.
    A reported adapter remains authoritative and may not use this fallback.
    """

    renderer = manifest.get("renderer")
    require(isinstance(renderer, dict), "Q3 renderer receipt is missing")
    require(renderer.get("backend") == "vulkan", "A065 artifact backend is not Vulkan")
    environment = manifest.get("environment")
    require(isinstance(environment, dict), "Q3 environment receipt is missing")
    adapter = environment.get("adapter")
    if adapter is not None:
        require(
            isinstance(adapter, str) and "adreno" in adapter.lower(),
            "A065 artifact adapter is not Adreno",
        )
        return

    unavailable = manifest.get("unavailable_fields")
    require(
        isinstance(unavailable, list) and "environment.adapter" in unavailable,
        "unavailable A065 adapter is not declared",
    )
    device_receipt = environment.get("android_device_receipt")
    require(
        isinstance(device_receipt, dict),
        "unavailable A065 adapter lacks its device receipt",
    )
    validate_a065_receipt(device_receipt)
    properties = device_receipt.get("device_properties")
    vulkan_hal = properties.get("vulkan_hal_property")
    require(
        isinstance(vulkan_hal, dict)
        and str(vulkan_hal.get("value", "")).lower() == "adreno",
        "A065 Vulkan HAL is not Adreno",
    )


A065_IDENTITY_FIELDS = (
    "schema",
    "source",
    "serial",
    "manufacturer",
    "model",
    "device",
    "android_release",
    "android_sdk",
    "hardware",
    "build_fingerprint",
    "device_properties",
    "renderer_identity",
)


def a065_identity(receipt: dict[str, Any]) -> dict[str, Any]:
    """Return the immutable device identity, excluding launch readiness state."""

    return {field: receipt.get(field) for field in A065_IDENTITY_FIELDS}


def preflight_a065(args: argparse.Namespace) -> dict[str, Any]:
    try:
        adb = BASE.resolve_adb(args.adb, dry_run=False)
        receipt = BASE.build_android_environment_receipt(
            BASE.device_info(adb, args.serial)
        )
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        raise EnvironmentPrerequisiteError(
            f"A065 device prerequisite is unavailable before protocol launch: {error}"
        ) from error
    validate_a065_receipt(receipt)
    try:
        receipt["launch_readiness"] = BASE.ensure_android_launch_screen_ready(
            adb, args.serial
        )
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise EnvironmentPrerequisiteError(
            f"A065 screen readiness is unavailable before build/install/launch: {error}"
        ) from error
    return receipt


def validate_lane_build_receipts(receipts: dict[str, dict[str, Any]]) -> None:
    require(set(receipts) == set(LANE_BY_NAME), "Q3 lane build set is incomplete")
    for lane in LANES:
        receipt = receipts[lane.name]
        require(receipt.get("lane") == lane.name, f"{lane.name} lane identity drifted")
        require(
            receipt.get("cargo_feature") == "qualification-q3-cpu-runtime",
            "Q3 runtime Cargo feature drifted",
        )
        require(
            receipt.get("selector_environment") == {Q3_LANE_ENV: Q3_RUNTIME_BUILD},
            "Q3 runtime build selector drifted",
        )
        for artifact in ("apk", "native_library"):
            identity = receipt.get(artifact)
            require(isinstance(identity, dict), f"{lane.name} {artifact} identity is missing")
            digest = identity.get("sha256")
            require(
                isinstance(digest, str)
                and len(digest) == 64
                and set(digest).issubset(set("0123456789abcdef")),
                f"{lane.name} {artifact} hash is invalid",
            )
    require(
        receipts["scalar"]["apk"] == receipts["neon"]["apk"]
        and receipts["scalar"]["native_library"] == receipts["neon"]["native_library"],
        "Scalar and Neon lanes must share one APK and native-library identity",
    )


def _environment_build_failure(stdout: str, stderr: str) -> bool:
    text = f"{stdout}\n{stderr}".lower()
    return any(
        marker in text
        for marker in (
            "android_sdk_root not found",
            "ndk not found",
            "android clang not found",
            "android llvm-strip not found",
            "no such file or directory",
            "could not execute process",
            "toolchain is not installed",
            "target may not be installed",
            "java_home",
        )
    )


def android_clang(environment: dict[str, str]) -> pathlib.Path:
    sdk_value = environment.get("ANDROID_SDK_ROOT") or environment.get("ANDROID_HOME")
    if not sdk_value:
        sdk_value = str(pathlib.Path.home() / "Library/Android/sdk")
    sdk = pathlib.Path(sdk_value).expanduser()
    if not sdk.is_dir():
        raise EnvironmentPrerequisiteError(f"ANDROID_SDK_ROOT is unavailable: {sdk}")
    host_candidates = (
        ("darwin-arm64", "darwin-x86_64")
        if sys.platform == "darwin" and os.uname().machine == "arm64"
        else (("darwin-x86_64", "darwin-arm64") if sys.platform == "darwin" else ("linux-x86_64",))
    )
    for host in host_candidates:
        clang = (
            sdk
            / "ndk"
            / ANDROID_NDK_VERSION
            / "toolchains/llvm/prebuilt"
            / host
            / "bin"
            / f"aarch64-linux-android{ANDROID_API_LEVEL}-clang"
        )
        if clang.is_file() and os.access(clang, os.X_OK):
            return clang
    raise EnvironmentPrerequisiteError(
        f"Android NDK {ANDROID_NDK_VERSION} AArch64 clang is unavailable under {sdk}"
    )


def build_element_oracle(
    stage: pathlib.Path, expected_git: dict[str, Any]
) -> dict[str, Any]:
    directory = stage / "element-oracle"
    directory.mkdir()
    environment = android_build_environment(None)
    clang = android_clang(environment)
    environment["CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"] = str(clang)
    environment.setdefault("CARGO_BUILD_JOBS", "1")
    command = [
        "cargo",
        "test",
        "-p",
        "gsplat-sort",
        "--locked",
        "--release",
        "--target",
        ANDROID_TARGET,
        "--lib",
        "--no-run",
        "--message-format=json",
    ]
    write_json(directory / "command.json", {"argv": command, "attempts": 1})
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    (directory / "stdout.log").write_text(completed.stdout, encoding="utf-8")
    (directory / "stderr.log").write_text(completed.stderr, encoding="utf-8")
    if completed.returncode != 0:
        error_type = EnvironmentPrerequisiteError if _environment_build_failure(
            completed.stdout, completed.stderr
        ) else IntegrityRejectedError
        raise error_type(f"A065 element-oracle build exited with {completed.returncode}")
    executables = []
    for line in completed.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        executable = message.get("executable")
        profile = message.get("profile")
        target = message.get("target")
        if (
            message.get("reason") == "compiler-artifact"
            and isinstance(executable, str)
            and isinstance(profile, dict)
            and profile.get("test") is True
            and isinstance(target, dict)
            and target.get("name") == "gsplat_sort"
        ):
            executables.append(pathlib.Path(executable))
    require(len(executables) == 1, "A065 element-oracle test executable is ambiguous")
    executable = executables[0]
    require(executable.is_file(), "A065 element-oracle test executable is missing")
    retained = directory / "gsplat-sort-a065-element-oracle"
    shutil.copy2(executable, retained)
    require(git_receipt() == expected_git, "git receipt changed during element-oracle build")
    return {
        "target": ANDROID_TARGET,
        "test": PARITY_TEST,
        "binary_path": str(retained.relative_to(stage)),
        "binary": BASE.local_file_identity(retained),
        "command": str((directory / "command.json").relative_to(stage)),
        "attempts": 1,
    }


def validate_element_parity_receipt(receipt: dict[str, Any]) -> None:
    require(receipt.get("schema") == PARITY_SCHEMA, "A065 element-parity schema drifted")
    require(receipt.get("decision") == "Accepted", "A065 element parity was not Accepted")
    require(receipt.get("target_arch") == "aarch64", "element parity did not run on AArch64")
    require(receipt.get("neon_required_by_target") is True, "AArch64 Neon receipt is missing")
    correctness = receipt.get("correctness")
    required = {
        "key",
        "source_id",
        "nan_bits",
        "boundary_bits",
        "fma_derived_key",
        "stable_tie",
    }
    require(
        isinstance(correctness, dict)
        and set(correctness) == required
        and all(correctness.values()),
        "A065 Scalar element oracle parity is incomplete",
    )
    kernels = receipt.get("kernels")
    require(isinstance(kernels, dict) and set(kernels) == {"scalar", "neon"}, "kernel receipt is incomplete")
    for lane in ("scalar", "neon"):
        require(
            isinstance(kernels[lane], dict) and kernels[lane].get("executed") is True,
            f"A065 {lane} kernel-executed receipt is missing",
        )
    require(receipt.get("whole_plan_promotion") is False, "element oracle cannot promote a plan")


def run_element_oracle(
    args: argparse.Namespace,
    stage: pathlib.Path,
    build: dict[str, Any],
    phase: ProtocolPhase,
) -> dict[str, Any]:
    adb = BASE.resolve_adb(args.adb, dry_run=False)
    binary = stage / str(build["binary_path"])
    remote_binary = "/data/local/tmp/gsplat-q3-a065-element-oracle"
    remote_receipt = "/data/local/tmp/gsplat-q3-a065-element-parity.json"
    phase.transition("install")
    try:
        BASE.run_command(BASE.adb_args(adb, args.serial, "push", str(binary), remote_binary))
        BASE.run_command(BASE.adb_args(adb, args.serial, "shell", "chmod", "700", remote_binary))
        BASE.run_command(BASE.adb_args(adb, args.serial, "shell", "rm", "-f", remote_receipt))
        phase.transition("launched")
        command = BASE.adb_args(
            adb,
            args.serial,
            "shell",
            f"GSPLAT_Q3_A065_PARITY_RECEIPT={remote_receipt}",
            remote_binary,
            PARITY_TEST,
            "--exact",
            "--ignored",
            "--nocapture",
        )
        completed = subprocess.run(
            command,
            cwd=REPO_ROOT,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=args.run_timeout_seconds,
        )
        write_json(stage / "element-oracle-run-command.json", {"argv": command, "attempts": 1})
        (stage / "element-oracle-run-stdout.log").write_text(completed.stdout, encoding="utf-8")
        (stage / "element-oracle-run-stderr.log").write_text(completed.stderr, encoding="utf-8")
        if completed.returncode != 0:
            raise IntegrityRejectedError(
                f"physical A065 element oracle exited with {completed.returncode} after launch"
            )
        receipt_text = BASE.run_command(
            BASE.adb_args(adb, args.serial, "exec-out", "cat", remote_receipt), capture=True
        ).stdout
        receipt = json.loads(receipt_text)
        validate_element_parity_receipt(receipt)
        phase.transition("evidence")
        receipt["binary"] = build["binary"]
        receipt["attempts"] = 1
        receipt["phase_receipt"] = str(phase.path.relative_to(stage))
        write_json(stage / "element-parity.json", receipt)
        return receipt
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError) as error:
        if phase.phase in {"launched", "evidence"}:
            raise IntegrityRejectedError(f"physical A065 element oracle failed after launch: {error}") from error
        raise
    finally:
        subprocess.run(
            BASE.adb_args(adb, args.serial, "shell", "rm", "-f", remote_binary, remote_receipt),
            cwd=REPO_ROOT,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def build_lane_apks(
    stage: pathlib.Path, expected_git: dict[str, Any]
) -> dict[str, dict[str, Any]]:
    build_root = stage / "build"
    build_root.mkdir()
    lane_dir = build_root / Q3_RUNTIME_BUILD
    lane_dir.mkdir()
    command = ["bash", str(BASE.BUILD_SCRIPT), str(BASE.APK_BOOTSTRAP_DATASET)]
    environment = android_build_environment(Q3_RUNTIME_BUILD)
    write_json(
        lane_dir / "command.json",
        {
            "argv": command,
            "environment": {
                "ANDROID_RUST_PROFILE": "release",
                Q3_LANE_ENV: Q3_RUNTIME_BUILD,
            },
        },
    )
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    (lane_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
    (lane_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
    if completed.returncode != 0:
        error_type = EnvironmentPrerequisiteError if _environment_build_failure(
            completed.stdout, completed.stderr
        ) else IntegrityRejectedError
        raise error_type(f"Q3 runtime APK build exited with {completed.returncode}")
    require(
        f"qualification_q3_cpu_lane={Q3_RUNTIME_BUILD}" in completed.stdout,
        "Q3 runtime build did not attest its controlled selector",
    )
    built = BASE.resolve_apk()
    retained = lane_dir / "sample-app-debug.apk"
    shutil.copy2(built, retained)
    common = {
        "cargo_feature": "qualification-q3-cpu-runtime",
        "selector_environment": {Q3_LANE_ENV: Q3_RUNTIME_BUILD},
        "apk_path": str(retained.relative_to(stage)),
        "apk": BASE.local_file_identity(retained),
        "native_library": BASE.sha256_apk_member(retained, BASE.APK_NATIVE_LIBRARY),
        "command": str((lane_dir / "command.json").relative_to(stage)),
        "stdout": str((lane_dir / "stdout.log").relative_to(stage)),
        "stderr": str((lane_dir / "stderr.log").relative_to(stage)),
    }
    receipts = {lane.name: {"lane": lane.name, **common} for lane in LANES}
    require(git_receipt() == expected_git, "git receipt changed during Q3 runtime build")
    validate_lane_build_receipts(receipts)
    return receipts


def collector_command(
    args: argparse.Namespace,
    workload: Workload,
    trace_path: pathlib.Path,
    apk: pathlib.Path,
    output: pathlib.Path,
    *,
    warmup: int,
    measured: int,
    capture_png: bool,
    phase_receipt: pathlib.Path,
    prepared_inputs_receipt: pathlib.Path,
    installed_apk_receipt: pathlib.Path,
    run_identity: str,
    lane_name: str,
) -> list[str]:
    require(lane_name in LANE_BY_NAME, "Q3 collector command lane is invalid")
    command = [
        sys.executable,
        str(BASE_PATH),
        "--serial",
        args.serial,
        "--ply",
        str(workload.path),
        "--apk",
        str(apk),
        "--backend",
        "cpu",
        "--repetitions",
        "1",
        "--camera-trace",
        str(trace_path),
        "--camera-frame-indices",
        "0,1",
        "--geometry-path",
        "packed",
        "--sort-interval",
        "1",
        "--frames",
        str(measured),
        "--warmup",
        str(warmup),
        "--frame-latency",
        "2",
        "--qualification-q3-phase-receipt",
        str(phase_receipt),
        "--qualification-q3-run-identity",
        run_identity,
        "--qualification-q3-prepared-inputs",
        str(prepared_inputs_receipt),
        "--qualification-q3-installed-apk-receipt",
        str(installed_apk_receipt),
        "--qualification-q3-cpu-kernel",
        lane_name,
        "--max-thermal-status",
        str(args.max_thermal_status),
        "--thermal-timeout-seconds",
        str(args.thermal_timeout_seconds),
        "--run-timeout-seconds",
        str(args.run_timeout_seconds),
        "--output",
        str(output),
    ]
    if capture_png:
        command.append("--qualification-q3-capture-final-png")
    if args.adb is not None:
        command.extend(["--adb", str(args.adb)])
    return command


def read_protocol_phase(path: pathlib.Path) -> dict[str, Any]:
    receipt = json.loads(path.read_text(encoding="utf-8"))
    require(receipt.get("schema") == PHASE_SCHEMA, "Q3 protocol phase schema drifted")
    phase = receipt.get("phase")
    phases = ("preflight", "build", "install", "launched", "evidence")
    require(phase in phases, "Q3 protocol phase is invalid")
    history = receipt.get("history")
    require(isinstance(history, list) and history, "Q3 protocol phase history is missing")
    require(history[-1].get("phase") == phase, "Q3 protocol phase/history drifted")
    expected_history = list(phases[: phases.index(phase) + 1])
    require(
        [item.get("phase") for item in history] == expected_history,
        "Q3 protocol phase history is incomplete or out of order",
    )
    return receipt


def validate_range_rejection_receipt(
    receipt: Any,
    phase: str,
    workload: Workload,
    expected_run_identity: str,
) -> dict[str, Any]:
    require(isinstance(receipt, dict), "range rejection receipt is not an object")
    require(
        phase in {"launched", "evidence"},
        "range rejection receipt predates Activity launch",
    )
    require(
        receipt.get("schema") == RANGE_REJECTION_SCHEMA,
        "range rejection receipt schema drifted",
    )
    require(
        receipt.get("producer") == "renderer_scene_admission",
        "range rejection receipt producer is not renderer scene admission",
    )
    require(
        receipt.get("reason") in {"capacity", "admission", "resource_range"},
        "range rejection receipt reason is invalid",
    )
    require(
        receipt.get("decision") == "Rejected",
        "range rejection receipt decision is not Rejected",
    )
    require(
        receipt.get("activity_phase") == phase,
        "range rejection receipt Activity phase is not bound",
    )
    identity = receipt.get("workload")
    require(isinstance(identity, dict), "range rejection workload identity is missing")
    require(
        identity.get("dataset_id") == workload.id
        and identity.get("dataset_sha256") == workload.identity.get("sha256")
        and identity.get("splat_count") == workload.identity.get("splat_count"),
        "range rejection workload identity drifted",
    )
    require(
        receipt.get("receipt_id") == expected_run_identity,
        "range rejection receipt_id does not match the host-issued run identity",
    )
    return receipt


def nonzero_collector_error(
    returncode: int,
    phase_receipt: pathlib.Path,
    context: str,
    output: pathlib.Path | None = None,
    workload: Workload | None = None,
    expected_run_identity: str | None = None,
) -> QualificationError | None:
    if returncode == 0:
        return None
    phase = read_protocol_phase(phase_receipt)["phase"]
    if output is not None:
        experiment_path = output / "experiment.json"
        if experiment_path.is_file():
            experiment = json.loads(experiment_path.read_text(encoding="utf-8"))
            if (
                expected_run_identity is not None
                and experiment.get("qualification_q3_run_identity")
                != expected_run_identity
            ):
                return MatrixInfrastructureError(
                    f"{context} collector run identity drifted"
                )
            range_receipt = experiment.get("range_rejection_receipt")
            if range_receipt is not None:
                if workload is None or expected_run_identity is None:
                    return IntegrityRejectedError(
                        f"{context} range receipt lacks its host-issued identity binding"
                    )
                try:
                    validate_range_rejection_receipt(
                        range_receipt,
                        phase,
                        workload,
                        expected_run_identity,
                    )
                except IntegrityRejectedError as error:
                    return error
                return RangeAdmissionRejectedError(
                    f"{context} published a bound renderer scene-admission range rejection"
                )
    if phase in {"launched", "evidence"}:
        return IntegrityRejectedError(
            f"{context} exited with {returncode} in explicit Android phase {phase}"
        )
    return IntegrityRejectedError(
        f"{context} exited with {returncode} in phase {phase}; no explicit environment prerequisite was recorded"
    )


def distribution(values: Sequence[float]) -> dict[str, float | int]:
    require(bool(values), "timing distribution is empty")
    return {
        "count": len(values),
        "mean": statistics.fmean(values),
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
    }


def _finite_frame_metric(frame: dict[str, Any], field: str, index: int) -> float:
    value = frame.get(field)
    require(
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(float(value))
        and float(value) >= 0.0,
        f"frame {index} lacks finite {field}",
    )
    return float(value)


def collected_run_metrics(
    frames: Sequence[dict[str, Any]], capture_png: bool
) -> dict[str, list[float]]:
    """Keep correctness capture independent from optional order timing evidence."""

    metrics = {
        field: [
            _finite_frame_metric(frame, field, index)
            for index, frame in enumerate(frames)
        ]
        for field in ("call_ms", "frame_wall_ms")
    }
    for field in ("preprocess_ms", "sort_ms", "cpu_frame_complete_ms"):
        values = [frame.get(field) for frame in frames]
        if all(value is None for value in values):
            require(
                capture_png,
                f"Q3 timing run lacks required {field} producer evidence",
            )
            continue
        require(
            all(value is not None for value in values),
            f"Q3 {field} evidence is only partially available",
        )
        metrics[field] = [
            _finite_frame_metric(frame, field, index)
            for index, frame in enumerate(frames)
        ]
    return metrics


def validate_terminal_kernel_attestation(log_text: str, lane: str) -> int:
    require(lane in LANE_BY_NAME, "Q3 terminal attestation lane is invalid")
    terminal_kernels = re.findall(
        r"SURFACE_CURRENT_STATS_TERMINAL .*?qualification_cpu_kernel=([^\s]+)(?:\s|$)",
        log_text,
    )
    require(
        terminal_kernels,
        "Q3 run lacks ticket-bound Rust CPU-kernel terminal attestation",
    )
    require(
        set(terminal_kernels) == {lane},
        f"Q3 requested {lane} but ticket-bound Rust terminals attested "
        f"{sorted(set(terminal_kernels))}",
    )
    return len(terminal_kernels)


def validate_collected_run(
    output: pathlib.Path,
    lane: str,
    lane_receipt: dict[str, Any],
    workload: Workload,
    trace: dict[str, Any],
    expected_git: dict[str, Any],
    prepared_workload: PreparedWorkload,
    installed_receipt_path: pathlib.Path,
    expected_run_identity: str,
    *,
    capture_png: bool,
) -> dict[str, Any]:
    experiment_path = output / "experiment.json"
    require(experiment_path.is_file(), "Android collector experiment receipt is missing")
    experiment = json.loads(experiment_path.read_text(encoding="utf-8"))
    require(experiment.get("status") == "complete", "Android collector experiment is incomplete")
    require_matrix_identity(
        experiment.get("qualification_q3_run_identity") == expected_run_identity,
        "Android collector run identity drifted",
    )
    require_matrix_identity(
        experiment.get("repository") == expected_git,
        "run git receipt drifted",
    )
    run_apk = experiment.get("apk")
    require_matrix_identity(isinstance(run_apk, dict), "run APK identity is missing")
    for field in ("bytes", "sha256"):
        require_matrix_identity(
            run_apk.get(field) == lane_receipt["apk"][field],
            f"run APK {field} drifted",
        )
    require_matrix_identity(
        experiment.get("native_library", {}).get("sha256")
        == lane_receipt["native_library"]["sha256"],
        "run native-library hash drifted",
    )
    require_matrix_identity(
        experiment.get("installed_apk", {}).get("sha256")
        == lane_receipt["apk"]["sha256"],
        "installed APK hash drifted",
    )
    require_matrix_identity(
        experiment.get("dataset", {}).get("sha256")
        == workload.identity["sha256"],
        "run dataset hash drifted",
    )
    prepared_receipt = experiment.get("prepared_inputs_receipt")
    require_matrix_identity(
        isinstance(prepared_receipt, dict)
        and prepared_receipt.get("preparation_id")
        == prepared_workload.receipt["preparation_id"],
        "run prepared-input receipt drifted",
    )
    installed_receipt = experiment.get("installed_apk_receipt")
    require_matrix_identity(
        isinstance(installed_receipt, dict)
        and installed_receipt.get("lane") == Q3_RUNTIME_BUILD,
        "run installed runtime-APK receipt drifted",
    )
    for name, receipt, expected_path in (
        ("prepared-input", prepared_receipt, prepared_workload.path),
        ("installed-APK", installed_receipt, installed_receipt_path),
    ):
        receipt_path = (output / str(receipt.get("path"))).resolve()
        require_matrix_identity(
            receipt_path == expected_path.resolve(),
            f"run {name} receipt path drifted",
        )
        actual_receipt_identity = BASE.local_file_identity(receipt_path)
        require_matrix_identity(
            all(
                receipt.get(field) == actual_receipt_identity[field]
                for field in ("bytes", "sha256")
            ),
            f"run {name} receipt hash drifted",
        )
    configuration = experiment.get("configuration")
    require(isinstance(configuration, dict), "run configuration is missing")
    require(configuration.get("backends") == ["cpu"], "Q3 run is not forced CPU")
    require(configuration.get("geometry_path") == "packed", "Q3 run is not Packed")
    require(
        configuration.get("qualification_q3_cpu_kernel") == lane,
        "Q3 requested CPU kernel drifted",
    )
    require(
        configuration.get("qualification_q3_capture_final_png") is capture_png,
        "Q3 run final-PNG mode drifted",
    )
    environment_path = output / "android-environment-receipt.json"
    require_matrix_identity(
        environment_path.is_file(), "Android environment receipt is missing"
    )
    environment = json.loads(environment_path.read_text(encoding="utf-8"))
    validate_a065_receipt(environment)
    runs = experiment.get("runs")
    require(isinstance(runs, list) and len(runs) == 1, "Q3 command must publish exactly one run")
    run = runs[0]
    require(run.get("status") == "complete", "Q3 Android run is incomplete")
    require_matrix_identity(
        run.get("qualification_q3_run_identity") == expected_run_identity,
        "Q3 retained run identity drifted",
    )
    run_prepared = run.get("prepared_inputs")
    require_matrix_identity(
        isinstance(run_prepared, dict)
        and run_prepared.get("preparation_id")
        == prepared_workload.receipt["preparation_id"]
        and run_prepared.get("workload") == workload.id,
        "Q3 run identity is not bound to its prepared workload",
    )
    artifact = output / str(run.get("artifact"))
    manifest = json.loads((artifact / "manifest.json").read_text(encoding="utf-8"))
    summary = json.loads((artifact / "summary.json").read_text(encoding="utf-8"))
    frames = BASE.read_artifact_frames(artifact / "frames.jsonl")
    log_text = (artifact.parent / "logcat.txt").read_text(
        encoding="utf-8", errors="replace"
    )
    terminal_record_count = validate_terminal_kernel_attestation(log_text, lane)
    BASE.validate_run_artifact(
        manifest,
        summary,
        frames,
        "cpu",
        "packed",
        experiment["dataset"],
        trace,
        experiment["trace"],
        None,
        environment,
    )
    renderer = manifest.get("renderer", {})
    benchmark_run_id = manifest.get("run_id")
    require(
        isinstance(benchmark_run_id, str) and bool(benchmark_run_id),
        "Q3 benchmark run identity is missing",
    )
    require(renderer.get("path") == "packed_atlas", "Q3 run did not execute Packed")
    require(renderer.get("order_backend_requested") == "cpu", "Q3 run did not request Exact CPU")
    validate_a065_artifact_renderer(manifest)
    exactness = manifest.get("exactness")
    require(isinstance(exactness, dict), "Q3 exactness receipt is missing")
    for field in (
        "source_splat_count",
        "decoded_splat_count",
        "encoded_splat_count",
        "resident_splat_count",
        "addressable_splat_count",
    ):
        require(
            exactness.get(field) == workload.identity["splat_count"],
            f"Q3 {field} is incomplete",
        )
    for field in ("source_sh_degree", "resident_sh_degree"):
        require(exactness.get(field) == workload.identity["sh_degree"], f"Q3 {field} drifted")
    for field, expected in {
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "partial_scene_published": False,
        "full_quality": True,
    }.items():
        require(exactness.get(field) == expected, f"Q3 exactness {field} drifted")
    image = manifest.get("image")
    if capture_png:
        require(isinstance(image, dict), "Q3 exact final PNG receipt is missing")
        require((image.get("width"), image.get("height")) == FORMAL_SIZE, "Q3 PNG resolution drifted")
    else:
        require(image is None, "Q3 timing run unexpectedly captured a PNG")
    metrics = collected_run_metrics(frames, capture_png)
    for index, frame in enumerate(frames):
        require(frame.get("drawn") == frame.get("visible"), f"frame {index} violates Exact CPU D=V")
    semantic_frames = [
        {
            field: frame.get(field)
            for field in (
                "trace_frame_index",
                "trace_timestamp_ns",
                "camera_receipt",
                "visible",
                "contributor",
                "drawn",
                "exact_contributor_compaction",
                "sort_refreshed",
                "order_backend",
                "current_stats_executed_plan",
            )
        }
        for frame in frames
    ]
    return {
        "lane": lane,
        "workload": workload.id,
        "apk_sha256": lane_receipt["apk"]["sha256"],
        "native_library_sha256": lane_receipt["native_library"]["sha256"],
        "preparation_id": prepared_workload.receipt["preparation_id"],
        "install_sequence": installed_receipt["install_sequence"],
        "benchmark_run_id": benchmark_run_id,
        "collector_run_identity": expected_run_identity,
        "qualification_cpu_kernel": {
            "requested": lane,
            "realized": lane,
            "owner": "gsplat-sort qualification runtime selector",
            "scope": "radix_histogram_and_value_unpack",
            "session_immutable": True,
            "terminal_records": terminal_record_count,
        },
        "environment": environment,
        "adapter": manifest.get("environment", {}).get("adapter"),
        "backend": renderer.get("backend"),
        "image_sha256": image.get("sha256") if isinstance(image, dict) else None,
        "semantic_fingerprint": {
            "dataset": workload.identity,
            "trace_id": trace.get("trace_id"),
            "trace_content_sha256": trace.get("content_sha256"),
            "exactness": {key: value for key, value in exactness.items() if key != "receipt_id"},
            "image_sha256": image.get("sha256") if isinstance(image, dict) else None,
            "frames": semantic_frames,
        },
        "distributions": {field: distribution(values) for field, values in metrics.items()},
        "artifact": str(artifact.relative_to(output.parent)),
    }


def bind_device_identity(result: dict[str, Any], run: dict[str, Any]) -> None:
    expected_environment = result.get("preflight_device")
    run_environment = run.get("environment")
    require_matrix_identity(
        isinstance(expected_environment, dict)
        and isinstance(run_environment, dict)
        and a065_identity(expected_environment) == a065_identity(run_environment),
        "A065 preflight/run device identity changed during Q3",
    )
    identity = {
        "android_environment_receipt": run_environment,
        "adapter": run.get("adapter"),
        "backend": run.get("backend"),
    }
    prior = result.get("device")
    if prior is None:
        result["device"] = identity
    else:
        require_matrix_identity(
            prior == identity,
            "A065 device/adapter identity changed during Q3",
        )


def validate_image_control_pair(scalar: dict[str, Any], neon: dict[str, Any]) -> None:
    require(scalar.get("lane") == "scalar", "image control baseline is not Scalar")
    require(neon.get("lane") == "neon", "image control candidate is not Neon")
    require(scalar.get("workload") == neon.get("workload"), "image control workload drifted")
    require(
        scalar.get("semantic_fingerprint") == neon.get("semantic_fingerprint"),
        f"Neon image control differs from Scalar for {scalar.get('workload')}",
    )


def paired_metric(pairs: Sequence[dict[str, Any]], metric: str) -> dict[str, Any]:
    deltas = [
        pair["runs"]["neon"]["distributions"][metric]["mean"]
        - pair["runs"]["scalar"]["distributions"][metric]["mean"]
        for pair in pairs
    ]
    return {
        "neon_minus_scalar_mean_ms_by_pair": deltas,
        "median_paired_difference_ms": statistics.median(deltas),
        "neon_faster_pair_count": sum(value < 0.0 for value in deltas),
        "scalar_faster_or_tied_pair_count": sum(value >= 0.0 for value in deltas),
    }


def workload_decision(pairs: Sequence[dict[str, Any]]) -> tuple[str, str]:
    require(len(pairs) == DEFAULT_PAIRS, "formal Q3 workload matrix is incomplete")
    completion = paired_metric(pairs, "cpu_frame_complete_ms")
    if completion["neon_faster_pair_count"] == DEFAULT_PAIRS:
        return "Accepted", "stable_neon_whole_plan_queue_completion_benefit"
    return "Rejected", "no_stable_neon_whole_plan_benefit"


def diagnostic_decision(pairs: Sequence[dict[str, Any]]) -> tuple[str, str]:
    require(len(pairs) == 1, "point-ladder diagnostic requires exactly one pair")
    completion = paired_metric(pairs, "cpu_frame_complete_ms")
    if completion["neon_faster_pair_count"] == 1:
        return "Accepted", "single_pair_diagnostic_neon_faster_no_promotion"
    return "Rejected", "single_pair_diagnostic_no_neon_benefit_no_promotion"


def rejects_larger_workload_range(cell: dict[str, Any]) -> bool:
    return (
        cell.get("decision") == "Rejected"
        and cell.get("reason") == "capacity_or_admission_range_rejected"
    )


def range_rejected_at_after_cell(
    prior: str | None, cell: dict[str, Any]
) -> str | None:
    if prior is not None:
        return prior
    if rejects_larger_workload_range(cell):
        workload = cell.get("workload")
        require(isinstance(workload, str) and bool(workload), "range cell lacks workload")
        return workload
    return None


def remove_private_apks(stage: pathlib.Path) -> None:
    build = stage / "build"
    if build.is_dir():
        for apk in build.glob("*/sample-app-debug.apk"):
            require(apk.is_file() and not apk.is_symlink(), f"private APK path is unsafe: {apk}")
            apk.unlink()
    oracle = stage / "element-oracle/gsplat-sort-a065-element-oracle"
    if oracle.exists():
        require(oracle.is_file() and not oracle.is_symlink(), f"private oracle path is unsafe: {oracle}")
        oracle.unlink()


def finalize_lane_receipts(result: dict[str, Any]) -> None:
    builds = result.get("lane_builds")
    if not isinstance(builds, dict):
        return
    for receipt in builds.values():
        if isinstance(receipt, dict):
            receipt.pop("apk_path", None)
            receipt["private_apk_retained"] = False
    oracle = result.get("element_oracle_build")
    if isinstance(oracle, dict):
        oracle.pop("binary_path", None)
        oracle["private_binary_retained"] = False


def publish(stage: pathlib.Path, output: pathlib.Path, result: dict[str, Any]) -> None:
    require(result.get("decision") in DECISIONS, "Q3 result is not finite")
    result["ended_at_utc"] = BASE.utc_now()
    write_json(stage / "experiment.json", result)
    require(not list(stage.glob("build/*/sample-app-debug.apk")), "private APK remains in evidence")
    require(not output.exists(), f"Q3 output appeared during collection: {output}")
    os.rename(stage, output)


def terminal_for_error(error: Exception) -> tuple[str, str]:
    if isinstance(error, MatrixInfrastructureError):
        return "Rejected", "matrix_infrastructure_rejected"
    if isinstance(error, IntegrityRejectedError):
        return "Rejected", "integrity_rejected"
    if isinstance(error, EnvironmentPrerequisiteError):
        return "Deferred", "environment_prerequisite"
    return "Rejected", "integrity_rejected"


def cell_failure_scope(error: Exception) -> str:
    if isinstance(error, RangeAdmissionRejectedError):
        return "range_cutoff"
    if isinstance(error, MatrixInfrastructureError):
        return "matrix_infrastructure"
    if isinstance(error, IntegrityRejectedError):
        return "cell_local"
    if isinstance(error, EnvironmentPrerequisiteError):
        return "environment_prerequisite"
    return "cell_local"


def record_cell_failure(
    result: dict[str, Any],
    workload: Workload,
    evidence_class: str,
    error: Exception,
) -> tuple[str, dict[str, Any]]:
    scope = cell_failure_scope(error)
    decision, reason = terminal_for_error(error)
    if scope == "range_cutoff":
        reason = "capacity_or_admission_range_rejected"
    cell = {
        "workload": workload.id,
        "evidence_class": evidence_class,
        "decision": decision,
        "reason": reason,
        "error": str(error),
        "failure_scope": scope,
        "whole_plan_promotion": False,
    }
    result["cells"].append(cell)
    return scope, cell


def run_collection_once(
    args: argparse.Namespace,
    stage: pathlib.Path,
    expected_git: dict[str, Any],
    lane_receipts: dict[str, dict[str, Any]],
    session: DeviceMatrixSession,
    prepared_workload: PreparedWorkload,
    workload: Workload,
    trace: dict[str, Any],
    trace_path: pathlib.Path,
    lane_name: str,
    label: str,
    *,
    warmup: int,
    measured: int,
    capture_png: bool,
) -> dict[str, Any]:
    lane_receipt = lane_receipts[lane_name]
    apk = stage / lane_receipt["apk_path"]
    phase = ProtocolPhase(stage / f"{label}-phase.json", label)
    phase.transition("build")
    phase.transition("install")
    installed_receipt_path, installed_now = ensure_matrix_lane(
        session, lane_name, args.run_timeout_seconds
    )
    run_output = stage / label
    run_identity = uuid.uuid4().hex
    command = collector_command(
        args,
        workload,
        trace_path,
        apk,
        run_output,
        warmup=warmup,
        measured=measured,
        capture_png=capture_png,
        phase_receipt=phase.path,
        prepared_inputs_receipt=prepared_workload.path,
        installed_apk_receipt=installed_receipt_path,
        run_identity=run_identity,
        lane_name=lane_name,
    )
    command_path = stage / f"{label}-command.json"
    write_json(
        command_path,
        {"argv": command, "attempts": 1, "run_identity": run_identity},
    )
    session.measurement_attempt_count += 1
    try:
        completed = subprocess.run(
            command,
            cwd=REPO_ROOT,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        (stage / f"{label}-stdout.log").write_text(
            completed.stdout, encoding="utf-8"
        )
        (stage / f"{label}-stderr.log").write_text(
            completed.stderr, encoding="utf-8"
        )
    except (OSError, subprocess.SubprocessError) as launch_error:
        raise MatrixInfrastructureError(
            f"{label} collector process could not be launched or recorded: {launch_error}"
        ) from launch_error
    try:
        error = nonzero_collector_error(
            completed.returncode,
            phase.path,
            label,
            run_output,
            workload,
            run_identity,
        )
    except (OSError, ValueError, KeyError, TypeError) as parse_error:
        raise IntegrityRejectedError(
            f"{label} evidence protocol could not be parsed: {parse_error}"
        ) from parse_error
    if error is not None:
        raise error
    try:
        current_apk_sha256 = BASE.sha256_file(apk)
        current_git = git_receipt()
    except (OSError, RuntimeError, subprocess.SubprocessError) as identity_error:
        raise MatrixInfrastructureError(
            f"{label} host identity could not be revalidated: {identity_error}"
        ) from identity_error
    require_matrix_identity(
        current_apk_sha256 == lane_receipt["apk"]["sha256"],
        f"{lane_name} APK changed",
    )
    require_matrix_identity(
        current_git == expected_git,
        "git receipt changed during Q3 device matrix",
    )
    try:
        validated = validate_collected_run(
            run_output,
            lane_name,
            lane_receipt,
            workload,
            trace,
            expected_git,
            prepared_workload,
            installed_receipt_path,
            run_identity,
            capture_png=capture_png,
        )
        phase_receipt = read_protocol_phase(phase.path)
    except (MatrixInfrastructureError, IntegrityRejectedError):
        raise
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as evidence_error:
        raise IntegrityRejectedError(
            f"{label} evidence validation failed: {evidence_error}"
        ) from evidence_error
    require(phase_receipt["phase"] == "evidence", f"{label} lacks explicit evidence phase")
    validated.update(
        {
            "command": str(command_path.relative_to(stage)),
            "collector_stdout": f"{label}-stdout.log",
            "collector_stderr": f"{label}-stderr.log",
            "attempts": 1,
            "output": label,
            "phase_receipt": str(phase.path.relative_to(stage)),
            "terminal_phase": phase_receipt["phase"],
            "installation": {
                "receipt": str(installed_receipt_path.relative_to(stage)),
                "replaced_for_run": installed_now,
            },
        }
    )
    return validated


def collect(args: argparse.Namespace) -> dict[str, Any]:
    output = args.output.expanduser().resolve()
    if output.exists():
        raise ValueError(f"Q3 output already exists: {output}")
    require_ignored_or_external_output(output)
    if (
        not isinstance(args.expected_commit, str)
        or len(args.expected_commit) != 40
        or not set(args.expected_commit).issubset(set("0123456789abcdef"))
    ):
        raise ValueError("--expected-commit must be a full 40-character SHA")
    initial_git = git_receipt()
    if initial_git != {"commit": args.expected_commit, "dirty": False}:
        raise ValueError(
            f"Q3 requires clean exact commit {args.expected_commit}, got {initial_git}"
        )
    if args.dry_run:
        return {
            "schema": SCHEMA,
            "cell": CELL,
            "decision": "Deferred",
            "reason": "dry_run_only",
            "repository": initial_git,
            "workloads": list(REQUIRED_WORKLOAD_IDS),
            "correctness_order": ["scalar", "neon"],
            "staged_plan": staged_plan(args.seed),
            "timing_forbidden_until_element_parity": True,
            "device_commands_executed": 0,
        }

    # Device readiness is the only mutable preflight and must happen before a
    # staging/output claim, build, install, or formal Activity launch. It may
    # wake/dismiss once, then fails closed without publishing a Deferred run.
    preflight_device = preflight_a065(args)
    stage = output.with_name(f".{output.name}.staging-{uuid.uuid4().hex}")
    stage.parent.mkdir(parents=True, exist_ok=True)
    stage.mkdir()
    result: dict[str, Any] = {
        "schema": SCHEMA,
        "cell": CELL,
        "decision": "Deferred",
        "reason": "environment_prerequisite",
        "started_at_utc": BASE.utc_now(),
        "repository": initial_git,
        "protocol": {
            "endpoint": "Nothing A065 / Adreno 730 / Vulkan",
            "resolution": list(FORMAL_SIZE),
            "renderer_path": "packed_atlas",
            "order_backend": "cpu",
            "source_membership": "all",
            "correctness_oracle": "physical_a065_scalar_element_order",
            "element_parity": [
                "key",
                "source_id",
                "nan_bits",
                "boundary_bits",
                "fma_derived_key",
                "stable_tie",
            ],
            "image_control": "one_short_scalar_neon_control_per_executed_workload",
            "correctness_before_timing": True,
            "point_ladder": "one_diagnostic_pair_per_tier_no_stable_claim",
            "terminal": "complete_truck_five_matched_pairs",
            "input_preparation": (
                "trace push/copy once per matrix; PLY push/copy once per workload; "
                "measurements reuse hash-bound private receipts"
            ),
            "apk_installation": (
                "build one runtime-selection APK; install and hash-verify it once"
            ),
            "range_stop_rule": (
                "only explicit capacity/admission/resource failure rejects larger tiers; "
                "diagnostic performance loss continues the ladder"
            ),
            "warmup_frames": args.warmup,
            "measured_frames": args.measured,
            "automatic_retries": 0,
            "decision_rule": (
                "Accepted iff Neon current-stats queue completion mean is lower "
                "in all five complete-Truck matched pairs; point-ladder cells are diagnostic"
            ),
        },
        "staged_plan": staged_plan(args.seed),
        "workloads": [],
        "correctness": [],
        "cells": [],
    }
    try:
        workloads, trace, trace_path = load_workloads((), args.matrix)
        result["workloads"] = [workload.identity for workload in workloads]
        result["preflight_device"] = preflight_device
        parity_phase = ProtocolPhase(stage / "element-oracle-phase.json", "element-oracle")
        parity_phase.transition("build")
        oracle_build = build_element_oracle(stage, initial_git)
        lane_receipts = build_lane_apks(stage, initial_git)
        result["lane_builds"] = lane_receipts
        result["element_parity"] = run_element_oracle(
            args, stage, oracle_build, parity_phase
        )
        result["element_oracle_build"] = oracle_build
        by_id = {workload.id: workload for workload in workloads}
        adb = BASE.resolve_adb(args.adb, dry_run=False)
        session = DeviceMatrixSession(
            adb, args.serial, stage, lane_receipts
        )
        ensure_matrix_lane(session, "scalar", args.run_timeout_seconds)
        trace_identity = BASE.local_file_identity(trace_path)
        range_rejected_at = None
        terminal_cell = None
        with staged_matrix_trace(
            session, trace_path, trace_identity
        ) as temporary_trace_path:
            for stage_spec in result["staged_plan"]:
                workload = by_id[stage_spec["workload"]]
                evidence_class = stage_spec["evidence_class"]
                if range_rejected_at is not None:
                    cell = {
                        "workload": workload.id,
                        "evidence_class": evidence_class,
                        "decision": "Rejected",
                        "reason": "outside_capacity_or_admission_range",
                        "range_rejected_at": range_rejected_at,
                        "whole_plan_promotion": False,
                        "pairs": [],
                    }
                    result["cells"].append(cell)
                    if evidence_class == "terminal":
                        terminal_cell = cell
                    continue
                try:
                    ensure_matrix_lane(session, "scalar", args.run_timeout_seconds)
                    with prepared_workload_inputs(
                        session,
                        workload,
                        trace_identity,
                        temporary_trace_path,
                    ) as prepared_workload:
                        lane_results = {}
                        for lane_name in ("scalar", "neon"):
                            lane_results[lane_name] = run_collection_once(
                                args,
                                stage,
                                initial_git,
                                lane_receipts,
                                session,
                                prepared_workload,
                                workload,
                                trace,
                                trace_path,
                                lane_name,
                                f"correctness-{workload.id}-{lane_name}",
                                warmup=0,
                                measured=args.correctness_frames,
                                capture_png=True,
                            )
                            bind_device_identity(result, lane_results[lane_name])
                        validate_image_control_pair(
                            lane_results["scalar"], lane_results["neon"]
                        )
                        result["correctness"].append(
                            {
                                "workload": workload.id,
                                "decision": "Accepted",
                                "oracle": "physical_element_parity_then_scalar_image_control",
                                "candidate": "neon",
                                "scalar": lane_results["scalar"],
                                "neon": lane_results["neon"],
                            }
                        )
                        schedule = [tuple(order) for order in stage_spec["schedule"]]
                        pairs = []
                        for pair_index, order in enumerate(schedule, 1):
                            pair_runs = {}
                            for position, lane_name in enumerate(order, 1):
                                pair_runs[lane_name] = run_collection_once(
                                    args,
                                    stage,
                                    initial_git,
                                    lane_receipts,
                                    session,
                                    prepared_workload,
                                    workload,
                                    trace,
                                    trace_path,
                                    lane_name,
                                    f"timing-{workload.id}-pair-{pair_index:02d}-pos-{position}-{lane_name}",
                                    warmup=args.warmup,
                                    measured=args.measured,
                                    capture_png=False,
                                )
                                bind_device_identity(result, pair_runs[lane_name])
                            pairs.append(
                                {
                                    "pair_index": pair_index,
                                    "order": list(order),
                                    "runs": pair_runs,
                                }
                            )
                        aggregate = {
                            metric: paired_metric(pairs, metric)
                            for metric in (
                                "preprocess_ms",
                                "sort_ms",
                                "call_ms",
                                "frame_wall_ms",
                                "cpu_frame_complete_ms",
                            )
                        }
                        decision, reason = (
                            diagnostic_decision(pairs)
                            if evidence_class == "diagnostic"
                            else workload_decision(pairs)
                        )
                        cell = {
                            "workload": workload.id,
                            "evidence_class": evidence_class,
                            "decision": decision,
                            "reason": reason,
                            "schedule": [list(pair) for pair in schedule],
                            "pairs": pairs,
                            "aggregate": aggregate,
                            "whole_plan_promotion": (
                                evidence_class == "terminal"
                                and decision == "Accepted"
                            ),
                        }
                        result["cells"].append(cell)
                        range_rejected_at = range_rejected_at_after_cell(
                            range_rejected_at, cell
                        )
                        if evidence_class == "terminal":
                            terminal_cell = cell
                except Exception as error:
                    scope, cell = record_cell_failure(
                        result, workload, evidence_class, error
                    )
                    if evidence_class == "terminal":
                        terminal_cell = cell
                    if scope == "range_cutoff":
                        range_rejected_at = range_rejected_at_after_cell(
                            range_rejected_at, cell
                        )
                        continue
                    if scope == "cell_local":
                        continue
                    raise
        require(isinstance(terminal_cell, dict), "complete Truck terminal cell is missing")
        result["device_preparation"] = session.snapshot()
        cell_local_failures = [
            cell
            for cell in result["cells"]
            if cell.get("failure_scope") == "cell_local"
            and cell.get("decision") == "Rejected"
        ]
        if cell_local_failures and terminal_cell["decision"] == "Accepted":
            terminal_cell["whole_plan_promotion"] = False
            result.update(
                {
                    "decision": "Rejected",
                    "reason": "matrix_contains_integrity_rejected_cells",
                }
            )
        else:
            result.update(
                {
                    "decision": terminal_cell["decision"],
                    "reason": terminal_cell["reason"],
                }
            )
        remove_private_apks(stage)
        finalize_lane_receipts(result)
        publish(stage, output, result)
        return result
    except Exception as error:
        decision, reason = terminal_for_error(error)
        result.update({"decision": decision, "reason": reason, "error": str(error)})
        if "session" in locals():
            result["device_preparation"] = session.snapshot()
        remove_private_apks(stage)
        finalize_lane_receipts(result)
        publish(stage, output, result)
        return result


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Collect one finite Q3 A065 Scalar/Neon whole-plan matrix"
    )
    parser.add_argument("--serial", required=True)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--matrix", type=pathlib.Path, default=MATRIX_PATH)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--warmup", type=int, default=DEFAULT_WARMUP)
    parser.add_argument("--measured", type=int, default=DEFAULT_MEASURED)
    parser.add_argument(
        "--correctness-frames", type=int, default=DEFAULT_CORRECTNESS_FRAMES
    )
    parser.add_argument("--max-thermal-status", type=int, default=0)
    parser.add_argument("--thermal-timeout-seconds", type=float, default=300.0)
    parser.add_argument("--run-timeout-seconds", type=float, default=1800.0)
    parser.add_argument("--adb", type=pathlib.Path)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)
    if (args.warmup, args.measured) != (DEFAULT_WARMUP, DEFAULT_MEASURED):
        parser.error(
            f"formal Q3 timing requires warmup={DEFAULT_WARMUP}, measured={DEFAULT_MEASURED}"
        )
    if args.correctness_frames != DEFAULT_CORRECTNESS_FRAMES:
        parser.error(
            f"Q3 correctness requires exactly {DEFAULT_CORRECTNESS_FRAMES} trace frames"
        )
    if not 0 <= args.max_thermal_status <= 6:
        parser.error("--max-thermal-status must be between 0 and 6")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        result = collect(args)
    except (OSError, QualificationError, subprocess.SubprocessError, ValueError) as error:
        classification = (
            "EnvironmentPrerequisite"
            if isinstance(error, EnvironmentPrerequisiteError)
            else type(error).__name__
        )
        print(
            "Q3 A065 SIMD collection failed before terminal publication "
            f"[{classification}]: {error}",
            file=sys.stderr,
        )
        return 1
    print(
        json.dumps(
            {
                "cell": result["cell"],
                "decision": result["decision"],
                "reason": result["reason"],
                "output": str(args.output),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
