from __future__ import annotations

import dataclasses
import math
import pathlib
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from q1_pair_admission.product_quality import (  # noqa: E402
    FORMAL_HEIGHT,
    FORMAL_WIDTH,
    RGB_MAE_NORMALIZED_MAXIMUM,
    SEVERE_PIXEL_FRACTION_MAXIMUM,
    SSIM_MINIMUM,
    EndpointProductQualityDecision,
    ProductQualityError,
    ProductQualityMetrics,
    ProductQualityState,
    compute_product_quality_metrics,
    decide_product_quality_metrics,
    evaluate_formal_product_quality,
    evaluate_formal_product_quality_endpoint,
)


def rgba(rgb: bytes, *, alpha: int = 255) -> bytes:
    return bytes(
        channel
        for pixel in range(len(rgb) // 3)
        for channel in (*rgb[pixel * 3 : pixel * 3 + 3], alpha)
    )


def patterned_rgb(width: int, height: int) -> bytes:
    return bytes(
        channel
        for y in range(height)
        for x in range(width)
        for channel in (
            (x * 29 + y * 7) % 256,
            (x * 11 + y * 31) % 256,
            (x * 17 + y * 19) % 256,
        )
    )


def shift_right(rgb: bytes, width: int, height: int) -> bytes:
    result = bytearray(len(rgb))
    for y in range(height):
        for x in range(width):
            source_x = (x - 1) % width
            source = (y * width + source_x) * 3
            destination = (y * width + x) * 3
            result[destination : destination + 3] = rgb[source : source + 3]
    return bytes(result)


def metrics(
    *,
    ssim: float = 1.0,
    mae: float = 0.0,
    severe: float = 0.0,
) -> ProductQualityMetrics:
    return ProductQualityMetrics(
        width=8,
        height=8,
        window_size=8,
        window_count=1,
        ssim_luma_srgb_window8=ssim,
        rgb_mae_normalized=mae,
        severe_rgb_error_pixel_fraction=severe,
    )


class ProductQualityMetricTests(unittest.TestCase):
    def test_exact_bytes_are_accepted_with_identity_metrics(self) -> None:
        source = patterned_rgb(10, 9)
        observed = compute_product_quality_metrics(
            source, rgba(source), width=10, height=9
        )
        self.assertEqual(observed.window_count, 4)
        self.assertAlmostEqual(observed.ssim_luma_srgb_window8, 1.0, places=15)
        self.assertEqual(observed.rgb_mae_normalized, 0.0)
        self.assertEqual(observed.severe_rgb_error_pixel_fraction, 0.0)
        self.assertEqual(
            decide_product_quality_metrics("gsplat_rs", observed).state,
            ProductQualityState.ACCEPTED,
        )

    def test_metrics_match_the_existing_javascript_byte_contract(self) -> None:
        source = patterned_rgb(10, 9)
        candidate = bytes(
            channel
            for y in range(9)
            for x in range(10)
            for channel in (
                ((x * 29 + y * 7) + 16) % 256,
                ((x * 11 + y * 31) + 16) % 256,
                ((x * 17 + y * 19) + 16) % 256,
            )
        )
        observed = compute_product_quality_metrics(
            source, rgba(candidate), width=10, height=9
        )
        # Locked from computeRawRgba8ImageMetrics in png-image-metrics.mjs.
        self.assertEqual(observed.ssim_luma_srgb_window8, 0.5120155614193975)
        self.assertEqual(observed.rgb_mae_normalized, 0.10829339143064633)

    def test_plus_sixteen_is_rejected_by_the_frozen_mae(self) -> None:
        source = bytes([64, 64, 64] * 64)
        candidate = bytes([80, 80, 80] * 64)
        observed = compute_product_quality_metrics(
            source, rgba(candidate), width=8, height=8
        )
        self.assertGreater(observed.rgb_mae_normalized, RGB_MAE_NORMALIZED_MAXIMUM)
        self.assertEqual(
            decide_product_quality_metrics("playcanvas", observed).state,
            ProductQualityState.REJECTED,
        )

    def test_one_pixel_shift_is_rejected(self) -> None:
        source = patterned_rgb(16, 16)
        candidate = shift_right(source, 16, 16)
        observed = compute_product_quality_metrics(
            source, rgba(candidate), width=16, height=16
        )
        self.assertTrue(
            observed.ssim_luma_srgb_window8 < SSIM_MINIMUM
            or observed.rgb_mae_normalized > RGB_MAE_NORMALIZED_MAXIMUM
            or observed.severe_rgb_error_pixel_fraction > SEVERE_PIXEL_FRACTION_MAXIMUM
        )
        self.assertEqual(
            decide_product_quality_metrics("gsplat_rs", observed).state,
            ProductQualityState.REJECTED,
        )

    def test_five_percent_blackout_tail_remains_below_ten_percent(self) -> None:
        width, height = 20, 10
        source = bytes([200, 200, 200] * (width * height))
        candidate = bytearray(source)
        for pixel in range(10):
            candidate[pixel * 3 : pixel * 3 + 3] = b"\x00\x00\x00"
        observed = compute_product_quality_metrics(
            source, rgba(bytes(candidate)), width=width, height=height
        )
        self.assertAlmostEqual(observed.severe_rgb_error_pixel_fraction, 0.05)
        self.assertLessEqual(observed.rgb_mae_normalized, RGB_MAE_NORMALIZED_MAXIMUM)
        self.assertLess(observed.ssim_luma_srgb_window8, SSIM_MINIMUM)
        self.assertEqual(
            decide_product_quality_metrics("playcanvas", observed).state,
            ProductQualityState.REJECTED,
        )

    def test_threshold_boundaries_are_inclusive_and_immutable(self) -> None:
        boundary = metrics(
            ssim=SSIM_MINIMUM,
            mae=RGB_MAE_NORMALIZED_MAXIMUM,
            severe=SEVERE_PIXEL_FRACTION_MAXIMUM,
        )
        self.assertEqual(
            decide_product_quality_metrics("gsplat_rs", boundary).state,
            ProductQualityState.ACCEPTED,
        )
        for observed in (
            metrics(ssim=math.nextafter(SSIM_MINIMUM, -math.inf)),
            metrics(mae=math.nextafter(RGB_MAE_NORMALIZED_MAXIMUM, math.inf)),
            metrics(severe=math.nextafter(SEVERE_PIXEL_FRACTION_MAXIMUM, math.inf)),
        ):
            with self.subTest(observed=observed):
                self.assertEqual(
                    decide_product_quality_metrics("playcanvas", observed).state,
                    ProductQualityState.REJECTED,
                )
        with self.assertRaises(TypeError):
            decide_product_quality_metrics(  # type: ignore[call-arg]
                "gsplat_rs", boundary, ssim_minimum=0.5
            )

    def test_severe_tail_uses_any_channel_and_strict_greater_than_32(self) -> None:
        source = bytes([0, 0, 0] * 10)
        candidate = bytearray(source)
        candidate[0] = 32
        candidate[3 + 1] = 33
        observed = compute_product_quality_metrics(
            source, rgba(bytes(candidate)), width=10, height=1
        )
        self.assertAlmostEqual(observed.severe_rgb_error_pixel_fraction, 0.1)

    def test_alpha_dimensions_lengths_and_types_fail_closed(self) -> None:
        source = bytes([0, 0, 0] * 4)
        opaque = rgba(source)
        malformed = (
            (True, 2, source, opaque),
            (2, False, source, opaque),
            (2, 2, bytearray(source), opaque),
            (2, 2, source, bytearray(opaque)),
            (2, 2, source[:-1], opaque),
            (2, 2, source, opaque[:-1]),
            (2, 2, source, rgba(source, alpha=254)),
        )
        for width, height, reference, candidate in malformed:
            with self.subTest(
                width=width, height=height, reference=type(reference)
            ), self.assertRaises(ProductQualityError):
                compute_product_quality_metrics(  # type: ignore[arg-type]
                    reference, candidate, width=width, height=height
                )

    def test_nonfinite_and_wrong_metric_types_fail_closed(self) -> None:
        for observed in (
            metrics(ssim=float("nan")),
            metrics(mae=float("inf")),
            metrics(severe=float("-inf")),
            metrics(ssim=True),
            metrics(ssim=1.01),
            metrics(mae=-0.1),
            metrics(severe=1.1),
            dataclasses.replace(metrics(), window_count=2),
        ):
            with self.subTest(observed=observed), self.assertRaises(
                ProductQualityError
            ):
                decide_product_quality_metrics("gsplat_rs", observed)
        with self.assertRaises(ProductQualityError):
            decide_product_quality_metrics(True, metrics())  # type: ignore[arg-type]


class FormalProductQualityTests(unittest.TestCase):
    def test_formal_endpoint_dimensions_are_not_overridable(self) -> None:
        source = bytes(FORMAL_WIDTH * FORMAL_HEIGHT * 3)
        endpoint = rgba(source)
        decision = evaluate_formal_product_quality_endpoint(
            "gsplat_rs", source, endpoint
        )
        self.assertEqual(decision.metrics.width, FORMAL_WIDTH)
        self.assertEqual(decision.metrics.height, FORMAL_HEIGHT)
        with self.assertRaises(TypeError):
            evaluate_formal_product_quality_endpoint(  # type: ignore[call-arg]
                "gsplat_rs", source, endpoint, width=8, height=8
            )

    def test_endpoints_are_evaluated_separately_without_averaging(self) -> None:
        source = bytes(FORMAL_WIDTH * FORMAL_HEIGHT * 3)
        accepted = rgba(source)
        rejected = bytes([255, 255, 255, 255] * (FORMAL_WIDTH * FORMAL_HEIGHT))
        result = evaluate_formal_product_quality(
            source,
            {"gsplat_rs": accepted, "playcanvas": rejected},
        )
        self.assertEqual(result.gsplat_rs.state, ProductQualityState.ACCEPTED)
        self.assertEqual(result.playcanvas.state, ProductQualityState.REJECTED)
        reversed_result = evaluate_formal_product_quality(
            source,
            {"gsplat_rs": rejected, "playcanvas": accepted},
        )
        self.assertEqual(reversed_result.gsplat_rs.state, ProductQualityState.REJECTED)
        self.assertEqual(reversed_result.playcanvas.state, ProductQualityState.ACCEPTED)

    def test_endpoint_mapping_and_public_output_contain_no_comparison_claims(self) -> None:
        source = bytes(FORMAL_WIDTH * FORMAL_HEIGHT * 3)
        exact = rgba(source)
        result = evaluate_formal_product_quality(
            source, {"gsplat_rs": exact, "playcanvas": exact}
        )
        serialized = repr(dataclasses.asdict(result)).lower()
        for forbidden in ("timing", "fps", "ratio", "winner"):
            self.assertNotIn(forbidden, serialized)
        for endpoints in (
            {"gsplat_rs": exact},
            {"gsplat_rs": exact, "playcanvas": exact, "extra": exact},
            {"gsplat_rs": exact, "playcanvas": bytearray(exact)},
            [("gsplat_rs", exact), ("playcanvas", exact)],
        ):
            with self.subTest(endpoints=type(endpoints)), self.assertRaises(
                ProductQualityError
            ):
                evaluate_formal_product_quality(source, endpoints)  # type: ignore[arg-type]

    def test_decision_type_carries_one_endpoint_only(self) -> None:
        decision = decide_product_quality_metrics("gsplat_rs", metrics())
        self.assertIsInstance(decision, EndpointProductQualityDecision)
        self.assertEqual(decision.endpoint, "gsplat_rs")
        self.assertFalse(hasattr(decision, "playcanvas"))


if __name__ == "__main__":
    unittest.main()
