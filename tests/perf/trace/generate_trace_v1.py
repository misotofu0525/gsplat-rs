#!/usr/bin/env python3
"""Generate the deterministic gsplat-camera-trace/v1 contract fixture."""

from __future__ import annotations

import argparse
import json
import math
import pathlib

from trace_v1 import SCHEMA, mat4_multiply, projection_matrix, view_matrix, with_content_hash


def generate() -> dict:
    width = 640
    height = 360
    intrinsics = {
        "vertical_fov_radians": math.pi / 2.0,
        "near_plane": 0.1,
        "far_plane": 100.0,
    }
    projection = projection_matrix(
        intrinsics["vertical_fov_radians"],
        intrinsics["near_plane"],
        intrinsics["far_plane"],
        width / height,
    )
    positions = ([0.0, 0.0, -3.0], [0.25, 0.0, -3.0], [0.5, 0.125, -3.0])
    timestamps = (0, 16_666_667, 33_333_334)
    frames = []
    for index, (timestamp, position) in enumerate(zip(timestamps, positions, strict=True)):
        rotation = [0.0, 0.0, 0.0, 1.0]
        view = view_matrix(list(position), rotation)
        frames.append({
            "frame_index": index,
            "timestamp_ns": timestamp,
            "pose": {"position": list(position), "rotation_xyzw": rotation},
            "intrinsics": dict(intrinsics),
            "view_matrix": view,
            "projection_matrix": list(projection),
            "view_projection_matrix": mat4_multiply(projection, view),
        })
    return with_content_hash({
        "schema": SCHEMA,
        "trace_id": "contract-lateral-three-frame-v1",
        "coordinate_system": {
            "handedness": "right",
            "axes": "RUF",
            "camera_forward": "+Z",
        },
        "matrix_convention": {
            "storage_order": "row-major",
            "vector_convention": "column",
            "composition": "projection * view * world_position",
            "ndc_xy": "[-1,1]",
            "ndc_z": "[0,1]",
            "clip_w": "camera_z",
        },
        "display": {"width": width, "height": height},
        "frames": frames,
    })


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(generate(), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
