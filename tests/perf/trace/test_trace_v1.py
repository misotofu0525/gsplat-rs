#!/usr/bin/env python3
"""Focused focal-ratio tests for the canonical camera trace validator."""

from __future__ import annotations

import copy
import json
import pathlib
import unittest

from trace_v1 import (
    MAX_FOCAL_LENGTH_X_OVER_Y,
    MIN_FOCAL_LENGTH_X_OVER_Y,
    mat4_multiply,
    projection_matrix,
    with_content_hash,
)
from validate_trace_v1 import ValidationError, validate


FIXTURE = json.loads(
    (pathlib.Path(__file__).parent / "fixtures/camera-trace-v1.json").read_text(encoding="utf-8")
)


def with_focal_ratio(ratio: object) -> dict:
    trace = copy.deepcopy(FIXTURE)
    aspect = trace["display"]["width"] / trace["display"]["height"]
    for frame in trace["frames"]:
        intrinsics = frame["intrinsics"]
        intrinsics["focal_length_x_over_y"] = ratio
        if isinstance(ratio, (int, float)) and not isinstance(ratio, bool):
            projection = projection_matrix(
                intrinsics["vertical_fov_radians"],
                intrinsics["near_plane"],
                intrinsics["far_plane"],
                aspect,
                float(ratio),
            )
            frame["projection_matrix"] = projection
            frame["view_projection_matrix"] = mat4_multiply(projection, frame["view_matrix"])
    return with_content_hash(trace)


class CameraTraceFocalRatioTests(unittest.TestCase):
    def test_legacy_trace_defaults_to_one_without_hash_or_content_drift(self) -> None:
        validate(copy.deepcopy(FIXTURE))
        self.assertNotIn("focal_length_x_over_y", FIXTURE["frames"][0]["intrinsics"])
        legacy = projection_matrix(1.0, 0.1, 100.0, 16.0 / 9.0)
        explicit = projection_matrix(1.0, 0.1, 100.0, 16.0 / 9.0, 1.0)
        self.assertEqual(legacy, explicit)

    def test_exact_centered_focal_ratio_recomputes_projection(self) -> None:
        ratio = 581.9245675736333 / 578.6701201866216
        trace = with_focal_ratio(ratio)
        validate(trace)
        self.assertAlmostEqual(
            trace["frames"][0]["projection_matrix"][0],
            ratio / (640.0 / 360.0),
            places=15,
        )

    def test_focal_ratio_matrix_mutation_fails_closed(self) -> None:
        trace = with_focal_ratio(1.125)
        trace["frames"][0]["projection_matrix"][0] += 1.0e-6
        trace = with_content_hash(trace)
        with self.assertRaisesRegex(ValidationError, r"projection_matrix\[0\] mismatch"):
            validate(trace)

    def test_bounds_are_inclusive_and_invalid_values_fail_closed(self) -> None:
        validate(with_focal_ratio(MIN_FOCAL_LENGTH_X_OVER_Y))
        validate(with_focal_ratio(MAX_FOCAL_LENGTH_X_OVER_Y))
        for ratio in [
            0.0,
            -1.0,
            MIN_FOCAL_LENGTH_X_OVER_Y / 2.0,
            MAX_FOCAL_LENGTH_X_OVER_Y * 2.0,
            float("nan"),
            float("inf"),
            None,
            "1",
            True,
        ]:
            with self.subTest(ratio=ratio):
                with self.assertRaisesRegex(ValidationError, "focal_length_x_over_y|intrinsics"):
                    validate(with_focal_ratio(ratio))


if __name__ == "__main__":
    unittest.main()
