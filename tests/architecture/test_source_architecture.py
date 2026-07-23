#!/usr/bin/env python3
"""Self-tests for the source-size and dependency ratchet."""

from __future__ import annotations

import copy
import json
import pathlib
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
                (root / "progress.md").write_text(
                    case.get("progress", "# Fixture progress\n"), encoding="utf-8"
                )

                policy = copy.deepcopy(self.base_policy)
                policy["progress_file"] = "progress.md"
                policy["grandfather"] = copy.deepcopy(case.get("grandfather", []))
                policy["exceptions"] = copy.deepcopy(case.get("exceptions", []))
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

    def test_policy_exactly_covers_a0_target_breaches(self) -> None:
        policy = copy.deepcopy(self.base_policy)
        sources = {
            kind: checker.discover(REPO_ROOT, source_set)
            for kind, source_set in policy["source_sets"].items()
        }
        breached: dict[str, int] = {}
        for kind, paths in sources.items():
            for relative in paths:
                loc = checker.physical_loc(REPO_ROOT / relative)
                profile = checker.size_profile(relative, kind, policy)
                if loc >= profile["target"]:
                    breached[relative] = loc

        grandfather = {
            entry["path"]: entry for entry in policy["grandfather"]
        }
        self.assertEqual(set(grandfather), set(breached))
        for path, loc in breached.items():
            self.assertEqual(grandfather[path]["baseline_physical_loc"], loc)
            self.assertTrue(grandfather[path]["owner_task"])
            self.assertTrue(grandfather[path]["exit_condition"])

    def test_declared_guardrail_values_and_future_activation(self) -> None:
        limits = self.base_policy["limits"]
        self.assertEqual(
            (limits["production_rust"]["target_lt"], limits["production_rust"]["hard_ceiling"]),
            (800, 1200),
        )
        self.assertEqual(
            (limits["concrete_plan"]["target_lt"], limits["concrete_plan"]["hard_ceiling"]),
            (600, 1000),
        )
        self.assertEqual(limits["render_lib"]["target_lt"], 200)
        self.assertEqual(limits["renderer_orchestrator"]["target_lt"], 800)
        self.assertEqual(limits["wgsl"]["target_lt"], 350)
        orchestration = self.base_policy["top_level_orchestration"]
        self.assertFalse(orchestration["enabled"])
        self.assertEqual(orchestration["target_lt"], 150)
        self.assertEqual(orchestration["activation_task"], "E1")
        self.assertTrue(orchestration["reason"])
        self.assertEqual(orchestration["functions"], [])
        plans = self.base_policy["dependency_rules"]["plans"]
        self.assertEqual(plans["activation_task"], "E1")
        self.assertTrue(plans["reason"])
        self.assertEqual(plans["per_frame_functions"], [])
        self.assertEqual(plans["preparation_only_files"], [])

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
