"""Pure Q1 upstream product-image quality evaluation.

The formal entrypoint compares both endpoints independently against the same
decoded upstream RGB8 source at its native 979x546 resolution.  It deliberately
contains no artifact, browser, device, or performance concerns.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from enum import Enum
from typing import Mapping


FORMAL_WIDTH = 979
FORMAL_HEIGHT = 546
WINDOW_SIZE = 8
SSIM_MINIMUM = 0.90
RGB_MAE_NORMALIZED_MAXIMUM = 0.05
SEVERE_RGB_ERROR_THRESHOLD_8BIT = 32
SEVERE_PIXEL_FRACTION_MAXIMUM = 0.10
PRODUCT_ENDPOINTS = frozenset({"gsplat_rs", "playcanvas"})

_C1 = (0.01 * 255.0) ** 2
_C2 = (0.03 * 255.0) ** 2


class ProductQualityError(ValueError):
    """An input or computed metric violates the frozen quality contract."""


class ProductQualityState(str, Enum):
    ACCEPTED = "Accepted"
    REJECTED = "Rejected"


@dataclass(frozen=True)
class ProductQualityMetrics:
    width: int
    height: int
    window_size: int
    window_count: int
    ssim_luma_srgb_window8: float
    rgb_mae_normalized: float
    severe_rgb_error_pixel_fraction: float


@dataclass(frozen=True)
class EndpointProductQualityDecision:
    endpoint: str
    state: ProductQualityState
    reason: str
    metrics: ProductQualityMetrics


@dataclass(frozen=True)
class ProductQualityEvaluation:
    gsplat_rs: EndpointProductQualityDecision
    playcanvas: EndpointProductQualityDecision


def _require_dimension(value: object, label: str) -> int:
    if type(value) is not int or value <= 0:
        raise ProductQualityError(f"{label} must be a positive integer")
    return value


def _require_bytes(value: object, expected_length: int, label: str) -> bytes:
    if type(value) is not bytes:
        raise ProductQualityError(f"{label} must be bytes")
    if len(value) != expected_length:
        raise ProductQualityError(
            f"{label} length must equal {expected_length}, got {len(value)}"
        )
    return value


def _window_ssim(reference: list[float], candidate: list[float]) -> float:
    count = len(reference)
    reference_sum = 0.0
    candidate_sum = 0.0
    for index in range(count):
        reference_sum += reference[index]
        candidate_sum += candidate[index]
    reference_mean = reference_sum / count
    candidate_mean = candidate_sum / count

    reference_variance = 0.0
    candidate_variance = 0.0
    covariance = 0.0
    for index in range(count):
        reference_delta = reference[index] - reference_mean
        candidate_delta = candidate[index] - candidate_mean
        reference_variance += reference_delta * reference_delta
        candidate_variance += candidate_delta * candidate_delta
        covariance += reference_delta * candidate_delta
    denominator = max(count - 1, 1)
    reference_variance /= denominator
    candidate_variance /= denominator
    covariance /= denominator
    return (
        (2.0 * reference_mean * candidate_mean + _C1)
        * (2.0 * covariance + _C2)
        / (
            (reference_mean * reference_mean + candidate_mean * candidate_mean + _C1)
            * (reference_variance + candidate_variance + _C2)
        )
    )


def _require_finite_metrics(metrics: ProductQualityMetrics) -> None:
    if type(metrics) is not ProductQualityMetrics:
        raise ProductQualityError("metrics must be ProductQualityMetrics")
    _require_dimension(metrics.width, "metrics.width")
    _require_dimension(metrics.height, "metrics.height")
    if metrics.window_size != WINDOW_SIZE or type(metrics.window_size) is not int:
        raise ProductQualityError("metrics.window_size must equal 8")
    _require_dimension(metrics.window_count, "metrics.window_count")
    expected_window_count = (
        ((metrics.width + WINDOW_SIZE - 1) // WINDOW_SIZE)
        * ((metrics.height + WINDOW_SIZE - 1) // WINDOW_SIZE)
    )
    if metrics.window_count != expected_window_count:
        raise ProductQualityError(
            f"metrics.window_count must equal {expected_window_count} for its dimensions"
        )
    for label, value in (
        ("ssim_luma_srgb_window8", metrics.ssim_luma_srgb_window8),
        ("rgb_mae_normalized", metrics.rgb_mae_normalized),
        ("severe_rgb_error_pixel_fraction", metrics.severe_rgb_error_pixel_fraction),
    ):
        if type(value) not in (int, float) or not math.isfinite(float(value)):
            raise ProductQualityError(f"metrics.{label} must be finite")
    if not -1.0 <= metrics.ssim_luma_srgb_window8 <= 1.0:
        raise ProductQualityError("metrics.ssim_luma_srgb_window8 must be in [-1,1]")
    if not 0.0 <= metrics.rgb_mae_normalized <= 1.0:
        raise ProductQualityError("metrics.rgb_mae_normalized must be in [0,1]")
    if not 0.0 <= metrics.severe_rgb_error_pixel_fraction <= 1.0:
        raise ProductQualityError(
            "metrics.severe_rgb_error_pixel_fraction must be in [0,1]"
        )


def compute_product_quality_metrics(
    source_rgb8: bytes,
    endpoint_rgba8: bytes,
    *,
    width: int,
    height: int,
) -> ProductQualityMetrics:
    """Compute the frozen metrics at explicit dimensions for pure unit fixtures.

    The calculation matches ``tests/perf/png-image-metrics.mjs``: non-overlapping
    8x8 sRGB-luma SSIM windows (including partial edge windows) and RGB MAE.
    The additional severe tail counts a pixel when any RGB channel differs by
    more than 32.  Endpoint alpha is a strict input invariant, not a score.
    """

    checked_width = _require_dimension(width, "width")
    checked_height = _require_dimension(height, "height")
    pixel_count = checked_width * checked_height
    source = _require_bytes(source_rgb8, pixel_count * 3, "source_rgb8")
    endpoint = _require_bytes(endpoint_rgba8, pixel_count * 4, "endpoint_rgba8")

    rgb_absolute_error = 0
    severe_pixels = 0
    for pixel in range(pixel_count):
        source_offset = pixel * 3
        endpoint_offset = pixel * 4
        if endpoint[endpoint_offset + 3] != 255:
            raise ProductQualityError(
                f"endpoint_rgba8 alpha must be 255 at every pixel; pixel {pixel} differs"
            )
        severe = False
        for channel in range(3):
            error = abs(
                source[source_offset + channel] - endpoint[endpoint_offset + channel]
            )
            rgb_absolute_error += error
            severe = severe or error > SEVERE_RGB_ERROR_THRESHOLD_8BIT
        severe_pixels += int(severe)

    scores: list[float] = []
    for top in range(0, checked_height, WINDOW_SIZE):
        for left in range(0, checked_width, WINDOW_SIZE):
            reference_luma: list[float] = []
            candidate_luma: list[float] = []
            bottom = min(top + WINDOW_SIZE, checked_height)
            right = min(left + WINDOW_SIZE, checked_width)
            for y in range(top, bottom):
                for x in range(left, right):
                    pixel = y * checked_width + x
                    source_offset = pixel * 3
                    endpoint_offset = pixel * 4
                    reference_luma.append(
                        0.2126 * source[source_offset]
                        + 0.7152 * source[source_offset + 1]
                        + 0.0722 * source[source_offset + 2]
                    )
                    candidate_luma.append(
                        0.2126 * endpoint[endpoint_offset]
                        + 0.7152 * endpoint[endpoint_offset + 1]
                        + 0.0722 * endpoint[endpoint_offset + 2]
                    )
            scores.append(_window_ssim(reference_luma, candidate_luma))

    metrics = ProductQualityMetrics(
        width=checked_width,
        height=checked_height,
        window_size=WINDOW_SIZE,
        window_count=len(scores),
        ssim_luma_srgb_window8=sum(scores) / len(scores),
        rgb_mae_normalized=rgb_absolute_error / (pixel_count * 3 * 255),
        severe_rgb_error_pixel_fraction=severe_pixels / pixel_count,
    )
    _require_finite_metrics(metrics)
    return metrics


def decide_product_quality_metrics(
    endpoint: str, metrics: ProductQualityMetrics
) -> EndpointProductQualityDecision:
    """Apply immutable thresholds to one endpoint without cross-endpoint averaging."""

    if type(endpoint) is not str or endpoint not in PRODUCT_ENDPOINTS:
        raise ProductQualityError("endpoint must be gsplat_rs or playcanvas")
    _require_finite_metrics(metrics)
    accepted = (
        metrics.ssim_luma_srgb_window8 >= SSIM_MINIMUM
        and metrics.rgb_mae_normalized <= RGB_MAE_NORMALIZED_MAXIMUM
        and metrics.severe_rgb_error_pixel_fraction <= SEVERE_PIXEL_FRACTION_MAXIMUM
    )
    return EndpointProductQualityDecision(
        endpoint=endpoint,
        state=ProductQualityState.ACCEPTED if accepted else ProductQualityState.REJECTED,
        reason="all_product_quality_metrics_meet_frozen_thresholds"
        if accepted
        else "one_or_more_product_quality_metrics_miss_frozen_thresholds",
        metrics=metrics,
    )


def evaluate_formal_product_quality_endpoint(
    endpoint: str, source_rgb8: bytes, endpoint_rgba8: bytes
) -> EndpointProductQualityDecision:
    """Evaluate one endpoint at the immutable 979x546 formal dimensions."""

    metrics = compute_product_quality_metrics(
        source_rgb8,
        endpoint_rgba8,
        width=FORMAL_WIDTH,
        height=FORMAL_HEIGHT,
    )
    return decide_product_quality_metrics(endpoint, metrics)


def evaluate_formal_product_quality(
    source_rgb8: bytes, endpoint_rgba8: Mapping[str, bytes]
) -> ProductQualityEvaluation:
    """Evaluate gsplat-rs and PlayCanvas separately against one upstream image."""

    if type(endpoint_rgba8) is not dict or set(endpoint_rgba8) != PRODUCT_ENDPOINTS:
        raise ProductQualityError(
            "endpoint_rgba8 must be a dict containing exactly gsplat_rs and playcanvas"
        )
    gsplat_rs = evaluate_formal_product_quality_endpoint(
        "gsplat_rs", source_rgb8, endpoint_rgba8["gsplat_rs"]
    )
    playcanvas = evaluate_formal_product_quality_endpoint(
        "playcanvas", source_rgb8, endpoint_rgba8["playcanvas"]
    )
    return ProductQualityEvaluation(gsplat_rs=gsplat_rs, playcanvas=playcanvas)
