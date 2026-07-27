from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("verification_bootstrap.py")
SPEC = importlib.util.spec_from_file_location("verification_bootstrap", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
BOOTSTRAP = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BOOTSTRAP
SPEC.loader.exec_module(BOOTSTRAP)


class FakeHost:
    def __init__(self, root: pathlib.Path) -> None:
        self.root = root
        self.calls: list[tuple[str, ...]] = []
        self.paths: dict[str, pathlib.Path] = {}

    def add_executable(self, name: str) -> pathlib.Path:
        path = self.root / "bin" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("", encoding="utf-8")
        path.chmod(0o755)
        self.paths[name] = path
        return path

    def which(self, name: str) -> str | None:
        path = self.paths.get(name)
        return str(path) if path is not None else None

    def capture(self, argv: tuple[str, ...]) -> tuple[int, str]:
        self.calls.append(tuple(argv))
        binary = pathlib.Path(argv[0]).name
        if binary == "rustup" and argv[1:] == ("target", "list", "--installed"):
            return 0, "\n".join(
                (
                    BOOTSTRAP.ANDROID_RUST_TARGET,
                    BOOTSTRAP.WEB_RUST_TARGET,
                    *BOOTSTRAP.APPLE_XCFRAMEWORK_TARGETS,
                )
            )
        if binary == "java" and argv[1:] == ("-version",):
            return 0, 'openjdk version "21.0.11" 2026-04-21'
        if binary == "git":
            if argv[-3:] == ("rev-parse", "--short=12", "HEAD"):
                return 0, "048344feb8fe"
            if argv[-2:] == ("rev-parse", "HEAD"):
                return 0, "1" * 40
            if "status" in argv:
                return 0, ""
            return 1, "unsupported fake git command"
        if binary == "wasm-bindgen":
            return 0, f"wasm-bindgen {BOOTSTRAP.read_locked_wasm_bindgen_version()}"
        return 1, "unsupported fake command"


def make_android_sdk(root: pathlib.Path) -> pathlib.Path:
    sdk = root / "android-sdk"
    (sdk / "ndk" / BOOTSTRAP.ANDROID_NDK_VERSION).mkdir(parents=True)
    (sdk / "platforms" / BOOTSTRAP.ANDROID_PLATFORM).mkdir(parents=True)
    (sdk / "build-tools" / BOOTSTRAP.ANDROID_BUILD_TOOLS).mkdir(parents=True)
    adb = sdk / "platform-tools" / "adb"
    adb.parent.mkdir(parents=True)
    adb.write_text("", encoding="utf-8")
    return sdk


def make_java_home(root: pathlib.Path) -> pathlib.Path:
    java_home = root / "jdk-21"
    java = java_home / "bin" / "java"
    java.parent.mkdir(parents=True)
    java.write_text("", encoding="utf-8")
    include = java_home / "include" / "jni.h"
    include.parent.mkdir(parents=True)
    include.write_text("", encoding="utf-8")
    return java_home


class VerificationBootstrapTests(unittest.TestCase):
    @staticmethod
    def install_fake_web_build(root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
        source = pathlib.Path(__file__).parents[1] / "packages/web/scripts/build-wasm.sh"
        script = root / "packages/web/scripts/build-wasm.sh"
        script.parent.mkdir(parents=True)
        shutil.copy2(source, script)
        script.chmod(0o755)

        log = root / "fake-cargo.log"
        cargo = root / "bin/cargo"
        cargo.parent.mkdir(parents=True)
        cargo.write_text(
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            f"printf '%s\\n' \"$*\" > {str(log)!r}\n"
            f"mkdir -p {str(root / 'target/wasm32-unknown-unknown/release')!r}\n"
            f": > {str(root / 'target/wasm32-unknown-unknown/release/gsplat_web.wasm')!r}\n",
            encoding="utf-8",
        )
        cargo.chmod(0o755)

        bindgen = root / "bin/wasm-bindgen"
        bindgen.write_text(
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            "out=''\n"
            "while (($#)); do\n"
            "  if [[ \"$1\" == '--out-dir' ]]; then out=\"$2\"; shift 2; else shift; fi\n"
            "done\n"
            "[[ -n \"$out\" ]]\n"
            "mkdir -p \"$out\"\n"
            ": > \"$out/gsplat_web.js\"\n",
            encoding="utf-8",
        )
        bindgen.chmod(0o755)
        return script, log

    def test_web_wasm_build_defaults_to_exact_and_candidate_is_isolated(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            script, log = self.install_fake_web_build(root)
            default_output = root / "examples/web/pkg"
            default_output.mkdir(parents=True)
            (default_output / "stale.txt").write_text("stale", encoding="utf-8")
            environment = {
                **os.environ,
                "PATH": f"{root / 'bin'}:{os.environ.get('PATH', '')}",
                "WASM_BINDGEN_BIN": str(root / "bin/wasm-bindgen"),
            }

            exact = subprocess.run(
                ("bash", str(script)), env=environment, text=True, capture_output=True
            )
            self.assertEqual(exact.returncode, 0, exact.stderr)
            self.assertNotIn("--features", log.read_text(encoding="utf-8"))
            self.assertFalse((default_output / "stale.txt").exists())
            self.assertTrue((default_output / "gsplat_web.js").is_file())

            marker = default_output / "exact-marker.txt"
            marker.write_text("keep", encoding="utf-8")
            candidate_output = root / "target/diagnostic/candidate20/pkg"
            candidate = subprocess.run(
                ("bash", str(script)),
                env={
                    **environment,
                    BOOTSTRAP.WEB_WASM_PROFILE_ENV: "candidate20",
                    BOOTSTRAP.WEB_WASM_OUT_DIR_ENV: str(candidate_output),
                },
                text=True,
                capture_output=True,
            )
            self.assertEqual(candidate.returncode, 0, candidate.stderr)
            self.assertIn(
                "--features diagnostic-web-depth-key-candidate20",
                log.read_text(encoding="utf-8"),
            )
            self.assertTrue((candidate_output / "gsplat_web.js").is_file())
            self.assertEqual(marker.read_text(encoding="utf-8"), "keep")

    def test_web_wasm_exact_removes_symlink_leaf_without_touching_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            script, log = self.install_fake_web_build(root)
            external = root / "external-exact-output"
            external.mkdir()
            sentinel = external / "sentinel.txt"
            sentinel.write_text("preserve", encoding="utf-8")
            default_output = root / "examples/web/pkg"
            default_output.parent.mkdir(parents=True)
            default_output.symlink_to(external, target_is_directory=True)
            environment = {
                **os.environ,
                "PATH": f"{root / 'bin'}:{os.environ.get('PATH', '')}",
                "WASM_BINDGEN_BIN": str(root / "bin/wasm-bindgen"),
            }

            exact = subprocess.run(
                ("bash", str(script)), env=environment, text=True, capture_output=True
            )

            self.assertEqual(exact.returncode, 0, exact.stderr)
            self.assertNotIn("--features", log.read_text(encoding="utf-8"))
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")
            self.assertFalse(default_output.is_symlink())
            self.assertTrue((default_output / "gsplat_web.js").is_file())

    def test_web_wasm_diagnostic_selection_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            script, log = self.install_fake_web_build(root)
            environment = {
                **os.environ,
                "PATH": f"{root / 'bin'}:{os.environ.get('PATH', '')}",
                "WASM_BINDGEN_BIN": str(root / "bin/wasm-bindgen"),
            }
            for extra_env, message in (
                ({BOOTSTRAP.WEB_WASM_PROFILE_ENV: "unknown"}, "expected exact or candidate20"),
                ({BOOTSTRAP.WEB_WASM_PROFILE_ENV: "candidate20"}, "requires an explicit fresh"),
            ):
                failed = subprocess.run(
                    ("bash", str(script)),
                    env={**environment, **extra_env},
                    text=True,
                    capture_output=True,
                )
                self.assertEqual(failed.returncode, 2)
                self.assertIn(message, failed.stderr)
                self.assertFalse(log.exists())

            occupied = root / "occupied"
            occupied.mkdir()
            failed = subprocess.run(
                ("bash", str(script)),
                env={
                    **environment,
                    BOOTSTRAP.WEB_WASM_PROFILE_ENV: "candidate20",
                    BOOTSTRAP.WEB_WASM_OUT_DIR_ENV: str(occupied),
                },
                text=True,
                capture_output=True,
            )
            self.assertEqual(failed.returncode, 2)
            self.assertIn("must be fresh", failed.stderr)
            self.assertFalse(log.exists())

    def test_web_wasm_candidate_rejects_normalized_default_output_aliases(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            script, log = self.install_fake_web_build(root)
            default_parent = root / "examples/web"
            default_output = default_parent / "pkg"
            default_output.mkdir(parents=True)
            marker = default_output / "exact-marker.txt"
            marker.write_text("keep", encoding="utf-8")
            alias_parent = root / "default-output-alias"
            alias_parent.symlink_to(default_parent, target_is_directory=True)
            environment = {
                **os.environ,
                "PATH": f"{root / 'bin'}:{os.environ.get('PATH', '')}",
                "WASM_BINDGEN_BIN": str(root / "bin/wasm-bindgen"),
                BOOTSTRAP.WEB_WASM_PROFILE_ENV: "candidate20",
            }

            for requested in (
                "examples/web//pkg",
                str(alias_parent / "pkg/candidate20"),
            ):
                failed = subprocess.run(
                    ("bash", str(script)),
                    env={
                        **environment,
                        BOOTSTRAP.WEB_WASM_OUT_DIR_ENV: requested,
                    },
                    text=True,
                    capture_output=True,
                )
                self.assertEqual(failed.returncode, 2, failed.stderr)
                self.assertIn("independent from examples/web/pkg", failed.stderr)
                self.assertFalse(log.exists())
                self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
                self.assertFalse((default_output / "candidate20").exists())

    def test_profile_discovery_never_invokes_device_or_browser_tools(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            for command in (
                "bash",
                "brew",
                "cargo",
                "git",
                "node",
                "npm",
                "python3",
                "rustup",
                "swiftc",
                "wasm-bindgen",
                "xcodebuild",
                "xcrun",
            ):
                host.add_executable(command)
            discovery = BOOTSTRAP.Discovery(
                env={},
                home=root,
                which=host.which,
                capture=host.capture,
                host_system="Darwin",
            )

            for profile in BOOTSTRAP.PROFILES:
                BOOTSTRAP.profile_result(profile, discovery)

            invoked = {pathlib.Path(call[0]).name for call in host.calls}
            self.assertTrue(invoked.isdisjoint({"adb", "simctl", "node", "chrome"}))

    def test_homebrew_cask_sdk_is_derived_from_the_brew_prefix(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            brew = host.add_executable("brew")
            prefix = root / "homebrew"
            sdk = prefix / "share" / "android-commandlinetools"
            sdk.mkdir(parents=True)

            def capture(argv: tuple[str, ...]) -> tuple[int, str]:
                host.calls.append(tuple(argv))
                if pathlib.Path(argv[0]).name == brew.name and argv[1:] == ("--prefix",):
                    return 0, str(prefix)
                return 1, "not a formula"

            discovery = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=capture, host_system="Darwin"
            )
            actual, source = discovery.android_sdk()
            self.assertEqual(actual, sdk)
            self.assertEqual(source, "Homebrew share directory")

    def test_android_sdk_automatic_discovery_has_documented_precedence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            brew = host.add_executable("brew")

            standard_sdk = root / "Library" / "Android" / "sdk"
            standard_sdk.mkdir(parents=True)
            brew_prefix = root / "homebrew"
            brew_share_sdk = brew_prefix / "share" / "android-commandlinetools"
            brew_share_sdk.mkdir(parents=True)
            brew_formula_sdk = root / "homebrew-formula"
            brew_formula_sdk.mkdir()
            adb_sdk = root / "adb-sdk"
            adb = adb_sdk / "platform-tools" / "adb"
            adb.parent.mkdir(parents=True)
            adb.write_text("", encoding="utf-8")
            host.paths["adb"] = adb

            def capture(argv: tuple[str, ...]) -> tuple[int, str]:
                host.calls.append(tuple(argv))
                if pathlib.Path(argv[0]).name != brew.name:
                    return 1, "unsupported fake command"
                if argv[1:] == ("--prefix",):
                    return 0, str(brew_prefix)
                if argv[1:] == ("--prefix", "android-commandlinetools"):
                    return 0, str(brew_formula_sdk)
                return 1, "unsupported brew query"

            discovery = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=capture, host_system="Darwin"
            )

            self.assertEqual(
                discovery.android_sdk(),
                (standard_sdk, "standard macOS SDK location"),
            )
            standard_sdk.rename(root / "standard-sdk-disabled")
            self.assertEqual(
                discovery.android_sdk(),
                (brew_share_sdk, "Homebrew share directory"),
            )
            brew_share_sdk.rename(root / "homebrew-share-sdk-disabled")
            self.assertEqual(
                discovery.android_sdk(),
                (brew_formula_sdk, "Homebrew android-commandlinetools formula"),
            )
            brew_formula_sdk.rename(root / "homebrew-formula-sdk-disabled")
            self.assertEqual(discovery.android_sdk(), (adb_sdk.resolve(), "adb on PATH"))

    def test_explicit_android_roots_are_exported_and_do_not_query_device(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            for command in ("bash", "cargo", "git", "python3", "rustup"):
                host.add_executable(command)
            sdk = make_android_sdk(root)
            java_home = make_java_home(root)
            dataset = root / "scene.ply"
            dataset.write_bytes(b"ply\n")
            trace = root / "trace.json"
            trace.write_text("{}", encoding="utf-8")
            discovery = BOOTSTRAP.Discovery(
                env={
                    "ANDROID_SDK_ROOT": str(sdk),
                    "JAVA_HOME": str(java_home),
                    "GSPLAT_ANDROID_SERIAL": "033ed212",
                    "GSPLAT_ANDROID_DATASET": str(dataset),
                    "GSPLAT_ANDROID_TRACE": str(trace),
                },
                home=root,
                which=host.which,
                capture=host.capture,
                host_system="Darwin",
            )

            result = BOOTSTRAP.profile_result("android-a065", discovery)

            self.assertTrue(result.ready)
            self.assertEqual(result.commands[0].env["ANDROID_SDK_ROOT"], str(sdk))
            self.assertEqual(result.commands[0].env["JAVA_HOME"], str(java_home))
            self.assertIn("--serial", result.commands[0].argv)
            self.assertTrue(
                any(
                    probe.key == "fresh-output:GSPLAT_ANDROID_OUTPUT"
                    for probe in result.probes
                )
            )
            self.assertFalse(
                any(pathlib.Path(call[0]).name in {"adb", "simctl"} for call in host.calls)
            )

    def test_locked_wasm_bindgen_version_is_required(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            host.add_executable("wasm-bindgen")
            discovery = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=host.capture
            )
            self.assertTrue(BOOTSTRAP.wasm_bindgen_probe(discovery).ok)

            def wrong_version(argv: tuple[str, ...]) -> tuple[int, str]:
                return 0, "wasm-bindgen 0.2.1"

            mismatch = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=wrong_version
            )
            probe = BOOTSTRAP.wasm_bindgen_probe(mismatch)
            self.assertFalse(probe.ok)
            self.assertIn(BOOTSTRAP.read_locked_wasm_bindgen_version(), probe.detail)

    def test_q3_a065_profile_is_read_only_and_emits_one_canonical_command(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            for command in ("bash", "cargo", "git", "python3", "rustup"):
                host.add_executable(command)
            sdk = make_android_sdk(root)
            java_home = make_java_home(root)
            dataset_ids = (
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
            datasets = []
            for dataset_id in dataset_ids:
                path = root / "fixtures" / f"{dataset_id}.ply"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"ply\n")
                datasets.append({"id": dataset_id, "local_path": str(path.relative_to(root))})
            trace = root / "fixtures" / "trace.json"
            trace.write_text("{}", encoding="utf-8")
            matrix = root / "tests/perf/full-quality-matrix-plan-v1.json"
            matrix.parent.mkdir(parents=True)
            matrix.write_text(
                json.dumps(
                    {
                        "datasets": datasets,
                        "traces": [
                            {
                                "id": "candidate-truck-quality-2view-2412x1080-v1",
                                "local_path": str(trace.relative_to(root)),
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            expected = "1" * 40
            discovery = BOOTSTRAP.Discovery(
                env={
                    "ANDROID_SDK_ROOT": str(sdk),
                    "JAVA_HOME": str(java_home),
                    "GSPLAT_ANDROID_SERIAL": "033ed212",
                    "GSPLAT_Q3_A065_EXPECTED_COMMIT": expected,
                    "GSPLAT_Q3_A065_OUTPUT": str(root / "fresh-output"),
                },
                home=root,
                which=host.which,
                capture=host.capture,
                host_system="Darwin",
            )
            with mock.patch.object(BOOTSTRAP, "REPO_ROOT", root):
                result = BOOTSTRAP.profile_result("android-a065-q3-simd", discovery)

            self.assertTrue(result.ready)
            self.assertTrue(result.touches_device)
            self.assertEqual(len(result.commands), 1)
            command = result.commands[0].argv
            self.assertIn("bindings/android/scripts/collect-q3-a065-simd.py", command)
            self.assertEqual(command[command.index("--expected-commit") + 1], expected)
            self.assertFalse(
                any(pathlib.Path(call[0]).name in {"adb", "simctl"} for call in host.calls)
            )

    def test_wasm_bindgen_falls_back_to_explicit_cargo_home(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            binary = root / "custom-cargo" / "bin" / "wasm-bindgen"
            binary.parent.mkdir(parents=True)
            binary.write_text("", encoding="utf-8")
            binary.chmod(0o755)
            discovery = BOOTSTRAP.Discovery(
                env={"CARGO_HOME": str(root / "custom-cargo")},
                home=root,
                which=host.which,
                capture=host.capture,
            )

            probe = BOOTSTRAP.wasm_bindgen_probe(discovery)

            self.assertTrue(probe.ok)
            self.assertIn(str(binary), probe.detail)

    def test_wasm_bindgen_falls_back_to_default_cargo_home(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            binary = root / ".cargo" / "bin" / "wasm-bindgen"
            binary.parent.mkdir(parents=True)
            binary.write_text("", encoding="utf-8")
            binary.chmod(0o755)
            discovery = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=host.capture
            )

            probe = BOOTSTRAP.wasm_bindgen_probe(discovery)

            self.assertTrue(probe.ok)
            self.assertIn(str(binary), probe.detail)

    def test_wrong_wasm_bindgen_version_in_cargo_home_remains_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            binary = root / ".cargo" / "bin" / "wasm-bindgen"
            binary.parent.mkdir(parents=True)
            binary.write_text("", encoding="utf-8")
            binary.chmod(0o755)

            def wrong_version(argv: tuple[str, ...]) -> tuple[int, str]:
                host.calls.append(tuple(argv))
                return 0, "wasm-bindgen 0.2.1"

            discovery = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=wrong_version
            )

            probe = BOOTSTRAP.wasm_bindgen_probe(discovery)

            self.assertFalse(probe.ok)
            self.assertIn("version=0.2.1", probe.detail)
            self.assertIn(BOOTSTRAP.read_locked_wasm_bindgen_version(), probe.detail)

    def test_path_wasm_bindgen_takes_priority_over_cargo_home(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            path_binary = host.add_executable("wasm-bindgen")
            cargo_binary = root / ".cargo" / "bin" / "wasm-bindgen"
            cargo_binary.parent.mkdir(parents=True)
            cargo_binary.write_text("", encoding="utf-8")
            cargo_binary.chmod(0o755)
            discovery = BOOTSTRAP.Discovery(
                env={}, home=root, which=host.which, capture=host.capture
            )

            probe = BOOTSTRAP.wasm_bindgen_probe(discovery)

            self.assertTrue(probe.ok)
            self.assertIn(str(path_binary.resolve()), probe.detail)
            self.assertEqual(host.calls[-1][0], str(path_binary.resolve()))

    def test_device_profile_requires_explicit_run_permission(self) -> None:
        result = BOOTSTRAP.ProfileResult(
            name="device",
            description="test",
            touches_device=True,
            probes=(BOOTSTRAP.Probe("ready", True, "ready"),),
            commands=(BOOTSTRAP.Command(("true",)),),
        )
        with mock.patch.object(BOOTSTRAP.subprocess, "run") as run:
            self.assertEqual(BOOTSTRAP.run_profile(result, allow_device=False), 2)
            run.assert_not_called()

    def test_existing_artifact_destination_blocks_before_execution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "retained-run"
            output.mkdir()
            probe = BOOTSTRAP.fresh_output_probe("GSPLAT_ARTIFACT_DIR", str(output))
            result = BOOTSTRAP.ProfileResult(
                name="web-webgpu",
                description="test",
                touches_device=False,
                probes=(probe,),
                commands=(BOOTSTRAP.Command(("must-not-run",)),),
            )

            self.assertFalse(probe.ok)
            self.assertIn("preserve the existing artifact", probe.remedy or "")
            with mock.patch.object(BOOTSTRAP.subprocess, "run") as run:
                self.assertEqual(BOOTSTRAP.run_profile(result, allow_device=False), 2)
                run.assert_not_called()

    def test_web_profile_checks_a_fresh_artifact_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            host = FakeHost(root)
            discovery = BOOTSTRAP.Discovery(
                env={"GSPLAT_ARTIFACT_DIR": str(root / "new-web-run")},
                home=root,
                which=host.which,
                capture=host.capture,
            )
            with mock.patch.object(
                BOOTSTRAP,
                "web_environment",
                return_value=([BOOTSTRAP.Probe("web", True, "ready")], {}),
            ):
                result = BOOTSTRAP.profile_result("web-webgpu", discovery)

            probe = next(
                probe
                for probe in result.probes
                if probe.key == "fresh-output:GSPLAT_ARTIFACT_DIR"
            )
            self.assertTrue(probe.ok)
            self.assertIn("new-web-run", probe.detail)

    def test_truck_web_profile_freezes_formal_collector_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            dataset = root / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"
            trace = (
                root
                / "tests/perf/trace/fixtures/quality/"
                "candidate-truck-quality-1920x1080-v1.json"
            )
            dataset.parent.mkdir(parents=True)
            dataset.write_text("test fixture", encoding="utf-8")
            trace.parent.mkdir(parents=True)
            trace.write_text("{}", encoding="utf-8")
            output = root / "fresh-truck-run"
            discovery = BOOTSTRAP.Discovery(
                env={"GSPLAT_WEB_TRUCK_OUTPUT": str(output)},
                home=root,
                which=FakeHost(root).which,
                capture=FakeHost(root).capture,
            )
            with (
                mock.patch.object(BOOTSTRAP, "REPO_ROOT", root),
                mock.patch.object(
                    BOOTSTRAP,
                    "web_environment",
                    return_value=([BOOTSTRAP.Probe("web", True, "ready")], {}),
                ),
            ):
                result = BOOTSTRAP.profile_result("web-webgpu-truck-1080p", discovery)

            self.assertTrue(result.ready)
            self.assertEqual(len(result.commands), 3)
            self.assertEqual(
                result.commands[0].env[BOOTSTRAP.WEB_WASM_PROFILE_ENV], "exact"
            )
            self.assertEqual(
                result.commands[0].env[BOOTSTRAP.WEB_WASM_OUT_DIR_ENV], ""
            )
            control = result.commands[1]
            throughput = result.commands[2]
            self.assertEqual(
                control.argv,
                ("node", "examples/web/scripts/collect-web-benchmark-artifact.mjs"),
            )
            self.assertEqual(throughput.argv, control.argv)
            for collector in (control, throughput):
                self.assertEqual(
                    collector.env["GSPLAT_PHASE_E_QUALIFICATION"],
                    "truck-quality-1080p-v1",
                )
                self.assertEqual(collector.env["GSPLAT_DATASET"], "truck")
                self.assertEqual(collector.env["GSPLAT_GEOMETRY_PATH"], "packed")
                self.assertEqual(collector.env["GSPLAT_ORDER_BACKEND"], "adaptive")
                self.assertEqual(collector.env["GSPLAT_PROJECTED_POLICY"], "adaptive")
                self.assertEqual(collector.env["GSPLAT_CAMERA_FRAME_INDICES"], "0,1")
                self.assertEqual(collector.env["GSPLAT_BENCHMARK_WARMUP_FRAMES"], "20")
                self.assertEqual(collector.env["GSPLAT_BENCHMARK_FRAMES"], "80")
                self.assertEqual(collector.env["GSPLAT_GPU_ORDER_PRODUCER"], "")
                self.assertEqual(collector.env["GSPLAT_BENCHMARK_SYNC"], "0")
                self.assertEqual(collector.env["GSPLAT_M4_SMOKE"], "0")
                self.assertEqual(
                    collector.env["GSPLAT_FULL_QUALITY_SUITE"],
                    str(output / "suite.json"),
                )
            self.assertEqual(control.env["GSPLAT_TRUCK_QUALIFICATION_STAGE"], "control")
            self.assertEqual(
                control.env["GSPLAT_BENCHMARK_WINDOW_MODE"],
                "current_stats_evidence_window",
            )
            self.assertEqual(control.env["GSPLAT_ORDER_COMPLETION_PROTOCOL"], "isolated_terminal")
            self.assertEqual(control.env["GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT"], "")
            self.assertEqual(
                control.env["GSPLAT_ARTIFACT_DIR"],
                str(output / "control-current-stats"),
            )
            self.assertEqual(throughput.env["GSPLAT_TRUCK_QUALIFICATION_STAGE"], "throughput")
            self.assertEqual(
                throughput.env["GSPLAT_BENCHMARK_WINDOW_MODE"],
                "terminal_queue_throughput_window",
            )
            self.assertEqual(
                throughput.env["GSPLAT_ORDER_COMPLETION_PROTOCOL"],
                "sustained_window",
            )
            self.assertEqual(
                throughput.env["GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT"],
                str(output / "control-current-stats" / "manifest.json"),
            )
            self.assertEqual(
                throughput.env["GSPLAT_ARTIFACT_DIR"],
                str(output / "throughput-terminal-queue"),
            )

    def test_formal_truck_web_profile_rejects_diagnostic_build_environment(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            dataset = root / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"
            trace = root / "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json"
            dataset.parent.mkdir(parents=True)
            trace.parent.mkdir(parents=True)
            dataset.write_bytes(b"ply")
            trace.write_text("{}", encoding="utf-8")
            discovery = BOOTSTRAP.Discovery(
                env={
                    "GSPLAT_WEB_TRUCK_OUTPUT": str(root / "fresh-output"),
                    BOOTSTRAP.WEB_WASM_PROFILE_ENV: "candidate20",
                    BOOTSTRAP.WEB_WASM_OUT_DIR_ENV: str(root / "diagnostic-pkg"),
                },
                home=root,
                which=FakeHost(root).which,
                capture=FakeHost(root).capture,
            )
            with (
                mock.patch.object(BOOTSTRAP, "REPO_ROOT", root),
                mock.patch.object(
                    BOOTSTRAP,
                    "web_environment",
                    return_value=([BOOTSTRAP.Probe("web", True, "ready")], {}),
                ),
            ):
                result = BOOTSTRAP.profile_result("web-webgpu-truck-1080p", discovery)

            self.assertFalse(result.ready)
            failed = {probe.key for probe in result.probes if not probe.ok}
            self.assertEqual(
                failed,
                {
                    f"exact-web-env:{BOOTSTRAP.WEB_WASM_PROFILE_ENV}",
                    f"exact-web-env:{BOOTSTRAP.WEB_WASM_OUT_DIR_ENV}",
                },
            )
            self.assertEqual(
                result.commands[0].env[BOOTSTRAP.WEB_WASM_PROFILE_ENV], "exact"
            )

    def test_truck_web_profile_blocks_on_missing_asset_or_used_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            trace = (
                root
                / "tests/perf/trace/fixtures/quality/"
                "candidate-truck-quality-1920x1080-v1.json"
            )
            trace.parent.mkdir(parents=True)
            trace.write_text("{}", encoding="utf-8")
            output = root / "retained-truck-run"
            output.mkdir()
            discovery = BOOTSTRAP.Discovery(
                env={"GSPLAT_WEB_TRUCK_OUTPUT": str(output)},
                home=root,
                which=FakeHost(root).which,
                capture=FakeHost(root).capture,
            )
            with (
                mock.patch.object(BOOTSTRAP, "REPO_ROOT", root),
                mock.patch.object(
                    BOOTSTRAP,
                    "web_environment",
                    return_value=([BOOTSTRAP.Probe("web", True, "ready")], {}),
                ),
            ):
                result = BOOTSTRAP.profile_result("web-webgpu-truck-1080p", discovery)

            self.assertFalse(result.ready)
            failed = {probe.key for probe in result.probes if not probe.ok}
            self.assertEqual(
                failed,
                {"truck-dataset", "fresh-output:GSPLAT_WEB_TRUCK_OUTPUT"},
            )

    def test_fixed_gpu_preproject_compact_truck_profile_is_independent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            dataset = root / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"
            trace = root / "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json"
            dataset.parent.mkdir(parents=True)
            trace.parent.mkdir(parents=True)
            dataset.write_bytes(b"ply")
            trace.write_text("{}", encoding="utf-8")
            output = root / "fixed-cell"
            discovery = BOOTSTRAP.Discovery(
                env={"GSPLAT_WEB_TRUCK_FIXED_GPU_COMPACT_OUTPUT": str(output)},
                home=root,
                which=FakeHost(root).which,
                capture=FakeHost(root).capture,
            )
            with (
                mock.patch.object(BOOTSTRAP, "REPO_ROOT", root),
                mock.patch.object(
                    BOOTSTRAP,
                    "web_environment",
                    return_value=([BOOTSTRAP.Probe("web", True, "ready")], {}),
                ),
            ):
                result = BOOTSTRAP.profile_result(
                    "web-webgpu-truck-fixed-gpu-preproject-compact", discovery
                )
            self.assertTrue(result.ready)
            self.assertEqual(len(result.commands), 3)
            for collector in result.commands[1:]:
                self.assertEqual(collector.env["GSPLAT_ORDER_BACKEND"], "gpu")
                self.assertEqual(collector.env["GSPLAT_PROJECTED_POLICY"], "compact")
                self.assertEqual(collector.env["GSPLAT_GPU_ORDER_PRODUCER"], "")
                self.assertEqual(
                    collector.env["GSPLAT_PHASE_E_QUALIFICATION"],
                    "truck-quality-1080p-fixed-gpu-preproject-compact-v1",
                )

    def test_run_stops_after_first_failed_command_without_retry(self) -> None:
        result = BOOTSTRAP.ProfileResult(
            name="host",
            description="test",
            touches_device=False,
            probes=(BOOTSTRAP.Probe("ready", True, "ready"),),
            commands=(
                BOOTSTRAP.Command(("first",)),
                BOOTSTRAP.Command(("second",)),
            ),
        )
        failed = BOOTSTRAP.subprocess.CompletedProcess(("first",), 17)
        with mock.patch.object(BOOTSTRAP.subprocess, "run", return_value=failed) as run:
            self.assertEqual(BOOTSTRAP.run_profile(result, allow_device=False), 17)
            run.assert_called_once()
            self.assertEqual(run.call_args.args[0], ("first",))

    def test_truck_two_stage_run_stops_after_control_failure(self) -> None:
        result = BOOTSTRAP.ProfileResult(
            name="web-webgpu-truck-1080p",
            description="test",
            touches_device=False,
            probes=(BOOTSTRAP.Probe("ready", True, "ready"),),
            commands=(
                BOOTSTRAP.Command(("build",)),
                BOOTSTRAP.Command(("control",)),
                BOOTSTRAP.Command(("throughput",)),
            ),
        )
        completed = (
            BOOTSTRAP.subprocess.CompletedProcess(("build",), 0),
            BOOTSTRAP.subprocess.CompletedProcess(("control",), 23),
        )
        with mock.patch.object(BOOTSTRAP.subprocess, "run", side_effect=completed) as run:
            self.assertEqual(BOOTSTRAP.run_profile(result, allow_device=False), 23)
            self.assertEqual(
                [call.args[0] for call in run.call_args_list],
                [("build",), ("control",)],
            )

    def test_default_doctor_is_host_only(self) -> None:
        self.assertNotIn("android-a065", BOOTSTRAP.DEFAULT_DOCTOR_PROFILES)
        self.assertNotIn("android-a065-q3-simd", BOOTSTRAP.DEFAULT_DOCTOR_PROFILES)
        self.assertNotIn("ios-simulator", BOOTSTRAP.DEFAULT_DOCTOR_PROFILES)
        self.assertNotIn("web-webgpu-truck-1080p", BOOTSTRAP.DEFAULT_DOCTOR_PROFILES)

    def test_command_display_shell_quotes_environment_and_arguments(self) -> None:
        command = BOOTSTRAP.Command(
            ("tool", "path with spaces"), {"CHROME_PATH": "/Applications/Google Chrome"}
        )
        self.assertEqual(
            command.display(),
            "CHROME_PATH='/Applications/Google Chrome' tool 'path with spaces'",
        )


if __name__ == "__main__":
    unittest.main()
