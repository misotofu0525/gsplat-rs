#!/usr/bin/env python3
"""Focused, device-free tests for the Q3 A065 SIMD orchestrator."""

from __future__ import annotations

import argparse
import contextlib
import copy
import importlib.util
import io
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = pathlib.Path(__file__).with_name("collect-q3-a065-simd.py")
SPEC = importlib.util.spec_from_file_location("q3_a065_simd_collector", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


def lane_receipts() -> dict[str, dict]:
    return {
        "scalar": {
            "lane": "scalar",
            "cargo_feature": "qualification-q3-cpu-scalar",
            "selector_environment": {
                COLLECTOR.Q3_LANE_ENV: "scalar",
            },
            "apk": {"bytes": 10, "sha256": "1" * 64},
            "native_library": {"bytes": 5, "sha256": "2" * 64},
        },
        "neon": {
            "lane": "neon",
            "cargo_feature": "qualification-q3-cpu-neon",
            "selector_environment": {
                COLLECTOR.Q3_LANE_ENV: "neon",
            },
            "apk": {"bytes": 10, "sha256": "3" * 64},
            "native_library": {"bytes": 5, "sha256": "4" * 64},
        },
    }


def write_phase(path: pathlib.Path, phase: str) -> None:
    phases = ["preflight", "build", "install", "launched", "evidence"]
    index = phases.index(phase)
    path.write_text(
        COLLECTOR.json.dumps(
            {
                "schema": COLLECTOR.PHASE_SCHEMA,
                "label": "fixture",
                "phase": phase,
                "history": [{"phase": item} for item in phases[: index + 1]],
            }
        ),
        encoding="utf-8",
    )


def prepared_workload(path: pathlib.Path, workload_id: str = "truck-050k") -> object:
    identity = {"bytes": 123, "sha256": "a" * 64}
    receipt = {
        "preparation_id": f"matrix:{workload_id}",
        "workload": workload_id,
        "dataset": {
            "package_internal": {
                "path": COLLECTOR.BASE.INTERNAL_DATASET,
                **identity,
            }
        },
        "trace": {
            "package_internal": {
                "path": COLLECTOR.BASE.INTERNAL_TRACE,
                **identity,
            }
        },
    }
    path.write_text("{}", encoding="utf-8")
    return COLLECTOR.PreparedWorkload(path, receipt)


def workload_fixture(workload_id: str = "truck-200k") -> object:
    return COLLECTOR.Workload(
        workload_id,
        pathlib.Path(f"{workload_id}.ply"),
        {
            "id": workload_id,
            "bytes": 123,
            "sha256": "a" * 64,
            "splat_count": 200_000,
            "sh_degree": 3,
        },
        "scaling_tier",
    )


def range_rejection_receipt(
    workload: object,
    run_identity: str,
    phase: str = "launched",
) -> dict:
    return {
        "schema": COLLECTOR.RANGE_REJECTION_SCHEMA,
        "receipt_id": run_identity,
        "producer": "renderer_scene_admission",
        "reason": "capacity",
        "decision": "Rejected",
        "activity_phase": phase,
        "workload": {
            "dataset_id": workload.id,
            "dataset_sha256": workload.identity["sha256"],
            "splat_count": workload.identity["splat_count"],
        },
    }


class Q3A065SimdCollectorTests(unittest.TestCase):
    def test_build_selector_allowlist_and_product_default(self) -> None:
        base = {"PATH": "/fixture", COLLECTOR.Q3_LANE_ENV: "stale"}
        ordinary = COLLECTOR.android_build_environment(None, base)
        self.assertNotIn(COLLECTOR.Q3_LANE_ENV, ordinary)
        self.assertEqual(ordinary["ANDROID_RUST_PROFILE"], "release")

        scalar = COLLECTOR.android_build_environment("scalar", base)
        neon = COLLECTOR.android_build_environment("neon", base)
        self.assertEqual(scalar[COLLECTOR.Q3_LANE_ENV], "scalar")
        self.assertEqual(neon[COLLECTOR.Q3_LANE_ENV], "neon")
        with self.assertRaisesRegex(ValueError, "unsupported Q3 Android CPU lane"):
            COLLECTOR.android_build_environment("avx2", base)
        build_script = (
            COLLECTOR.BASE.REPO_ROOT / "bindings/android/scripts/build-native.sh"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "CARGO_BUILD_ARGS=(build -p gsplat-ffi-c --target aarch64-linux-android)",
            build_script,
        )
        self.assertIn(
            'CARGO_BUILD_ARGS+=(--features "qualification-q3-cpu-$GSPLAT_ANDROID_Q3_CPU_LANE")',
            build_script,
        )
        self.assertNotIn("CARGO_FEATURE_ARGS", build_script)

    def test_native_build_rejects_arbitrary_lane_before_sdk_probe(self) -> None:
        environment = os.environ.copy()
        environment[COLLECTOR.Q3_LANE_ENV] = "scalar,neon"
        environment["ANDROID_SDK_ROOT"] = "/definitely/not/an/android/sdk"
        completed = subprocess.run(
            ["bash", str(COLLECTOR.BASE.REPO_ROOT / "bindings/android/scripts/build-native.sh")],
            cwd=COLLECTOR.REPO_ROOT,
            env=environment,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("Unsupported GSPLAT_ANDROID_Q3_CPU_LANE", completed.stdout)
        self.assertNotIn("ANDROID_SDK_ROOT not found", completed.stdout)

    def test_lane_receipts_bind_feature_selector_and_distinct_binary_hash(self) -> None:
        receipts = lane_receipts()
        COLLECTOR.validate_lane_build_receipts(receipts)
        receipts["neon"]["native_library"]["sha256"] = "2" * 64
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "same native-library hash"
        ):
            COLLECTOR.validate_lane_build_receipts(receipts)

    def test_schedule_is_finite_staged_and_terminal_is_five_pairs(self) -> None:
        self.assertEqual(COLLECTOR.REQUIRED_WORKLOAD_IDS[-1], "truck-full")
        self.assertEqual(len(COLLECTOR.REQUIRED_WORKLOAD_IDS), 9)
        first = COLLECTOR.schedule_pairs(5, 17, "truck-050k")
        second = COLLECTOR.schedule_pairs(5, 17, "truck-050k")
        self.assertEqual(first, second)
        self.assertEqual(len(first), 5)
        self.assertLessEqual(
            abs(first.count(("scalar", "neon")) - first.count(("neon", "scalar"))),
            1,
        )
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "exactly 5"
        ):
            COLLECTOR.schedule_pairs(4, 17, "truck-050k")

        plan = COLLECTOR.staged_plan(17)
        self.assertEqual(len(plan), 9)
        self.assertTrue(all(item["timing_pairs"] == 1 for item in plan[:-1]))
        self.assertTrue(all(item["evidence_class"] == "diagnostic" for item in plan[:-1]))
        self.assertEqual(plan[-1]["workload"], "truck-full")
        self.assertEqual(plan[-1]["timing_pairs"], 5)
        self.assertEqual(plan[-1]["evidence_class"], "terminal")
        diagnostic_orders = [tuple(item["schedule"][0]) for item in plan[:-1]]
        self.assertEqual(diagnostic_orders.count(("scalar", "neon")), 4)
        self.assertEqual(diagnostic_orders.count(("neon", "scalar")), 4)
        # Two control runs plus one pair per ladder tier, then two controls and
        # five pairs for complete Truck. The former 108-run matrix is gone.
        self.assertEqual(sum(2 + 2 * item["timing_pairs"] for item in plan), 44)

    def test_performance_loss_does_not_cut_larger_point_ladder(self) -> None:
        performance_cell = {
            "workload": "truck-050k",
            "decision": "Rejected",
            "reason": "single_pair_diagnostic_no_neon_benefit_no_promotion",
        }
        self.assertFalse(COLLECTOR.rejects_larger_workload_range(performance_cell))
        self.assertIsNone(
            COLLECTOR.range_rejected_at_after_cell(None, performance_cell)
        )
        later = list(COLLECTOR.REQUIRED_WORKLOAD_IDS[1:])
        self.assertEqual(
            later,
            [
                "truck-100k",
                "truck-200k",
                "truck-300k",
                "truck-500k",
                "truck-1m",
                "truck-1p5m",
                "truck-2m",
                "truck-full",
            ],
        )

    def test_explicit_capacity_or_admission_failure_cuts_larger_range(self) -> None:
        range_cell = {
            "workload": "truck-200k",
            "decision": "Rejected",
            "reason": "capacity_or_admission_range_rejected",
        }
        self.assertTrue(COLLECTOR.rejects_larger_workload_range(range_cell))
        self.assertEqual(
            COLLECTOR.range_rejected_at_after_cell(None, range_cell),
            "truck-200k",
        )
        range_cell["reason"] = "integrity_rejected"
        self.assertFalse(COLLECTOR.rejects_larger_workload_range(range_cell))

    def test_only_explicit_range_or_matrix_identity_changes_control_flow(self) -> None:
        self.assertEqual(
            COLLECTOR.cell_failure_scope(
                COLLECTOR.RangeAdmissionRejectedError("range")
            ),
            "range_cutoff",
        )
        self.assertEqual(
            COLLECTOR.cell_failure_scope(
                COLLECTOR.MatrixInfrastructureError("identity")
            ),
            "matrix_infrastructure",
        )
        self.assertEqual(
            COLLECTOR.cell_failure_scope(
                COLLECTOR.IntegrityRejectedError("artifact")
            ),
            "cell_local",
        )
        self.assertEqual(
            COLLECTOR.cell_failure_scope(ValueError("malformed artifact")),
            "cell_local",
        )

    def test_structured_capacity_failure_is_a_range_rejection(self) -> None:
        workload = workload_fixture()
        run_identity = "1" * 32
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            phase = root / "phase.json"
            output = root / "run"
            output.mkdir()
            write_phase(phase, "launched")
            (output / "experiment.json").write_text(
                COLLECTOR.json.dumps(
                    {
                        "qualification_q3_run_identity": run_identity,
                        "range_rejection_receipt": range_rejection_receipt(
                            workload, run_identity
                        )
                    }
                ),
                encoding="utf-8",
            )
            error = COLLECTOR.nonzero_collector_error(
                1,
                phase,
                "truck-200k",
                output,
                workload,
                run_identity,
            )
        self.assertIsInstance(error, COLLECTOR.RangeAdmissionRejectedError)

    def test_stale_range_receipt_id_cannot_cut_current_run(self) -> None:
        workload = workload_fixture()
        current_run_identity = "1" * 32
        stale_run_identity = "2" * 32
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            phase = root / "phase.json"
            output = root / "run"
            output.mkdir()
            write_phase(phase, "launched")
            (output / "experiment.json").write_text(
                COLLECTOR.json.dumps(
                    {
                        "qualification_q3_run_identity": current_run_identity,
                        "range_rejection_receipt": range_rejection_receipt(
                            workload, stale_run_identity
                        ),
                    }
                ),
                encoding="utf-8",
            )
            error = COLLECTOR.nonzero_collector_error(
                1,
                phase,
                "truck-200k",
                output,
                workload,
                current_run_identity,
            )
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
        self.assertNotIsInstance(error, COLLECTOR.RangeAdmissionRejectedError)
        self.assertIn("host-issued run identity", str(error))

    def test_generic_post_launch_oom_text_is_integrity_not_range(self) -> None:
        workload = workload_fixture()
        run_identity = "1" * 32
        messages = (
            "artifact PNG encoder crashed: java.lang.OutOfMemoryError",
            "PackageManager OutOfMemoryError while replacing package",
            "collector parse ledger failed: outofmemoryerror",
        )
        for message in messages:
            with self.subTest(message=message), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                phase = root / "phase.json"
                output = root / "run"
                log = output / "run-001/logcat.txt"
                log.parent.mkdir(parents=True)
                log.write_text(message + "\n", encoding="utf-8")
                write_phase(phase, "evidence")
                (output / "experiment.json").write_text(
                    COLLECTOR.json.dumps(
                        {
                            "qualification_q3_run_identity": run_identity,
                            "status": "failed",
                            "error": message,
                            "failure_class": "capacity_or_admission_range",
                        }
                    ),
                    encoding="utf-8",
                )
                error = COLLECTOR.nonzero_collector_error(
                    1,
                    phase,
                    "truck-200k",
                    output,
                    workload,
                    run_identity,
                )
            self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
            self.assertNotIsInstance(error, COLLECTOR.RangeAdmissionRejectedError)

    def test_unbound_or_prelaunch_range_receipt_is_integrity_rejected(self) -> None:
        workload = workload_fixture()
        run_identity = "1" * 32
        cases = (("launched", "wrong_hash"), ("install", "prelaunch"))
        for phase_name, mutation in cases:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                phase = root / "phase.json"
                output = root / "run"
                output.mkdir()
                write_phase(phase, phase_name)
                receipt = range_rejection_receipt(
                    workload, run_identity, phase_name
                )
                if mutation == "wrong_hash":
                    receipt["workload"]["dataset_sha256"] = "0" * 64
                (output / "experiment.json").write_text(
                    COLLECTOR.json.dumps(
                        {
                            "qualification_q3_run_identity": run_identity,
                            "range_rejection_receipt": receipt,
                        }
                    ),
                    encoding="utf-8",
                )
                error = COLLECTOR.nonzero_collector_error(
                    1,
                    phase,
                    "truck-200k",
                    output,
                    workload,
                    run_identity,
                )
            self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
            self.assertNotIsInstance(error, COLLECTOR.RangeAdmissionRejectedError)

    def test_range_receipt_requires_complete_activity_phase_history(self) -> None:
        workload = workload_fixture()
        run_identity = "1" * 32
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            phase = root / "phase.json"
            output = root / "run"
            output.mkdir()
            phase.write_text(
                COLLECTOR.json.dumps(
                    {
                        "schema": COLLECTOR.PHASE_SCHEMA,
                        "label": "fixture",
                        "phase": "launched",
                        "history": [{"phase": "launched"}],
                    }
                ),
                encoding="utf-8",
            )
            (output / "experiment.json").write_text(
                COLLECTOR.json.dumps(
                    {
                        "qualification_q3_run_identity": run_identity,
                        "range_rejection_receipt": range_rejection_receipt(
                            workload, run_identity
                        )
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                COLLECTOR.IntegrityRejectedError,
                "history is incomplete or out of order",
            ):
                COLLECTOR.nonzero_collector_error(
                    1,
                    phase,
                    "truck-200k",
                    output,
                    workload,
                    run_identity,
                )

    def test_cell_local_integrity_rejection_continues_ladder_and_truck(self) -> None:
        expected_commit = "1" * 40
        environment = {"fixture": "a065"}
        workloads = [
            COLLECTOR.Workload(
                workload_id,
                pathlib.Path(f"{workload_id}.ply"),
                {
                    "id": workload_id,
                    "bytes": 123,
                    "sha256": f"{index + 1:064x}",
                    "splat_count": (index + 1) * 50_000,
                    "sh_degree": 3,
                },
                "full_scene" if workload_id == "truck-full" else "scaling_tier",
            )
            for index, workload_id in enumerate(COLLECTOR.REQUIRED_WORKLOAD_IDS)
        ]
        calls: list[str] = []

        def collected_run(*positional, **unused_keywords):
            workload = positional[6]
            lane = positional[9]
            calls.append(workload.id)
            if workload.id == "truck-050k":
                raise COLLECTOR.IntegrityRejectedError(
                    "artifact PNG encoder failed after launch"
                )
            mean = 2.0 if lane == "scalar" else 1.0
            return {
                "lane": lane,
                "workload": workload.id,
                "environment": environment,
                "adapter": "Adreno 730",
                "backend": "vulkan",
                "semantic_fingerprint": {"workload": workload.id},
                "distributions": {
                    metric: {"mean": mean}
                    for metric in (
                        "preprocess_ms",
                        "sort_ms",
                        "call_ms",
                        "frame_wall_ms",
                        "cpu_frame_complete_ms",
                    )
                },
            }

        @contextlib.contextmanager
        def staged_trace(*unused_args):
            yield "/data/local/tmp/q3-trace.json"

        @contextlib.contextmanager
        def prepared_inputs(session, workload, *unused_args):
            path = session.stage / f"prepared-{workload.id}.json"
            path.write_text("{}", encoding="utf-8")
            yield COLLECTOR.PreparedWorkload(
                path,
                {"preparation_id": f"matrix:{workload.id}"},
            )

        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            output = root / "q3-output"
            trace_path = root / "trace.json"
            trace_path.write_text("{}", encoding="utf-8")
            args = argparse.Namespace(
                output=output,
                expected_commit=expected_commit,
                dry_run=False,
                matrix=COLLECTOR.MATRIX_PATH,
                seed=17,
                serial="fixture-serial",
                adb=None,
                run_timeout_seconds=10.0,
                warmup=1,
                measured=2,
                correctness_frames=1,
                max_thermal_status=0,
                thermal_timeout_seconds=1.0,
            )
            with (
                mock.patch.object(
                    COLLECTOR,
                    "git_receipt",
                    return_value={"commit": expected_commit, "dirty": False},
                ),
                mock.patch.object(
                    COLLECTOR,
                    "load_workloads",
                    return_value=(workloads, {}, trace_path),
                ),
                mock.patch.object(
                    COLLECTOR, "preflight_a065", return_value=environment
                ),
                mock.patch.object(
                    COLLECTOR,
                    "build_element_oracle",
                    return_value={"fixture": "oracle"},
                ),
                mock.patch.object(
                    COLLECTOR,
                    "build_lane_apks",
                    return_value=lane_receipts(),
                ),
                mock.patch.object(
                    COLLECTOR,
                    "run_element_oracle",
                    return_value={"decision": "Accepted"},
                ),
                mock.patch.object(COLLECTOR.BASE, "resolve_adb", return_value="adb"),
                mock.patch.object(
                    COLLECTOR.DeviceMatrixSession,
                    "ensure_lane",
                    return_value=(root / "installed.json", False),
                ),
                mock.patch.object(
                    COLLECTOR, "staged_matrix_trace", side_effect=staged_trace
                ),
                mock.patch.object(
                    COLLECTOR,
                    "prepared_workload_inputs",
                    side_effect=prepared_inputs,
                ),
                mock.patch.object(
                    COLLECTOR, "run_collection_once", side_effect=collected_run
                ),
            ):
                result = COLLECTOR.collect(args)

        self.assertEqual(result["cells"][0]["workload"], "truck-050k")
        self.assertEqual(result["cells"][0]["reason"], "integrity_rejected")
        self.assertEqual(result["cells"][0]["failure_scope"], "cell_local")
        self.assertIn("truck-100k", calls)
        self.assertIn("truck-full", calls)
        self.assertEqual(result["cells"][-1]["workload"], "truck-full")
        self.assertEqual(len(result["cells"]), len(COLLECTOR.REQUIRED_WORKLOAD_IDS))
        self.assertEqual(result["cells"][-1]["decision"], "Accepted")
        self.assertFalse(result["cells"][-1]["whole_plan_promotion"])
        self.assertEqual(result["decision"], "Rejected")
        self.assertEqual(
            result["reason"], "matrix_contains_integrity_rejected_cells"
        )
        self.assertEqual(calls.count("truck-050k"), 1)

    def test_prepared_receipts_bind_local_device_and_installed_hashes(self) -> None:
        args = argparse.Namespace(serial="fixture-serial")
        dataset = {"bytes": 123, "sha256": "a" * 64}
        trace = {"bytes": 17, "sha256": "b" * 64}
        apk = {"bytes": 29, "sha256": "c" * 64}
        native = {"bytes": 31, "sha256": "d" * 64}
        prepared = {
            "schema": COLLECTOR.PREPARED_INPUTS_SCHEMA,
            "preparation_id": "matrix:truck-050k",
            "serial": args.serial,
            "package": COLLECTOR.BASE.PACKAGE,
            "workload": "truck-050k",
            "preparation_counts_at_publish": {
                "package_clear": 1,
                "dataset_push": 1,
                "dataset_copy": 1,
                "trace_push": 1,
                "trace_copy": 1,
            },
            "dataset": {
                "local": dataset,
                "device_staged": {
                    "path": COLLECTOR.BASE.device_dataset_path(dataset["sha256"]),
                    **dataset,
                },
                "package_internal": {
                    "path": COLLECTOR.BASE.INTERNAL_DATASET,
                    **dataset,
                },
            },
            "trace": {
                "local": trace,
                "device_staged": {
                    "path": COLLECTOR.BASE.device_trace_path(trace["sha256"]),
                    **trace,
                },
                "package_internal": {
                    "path": COLLECTOR.BASE.INTERNAL_TRACE,
                    **trace,
                },
            },
        }
        installed = {
            "schema": COLLECTOR.INSTALLED_APK_SCHEMA,
            "serial": args.serial,
            "package": COLLECTOR.BASE.PACKAGE,
            "lane": "scalar",
            "install_sequence": 1,
            "local_apk": apk,
            "installed_apk": {
                "device_path": "/data/app/base.apk",
                "run_as_verified": True,
                **apk,
            },
            "native_library": native,
        }
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            prepared_path = root / "prepared.json"
            installed_path = root / "installed.json"
            prepared_path.write_text(COLLECTOR.json.dumps(prepared), encoding="utf-8")
            installed_path.write_text(COLLECTOR.json.dumps(installed), encoding="utf-8")
            self.assertEqual(
                COLLECTOR.BASE.load_q3_prepared_inputs(
                    prepared_path, args, dataset, trace
                )["preparation_id"],
                "matrix:truck-050k",
            )
            self.assertEqual(
                COLLECTOR.BASE.load_q3_installed_apk_receipt(
                    installed_path, args, apk, native
                )["install_sequence"],
                1,
            )
            prepared["dataset"]["package_internal"]["sha256"] = "0" * 64
            prepared_path.write_text(COLLECTOR.json.dumps(prepared), encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "device identity drifted"):
                COLLECTOR.BASE.load_q3_prepared_inputs(
                    prepared_path, args, dataset, trace
                )

    def test_lane_install_is_reused_until_the_counterbalanced_lane_changes(self) -> None:
        receipts = lane_receipts()
        for lane_name in ("scalar", "neon"):
            receipts[lane_name]["apk_path"] = (
                f"build/{lane_name}/sample-app-debug.apk"
            )
        with tempfile.TemporaryDirectory() as directory:
            stage = pathlib.Path(directory)
            session = COLLECTOR.DeviceMatrixSession(
                "adb", "fixture-serial", stage, receipts
            )
            installed_by_lane = {
                lane_name: {
                    "device_path": "/data/app/base.apk",
                    **receipts[lane_name]["apk"],
                    "run_as_verified": True,
                }
                for lane_name in ("scalar", "neon")
            }
            current_lane = {"value": None}

            def install(*args: object, **kwargs: object) -> None:
                apk = pathlib.Path(args[2])
                current_lane["value"] = "scalar" if "scalar" in apk.parts else "neon"

            def verify(*unused_args: object, **unused_kwargs: object) -> dict:
                return installed_by_lane[current_lane["value"]]

            requested_lanes = []
            with (
                mock.patch.object(COLLECTOR.BASE, "install_apk", side_effect=install) as install_mock,
                mock.patch.object(COLLECTOR.BASE, "verify_installed_apk", side_effect=verify),
            ):
                requested_lanes.append("scalar")
                session.ensure_lane("scalar", 10.0)
                for stage_spec in COLLECTOR.staged_plan(17):
                    workload_lanes = ["scalar", "scalar", "neon"]
                    workload_lanes.extend(
                        lane
                        for pair in stage_spec["schedule"]
                        for lane in pair
                    )
                    for lane_name in workload_lanes:
                        requested_lanes.append(lane_name)
                        session.ensure_lane(lane_name, 10.0)
            expected_transitions = sum(
                index == 0 or lane != requested_lanes[index - 1]
                for index, lane in enumerate(requested_lanes)
            )
            self.assertEqual(install_mock.call_count, expected_transitions)
            self.assertEqual(session.install_count, expected_transitions)
            self.assertEqual(session.install_count, 34)
            self.assertLess(session.install_count, 44)

    def test_workload_preparation_pushes_and_copies_only_at_workload_boundaries(self) -> None:
        trace_identity = {"bytes": 17, "sha256": "e" * 64}
        dataset_identity = {"bytes": 123, "sha256": "d" * 64}

        @contextlib.contextmanager
        def staged_trace(*unused_args: object, **unused_kwargs: object):
            yield "/data/local/tmp/trace.json"

        @contextlib.contextmanager
        def staged_dataset(*unused_args: object, **unused_kwargs: object):
            yield "/data/local/tmp/model.ply"

        success = subprocess.CompletedProcess([], 0, "Success")
        with tempfile.TemporaryDirectory() as directory:
            stage = pathlib.Path(directory)
            session = COLLECTOR.DeviceMatrixSession(
                "adb", "fixture-serial", stage, {}
            )
            workloads = [
                COLLECTOR.Workload(
                    workload_id,
                    pathlib.Path(f"{workload_id}.ply"),
                    {"id": workload_id, **dataset_identity},
                    "full_scene" if workload_id == "truck-full" else "scaling_tier",
                )
                for workload_id in COLLECTOR.REQUIRED_WORKLOAD_IDS
            ]
            with (
                mock.patch.object(COLLECTOR.BASE, "staged_device_trace", side_effect=staged_trace) as trace_stage,
                mock.patch.object(COLLECTOR.BASE, "staged_device_dataset", side_effect=staged_dataset) as dataset_stage,
                mock.patch.object(COLLECTOR.BASE, "run_command", return_value=success) as run_command,
                mock.patch.object(COLLECTOR.BASE, "inject_device_dataset", return_value=dataset_identity) as copy_dataset,
                mock.patch.object(COLLECTOR.BASE, "inject_device_trace", return_value=trace_identity) as copy_trace,
                mock.patch.object(COLLECTOR.BASE, "read_device_file_identity", return_value=trace_identity),
            ):
                with COLLECTOR.staged_matrix_trace(
                    session, pathlib.Path("trace.json"), trace_identity
                ) as temporary_trace:
                    for workload in workloads:
                        with COLLECTOR.prepared_workload_inputs(
                            session, workload, trace_identity, temporary_trace
                        ):
                            pass
            trace_stage.assert_called_once()
            self.assertEqual(dataset_stage.call_count, len(workloads))
            self.assertEqual(copy_dataset.call_count, len(workloads))
            copy_trace.assert_called_once()
            self.assertEqual(session.package_clear_count, 1)
            self.assertEqual(session.trace_push_count, 1)
            self.assertEqual(session.trace_copy_count, 1)
            self.assertEqual(session.dataset_push_count, len(workloads))
            self.assertEqual(session.dataset_copy_count, len(workloads))
            self.assertEqual(run_command.call_count, 2)

    def test_prepared_measurement_does_not_clear_push_or_copy_and_is_not_retried(self) -> None:
        args = argparse.Namespace(
            serial="fixture-serial",
            cooldown_seconds=0.0,
            frames=80,
            warmup=20,
            yaw=0.001,
            sort_interval=1,
            async_sort=False,
            frame_latency=2,
            geometry_path="packed",
            gpu_producer=None,
            max_thermal_status=None,
            thermal_timeout_seconds=300.0,
            thermal_poll_seconds=5.0,
            qualification_q3_capture_final_png=False,
            formal_artifact=False,
            qualification_q3_phase_receipt=None,
            qualification_q3_run_identity=None,
            run_timeout_seconds=10.0,
            camera_trace=pathlib.Path("trace.json"),
            camera_frame=None,
            camera_frame_indices="0,1",
        )
        identity = {"bytes": 123, "sha256": "a" * 64}
        prepared = {
            "preparation_id": "matrix:truck-050k",
            "workload": "truck-050k",
            "dataset": {
                "package_internal": {
                    "path": COLLECTOR.BASE.INTERNAL_DATASET,
                    **identity,
                }
            },
            "trace": {
                "package_internal": {
                    "path": COLLECTOR.BASE.INTERNAL_TRACE,
                    **identity,
                }
            },
        }
        experiment = {
            "dataset": identity,
            "trace": identity,
            "prepared_inputs_receipt": {"preparation_id": prepared["preparation_id"]},
            "runs": [],
        }
        schedule = [COLLECTOR.BASE.RunSpec(1, 1, 1, "cpu")]
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            experiment_path = output / "experiment.json"
            environment_path = output / "environment.json"
            environment_path.write_text("{}", encoding="utf-8")
            with (
                mock.patch.object(COLLECTOR.BASE, "run_command") as run_command,
                mock.patch.object(COLLECTOR.BASE, "inject_device_dataset") as copy_dataset,
                mock.patch.object(COLLECTOR.BASE, "inject_device_trace") as copy_trace,
                mock.patch.object(COLLECTOR.BASE, "read_thermal_status", return_value=0),
                mock.patch.object(
                    COLLECTOR.BASE,
                    "collect_logcat_run",
                    side_effect=RuntimeError("synthetic one-shot failure"),
                ) as launch,
            ):
                with self.assertRaisesRegex(RuntimeError, "synthetic one-shot failure"):
                    COLLECTOR.BASE.collect_scheduled_runs(
                        args,
                        "adb",
                        schedule,
                        output,
                        experiment,
                        experiment_path,
                        None,
                        None,
                        {},
                        environment_path,
                        {},
                        prepared,
                    )
            run_command.assert_not_called()
            copy_dataset.assert_not_called()
            copy_trace.assert_not_called()
            launch.assert_called_once()

    def test_nonzero_after_launched_without_evidence_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            phase = pathlib.Path(directory) / "phase.json"
            write_phase(phase, "launched")
            error = COLLECTOR.nonzero_collector_error(1, phase, "scalar")
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)
        self.assertEqual(
            COLLECTOR.terminal_for_error(error),
            ("Rejected", "integrity_rejected"),
        )

    def test_nonzero_in_explicit_evidence_phase_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            phase = pathlib.Path(directory) / "phase.json"
            write_phase(phase, "evidence")
            error = COLLECTOR.nonzero_collector_error(1, phase, "neon")
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)

    def test_post_launch_exactness_capacity_crash_and_timeout_are_rejected(self) -> None:
        for terminal_phase in ("launched", "evidence"):
            for failure in ("exactness", "capacity", "crash", "timeout"):
                with self.subTest(phase=terminal_phase, failure=failure):
                    with tempfile.TemporaryDirectory() as directory:
                        phase = pathlib.Path(directory) / "phase.json"
                        write_phase(phase, terminal_phase)
                        error = COLLECTOR.nonzero_collector_error(1, phase, failure)
                    self.assertEqual(
                        COLLECTOR.terminal_for_error(error),
                        ("Rejected", "integrity_rejected"),
                    )

    def test_nonzero_before_launch_without_explicit_environment_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            phase = pathlib.Path(directory) / "phase.json"
            write_phase(phase, "install")
            error = COLLECTOR.nonzero_collector_error(1, phase, "scalar")
        self.assertIsInstance(error, COLLECTOR.IntegrityRejectedError)

    def test_true_preflight_environment_exit_is_deferred(self) -> None:
        error = COLLECTOR.EnvironmentPrerequisiteError("adb device unavailable")
        self.assertEqual(
            COLLECTOR.terminal_for_error(error),
            ("Deferred", "environment_prerequisite"),
        )

    @staticmethod
    def _screen_runner(
        *,
        power: list[str],
        display: list[str],
        policy: list[str],
    ) -> tuple[object, list[tuple[str, ...]]]:
        calls: list[tuple[str, ...]] = []
        outputs = {"power": power, "display": display, "policy": policy}

        def run(args, **unused_kwargs):
            command = tuple(os.fspath(item) for item in args)
            calls.append(command)
            tail = command[-1]
            stdout = outputs[tail].pop(0) if tail in outputs else ""
            return subprocess.CompletedProcess(command, 0, stdout=stdout)

        return run, calls

    def test_q3_screen_ready_preflight_is_read_only_when_already_ready(self) -> None:
        run, calls = self._screen_runner(
            power=["mWakefulness=Awake\nDisplay Power: state=ON\n"],
            display=["mState=ON\n"],
            policy=["mKeyguardShowing=false\nisStatusBarKeyguard=false\n"],
        )
        with mock.patch.object(COLLECTOR.BASE, "run_command", side_effect=run):
            receipt = COLLECTOR.BASE.ensure_android_launch_screen_ready(
                "adb", "fixture-serial"
            )

        self.assertTrue(receipt["final"]["ready"])
        self.assertEqual(receipt["actions"], [])
        self.assertEqual(receipt["automatic_retries"], 0)
        self.assertFalse(receipt["credential_input_attempted"])
        self.assertEqual(len(calls), 3)
        self.assertTrue(all("dumpsys" in command for command in calls))

    def test_q3_screen_preflight_wakes_and_dismisses_at_most_once(self) -> None:
        run, calls = self._screen_runner(
            power=[
                "mWakefulness=Asleep\nDisplay Power: state=OFF\n",
                "mWakefulness=Awake\nDisplay Power: state=ON\n",
            ],
            display=["mState=OFF\n", "mState=ON\n"],
            policy=["mKeyguardShowing=true\n", "mKeyguardShowing=false\n"],
        )
        with mock.patch.object(COLLECTOR.BASE, "run_command", side_effect=run):
            receipt = COLLECTOR.BASE.ensure_android_launch_screen_ready(
                "adb", "fixture-serial"
            )

        self.assertEqual(
            receipt["actions"], ["keycode_wakeup", "wm_dismiss_keyguard"]
        )
        self.assertEqual(
            sum(command[-2:] == ("keyevent", "KEYCODE_WAKEUP") for command in calls),
            1,
        )
        self.assertEqual(
            sum(command[-2:] == ("wm", "dismiss-keyguard") for command in calls),
            1,
        )
        self.assertTrue(receipt["final"]["ready"])

    def test_q3_screen_preflight_does_not_bypass_a_secure_keyguard(self) -> None:
        run, calls = self._screen_runner(
            power=[
                "mWakefulness=Asleep\nDisplay Power: state=OFF\n",
                "mWakefulness=Awake\nDisplay Power: state=ON\n",
            ],
            display=["mState=OFF\n", "mState=ON\n"],
            policy=["mKeyguardShowing=true\n", "mKeyguardShowing=true\n"],
        )
        with (
            mock.patch.object(COLLECTOR.BASE, "run_command", side_effect=run),
            self.assertRaisesRegex(RuntimeError, "keyguard_locked=True"),
        ):
            COLLECTOR.BASE.ensure_android_launch_screen_ready(
                "adb", "fixture-serial"
            )

        flattened = " ".join(" ".join(command) for command in calls)
        self.assertNotIn(" input text ", flattened)
        self.assertNotIn(" swipe ", flattened)
        self.assertEqual(flattened.count("KEYCODE_WAKEUP"), 1)
        self.assertEqual(flattened.count("dismiss-keyguard"), 1)

    def test_q3_screen_readiness_error_is_classified_as_environment_prerequisite(self) -> None:
        args = argparse.Namespace(adb=None, serial="fixture-serial")
        environment = {
            "manufacturer": "Nothing",
            "model": "A065",
            "device": "pong",
            "device_properties": {
                "soc_model_property": {"value": "SM8475"},
            },
        }
        with (
            mock.patch.object(COLLECTOR.BASE, "resolve_adb", return_value="adb"),
            mock.patch.object(COLLECTOR.BASE, "device_info", return_value={}),
            mock.patch.object(
                COLLECTOR.BASE,
                "build_android_environment_receipt",
                return_value=environment,
            ),
            mock.patch.object(
                COLLECTOR.BASE,
                "ensure_android_launch_screen_ready",
                side_effect=RuntimeError("keyguard_locked=True"),
            ),
            self.assertRaisesRegex(
                COLLECTOR.EnvironmentPrerequisiteError,
                "before build/install/launch",
            ),
        ):
            COLLECTOR.preflight_a065(args)

    def test_q3_screen_failure_is_environment_prerequisite_before_output_claim(self) -> None:
        expected_commit = "1" * 40
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            output = root / "q3-output"
            args = argparse.Namespace(
                output=output,
                expected_commit=expected_commit,
                dry_run=False,
                matrix=COLLECTOR.MATRIX_PATH,
                seed=17,
                serial="fixture-serial",
                adb=None,
                run_timeout_seconds=10.0,
                warmup=COLLECTOR.DEFAULT_WARMUP,
                measured=COLLECTOR.DEFAULT_MEASURED,
                correctness_frames=COLLECTOR.DEFAULT_CORRECTNESS_FRAMES,
                max_thermal_status=0,
                thermal_timeout_seconds=1.0,
            )
            prerequisite = COLLECTOR.EnvironmentPrerequisiteError(
                "manually unlock the selected device"
            )
            with (
                mock.patch.object(
                    COLLECTOR,
                    "git_receipt",
                    return_value={"commit": expected_commit, "dirty": False},
                ),
                mock.patch.object(
                    COLLECTOR, "preflight_a065", side_effect=prerequisite
                ),
                mock.patch.object(COLLECTOR, "load_workloads") as load_workloads,
                self.assertRaises(COLLECTOR.EnvironmentPrerequisiteError),
            ):
                COLLECTOR.collect(args)

            load_workloads.assert_not_called()
            self.assertFalse(output.exists())
            self.assertEqual(list(root.glob(".q3-output.staging-*")), [])

    def test_q3_cli_names_prepublication_environment_prerequisite(self) -> None:
        error = COLLECTOR.EnvironmentPrerequisiteError("device is still locked")
        stderr = io.StringIO()
        with (
            mock.patch.object(COLLECTOR, "parse_args", return_value=object()),
            mock.patch.object(COLLECTOR, "collect", side_effect=error),
            contextlib.redirect_stderr(stderr),
        ):
            self.assertEqual(COLLECTOR.main([]), 1)
        self.assertIn("[EnvironmentPrerequisite]", stderr.getvalue())
        self.assertIn("device is still locked", stderr.getvalue())

    def test_base_collector_records_launched_and_evidence_from_control_flow(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            phase = pathlib.Path(directory) / "phase.json"
            write_phase(phase, "install")
            COLLECTOR.BASE.qualification_q3_phase_transition(phase, "launched")
            self.assertEqual(COLLECTOR.read_protocol_phase(phase)["phase"], "launched")
            COLLECTOR.BASE.qualification_q3_phase_transition(phase, "evidence")
            receipt = COLLECTOR.read_protocol_phase(phase)
        self.assertEqual(receipt["phase"], "evidence")
        self.assertEqual([item["phase"] for item in receipt["history"]][-2:], ["launched", "evidence"])

    def test_png_and_counts_are_only_the_scalar_image_control(self) -> None:
        scalar = {
            "lane": "scalar",
            "workload": "truck-050k",
            "semantic_fingerprint": {"image_sha256": "a" * 64, "frames": [1, 2]},
        }
        neon = {
            "lane": "neon",
            "workload": "truck-050k",
            "semantic_fingerprint": copy.deepcopy(scalar["semantic_fingerprint"]),
        }
        COLLECTOR.validate_image_control_pair(scalar, neon)
        neon["semantic_fingerprint"]["frames"][1] = 3
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError, "image control"
        ):
            COLLECTOR.validate_image_control_pair(scalar, neon)

    def test_physical_element_receipt_requires_every_parity_and_both_kernels(self) -> None:
        receipt = {
            "schema": COLLECTOR.PARITY_SCHEMA,
            "decision": "Accepted",
            "target_arch": "aarch64",
            "neon_required_by_target": True,
            "kernels": {
                "scalar": {"executed": True},
                "neon": {"executed": True},
            },
            "correctness": {
                "key": True,
                "source_id": True,
                "nan_bits": True,
                "boundary_bits": True,
                "fma_derived_key": True,
                "stable_tie": True,
            },
            "whole_plan_promotion": False,
        }
        COLLECTOR.validate_element_parity_receipt(receipt)
        receipt["correctness"]["stable_tie"] = False
        with self.assertRaisesRegex(COLLECTOR.IntegrityRejectedError, "parity is incomplete"):
            COLLECTOR.validate_element_parity_receipt(receipt)

    def test_whole_plan_decision_is_finite_without_tuning_threshold(self) -> None:
        def pairs(deltas: list[float]) -> list[dict]:
            return [
                {
                    "runs": {
                        "scalar": {
                            "distributions": {
                                "cpu_frame_complete_ms": {"mean": 10.0}
                            }
                        },
                        "neon": {
                            "distributions": {
                                "cpu_frame_complete_ms": {"mean": 10.0 + delta}
                            }
                        },
                    }
                }
                for delta in deltas
            ]

        self.assertEqual(
            COLLECTOR.workload_decision(pairs([-1.0, -0.5, -0.2, -0.3, -0.1]))[0],
            "Accepted",
        )
        self.assertEqual(
            COLLECTOR.workload_decision(pairs([-1.0, -0.5, 0.0, -0.3, -0.1]))[0],
            "Rejected",
        )

    def test_correctness_capture_accepts_absent_independent_order_timing(self) -> None:
        frames = [
            {
                "call_ms": 1.0,
                "frame_wall_ms": 2.0,
                "preprocess_ms": None,
                "sort_ms": None,
                "cpu_frame_complete_ms": None,
            }
        ]
        metrics = COLLECTOR.collected_run_metrics(frames, capture_png=True)
        self.assertEqual(set(metrics), {"call_ms", "frame_wall_ms"})

        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "timing run lacks required preprocess_ms",
        ):
            COLLECTOR.collected_run_metrics(frames, capture_png=False)

    def test_correctness_capture_rejects_partial_order_timing(self) -> None:
        frames = [
            {
                "call_ms": 1.0,
                "frame_wall_ms": 2.0,
                "preprocess_ms": 0.1,
                "sort_ms": None,
                "cpu_frame_complete_ms": None,
            },
            {
                "call_ms": 1.1,
                "frame_wall_ms": 2.1,
                "preprocess_ms": None,
                "sort_ms": None,
                "cpu_frame_complete_ms": None,
            },
        ]
        with self.assertRaisesRegex(
            COLLECTOR.IntegrityRejectedError,
            "preprocess_ms evidence is only partially available",
        ):
            COLLECTOR.collected_run_metrics(frames, capture_png=True)

    def test_collector_command_reuses_strict_android_collector(self) -> None:
        args = argparse.Namespace(
            serial="fixture-serial",
            max_thermal_status=0,
            thermal_timeout_seconds=300.0,
            run_timeout_seconds=1800.0,
            adb=None,
        )
        workload = COLLECTOR.Workload(
            "truck-050k",
            pathlib.Path("truck-050k.ply"),
            {"splat_count": 50_000},
            "scaling_tier",
        )
        command = COLLECTOR.collector_command(
            args,
            workload,
            pathlib.Path("trace.json"),
            pathlib.Path("scalar.apk"),
            pathlib.Path("fresh-output"),
            warmup=20,
            measured=80,
            capture_png=False,
            phase_receipt=pathlib.Path("phase.json"),
            prepared_inputs_receipt=pathlib.Path("prepared.json"),
            installed_apk_receipt=pathlib.Path("installed.json"),
            run_identity="1" * 32,
        )
        self.assertEqual(command[1], str(COLLECTOR.BASE_PATH))
        self.assertNotIn("--qualification-q3-capture-final-png", command)
        self.assertIn("--qualification-q3-phase-receipt", command)
        self.assertEqual(
            command[command.index("--qualification-q3-run-identity") + 1],
            "1" * 32,
        )
        self.assertIn("--qualification-q3-prepared-inputs", command)
        self.assertIn("--qualification-q3-installed-apk-receipt", command)
        self.assertEqual(command[command.index("--backend") + 1], "cpu")
        self.assertEqual(command[command.index("--geometry-path") + 1], "packed")
        self.assertNotIn("--prepare-apk", command)
        self.assertNotIn("--formal-artifact", command)
        control = COLLECTOR.collector_command(
            args,
            workload,
            pathlib.Path("trace.json"),
            pathlib.Path("scalar.apk"),
            pathlib.Path("fresh-control"),
            warmup=0,
            measured=2,
            capture_png=True,
            phase_receipt=pathlib.Path("control-phase.json"),
            prepared_inputs_receipt=pathlib.Path("prepared.json"),
            installed_apk_receipt=pathlib.Path("installed.json"),
            run_identity="2" * 32,
        )
        self.assertIn("--qualification-q3-capture-final-png", control)

    def test_private_capture_requests_png_without_changing_ordinary_mode(self) -> None:
        ordinary = argparse.Namespace(
            frames=2,
            warmup=0,
            yaw=0.001,
            sort_interval=1,
            async_sort=False,
            frame_latency=2,
            geometry_path="packed",
            gpu_producer=None,
            camera_trace=pathlib.Path("trace.json"),
            camera_frame=None,
            camera_frame_indices="0,1",
            formal_artifact=False,
            qualification_q3_capture_final_png=False,
        )
        q3 = copy.copy(ordinary)
        q3.qualification_q3_capture_final_png = True
        ordinary_command = COLLECTOR.BASE.benchmark_launch_args(ordinary, "cpu")
        q3_command = COLLECTOR.BASE.benchmark_launch_args(q3, "cpu")
        self.assertFalse(
            any(COLLECTOR.BASE.INTERNAL_FINAL_PNG in item for item in ordinary_command)
        )
        self.assertTrue(
            any(COLLECTOR.BASE.INTERNAL_FINAL_PNG in item for item in q3_command)
        )

    def test_failed_collection_command_is_attempted_once(self) -> None:
        args = argparse.Namespace(
            serial="fixture-serial",
            max_thermal_status=0,
            thermal_timeout_seconds=300.0,
            run_timeout_seconds=1800.0,
            adb=None,
        )
        workload = COLLECTOR.Workload(
            "truck-050k",
            pathlib.Path("truck-050k.ply"),
            {"splat_count": 50_000},
            "scaling_tier",
        )
        receipts = lane_receipts()
        receipts["scalar"]["apk_path"] = "build/scalar/sample-app-debug.apk"
        with tempfile.TemporaryDirectory() as directory:
            stage = pathlib.Path(directory)
            apk = stage / receipts["scalar"]["apk_path"]
            apk.parent.mkdir(parents=True)
            apk.write_bytes(b"fixture")
            prepared = prepared_workload(stage / "prepared.json")
            session = COLLECTOR.DeviceMatrixSession(
                "adb", args.serial, stage, receipts
            )
            installed = {
                "device_path": "/data/app/base.apk",
                **receipts["scalar"]["apk"],
                "run_as_verified": True,
            }
            with (
                mock.patch.object(COLLECTOR.BASE, "install_apk") as install,
                mock.patch.object(
                    COLLECTOR.BASE, "verify_installed_apk", return_value=installed
                ),
                mock.patch.object(
                    COLLECTOR.BASE,
                    "read_device_file_identity",
                    return_value={"bytes": 123, "sha256": "a" * 64},
                ) as reread_input,
                mock.patch.object(
                    COLLECTOR.subprocess,
                    "run",
                    side_effect=lambda *unused_args, **unused_kwargs: (
                        COLLECTOR.BASE.qualification_q3_phase_transition(
                            stage / "fixture-run-phase.json", "launched"
                        )
                        or subprocess.CompletedProcess([], 1, "", "activity crashed")
                    ),
                ) as run,
            ):
                with self.assertRaises(COLLECTOR.IntegrityRejectedError):
                    COLLECTOR.run_collection_once(
                        args,
                        stage,
                        {"commit": "1" * 40, "dirty": False},
                        receipts,
                        session,
                        prepared,
                        workload,
                        {},
                        pathlib.Path("trace.json"),
                        "scalar",
                        "fixture-run",
                        warmup=20,
                        measured=80,
                        capture_png=False,
                    )
            install.assert_called_once()
            reread_input.assert_not_called()
            run.assert_called_once()


if __name__ == "__main__":
    unittest.main()
