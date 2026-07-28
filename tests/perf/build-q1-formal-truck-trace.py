#!/usr/bin/env python3
"""Build one immutable formal Truck Product Quality camera trace."""

from __future__ import annotations

import argparse
import json
import pathlib

from q1_formal_truck_trace_authority import FormalTraceError, build_output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-camera-authority", required=True, type=pathlib.Path)
    parser.add_argument("--evaluation-authority", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    try:
        trace, receipt = build_output(
            args.source_camera_authority,
            args.evaluation_authority,
            args.output,
        )
    except FormalTraceError as error:
        parser.error(str(error))
    print(
        json.dumps(
            {
                "status": "valid_formal_camera_trace_input",
                "output": str(args.output),
                "trace_id": trace["trace_id"],
                "content_sha256": trace["content_sha256"],
                "product_quality": receipt["qualification"]["product_quality"],
                "performance_authorized": False,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
