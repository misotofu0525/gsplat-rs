#!/usr/bin/env python3
"""Fail-closed validation for Apple projected-draw benchmark evidence."""

from __future__ import annotations

import argparse
import json
import math
import pathlib
from typing import Any


FIRST_PROJECTED_TICKET = 1 << 52
MAX_SAFE_INTEGER = (1 << 53) - 1
POLICIES = {"candidate", "compact", "adaptive"}
EXECUTIONS = {"candidate", "compact"}
ADAPTIVE_STATES = {
    "disabled",
    "candidate_learning",
    "candidate_stable",
    "compact_probe",
    "compact_stable",
    "candidate_probe",
    "cooldown",
    "candidate_only",
}
FAILURE_REASONS = {"readback_map", "generation_invalidated", "invariant_violation"}
PROJECTED_EVIDENCE_VERSION = 1
LEGACY_PROJECTED_EVIDENCE_VERSION = 0


class ValidationError(ValueError):
    pass


def require_object(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValidationError(f"{name} must be an object")
    return value


def require_array(value: Any, name: str) -> list[Any]:
    if not isinstance(value, list):
        raise ValidationError(f"{name} must be an array")
    return value


def require_int(value: Any, name: str, *, projected_ticket: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValidationError(f"{name} must be a non-negative integer")
    if projected_ticket and not FIRST_PROJECTED_TICKET <= value <= MAX_SAFE_INTEGER:
        raise ValidationError(f"{name} is outside the projected ticket namespace")
    return value


def require_number(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValidationError(f"{name} must be a finite non-negative number")
    result = float(value)
    if not math.isfinite(result) or result < 0:
        raise ValidationError(f"{name} must be a finite non-negative number")
    return result


def require_identity(record: dict[str, Any], name: str) -> tuple[int, int, str, str]:
    ticket = require_int(record.get("ticket"), f"{name}.ticket", projected_ticket=True)
    revision = require_int(record.get("camera_revision"), f"{name}.camera_revision")
    execution = record.get("execution")
    backend = record.get("order_backend")
    if execution not in EXECUTIONS:
        raise ValidationError(f"{name}.execution is invalid")
    if backend not in {"cpu", "gpu"}:
        raise ValidationError(f"{name}.order_backend is invalid")
    return ticket, revision, execution, backend


FrameTicketIdentity = tuple[int, str, str, int, int, int, bool]


def validate_frames(
    frames: list[dict[str, Any]], requested_policy: str
) -> dict[int, FrameTicketIdentity]:
    issued: dict[int, FrameTicketIdentity] = {}
    for index, frame in enumerate(frames):
        name = f"frames[{index}]"
        if frame.get("projected_policy") != requested_policy:
            raise ValidationError(f"{name}.projected_policy does not match the manifest")
        execution = frame.get("projected_execution")
        state = frame.get("projected_adaptive_state")
        submission = frame.get("projected_measurement_submission")
        ticket = frame.get("projected_measurement_ticket")
        submitted_execution = frame.get("projected_measurement_execution")
        unsampled = frame.get("projected_measurement_unsampled_reason")
        flags = require_int(frame.get("projected_submission_flags"), f"{name}.projected_submission_flags")
        if flags & ~0b111:
            raise ValidationError(f"{name}.projected_submission_flags contains unknown bits")
        if execution not in EXECUTIONS or state not in ADAPTIVE_STATES:
            raise ValidationError(f"{name} contains invalid projected execution/state")

        visible = require_int(frame.get("visible"), f"{name}.visible")
        contributor = require_int(frame.get("contributor"), f"{name}.contributor")
        drawn = require_int(frame.get("drawn"), f"{name}.drawn")
        exact = frame.get("exact_contributor_compaction")
        if not isinstance(exact, bool) or contributor > visible:
            raise ValidationError(f"{name} contains invalid V/C/D evidence")
        if execution == "candidate" and (exact or drawn != visible):
            raise ValidationError(f"{name} Candidate execution requires D=V")
        if execution == "compact" and (not exact or drawn != contributor):
            raise ValidationError(f"{name} Compact execution requires D=C")

        if "order_submission_ticket" not in frame:
            raise ValidationError(f"{name}.order_submission_ticket is required")
        order_ticket = frame.get("order_submission_ticket")
        if order_ticket is not None:
            order_ticket = require_int(order_ticket, f"{name}.order_submission_ticket")
            if not 0 < order_ticket < FIRST_PROJECTED_TICKET:
                raise ValidationError(f"{name}.order_submission_ticket is outside the order namespace")
        if requested_policy != "adaptive":
            if (
                execution != requested_policy
                or state != "disabled"
                or submission != "not_requested"
                or ticket is not None
                or submitted_execution is not None
                or unsampled is not None
                or flags != 0
            ):
                raise ValidationError(f"{name} fabricates telemetry for a forced policy")
            continue
        if submission == "not_requested":
            if ticket is not None or submitted_execution is not None or unsampled is not None or flags != 0:
                raise ValidationError(f"{name} exposes identity for an unrequested sample")
        elif submission == "issued":
            ticket = require_int(ticket, f"{name}.projected_measurement_ticket", projected_ticket=True)
            if submitted_execution != execution or unsampled is not None or flags != 0b001:
                raise ValidationError(f"{name} issued projected identity is inconsistent")
            if order_ticket is not None:
                raise ValidationError(f"{name} issued order and projected formal tickets")
            revision = require_int(frame.get("camera_revision"), f"{name}.camera_revision")
            backend = frame.get("order_backend")
            if backend not in {"cpu", "gpu"}:
                raise ValidationError(f"{name}.order_backend is invalid")
            if ticket in issued:
                raise ValidationError(f"{name} repeats a projected ticket")
            issued[ticket] = (
                revision,
                execution,
                backend,
                visible,
                contributor,
                drawn,
                exact,
            )
        elif submission == "unsampled":
            if flags not in {0b010, 0b100}:
                raise ValidationError(f"{name} unsampled flags are inconsistent")
            raise ValidationError(f"{name} contains ring-busy or Surface-unavailable evidence")
        else:
            raise ValidationError(f"{name}.projected_measurement_submission is invalid")
        if state == "candidate_only" and (execution != "candidate" or submission != "not_requested"):
            raise ValidationError(f"{name} CandidateOnly state fabricated Compact telemetry")
    return issued


def validate_ledger(
    summary: dict[str, Any],
    frames: list[dict[str, Any]],
    frame_tickets: dict[int, FrameTicketIdentity],
    requested_policy: str,
    exactness_receipt_id: str,
) -> None:
    ledger = require_object(summary.get("projected_terminal_ledger"), "projected_terminal_ledger")
    submissions = require_array(ledger.get("submissions"), "projected_terminal_ledger.submissions")
    successes = require_array(ledger.get("successes"), "projected_terminal_ledger.successes")
    failures = require_array(ledger.get("failures"), "projected_terminal_ledger.failures")
    if failures:
        for index, raw in enumerate(failures):
            failure = require_object(raw, f"projected_terminal_ledger.failures[{index}]")
            require_identity(failure, f"projected_terminal_ledger.failures[{index}]")
            require_int(
                failure.get("projection_generation"),
                f"projected_terminal_ledger.failures[{index}].projection_generation",
            )
            require_int(
                failure.get("probe_generation"),
                f"projected_terminal_ledger.failures[{index}].probe_generation",
            )
            if failure.get("failure_reason") not in FAILURE_REASONS:
                raise ValidationError("projected failure has an invalid reason")
        raise ValidationError("formal Apple evidence contains a projected terminal failure")

    issued: dict[int, tuple[int, str, str]] = {}
    for index, raw in enumerate(submissions):
        submission = require_object(raw, f"projected_terminal_ledger.submissions[{index}]")
        ticket, revision, execution, backend = require_identity(
            submission, f"projected_terminal_ledger.submissions[{index}]"
        )
        if ticket in issued:
            raise ValidationError(f"projected ticket {ticket} was issued more than once")
        issued[ticket] = (revision, execution, backend)

    completed: dict[int, FrameTicketIdentity] = {}
    for index, raw in enumerate(successes):
        success = require_object(raw, f"projected_terminal_ledger.successes[{index}]")
        name = f"projected_terminal_ledger.successes[{index}]"
        ticket, revision, execution, backend = require_identity(success, name)
        if ticket in completed:
            raise ValidationError(f"projected ticket {ticket} has multiple terminals")
        if issued.get(ticket) != (revision, execution, backend):
            raise ValidationError(f"projected ticket {ticket} changed terminal identity")
        require_int(success.get("projection_generation"), f"{name}.projection_generation")
        require_int(success.get("probe_generation"), f"{name}.probe_generation")
        require_number(success.get("frame_complete_ms"), f"{name}.frame_complete_ms")
        if success.get("outcome") != "success":
            raise ValidationError(f"{name}.outcome must be success")
        if success.get("projection_rebuilt") is not True or success.get("order_refreshed") is not False:
            raise ValidationError(f"{name} is not an isolated projected probe")
        flags = require_int(success.get("measurement_flags"), f"{name}.measurement_flags")
        if flags & ~0b1111:
            raise ValidationError(f"{name}.measurement_flags contains unknown bits")
        if flags & (1 << 3):
            raise ValidationError(f"{name} reports a dropped prior terminal")
        if (
            bool(flags & (1 << 0)) != success.get("projection_rebuilt")
            or bool(flags & (1 << 1)) != success.get("order_refreshed")
        ):
            raise ValidationError(f"{name} flags disagree with the isolated probe fields")
        if success.get("exactness_receipt_id") != exactness_receipt_id:
            raise ValidationError(f"{name}.exactness_receipt_id mismatch")
        visible = require_int(success.get("visible"), f"{name}.visible")
        contributor = require_int(success.get("contributor"), f"{name}.contributor")
        drawn = require_int(success.get("drawn"), f"{name}.drawn")
        exact = success.get("exact_contributor_compaction")
        if contributor > visible or not isinstance(exact, bool):
            raise ValidationError(f"{name} contains invalid V/C/D evidence")
        if bool(flags & (1 << 2)) != exact:
            raise ValidationError(f"{name} flags disagree with exact compaction")
        counts_flags = require_int(success.get("counts_flags"), f"{name}.counts_flags")
        if counts_flags & ~0b1:
            raise ValidationError(f"{name}.counts_flags contains unknown bits")
        if bool(counts_flags & 0b1) != exact:
            raise ValidationError(f"{name}.counts_flags disagree with exact compaction")
        if execution == "candidate" and (exact or drawn != visible):
            raise ValidationError(f"{name} Candidate execution requires D=V")
        if execution == "compact" and (not exact or drawn != contributor):
            raise ValidationError(f"{name} Compact execution requires D=C")
        completed[ticket] = (
            revision,
            execution,
            backend,
            visible,
            contributor,
            drawn,
            exact,
        )

    if set(issued) != set(completed):
        missing = sorted(set(issued) - set(completed))
        raise ValidationError(f"projected terminal ledger is missing tickets: {missing}")
    for ticket, frame_identity in frame_tickets.items():
        if issued.get(ticket) != frame_identity[:3]:
            raise ValidationError(
                f"measured frame projected ticket {ticket} changed "
                "camera revision, execution, or order lane"
            )
        if completed.get(ticket) != frame_identity:
            raise ValidationError(
                f"measured frame projected ticket {ticket} changed its terminal V/C/D identity"
            )
    if requested_policy != "adaptive" and issued:
        raise ValidationError("a forced projected policy emitted a submission ledger")

    telemetry = require_object(summary.get("projected_draw_telemetry"), "projected_draw_telemetry")
    expected = {
        "measurement_scheduled_count": len(issued),
        "measurement_completed_count": len(completed),
        "measurement_terminal_failure_count": 0,
        "measurement_unsampled_count": 0,
    }
    if telemetry.get("policy_requested") != requested_policy:
        raise ValidationError("projected_draw_telemetry.policy_requested mismatch")
    if telemetry.get("candidate_frame_count") != sum(
        frame.get("projected_execution") == "candidate" for frame in frames
    ):
        raise ValidationError("projected_draw_telemetry.candidate_frame_count mismatch")
    if telemetry.get("compact_frame_count") != sum(
        frame.get("projected_execution") == "compact" for frame in frames
    ):
        raise ValidationError("projected_draw_telemetry.compact_frame_count mismatch")
    if telemetry.get("adaptive_final_state") != frames[-1].get("projected_adaptive_state"):
        raise ValidationError("projected_draw_telemetry.adaptive_final_state mismatch")
    if telemetry.get("execution_final") != frames[-1].get("projected_execution"):
        raise ValidationError("projected_draw_telemetry.execution_final mismatch")
    for key, value in expected.items():
        if telemetry.get(key) != value:
            raise ValidationError(f"projected_draw_telemetry.{key} mismatch")
    for key, value in (
        ("issued_count", len(issued)),
        ("success_count", len(completed)),
        ("failure_count", 0),
    ):
        if ledger.get(key) != value:
            raise ValidationError(f"projected_terminal_ledger.{key} mismatch")


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(math.ceil(fraction * len(ordered)) - 1, 0)
    return ordered[index]


def validate_cpu_completion_availability(
    manifest: dict[str, Any], summary: dict[str, Any], frames: list[dict[str, Any]]
) -> None:
    unavailable = require_array(manifest.get("unavailable_fields"), "manifest.unavailable_fields")
    unavailable_fields = set(unavailable)
    frame_marker = "frames[*].cpu_frame_complete_ms"
    summary_marker = "summary.distributions.cpu_frame_complete_ms"
    values: list[float] = []
    for index, frame in enumerate(frames):
        name = f"frames[{index}].cpu_frame_complete_ms"
        if "cpu_frame_complete_ms" not in frame:
            raise ValidationError(f"{name} is required by projected evidence v1")
        value = frame["cpu_frame_complete_ms"]
        if value is not None:
            values.append(require_number(value, name))

    distributions = require_object(summary.get("distributions"), "summary.distributions")
    distribution = distributions.get("cpu_frame_complete_ms")
    if not values:
        if distribution is not None:
            raise ValidationError(
                "summary.distributions.cpu_frame_complete_ms must be null when all frames are null"
            )
        if frame_marker not in unavailable_fields or summary_marker not in unavailable_fields:
            raise ValidationError("unavailable_fields must declare absent CPU completion timing")
        return

    if frame_marker in unavailable_fields or summary_marker in unavailable_fields:
        raise ValidationError("available CPU completion timing is declared unavailable")
    distribution = require_object(distribution, "summary.distributions.cpu_frame_complete_ms")
    if require_int(distribution.get("count"), "cpu_frame_complete_ms.count") != len(values):
        raise ValidationError("summary.distributions.cpu_frame_complete_ms.count mismatch")
    expected = {
        "mean": sum(values) / len(values),
        "p50": percentile(values, 0.50),
        "p90": percentile(values, 0.90),
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "max": max(values),
    }
    for field, expected_value in expected.items():
        actual = require_number(
            distribution.get(field), f"cpu_frame_complete_ms.{field}"
        )
        if not math.isclose(actual, expected_value, rel_tol=1e-9, abs_tol=1e-9):
            raise ValidationError(
                f"summary.distributions.cpu_frame_complete_ms.{field} mismatch"
            )


def validate(path: pathlib.Path) -> None:
    manifest = require_object(json.loads((path / "manifest.json").read_text()), "manifest")
    summary = require_object(json.loads((path / "summary.json").read_text()), "summary")
    frames = [
        require_object(json.loads(line), f"frames[{index}]")
        for index, line in enumerate((path / "frames.jsonl").read_text().splitlines())
        if line.strip()
    ]
    renderer = require_object(manifest.get("renderer"), "manifest.renderer")
    evidence_version = renderer.get("projected_evidence_version")
    if isinstance(evidence_version, bool) or not isinstance(evidence_version, int):
        raise ValidationError(
            "manifest.renderer.projected_evidence_version must be the integer 0 or 1"
        )
    if evidence_version == LEGACY_PROJECTED_EVIDENCE_VERSION:
        if renderer.get("projected_policy_requested") is not None:
            raise ValidationError("legacy projected evidence cannot declare a projected policy")
        if any(any(key.startswith("projected_") for key in frame) for frame in frames):
            raise ValidationError("legacy projected evidence contains projected frame fields")
        if "projected_draw_telemetry" in summary or "projected_terminal_ledger" in summary:
            raise ValidationError("legacy projected evidence contains a projected terminal ledger")
        return
    if evidence_version != PROJECTED_EVIDENCE_VERSION:
        raise ValidationError("manifest.renderer.projected_evidence_version must equal 0 or 1")
    requested_policy = renderer.get("projected_policy_requested")
    if requested_policy is None:
        raise ValidationError("manifest.renderer.projected_policy_requested is required")
    if requested_policy not in POLICIES:
        raise ValidationError("manifest.renderer.projected_policy_requested is invalid")
    exactness = require_object(manifest.get("exactness"), "manifest.exactness")
    exactness_receipt_id = exactness.get("receipt_id")
    if not isinstance(exactness_receipt_id, str) or not exactness_receipt_id:
        raise ValidationError("manifest.exactness.receipt_id is invalid")
    if not frames:
        raise ValidationError("projected evidence has no frames")
    frame_tickets = validate_frames(frames, requested_policy)
    validate_ledger(summary, frames, frame_tickets, requested_policy, exactness_receipt_id)
    validate_cpu_completion_availability(manifest, summary, frames)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact", type=pathlib.Path)
    args = parser.parse_args()
    try:
        validate(args.artifact)
    except (OSError, json.JSONDecodeError, ValidationError) as error:
        parser.error(str(error))
    print(f"valid_ios_projected_artifact={args.artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
