#!/usr/bin/env python3
"""Validate and summarize a five-pair Phase E benchmark series."""

from __future__ import annotations

import argparse
import json
import random
import statistics
from pathlib import Path
from typing import Any


P95_LIMIT = 1.10
P99_LIMIT = 1.20
SSIM_LIMIT = 0.99
BOOTSTRAP_SAMPLES = 100_000


def load(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object: {path}")
    return value


def load_frames(path: Path) -> list[dict[str, Any]]:
    frames = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                value = json.loads(line)
                require(isinstance(value, dict), f"expected frame object: {path}")
                frames.append(value)
    return frames


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * fraction)]


def bootstrap_median_ci(values: list[float]) -> list[float]:
    rng = random.Random(0x4753504C4154)
    count = len(values)
    medians = [
        statistics.median(values[rng.randrange(count)] for _ in range(count))
        for _ in range(BOOTSTRAP_SAMPLES)
    ]
    return [percentile(medians, 0.025), percentile(medians, 0.975)]


def matching_scope(left: dict[str, Any], right: dict[str, Any], pair_id: str) -> None:
    for key in ("dataset", "trace"):
        require(left[key] == right[key], f"{pair_id}: mismatched {key}")
    for key in ("width", "height", "dpr", "refresh_hz", "frame_budget_ms"):
        require(
            left["display"][key] == right["display"][key],
            f"{pair_id}: mismatched display.{key}",
        )
    require(left["renderer"]["backend"] == "webgpu", f"{pair_id}: PlayCanvas not WebGPU")
    require(right["renderer"]["backend"] == "webgpu", f"{pair_id}: gsplat-rs not WebGPU")
    require(
        left["renderer"]["implementation"] == "playcanvas-d5fe888"
        and left["renderer"]["path"] == "GSplatHybridRenderer"
        and left["renderer"]["sort_policy"] == "raster_gpu_sort",
        f"{pair_id}: PlayCanvas GPU-sort path receipt mismatch",
    )
    require(
        right["renderer"]["implementation"] == "gsplat-rs"
        and right["renderer"]["path"] == "wasm_sorted_index_direct",
        f"{pair_id}: gsplat-rs direct path receipt mismatch",
    )


def read_pair(pair_dir: Path) -> dict[str, Any]:
    pair_id = pair_dir.name
    pc_manifest = load(pair_dir / "playcanvas" / "manifest.json")
    gs_manifest = load(pair_dir / "gsplat-rs" / "manifest.json")
    pc_summary = load(pair_dir / "playcanvas" / "summary.json")
    gs_summary = load(pair_dir / "gsplat-rs" / "summary.json")
    pc_frames = load_frames(pair_dir / "playcanvas" / "frames.jsonl")
    gs_frames = load_frames(pair_dir / "gsplat-rs" / "frames.jsonl")
    image = load(pair_dir / "image-diff.json")

    pc_pairing = pc_manifest.get("pairing", {})
    gs_pairing = gs_manifest.get("pairing", {})
    require(pc_pairing.get("pair_id") == pair_id, f"{pair_id}: PlayCanvas pair id mismatch")
    require(gs_pairing.get("pair_id") == pair_id, f"{pair_id}: gsplat-rs pair id mismatch")
    order = pc_pairing.get("run_order")
    require(order == gs_pairing.get("run_order"), f"{pair_id}: run order mismatch")
    require(order in ("playcanvas-first", "gsplat-rs-first"), f"{pair_id}: invalid run order")
    expected_positions = (1, 2) if order == "playcanvas-first" else (2, 1)
    require(
        (pc_pairing.get("position"), gs_pairing.get("position")) == expected_positions,
        f"{pair_id}: positions do not match {order}",
    )
    matching_scope(pc_manifest, gs_manifest, pair_id)
    for label, summary in (("PlayCanvas", pc_summary), ("gsplat-rs", gs_summary)):
        require(summary["sample_count"] >= 3600, f"{pair_id}: {label} has fewer than 3600 samples")
        require(summary["warmup_count"] >= 120, f"{pair_id}: {label} has fewer than 120 warmups")
    expected_count = pc_manifest["dataset"]["splat_count"]
    for label, frames in (("PlayCanvas", pc_frames), ("gsplat-rs", gs_frames)):
        require(len(frames) == 3600, f"{pair_id}: {label} frame record count mismatch")
        require(
            all(frame["visible"] == expected_count and frame["drawn"] == expected_count for frame in frames),
            f"{pair_id}: {label} count receipt is not full-dataset parity",
        )

    pc_wall = pc_summary["distributions"]["frame_wall_ms"]
    gs_wall = gs_summary["distributions"]["frame_wall_ms"]
    p95_ratio = gs_wall["p95"] / pc_wall["p95"]
    p99_ratio = gs_wall["p99"] / pc_wall["p99"]
    require(image.get("metric") == "ssim-luma-srgb-window8", f"{pair_id}: wrong image metric")
    require(
        image.get("width") == 640 and image.get("height") == 480,
        f"{pair_id}: image dimensions mismatch",
    )
    require(image.get("pass") is True, f"{pair_id}: image gate failed")

    return {
        "pair_id": pair_id,
        "run_order": order,
        "playcanvas": {
            "p95_frame_wall_ms": pc_wall["p95"],
            "p99_frame_wall_ms": pc_wall["p99"],
            "missed_frames": pc_summary["missed_frame_count"],
        },
        "gsplat_rs": {
            "p95_frame_wall_ms": gs_wall["p95"],
            "p99_frame_wall_ms": gs_wall["p99"],
            "missed_frames": gs_summary["missed_frame_count"],
        },
        "ratios": {"p95": p95_ratio, "p99": p99_ratio},
        "ssim": image["score"],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("series_dir", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    pairs = [read_pair(path) for path in sorted(args.series_dir.glob("pair-*")) if path.is_dir()]
    require(len(pairs) >= 5, "paired comparison requires at least five pairs")
    p95 = [pair["ratios"]["p95"] for pair in pairs]
    p99 = [pair["ratios"]["p99"] for pair in pairs]
    p95_ci = bootstrap_median_ci(p95)
    p99_ci = bootstrap_median_ci(p99)
    aggregate = {
        "p95_ratio": {"median": statistics.median(p95), "bootstrap_95_ci": p95_ci, "limit": P95_LIMIT},
        "p99_ratio": {"median": statistics.median(p99), "bootstrap_95_ci": p99_ci, "limit": P99_LIMIT},
        "minimum_ssim": min(pair["ssim"] for pair in pairs),
        "ssim_limit": SSIM_LIMIT,
    }
    passed = (
        p95_ci[1] <= P95_LIMIT
        and p99_ci[1] <= P99_LIMIT
        and aggregate["minimum_ssim"] >= SSIM_LIMIT
    )
    output = {
        "schema": "gsplat-paired-comparison/v1",
        "series": "phase-e-paired-kitsune-static-v1",
        "bootstrap_samples": BOOTSTRAP_SAMPLES,
        "pairs": pairs,
        "aggregate": aggregate,
        "pass": passed,
        "claim_scope": "desktop-web-kitsune-static-only",
    }
    encoded = json.dumps(output, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
