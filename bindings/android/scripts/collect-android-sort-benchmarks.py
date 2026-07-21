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
APK_METADATA = REPO_ROOT / "examples/android/app/build/outputs/apk/debug/output-metadata.json"
APK_DIR = APK_METADATA.parent
APK_NATIVE_LIBRARY = "lib/arm64-v8a/libgsplat_jni.so"
PACKAGE = "com.gsplat.example"
ACTIVITY = f"{PACKAGE}/.MainActivity"
LOG_TAG = "GsplatExample:I"
BACKENDS = ("cpu", "gpu", "adaptive")
INTERNAL_DATASET = "files/imported_scene.ply"
DEVICE_DATASET_PREFIX = "/data/local/tmp/gsplat-benchmark-"


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
    return [
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
        "direct",
        "--es",
        "gsplat_surface_order_backend",
        backend,
    ]


def stop_logcat(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


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
                    and "GSPLAT_BENCHMARK_SUMMARY " in contents
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


def validate_run_artifact(
    manifest: dict[str, Any],
    summary: dict[str, Any],
    expected_backend: str,
    expected_dataset: dict[str, Any],
) -> None:
    requested = manifest.get("renderer", {}).get("order_backend_requested")
    if requested != expected_backend:
        raise RuntimeError(
            f"artifact requested backend {requested!r}, expected {expected_backend!r}"
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
    backends = args.backend or ["cpu", "gpu"]
    if len(set(backends)) != len(backends):
        raise ValueError("--backend values must be unique")
    if args.async_sort and any(backend != "cpu" for backend in backends):
        raise ValueError("--async-sort is only compatible with the cpu backend")
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


def collect_scheduled_runs(
    args: argparse.Namespace,
    adb: pathlib.Path | str,
    schedule: list[RunSpec],
    output: pathlib.Path,
    experiment: dict[str, Any],
    experiment_path: pathlib.Path,
    temporary_dataset_path: str,
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
                "geometry_path": "direct",
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
            ]
        )
        manifest = json.loads(
            (artifact_dir / "manifest.json").read_text(encoding="utf-8")
        )
        summary = json.loads(
            (artifact_dir / "summary.json").read_text(encoding="utf-8")
        )
        validate_run_artifact(
            manifest,
            summary,
            spec.backend,
            experiment["dataset"],
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
    experiment: dict[str, Any] = {
        "schema": "gsplat-android-sort-experiment/v1",
        "status": "running",
        "started_at_utc": utc_now(),
        "package_cleared_before_each_run": PACKAGE,
        "dataset": dataset_identity,
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
            collect_scheduled_runs(
                args,
                adb,
                schedule,
                output,
                experiment,
                experiment_path,
                temporary_dataset_path,
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
