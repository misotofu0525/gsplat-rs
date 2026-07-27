#!/usr/bin/env python3
"""Collect a bounded, paired Balanced timing experiment after image admission.

This is deliberately narrower than a general benchmark runner.  It compares
the two binaries of one already image-qualified B1/B2/B3 experiment on the
complete Truck scene and the committed moving 1920x1080 trace.  The collector
owns the randomized, counterbalanced schedule and validates every terminal
Surface receipt before publishing a fresh directory.  It does not decide that
the candidate is good: it reports one of ``candidate``, ``exact`` or
``inconclusive`` from the retained paired deltas.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import random
import shutil
import subprocess
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence


REPO_ROOT = Path(__file__).resolve().parents[2]
B1_PATH = REPO_ROOT / "tests/perf/collect-balanced-desktop-b1.py"
MATRIX_PATH = REPO_ROOT / "tests/perf/full-quality-matrix-plan-v1.json"
QUALITY_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-balanced-image-gate.py"
BENCHMARK_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
SCHEMA = "gsplat-balanced-paired-timing/v1"
FORMAL_SIZE = (1920, 1080)
DEFAULT_WARMUP = 20
DEFAULT_MEASURED = 80
DEFAULT_PAIRS = 3
RUN_TIMEOUT_SECONDS = 30 * 60


def load_module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


B1 = load_module("collect_balanced_desktop_b1_for_timing", B1_PATH)
M2B = B1.M2B
ValidationError = M2B.ValidationError
require = M2B.require
sha256_file = M2B.sha256_file
write_json = M2B.write_json
git_receipt = M2B.git_receipt
validate_ignored_output = M2B.validate_ignored_output
distribution = M2B.distribution


@dataclass(frozen=True)
class Workload:
    dataset: dict[str, Any]
    dataset_path: Path
    trace: dict[str, Any]
    trace_path: Path


def require_object(value: Any, context: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{context} must be an object")
    return value


def require_string(value: Any, context: str) -> str:
    require(isinstance(value, str) and value, f"{context} must be a non-empty string")
    return value


def load_trace_identity(trace_path: Path, trace_entry: dict[str, Any]) -> tuple[dict[str, Any], str]:
    """Join the matrix's canonical content identity to the exact file bytes."""

    file_sha256 = sha256_file(trace_path)
    trace_value = require_object(
        json.loads(trace_path.read_text(encoding="utf-8")), "Truck trace"
    )
    require(
        trace_value.get("content_sha256") == trace_entry.get("sha256"),
        "Truck trace content SHA-256 mismatch",
    )
    return trace_value, file_sha256


