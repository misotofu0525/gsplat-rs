#!/usr/bin/env python3
"""Build one immutable upstream Truck product-quality authority."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys

from q1_product_quality_authority import (
    AuthorityError,
    build_authority,
    validate_authority,
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=pathlib.Path, required=True)
    parser.add_argument("--source-dir", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    archive = pathlib.Path(os.path.abspath(args.archive))
    source_dir = pathlib.Path(os.path.abspath(args.source_dir))
    output = pathlib.Path(os.path.abspath(args.output))
    try:
        receipt = build_authority(archive, source_dir, output)
        validate_authority(output)
    except (AuthorityError, OSError) as error:
        print(f"product authority rejected: {error}", file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "status": "valid_product_quality_authority",
                "output": str(output),
                "authority_class": receipt["authority_class"],
                "views": [view["name"] for view in receipt["views"]],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
