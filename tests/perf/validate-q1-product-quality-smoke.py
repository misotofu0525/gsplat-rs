#!/usr/bin/env python3
"""Validate and atomically publish Q1 view 000001 Product Quality evidence."""

from __future__ import annotations

import argparse
import json
import pathlib

from q1_product_quality_smoke import (
    OneViewQualityError,
    evaluate_one_view,
    publish_one_view,
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--formal-trace-authority", required=True, type=pathlib.Path)
    parser.add_argument("--evaluation-authority", required=True, type=pathlib.Path)
    parser.add_argument("--gsplat-capture", required=True, type=pathlib.Path)
    parser.add_argument("--playcanvas-capture", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    try:
        result = evaluate_one_view(
            formal_trace_authority=args.formal_trace_authority,
            evaluation_authority=args.evaluation_authority,
            gsplat_capture=args.gsplat_capture,
            playcanvas_capture=args.playcanvas_capture,
        )
        publish_one_view(result, args.output)
    except OneViewQualityError as error:
        parser.error(str(error))
    print(
        json.dumps(
            {
                "status": result["status"],
                "view_id": result["view_id"],
                "output": str(args.output),
                "product_quality": result["qualification"]["product_quality"],
                "performance_eligible": False,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