def load_workload(repo: Path, matrix_path: Path = MATRIX_PATH) -> Workload:
    """Pin the B0 workload to the full Truck, not a ladder subset."""

    matrix = require_object(json.loads(matrix_path.read_text(encoding="utf-8")), "matrix")
    datasets = matrix.get("datasets")
    traces = matrix.get("traces")
    require(isinstance(datasets, list) and isinstance(traces, list), "matrix lacks datasets/traces")
    dataset = next(
        (item for item in datasets if isinstance(item, dict) and item.get("id") == "truck-full"),
        None,
    )
    trace_entry = next(
        (
            item
            for item in traces
            if isinstance(item, dict)
            and item.get("id") == "candidate-truck-quality-2view-1920x1080-v1"
        ),
        None,
    )
    dataset = require_object(dataset, "matrix truck-full dataset")
    trace_entry = require_object(trace_entry, "matrix Truck desktop trace")
    require(dataset.get("role") == "full_scene", "Balanced timing requires full Truck")
    require(trace_entry.get("dataset_id") == "truck-full", "Truck trace dataset mismatch")
    require((trace_entry.get("width"), trace_entry.get("height")) == FORMAL_SIZE,
            "Balanced timing requires 1920x1080 Truck trace")
    dataset_path = (repo / require_string(dataset.get("local_path"), "dataset.local_path")).resolve()
    trace_path = (repo / require_string(trace_entry.get("local_path"), "trace.local_path")).resolve()
    require(dataset_path.is_file(), f"full Truck dataset is unavailable: {dataset_path}")
    require(trace_path.is_file(), f"Truck trace is unavailable: {trace_path}")
    require(sha256_file(dataset_path) == dataset.get("sha256"), "full Truck dataset SHA-256 mismatch")
    require(dataset_path.stat().st_size == dataset.get("bytes"), "full Truck dataset byte count mismatch")
    M2B.run_trace_validator(repo, trace_path)
    trace_value, trace_file_sha256 = load_trace_identity(trace_path, trace_entry)
    require(trace_value.get("trace_id") == trace_entry["id"], "Truck trace identity mismatch")
    require(trace_value.get("content_sha256"), "Truck trace content hash is unavailable")
    frames = trace_value.get("frames")
    require(isinstance(frames, list) and len(frames) >= 2, "Truck trace is not moving")
    frame_indices = tuple(frame.get("frame_index") for frame in frames if isinstance(frame, dict))
    frame_timestamps = tuple(frame.get("timestamp_ns") for frame in frames if isinstance(frame, dict))
    require(len(frame_indices) == len(frames) == len(frame_timestamps), "Truck trace frames are malformed")
    derivation = require_object(trace_value.get("derivation"), "Truck trace derivation")
    require(derivation.get("source_sha256") == dataset["sha256"], "Truck trace source SHA mismatch")
    require(derivation.get("source_splat_count") == dataset["splat_count"], "Truck trace source count mismatch")
    require(derivation.get("source_sh_degree") == dataset["sh_degree"], "Truck trace source SH mismatch")
    return Workload(
        dataset=dataset,
        dataset_path=dataset_path,
        trace={
            "trace_id": trace_value["trace_id"],
            "content_sha256": trace_value["content_sha256"],
            "file_sha256": trace_file_sha256,
            "width": FORMAL_SIZE[0],
            "height": FORMAL_SIZE[1],
            "frame_indices": frame_indices,
            "frame_timestamps_ns": frame_timestamps,
            "path": str(trace_path),
        },
        trace_path=trace_path,
    )


