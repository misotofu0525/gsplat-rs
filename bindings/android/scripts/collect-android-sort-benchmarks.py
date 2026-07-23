#!/usr/bin/env python3
"""Collect reproducible CPU/GPU/adaptive Android sort benchmark artifacts.

The collector deliberately keeps the app identity fixed to the repository's
debuggable sample package. By default it refuses to run unless the installed
base.apk is byte-for-byte the local APK, then stages the selected PLY once and
restores it with ``run-as`` after every per-run package-data clear. Building and
installing the APK is an explicit one-time preparation mode, not dataset setup.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import datetime as dt
import hashlib
import json
import math
import os
import pathlib
import random
import re
import shlex
import shutil
import struct
import subprocess
import sys
import time
import zipfile
from collections.abc import Sequence
from typing import Any, Iterator


REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
BUILD_SCRIPT = REPO_ROOT / "bindings/android/scripts/build-sample-apk.sh"
APK_BOOTSTRAP_DATASET = REPO_ROOT / "tests/datasets/minimal_ascii.ply"
EXTRACTOR = REPO_ROOT / "bindings/android/scripts/extract-android-benchmark-artifacts.py"
VALIDATOR = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
TRACE_VALIDATOR = REPO_ROOT / "tests/perf/trace/validate_trace_v1.py"
CAMERA_RECEIPT_VALIDATOR = (
    REPO_ROOT / "bindings/android/scripts/validate-android-camera-receipts.py"
)
APK_METADATA = REPO_ROOT / "examples/android/app/build/outputs/apk/debug/output-metadata.json"
APK_DIR = APK_METADATA.parent
APK_NATIVE_LIBRARY = "lib/arm64-v8a/libgsplat_jni.so"
PACKAGE = "com.gsplat.example"
ACTIVITY = f"{PACKAGE}/.MainActivity"
LOG_TAG = "GsplatExample:I"
BACKENDS = ("cpu", "gpu", "adaptive")
GPU_PRODUCERS = ("post_sort", "preproject")
GEOMETRY_PATHS = ("packed", "direct")
RENDERER_PATHS = {
    "packed": "packed_atlas",
    "direct": "sorted_index_direct",
}
INTERNAL_DATASET = "files/imported_scene.ply"
INTERNAL_TRACE = "files/camera_trace.json"
DEVICE_DATASET_PREFIX = "/data/local/tmp/gsplat-benchmark-"
DEVICE_TRACE_PREFIX = "/data/local/tmp/gsplat-camera-trace-"
CAMERA_RECEIPT_SCHEMA = "gsplat-surface-camera-receipt/v1"
CAMERA_RECEIPT_TOLERANCE = 5.0e-5
CANONICAL_COORDINATE_SYSTEM = {
    "handedness": "right",
    "axes": "RUF",
    "camera_forward": "+Z",
}
CANONICAL_MATRIX_CONVENTION = {
    "storage_order": "row-major",
    "vector_convention": "column",
    "composition": "projection * view * world_position",
    "ndc_xy": "[-1,1]",
    "ndc_z": "[0,1]",
    "clip_w": "camera_z",
}


@dataclasses.dataclass(frozen=True)
class RunSpec:
    index: int
    repetition: int
    position: int
    backend: str


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def command_text(args: Sequence[str | os.PathLike[str]]) -> str:
    return shlex.join([os.fspath(arg) for arg in args])


def run_command(
    args: Sequence[str | os.PathLike[str]],
    *,
    cwd: pathlib.Path = REPO_ROOT,
    env: dict[str, str] | None = None,
    capture: bool = False,
    timeout: float | None = None,
) -> subprocess.CompletedProcess[str]:
    print(f"+ {command_text(args)}", flush=True)
    return subprocess.run(
        [os.fspath(arg) for arg in args],
        cwd=cwd,
        env=env,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
        timeout=timeout,
    )


def adb_args(adb: pathlib.Path | str, serial: str, *args: str) -> list[str]:
    return [os.fspath(adb), "-s", serial, *args]


def build_schedule(
    backends: Sequence[str], repetitions: int, randomize_order: bool, seed: int
) -> list[RunSpec]:
    rng = random.Random(seed)
    result: list[RunSpec] = []
    for repetition in range(1, repetitions + 1):
        ordered = list(backends)
        if randomize_order:
            rng.shuffle(ordered)
        for position, backend in enumerate(ordered, start=1):
            result.append(
                RunSpec(
                    index=len(result) + 1,
                    repetition=repetition,
                    position=position,
                    backend=backend,
                )
            )
    return result


def resolve_adb(explicit: pathlib.Path | None, *, dry_run: bool) -> pathlib.Path | str:
    if explicit is not None:
        if not dry_run and not explicit.is_file():
            raise ValueError(f"adb does not exist: {explicit}")
        return explicit

    for variable in ("ANDROID_SDK_ROOT", "ANDROID_HOME"):
        sdk_root = os.environ.get(variable)
        if sdk_root:
            candidate = pathlib.Path(sdk_root) / "platform-tools/adb"
            if candidate.is_file():
                return candidate

    located = shutil.which("adb")
    if located:
        return pathlib.Path(located)
    if dry_run:
        return "adb"
    raise ValueError(
        "adb was not found; pass --adb or set ANDROID_SDK_ROOT/ANDROID_HOME"
    )


def parse_thermal_status(output: str) -> int | None:
    patterns = (
        r"(?im)^\s*thermal\s+status\s*:\s*(\d+)\s*$",
        r"(?im)^\s*status\s*:\s*(\d+)\s*$",
        r"(?m)^\s*(\d+)\s*$",
    )
    for pattern in patterns:
        match = re.search(pattern, output)
        if match:
            return int(match.group(1))
    return None


def read_thermal_status(adb: pathlib.Path | str, serial: str) -> int | None:
    commands = (
        adb_args(adb, serial, "shell", "cmd", "thermalservice", "get-status"),
        adb_args(adb, serial, "shell", "dumpsys", "thermalservice"),
    )
    for command in commands:
        try:
            result = subprocess.run(
                command,
                cwd=REPO_ROOT,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=15,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        if result.returncode == 0:
            status = parse_thermal_status(result.stdout)
            if status is not None:
                return status
    return None


def wait_for_thermal_status(
    adb: pathlib.Path | str,
    serial: str,
    maximum: int,
    timeout_seconds: float,
    poll_seconds: float,
) -> int:
    deadline = time.monotonic() + timeout_seconds
    while True:
        status = read_thermal_status(adb, serial)
        if status is None:
            raise RuntimeError(
                "thermal status is unavailable; cannot enforce --max-thermal-status"
            )
        if status <= maximum:
            return status
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError(
                f"thermal status stayed at {status}, above requested maximum {maximum}"
            )
        delay = min(poll_seconds, remaining)
        print(
            f"thermal_status={status} above_max={maximum} cooling_for={delay:.1f}s",
            flush=True,
        )
        time.sleep(delay)


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def local_file_identity(path: pathlib.Path) -> dict[str, Any]:
    return {"bytes": path.stat().st_size, "sha256": sha256_file(path)}


def sha256_apk_member(apk: pathlib.Path, member: str) -> dict[str, Any]:
    digest = hashlib.sha256()
    try:
        with zipfile.ZipFile(apk) as archive:
            info = archive.getinfo(member)
            with archive.open(info) as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(chunk)
    except (KeyError, zipfile.BadZipFile) as error:
        raise RuntimeError(f"local APK does not contain {member}: {apk}") from error
    return {"bytes": info.file_size, "sha256": digest.hexdigest()}


def device_dataset_path(sha256: str) -> str:
    if re.fullmatch(r"[0-9a-f]{64}", sha256) is None:
        raise ValueError(f"invalid dataset SHA-256 for device path: {sha256!r}")
    return f"{DEVICE_DATASET_PREFIX}{sha256}.ply"


def device_trace_path(sha256: str) -> str:
    if re.fullmatch(r"[0-9a-f]{64}", sha256) is None:
        raise ValueError(f"invalid trace SHA-256 for device path: {sha256!r}")
    return f"{DEVICE_TRACE_PREFIX}{sha256}.json"


def parse_sha256sum(output: str, description: str) -> str:
    match = re.search(r"(?m)^\s*([0-9A-Fa-f]{64})(?:\s|$)", output)
    if match is None:
        raise RuntimeError(f"cannot parse {description} SHA-256: {output!r}")
    return match.group(1).lower()


def read_device_file_identity(
    adb: pathlib.Path | str,
    serial: str,
    path: str,
    *,
    run_as_package: str | None = None,
) -> dict[str, Any]:
    prefix = ["shell"]
    if run_as_package is not None:
        prefix.extend(["run-as", run_as_package])
    sha256 = parse_sha256sum(
        run_command(
            adb_args(adb, serial, *prefix, "sha256sum", path), capture=True
        ).stdout,
        path,
    )
    size_output = run_command(
        adb_args(adb, serial, *prefix, "stat", "-c", "%s", path), capture=True
    ).stdout.strip()
    try:
        size = int(size_output)
    except ValueError as error:
        raise RuntimeError(
            f"cannot parse {path} byte count: {size_output!r}"
        ) from error
    return {"bytes": size, "sha256": sha256}


def require_matching_identity(
    expected: dict[str, Any], actual: dict[str, Any], description: str
) -> None:
    if actual.get("sha256") != expected.get("sha256"):
        raise RuntimeError(
            f"{description} SHA-256 mismatch: device={actual.get('sha256')!r} "
            f"local={expected.get('sha256')!r}; refusing to reuse it"
        )
    if actual.get("bytes") != expected.get("bytes"):
        raise RuntimeError(
            f"{description} byte-count mismatch: device={actual.get('bytes')!r} "
            f"local={expected.get('bytes')!r}; refusing to reuse it"
        )


def installed_base_apk_path(
    adb: pathlib.Path | str, serial: str
) -> str:
    output = run_command(
        adb_args(adb, serial, "shell", "pm", "path", PACKAGE), capture=True
    ).stdout
    paths = [
        line.removeprefix("package:").strip()
        for line in output.splitlines()
        if line.startswith("package:")
    ]
    base_paths = [
        path for path in paths if pathlib.PurePosixPath(path).name == "base.apk"
    ]
    if len(base_paths) != 1:
        raise RuntimeError(
            f"expected exactly one installed base.apk for {PACKAGE}, "
            f"found {base_paths!r}"
        )
    return base_paths[0]


def verify_installed_apk(
    adb: pathlib.Path | str, serial: str, apk: pathlib.Path
) -> dict[str, Any]:
    expected = local_file_identity(apk)
    device_path = installed_base_apk_path(adb, serial)
    actual = read_device_file_identity(adb, serial, device_path)
    require_matching_identity(expected, actual, "installed base.apk")
    try:
        run_command(
            adb_args(adb, serial, "shell", "run-as", PACKAGE, "pwd"),
            capture=True,
        )
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"installed {PACKAGE} does not permit run-as; a matching debuggable "
            "sample APK is required"
        ) from error
    return {"device_path": device_path, **actual, "run_as_verified": True}


def inject_device_dataset(
    adb: pathlib.Path | str,
    serial: str,
    temporary_path: str,
    expected_identity: dict[str, Any],
) -> dict[str, Any]:
    run_command(
        adb_args(
            adb, serial, "shell", "run-as", PACKAGE, "mkdir", "-p", "files"
        )
    )
    run_command(
        adb_args(
            adb,
            serial,
            "shell",
            "run-as",
            PACKAGE,
            "cp",
            temporary_path,
            INTERNAL_DATASET,
        )
    )
    actual = read_device_file_identity(
        adb,
        serial,
        INTERNAL_DATASET,
        run_as_package=PACKAGE,
    )
    require_matching_identity(expected_identity, actual, "injected imported_scene.ply")
    return actual


def inject_device_trace(
    adb: pathlib.Path | str,
    serial: str,
    temporary_path: str,
    expected_identity: dict[str, Any],
) -> dict[str, Any]:
    run_command(
        adb_args(adb, serial, "shell", "run-as", PACKAGE, "cp", temporary_path, INTERNAL_TRACE)
    )
    actual = read_device_file_identity(
        adb, serial, INTERNAL_TRACE, run_as_package=PACKAGE
    )
    require_matching_identity(expected_identity, actual, "injected camera_trace.json")
    return actual


def cleanup_device_dataset(
    adb: pathlib.Path | str, serial: str, temporary_path: str
) -> None:
    if re.fullmatch(
        rf"{re.escape(DEVICE_DATASET_PREFIX)}[0-9a-f]{{64}}\.ply", temporary_path
    ) is None:
        raise ValueError(
            f"refusing to clean unexpected device dataset path: {temporary_path!r}"
        )
    run_command(
        adb_args(adb, serial, "shell", "rm", "-f", temporary_path), capture=True
    )


def cleanup_device_trace(
    adb: pathlib.Path | str, serial: str, temporary_path: str
) -> None:
    if re.fullmatch(
        rf"{re.escape(DEVICE_TRACE_PREFIX)}[0-9a-f]{{64}}\.json", temporary_path
    ) is None:
        raise ValueError(
            f"refusing to clean unexpected device trace path: {temporary_path!r}"
        )
    run_command(
        adb_args(adb, serial, "shell", "rm", "-f", temporary_path), capture=True
    )


@contextlib.contextmanager
def staged_device_dataset(
    adb: pathlib.Path | str,
    serial: str,
    local_path: pathlib.Path,
    expected_identity: dict[str, Any],
) -> Iterator[str]:
    temporary_path = device_dataset_path(expected_identity["sha256"])
    body_error: BaseException | None = None
    try:
        run_command(adb_args(adb, serial, "push", str(local_path), temporary_path))
        actual = read_device_file_identity(adb, serial, temporary_path)
        require_matching_identity(expected_identity, actual, "staged dataset")
        yield temporary_path
    except BaseException as error:
        body_error = error
        raise
    finally:
        try:
            cleanup_device_dataset(adb, serial, temporary_path)
        except (OSError, ValueError, subprocess.CalledProcessError) as cleanup_error:
            if body_error is None:
                raise
            print(
                f"warning: failed to clean exact staged dataset "
                f"{temporary_path}: {cleanup_error}",
                file=sys.stderr,
            )


@contextlib.contextmanager
def staged_device_trace(
    adb: pathlib.Path | str,
    serial: str,
    local_path: pathlib.Path,
    expected_identity: dict[str, Any],
) -> Iterator[str]:
    temporary_path = device_trace_path(expected_identity["sha256"])
    body_error: BaseException | None = None
    try:
        run_command(adb_args(adb, serial, "push", str(local_path), temporary_path))
        actual = read_device_file_identity(adb, serial, temporary_path)
        require_matching_identity(expected_identity, actual, "staged camera trace")
        yield temporary_path
    except BaseException as error:
        body_error = error
        raise
    finally:
        try:
            cleanup_device_trace(adb, serial, temporary_path)
        except (OSError, ValueError, subprocess.CalledProcessError) as cleanup_error:
            if body_error is None:
                raise
            print(
                f"warning: failed to clean exact staged camera trace "
                f"{temporary_path}: {cleanup_error}",
                file=sys.stderr,
            )


def repository_identity() -> dict[str, Any]:
    commit = run_command(
        ["git", "rev-parse", "HEAD"], capture=True
    ).stdout.strip()
    dirty = bool(
        run_command(
            ["git", "status", "--porcelain", "--untracked-files=normal"],
            capture=True,
        ).stdout.strip()
    )
    return {"commit": commit, "dirty": dirty}


def fresh_output_root(path: pathlib.Path, *, dry_run: bool) -> None:
    if path.exists():
        raise ValueError(f"output already exists; refusing to overwrite: {path}")
    if not dry_run:
        path.mkdir(parents=True)


def atomic_write_json(path: pathlib.Path, payload: dict[str, Any]) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    os.replace(temporary, path)


def resolve_apk(explicit: pathlib.Path | None = None) -> pathlib.Path:
    if explicit is not None:
        apk = explicit.expanduser().resolve()
        if not apk.is_file():
            raise FileNotFoundError(f"local sample APK is missing: {apk}")
        return apk
    metadata = json.loads(APK_METADATA.read_text(encoding="utf-8"))
    elements = metadata.get("elements", [])
    output_file = elements[0].get("outputFile") if elements else None
    apk = APK_DIR / (output_file or "sample-app-debug.apk")
    if not apk.is_file():
        raise FileNotFoundError(f"built sample APK is missing: {apk}")
    return apk


def benchmark_launch_args(args: argparse.Namespace, backend: str) -> list[str]:
    result = [
        "shell",
        "am",
        "start",
        "-W",
        "-n",
        ACTIVITY,
        "--ez",
        "gsplat_benchmark",
        "true",
        "--ei",
        "gsplat_benchmark_frames",
        str(args.frames),
        "--ei",
        "gsplat_benchmark_warmup_frames",
        str(args.warmup),
        "--ef",
        "gsplat_benchmark_yaw_step",
        str(args.yaw),
        "--ei",
        "gsplat_surface_sort_interval",
        str(args.sort_interval),
        "--ez",
        "gsplat_surface_async_sort",
        str(args.async_sort).lower(),
        "--ei",
        "gsplat_surface_frame_latency",
        str(args.frame_latency),
        "--es",
        "gsplat_geometry_path",
        args.geometry_path,
        "--es",
        "gsplat_surface_order_backend",
        backend,
    ]
    gpu_producer = getattr(args, "gpu_producer", None)
    if gpu_producer is not None:
        result.extend(
            [
                "--es",
                "gsplat_surface_gpu_producer",
                gpu_producer,
                "--ez",
                "gsplat_surface_gpu_producer_measurement",
                "true",
            ]
        )
    if args.camera_trace is not None:
        result.extend(
            [
                "--es",
                "gsplat_camera_trace_path",
                f"/data/user/0/{PACKAGE}/{INTERNAL_TRACE}",
            ]
        )
        if args.camera_frame is None:
            result.extend(
                [
                    "--ez",
                    "gsplat_camera_trace_sequence",
                    "true",
                    "--es",
                    "gsplat_camera_frame_indices",
                    args.camera_frame_indices,
                ]
            )
        else:
            result.extend(
                [
                    "--ei",
                    "gsplat_camera_trace_frame",
                    str(args.camera_frame),
                ]
            )
        result.extend(
            [
                "--ez",
                "gsplat_require_trace_display_match",
                "true",
            ]
        )
    return result


def stop_logcat(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def has_complete_summary_artifact(log: str) -> bool:
    """Return true only after a whole direct or chunked summary is visible.

    The renderer can emit a large terminal-ticket ledger as several logcat
    records. Seeing chunk zero is not completion: stopping logcat at that point
    races the remaining chunks and creates an invalid artifact on fast scenes.
    """
    if "GSPLAT_BENCHMARK_SUMMARY " in log:
        return True

    chunks: dict[tuple[str, int, str], set[int]] = {}
    pattern = re.compile(
        r"GSPLAT_BENCHMARK_CHUNK record=summary "
        r"run_id=(\S+) index=(\d+) total=(\d+) "
        r"encoding=base64 sha256=([0-9a-f]{64}) payload=\S+"
    )
    for match in pattern.finditer(log):
        run_id, index_text, total_text, digest = match.groups()
        index = int(index_text)
        total = int(total_text)
        if total <= 0 or index < 0 or index >= total:
            continue
        key = (run_id, total, digest)
        indices = chunks.setdefault(key, set())
        indices.add(index)
        if len(indices) == total:
            return True
    return False


def collect_logcat_run(
    adb: pathlib.Path | str,
    serial: str,
    launch_args: list[str],
    log_path: pathlib.Path,
    timeout_seconds: float,
) -> str:
    # A clean buffer ensures an old summary cannot satisfy this run's polling.
    run_command(adb_args(adb, serial, "logcat", "-c"))
    run_command(adb_args(adb, serial, "shell", "am", "force-stop", PACKAGE))

    log_command = adb_args(
        adb, serial, "logcat", "-v", "threadtime", "-s", LOG_TAG, "*:S"
    )
    print(f"+ {command_text(log_command)} > {log_path}", flush=True)
    with log_path.open("w", encoding="utf-8") as log_file:
        process = subprocess.Popen(
            log_command,
            cwd=REPO_ROOT,
            text=True,
            stdout=log_file,
            stderr=subprocess.STDOUT,
        )
        try:
            run_command(adb_args(adb, serial, *launch_args))
            deadline = time.monotonic() + timeout_seconds
            while time.monotonic() < deadline:
                log_file.flush()
                contents = log_path.read_text(encoding="utf-8", errors="replace")
                if (
                    "BENCHMARK_RESULT " in contents
                    and has_complete_summary_artifact(contents)
                ):
                    return contents
                if process.poll() is not None:
                    raise RuntimeError(
                        f"logcat exited before benchmark completion; see {log_path}"
                    )
                time.sleep(0.25)
        finally:
            stop_logcat(process)

    contents = log_path.read_text(encoding="utf-8", errors="replace")
    raise TimeoutError(
        f"benchmark did not emit a complete summary within {timeout_seconds:.1f}s; "
        f"see {log_path}"
    )


def extract_result_line(log: str) -> str:
    lines = [
        line[line.index("BENCHMARK_RESULT ") :]
        for line in log.splitlines()
        if "BENCHMARK_RESULT " in line
    ]
    if len(lines) != 1:
        raise RuntimeError(
            f"expected exactly one BENCHMARK_RESULT line, found {len(lines)}"
        )
    return lines[0]


def read_artifact_frames(path: pathlib.Path) -> list[dict[str, Any]]:
    frames: list[dict[str, Any]] = []
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not line:
            continue
        try:
            frame = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(
                f"artifact frame line {line_number} is invalid JSON: {error}"
            ) from error
        if not isinstance(frame, dict):
            raise RuntimeError(f"artifact frame line {line_number} is not an object")
        frames.append(frame)
    if not frames:
        raise RuntimeError("artifact contains no frame records")
    return frames


def _require_finite_number(value: Any, field: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise RuntimeError(f"{field} must be a finite number")
    result = float(value)
    if not math.isfinite(result):
        raise RuntimeError(f"{field} must be a finite number")
    return result


def _require_number_array(value: Any, size: int, field: str) -> list[float]:
    if not isinstance(value, list) or len(value) != size:
        raise RuntimeError(f"{field} must contain exactly {size} numbers")
    return [
        _require_finite_number(item, f"{field}[{index}]")
        for index, item in enumerate(value)
    ]


def _require_close(actual: float, expected: float, field: str) -> None:
    if not math.isclose(
        actual,
        expected,
        rel_tol=CAMERA_RECEIPT_TOLERANCE,
        abs_tol=CAMERA_RECEIPT_TOLERANCE,
    ):
        raise RuntimeError(
            f"{field} mismatch: expected {expected!r}, got {actual!r}"
        )


def _require_close_array(
    actual: Any, expected: Any, size: int, field: str
) -> list[float]:
    actual_values = _require_number_array(actual, size, field)
    expected_values = _require_number_array(expected, size, f"expected {field}")
    for index, (actual_value, expected_value) in enumerate(
        zip(actual_values, expected_values, strict=True)
    ):
        _require_close(actual_value, expected_value, f"{field}[{index}]")
    return actual_values


def _f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _canonical_matrices_from_runtime_receipt(
    position: list[float],
    rotation_xyzw: list[float],
    intrinsics: dict[str, Any],
    aspect: float,
) -> tuple[list[float], list[float], list[float]]:
    # Recompute from the independently emitted runtime pose/intrinsics. This
    # catches a matrix payload that was copied or mutated independently of the
    # state the native receipt says it adopted. Rounding is intentionally f32.
    quaternion = [_f32(value) for value in rotation_xyzw]
    norm = math.sqrt(sum(value * value for value in quaternion))
    if not math.isfinite(norm) or norm <= 0.0:
        raise RuntimeError("camera_receipt.pose.rotation_xyzw has invalid norm")
    x, y, z, w = [_f32(value / norm) for value in quaternion]
    x, y, z = -x, -y, -z
    inverse_norm = math.sqrt(x * x + y * y + z * z + w * w)
    x, y, z, w = [_f32(value / inverse_norm) for value in (x, y, z, w)]
    rotation = [
        _f32(1.0 - 2.0 * (y * y + z * z)),
        _f32(2.0 * (x * y - w * z)),
        _f32(2.0 * (x * z + w * y)),
        _f32(2.0 * (x * y + w * z)),
        _f32(1.0 - 2.0 * (x * x + z * z)),
        _f32(2.0 * (y * z - w * x)),
        _f32(2.0 * (x * z - w * y)),
        _f32(2.0 * (y * z + w * x)),
        _f32(1.0 - 2.0 * (x * x + y * y)),
    ]
    position_f32 = [_f32(value) for value in position]
    translation = [
        _f32(
            -sum(
                rotation[row * 3 + column] * position_f32[column]
                for column in range(3)
            )
        )
        for row in range(3)
    ]
    view = [
        rotation[0], rotation[1], rotation[2], translation[0],
        rotation[3], rotation[4], rotation[5], translation[1],
        rotation[6], rotation[7], rotation[8], translation[2],
        0.0, 0.0, 0.0, 1.0,
    ]
    vertical_fov = _f32(
        _require_finite_number(
            intrinsics.get("vertical_fov_radians"),
            "camera_receipt.intrinsics.vertical_fov_radians",
        )
    )
    near = _f32(
        _require_finite_number(
            intrinsics.get("near_plane"), "camera_receipt.intrinsics.near_plane"
        )
    )
    far = _f32(
        _require_finite_number(
            intrinsics.get("far_plane"), "camera_receipt.intrinsics.far_plane"
        )
    )
    focal = _f32(1.0 / math.tan(_f32(vertical_fov * 0.5)))
    depth = _f32(far / _f32(far - near))
    projection = [
        _f32(focal / _f32(aspect)), 0.0, 0.0, 0.0,
        0.0, focal, 0.0, 0.0,
        0.0, 0.0, depth, _f32(-near * depth),
        0.0, 0.0, 1.0, 0.0,
    ]
    view_projection = [
        _f32(
            sum(
                projection[(index // 4) * 4 + k] * view[k * 4 + index % 4]
                for k in range(4)
            )
        )
        for index in range(16)
    ]
    return view, projection, view_projection


def validate_camera_receipts(
    manifest: dict[str, Any],
    frames: Sequence[dict[str, Any]],
    expected_trace: dict[str, Any],
    expected_trace_identity: dict[str, Any],
) -> None:
    trace = manifest.get("trace")
    if not isinstance(trace, dict):
        raise RuntimeError("artifact trace metadata is missing")
    if trace.get("schema") != "gsplat-camera-trace/v1":
        raise RuntimeError("artifact trace schema is not gsplat-camera-trace/v1")
    if trace.get("id") != expected_trace.get("trace_id"):
        raise RuntimeError("artifact trace id does not match the expected trace")
    if trace.get("sha256") != expected_trace.get("content_sha256"):
        raise RuntimeError("artifact trace content hash does not match the expected trace")
    if trace.get("file_sha256") != expected_trace_identity.get("sha256"):
        raise RuntimeError("artifact trace file hash does not match the injected trace")
    if trace.get("coordinate_system") != CANONICAL_COORDINATE_SYSTEM:
        raise RuntimeError("artifact coordinate-system convention is not canonical")
    if expected_trace.get("coordinate_system") != CANONICAL_COORDINATE_SYSTEM:
        raise RuntimeError("expected trace coordinate-system convention is not canonical")
    if trace.get("matrix_convention") != CANONICAL_MATRIX_CONVENTION:
        raise RuntimeError("artifact matrix convention is not canonical")
    if expected_trace.get("matrix_convention") != CANONICAL_MATRIX_CONVENTION:
        raise RuntimeError("expected trace matrix convention is not canonical")
    contract = trace.get("runtime_camera_receipt")
    expected_contract = {
        "schema": CAMERA_RECEIPT_SCHEMA,
        "source": "native_runtime_after_present",
        "scalar_storage": "float32",
        "absolute_tolerance": CAMERA_RECEIPT_TOLERANCE,
        "relative_tolerance": CAMERA_RECEIPT_TOLERANCE,
    }
    if contract != expected_contract:
        raise RuntimeError("artifact runtime camera-receipt contract is missing or changed")

    expected_frames = expected_trace.get("frames")
    if not isinstance(expected_frames, list) or not expected_frames:
        raise RuntimeError("expected trace has no frames")
    playback_mode = trace.get("playback_mode")
    measured_per_loop = trace.get("measured_frames_per_loop")
    loops = trace.get("loops")
    if (
        isinstance(measured_per_loop, bool)
        or not isinstance(measured_per_loop, int)
        or measured_per_loop <= 0
        or isinstance(loops, bool)
        or not isinstance(loops, int)
        or loops <= 0
    ):
        raise RuntimeError("artifact trace playback dimensions are invalid")
    if len(frames) != measured_per_loop * loops:
        raise RuntimeError("artifact frame count does not match trace playback dimensions")
    if playback_mode == "sequence":
        selected = trace.get("frame_indices")
        if (
            not isinstance(selected, list)
            or len(selected) < 2
            or any(isinstance(value, bool) or not isinstance(value, int) for value in selected)
        ):
            raise RuntimeError("artifact trace sequence frame_indices are invalid")
    elif playback_mode == "fixed":
        fixed_index = trace.get("frame_index")
        if isinstance(fixed_index, bool) or not isinstance(fixed_index, int):
            raise RuntimeError("artifact fixed trace frame_index is invalid")
        selected = [fixed_index]
        if loops != 1:
            raise RuntimeError("artifact fixed trace playback must have one loop")
    else:
        raise RuntimeError(f"artifact trace playback mode {playback_mode!r} is unsupported")

    expected_display = expected_trace.get("display", {})
    expected_width = expected_display.get("width")
    expected_height = expected_display.get("height")
    if (
        trace.get("reference_width") != expected_width
        or trace.get("reference_height") != expected_height
    ):
        raise RuntimeError("artifact trace reference display does not match expected trace")

    for sample_index, frame in enumerate(frames):
        expected_index = selected[(sample_index % measured_per_loop) % len(selected)]
        if expected_index < 0 or expected_index >= len(expected_frames):
            raise RuntimeError(f"artifact trace frame index {expected_index} is out of range")
        if frame.get("trace_frame_index") != expected_index:
            raise RuntimeError(
                f"artifact frame {sample_index} trace_frame_index does not match playback"
            )
        expected_frame = expected_frames[expected_index]
        if frame.get("trace_timestamp_ns") != expected_frame.get("timestamp_ns"):
            raise RuntimeError(
                f"artifact frame {sample_index} trace timestamp does not match expected trace"
            )
        expected_loop = sample_index // measured_per_loop if playback_mode == "sequence" else 0
        if frame.get("trace_loop_index") != expected_loop:
            raise RuntimeError(f"artifact frame {sample_index} trace loop index is wrong")

        receipt = frame.get("camera_receipt")
        if not isinstance(receipt, dict):
            raise RuntimeError(f"artifact frame {sample_index} camera_receipt is missing")
        if receipt.get("schema") != CAMERA_RECEIPT_SCHEMA:
            raise RuntimeError(f"artifact frame {sample_index} camera receipt schema is wrong")
        if receipt.get("source") != "native_runtime_after_present":
            raise RuntimeError(f"artifact frame {sample_index} camera receipt source is wrong")
        camera_revision = frame.get("camera_revision")
        if (
            isinstance(camera_revision, bool)
            or not isinstance(camera_revision, int)
            or camera_revision < 0
            or receipt.get("camera_revision") != camera_revision
            or receipt.get("presented_camera_revision") != camera_revision
        ):
            raise RuntimeError(
                f"artifact frame {sample_index} camera receipt revision is not presented-frame exact"
            )
        if receipt.get("flags") != 3:
            raise RuntimeError(
                f"artifact frame {sample_index} camera receipt is not current-and-presented"
            )
        if (
            receipt.get("surface_width") != expected_width
            or receipt.get("surface_height") != expected_height
        ):
            raise RuntimeError(
                f"artifact frame {sample_index} camera receipt Surface size differs from trace"
            )

        pose = receipt.get("pose")
        intrinsics = receipt.get("intrinsics")
        if not isinstance(pose, dict) or not isinstance(intrinsics, dict):
            raise RuntimeError(f"artifact frame {sample_index} camera state is malformed")
        expected_pose = expected_frame.get("pose")
        expected_intrinsics = expected_frame.get("intrinsics")
        if not isinstance(expected_pose, dict) or not isinstance(expected_intrinsics, dict):
            raise RuntimeError(f"expected trace frame {expected_index} camera state is malformed")
        position = _require_close_array(
            pose.get("position"),
            expected_pose.get("position"),
            3,
            f"frames[{sample_index}].camera_receipt.pose.position",
        )
        rotation = _require_close_array(
            pose.get("rotation_xyzw"),
            expected_pose.get("rotation_xyzw"),
            4,
            f"frames[{sample_index}].camera_receipt.pose.rotation_xyzw",
        )
        for field in ("vertical_fov_radians", "near_plane", "far_plane"):
            _require_close(
                _require_finite_number(
                    intrinsics.get(field),
                    f"frames[{sample_index}].camera_receipt.intrinsics.{field}",
                ),
                _require_finite_number(
                    expected_intrinsics.get(field),
                    f"expected frames[{expected_index}].intrinsics.{field}",
                ),
                f"frames[{sample_index}].camera_receipt.intrinsics.{field}",
            )

        runtime_matrices = _canonical_matrices_from_runtime_receipt(
            position,
            rotation,
            intrinsics,
            float(expected_width) / float(expected_height),
        )
        for field, runtime_matrix in zip(
            ("view_matrix", "projection_matrix", "view_projection_matrix"),
            runtime_matrices,
            strict=True,
        ):
            actual_matrix = _require_close_array(
                receipt.get(field),
                expected_frame.get(field),
                16,
                f"frames[{sample_index}].camera_receipt.{field}",
            )
            for element_index, (actual, recomputed) in enumerate(
                zip(actual_matrix, runtime_matrix, strict=True)
            ):
                _require_close(
                    actual,
                    recomputed,
                    f"frames[{sample_index}].camera_receipt.{field}"
                    f"[{element_index}] runtime consistency",
                )


def validate_run_artifact(
    manifest: dict[str, Any],
    summary: dict[str, Any],
    frames: Sequence[dict[str, Any]],
    expected_backend: str,
    expected_geometry_path: str,
    expected_dataset: dict[str, Any],
    expected_trace: dict[str, Any],
    expected_trace_identity: dict[str, Any],
    expected_gpu_producer: str | None = None,
) -> None:
    renderer = manifest.get("renderer", {})
    requested = renderer.get("order_backend_requested")
    if requested != expected_backend:
        raise RuntimeError(
            f"artifact requested backend {requested!r}, expected {expected_backend!r}"
        )

    renderer_path = renderer.get("path")
    expected_renderer_path = RENDERER_PATHS[expected_geometry_path]
    if renderer_path != expected_renderer_path:
        raise RuntimeError(
            f"artifact renderer path {renderer_path!r}, "
            f"expected {expected_renderer_path!r}"
        )

    if manifest.get("trace", {}).get("display_policy") != "trace_display_exact":
        raise RuntimeError("artifact did not use trace_display_exact")
    if manifest.get("trace", {}).get("quality_comparable") is not True:
        raise RuntimeError("artifact is not quality-comparable")

    validate_camera_receipts(
        manifest,
        frames,
        expected_trace,
        expected_trace_identity,
    )

    dataset = manifest.get("dataset", {})
    for field in ("sha256", "bytes"):
        if dataset.get(field) != expected_dataset.get(field):
            raise RuntimeError(
                f"artifact dataset {field} {dataset.get(field)!r}, "
                f"expected {expected_dataset.get(field)!r}"
            )

    sample_count = summary.get("sample_count")
    telemetry = summary.get("sort_telemetry", {})
    cpu_frames = telemetry.get("cpu_frame_count")
    gpu_frames = telemetry.get("gpu_frame_count")
    gpu_fallbacks = telemetry.get("gpu_sort_fallback_count")
    if expected_backend == "cpu" and not (
        cpu_frames == sample_count and gpu_frames == 0 and gpu_fallbacks == 0
    ):
        raise RuntimeError(
            "forced cpu artifact contains non-cpu or fallback frames: "
            f"samples={sample_count!r} cpu={cpu_frames!r} gpu={gpu_frames!r} "
            f"gpu_fallbacks={gpu_fallbacks!r}"
        )
    if expected_backend == "gpu" and not (
        gpu_frames == sample_count and cpu_frames == 0 and gpu_fallbacks == 0
    ):
        raise RuntimeError(
            "forced gpu artifact contains cpu or fallback frames: "
            f"samples={sample_count!r} cpu={cpu_frames!r} gpu={gpu_frames!r} "
            f"gpu_fallbacks={gpu_fallbacks!r}"
        )

    producer_requested = renderer.get("gpu_order_producer_requested")
    producer_enabled = renderer.get("gpu_producer_measurement_enabled")
    producer_summary = summary.get("gpu_producer_telemetry")
    producer_ledger = summary.get("gpu_producer_terminal_ledger")
    if expected_gpu_producer is None:
        if producer_requested is not None or producer_enabled is not False:
            raise RuntimeError("non-diagnostic artifact enabled GPU producer telemetry")
        if producer_summary is not None or producer_ledger is not None:
            raise RuntimeError("non-diagnostic artifact published GPU producer evidence")
        for index, frame in enumerate(frames):
            if frame.get("gpu_order_producer") is not None:
                raise RuntimeError(
                    f"frame {index} published GPU producer evidence while disabled"
                )
            producer_fields = [
                field for field in frame if field.startswith("gpu_producer_")
            ]
            if producer_fields:
                raise RuntimeError(
                    f"frame {index} published GPU producer fields while disabled: "
                    f"{sorted(producer_fields)!r}"
                )
        return

    if expected_gpu_producer not in GPU_PRODUCERS:
        raise RuntimeError(f"unsupported expected GPU producer {expected_gpu_producer!r}")
    if expected_backend != "gpu" or expected_geometry_path != "packed":
        raise RuntimeError("GPU producer qualification requires packed + forced GPU")
    expected_renderer = {
        "raster_plan": "projected_quads_exact",
        "projected_policy_requested": "compact",
        "gpu_order_producer_requested": expected_gpu_producer,
        "gpu_producer_measurement_enabled": True,
    }
    for field, expected in expected_renderer.items():
        if renderer.get(field) != expected:
            raise RuntimeError(
                f"artifact renderer {field} {renderer.get(field)!r}, expected {expected!r}"
            )
    if not isinstance(sample_count, int) or sample_count <= 0 or len(frames) != sample_count:
        raise RuntimeError("GPU producer artifact has an invalid sample count")
    source_count = manifest.get("dataset", {}).get("splat_count")
    if not isinstance(source_count, int) or source_count <= 0:
        raise RuntimeError("GPU producer artifact lacks a positive dataset splat_count")

    frame_tickets: set[int] = set()
    for index, frame in enumerate(frames):
        ticket = frame.get("gpu_producer_measurement_ticket")
        contributor = frame.get("gpu_producer_contributor")
        drawn = frame.get("gpu_producer_drawn")
        completion_ms = frame.get("gpu_producer_frame_complete_ms")
        if frame.get("gpu_order_producer") != expected_gpu_producer:
            raise RuntimeError(f"frame {index} used the wrong GPU producer")
        if not isinstance(ticket, int) or ticket <= 0 or ticket in frame_tickets:
            raise RuntimeError(f"frame {index} has a missing or duplicate producer ticket")
        frame_tickets.add(ticket)
        if frame.get("gpu_producer_submission_flags") != 9:
            raise RuntimeError(
                f"frame {index} producer submission was unsampled or malformed"
            )
        if frame.get("gpu_producer_measurement_camera_revision") != frame.get(
            "camera_revision"
        ):
            raise RuntimeError(f"frame {index} producer camera revision is stale")
        if frame.get("gpu_producer_source") != source_count:
            raise RuntimeError(f"frame {index} producer source count is incomplete")
        if not isinstance(contributor, int) or not 0 <= contributor <= source_count:
            raise RuntimeError(f"frame {index} producer contributor count is invalid")
        if drawn != contributor:
            raise RuntimeError(f"frame {index} Compact producer receipt violates D=C")
        if frame.get("gpu_producer_draw_scope") != "exact_current_contributors":
            raise RuntimeError(f"frame {index} producer draw scope is not exact-current")
        if frame.get("gpu_producer_order_refreshed") is not True:
            raise RuntimeError(f"frame {index} producer did not refresh order")
        if frame.get("gpu_producer_exact_current_draw") is not True:
            raise RuntimeError(f"frame {index} producer draw is not exact-current")
        if frame.get("gpu_producer_stale_order") is not False:
            raise RuntimeError(f"frame {index} producer retained stale order")
        if frame.get("gpu_producer_dropped_prior") is not False:
            raise RuntimeError(f"frame {index} producer evidence dropped a prior receipt")
        for field in ("gpu_producer_order_generation", "gpu_producer_projection_generation"):
            if not isinstance(frame.get(field), int) or frame[field] <= 0:
                raise RuntimeError(f"frame {index} producer {field} is invalid")
        if not isinstance(completion_ms, (int, float)) or isinstance(completion_ms, bool):
            raise RuntimeError(f"frame {index} producer completion timing is missing")
        if not math.isfinite(float(completion_ms)) or float(completion_ms) < 0.0:
            raise RuntimeError(f"frame {index} producer completion timing is invalid")

    if not isinstance(producer_summary, dict):
        raise RuntimeError("GPU producer summary is missing")
    required_summary = {
        "requested_producer": expected_gpu_producer,
        "scheduled_count": sample_count,
        "completed_count": sample_count,
        "failure_count": 0,
        "unsampled_count": 0,
        "exact_current_count": sample_count,
        "stale_count": 0,
        "dropped_count": 0,
        "order_refreshed_count": sample_count,
    }
    for field, expected in required_summary.items():
        if producer_summary.get(field) != expected:
            raise RuntimeError(
                f"GPU producer summary {field} {producer_summary.get(field)!r}, "
                f"expected {expected!r}"
            )
    if producer_summary.get("frame_complete_ms", {}).get("count") != sample_count:
        raise RuntimeError("GPU producer timing distribution is incomplete")
    if not isinstance(producer_ledger, list):
        raise RuntimeError("GPU producer terminal ledger is missing")
    ledger_tickets: set[int] = set()
    for entry in producer_ledger:
        ticket = entry.get("ticket") if isinstance(entry, dict) else None
        if not isinstance(ticket, int) or ticket <= 0 or ticket in ledger_tickets:
            raise RuntimeError("GPU producer terminal ledger contains duplicate tickets")
        ledger_tickets.add(ticket)
        if entry.get("outcome") != "success":
            raise RuntimeError("GPU producer terminal ledger contains a failure")
        if entry.get("producer") != expected_gpu_producer:
            raise RuntimeError("GPU producer terminal ledger contains the wrong producer")
        if entry.get("source") != source_count or entry.get("drawn") != entry.get(
            "contributor"
        ):
            raise RuntimeError("GPU producer terminal ledger violates exact S/C/D")
    if not frame_tickets.issubset(ledger_tickets):
        raise RuntimeError("GPU producer terminal ledger is missing measured tickets")


def device_info(adb: pathlib.Path | str, serial: str) -> dict[str, str]:
    state = run_command(adb_args(adb, serial, "get-state"), capture=True).stdout.strip()
    if state != "device":
        raise RuntimeError(f"adb target {serial} is not ready (state={state!r})")
    properties = {}
    for key in (
        "ro.product.manufacturer",
        "ro.product.model",
        "ro.product.device",
        "ro.build.version.release",
        "ro.build.version.sdk",
        "ro.hardware",
        "ro.build.fingerprint",
        "ro.soc.manufacturer",
        "ro.soc.model",
        "ro.board.platform",
        "ro.hardware.vulkan",
        "ro.gfx.driver.0",
    ):
        properties[key] = run_command(
            adb_args(adb, serial, "shell", "getprop", key), capture=True
        ).stdout.strip()
    return {"serial": serial, **properties}


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        description=(
            "Validate one installed Android sample APK, inject one PLY with "
            "run-as, and collect paired CPU/GPU/adaptive benchmark artifacts."
        )
    )
    result.add_argument("--serial", required=True, help="exact adb device serial")
    result.add_argument("--ply", required=True, type=pathlib.Path, help="PLY to inject")
    result.add_argument(
        "--apk",
        type=pathlib.Path,
        help=(
            "local debuggable sample APK to compare with the installed base.apk "
            "(default: resolve the latest sample-app debug output)"
        ),
    )
    result.add_argument(
        "--prepare-apk",
        action="store_true",
        help=(
            "build and install the sample APK once before exact hash validation; "
            "omit for subsequent dataset experiments"
        ),
    )
    result.add_argument(
        "--backend",
        action="append",
        choices=BACKENDS,
        help="backend to include; repeat for an A/B set (default: cpu, gpu)",
    )
    result.add_argument(
        "--gpu-producer",
        choices=GPU_PRODUCERS,
        help=(
            "strict Packed+GPU+Compact diagnostic producer; enables exact per-frame "
            "S/C/D completion receipts and leaves product defaults unchanged"
        ),
    )
    result.add_argument("--repetitions", type=int, default=1, help="runs per backend")
    result.add_argument(
        "--randomize-order",
        action="store_true",
        help="shuffle backend order inside each repetition",
    )
    result.add_argument("--seed", type=int, default=0, help="randomization seed")
    result.add_argument("--sort-interval", type=int, default=1)
    # Keep the default artifact burst below conservative Android logd per-tag
    # quotas. Larger runs remain available explicitly and still fail closed if
    # logd drops any indexed frame record.
    result.add_argument("--frames", type=int, default=80)
    result.add_argument("--warmup", type=int, default=20)
    result.add_argument("--yaw", type=float, default=0.001)
    result.add_argument("--frame-latency", type=int, default=2)
    result.add_argument(
        "--camera-trace",
        type=pathlib.Path,
        help=(
            "validated exact-display gsplat-camera-trace/v1; when present the "
            "collector runs strict sequence mode"
        ),
    )
    camera_playback = result.add_mutually_exclusive_group()
    camera_playback.add_argument(
        "--camera-frame",
        type=int,
        help="one static trace frame to retain for every measured sample",
    )
    camera_playback.add_argument(
        "--camera-frame-indices",
        help="comma-separated trace frames used in strict sequence mode (default: 0,1)",
    )
    result.add_argument(
        "--geometry-path",
        choices=GEOMETRY_PATHS,
        default="packed",
        help=(
            "exact geometry path (default: packed, whose production Surface "
            "raster plan is ProjectedQuadsExact; direct is the wide-f32 oracle)"
        ),
    )
    result.add_argument(
        "--async-sort",
        action="store_true",
        help="enable the existing CPU async-latest path (normally leave disabled for backend A/B)",
    )
    result.add_argument(
        "--cooldown-seconds",
        type=float,
        default=0.0,
        help="fixed delay between runs",
    )
    result.add_argument(
        "--max-thermal-status",
        type=int,
        help="wait for Android thermal status at or below this value before each run",
    )
    result.add_argument("--thermal-timeout-seconds", type=float, default=300.0)
    result.add_argument("--thermal-poll-seconds", type=float, default=5.0)
    result.add_argument("--run-timeout-seconds", type=float, default=180.0)
    result.add_argument("--adb", type=pathlib.Path, help="adb executable")
    result.add_argument("--output", type=pathlib.Path, help="fresh experiment directory")
    result.add_argument(
        "--rust-profile",
        choices=("release", "dev"),
        default="release",
        help="native Rust profile used only with --prepare-apk",
    )
    result.add_argument(
        "--dry-run", action="store_true", help="print the schedule and commands only"
    )
    return result


def validate_args(args: argparse.Namespace) -> list[str]:
    args.serial = args.serial.strip()
    if not args.serial or any(character.isspace() for character in args.serial):
        raise ValueError("--serial must be one non-empty adb serial")
    args.ply = args.ply.expanduser().resolve()
    if not args.ply.is_file():
        raise ValueError(f"PLY does not exist: {args.ply}")
    if args.prepare_apk and args.apk is not None:
        raise ValueError("--apk cannot be combined with --prepare-apk")
    if args.apk is not None:
        args.apk = args.apk.expanduser().resolve()
        if not args.dry_run and not args.apk.is_file():
            raise ValueError(f"APK does not exist: {args.apk}")
    if args.camera_trace is None:
        raise ValueError(
            "--camera-trace is required so retained runs are trace_display_exact"
        )
    args.camera_trace = args.camera_trace.expanduser().resolve()
    if not args.camera_trace.is_file():
        raise ValueError(f"camera trace does not exist: {args.camera_trace}")
    try:
        run_command([sys.executable, TRACE_VALIDATOR, args.camera_trace], capture=True)
    except subprocess.CalledProcessError as error:
        raise ValueError(f"camera trace validation failed: {error.stdout}") from error
    trace_frame_count = len(
        json.loads(args.camera_trace.read_text(encoding="utf-8"))["frames"]
    )
    if args.camera_frame is not None:
        if args.camera_frame < 0:
            raise ValueError("--camera-frame must be non-negative")
        if args.camera_frame >= trace_frame_count:
            raise ValueError(
                f"--camera-frame {args.camera_frame} is out of range for "
                f"{trace_frame_count} trace frames"
            )
    else:
        args.camera_frame_indices = args.camera_frame_indices or "0,1"
        if re.fullmatch(r"[0-9]+(?:,[0-9]+)+", args.camera_frame_indices) is None:
            raise ValueError(
                "--camera-frame-indices requires at least two comma-separated integers"
            )
        selected_frames = [int(value) for value in args.camera_frame_indices.split(",")]
        if len(set(selected_frames)) != len(selected_frames):
            raise ValueError("--camera-frame-indices cannot contain duplicates")
        if any(value >= trace_frame_count for value in selected_frames):
            raise ValueError(
                "--camera-frame-indices contains a frame outside the expected trace"
            )
    backends = args.backend or ["cpu", "gpu"]
    if len(set(backends)) != len(backends):
        raise ValueError("--backend values must be unique")
    if args.async_sort and any(backend != "cpu" for backend in backends):
        raise ValueError("--async-sort is only compatible with the cpu backend")
    if args.gpu_producer is not None:
        if backends != ["gpu"]:
            raise ValueError("--gpu-producer requires exactly one --backend gpu")
        if args.geometry_path != "packed":
            raise ValueError("--gpu-producer requires --geometry-path packed")
        if args.async_sort or args.sort_interval != 1:
            raise ValueError("--gpu-producer requires synchronous sort interval 1")
        if args.camera_frame is not None:
            raise ValueError("--gpu-producer requires camera trace sequence playback")
    if args.repetitions < 1:
        raise ValueError("--repetitions must be positive")
    if args.sort_interval < 1:
        raise ValueError("--sort-interval must be positive")
    if args.frames < 1:
        raise ValueError("--frames must be positive")
    if args.warmup < 0:
        raise ValueError("--warmup cannot be negative")
    if not math.isfinite(args.yaw) or not (-1.0 <= args.yaw <= 1.0):
        raise ValueError("--yaw must be between -1 and 1 radians per frame")
    if not 1 <= args.frame_latency <= 4:
        raise ValueError("--frame-latency must be between 1 and 4")
    if not math.isfinite(args.cooldown_seconds) or args.cooldown_seconds < 0:
        raise ValueError("--cooldown-seconds cannot be negative")
    if args.max_thermal_status is not None and not 0 <= args.max_thermal_status <= 6:
        raise ValueError("--max-thermal-status must be between 0 and 6")
    for label in (
        "thermal_timeout_seconds",
        "thermal_poll_seconds",
        "run_timeout_seconds",
    ):
        value = getattr(args, label)
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f"--{label.replace('_', '-')} must be positive")
    return backends


def default_output(ply: pathlib.Path) -> pathlib.Path:
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    safe_stem = re.sub(r"[^A-Za-z0-9_.-]+", "-", ply.stem).strip("-") or "scene"
    return REPO_ROOT / "target/android-sort-benchmarks" / f"{stamp}-{safe_stem}"


def dry_run(
    args: argparse.Namespace,
    adb: pathlib.Path | str,
    schedule: list[RunSpec],
    output: pathlib.Path,
) -> None:
    dataset_identity = local_file_identity(args.ply)
    temporary_path = device_dataset_path(dataset_identity["sha256"])
    trace_identity = local_file_identity(args.camera_trace)
    temporary_trace_path = device_trace_path(trace_identity["sha256"])
    local_apk = args.apk or "<resolved from output-metadata.json>"
    print(f"output_root={output}")
    print(f"package_to_clear={PACKAGE}")
    print(
        f"apk_mode={'prepare-once' if args.prepare_apk else 'reuse-exact-installed'}"
    )
    if args.prepare_apk:
        print(f"+ {command_text(['bash', BUILD_SCRIPT, APK_BOOTSTRAP_DATASET])}")
        print("apk=<resolved from output-metadata.json>")
        print(f"+ {command_text(adb_args(adb, args.serial, 'install', '-r', '<apk>'))}")
    else:
        print(f"apk={local_apk}")
    print(f"+ {command_text(adb_args(adb, args.serial, 'shell', 'pm', 'path', PACKAGE))}")
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'sha256sum', '<device-base.apk>'))}"
    )
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'stat', '-c', '%s', '<device-base.apk>'))}"
    )
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'run-as', PACKAGE, 'pwd'))}"
    )
    print(f"+ {command_text(adb_args(adb, args.serial, 'push', args.ply, temporary_path))}")
    print(f"+ {command_text(adb_args(adb, args.serial, 'push', args.camera_trace, temporary_trace_path))}")
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'sha256sum', temporary_path))}"
    )
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'stat', '-c', '%s', temporary_path))}"
    )
    for spec in schedule:
        print(
            f"run={spec.index} repetition={spec.repetition} position={spec.position} "
            f"backend={spec.backend}"
        )
        print(f"+ {command_text(adb_args(adb, args.serial, 'shell', 'pm', 'clear', PACKAGE))}")
        print(
            f"+ {command_text(adb_args(adb, args.serial, 'shell', 'run-as', PACKAGE, 'mkdir', '-p', 'files'))}"
        )
        print(
            f"+ {command_text(adb_args(adb, args.serial, 'shell', 'run-as', PACKAGE, 'cp', temporary_path, INTERNAL_DATASET))}"
        )
        print(
            f"+ {command_text(adb_args(adb, args.serial, 'shell', 'run-as', PACKAGE, 'cp', temporary_trace_path, INTERNAL_TRACE))}"
        )
        print(
            f"+ {command_text(adb_args(adb, args.serial, 'shell', 'run-as', PACKAGE, 'sha256sum', INTERNAL_DATASET))}"
        )
        print(
            f"+ {command_text(adb_args(adb, args.serial, 'shell', 'run-as', PACKAGE, 'stat', '-c', '%s', INTERNAL_DATASET))}"
        )
        print(
            f"+ {command_text(adb_args(adb, args.serial, *benchmark_launch_args(args, spec.backend)))}"
        )
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'rm', '-f', temporary_path))}"
    )
    print(
        f"+ {command_text(adb_args(adb, args.serial, 'shell', 'rm', '-f', temporary_trace_path))}"
    )


def collect_scheduled_runs(
    args: argparse.Namespace,
    adb: pathlib.Path | str,
    schedule: list[RunSpec],
    output: pathlib.Path,
    experiment: dict[str, Any],
    experiment_path: pathlib.Path,
    temporary_dataset_path: str,
    temporary_trace_path: str,
    expected_trace: dict[str, Any],
) -> None:
    for spec in schedule:
        if spec.index > 1 and args.cooldown_seconds > 0:
            print(f"cooldown_seconds={args.cooldown_seconds:.1f}", flush=True)
            time.sleep(args.cooldown_seconds)

        label = (
            f"run-{spec.index:03d}-pair-{spec.repetition:03d}-"
            f"pos-{spec.position:02d}-{spec.backend}"
        )
        run_dir = output / label
        run_dir.mkdir()
        log_path = run_dir / "logcat.txt"
        artifact_dir = run_dir / "artifact"
        run_record: dict[str, Any] = {
            **dataclasses.asdict(spec),
            "status": "running",
            "started_at_utc": utc_now(),
            "thermal_status_before": None,
            "configuration": {
                "frames": args.frames,
                "warmup": args.warmup,
                "yaw": args.yaw,
                "sort_interval": args.sort_interval,
                "async_sort": args.async_sort,
                "frame_latency": args.frame_latency,
                "geometry_path": args.geometry_path,
                "gpu_producer": args.gpu_producer,
            },
            "log": str(log_path.relative_to(output)),
            "artifact": str(artifact_dir.relative_to(output)),
        }
        experiment["runs"].append(run_record)
        atomic_write_json(experiment_path, experiment)

        clear_result = run_command(
            adb_args(adb, args.serial, "shell", "pm", "clear", PACKAGE),
            capture=True,
        ).stdout.strip()
        if clear_result != "Success":
            raise RuntimeError(
                f"failed to clear exact benchmark package {PACKAGE}: {clear_result}"
            )
        run_command(
            adb_args(
                adb,
                args.serial,
                "shell",
                "cmd",
                "package",
                "wait-for-handler",
                "--timeout",
                "10000",
            ),
            timeout=15.0,
        )

        injected_identity = inject_device_dataset(
            adb,
            args.serial,
            temporary_dataset_path,
            experiment["dataset"],
        )
        run_record["injected_dataset"] = {
            "internal_path": INTERNAL_DATASET,
            **injected_identity,
        }
        injected_trace_identity = inject_device_trace(
            adb,
            args.serial,
            temporary_trace_path,
            experiment["trace"],
        )
        run_record["injected_trace"] = {
            "internal_path": INTERNAL_TRACE,
            **injected_trace_identity,
        }

        # Large tiers can make the per-run verified copy itself observable in
        # thermal state, so gate immediately before launch, after injection.
        if args.max_thermal_status is None:
            thermal_before = read_thermal_status(adb, args.serial)
        else:
            thermal_before = wait_for_thermal_status(
                adb,
                args.serial,
                args.max_thermal_status,
                args.thermal_timeout_seconds,
                args.thermal_poll_seconds,
            )
        run_record["thermal_status_before"] = thermal_before
        atomic_write_json(experiment_path, experiment)

        print(
            f"run={spec.index}/{len(schedule)} repetition={spec.repetition} "
            f"position={spec.position} backend={spec.backend} "
            f"interval={args.sort_interval} frames={args.frames} "
            f"warmup={args.warmup} yaw={args.yaw} "
            f"async_sort={str(args.async_sort).lower()} "
            f"frame_latency={args.frame_latency} thermal_before={thermal_before} "
            f"gpu_producer={args.gpu_producer or 'disabled'} "
            f"run_dir={run_dir}",
            flush=True,
        )

        log = collect_logcat_run(
            adb,
            args.serial,
            benchmark_launch_args(args, spec.backend),
            log_path,
            args.run_timeout_seconds,
        )
        result_line = extract_result_line(log)
        run_command(
            [
                sys.executable,
                EXTRACTOR,
                log_path,
                artifact_dir,
                "--validator",
                VALIDATOR,
                "--camera-trace",
                args.camera_trace,
                "--camera-validator",
                CAMERA_RECEIPT_VALIDATOR,
            ]
        )
        manifest = json.loads(
            (artifact_dir / "manifest.json").read_text(encoding="utf-8")
        )
        summary = json.loads(
            (artifact_dir / "summary.json").read_text(encoding="utf-8")
        )
        frames = read_artifact_frames(artifact_dir / "frames.jsonl")
        validate_run_artifact(
            manifest,
            summary,
            frames,
            spec.backend,
            args.geometry_path,
            experiment["dataset"],
            expected_trace,
            experiment["trace"],
            args.gpu_producer,
        )

        run_record.update(
            {
                "status": "complete",
                "ended_at_utc": utc_now(),
                "thermal_status_after": read_thermal_status(adb, args.serial),
                "benchmark_result": result_line,
                "artifact_run_id": manifest.get("run_id"),
            }
        )
        atomic_write_json(run_dir / "run.json", run_record)
        atomic_write_json(experiment_path, experiment)
        print(result_line)
        print(f"log={log_path}")
        print(f"artifact={artifact_dir}")


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        backends = validate_args(args)
        adb = resolve_adb(args.adb, dry_run=args.dry_run)
        schedule = build_schedule(
            backends, args.repetitions, args.randomize_order, args.seed
        )
        output = (args.output or default_output(args.ply)).expanduser().resolve()
        fresh_output_root(output, dry_run=args.dry_run)
    except (OSError, ValueError) as error:
        parser().error(str(error))

    if args.dry_run:
        dry_run(args, adb, schedule, output)
        return 0

    dataset_identity = {"path": str(args.ply), **local_file_identity(args.ply)}
    trace_json = json.loads(args.camera_trace.read_text(encoding="utf-8"))
    trace_identity = {
        "path": str(args.camera_trace),
        **local_file_identity(args.camera_trace),
        "trace_id": trace_json["trace_id"],
        "content_sha256": trace_json["content_sha256"],
        "display": trace_json["display"],
        "playback_mode": "fixed" if args.camera_frame is not None else "sequence",
        "frame_index": args.camera_frame,
        "frame_indices": (
            None
            if args.camera_frame is not None
            else [int(value) for value in args.camera_frame_indices.split(",")]
        ),
    }
    experiment: dict[str, Any] = {
        "schema": "gsplat-android-sort-experiment/v1",
        "status": "running",
        "started_at_utc": utc_now(),
        "package_cleared_before_each_run": PACKAGE,
        "dataset": dataset_identity,
        "trace": trace_identity,
        "dataset_delivery": {
            "mode": "adb-push-once+run-as-copy-per-run",
            "internal_path": INTERNAL_DATASET,
            "temporary_path": device_dataset_path(dataset_identity["sha256"]),
        },
        "configuration": {
            "backends": backends,
            "repetitions": args.repetitions,
            "randomize_order": args.randomize_order,
            "seed": args.seed,
            "frames": args.frames,
            "warmup": args.warmup,
            "yaw": args.yaw,
            "sort_interval": args.sort_interval,
            "async_sort": args.async_sort,
            "frame_latency": args.frame_latency,
            "geometry_path": args.geometry_path,
            "gpu_producer": args.gpu_producer,
            "cooldown_seconds": args.cooldown_seconds,
            "max_thermal_status": args.max_thermal_status,
            "apk_mode": "prepare-once" if args.prepare_apk else "reuse-exact-installed",
            "rust_profile": args.rust_profile if args.prepare_apk else None,
        },
        "schedule": [dataclasses.asdict(spec) for spec in schedule],
        "runs": [],
    }
    experiment_path = output / "experiment.json"

    try:
        experiment["repository"] = repository_identity()
        experiment["device"] = device_info(adb, args.serial)
        atomic_write_json(experiment_path, experiment)

        if args.prepare_apk:
            build_env = os.environ.copy()
            build_env["ANDROID_RUST_PROFILE"] = args.rust_profile
            run_command(
                ["bash", BUILD_SCRIPT, APK_BOOTSTRAP_DATASET], env=build_env
            )
        apk = resolve_apk(args.apk)
        apk_identity = local_file_identity(apk)
        native_identity = sha256_apk_member(apk, APK_NATIVE_LIBRARY)
        experiment["native_library"] = {
            "path": f"{apk}!/{APK_NATIVE_LIBRARY}",
            **native_identity,
            "rust_profile": args.rust_profile if args.prepare_apk else None,
            "identity_source": "local-apk-member",
        }
        experiment["apk"] = {"path": str(apk), **apk_identity}
        if args.prepare_apk:
            experiment["apk"]["bootstrap_dataset"] = {
                "path": str(APK_BOOTSTRAP_DATASET),
                **local_file_identity(APK_BOOTSTRAP_DATASET),
            }
        atomic_write_json(experiment_path, experiment)

        if args.prepare_apk:
            # Installation is an explicit one-time preparation step. Dataset
            # changes after this point are delivered through run-as only.
            run_command(
                adb_args(adb, args.serial, "install", "-r", str(apk)),
                timeout=args.run_timeout_seconds,
            )
            run_command(
                adb_args(
                    adb,
                    args.serial,
                    "shell",
                    "cmd",
                    "package",
                    "wait-for-handler",
                    "--timeout",
                    "10000",
                ),
                timeout=15.0,
            )
            run_command(
                adb_args(
                    adb,
                    args.serial,
                    "shell",
                    "cmd",
                    "package",
                    "wait-for-background-handler",
                    "--timeout",
                    "10000",
                ),
                timeout=15.0,
            )
            # Some vendor builds deliver PACKAGE_REPLACED after both package
            # queues report idle. This wait happens only in preparation mode.
            time.sleep(2.0)

        experiment["installed_apk"] = verify_installed_apk(
            adb, args.serial, apk
        )
        experiment["installed_apk"]["prepared_by_this_invocation"] = bool(
            args.prepare_apk
        )
        atomic_write_json(experiment_path, experiment)

        with staged_device_dataset(
            adb, args.serial, args.ply, experiment["dataset"]
        ) as temporary_dataset_path:
            with staged_device_trace(
                adb, args.serial, args.camera_trace, experiment["trace"]
            ) as temporary_trace_path:
                collect_scheduled_runs(
                    args,
                    adb,
                    schedule,
                    output,
                    experiment,
                    experiment_path,
                    temporary_dataset_path,
                    temporary_trace_path,
                    trace_json,
                )

        experiment["status"] = "complete"
        experiment["ended_at_utc"] = utc_now()
        atomic_write_json(experiment_path, experiment)
        print(f"experiment={output}")
        return 0
    except (
        OSError,
        RuntimeError,
        TimeoutError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
    ) as error:
        experiment["status"] = "failed"
        experiment["ended_at_utc"] = utc_now()
        experiment["error"] = str(error)
        atomic_write_json(experiment_path, experiment)
        print(f"benchmark collection failed: {error}", file=sys.stderr)
        print(f"partial_experiment={output}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
