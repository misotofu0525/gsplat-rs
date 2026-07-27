#!/usr/bin/env python3
"""Collect the fixed Q3 Apple M4 Scalar/Neon diagnostic cell."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


SCHEMA = "gsplat-q3-simd-cell/v1"
CELL = "Q3.M4.PackedCpuExact.ScalarVsNeon"
DECISIONS = {"Accepted", "Rejected", "Deferred"}


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def git_receipt(root: Path) -> dict[str, Any]:
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    status = subprocess.run(
        ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
        cwd=root,
        check=True,
        capture_output=True,
    ).stdout
    return {
        "commit": commit,
        "dirty": bool(status),
        "status_porcelain_sha256": hashlib.sha256(status).hexdigest(),
    }


def apple_cpu_brand() -> str:
    try:
        result = subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError:
        return ""
    return result.stdout.strip() if result.returncode == 0 else ""


def host_deferral(system: str, machine: str, brand: str) -> str | None:
    if system != "Darwin":
        return "requires_physical_apple_m4_darwin"
    if machine not in {"arm64", "aarch64"}:
        return "requires_native_aarch64_not_rosetta"
    if brand != "Apple M4" and not brand.startswith("Apple M4 "):
        return "requires_physical_apple_m4_cpu"
    return None


def terminal_cell(decision: str, reason: str, *, brand: str) -> dict[str, Any]:
    if decision not in DECISIONS:
        raise ValueError(f"invalid Q3 decision: {decision}")
    return {
        "schema": SCHEMA,
        "cell": CELL,
        "decision": decision,
        "reason": reason,
        "host": {
            "system": platform.system(),
            "machine": platform.machine(),
            "cpu_brand": brand,
        },
        "evidence_scope": "microbenchmark_only",
        "whole_plan_promotion": False,
    }


def validate_receipt(receipt: dict[str, Any]) -> None:
    if receipt.get("schema") != SCHEMA:
        raise ValueError("unexpected Q3 SIMD receipt schema")
    if receipt.get("cell") != CELL:
        raise ValueError("unexpected Q3 SIMD cell identity")
    if receipt.get("decision") not in DECISIONS:
        raise ValueError("Q3 SIMD cell must be Accepted, Rejected or Deferred")
    if receipt.get("whole_plan_promotion") is not False:
        raise ValueError("microbenchmark receipt cannot promote a whole plan")


def collect(root: Path, brand: str) -> tuple[dict[str, Any], int]:
    reason = host_deferral(platform.system(), platform.machine(), brand)
    if reason is not None:
        return terminal_cell("Deferred", reason, brand=brand), 0

    build = git_receipt(root)
    if build["dirty"]:
        cell = terminal_cell(
            "Deferred", "requires_clean_exact_commit_for_correctness", brand=brand
        )
        cell["build"] = build
        return cell, 0

    with tempfile.TemporaryDirectory(prefix="gsplat-q3-m4-simd-") as temp_dir:
        receipt_path = Path(temp_dir) / "cell.json"
        environment = os.environ.copy()
        environment["GSPLAT_Q3_SIMD_RECEIPT"] = str(receipt_path)
        environment.setdefault("CARGO_BUILD_JOBS", "1")
        command = [
            "cargo",
            "test",
            "-p",
            "gsplat-sort",
            "--locked",
            "--release",
            "--lib",
            "cpu::q3_m4::q3_m4_forced_scalar_neon_microbenchmark",
            "--",
            "--exact",
            "--ignored",
        ]
        result = subprocess.run(
            command,
            cwd=root,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )
        if not receipt_path.is_file():
            cell = terminal_cell(
                "Deferred",
                "collector_command_failed_before_receipt",
                brand=brand,
            )
            cell["command_exit_code"] = result.returncode
            return cell, 1

        try:
            receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
            validate_receipt(receipt)
        except (OSError, ValueError, json.JSONDecodeError) as error:
            cell = terminal_cell("Rejected", "invalid_collector_receipt", brand=brand)
            cell["receipt_error"] = str(error)
            return cell, 1

        receipt["host"] = {
            "system": platform.system(),
            "machine": platform.machine(),
            "cpu_brand": brand,
        }
        receipt["build"] = build
        receipt["evidence_scope"] = "microbenchmark_only"
        if result.returncode != 0 and receipt["decision"] != "Rejected":
            receipt["decision"] = "Rejected"
            receipt["reason"] = "collector_test_failed_after_receipt"
        return receipt, 1 if receipt["decision"] == "Rejected" else 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        type=Path,
        help="also write the single canonical JSON cell to this path",
    )
    return parser.parse_args(argv)


def require_fresh_output(path: Path) -> None:
    if path.exists():
        raise ValueError(f"Q3 SIMD output already exists: {path}")


def publish_output(path: Path, contents: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        handle.write(contents + "\n")


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    if args.output is not None:
        require_fresh_output(args.output)
    brand = apple_cpu_brand()
    cell, exit_code = collect(repository_root(), brand)
    validate_receipt(cell)
    output = json.dumps(cell, sort_keys=True, separators=(",", ":"))
    if args.output is not None:
        publish_output(args.output, output)
    print(output)
    return exit_code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
