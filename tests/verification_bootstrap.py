#!/usr/bin/env python3
"""Discover prerequisites and run the repository's canonical verification paths.

The doctor and command modes are read-only. They never start a browser, invoke
adb/simctl, build code, install a package, or change global shell configuration.
Run mode only delegates to existing repository scripts after every prerequisite
for the selected profile is already present.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import pathlib
import platform
import re
import shlex
import shutil
import subprocess
import sys
from collections.abc import Callable, Iterable, Mapping, Sequence


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
ANDROID_NDK_VERSION = "29.0.14206865"
ANDROID_PLATFORM = "android-35"
ANDROID_BUILD_TOOLS = "35.0.0"
ANDROID_RUST_TARGET = "aarch64-linux-android"
WEB_RUST_TARGET = "wasm32-unknown-unknown"
WEB_WASM_PROFILE_ENV = "GSPLAT_WEB_WASM_PROFILE"
WEB_WASM_OUT_DIR_ENV = "GSPLAT_WEB_WASM_OUT_DIR"
Q1_QUALITY_EXPECTED_COMMIT_ENV = "GSPLAT_Q1_PRODUCT_QUALITY_EXPECTED_COMMIT"
Q1_QUALITY_OUTPUT_ENV = "GSPLAT_Q1_PRODUCT_QUALITY_OUTPUT"
Q1_QUALITY_FORMAL_TRACE_ENV = "GSPLAT_Q1_FORMAL_TRACE_AUTHORITY"
Q1_QUALITY_EVALUATION_ENV = "GSPLAT_Q1_EVALUATION_AUTHORITY"
Q1_QUALITY_TRUCK_ENV = "GSPLAT_Q1_TRUCK_DATASET"
APPLE_XCFRAMEWORK_TARGETS = (
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
    "x86_64-apple-ios",
)


@dataclasses.dataclass(frozen=True)
class Probe:
    key: str
    ok: bool
    detail: str
    remedy: str | None = None

    def to_json(self) -> dict[str, object]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class Command:
    argv: tuple[str, ...]
    env: Mapping[str, str] = dataclasses.field(default_factory=dict)

    def display(self) -> str:
        assignments = " ".join(
            f"{key}={shlex.quote(value)}" for key, value in sorted(self.env.items())
        )
        command = shlex.join(self.argv)
        return f"{assignments} {command}".strip()


@dataclasses.dataclass(frozen=True)
class ProfileResult:
    name: str
    description: str
    touches_device: bool
    probes: tuple[Probe, ...]
    commands: tuple[Command, ...]

    @property
    def ready(self) -> bool:
        return all(probe.ok for probe in self.probes)

    def to_json(self) -> dict[str, object]:
        return {
            "name": self.name,
            "description": self.description,
            "touches_device": self.touches_device,
            "ready": self.ready,
            "probes": [probe.to_json() for probe in self.probes],
            "commands": [
                {"argv": list(command.argv), "env": dict(command.env)}
                for command in self.commands
            ],
        }


class Discovery:
    """Side-effect-free prerequisite discovery with injectable process helpers."""

    def __init__(
        self,
        *,
        env: Mapping[str, str] | None = None,
        home: pathlib.Path | None = None,
        which: Callable[[str], str | None] = shutil.which,
        capture: Callable[[Sequence[str]], tuple[int, str]] | None = None,
        host_system: str | None = None,
        host_machine: str | None = None,
    ) -> None:
        self.env = dict(os.environ if env is None else env)
        self.home = pathlib.Path.home() if home is None else home
        self.which = which
        self.capture = capture or self._capture
        self.host_system = platform.system() if host_system is None else host_system
        self.host_machine = platform.machine() if host_machine is None else host_machine

    @staticmethod
    def _capture(argv: Sequence[str]) -> tuple[int, str]:
        completed = subprocess.run(
            argv,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        return completed.returncode, completed.stdout.strip()

    def command_path(self, name: str) -> pathlib.Path | None:
        located = self.which(name)
        return pathlib.Path(located).resolve() if located else None

    def cargo_command_path(self, name: str) -> pathlib.Path | None:
        path_command = self.command_path(name)
        if path_command is not None:
            return path_command

        cargo_home = self.env.get("CARGO_HOME")
        cargo_root = (
            pathlib.Path(cargo_home).expanduser()
            if cargo_home
            else self.home / ".cargo"
        )
        candidate = cargo_root / "bin" / name
        return (
            candidate.resolve()
            if candidate.is_file() and os.access(candidate, os.X_OK)
            else None
        )

    def brew_prefix(self, formula: str) -> pathlib.Path | None:
        brew = self.command_path("brew")
        if brew is None:
            return None
        code, output = self.capture((str(brew), "--prefix", formula))
        candidate = pathlib.Path(output) if code == 0 and output else None
        return candidate if candidate is not None and candidate.is_dir() else None

    def homebrew_prefix(self) -> pathlib.Path | None:
        brew = self.command_path("brew")
        if brew is None:
            return None
        code, output = self.capture((str(brew), "--prefix"))
        candidate = pathlib.Path(output) if code == 0 and output else None
        return candidate if candidate is not None and candidate.is_dir() else None

    def android_sdk(self) -> tuple[pathlib.Path | None, str]:
        for variable in ("ANDROID_SDK_ROOT", "ANDROID_HOME"):
            value = self.env.get(variable)
            if value:
                candidate = pathlib.Path(value).expanduser()
                return (candidate if candidate.is_dir() else None, variable)

        adb = self.command_path("adb")
        adb_sdk = (
            adb.parent.parent
            if adb is not None and adb.parent.name == "platform-tools"
            else None
        )
        homebrew = self.homebrew_prefix()
        homebrew_sdk = homebrew / "share/android-commandlinetools" if homebrew else None
        candidates: list[tuple[pathlib.Path | None, str]] = [
            (self.home / "Library/Android/sdk", "standard macOS SDK location"),
            (homebrew_sdk, "Homebrew share directory"),
            (
                self.brew_prefix("android-commandlinetools"),
                "Homebrew android-commandlinetools formula",
            ),
            (adb_sdk, "adb on PATH"),
        ]
        for candidate, source in candidates:
            if candidate is not None and candidate.is_dir():
                return candidate, source
        return None, "automatic discovery"

    def java_home(self) -> tuple[pathlib.Path | None, str]:
        explicit = self.env.get("JAVA_HOME")
        if explicit:
            candidate = pathlib.Path(explicit).expanduser()
            return (candidate if candidate.is_dir() else None, "JAVA_HOME")

        java_home = pathlib.Path("/usr/libexec/java_home")
        if self.host_system == "Darwin" and java_home.is_file():
            code, output = self.capture((str(java_home), "-v", "21"))
            if code == 0 and output:
                candidate = pathlib.Path(output)
                if candidate.is_dir():
                    return candidate, "/usr/libexec/java_home -v 21"

        brew = self.brew_prefix("openjdk@21")
        if brew is not None:
            return brew, "Homebrew openjdk@21"

        java = self.command_path("java")
        if java is not None:
            candidate = java.parent.parent
            if candidate.is_dir():
                return candidate, "java on PATH"
        return None, "automatic discovery"

    def chrome(self) -> tuple[pathlib.Path | None, str]:
        explicit = self.env.get("CHROME_PATH")
        if explicit:
            candidate = pathlib.Path(explicit).expanduser()
            return (candidate if os.access(candidate, os.X_OK) else None, "CHROME_PATH")

        candidates = (
            pathlib.Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            pathlib.Path("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        )
        for candidate in candidates:
            if os.access(candidate, os.X_OK):
                return candidate, "standard application location"
        for binary in ("google-chrome", "chromium", "chromium-browser"):
            candidate = self.command_path(binary)
            if candidate is not None:
                return candidate, f"{binary} on PATH"
        return None, "automatic discovery"

    def rust_targets(self) -> set[str]:
        rustup = self.command_path("rustup")
        if rustup is None:
            return set()
        code, output = self.capture((str(rustup), "target", "list", "--installed"))
        return set(output.splitlines()) if code == 0 else set()

    def git_head(self) -> str:
        git = self.command_path("git")
        if git is None:
            return "unknown"
        code, output = self.capture(
            (str(git), "-C", str(REPO_ROOT), "rev-parse", "--short=12", "HEAD")
        )
        return output if code == 0 and output else "unknown"


def executable_probe(discovery: Discovery, name: str, remedy: str) -> Probe:
    path = discovery.command_path(name)
    return Probe(
        key=name,
        ok=path is not None,
        detail=str(path) if path else "not found on PATH",
        remedy=None if path else remedy,
    )


def file_probe(key: str, path: pathlib.Path, remedy: str) -> Probe:
    return Probe(
        key=key,
        ok=path.is_file(),
        detail=str(path),
        remedy=None if path.is_file() else remedy,
    )


def directory_probe(key: str, path: pathlib.Path, remedy: str) -> Probe:
    return Probe(
        key=key,
        ok=path.is_dir(),
        detail=str(path),
        remedy=None if path.is_dir() else remedy,
    )


def fresh_output_probe(variable: str, value: str) -> Probe:
    """Require a collector destination that has never been published.

    These reusable profiles treat a published or partial run as immutable.
    Checking the destination in doctor/command mode keeps that failure before
    builds, browser launch, or device mutation while preserving the old
    evidence for diagnosis.
    """

    # Match the delegated collectors exactly: absolute paths stay absolute and
    # relative paths resolve from the repository root. Neither collector
    # performs shell tilde expansion because bootstrap executes argv directly.
    path = pathlib.Path(value)
    if not path.is_absolute():
        path = REPO_ROOT / path
    occupied = os.path.lexists(path)
    return Probe(
        key=f"fresh-output:{variable}",
        ok=not occupied,
        detail=f"{path} ({'already exists' if occupied else 'available'})",
        remedy=None
        if not occupied
        else (
            f"set {variable} to a new output path; preserve the existing "
            "artifact instead of deleting or overwriting it"
        ),
    )


def rust_target_probes(discovery: Discovery, targets: Iterable[str]) -> list[Probe]:
    installed = discovery.rust_targets()
    return [
        Probe(
            key=f"rust-target:{target}",
            ok=target in installed,
            detail="installed" if target in installed else "not installed",
            remedy=None
            if target in installed
            else f"install explicitly: rustup target add {target}",
        )
        for target in targets
    ]


def read_locked_wasm_bindgen_version() -> str:
    lines = (REPO_ROOT / "Cargo.lock").read_text(encoding="utf-8").splitlines()
    for index, line in enumerate(lines):
        if line == 'name = "wasm-bindgen"':
            for next_line in lines[index + 1 : index + 5]:
                match = re.fullmatch(r'version = "([^"]+)"', next_line)
                if match:
                    return match.group(1)
    raise RuntimeError("Cargo.lock does not contain a wasm-bindgen package version")


def wasm_bindgen_probe(discovery: Discovery) -> Probe:
    expected = read_locked_wasm_bindgen_version()
    binary = discovery.cargo_command_path("wasm-bindgen")
    if binary is None:
        return Probe(
            key="wasm-bindgen",
            ok=False,
            detail=f"missing; Cargo.lock requires {expected}",
            remedy=(
                "install explicitly: cargo install wasm-bindgen-cli "
                f"--version {expected} --locked"
            ),
        )
    code, output = discovery.capture((str(binary), "--version"))
    match = re.search(r"(\d+\.\d+\.\d+)", output)
    actual = match.group(1) if code == 0 and match else "unknown"
    return Probe(
        key="wasm-bindgen",
        ok=actual == expected,
        detail=f"{binary} version={actual} expected={expected}",
        remedy=None
        if actual == expected
        else (
            "install the locked CLI explicitly: cargo install wasm-bindgen-cli "
            f"--version {expected} --locked"
        ),
    )


def android_environment(discovery: Discovery) -> tuple[list[Probe], dict[str, str]]:
    probes = [
        executable_probe(discovery, "bash", "install Bash to run repository scripts"),
        executable_probe(discovery, "cargo", "install the repository-pinned Rust toolchain"),
        executable_probe(
            discovery, "rustup", "install rustup and the repository-pinned Rust toolchain"
        ),
        executable_probe(discovery, "python3", "install Python 3"),
    ]
    sdk, sdk_source = discovery.android_sdk()
    probes.append(
        Probe(
            key="android-sdk",
            ok=sdk is not None,
            detail=f"{sdk or 'not found'} ({sdk_source})",
            remedy=None
            if sdk is not None
            else "set ANDROID_SDK_ROOT or install Android command-line tools",
        )
    )
    java_home, java_source = discovery.java_home()
    probes.append(
        Probe(
            key="java-21-home",
            ok=java_home is not None,
            detail=f"{java_home or 'not found'} ({java_source})",
            remedy=None if java_home is not None else "set JAVA_HOME to a JDK 21 installation",
        )
    )
    env: dict[str, str] = {}
    if sdk is not None:
        env["ANDROID_SDK_ROOT"] = str(sdk)
        env["ANDROID_HOME"] = str(sdk)
        probes.extend(
            (
                directory_probe(
                    "android-ndk",
                    sdk / "ndk" / ANDROID_NDK_VERSION,
                    f"install NDK {ANDROID_NDK_VERSION} into the selected SDK",
                ),
                directory_probe(
                    "android-platform",
                    sdk / "platforms" / ANDROID_PLATFORM,
                    f"install SDK platform {ANDROID_PLATFORM}",
                ),
                directory_probe(
                    "android-build-tools",
                    sdk / "build-tools" / ANDROID_BUILD_TOOLS,
                    f"install Android build-tools {ANDROID_BUILD_TOOLS}",
                ),
                file_probe(
                    "adb",
                    sdk / "platform-tools" / "adb",
                    "install Android platform-tools in the selected SDK",
                ),
            )
        )
    if java_home is not None:
        env["JAVA_HOME"] = str(java_home)
        probes.extend(
            (
                file_probe(
                    "java",
                    java_home / "bin" / "java",
                    "use a complete JDK 21 installation",
                ),
                file_probe(
                    "jni-header",
                    java_home / "include" / "jni.h",
                    "use a JDK with JNI headers",
                ),
            )
        )
        code, output = discovery.capture((str(java_home / "bin" / "java"), "-version"))
        match = re.search(r'version "(\d+)', output)
        major = int(match.group(1)) if code == 0 and match else None
        probes.append(
            Probe(
                key="java-version",
                ok=major == 21,
                detail=f"major={major if major is not None else 'unknown'}",
                remedy=None if major == 21 else "select JDK 21 with JAVA_HOME",
            )
        )
    probes.extend(rust_target_probes(discovery, (ANDROID_RUST_TARGET,)))
    return probes, env


def common_rust_probes(discovery: Discovery) -> list[Probe]:
    return [
        executable_probe(discovery, "bash", "install Bash to run repository scripts"),
        executable_probe(discovery, "cargo", "install the repository-pinned Rust toolchain"),
        executable_probe(
            discovery, "rustup", "install rustup and the repository-pinned Rust toolchain"
        ),
    ]


def macos_probes(discovery: Discovery) -> list[Probe]:
    probes = common_rust_probes(discovery)
    probes.append(
        Probe(
            key="macos-host",
            ok=discovery.host_system == "Darwin",
            detail=discovery.host_system,
            remedy=None if discovery.host_system == "Darwin" else "run this profile on macOS",
        )
    )
    return probes


def web_environment(discovery: Discovery) -> tuple[list[Probe], dict[str, str]]:
    probes = common_rust_probes(discovery)
    probes.extend(
        (
            executable_probe(
                discovery, "node", "install the Node.js version used by this repository"
            ),
            executable_probe(discovery, "npm", "install npm"),
            executable_probe(discovery, "python3", "install Python 3 for the local HTTP server"),
            wasm_bindgen_probe(discovery),
        )
    )
    probes.extend(rust_target_probes(discovery, (WEB_RUST_TARGET,)))
    chrome, chrome_source = discovery.chrome()
    probes.append(
        Probe(
            key="chrome",
            ok=chrome is not None,
            detail=f"{chrome or 'not found'} ({chrome_source})",
            remedy=None
            if chrome is not None
            else "set CHROME_PATH to an executable Chrome/Chromium binary",
        )
    )
    puppeteer = REPO_ROOT / "tests/competitive/playcanvas/node_modules/puppeteer-core/package.json"
    probes.append(
        file_probe(
            "puppeteer-core",
            puppeteer,
            "install the pinned harness dependencies explicitly: "
            "npm ci --ignore-scripts --prefix tests/competitive/playcanvas",
        )
    )
    env = {"CHROME_PATH": str(chrome)} if chrome is not None else {}
    return probes, env


def apple_environment(
    discovery: Discovery,
    *,
    targets: Iterable[str] = (),
    require_xcode_tools: bool,
) -> tuple[list[Probe], dict[str, str]]:
    probes = macos_probes(discovery)
    probes.append(
        executable_probe(discovery, "swiftc", "install/select Xcode command-line tools")
    )
    if require_xcode_tools:
        probes.extend(
            (
                executable_probe(
                    discovery, "xcrun", "install/select Xcode command-line tools"
                ),
                executable_probe(discovery, "xcodebuild", "install/select full Xcode"),
            )
        )
    probes.extend(rust_target_probes(discovery, targets))
    return probes, {}


def required_env_probe(discovery: Discovery, variable: str, purpose: str) -> Probe:
    value = discovery.env.get(variable, "").strip()
    return Probe(
        key=f"env:{variable}",
        ok=bool(value),
        detail=value or "not set",
        remedy=None if value else f"set {variable} ({purpose})",
    )


def full_sha_env_probe(discovery: Discovery, variable: str, purpose: str) -> Probe:
    value = discovery.env.get(variable, "").strip()
    ok = re.fullmatch(r"[0-9a-f]{40}", value) is not None
    return Probe(
        key=f"env:{variable}",
        ok=ok,
        detail=value or "not set",
        remedy=None if ok else f"set {variable} to the exact 40-character lowercase commit ({purpose})",
    )


def clean_exact_repository_probe(
    discovery: Discovery,
    expected: str,
    *,
    key: str = "q3-clean-exact-repository",
) -> Probe:
    git = discovery.command_path("git")
    if git is None:
        return Probe(key, False, "git not found", "install Git")
    head_code, head = discovery.capture((str(git), "-C", str(REPO_ROOT), "rev-parse", "HEAD"))
    status_code, status = discovery.capture(
        (str(git), "-C", str(REPO_ROOT), "status", "--porcelain", "--untracked-files=normal")
    )
    ok = head_code == 0 and status_code == 0 and head == expected and not status
    detail = f"head={head or 'unknown'} clean={status_code == 0 and not status} expected={expected or 'unset'}"
    return Probe(
        key,
        ok,
        detail,
        None if ok else "check out the exact integrated SHA in a clean worktree before qualification",
    )


def q1_product_quality_platform_probes(discovery: Discovery) -> list[Probe]:
    apple_silicon = discovery.host_machine.lower() in {"arm64", "aarch64"}
    profiler = discovery.command_path("system_profiler")
    metal_detail = "system_profiler not found"
    metal_ok = False
    if profiler is not None:
        code, output = discovery.capture((str(profiler), "SPDisplaysDataType"))
        metal_detail = (output[:500] if output else f"system_profiler exited {code}")
        metal_ok = code == 0 and "metal" in output.lower()
    return [
        Probe(
            "q1-darwin-host",
            discovery.host_system == "Darwin",
            discovery.host_system,
            None if discovery.host_system == "Darwin" else "run Q1 on macOS",
        ),
        Probe(
            "q1-apple-silicon",
            apple_silicon,
            discovery.host_machine,
            None if apple_silicon else "run Q1 on an Apple Silicon host",
        ),
        Probe(
            "q1-metal-runtime",
            metal_ok,
            metal_detail,
            None
            if metal_ok
            else "select a macOS host whose display adapter reports Metal support",
        ),
    ]


def q1_pinned_puppeteer_probe() -> Probe:
    lock_path = REPO_ROOT / "tests/competitive/playcanvas/package-lock.json"
    installed_path = (
        REPO_ROOT
        / "tests/competitive/playcanvas/node_modules/puppeteer-core/package.json"
    )
    try:
        lock = json.loads(lock_path.read_text(encoding="utf-8"))
        installed = json.loads(installed_path.read_text(encoding="utf-8"))
        locked = lock["packages"]["node_modules/puppeteer-core"]["version"]
        actual = installed["version"]
        ok = isinstance(locked, str) and actual == locked
        detail = f"installed={actual!r} locked={locked!r}"
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, KeyError, TypeError) as error:
        ok = False
        detail = f"cannot verify pinned puppeteer-core: {error}"
    return Probe(
        "q1-pinned-puppeteer-core",
        ok,
        detail,
        None
        if ok
        else "run npm ci --ignore-scripts --prefix tests/competitive/playcanvas",
    )


def q1_input_path(discovery: Discovery, variable: str, default: str) -> pathlib.Path:
    value = discovery.env.get(variable, default)
    path = pathlib.Path(value)
    return path if path.is_absolute() else REPO_ROOT / path


def q3_a065_input_probes() -> list[Probe]:
    matrix_path = REPO_ROOT / "tests/perf/full-quality-matrix-plan-v1.json"
    if not matrix_path.is_file():
        return [
            file_probe(
                "q3-a065-matrix",
                matrix_path,
                "restore the committed full-quality matrix",
            )
        ]
    matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
    ids = {
        "truck-050k",
        "truck-100k",
        "truck-200k",
        "truck-300k",
        "truck-500k",
        "truck-1m",
        "truck-1p5m",
        "truck-2m",
        "truck-full",
    }
    dataset_entries = {
        entry["id"]: entry
        for entry in matrix.get("datasets", [])
        if isinstance(entry, dict) and entry.get("id") in ids
    }
    probes = [
        file_probe(
            f"q3-a065-dataset:{dataset_id}",
            REPO_ROOT / dataset_entries[dataset_id]["local_path"],
            f"install the pinned {dataset_id} input at its canonical matrix path",
        )
        for dataset_id in sorted(ids & set(dataset_entries))
    ]
    probes.extend(
        Probe(
            f"q3-a065-dataset:{dataset_id}",
            False,
            "matrix entry missing",
            f"restore the canonical {dataset_id} matrix entry",
        )
        for dataset_id in sorted(ids - set(dataset_entries))
    )
    trace = next(
        (
            entry
            for entry in matrix.get("traces", [])
            if isinstance(entry, dict)
            and entry.get("id") == "candidate-truck-quality-2view-2412x1080-v1"
        ),
        None,
    )
    if isinstance(trace, dict):
        probes.append(
            file_probe(
                "q3-a065-trace",
                REPO_ROOT / trace["local_path"],
                "restore the canonical A065 Truck trace",
            )
        )
    else:
        probes.append(Probe("q3-a065-trace", False, "matrix entry missing", "restore the canonical A065 Truck trace entry"))
    return probes


def dataset_probe(discovery: Discovery, variable: str, default: str) -> tuple[Probe, pathlib.Path]:
    value = discovery.env.get(variable, default)
    path = pathlib.Path(value)
    if not path.is_absolute():
        path = REPO_ROOT / path
    return (
        file_probe(variable.lower(), path, f"fetch or set {variable} to the intended dataset"),
        path,
    )


def exact_web_build_environment(
    discovery: Discovery,
    probes: list[Probe],
    env: Mapping[str, str],
) -> dict[str, str]:
    """Freeze canonical Web profiles to Exact and reject diagnostic inheritance."""

    requested_profile = discovery.env.get(WEB_WASM_PROFILE_ENV, "")
    diagnostic_output = discovery.env.get(WEB_WASM_OUT_DIR_ENV, "")
    profile_ok = requested_profile in ("", "exact")
    output_ok = diagnostic_output == ""
    probes.extend(
        (
            Probe(
                key=f"exact-web-env:{WEB_WASM_PROFILE_ENV}",
                ok=profile_ok,
                detail=requested_profile or "unset (Exact default)",
                remedy=None
                if profile_ok
                else f"unset {WEB_WASM_PROFILE_ENV}; canonical Web profiles are Exact",
            ),
            Probe(
                key=f"exact-web-env:{WEB_WASM_OUT_DIR_ENV}",
                ok=output_ok,
                detail=diagnostic_output or "unset",
                remedy=None
                if output_ok
                else f"unset {WEB_WASM_OUT_DIR_ENV}; it is diagnostic-only",
            ),
        )
    )
    frozen = dict(env)
    frozen[WEB_WASM_PROFILE_ENV] = "exact"
    frozen[WEB_WASM_OUT_DIR_ENV] = ""
    return frozen


def profile_result(name: str, discovery: Discovery) -> ProfileResult:
    head = discovery.git_head()
    python = discovery.command_path("python3") or pathlib.Path("python3")

    if name == "android-build":
        probes, env = android_environment(discovery)
        commands = (
            Command(
                (
                    "bash",
                    "bindings/android/scripts/build-sample-apk.sh",
                    "tests/datasets/minimal_ascii.ply",
                ),
                env,
            ),
            Command(("bash", "bindings/android/scripts/build-aar.sh"), env),
            Command(("bash", "bindings/android/scripts/run-jni-smoke.sh"), env),
        )
        return ProfileResult(
            name,
            "Android release-native, sample APK, AAR, and host JNI smoke",
            False,
            tuple(probes),
            commands,
        )

    if name == "android-a065":
        probes, env = android_environment(discovery)
        probes.append(
            required_env_probe(
                discovery,
                "GSPLAT_ANDROID_SERIAL",
                "exact adb serial; no device is queried by doctor",
            )
        )
        dataset_check, dataset = dataset_probe(
            discovery,
            "GSPLAT_ANDROID_DATASET",
            "tests/datasets/external/wakufactory_kitune/kitune1.ply",
        )
        probes.append(dataset_check)
        trace_check, trace = dataset_probe(
            discovery,
            "GSPLAT_ANDROID_TRACE",
            "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2412x1080-v1.json",
        )
        probes.append(trace_check)
        serial = discovery.env.get("GSPLAT_ANDROID_SERIAL", "<set-GSPLAT_ANDROID_SERIAL>")
        output = discovery.env.get(
            "GSPLAT_ANDROID_OUTPUT",
            f"target/android-sort-benchmarks/verification-a065-{head}",
        )
        probes.append(fresh_output_probe("GSPLAT_ANDROID_OUTPUT", output))
        command = Command(
            (
                str(python),
                "bindings/android/scripts/collect-android-sort-benchmarks.py",
                "--serial",
                serial,
                "--ply",
                str(dataset),
                "--prepare-apk",
                "--rust-profile",
                "release",
                "--formal-artifact",
                "--backend",
                "cpu",
                "--repetitions",
                "1",
                "--frames",
                "20",
                "--warmup",
                "10",
                "--camera-trace",
                str(trace),
                "--camera-frame-indices",
                "0,1",
                "--geometry-path",
                "packed",
                "--sort-interval",
                "1",
                "--cooldown-seconds",
                "0",
                "--max-thermal-status",
                "0",
                "--output",
                output,
            ),
            env,
        )
        return ProfileResult(
            name,
            "Short formal A065 Packed/CPU ledger and native-Surface PNG "
            "verification; not a performance comparison",
            True,
            tuple(probes),
            (command,),
        )

    if name == "android-a065-q3-simd":
        probes, env = android_environment(discovery)
        probes.extend(
            (
                required_env_probe(
                    discovery,
                    "GSPLAT_ANDROID_SERIAL",
                    "exact adb serial; doctor never queries the device",
                ),
                full_sha_env_probe(
                    discovery,
                    "GSPLAT_Q3_A065_EXPECTED_COMMIT",
                    "the clean integrated Q3 candidate",
                ),
            )
        )
        probes.extend(q3_a065_input_probes())
        serial = discovery.env.get("GSPLAT_ANDROID_SERIAL", "<set-GSPLAT_ANDROID_SERIAL>")
        expected = discovery.env.get(
            "GSPLAT_Q3_A065_EXPECTED_COMMIT",
            "<set-GSPLAT_Q3_A065_EXPECTED_COMMIT>",
        )
        probes.append(clean_exact_repository_probe(discovery, expected))
        output = discovery.env.get(
            "GSPLAT_Q3_A065_OUTPUT",
            f"target/qualification/q3-a065-simd-{head}",
        )
        probes.append(fresh_output_probe("GSPLAT_Q3_A065_OUTPUT", output))
        command = Command(
            (
                str(python),
                "bindings/android/scripts/collect-q3-a065-simd.py",
                "--serial",
                serial,
                "--expected-commit",
                expected,
                "--output",
                output,
            ),
            env,
        )
        return ProfileResult(
            name,
            "One-shot physical A065 Q3 Scalar/Neon staged qualification: "
            "diagnostic point ladder then complete-Truck terminal pairs",
            True,
            tuple(probes),
            (command,),
        )

    if name == "macos-metal":
        probes = macos_probes(discovery)
        command = Command(
            ("cargo", "test", "-p", "gsplat-render-wgpu", "--test", "conformance_sorted_alpha"),
            {"GSPLAT_REQUIRE_GPU_CONFORMANCE": "1"},
        )
        return ProfileResult(
            name,
            "Required hardware-backed macOS Metal SortedAlpha conformance",
            False,
            tuple(probes),
            (command,),
        )

    if name == "q1-product-quality-view000001":
        probes, env = web_environment(discovery)
        env = exact_web_build_environment(discovery, probes, env)
        probes.extend(q1_product_quality_platform_probes(discovery))
        probes.append(q1_pinned_puppeteer_probe())
        probes.append(
            full_sha_env_probe(
                discovery,
                Q1_QUALITY_EXPECTED_COMMIT_ENV,
                "the clean integrated Product Quality candidate",
            )
        )
        expected = discovery.env.get(
            Q1_QUALITY_EXPECTED_COMMIT_ENV,
            f"<set-{Q1_QUALITY_EXPECTED_COMMIT_ENV}>",
        )
        probes.append(
            clean_exact_repository_probe(
                discovery,
                expected,
                key="q1-clean-exact-repository",
            )
        )
        formal = q1_input_path(
            discovery,
            Q1_QUALITY_FORMAL_TRACE_ENV,
            "target/qualification/q1-formal-truck-product-quality-trace-v1",
        )
        evaluation = q1_input_path(
            discovery,
            Q1_QUALITY_EVALUATION_ENV,
            "target/qualification/q1-product-quality-evaluation-authority-v1",
        )
        truck = q1_input_path(
            discovery,
            Q1_QUALITY_TRUCK_ENV,
            "tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
        )
        probes.extend(
            (
                directory_probe(
                    "q1-formal-trace-authority",
                    formal,
                    f"set {Q1_QUALITY_FORMAL_TRACE_ENV} to the retained formal authority",
                ),
                file_probe(
                    "q1-formal-trace-camera",
                    formal / "camera-trace.json",
                    "restore the reviewed formal Truck camera trace",
                ),
                file_probe(
                    "q1-formal-trace-receipt",
                    formal / "receipt.json",
                    "restore the reviewed formal Truck trace receipt",
                ),
                directory_probe(
                    "q1-evaluation-authority",
                    evaluation,
                    f"set {Q1_QUALITY_EVALUATION_ENV} to the retained Evaluation Images authority",
                ),
                file_probe(
                    "q1-evaluation-receipt",
                    evaluation / "authority.json",
                    "restore the reviewed Evaluation Images authority receipt",
                ),
                file_probe(
                    "q1-evaluation-gt-000001",
                    evaluation / "source/gt/000001.png",
                    "restore the official Truck 000001 ground-truth PNG",
                ),
                file_probe(
                    "q1-complete-truck",
                    truck,
                    f"set {Q1_QUALITY_TRUCK_ENV} to the complete pinned Truck PLY",
                ),
            )
        )
        output = discovery.env.get(
            Q1_QUALITY_OUTPUT_ENV,
            f"target/qualification/q1-product-quality-view000001-{head}",
        )
        probes.append(fresh_output_probe(Q1_QUALITY_OUTPUT_ENV, output))
        command = Command(
            (
                str(python),
                "tests/perf/collect-q1-product-quality-view000001.py",
                "--expected-commit",
                expected,
                "--formal-trace-authority",
                str(formal),
                "--evaluation-authority",
                str(evaluation),
                "--dataset",
                str(truck),
                "--output",
                output,
            ),
            env,
        )
        return ProfileResult(
            name,
            "One-shot Q1 formal Truck view 000001 native plus PlayCanvas "
            "quality-only capture and offline admission; no performance evidence",
            False,
            tuple(probes),
            (command,),
        )

    if name == "web-webgpu":
        probes, env = web_environment(discovery)
        env = exact_web_build_environment(discovery, probes, env)
        artifact = discovery.env.get(
            "GSPLAT_ARTIFACT_DIR",
            f"target/benchmarks/m4-webgpu-smoke-{head}",
        )
        probes.append(fresh_output_probe("GSPLAT_ARTIFACT_DIR", artifact))
        collector_env = dict(env)
        collector_env.update({"GSPLAT_M4_SMOKE": "1", "GSPLAT_ARTIFACT_DIR": artifact})
        commands = (
            Command(("bash", "packages/web/scripts/build-wasm.sh"), env),
            Command(
                ("node", "examples/web/scripts/collect-web-benchmark-artifact.mjs"),
                collector_env,
            ),
        )
        return ProfileResult(
            name,
            "Real Chrome WebGPU/WASM exact functional smoke",
            False,
            tuple(probes),
            commands,
        )

    if name in (
        "web-webgpu-truck-1080p",
        "web-webgpu-truck-fixed-gpu-preproject-compact",
    ):
        probes, env = web_environment(discovery)
        env = exact_web_build_environment(discovery, probes, env)
        dataset = REPO_ROOT / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"
        trace = (
            REPO_ROOT
            / "tests/perf/trace/fixtures/quality/"
            "candidate-truck-quality-1920x1080-v1.json"
        )
        probes.extend(
            (
                file_probe(
                    "truck-dataset",
                    dataset,
                    "install the pinned Truck PLY at the canonical path; "
                    "the profile does not fetch or copy the 630 MB asset",
                ),
                file_probe(
                    "truck-trace",
                    trace,
                    "restore the canonical two-frame Truck 1920x1080 trace",
                ),
            )
        )
        fixed_cell = name == "web-webgpu-truck-fixed-gpu-preproject-compact"
        output_env = (
            "GSPLAT_WEB_TRUCK_FIXED_GPU_COMPACT_OUTPUT"
            if fixed_cell else "GSPLAT_WEB_TRUCK_OUTPUT"
        )
        output = discovery.env.get(
            output_env,
            (
                f"target/qualification/q1-webgpu-truck-fixed-gpu-preproject-compact-{head}"
                if fixed_cell else f"target/qualification/q1-webgpu-truck-1080p-{head}"
            ),
        )
        probes.append(fresh_output_probe(output_env, output))
        control_artifact = str(pathlib.Path(output) / "control-current-stats")
        throughput_artifact = str(pathlib.Path(output) / "throughput-terminal-queue")
        control_manifest = str(pathlib.Path(control_artifact) / "manifest.json")
        suite = str(pathlib.Path(output) / "suite.json")
        common_collector_env = dict(env)
        common_collector_env.update(
            {
                "GSPLAT_PHASE_E_QUALIFICATION": (
                    "truck-quality-1080p-fixed-gpu-preproject-compact-v1"
                    if fixed_cell else "truck-quality-1080p-v1"
                ),
                "GSPLAT_DATASET": "truck",
                "GSPLAT_GEOMETRY_PATH": "packed",
                "GSPLAT_ORDER_BACKEND": "gpu" if fixed_cell else "adaptive",
                "GSPLAT_PROJECTED_POLICY": "compact" if fixed_cell else "adaptive",
                "GSPLAT_GPU_ORDER_PRODUCER": "",
                "GSPLAT_SORT_INTERVAL": "1",
                "GSPLAT_BENCHMARK_SYNC": "0",
                "GSPLAT_M4_SMOKE": "0",
                "GSPLAT_CAMERA_TRACE_URL": (
                    "/tests/perf/trace/fixtures/quality/"
                    "candidate-truck-quality-1920x1080-v1.json"
                ),
                "GSPLAT_CAMERA_TRACE_SEQUENCE": "1",
                "GSPLAT_CAMERA_FRAME_INDICES": "0,1",
                "GSPLAT_CAMERA_TRACE_LOOPS": "1",
                "GSPLAT_CAMERA_FRAME": "",
                "GSPLAT_BENCHMARK_WARMUP_FRAMES": "20",
                "GSPLAT_BENCHMARK_FRAMES": "80",
                "GSPLAT_FULL_QUALITY_SUITE": suite,
            }
        )
        control_env = dict(common_collector_env)
        control_env.update(
            {
                "GSPLAT_TRUCK_QUALIFICATION_STAGE": "control",
                "GSPLAT_BENCHMARK_WINDOW_MODE": "current_stats_evidence_window",
                "GSPLAT_ORDER_COMPLETION_PROTOCOL": "isolated_terminal",
                "GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT": "",
                "GSPLAT_ARTIFACT_DIR": control_artifact,
            }
        )
        throughput_env = dict(common_collector_env)
        throughput_env.update(
            {
                "GSPLAT_TRUCK_QUALIFICATION_STAGE": "throughput",
                "GSPLAT_BENCHMARK_WINDOW_MODE": "terminal_queue_throughput_window",
                "GSPLAT_ORDER_COMPLETION_PROTOCOL": "sustained_window",
                "GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT": control_manifest,
                "GSPLAT_ARTIFACT_DIR": throughput_artifact,
            }
        )
        commands = (
            Command(("bash", "packages/web/scripts/build-wasm.sh"), env),
            Command(
                ("node", "examples/web/scripts/collect-web-benchmark-artifact.mjs"),
                control_env,
            ),
            Command(
                ("node", "examples/web/scripts/collect-web-benchmark-artifact.mjs"),
                throughput_env,
            ),
        )
        return ProfileResult(
            name,
            "Formal 1920x1080 Truck Packed Exact WebGPU two-stage prerequisite: "
            "validated untimed current-stats control, then bound terminal throughput; "
            "not a product performance conclusion",
            False,
            tuple(probes),
            commands,
        )

    if name == "apple-host":
        probes, env = apple_environment(discovery, require_xcode_tools=False)
        command = Command(("bash", "bindings/apple/scripts/run-swift-smoke.sh"), env)
        return ProfileResult(
            name,
            "Host Swift -> C ABI -> Rust smoke",
            False,
            tuple(probes),
            (command,),
        )

    if name == "apple-xcframework":
        probes, env = apple_environment(
            discovery,
            targets=APPLE_XCFRAMEWORK_TARGETS,
            require_xcode_tools=True,
        )
        command = Command(("bash", "bindings/apple/scripts/build-xcframework.sh"), env)
        return ProfileResult(
            name,
            "Local device+simulator XCFramework packaging",
            False,
            tuple(probes),
            (command,),
        )

    if name == "ios-simulator":
        probes, env = apple_environment(
            discovery,
            targets=("aarch64-apple-ios-sim",),
            require_xcode_tools=True,
        )
        probes.append(
            required_env_probe(
                discovery,
                "IOS_SIMULATOR_ID",
                "explicit simulator UUID; doctor does not call simctl",
            )
        )
        dataset_check, dataset = dataset_probe(
            discovery,
            "GSPLAT_IOS_DATASET",
            "tests/datasets/external/wakufactory_kitune/kitune1.ply",
        )
        trace_check, trace = dataset_probe(
            discovery,
            "GSPLAT_IOS_TRACE",
            "tests/perf/trace/fixtures/quality/candidate-kitsune-quality-2622x1206-v1.json",
        )
        probes.extend((dataset_check, trace_check))
        simulator = discovery.env.get("IOS_SIMULATOR_ID", "<set-IOS_SIMULATOR_ID>")
        output = discovery.env.get(
            "GSPLAT_IOS_OUTPUT",
            f"target/ios-sim-benchmarks/verification-{head}",
        )
        command = Command(
            (
                str(python),
                "bindings/apple/scripts/collect-ios-sim-benchmark.py",
                output,
                "--simulator-id",
                simulator,
                "--dataset",
                str(dataset),
                "--trace",
                str(trace),
            ),
            env,
        )
        return ProfileResult(
            name,
            "Attested iOS simulator Surface artifact",
            True,
            tuple(probes),
            (command,),
        )

    raise ValueError(f"unknown profile: {name}")


PROFILES = (
    "android-build",
    "android-a065",
    "android-a065-q3-simd",
    "macos-metal",
    "q1-product-quality-view000001",
    "web-webgpu",
    "web-webgpu-truck-1080p",
    "web-webgpu-truck-fixed-gpu-preproject-compact",
    "apple-host",
    "apple-xcframework",
    "ios-simulator",
)
DEFAULT_DOCTOR_PROFILES = (
    "android-build",
    "macos-metal",
    "web-webgpu",
    "apple-host",
    "apple-xcframework",
)


def print_human(results: Iterable[ProfileResult], *, include_commands: bool) -> None:
    for result in results:
        status = "READY" if result.ready else "BLOCKED"
        print(f"[{status}] {result.name}: {result.description}")
        for probe in result.probes:
            marker = "ok" if probe.ok else "missing"
            print(f"  [{marker}] {probe.key}: {probe.detail}")
            if probe.remedy:
                print(f"    remedy: {probe.remedy}")
        if include_commands:
            for index, command in enumerate(result.commands, start=1):
                print(f"  command[{index}]: {command.display()}")


def run_profile(result: ProfileResult, *, allow_device: bool) -> int:
    if not result.ready:
        print_human((result,), include_commands=True)
        print("refusing to run: profile prerequisites are incomplete", file=sys.stderr)
        return 2
    if result.touches_device and not allow_device:
        print(
            f"refusing to run device profile {result.name!r} without --allow-device; "
            "use the command subcommand for a side-effect-free preview",
            file=sys.stderr,
        )
        return 2
    for command in result.commands:
        env = os.environ.copy()
        env.update(command.env)
        print(f"+ {command.display()}", flush=True)
        completed = subprocess.run(command.argv, cwd=REPO_ROOT, env=env, check=False)
        if completed.returncode != 0:
            return completed.returncode
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="action", required=True)

    doctor = subparsers.add_parser("doctor", help="read-only prerequisite report")
    doctor.add_argument("--profile", action="append", choices=PROFILES)
    doctor.add_argument("--json", action="store_true")

    command = subparsers.add_parser("command", help="print a profile command without running it")
    command.add_argument("profile", choices=PROFILES)
    command.add_argument("--json", action="store_true")

    run = subparsers.add_parser("run", help="run a ready profile through existing repo scripts")
    run.add_argument("profile", choices=PROFILES)
    run.add_argument("--allow-device", action="store_true")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    discovery = Discovery()
    if args.action == "doctor":
        names = tuple(args.profile or DEFAULT_DOCTOR_PROFILES)
        results = tuple(profile_result(name, discovery) for name in names)
        if args.json:
            print(json.dumps({"profiles": [result.to_json() for result in results]}, indent=2))
        else:
            print_human(results, include_commands=True)
        return 0 if all(result.ready for result in results) else 2

    result = profile_result(args.profile, discovery)
    if args.action == "command":
        if args.json:
            print(json.dumps(result.to_json(), indent=2))
        else:
            print_human((result,), include_commands=True)
        return 0 if result.ready else 2
    return run_profile(result, allow_device=args.allow_device)


if __name__ == "__main__":
    raise SystemExit(main())
