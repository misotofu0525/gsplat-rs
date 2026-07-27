#!/usr/bin/env python3
"""Offline admission and finite verdict for the Q1 Truck WebGPU comparison."""

from __future__ import annotations

import argparse
import pathlib
import sys

from q1_pair_admission import ValidationError, admission_rejection, evaluate, write_result
from q1_pair_admission.common import load_json


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("schedule", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args()
    series_id = None
    try:
        raw = load_json(args.schedule, "schedule")
        series_id = raw.get("series_id") if isinstance(raw.get("series_id"), str) else None
        write_result(args.output, evaluate(args.schedule))
        return 0
    except ValidationError as error:
        try:
            write_result(args.output, admission_rejection(series_id, str(error)))
        except ValidationError as output_error:
            print(f"admission rejected: {error}; cannot publish result: {output_error}", file=sys.stderr)
            return 2
        print(f"admission rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
