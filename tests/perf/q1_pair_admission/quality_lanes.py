"""Pure Q1 Native Exact and Product Quality state reduction.

This module deliberately knows nothing about collectors, browsers, artifacts,
or timing.  Producers validate pixels and authority identity elsewhere, then
pass only their terminal lane inputs through this finite reducer.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from enum import Enum
from typing import Any, Mapping, Sequence


NATIVE_EXACT_THRESHOLD = 0.99
PRODUCT_AUTHORITY_CLASS = "upstream_source_camera_images"
PRODUCT_ENDPOINTS = frozenset({"gsplat_rs", "playcanvas"})
COMPARISON_FIELDS = frozenset(
    {
        "delta",
        "ratio",
        "fps",
        "direction",
        "winner",
        "gsplat_rs_minus_playcanvas_ms",
        "gsplat_rs_over_playcanvas_ratio",
        "required_lead_percentage",
    }
)
COMPARISON_FIELD_TOKENS = frozenset(
    {
        "advantage",
        "delta",
        "deltas",
        "direction",
        "faster",
        "fps",
        "lead",
        "ratio",
        "ratios",
        "slower",
        "speedup",
        "winner",
    }
)


class QualityLaneError(ValueError):
    """A lane input or attempted publication violates the frozen contract."""


class QualityState(str, Enum):
    ACCEPTED = "Accepted"
    REJECTED = "Rejected"
    DEFERRED = "Deferred"


@dataclass(frozen=True)
class LaneDecision:
    state: QualityState
    reason: str
    scores: tuple[float, ...] = ()
    threshold: float | None = None


@dataclass(frozen=True)
class QualityDecision:
    native_exact: QualityState
    product_quality: QualityState
    state: QualityState
    same_quality_performance_eligible: bool


def _state(value: object, context: str) -> QualityState:
    if type(value) is not str:
        raise QualityLaneError(f"{context} must be Accepted, Rejected or Deferred")
    try:
        return QualityState(value)
    except ValueError as error:
        raise QualityLaneError(
            f"{context} must be Accepted, Rejected or Deferred"
        ) from error


def _scores(values: object, context: str) -> tuple[float, ...]:
    if not isinstance(values, Sequence) or isinstance(values, (str, bytes, bytearray)):
        raise QualityLaneError(f"{context} must be a non-empty score sequence")
    if not values:
        raise QualityLaneError(f"{context} must not be empty")
    result: list[float] = []
    for index, value in enumerate(values):
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise QualityLaneError(f"{context}[{index}] must be a finite number")
        score = float(value)
        if not math.isfinite(score) or not 0.0 <= score <= 1.0:
            raise QualityLaneError(f"{context}[{index}] must be finite in [0,1]")
        result.append(score)
    return tuple(result)


def evaluate_native_exact(
    gsplat_rs_scores: Sequence[float], *, threshold: float = NATIVE_EXACT_THRESHOLD
) -> LaneDecision:
    """Evaluate only gsplat-rs against the immutable Direct-f32 authority."""

    if isinstance(threshold, bool) or not isinstance(threshold, (int, float)):
        raise QualityLaneError("Native Exact threshold must equal 0.99")
    if not math.isfinite(float(threshold)) or float(threshold) != NATIVE_EXACT_THRESHOLD:
        raise QualityLaneError("Native Exact threshold must equal 0.99")
    scores = _scores(gsplat_rs_scores, "gsplat-rs Native Exact scores")
    accepted = all(score >= NATIVE_EXACT_THRESHOLD for score in scores)
    return LaneDecision(
        state=QualityState.ACCEPTED if accepted else QualityState.REJECTED,
        reason="all_gsplat_rs_scores_meet_native_exact_threshold"
        if accepted
        else "gsplat_rs_score_below_native_exact_threshold",
        scores=scores,
        threshold=NATIVE_EXACT_THRESHOLD,
    )


def evaluate_product_quality(
    authority_class: str | None,
    endpoint_states: Mapping[str, str] | None = None,
) -> LaneDecision:
    """Reduce independently evaluated endpoint results under upstream authority."""

    if authority_class is None:
        if endpoint_states is not None:
            raise QualityLaneError(
                "Product Quality endpoint states require an upstream authority"
            )
        return LaneDecision(
            state=QualityState.DEFERRED,
            reason="upstream_product_quality_authority_unavailable",
        )
    if type(authority_class) is not str or authority_class != PRODUCT_AUTHORITY_CLASS:
        raise QualityLaneError(
            f"Product Quality authority_class must equal {PRODUCT_AUTHORITY_CLASS}"
        )
    if not isinstance(endpoint_states, Mapping):
        raise QualityLaneError("Product Quality endpoint states must be a mapping")
    if set(endpoint_states) != PRODUCT_ENDPOINTS:
        raise QualityLaneError(
            "Product Quality endpoint states must contain exactly gsplat_rs and playcanvas"
        )
    states = {
        endpoint: _state(endpoint_states[endpoint], f"Product Quality {endpoint} state")
        for endpoint in PRODUCT_ENDPOINTS
    }
    if QualityState.REJECTED in states.values():
        state = QualityState.REJECTED
        reason = "product_quality_endpoint_rejected"
    elif QualityState.DEFERRED in states.values():
        state = QualityState.DEFERRED
        reason = "product_quality_endpoint_deferred"
    else:
        state = QualityState.ACCEPTED
        reason = "both_endpoints_accept_upstream_product_quality"
    return LaneDecision(state=state, reason=reason)


def reduce_quality_lanes(native_exact: object, product_quality: object) -> QualityDecision:
    """Apply the frozen Q1 lane priority without inspecting endpoint metrics."""

    native = _state(native_exact, "Native Exact state")
    product = _state(product_quality, "Product Quality state")
    if native is QualityState.REJECTED:
        state = QualityState.REJECTED
    elif product is QualityState.REJECTED:
        state = QualityState.REJECTED
    elif QualityState.DEFERRED in (native, product):
        state = QualityState.DEFERRED
    else:
        state = QualityState.ACCEPTED
    return QualityDecision(
        native_exact=native,
        product_quality=product,
        state=state,
        same_quality_performance_eligible=state is QualityState.ACCEPTED,
    )


def clear_ineligible_comparison_fields(
    decision: QualityDecision, output: Mapping[str, Any]
) -> dict[str, Any]:
    """Return a copy that cannot carry comparative claims when quality is ineligible."""

    if not isinstance(decision, QualityDecision):
        raise QualityLaneError("quality decision must be a QualityDecision")
    if not isinstance(output, Mapping):
        raise QualityLaneError("comparison output must be a mapping")
    _comparison_fields(output)
    if decision.same_quality_performance_eligible:
        return dict(output)
    return _clear_comparison_fields(output)


def require_comparison_fields_eligible(
    decision: QualityDecision, output: Mapping[str, Any]
) -> None:
    """Reject an ineligible output that still contains a relative comparison."""

    cleaned = clear_ineligible_comparison_fields(decision, output)
    if cleaned != dict(output):
        fields = sorted(_comparison_fields(output))
        raise QualityLaneError(
            "same-quality performance is ineligible; forbidden comparison fields: "
            + ", ".join(fields)
        )


def _comparison_fields(output: Mapping[str, Any]) -> frozenset[str]:
    fields: set[str] = set()
    for field, value in output.items():
        if type(field) is not str:
            raise QualityLaneError("comparison output field names must be strings")
        tokens = set(field.lower().split("_"))
        if field in COMPARISON_FIELDS or tokens.intersection(COMPARISON_FIELD_TOKENS):
            fields.add(field)
        elif isinstance(value, Mapping):
            fields.update(_comparison_fields(value))
        elif isinstance(value, (list, tuple)):
            for item in value:
                if isinstance(item, Mapping):
                    fields.update(_comparison_fields(item))
    return frozenset(fields)


def _clear_comparison_fields(output: Mapping[str, Any]) -> dict[str, Any]:
    cleaned: dict[str, Any] = {}
    for field, value in output.items():
        if type(field) is not str:
            raise QualityLaneError("comparison output field names must be strings")
        tokens = set(field.lower().split("_"))
        if field in COMPARISON_FIELDS or tokens.intersection(COMPARISON_FIELD_TOKENS):
            continue
        if isinstance(value, Mapping):
            value = _clear_comparison_fields(value)
        elif isinstance(value, list):
            value = [
                _clear_comparison_fields(item) if isinstance(item, Mapping) else item
                for item in value
            ]
        elif isinstance(value, tuple):
            value = tuple(
                _clear_comparison_fields(item) if isinstance(item, Mapping) else item
                for item in value
            )
        cleaned[field] = value
    return cleaned
