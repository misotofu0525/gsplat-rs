#!/usr/bin/env python3
"""Build a pinned, extracted-entry Q1 evaluation image authority."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

from q1_product_quality_evaluation_authority import (
    EvaluationAuthorityError,
    build_authority,
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    arguments = parser.parse_args()
    try:
        receipt = build_authority(arguments.source_dir, arguments.output)
    except (EvaluationAuthorityError, OSError) as error:
        print(f"evaluation authority rejected: {error}", file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "status": "valid_upstream_evaluation_authority",
                "authority_class": receipt["authority_class"],
                "output": str(arguments.output.resolve()),
                "performance_authorized": False,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
