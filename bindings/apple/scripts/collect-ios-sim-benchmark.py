#!/usr/bin/env python3
"""Collect one attested iOS Simulator benchmark without changing artifact v1."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time
from collections.abc import Sequence
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
BUILD_SCRIPT = REPO_ROOT / "bindings/apple/scripts/build-ios-sim-app.sh"
RUN_SCRIPT = REPO_ROOT / "bindings/apple/scripts/run-ios-sim-app.sh"
EXTRACTOR = REPO_ROOT / "bindings/apple/scripts/extract-ios-benchmark-artifacts.py"
VALIDATOR = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
APP_BUNDLE = REPO_ROOT / "target/ios-sim-app/GsplatIOSExample.app"
DEFAULT_XCFRAMEWORK = (
    REPO_ROOT / "bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework"
)
BUNDLE_ID = "com.gsplat.example.ios"
RESULT_PREFIX = "BENCHMARK_RESULT "
RECEIPT_SCHEMA = "gsplat-ios-simulator-collector-receipt/v1"
FAILURE_RECEIPT_SCHEMA = "gsplat-ios-simulator-collector-failure/v1"
FAILURE_ROOT = REPO_ROOT / "target/ios-sim-benchmark-failures"

DEFAULT_BENCHMARK_ARGS = (
    "--gsplat_benchmark",
    "true",
    "--gsplat_camera_trace",
    "camera_trace.json",
    "--gsplat_camera_trace_sequence",
    "true",
    "--gsplat_require_trace_display_match",
    "true",
    "--gsplat_surface_sort_interval",
    "1",
    "--gsplat_surface_async_sort",
    "false",
    "--gsplat_surface_frame_latency",
    "2",
    "--gsplat_surface_order_backend",
    "adaptive",
    "--gsplat_surface_projected_policy",
    "adaptive",
    "--gsplat_geometry_path",
    "packed",
)


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_identity(path: pathlib.Path) -> dict[str, Any]:
    if not path.is_file():
        raise ValueError(f"required file does not exist: {path}")
    return {"bytes": path.stat().st_size, "sha256": sha256_file(path)}


def repository_path(path: pathlib.Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return str(path.resolve())


def run_command(
    args: Sequence[str | os.PathLike[str]],
    *,
    capture: bool = False,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [os.fspath(arg) for arg in args],
        cwd=REPO_ROOT,
        env=env,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
    )


def simulator_inventory(text: str) -> list[dict[str, Any]]:
    try:
        root = json.loads(text)
    except json.JSONDecodeError as error:
        raise ValueError("simctl device inventory is not valid JSON") from error
    devices = root.get("devices") if isinstance(root, dict) else None
    if not isinstance(devices, dict):
        raise ValueError("simctl device inventory has no devices object")
    result: list[dict[str, Any]] = []
    for runtime, entries in devices.items():
        if not isinstance(runtime, str) or not isinstance(entries, list):
            raise ValueError("simctl device inventory is malformed")
        for entry in entries:
            if not isinstance(entry, dict):
                raise ValueError("simctl device record is malformed")
            record = dict(entry)
            record["runtime_identifier"] = runtime
            result.append(record)
    return result


def selected_simulator(records: list[dict[str, Any]], udid: str) -> dict[str, Any]:
    matches = [record for record in records if record.get("udid") == udid]
    if len(matches) != 1:
        raise ValueError(f"selected simulator identity was not found exactly once: {udid}")
    record = matches[0]
    required_strings = (
        "name",
        "udid",
        "state",
        "deviceTypeIdentifier",
        "runtime_identifier",
    )
    if any(not isinstance(record.get(key), str) or not record[key] for key in required_strings):
        raise ValueError("selected simulator identity is incomplete")
    if record.get("isAvailable") is not True:
        raise ValueError("selected simulator is unavailable")
    if "SimDeviceType.iPhone" not in record["deviceTypeIdentifier"]:
        raise ValueError("selected simulator is not an iPhone")
    return {key: record[key] for key in required_strings}


def require_same_simulator(
    expected: dict[str, Any], actual: dict[str, Any], *, require_booted: bool
) -> None:
    for key in ("name", "udid", "deviceTypeIdentifier", "runtime_identifier"):
        if expected.get(key) != actual.get(key):
            raise ValueError(f"simulator identity mismatch for {key}")
    if require_booted and actual.get("state") != "Booted":
        raise ValueError("selected simulator did not reach Booted state")


def target_architecture() -> tuple[str, str]:
    machine = os.uname().machine
    if machine == "arm64":
        return "arm64", "aarch64-apple-ios-sim"
    if machine == "x86_64":
        return "x86_64", "x86_64-apple-ios"
    raise ValueError(f"unsupported macOS host architecture: {machine}")


def relevant_xcframework_archives(
    xcframework: pathlib.Path, architecture: str
) -> list[dict[str, Any]]:
    info_path = xcframework / "Info.plist"
    if not info_path.is_file():
        raise ValueError(f"XCFramework Info.plist does not exist: {info_path}")
    with info_path.open("rb") as source:
        info = plistlib.load(source)
    libraries = info.get("AvailableLibraries") if isinstance(info, dict) else None
    if not isinstance(libraries, list):
        raise ValueError("XCFramework AvailableLibraries is missing")
    archives: list[dict[str, Any]] = []
    for library in libraries:
        if not isinstance(library, dict):
            raise ValueError("XCFramework library record is malformed")
        if library.get("SupportedPlatform") != "ios":
            continue
        if library.get("SupportedPlatformVariant") != "simulator":
            continue
        architectures = library.get("SupportedArchitectures")
        if not isinstance(architectures, list) or architecture not in architectures:
            continue
        identifier = library.get("LibraryIdentifier")
        library_path = library.get("LibraryPath")
        if not isinstance(identifier, str) or not isinstance(library_path, str):
            raise ValueError("XCFramework simulator archive identity is incomplete")
        archive = xcframework / identifier / library_path
        identity = file_identity(archive)
        archives.append(
            {
                "xcframework": repository_path(xcframework),
                "role": "packaged_ios_simulator_slice",
                "library_identifier": identifier,
                "path": repository_path(archive),
                **identity,
            }
        )
    if not archives:
        raise ValueError(
            f"XCFramework has no iOS Simulator archive for {architecture}: {xcframework}"
        )
    return archives


def trace_identity(path: pathlib.Path) -> dict[str, Any]:
    identity = file_identity(path)
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"trace is not valid JSON: {path}") from error
    if not isinstance(value, dict):
        raise ValueError("trace root must be an object")
    trace_id = value.get("trace_id")
    content_sha256 = value.get("content_sha256")
    if not isinstance(trace_id, str) or not trace_id:
        raise ValueError("trace_id is missing")
    if (
        not isinstance(content_sha256, str)
        or len(content_sha256) != 64
        or any(character not in "0123456789abcdef" for character in content_sha256)
    ):
        raise ValueError("trace content_sha256 is malformed")
    return {
        "path": repository_path(path),
        "trace_id": trace_id,
        "content_sha256": content_sha256,
        **identity,
    }


def app_identity(
    app_bundle: pathlib.Path, expected_commit: str, expected_dirty: bool
) -> dict[str, Any]:
    info_path = app_bundle / "Info.plist"
    if not info_path.is_file():
        raise ValueError(f"app Info.plist does not exist: {info_path}")
    with info_path.open("rb") as source:
        info = plistlib.load(source)
    if info.get("CFBundleIdentifier") != BUNDLE_ID:
        raise ValueError("app bundle identifier mismatch")
    executable_name = info.get("CFBundleExecutable")
    if not isinstance(executable_name, str) or not executable_name:
        raise ValueError("app executable name is missing")
    if info.get("GsplatRepositoryCommit") != expected_commit:
        raise ValueError("app repository commit identity mismatch")
    if info.get("GsplatRepositoryDirty") is not expected_dirty:
        raise ValueError("app repository dirty identity mismatch")
    executable = app_bundle / executable_name
    return {
        "bundle_path": repository_path(app_bundle),
        "bundle_id": BUNDLE_ID,
        "executable_path": repository_path(executable),
        "repository_commit": expected_commit,
        "repository_dirty": expected_dirty,
        "build_profile": info.get("GsplatBuildProfile"),
        **file_identity(executable),
    }


def require_artifact_identity(
    artifact_dir: pathlib.Path,
    *,
    commit: str,
    dataset: dict[str, Any],
    trace: dict[str, Any],
    simulator: dict[str, Any],
) -> dict[str, Any]:
    manifest_path = artifact_dir / "manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("extracted manifest is unavailable or malformed") from error
    if not isinstance(manifest, dict):
        raise ValueError("extracted manifest must be an object")
    build = manifest.get("build")
    if not isinstance(build, dict):
        raise ValueError("manifest build identity is missing")
    if build.get("repository_commit") != commit or build.get("dirty") is not False:
        raise ValueError("manifest repository identity mismatch")
    manifest_dataset = manifest.get("dataset")
    if not isinstance(manifest_dataset, dict):
        raise ValueError("manifest dataset identity is missing")
    if (
        manifest_dataset.get("sha256") != dataset["sha256"]
        or manifest_dataset.get("bytes") != dataset["bytes"]
    ):
        raise ValueError("manifest dataset identity mismatch")
    manifest_trace = manifest.get("trace")
    if not isinstance(manifest_trace, dict):
        raise ValueError("manifest trace identity is missing")
    if (
        manifest_trace.get("id") != trace["trace_id"]
        or manifest_trace.get("sha256") != trace["content_sha256"]
    ):
        raise ValueError("manifest trace identity mismatch")
    environment = manifest.get("environment")
    if not isinstance(environment, dict) or environment.get("platform") != "ios":
        raise ValueError("manifest platform identity mismatch")
    runtime_version = simulator["runtime_identifier"].rsplit(".iOS-", 1)[-1].replace("-", ".")
    if environment.get("os") != runtime_version:
        raise ValueError("manifest simulator runtime identity mismatch")
    run_id = manifest.get("run_id")
    if not isinstance(run_id, str) or not run_id:
        raise ValueError("manifest run_id is missing")
    return {"run_id": run_id, "manifest_sha256": sha256_file(manifest_path)}


def terminal_count(log: str) -> int:
    return sum(RESULT_PREFIX in line for line in log.splitlines())


def terminal_lines(log: str) -> list[str]:
    return [line for line in log.splitlines() if RESULT_PREFIX in line]


def merged_launch_streams(
    stdout_path: pathlib.Path, stderr_path: pathlib.Path
) -> str:
    streams: list[str] = []
    for path in (stdout_path, stderr_path):
        if not os.path.lexists(path):
            continue
        if not path.is_file():
            raise ValueError(f"simulator launch stream is not a file: {path}")
        text = path.read_text(encoding="utf-8", errors="replace")
        if text and not text.endswith("\n"):
            text += "\n"
        streams.append(text)
    return "".join(streams)


def finalized_launch_capture(observed: str, after_termination: str) -> str:
    observed_terminals = terminal_lines(observed)
    if len(observed_terminals) != 1:
        raise RuntimeError(
            "observed launch snapshot must contain exactly one BENCHMARK_RESULT terminal"
        )
    final_terminals = terminal_lines(after_termination)
    if len(final_terminals) == 0:
        raise RuntimeError(
            "launch streams lost the observed BENCHMARK_RESULT terminal"
        )
    if len(final_terminals) > 1:
        raise RuntimeError(
            "launch streams contain duplicate BENCHMARK_RESULT terminals"
        )
    if final_terminals != observed_terminals:
        raise RuntimeError(
            "launch terminal changed while terminating simulator app"
        )
    return after_termination


def preserve_collector_failure(
    staging: pathlib.Path,
    *,
    failure_root: pathlib.Path,
    destination: pathlib.Path,
    started_at: str,
    commit: str,
    simulator: dict[str, Any],
    dataset: dict[str, Any],
    trace: dict[str, Any],
    benchmark_args: Sequence[str],
    stage: str,
    error: BaseException,
    stdout_path: pathlib.Path | None,
    stderr_path: pathlib.Path | None,
) -> pathlib.Path:
    streams_dir = staging / "launch-streams"
    streams_dir.mkdir(exist_ok=True)
    stream_metadata: dict[str, Any] = {}
    for name, source in (("stdout", stdout_path), ("stderr", stderr_path)):
        if source is None or not source.is_file():
            stream_metadata[name] = {"available": False}
            continue
        retained = streams_dir / f"{name}.log"
        shutil.copy2(source, retained)
        text = retained.read_text(encoding="utf-8", errors="replace")
        stream_metadata[name] = {
            "available": True,
            "path": f"launch-streams/{name}.log",
            "terminal_count": terminal_count(text),
            **file_identity(retained),
        }
    merged = merged_launch_streams(
        streams_dir / "stdout.log", streams_dir / "stderr.log"
    )
    observed_path = staging / "raw-console.log"
    observed_metadata: dict[str, Any] = {"available": False}
    if observed_path.is_file():
        observed_text = observed_path.read_text(encoding="utf-8", errors="replace")
        observed_metadata = {
            "available": True,
            "path": "raw-console.log",
            "terminal_count": terminal_count(observed_text),
            **file_identity(observed_path),
        }
    receipt = {
        "schema": FAILURE_RECEIPT_SCHEMA,
        "record_type": "collector_failure",
        "started_at_utc": started_at,
        "ended_at_utc": utc_now(),
        "repository": {"commit": commit, "dirty": False},
        "simulator": simulator,
        "dataset": dataset,
        "trace": trace,
        "requested_destination": str(destination),
        "benchmark_args": list(benchmark_args),
        "failure": {
            "stage": stage,
            "type": type(error).__name__,
            "message": str(error),
        },
        "launch_streams": {
            **stream_metadata,
            "merged_terminal_count": terminal_count(merged),
        },
        "observed_capture": observed_metadata,
    }
    (staging / "collector-failure.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    failure_root.mkdir(parents=True, exist_ok=True)
    retained_root = pathlib.Path(
        tempfile.mkdtemp(
            prefix=f"{destination.name}-{commit[:12]}-", dir=failure_root
        )
    )
    shutil.copytree(staging, retained_root, dirs_exist_ok=True)
    shutil.rmtree(staging, ignore_errors=True)
    return retained_root


def collector_receipt(
    *,
    started_at: str,
    ended_at: str,
    commit: str,
    simulator: dict[str, Any],
    built_app: dict[str, Any],
    installed_bundle: pathlib.Path,
    installed_app: dict[str, Any],
    xcframework_archives: list[dict[str, Any]],
    native_archive: dict[str, Any],
    dataset: dict[str, Any],
    trace: dict[str, Any],
    raw_log: dict[str, Any],
    artifact: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": RECEIPT_SCHEMA,
        "record_type": "collector_metadata",
        "started_at_utc": started_at,
        "ended_at_utc": ended_at,
        "repository": {"commit": commit, "dirty": False},
        "simulator": simulator,
        "app_executable": {
            **built_app,
            "installed_bundle_path": str(installed_bundle),
            "installed_executable_sha256": installed_app["sha256"],
            "installed_executable_bytes": installed_app["bytes"],
        },
        "xcframework_archives": xcframework_archives,
        "native_archive": native_archive,
        "dataset": dataset,
        "trace": trace,
        "raw_log": {"path": "raw-console.log", **raw_log},
        "benchmark_artifact": {"path": "artifact", **artifact},
    }


def capture_launch_session(
    command: Sequence[str],
    terminate_command: Sequence[str],
    stdout_path: pathlib.Path,
    stderr_path: pathlib.Path,
    log_path: pathlib.Path,
    *,
    env: dict[str, str],
    timeout_seconds: float,
) -> None:
    for path in (stdout_path, stderr_path, log_path):
        if os.path.lexists(path):
            raise ValueError(f"launch capture path already exists: {path}")
    observed_capture: str | None = None
    try:
        launch = run_command(command, capture=True, env=env)
        if terminal_count(launch.stdout) != 0:
            raise RuntimeError(
                "launch control output unexpectedly contains BENCHMARK_RESULT"
            )
        deadline = time.monotonic() + timeout_seconds
        found_terminal = False
        while time.monotonic() < deadline:
            count = terminal_count(merged_launch_streams(stdout_path, stderr_path))
            if count > 1:
                raise RuntimeError(
                    "launch streams contain duplicate BENCHMARK_RESULT terminals"
                )
            if count == 1:
                observed_capture = merged_launch_streams(stdout_path, stderr_path)
                log_path.write_text(observed_capture, encoding="utf-8")
                found_terminal = True
                break
            time.sleep(0.1)
        if not found_terminal:
            raise TimeoutError(
                f"benchmark timed out after {timeout_seconds:g}s without BENCHMARK_RESULT"
            )
    finally:
        active_error = sys.exc_info()[0] is not None
        terminated = subprocess.run(
            [os.fspath(arg) for arg in terminate_command],
            cwd=REPO_ROOT,
            env=env,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        if not active_error and terminated.returncode != 0:
            detail = terminated.stdout.strip()
            raise RuntimeError(
                "failed to terminate simulator app after benchmark terminal"
                + (f": {detail}" if detail else "")
            )

    time.sleep(0.25)
    if observed_capture is None:
        raise RuntimeError("benchmark terminal snapshot was not retained")
    captured = finalized_launch_capture(
        observed_capture,
        merged_launch_streams(stdout_path, stderr_path),
    )
    if terminal_count(captured) != 1:
        raise RuntimeError(
            "launch streams must contain exactly one BENCHMARK_RESULT terminal"
        )
    log_path.write_text(captured, encoding="utf-8")


def installed_app_bundle(udid: str) -> pathlib.Path:
    result = run_command(
        ["xcrun", "simctl", "get_app_container", udid, BUNDLE_ID, "app"],
        capture=True,
    )
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise ValueError("installed app container identity is malformed")
    return pathlib.Path(lines[0])


def installed_data_container(udid: str) -> pathlib.Path:
    result = run_command(
        ["xcrun", "simctl", "get_app_container", udid, BUNDLE_ID, "data"],
        capture=True,
    )
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise ValueError("installed app data container identity is malformed")
    container = pathlib.Path(lines[0])
    temporary = container / "tmp"
    if not temporary.is_dir():
        raise ValueError("installed app temporary container does not exist")
    return container


def collect(args: argparse.Namespace) -> None:
    destination = args.output.resolve()
    if os.path.lexists(destination):
        raise ValueError(f"destination already exists: {destination}")
    dataset_path = args.dataset.resolve()
    trace_path = args.trace.resolve()
    dataset = {"path": repository_path(dataset_path), **file_identity(dataset_path)}
    trace = trace_identity(trace_path)

    status = run_command(["git", "status", "--porcelain"], capture=True).stdout
    if status:
        raise ValueError("repository must be clean before collector build")
    commit = run_command(["git", "rev-parse", "HEAD"], capture=True).stdout.strip()
    if len(commit) != 40:
        raise ValueError("repository HEAD identity is malformed")

    architecture, rust_target = target_architecture()
    xcframework_archives: list[dict[str, Any]] = []
    for path in args.xcframework:
        xcframework_archives.extend(
            relevant_xcframework_archives(path.resolve(), architecture)
        )

    inventory = simulator_inventory(
        run_command(
            ["xcrun", "simctl", "list", "devices", "available", "-j"],
            capture=True,
        ).stdout
    )
    selected = selected_simulator(inventory, args.simulator_id)
    if selected["state"] == "Shutdown":
        run_command(["xcrun", "simctl", "boot", args.simulator_id])
    elif selected["state"] != "Booted":
        raise ValueError(f"selected simulator has unsupported state: {selected['state']}")
    run_command(["xcrun", "simctl", "bootstatus", args.simulator_id, "-b"])
    booted = selected_simulator(
        simulator_inventory(
            run_command(
                ["xcrun", "simctl", "list", "devices", "available", "-j"],
                capture=True,
            ).stdout
        ),
        args.simulator_id,
    )
    require_same_simulator(selected, booted, require_booted=True)

    build_env = os.environ.copy()
    build_env["GSPLAT_CAMERA_TRACE_PATH"] = str(trace_path)
    run_command(["bash", BUILD_SCRIPT, dataset_path], env=build_env)
    built_app = app_identity(APP_BUNDLE, commit, False)
    if file_identity(APP_BUNDLE / "showcase.ply") != file_identity(dataset_path):
        raise ValueError("bundled dataset identity mismatch")
    if file_identity(APP_BUNDLE / "camera_trace.json") != file_identity(trace_path):
        raise ValueError("bundled trace identity mismatch")
    native_archive_path = REPO_ROOT / f"target/{rust_target}/debug/libgsplat_ffi_c.a"
    native_archive = {
        "path": repository_path(native_archive_path),
        "role": "app_link_input",
        "rust_target": rust_target,
        **file_identity(native_archive_path),
    }

    destination.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(
        tempfile.mkdtemp(prefix=f".{destination.name}.", dir=destination.parent)
    )
    started_at = utc_now()
    launch_session: pathlib.Path | None = None
    launch_stdout: pathlib.Path | None = None
    launch_stderr: pathlib.Path | None = None
    launch_started = False
    failure_stage = "install"
    try:
        raw_log = staging / "raw-console.log"
        run_command(["xcrun", "simctl", "install", args.simulator_id, APP_BUNDLE])
        installed_bundle = installed_app_bundle(args.simulator_id)
        installed_app = app_identity(installed_bundle, commit, False)
        if (
            installed_app["sha256"] != built_app["sha256"]
            or installed_app["bytes"] != built_app["bytes"]
        ):
            raise ValueError("installed app executable identity mismatch")
        data_container = installed_data_container(args.simulator_id)
        launch_session = pathlib.Path(
            tempfile.mkdtemp(
                prefix=".gsplat-benchmark-launch-", dir=data_container / "tmp"
            )
        )
        launch_stdout = launch_session / "stdout.log"
        launch_stderr = launch_session / "stderr.log"
        launch_env = os.environ.copy()
        launch_env["IOS_SIMULATOR_ID"] = args.simulator_id
        launch_env["IOS_SIMULATOR_SKIP_BUILD"] = "1"
        launch_env["IOS_SIMULATOR_SKIP_INSTALL"] = "1"
        launch_env["IOS_SIMULATOR_STDOUT_PATH"] = str(launch_stdout)
        launch_env["IOS_SIMULATOR_STDERR_PATH"] = str(launch_stderr)
        benchmark_args = args.benchmark_args or list(DEFAULT_BENCHMARK_ARGS)
        if benchmark_args and benchmark_args[0] == "--":
            benchmark_args = benchmark_args[1:]
        failure_stage = "launch_capture"
        launch_started = True
        capture_launch_session(
            ["bash", os.fspath(RUN_SCRIPT), "--", *benchmark_args],
            ["xcrun", "simctl", "terminate", args.simulator_id, BUNDLE_ID],
            launch_stdout,
            launch_stderr,
            raw_log,
            env=launch_env,
            timeout_seconds=args.timeout_seconds,
        )
        if terminal_count(raw_log.read_text(encoding="utf-8", errors="replace")) != 1:
            raise ValueError("raw console log must contain exactly one BENCHMARK_RESULT")

        failure_stage = "extract_validate"
        artifact_dir = staging / "artifact"
        run_command(
            [
                sys.executable,
                EXTRACTOR,
                raw_log,
                artifact_dir,
                "--validator",
                args.validator.resolve(),
            ]
        )
        artifact_identity = require_artifact_identity(
            artifact_dir,
            commit=commit,
            dataset=dataset,
            trace=trace,
            simulator=booted,
        )
        final_inventory = selected_simulator(
            simulator_inventory(
                run_command(
                    ["xcrun", "simctl", "list", "devices", "available", "-j"],
                    capture=True,
                ).stdout
            ),
            args.simulator_id,
        )
        require_same_simulator(booted, final_inventory, require_booted=True)

        receipt = collector_receipt(
            started_at=started_at,
            ended_at=utc_now(),
            commit=commit,
            simulator=final_inventory,
            built_app=built_app,
            installed_bundle=installed_bundle,
            installed_app=installed_app,
            xcframework_archives=xcframework_archives,
            native_archive=native_archive,
            dataset=dataset,
            trace=trace,
            raw_log=file_identity(raw_log),
            artifact=artifact_identity,
        )
        (staging / "collector-receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        os.rename(staging, destination)
    except BaseException as error:
        if launch_started:
            retained = preserve_collector_failure(
                staging,
                failure_root=FAILURE_ROOT,
                destination=destination,
                started_at=started_at,
                commit=commit,
                simulator=booted,
                dataset=dataset,
                trace=trace,
                benchmark_args=benchmark_args,
                stage=failure_stage,
                error=error,
                stdout_path=launch_stdout,
                stderr_path=launch_stderr,
            )
            print(f"collector_failure_dir={retained}", file=sys.stderr)
        else:
            shutil.rmtree(staging, ignore_errors=True)
        raise
    finally:
        if launch_session is not None:
            shutil.rmtree(launch_session, ignore_errors=True)
    print(f"collector_dir={destination}")


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    raw_args = list(sys.argv[1:] if argv is None else argv)
    if "--" in raw_args:
        separator = raw_args.index("--")
        benchmark_args = raw_args[separator + 1 :]
        raw_args = raw_args[:separator]
    else:
        benchmark_args = []
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument(
        "--simulator-id", default=os.environ.get("IOS_SIMULATOR_ID"), required=False
    )
    parser.add_argument("--dataset", type=pathlib.Path, required=True)
    parser.add_argument("--trace", type=pathlib.Path, required=True)
    parser.add_argument(
        "--xcframework",
        type=pathlib.Path,
        action="append",
        default=None,
        help="XCFramework to attest (default: local GsplatFFI.xcframework)",
    )
    parser.add_argument("--validator", type=pathlib.Path, default=VALIDATOR)
    parser.add_argument("--timeout-seconds", type=float, default=180.0)
    args = parser.parse_args(raw_args)
    if not args.simulator_id:
        parser.error("--simulator-id or IOS_SIMULATOR_ID is required")
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    if args.xcframework is None:
        args.xcframework = [DEFAULT_XCFRAMEWORK]
    args.benchmark_args = benchmark_args
    return args


def main() -> int:
    try:
        collect(parse_args())
    except (
        OSError,
        ValueError,
        RuntimeError,
        TimeoutError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"collector failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
