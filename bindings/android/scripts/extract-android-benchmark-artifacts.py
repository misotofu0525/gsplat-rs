#!/usr/bin/env python3
"""Extract gsplat-benchmark/v1 records from an Android logcat dump atomically."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


MANIFEST_PREFIX = "GSPLAT_BENCHMARK_MANIFEST "
FRAME_PREFIX = "GSPLAT_BENCHMARK_FRAME "
SUMMARY_PREFIX = "GSPLAT_BENCHMARK_SUMMARY "
CHUNK_PREFIX = "GSPLAT_BENCHMARK_CHUNK "
CHUNK_SHA256 = re.compile(r"[0-9a-f]{64}")
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
FORMAL_ANDROID_WIDTH = 2412
FORMAL_ANDROID_HEIGHT = 1080
DEVICE_PULL_RECEIPT_SCHEMA = "gsplat-android-device-png-pull/v1"
DEVICE_FINAL_PNG_PATH = "files/benchmark-final-frame.png"
CURRENT_STATS_VALIDATOR = pathlib.Path(__file__).with_name(
    "collect-android-sort-benchmarks.py"
)


def extract_payload(line: str, prefix: str) -> str | None:
    marker = line.find(prefix)
    if marker < 0:
        return None
    return line[marker + len(prefix) :]


def parse_chunk(payload: str) -> tuple[tuple[str, str, str, int], int, bytes]:
    metadata_text, separator, encoded = payload.partition(" payload=")
    if not separator or not encoded:
        raise ValueError("chunk is missing its payload")
    fields: dict[str, str] = {}
    for item in metadata_text.split():
        key, separator, value = item.partition("=")
        if not separator or not key or not value or key in fields:
            raise ValueError("chunk metadata is malformed")
        fields[key] = value
    required = {"record", "run_id", "index", "total", "encoding", "sha256"}
    if set(fields) != required:
        raise ValueError(f"chunk metadata fields differ: {sorted(fields)}")
    if fields["record"] not in {"manifest", "frame", "summary"}:
        raise ValueError("chunk record type is unknown")
    if fields["encoding"] != "base64":
        raise ValueError("chunk encoding is not base64")
    if CHUNK_SHA256.fullmatch(fields["sha256"]) is None:
        raise ValueError("chunk SHA-256 is malformed")
    try:
        index = int(fields["index"])
        total = int(fields["total"])
    except ValueError as error:
        raise ValueError("chunk index/total is not an integer") from error
    if total < 1 or total > 100_000 or index < 0 or index >= total:
        raise ValueError("chunk index/total is out of range")
    try:
        decoded = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        raise ValueError("chunk payload is invalid base64") from error
    if not decoded:
        raise ValueError("chunk payload is empty")
    key = (fields["record"], fields["run_id"], fields["sha256"], total)
    return key, index, decoded


def load_android_validator():
    spec = importlib.util.spec_from_file_location(
        "android_sort_collector_current_stats_validator",
        CURRENT_STATS_VALIDATOR,
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load Android current-stats artifact validator")
    validator = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = validator
    spec.loader.exec_module(validator)
    return validator


def attach_android_environment_receipt(
    staging: pathlib.Path, receipt_path: pathlib.Path
) -> None:
    manifest = json.loads((staging / "manifest.json").read_text(encoding="utf-8"))
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if not isinstance(receipt, dict):
        raise RuntimeError("Android environment receipt file must contain an object")
    validator = load_android_validator()
    validator.attach_android_environment_receipt(manifest, receipt)
    (staging / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def validate_declared_current_stats(staging: pathlib.Path) -> None:
    manifest = json.loads((staging / "manifest.json").read_text(encoding="utf-8"))
    renderer = manifest.get("renderer")
    if not isinstance(renderer, dict) or renderer.get("current_stats_strict") is not True:
        return

    summary = json.loads((staging / "summary.json").read_text(encoding="utf-8"))
    frames = [
        json.loads(line)
        for line in (staging / "frames.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    validator = load_android_validator()
    validator.validate_current_stats_evidence(
        manifest,
        summary,
        frames,
        renderer.get("order_backend_requested"),
    )


def png_dimensions(data: bytes) -> tuple[int, int]:
    if len(data) < 24 or data[:8] != PNG_SIGNATURE or data[12:16] != b"IHDR":
        raise RuntimeError("final image is not a PNG with an IHDR header")
    return int.from_bytes(data[16:20], "big"), int.from_bytes(data[20:24], "big")


def attach_device_pulled_final_png(
    staging: pathlib.Path,
    image_path: pathlib.Path,
    receipt_path: pathlib.Path,
) -> None:
    manifest = json.loads((staging / "manifest.json").read_text(encoding="utf-8"))
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if not isinstance(receipt, dict):
        raise RuntimeError("device PNG pull receipt must contain an object")
    required = {
        "schema": DEVICE_PULL_RECEIPT_SCHEMA,
        "source": "adb-exec-out-run-as-after-benchmark-complete",
        "device_path": DEVICE_FINAL_PNG_PATH,
        "package": "com.gsplat.example",
        "benchmark_completed": True,
        "device_path_absent_after_package_clear": True,
    }
    for field, expected in required.items():
        if receipt.get(field) != expected:
            raise RuntimeError(
                f"device PNG pull receipt {field} must equal {expected!r}"
            )
    run_id = manifest.get("run_id")
    if not isinstance(run_id, str) or not run_id:
        raise RuntimeError("benchmark manifest run_id is missing")
    if receipt.get("benchmark_run_id") != run_id:
        raise RuntimeError("device PNG pull receipt run_id does not match the benchmark")
    if receipt.get("pulled_after_completed_log") is not True:
        raise RuntimeError("device PNG was not pulled after the completed benchmark log")

    data = image_path.read_bytes()
    width, height = png_dimensions(data)
    if (width, height) != (FORMAL_ANDROID_WIDTH, FORMAL_ANDROID_HEIGHT):
        raise RuntimeError(
            "formal Android final PNG must be "
            f"{FORMAL_ANDROID_WIDTH}x{FORMAL_ANDROID_HEIGHT}, got {width}x{height}"
        )
    device_identity = receipt.get("device_identity")
    local_identity = receipt.get("local_identity")
    if not isinstance(device_identity, dict) or not isinstance(local_identity, dict):
        raise RuntimeError("device PNG pull receipt identities are missing")
    actual_sha256 = hashlib.sha256(data).hexdigest()
    actual_identity = {"bytes": len(data), "sha256": actual_sha256}
    if device_identity != actual_identity or {
        "bytes": local_identity.get("bytes"),
        "sha256": local_identity.get("sha256"),
    } != actual_identity:
        raise RuntimeError("device/local PNG bytes/hash do not match the pull receipt")
    if local_identity.get("width") != width or local_identity.get("height") != height:
        raise RuntimeError("device PNG dimensions do not match the pull receipt")

    destination = staging / "final-frame.png"
    destination.write_bytes(data)
    manifest["image"] = {
        "path": destination.name,
        "sha256": actual_sha256,
        "width": width,
        "height": height,
    }
    manifest["android_device_png_pull"] = receipt
    (staging / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--validator", type=pathlib.Path, required=True)
    parser.add_argument("--camera-trace", type=pathlib.Path)
    parser.add_argument("--camera-validator", type=pathlib.Path)
    parser.add_argument("--android-environment-receipt", type=pathlib.Path)
    parser.add_argument("--final-png", type=pathlib.Path)
    parser.add_argument("--device-png-pull-receipt", type=pathlib.Path)
    args = parser.parse_args()

    if (args.camera_trace is None) != (args.camera_validator is None):
        parser.error("--camera-trace and --camera-validator must be provided together")
    if args.camera_trace is not None and not args.camera_trace.is_file():
        parser.error(f"camera trace does not exist: {args.camera_trace}")
    if args.camera_validator is not None and not args.camera_validator.is_file():
        parser.error(f"camera validator does not exist: {args.camera_validator}")
    if (
        args.android_environment_receipt is not None
        and not args.android_environment_receipt.is_file()
    ):
        parser.error(
            "Android environment receipt does not exist: "
            f"{args.android_environment_receipt}"
        )
    if (args.final_png is None) != (args.device_png_pull_receipt is None):
        parser.error(
            "--final-png and --device-png-pull-receipt must be provided together"
        )
    if args.final_png is not None and not args.final_png.is_file():
        parser.error(f"final PNG does not exist: {args.final_png}")
    if (
        args.device_png_pull_receipt is not None
        and not args.device_png_pull_receipt.is_file()
    ):
        parser.error(
            "device PNG pull receipt does not exist: "
            f"{args.device_png_pull_receipt}"
        )

    if args.destination.exists():
        parser.error(f"destination already exists: {args.destination}")

    manifests: list[str] = []
    frames: list[str] = []
    summaries: list[str] = []
    chunk_groups: dict[tuple[str, str, str, int], dict[int, bytes]] = {}
    for line in args.log.read_text(encoding="utf-8", errors="replace").splitlines():
        if payload := extract_payload(line, MANIFEST_PREFIX):
            manifests.append(payload)
        elif payload := extract_payload(line, FRAME_PREFIX):
            frames.append(payload)
        elif payload := extract_payload(line, SUMMARY_PREFIX):
            summaries.append(payload)
        elif CHUNK_PREFIX in line:
            payload = extract_payload(line, CHUNK_PREFIX)
            assert payload is not None
            try:
                key, index, decoded = parse_chunk(payload)
            except ValueError as error:
                parser.error(f"invalid benchmark chunk: {error}")
            group = chunk_groups.setdefault(key, {})
            if index in group:
                parser.error(
                    f"duplicate benchmark chunk record={key[0]} "
                    f"run_id={key[1]} index={index}"
                )
            group[index] = decoded

    reconstructed: dict[str, list[str]] = {
        "manifest": manifests,
        "frame": frames,
        "summary": summaries,
    }
    for (record, run_id, expected_sha256, total), chunks in chunk_groups.items():
        missing = sorted(set(range(total)) - chunks.keys())
        if missing:
            parser.error(
                f"incomplete benchmark chunk record={record} run_id={run_id}: "
                f"missing={missing}"
            )
        raw = b"".join(chunks[index] for index in range(total))
        actual_sha256 = hashlib.sha256(raw).hexdigest()
        if actual_sha256 != expected_sha256:
            parser.error(
                f"benchmark chunk SHA-256 mismatch record={record} run_id={run_id}"
            )
        try:
            decoded = raw.decode("utf-8")
        except UnicodeDecodeError as error:
            parser.error(
                f"benchmark chunk is not UTF-8 record={record} run_id={run_id}: {error}"
            )
        reconstructed[record].append(decoded)

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
        if args.android_environment_receipt is not None:
            try:
                attach_android_environment_receipt(
                    staging, args.android_environment_receipt
                )
            except (OSError, json.JSONDecodeError, RuntimeError) as error:
                parser.error(f"invalid Android environment receipt: {error}")
        if args.final_png is not None:
            assert args.device_png_pull_receipt is not None
            try:
                attach_device_pulled_final_png(
                    staging, args.final_png, args.device_png_pull_receipt
                )
            except (OSError, json.JSONDecodeError, RuntimeError) as error:
                parser.error(f"invalid device-pulled final PNG: {error}")
        subprocess.run([sys.executable, str(args.validator), str(staging)], check=True)
        try:
            validate_declared_current_stats(staging)
        except (RuntimeError, ValueError) as error:
            parser.error(f"invalid strict Android current-stats artifact: {error}")
        if args.camera_trace is not None:
            assert args.camera_validator is not None
            subprocess.run(
                [
                    sys.executable,
                    str(args.camera_validator),
                    str(staging),
                    str(args.camera_trace),
                ],
                check=True,
            )
        os.rename(staging, args.destination)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    print(f"artifact_dir={args.destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
