#!/usr/bin/env python3
"""Validate Android native runtime camera receipts against a trace file."""

from __future__ import annotations

import argparse
import importlib.util
import json
import pathlib
import sys


SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
COLLECTOR_PATH = SCRIPT_DIR / "collect-android-sort-benchmarks.py"
SPEC = importlib.util.spec_from_file_location("android_sort_collector", COLLECTOR_PATH)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact", type=pathlib.Path)
    parser.add_argument("trace", type=pathlib.Path)
    args = parser.parse_args()

    manifest = json.loads((args.artifact / "manifest.json").read_text(encoding="utf-8"))
    frames = COLLECTOR.read_artifact_frames(args.artifact / "frames.jsonl")
    expected_trace = json.loads(args.trace.read_text(encoding="utf-8"))
    COLLECTOR.validate_camera_receipts(
        manifest,
        frames,
        expected_trace,
        COLLECTOR.local_file_identity(args.trace),
    )
    print(
        f"android camera receipts valid: {len(frames)} frame(s), "
        f"trace={expected_trace['trace_id']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
