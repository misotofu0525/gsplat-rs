#!/usr/bin/env python3
"""Self-tests for the source-size and dependency ratchet."""

from __future__ import annotations

import copy
import json
import pathlib
import subprocess
import tempfile
import unittest

import check_source_architecture as checker


TEST_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = TEST_DIR.parents[1]
POLICY_PATH = TEST_DIR / "source_architecture_policy.json"
FIXTURES_PATH = TEST_DIR / "fixtures/cases.json"


def deep_update(target: dict, patch: dict) -> None:
    for key, value in patch.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            deep_update(target[key], value)
        else:
            target[key] = copy.deepcopy(value)


def materialize_file(root: pathlib.Path, relative: str, spec: dict) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    if "lines" in spec:
        lines = list(spec["lines"])
    else:
        total = spec["physical_lines"]
        prefix = list(spec.get("prefix", []))
        suffix = list(spec.get("suffix", []))
        padding = total - len(prefix) - len(suffix)
        if padding < 0:
            raise AssertionError(f"fixture {relative} has more fixed lines than physical_lines")
        lines = prefix + ([""] * padding) + suffix
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    if checker.physical_loc(path) != len(lines):
        raise AssertionError(f"fixture writer did not create the requested LOC for {relative}")


class SourceArchitectureFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.base_policy = checker.load_policy(POLICY_PATH)
        fixtures = json.loads(FIXTURES_PATH.read_text(encoding="utf-8"))
        if fixtures.get("schema") != "gsplat-source-architecture-fixtures/v1":
            raise AssertionError("unexpected fixture schema")
        cls.cases = fixtures["cases"]

    def test_fixture_matrix(self) -> None:
        for case in self.cases:
            with self.subTest(case=case["name"]), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                for relative, spec in case["files"].items():
                    materialize_file(root, relative, spec)

                policy = copy.deepcopy(self.base_policy)
                policy["grandfather"] = copy.deepcopy(case.get("grandfather", []))
                policy["exceptions"] = copy.deepcopy(case.get("exceptions", []))
                # The repository policy carries only the currently executing
                # writer pair. Fixtures opt into their own exact execution.
                policy["program_task_state"]["parallel_execution"] = None
                if "top_level_orchestration" in case:
                    deep_update(
                        policy["top_level_orchestration"],
                        case["top_level_orchestration"],
                    )
                if "plan_rules" in case:
                    deep_update(
                        policy["dependency_rules"]["plans"],
                        case["plan_rules"],
                    )
                if "program_task_state" in case:
                    deep_update(
                        policy["program_task_state"],
                        case["program_task_state"],
                    )

                default_ledger = policy["program_task_state"]["package_ledgers"]["A"][
                    "active"
                ]
                default_lines = [
                    "# Fixture progress",
                    checker.TASK_STATE_BLOCK_BEGIN,
                    *case.get("task_states", []),
                    checker.TASK_STATE_BLOCK_END,
                ]
                if "active_lanes" in case:
                    default_lines.extend(
                        [
                            checker.ACTIVE_LANE_BLOCK_BEGIN,
                            f"activation_commit = {case.get('activation_commit', '0' * 40)}",
                            *case["active_lanes"],
                            checker.ACTIVE_LANE_BLOCK_END,
                        ]
                    )
                default_lines.append(case.get("progress_prose", ""))
                default_content = "\n".join(default_lines)
                ledgers = case.get(
                    "ledgers",
                    {default_ledger: default_content},
                )
                if "ledgers" not in case:
                    for package, records in case.get("package_task_states", {}).items():
                        path = policy["program_task_state"]["package_ledgers"][package][
                            "active"
                        ]
                        ledgers[path] = "\n".join(
                            [
                                f"# Fixture package {package}",
                                checker.TASK_STATE_BLOCK_BEGIN,
                                *records,
                                checker.TASK_STATE_BLOCK_END,
                            ]
                        )
                for relative, content in ledgers.items():
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(content, encoding="utf-8")

                execution = policy["program_task_state"]["parallel_execution"]
                if execution is not None:
                    subprocess.run(
                        ["git", "init", "--quiet"],
                        cwd=root,
                        check=True,
                    )
                    subprocess.run(
                        ["git", "add", "."],
                        cwd=root,
                        check=True,
                    )
                    subprocess.run(
                        [
                            "git",
                            "-c",
                            "user.name=gsplat-fixture",
                            "-c",
                            "user.email=fixture@example.invalid",
                            "commit",
                            "--quiet",
                            "-m",
                            "fixture",
                        ],
                        cwd=root,
                        check=True,
                    )
                    fixture_head = subprocess.run(
                        ["git", "rev-parse", "HEAD"],
                        cwd=root,
                        check=True,
                        capture_output=True,
                        text=True,
                    ).stdout.strip()
                    if execution["activation_commit"] == "0" * 40:
                        execution["activation_commit"] = fixture_head
                        for relative in ledgers:
                            ledger_path = root / relative
                            ledger_text = ledger_path.read_text(encoding="utf-8")
                            ledger_path.write_text(
                                ledger_text.replace(
                                    f"activation_commit = {'0' * 40}",
                                    f"activation_commit = {fixture_head}",
                                ),
                                encoding="utf-8",
                            )

                issues, _ = checker.check_repository(root, policy)
                error_codes = sorted(
                    issue.code for issue in issues if issue.severity == "error"
                )
                notice_codes = sorted(
                    issue.code for issue in issues if issue.severity == "notice"
                )
                self.assertEqual(
                    error_codes,
                    sorted(case["expected_error_codes"]),
                    msg="\n".join(issue.render() for issue in issues),
                )
                if "expected_notice_codes" in case:
                    self.assertEqual(
                        notice_codes,
                        sorted(case["expected_notice_codes"]),
                        msg="\n".join(issue.render() for issue in issues),
                    )

    def test_policy_registers_circuit_breaker_breaches_without_turning_targets_into_quotas(self) -> None:
        policy = copy.deepcopy(self.base_policy)
        sources = {
            kind: checker.discover(REPO_ROOT, source_set)
            for kind, source_set in policy["source_sets"].items()
        }
        hard_breached: dict[str, int] = {}
        for kind, paths in sources.items():
            for relative in paths:
                loc = checker.physical_loc(REPO_ROOT / relative)
                profile = checker.size_profile(relative, kind, policy)
                if profile["hard"] is not None and loc > profile["hard"]:
                    hard_breached[relative] = loc

        grandfather = {
            entry["path"]: entry for entry in policy["grandfather"]
        }
        exceptions = {entry["path"] for entry in policy["exceptions"]}
        self.assertLessEqual(set(hard_breached), set(grandfather) | exceptions)
        growth_tolerance = policy["grandfather_ratchet"]["growth_tolerance_lines"]
        for path, entry in grandfather.items():
            loc = checker.physical_loc(REPO_ROOT / path)
            baseline = entry["baseline_physical_loc"]
            self.assertLessEqual(loc, baseline + growth_tolerance)
            self.assertTrue(entry["owner_task"])
            self.assertTrue(entry["exit_condition"])

    def test_declared_guardrail_values_and_future_activation(self) -> None:
        limits = self.base_policy["limits"]
        self.assertEqual(
            (limits["production_rust"]["target_lt"], limits["production_rust"]["hard_ceiling"]),
            (800, 2500),
        )
        self.assertEqual(
            (limits["concrete_plan"]["target_lt"], limits["concrete_plan"]["hard_ceiling"]),
            (600, 2500),
        )
        self.assertFalse(limits["production_rust"]["enforce_target"])
        self.assertFalse(limits["concrete_plan"]["enforce_target"])
        self.assertEqual(limits["render_lib"]["target_lt"], 200)
        self.assertFalse(limits["render_lib"]["enforce_target"])
        self.assertEqual(limits["renderer_orchestrator"]["target_lt"], 800)
        self.assertFalse(limits["renderer_orchestrator"]["enforce_target"])
        self.assertEqual(limits["wgsl"]["target_lt"], 350)
        self.assertEqual(limits["wgsl"]["hard_ceiling"], 2500)
        self.assertFalse(limits["wgsl"]["enforce_target"])
        self.assertEqual(
            self.base_policy["grandfather_ratchet"],
            {"growth_tolerance_lines": 32},
        )
        orchestration = self.base_policy["top_level_orchestration"]
        self.assertFalse(orchestration["enabled"])
        self.assertEqual(orchestration["target_lt"], 150)
        self.assertFalse(orchestration["enforce_target"])
        self.assertEqual(orchestration["activation_task"], "E1")
        self.assertTrue(orchestration["reason"])
        self.assertEqual(orchestration["functions"], [])
        plans = self.base_policy["dependency_rules"]["plans"]
        self.assertEqual(plans["activation_task"], "E1")
        self.assertTrue(plans["reason"])
        self.assertEqual(plans["per_frame_functions"], [])
        self.assertEqual(plans["preparation_only_files"], [])
        self.assertIn("crates/gsplat-render-wgpu/src/plans.rs", plans["include"])
        self.assertIn(
            "crates/gsplat-render-wgpu/src/gpu.rs",
            self.base_policy["dependency_rules"]["gpu"]["include"],
        )
        hosts = self.base_policy["dependency_rules"]["platform_hosts"]["include"]
        self.assertIn("crates/gsplat-render-wgpu/src/surface.rs", hosts)
        self.assertIn("crates/gsplat-render-wgpu/src/offscreen.rs", hosts)
        self.assertIn(
            "crates/gsplat-render-wgpu/src/renderer.rs",
            limits["renderer_orchestrator"]["paths"],
        )
        rust_sources = self.base_policy["source_sets"]["rust"]
        self.assertNotIn("bindings/**/*.rs", rust_sources["include"])
        self.assertNotIn("exclude_dir_names", rust_sources)

        state = self.base_policy["program_task_state"]
        self.assertEqual(
            set(state),
            {
                "external_owner_review_allowlist",
                "parallel_execution",
                "task_catalog",
                "package_ledgers",
            },
        )
        self.assertEqual(set(state["package_ledgers"]), {"A", "E", "M", "B", "S", "Q"})
        self.assertEqual(set(state["task_catalog"].values()), {"A", "E", "M", "B", "S", "Q"})
        expected_tasks = {
            *(f"A{index}" for index in range(10)),
            *(f"E{index}" for index in range(14)),
            *(f"M{index}" for index in range(9)),
            *(f"B{index}" for index in range(7)),
            *(f"S{index}" for index in range(8)),
            *(f"Q{index}" for index in range(5)),
        }
        self.assertEqual(set(state["task_catalog"]), expected_tasks)
        for package, pair in state["package_ledgers"].items():
            self.assertEqual(
                set(pair), {"active", "completed", "required_after"}, package
            )
            self.assertNotEqual(pair["active"], pair["completed"])
        self.assertIsNone(state["package_ledgers"]["A"]["required_after"])
        self.assertEqual(state["package_ledgers"]["E"]["required_after"], "A9")
        self.assertEqual(state["package_ledgers"]["M"]["required_after"], "E13")
        for package in ("B", "S", "Q"):
            self.assertEqual(state["package_ledgers"][package]["required_after"], "M8")
        self.assertEqual(
            set(state["external_owner_review_allowlist"]),
            {"IO-PLY-1", "IO-SPZ-1"},
        )
        # Parallel execution is an ephemeral lease. The adversarial fixtures
        # prove its schema and overlap rules; the checked-in program state must
        # return to null as soon as the declared writers are integrated.
        self.assertIsNone(state["parallel_execution"])

        external = {
            entry["owner_task"]: entry
            for entry in self.base_policy["grandfather"]
            if entry["owner_task"] in {"IO-PLY-1", "IO-SPZ-1"}
        }
        self.assertEqual(set(external), {"IO-PLY-1", "IO-SPZ-1"})
        for entry in external.values():
            self.assertEqual(entry["review_task"], "A9")
            self.assertTrue(entry["review_action"])


if __name__ == "__main__":
    unittest.main()
