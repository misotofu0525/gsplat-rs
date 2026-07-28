from __future__ import annotations

import pathlib
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from q1_pair_admission.quality_lanes import (  # noqa: E402
    COMPARISON_FIELDS,
    NATIVE_EXACT_THRESHOLD,
    PRODUCT_AUTHORITY_CLASS,
    QualityLaneError,
    QualityState,
    clear_ineligible_comparison_fields,
    evaluate_native_exact,
    evaluate_product_quality,
    reduce_quality_lanes,
    require_comparison_fields_eligible,
)


class NativeExactLaneTests(unittest.TestCase):
    def test_accepts_only_when_every_gsplat_score_meets_exact_threshold(self) -> None:
        accepted = evaluate_native_exact([0.99, 1.0])
        rejected = evaluate_native_exact([0.999, 0.989999])
        self.assertEqual(accepted.state, QualityState.ACCEPTED)
        self.assertEqual(rejected.state, QualityState.REJECTED)
        self.assertEqual(accepted.threshold, NATIVE_EXACT_THRESHOLD)

    def test_threshold_cannot_be_lowered_or_changed(self) -> None:
        for threshold in (0.98, 0.9900001, float("nan"), True, "0.99"):
            with self.subTest(threshold=threshold), self.assertRaisesRegex(
                QualityLaneError, "must equal 0.99"
            ):
                evaluate_native_exact([1.0], threshold=threshold)  # type: ignore[arg-type]

    def test_scores_must_be_nonempty_finite_numbers(self) -> None:
        for scores in (
            [],
            [float("nan")],
            [float("inf")],
            [-0.001],
            [1.001],
            [True],
            ["1.0"],
            "1.0",
        ):
            with self.subTest(scores=scores), self.assertRaises(QualityLaneError):
                evaluate_native_exact(scores)  # type: ignore[arg-type]

    def test_playcanvas_diagnostic_score_has_no_native_api_entry(self) -> None:
        with self.assertRaises(TypeError):
            evaluate_native_exact([1.0], playcanvas_scores=[0.5])  # type: ignore[call-arg]


class ProductQualityLaneTests(unittest.TestCase):
    def test_missing_authority_is_finitely_deferred(self) -> None:
        result = evaluate_product_quality(None)
        self.assertEqual(result.state, QualityState.DEFERRED)
        self.assertEqual(result.reason, "upstream_product_quality_authority_unavailable")
        with self.assertRaisesRegex(QualityLaneError, "require an upstream authority"):
            evaluate_product_quality(
                None,
                {"gsplat_rs": "Accepted", "playcanvas": "Accepted"},
            )

    def test_wrong_or_malformed_authority_is_not_treated_as_missing(self) -> None:
        for authority in ("direct_f32", "", True):
            with self.subTest(authority=authority), self.assertRaisesRegex(
                QualityLaneError, "authority_class"
            ):
                evaluate_product_quality(authority)  # type: ignore[arg-type]

    def test_both_upstream_evaluations_must_accept(self) -> None:
        accepted = evaluate_product_quality(
            PRODUCT_AUTHORITY_CLASS,
            {"gsplat_rs": "Accepted", "playcanvas": "Accepted"},
        )
        rejected = evaluate_product_quality(
            PRODUCT_AUTHORITY_CLASS,
            {"gsplat_rs": "Accepted", "playcanvas": "Rejected"},
        )
        deferred = evaluate_product_quality(
            PRODUCT_AUTHORITY_CLASS,
            {"gsplat_rs": "Deferred", "playcanvas": "Accepted"},
        )
        self.assertEqual(accepted.state, QualityState.ACCEPTED)
        self.assertEqual(rejected.state, QualityState.REJECTED)
        self.assertEqual(deferred.state, QualityState.DEFERRED)

    def test_product_endpoint_state_shape_and_types_are_strict(self) -> None:
        for states in (
            None,
            {"gsplat_rs": "Accepted"},
            {"gsplat_rs": "Accepted", "playcanvas": "Accepted", "extra": "Accepted"},
            {"gsplat_rs": True, "playcanvas": "Accepted"},
            {"gsplat_rs": "accepted", "playcanvas": "Accepted"},
        ):
            with self.subTest(states=states), self.assertRaises(QualityLaneError):
                evaluate_product_quality(PRODUCT_AUTHORITY_CLASS, states)  # type: ignore[arg-type]


class QualityReductionTests(unittest.TestCase):
    def test_native_rejection_has_first_priority(self) -> None:
        for product in ("Accepted", "Rejected", "Deferred"):
            self.assertEqual(
                reduce_quality_lanes("Rejected", product).state,
                QualityState.REJECTED,
            )

    def test_product_rejection_rejects_an_accepted_native_lane(self) -> None:
        self.assertEqual(
            reduce_quality_lanes("Accepted", "Rejected").state,
            QualityState.REJECTED,
        )

    def test_any_remaining_deferred_lane_defers(self) -> None:
        for native, product in (("Accepted", "Deferred"), ("Deferred", "Accepted")):
            decision = reduce_quality_lanes(native, product)
            self.assertEqual(decision.state, QualityState.DEFERRED)
            self.assertFalse(decision.same_quality_performance_eligible)

    def test_only_double_accept_unlocks_same_quality_performance(self) -> None:
        decision = reduce_quality_lanes("Accepted", "Accepted")
        self.assertEqual(decision.state, QualityState.ACCEPTED)
        self.assertTrue(decision.same_quality_performance_eligible)

    def test_reducer_rejects_noncanonical_state_values(self) -> None:
        for native, product in ((True, "Accepted"), ("accepted", "Accepted"), ("Accepted", None)):
            with self.subTest(native=native, product=product), self.assertRaises(
                QualityLaneError
            ):
                reduce_quality_lanes(native, product)

    def test_ineligible_output_clears_and_rejects_every_comparison_field(self) -> None:
        decision = reduce_quality_lanes("Accepted", "Deferred")
        output = {
            "state": "Deferred",
            "terminal_fps": 60,
            "paired_deltas_frame_wall_ms": [-1.0],
            "median_ratio": 1.2,
            "faster_endpoint": "playcanvas",
            "native_speedup": 1.3,
            "performance": {"winner": "playcanvas", "samples": [{"delta": 1.0}]},
            **{field: 1 for field in COMPARISON_FIELDS},
        }
        self.assertEqual(
            clear_ineligible_comparison_fields(decision, output),
            {"state": "Deferred", "performance": {"samples": [{}]}},
        )
        with self.assertRaisesRegex(QualityLaneError, "same-quality performance is ineligible"):
            require_comparison_fields_eligible(decision, output)

    def test_eligible_output_preserves_comparison_fields(self) -> None:
        decision = reduce_quality_lanes("Accepted", "Accepted")
        output = {"ratio": 1.25, "winner": "playcanvas"}
        self.assertEqual(clear_ineligible_comparison_fields(decision, output), output)
        require_comparison_fields_eligible(decision, output)


if __name__ == "__main__":
    unittest.main()
