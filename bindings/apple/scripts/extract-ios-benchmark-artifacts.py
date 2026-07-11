#!/usr/bin/env python3
"""Extract base64 benchmark records from an iOS console log atomically."""

from __future__ import annotations

import argparse
import base64
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile


PREFIX = "BENCHMARK_ARTIFACT "


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--validator", type=pathlib.Path, required=True)
    args = parser.parse_args()

    if args.destination.exists():
        parser.error(f"destination already exists: {args.destination}")
    records: dict[str, list[bytes]] = {"manifest": [], "frame": [], "summary": []}
    for line in args.log.read_text(encoding="utf-8", errors="replace").splitlines():
        marker = line.find(PREFIX)
        if marker < 0:
            continue
        parts = line[marker + len(PREFIX):].split(" ", 1)
        if len(parts) != 2 or parts[0] not in records:
            continue
        try:
            payload = base64.b64decode(parts[1], validate=True)
            json.loads(payload)
        except (ValueError, json.JSONDecodeError) as error:
            parser.error(f"invalid {parts[0]} artifact payload: {error}")
        records[parts[0]].append(payload)
    if len(records["manifest"]) != 1 or len(records["summary"]) != 1 or not records["frame"]:
        parser.error("log must contain one manifest, one summary, and at least one frame")

    args.destination.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(tempfile.mkdtemp(prefix=f".{args.destination.name}.", dir=args.destination.parent))
    try:
        (staging / "manifest.json").write_bytes(records["manifest"][0] + b"\n")
        (staging / "summary.json").write_bytes(records["summary"][0] + b"\n")
        (staging / "frames.jsonl").write_bytes(b"\n".join(records["frame"]) + b"\n")
        subprocess.run([sys.executable, str(args.validator), str(staging)], check=True)
        os.rename(staging, args.destination)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    print(f"artifact_dir={args.destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
