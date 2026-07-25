#!/usr/bin/env python3
"""Extract base64 benchmark records from an iOS console log atomically."""

from __future__ import annotations

import argparse
import base64
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


PREFIX = "BENCHMARK_ARTIFACT "
RESULT_PREFIX = "BENCHMARK_RESULT "
RESULT_IDENTITY_PATTERN = re.compile(
    r"dataset=(?P<dataset>.+?) samples=(?P<samples>\d+)(?:\s|$)"
)
PROJECTED_VALIDATOR = pathlib.Path(__file__).with_name(
    "validate-ios-projected-artifacts.py"
)
CURRENT_STATS_VALIDATOR = pathlib.Path(__file__).with_name(
    "validate-ios-current-stats-artifacts.py"
)


def require_matching_benchmark_result(
    log: str, manifest: object, summary: object
) -> None:
    """Require one real iOS terminal and join it to the extracted artifacts."""
    result_payloads = [
        line.split(RESULT_PREFIX, 1)[1]
        for line in log.splitlines()
        if RESULT_PREFIX in line
    ]
    if len(result_payloads) != 1:
        raise ValueError(
            "log must contain exactly one BENCHMARK_RESULT "
            f"(found {len(result_payloads)})"
        )
    identity_text = result_payloads[0].split(" warmup=", 1)[0]
    match = RESULT_IDENTITY_PATTERN.fullmatch(identity_text)
    if match is None:
        raise ValueError("BENCHMARK_RESULT dataset/samples identity is malformed")
    terminal_dataset = match.group("dataset")
    terminal_samples = int(match.group("samples"))

    if not isinstance(manifest, dict) or not isinstance(summary, dict):
        raise ValueError("manifest and summary artifacts must be JSON objects")
    manifest_run_id = manifest.get("run_id")
    summary_run_id = summary.get("run_id")
    if (
        not isinstance(manifest_run_id, str)
        or not manifest_run_id
        or manifest_run_id != summary_run_id
    ):
        raise ValueError("manifest and summary run_id identity does not match")
    dataset = manifest.get("dataset")
    manifest_dataset = dataset.get("id") if isinstance(dataset, dict) else None
    if not isinstance(manifest_dataset, str) or terminal_dataset != manifest_dataset:
        raise ValueError("BENCHMARK_RESULT dataset does not match manifest identity")
    summary_samples = summary.get("sample_count")
    if type(summary_samples) is not int or terminal_samples != summary_samples:
        raise ValueError("BENCHMARK_RESULT samples does not match summary sample_count")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--validator", type=pathlib.Path, required=True)
    args = parser.parse_args()

    if args.destination.exists():
        parser.error(f"destination already exists: {args.destination}")
    log = args.log.read_text(encoding="utf-8", errors="replace")
    records: dict[str, list[bytes]] = {"manifest": [], "frame": [], "summary": []}
    parsed: dict[str, list[object]] = {"manifest": [], "frame": [], "summary": []}
    for line in log.splitlines():
        marker = line.find(PREFIX)
        if marker < 0:
            continue
        parts = line[marker + len(PREFIX):].split(" ", 1)
        if len(parts) != 2 or parts[0] not in records:
            continue
        try:
            payload = base64.b64decode(parts[1], validate=True)
            value = json.loads(payload)
        except (ValueError, json.JSONDecodeError) as error:
            parser.error(f"invalid {parts[0]} artifact payload: {error}")
        records[parts[0]].append(payload)
        parsed[parts[0]].append(value)
    if len(records["manifest"]) != 1 or len(records["summary"]) != 1 or not records["frame"]:
        parser.error("log must contain one manifest, one summary, and at least one frame")
    try:
        require_matching_benchmark_result(
            log, parsed["manifest"][0], parsed["summary"][0]
        )
    except ValueError as error:
        parser.error(str(error))

    args.destination.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(tempfile.mkdtemp(prefix=f".{args.destination.name}.", dir=args.destination.parent))
    try:
        (staging / "manifest.json").write_bytes(records["manifest"][0] + b"\n")
        (staging / "summary.json").write_bytes(records["summary"][0] + b"\n")
        (staging / "frames.jsonl").write_bytes(b"\n".join(records["frame"]) + b"\n")
        subprocess.run([sys.executable, str(args.validator), str(staging)], check=True)
        subprocess.run([sys.executable, str(PROJECTED_VALIDATOR), str(staging)], check=True)
        subprocess.run([sys.executable, str(CURRENT_STATS_VALIDATOR), str(staging)], check=True)
        os.rename(staging, args.destination)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    print(f"artifact_dir={args.destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