def schedule_pairs(pairs: int, seed: int) -> list[tuple[str, str]]:
    """Counterbalance lanes before a seeded shuffle; no lane is always first."""

    require(pairs >= DEFAULT_PAIRS, f"Balanced timing requires at least {DEFAULT_PAIRS} pairs")
    forward = ("exact", "candidate")
    reverse = tuple(reversed(forward))
    schedule: list[tuple[str, str]] = [forward] * (pairs // 2) + [reverse] * (pairs // 2)
    if pairs % 2:
        bit = hashlib.sha256(f"gsplat-balanced-timing/{seed}".encode("ascii")).digest()[0] & 1
        schedule.append(forward if bit == 0 else reverse)
    random.Random(seed).shuffle(schedule)
    imbalance = abs(sum(order == forward for order in schedule) - sum(order == reverse for order in schedule))
    require(imbalance <= 1, "counterbalanced schedule is invalid")
    return schedule


def quality_suite_receipt(path: Path, experiment: Any, commit: str) -> dict[str, Any]:
    """Require an image gate from this exact source before timing can start."""

    path = path.resolve()
    require(path.is_file(), f"quality suite is unavailable: {path}")
    result = subprocess.run(
        [sys.executable, str(QUALITY_VALIDATOR_PATH), str(path)],
        cwd=REPO_ROOT,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    detail = result.stderr.strip() or result.stdout.strip()
    require(result.returncode == 0, f"quality suite validator rejected input: {detail}")
    suite = require_object(json.loads(path.read_text(encoding="utf-8")), "quality suite")
    experiment_value = require_object(suite.get("experiment"), "quality suite experiment")
    require(experiment_value.get("name") == experiment.name, "quality suite experiment mismatch")
    collector = require_object(suite.get("collector"), "quality suite collector")
    require(collector.get("repository_commit") == commit,
            "quality suite commit differs from timing candidate")
    return {"path": str(path), "sha256": sha256_file(path), "commit": commit}


def make_command(binary: Path, workload: Workload, *, warmup: int, measured: int) -> list[str]:
    return [
        str(binary), str(workload.dataset_path),
        "--geometry-path", "packed", "--interactive",
        "--camera-trace", str(workload.trace_path), "--camera-sequence",
        "--camera-warmup-frames", str(warmup),
        "--camera-measured-frames", str(measured), "--camera-loops", "1",
        "--surface-benchmark-mode", "isolated",
        "--surface-sort-policy", "every-frame",
        "--surface-evidence-plan", B1.PLAN_CLI,
        "--surface-diagnostic-capture-receipt",
        "--png", M2B.FINAL_CAPTURE_IDENTITY,
    ]


def parse_diagnostic_capture(stdout: str, stderr: str, *, lane: Any, capture: dict[str, str], workload: Workload) -> dict[str, Any]:
    prefix = "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT "
    records: list[dict[str, str]] = []
    for stream in (stdout, stderr):
        for line in stream.splitlines():
            if line.startswith(prefix):
                records.append(M2B.parse_payload(line[len(prefix):], "diagnostic capture"))
    require(len(records) == 1, "expected exactly one diagnostic capture receipt")
    record = records[0]
    M2B.assert_fields(record, {
        "depth_precision_profile": lane.profile,
        "projected_cache_precision_profile": lane.projected_cache_profile,
        "resident_sh_codec_profile": lane.resident_sh_codec_profile,
        "resident_sh_source_count": str(workload.dataset["splat_count"]),
        "resident_sh_encoded_count": str(workload.dataset["splat_count"]),
        "resident_sh_resident_count": str(workload.dataset["splat_count"]),
        "resident_sh_addressable_count": str(workload.dataset["splat_count"]),
        "resident_sh_source_degree": str(workload.dataset["sh_degree"]),
        "resident_sh_resident_degree": str(workload.dataset["sh_degree"]),
        "plan_id": B1.PLAN_CAPTURE_RECEIPT,
        "width": str(workload.trace["width"]),
        "height": str(workload.trace["height"]),
    }, "diagnostic capture")
    for key, capture_key in (
        ("scene_generation", "scene_generation"),
        ("camera_revision", "camera_revision"),
        ("viewport_generation", "viewport_generation"),
        ("contract_generation", "contract_generation"),
        ("plan_set_generation", "plan_set_generation"),
        ("order_generation", "order_generation"),
        ("presentation_sequence", "presentation_sequence"),
    ):
        require(record.get(key) == capture.get(capture_key), f"diagnostic capture {key} did not join final frame")
    return record


def verdict(pair_results: Sequence[dict[str, Any]]) -> str:
    deltas = [pair["candidate_minus_exact_frame_wall_ms"] for pair in pair_results]
    if all(delta < 0.0 for delta in deltas):
        return "candidate"
    if all(delta > 0.0 for delta in deltas):
        return "exact"
    return "inconclusive"


def remove_private_builds(stage: Path, lanes: Sequence[Any]) -> None:
    """Drop only the collector-owned build intermediates before publication."""

    target = stage / "cargo-target"
    require(target.is_dir() and not target.is_symlink(), "private Cargo target is unavailable")
    shutil.rmtree(target)
    for lane in lanes:
        binary = stage / "build" / lane.name / "desktop-example-bin"
        require(binary.is_file() and not binary.is_symlink(), f"{lane.name} retained binary is unavailable")
        binary.unlink()


def collect(args: argparse.Namespace, repo: Path = REPO_ROOT) -> dict[str, Any]:
    experiment = B1.EXPERIMENTS[args.experiment]
    require(args.pairs >= DEFAULT_PAIRS, "at least three paired runs are required")
    require(args.warmup >= 0 and args.measured > 0, "invalid warmup/measured schedule")
    require(math.isfinite(args.refresh_hz) and args.refresh_hz > 0.0, "refresh-hz must be positive")
    output = args.output.resolve()
    require(not output.exists(), f"output already exists: {output}")
    validate_ignored_output(repo, output)
    initial_git = git_receipt(repo)
    require(not initial_git["dirty"], "formal Balanced timing requires a clean repository")
    quality = quality_suite_receipt(args.quality_suite, experiment, initial_git["commit"])
    workload = load_workload(repo, args.matrix.resolve())
    schedule = schedule_pairs(args.pairs, args.seed)
    stage = output.parent / f".{output.name}.stage-{os.getpid()}-{uuid.uuid4().hex[:10]}"
    require(not stage.exists(), f"staging path exists: {stage}")
    stage.mkdir(parents=True)
    runs: list[dict[str, Any]] = []
    pair_results: list[dict[str, Any]] = []
    result: dict[str, Any] = {
        "schema": SCHEMA, "status": "running", "experiment": experiment.name,
        "quality_gate": quality, "workload": {"dataset": workload.dataset["id"], "trace": workload.trace["trace_id"]},
        "config": {"pairs": args.pairs, "seed": args.seed, "warmup": args.warmup, "measured": args.measured, "refresh_hz": args.refresh_hz, "order": "counterbalanced_seeded"},
        "schedule": [{"pair_index": index + 1, "order": list(order)} for index, order in enumerate(schedule)],
        "runs": runs, "pairs": pair_results,
    }
    try:
        binaries, lane_builds = B1.build_desktop_binaries(repo, stage, initial_git, lanes=experiment.lanes)
        build = {"git": initial_git, "package_version": B1.package_version(repo)}
        for pair_index, order in enumerate(schedule, 1):
            pair_runs: dict[str, dict[str, Any]] = {}
            for position, lane_name in enumerate(order, 1):
                lane = next(lane for lane in experiment.lanes if lane.name == lane_name)
                run_dir = stage / f"pair-{pair_index:03d}" / f"{position:02d}-{lane_name}"
                artifact = run_dir / "artifact"
                artifact.mkdir(parents=True)
                command = make_command(binaries[lane_name], workload, warmup=args.warmup, measured=args.measured)
                write_json(run_dir / "command.json", {"argv": command})
                started_at = M2B.utc_now()
                completed = subprocess.run(command, cwd=artifact, check=False, text=True,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=RUN_TIMEOUT_SECONDS)
                ended_at = M2B.utc_now()
                (run_dir / "stdout.log").write_text(completed.stdout, encoding="utf-8")
                (run_dir / "stderr.log").write_text(completed.stderr, encoding="utf-8")
                require(completed.returncode == 0, f"pair {pair_index} {lane_name} exited with {completed.returncode}")
                require(sha256_file(binaries[lane_name]) == lane_builds[lane_name]["binary_sha256"], "candidate binary changed during collection")
                require(sha256_file(workload.dataset_path) == workload.dataset["sha256"], "Truck dataset changed during collection")
                require(sha256_file(workload.trace_path) == workload.trace["file_sha256"], "Truck trace changed during collection")
                require(git_receipt(repo) == initial_git, "source changed during collection")
                validated = M2B.validate_run_log(completed.stdout, completed.stderr, arm="gpu_post_sort", dataset=workload.dataset, trace=workload.trace, warmup=args.warmup, measured=args.measured, capture_path=artifact / M2B.FINAL_CAPTURE_IDENTITY)
                capture = M2B.only(M2B.parse_log(completed.stdout, completed.stderr)["capture"], "final capture")
                diagnostic = parse_diagnostic_capture(completed.stdout, completed.stderr, lane=lane, capture=capture, workload=workload)
                M2B.build_artifact(artifact, arm="gpu_post_sort", validated=validated, dataset=workload.dataset, trace=workload.trace, build={**build, "binary_sha256": lane_builds[lane_name]["binary_sha256"]}, capture_path=artifact / M2B.FINAL_CAPTURE_IDENTITY, started_at=started_at, ended_at=ended_at, warmup=args.warmup, refresh_hz=args.refresh_hz, host_device=B1.host_device_name())
                manifest_path = artifact / "manifest.json"
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                manifest["renderer"]["balanced_experiment"] = experiment.name
                manifest["renderer"]["depth_precision_profile"] = diagnostic["depth_precision_profile"]
                manifest["renderer"]["projected_cache_precision_profile"] = diagnostic["projected_cache_precision_profile"]
                manifest["renderer"]["resident_sh_codec_profile"] = diagnostic["resident_sh_codec_profile"]
                write_json(manifest_path, manifest)
                checked = subprocess.run([sys.executable, str(BENCHMARK_VALIDATOR_PATH), str(artifact)], cwd=repo, check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                (run_dir / "benchmark-validator.stdout.log").write_text(checked.stdout, encoding="utf-8")
                (run_dir / "benchmark-validator.stderr.log").write_text(checked.stderr, encoding="utf-8")
                require(checked.returncode == 0, f"pair {pair_index} {lane_name} artifact validator rejected evidence")
                receipt = {"lane": lane_name, "pair_index": pair_index, "position": position, "artifact": artifact.relative_to(stage).as_posix(), "binary_sha256": lane_builds[lane_name]["binary_sha256"], "frame_wall_ms": validated["frames"][-1]["frame_wall_ms"], "frame_wall_distribution": distribution([frame["frame_wall_ms"] for frame in validated["frames"]]), "diagnostic_capture": diagnostic}
                write_json(run_dir / "run.json", receipt)
                runs.append(receipt)
                pair_runs[lane_name] = receipt
            require(set(pair_runs) == {"exact", "candidate"}, f"pair {pair_index} is incomplete")
            exact = pair_runs["exact"]["frame_wall_distribution"]["mean"]
            candidate = pair_runs["candidate"]["frame_wall_distribution"]["mean"]
            pair_results.append({"pair_index": pair_index, "order": list(order), "exact_frame_wall_mean_ms": exact, "candidate_frame_wall_mean_ms": candidate, "candidate_minus_exact_frame_wall_ms": candidate - exact})
        result.update({"status": "ok", "winner": verdict(pair_results), "aggregate": {"paired_deltas_frame_wall_ms": [pair["candidate_minus_exact_frame_wall_ms"] for pair in pair_results]}})
        remove_private_builds(stage, experiment.lanes)
        write_json(stage / "experiment.json", result)
        require(not output.exists(), "output appeared during collection")
        os.rename(stage, output)
        return result
    except Exception as error:
        result.update({"status": "failed", "error": str(error)})
        write_json(stage / "experiment.json", result)
        failed = output.parent / f"{output.name}.failed-{os.getpid()}-{uuid.uuid4().hex[:8]}"
        os.replace(stage, failed)
        raise ValidationError(f"{error}; retained_failed_evidence={failed}") from error


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect paired B1/B2/B3 Truck timing after image admission")
    parser.add_argument("--experiment", choices=tuple(B1.EXPERIMENTS), required=True)
    parser.add_argument("--quality-suite", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--matrix", type=Path, default=MATRIX_PATH)
    parser.add_argument("--pairs", type=int, default=DEFAULT_PAIRS)
    parser.add_argument("--seed", type=int, default=0x4753504C4154)
    parser.add_argument("--warmup", type=int, default=DEFAULT_WARMUP)
    parser.add_argument("--measured", type=int, default=DEFAULT_MEASURED)
    parser.add_argument("--refresh-hz", type=float, default=60.0)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        result = collect(args)
    except (OSError, subprocess.SubprocessError, ValidationError, ValueError) as error:
        print(f"Balanced paired timing failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"status": result["status"], "winner": result["winner"], "output": str(args.output)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
