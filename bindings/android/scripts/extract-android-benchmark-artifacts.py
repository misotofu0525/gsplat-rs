#!/usr/bin/env python3
"""Extract gsplat-benchmark/v1 records from an Android logcat dump atomically."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile


MANIFEST_PREFIX = "GSPLAT_BENCHMARK_MANIFEST "
FRAME_PREFIX = "GSPLAT_BENCHMARK_FRAME "
SUMMARY_PREFIX = "GSPLAT_BENCHMARK_SUMMARY "


def extract_payload(line: str, prefix: str) -> str | None:
    marker = line.find(prefix)
    if marker < 0:
        return None
    return line[marker + len(prefix) :]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--validator", type=pathlib.Path, required=True)
    args = parser.parse_args()

    if args.destination.exists():
        parser.error(f"destination already exists: {args.destination}")

    manifests: list[str] = []
    frames: list[str] = []
    summaries: list[str] = []
    for line in args.log.read_text(encoding="utf-8", errors="replace").splitlines():
        if payload := extract_payload(line, MANIFEST_PREFIX):
            manifests.append(payload)
        elif payload := extract_payload(line, FRAME_PREFIX):
            frames.append(payload)
        elif payload := extract_payload(line, SUMMARY_PREFIX):
            summaries.append(payload)

    if len(manifests) != 1 or len(summaries) != 1 or not frames:
        parser.error(
            "log must contain one manifest, one summary, and at least one frame "
            f"(found manifest={len(manifests)} frame={len(frames)} summary={len(summaries)})"
        )

    for label, payloads in (
        ("manifest", manifests),
        ("frame", frames),
        ("summary", summaries),
    ):
        for index, payload in enumerate(payloads):
            try:
                json.loads(payload)
            except json.JSONDecodeError as error:
                parser.error(f"invalid {label} payload at index {index}: {error}")

    args.destination.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(
        tempfile.mkdtemp(prefix=f".{args.destination.name}.", dir=args.destination.parent)
    )
    try:
        (staging / "manifest.json").write_text(manifests[0] + "\n", encoding="utf-8")
        (staging / "summary.json").write_text(summaries[0] + "\n", encoding="utf-8")
        (staging / "frames.jsonl").write_text("\n".join(frames) + "\n", encoding="utf-8")
        subprocess.run([sys.executable, str(args.validator), str(staging)], check=True)
        os.rename(staging, args.destination)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    print(f"artifact_dir={args.destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
