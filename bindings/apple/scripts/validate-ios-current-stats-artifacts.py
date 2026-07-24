#!/usr/bin/env python3
"""Fail-closed validation for Apple current-stats v1 benchmark evidence."""

from __future__ import annotations

import argparse
import json
import math
import pathlib
from typing import Any


CURRENT_STATS_EVIDENCE_VERSION = 1
LEGACY_CURRENT_STATS_EVIDENCE_VERSION = 0
PLANS = {"cpu_post_sort", "gpu_post_sort", "gpu_preproject"}
SEMANTICS = {
    "direct_draw_equals_visible",
    "indirect_draw_equals_visible",
    "indirect_draw_equals_contributor",
}
FAILURE_REASONS = {
    "map_failure",
    "generation_invalidated",
    "expired",
    "dropped",
}
IDENTITY_FIELDS = (
    "scene_generation",
    "camera_revision",
    "viewport_generation",
    "contract_generation",
    "plan_set_generation",
    "order_generation",
    "raster_generation",
    "encode_attempt",
    "presentation_sequence",
)


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


def require_int(value: Any, name: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValidationError(f"{name} must be a non-negative integer")
    if positive and value == 0:
        raise ValidationError(f"{name} must be positive")
    return value


def require_number(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValidationError(f"{name} must be a finite non-negative number")
    result = float(value)
    if not math.isfinite(result) or result < 0:
        raise ValidationError(f"{name} must be a finite non-negative number")
    return result


Identity = tuple[int, int, int, int, int, str, int, int, int, int]
SampleJoin = tuple[int, int, str, str, Identity]
Counts = tuple[str, int, int, int, int]


def require_identity(value: Any, name: str) -> Identity:
    identity = require_object(value, name)
    integers = {
        field: require_int(identity.get(field), f"{name}.{field}")
        for field in IDENTITY_FIELDS
    }
    plan = identity.get("executed_plan")
    if plan not in PLANS:
        raise ValidationError(f"{name}.executed_plan is invalid")
    return (
        integers["scene_generation"],
        integers["camera_revision"],
        integers["viewport_generation"],
        integers["contract_generation"],
        integers["plan_set_generation"],
        plan,
        integers["order_generation"],
        integers["raster_generation"],
        integers["encode_attempt"],
        integers["presentation_sequence"],
    )


def require_sample_join(record: dict[str, Any], name: str) -> SampleJoin:
    ticket = require_int(record.get("ticket"), f"{name}.ticket", positive=True)
    sample_index = require_int(record.get("sample_index"), f"{name}.sample_index")
    sample_key = record.get("sample_key")
    trace_key = record.get("trace_key")
    if not isinstance(sample_key, str) or not sample_key:
        raise ValidationError(f"{name}.sample_key must be a non-empty string")
    if not isinstance(trace_key, str) or not trace_key:
        raise ValidationError(f"{name}.trace_key must be a non-empty string")
    return ticket, sample_index, sample_key, trace_key, require_identity(
        record.get("identity"), f"{name}.identity"
    )


def require_counts(record: dict[str, Any], name: str) -> Counts:
    semantics = record.get("count_semantics")
    if semantics not in SEMANTICS:
        raise ValidationError(f"{name}.count_semantics is invalid")
    source = require_int(record.get("source"), f"{name}.source")
    visible = require_int(record.get("visible"), f"{name}.visible")
    contributor = require_int(record.get("contributor"), f"{name}.contributor")
    drawn = require_int(record.get("drawn"), f"{name}.drawn")
    if not contributor <= visible <= source:
        raise ValidationError(f"{name} violates 0 <= C <= V <= S")
    return semantics, source, visible, contributor, drawn


def validate_plan_counts(
    identity: Identity,
    counts: Counts,
    *,
    backend: str,
    projected_execution: str,
    exact_compaction: Any,
    name: str,
) -> None:
    plan = identity[5]
    semantics, _, visible, contributor, drawn = counts
    expected = {
        "cpu_post_sort": ("direct_draw_equals_visible", "cpu", "candidate"),
        "gpu_post_sort": ("indirect_draw_equals_visible", "gpu", "candidate"),
        "gpu_preproject": (
            "indirect_draw_equals_contributor",
            "gpu",
            "compact",
        ),
    }[plan]
    if (semantics, backend, projected_execution) != expected:
        raise ValidationError(f"{name} plan/semantics/order/projected tuple mismatch")
    expected_compaction = plan == "gpu_preproject"
    if exact_compaction is not expected_compaction:
        raise ValidationError(f"{name}.exact_contributor_compaction disagrees with plan")
    if expected_compaction:
        if drawn != contributor:
            raise ValidationError(f"{name} GPU Preproject requires D=C")
    elif drawn != visible:
        raise ValidationError(f"{name} PostSort requires D=V")


def validate_frames(
    manifest: dict[str, Any], frames: list[dict[str, Any]]
) -> tuple[dict[int, tuple[SampleJoin, Counts]], set[int]]:
    dataset = require_object(manifest.get("dataset"), "manifest.dataset")
    source_count = require_int(dataset.get("splat_count"), "manifest.dataset.splat_count")
    unavailable = set(
        require_array(manifest.get("unavailable_fields"), "manifest.unavailable_fields")
    )
    required_unavailable = {
        "frames[*].geometry_submit_ms",
        "summary.distributions.geometry_submit_ms",
    }
    if not required_unavailable.issubset(unavailable):
        raise ValidationError("legacy geometry timing must be declared unavailable")
    trace = require_object(manifest.get("trace"), "manifest.trace")
    trace_sha256 = trace.get("sha256")
    if not isinstance(trace_sha256, str) or not trace_sha256:
        raise ValidationError("manifest.trace.sha256 must be a non-empty string")

    issued: dict[int, tuple[SampleJoin, Counts]] = {}
    presentation_sequences: set[int] = set()
    has_missing_cpu_phase = False
    for index, frame in enumerate(frames):
        name = f"frames[{index}]"
        ticket = require_int(
            frame.get("current_stats_ticket"),
            f"{name}.current_stats_ticket",
            positive=True,
        )
        if ticket in issued:
            raise ValidationError(f"{name} repeats a current-stats ticket")
        sample_key = frame.get("current_stats_sample_key")
        if sample_key != f"measure:{index}":
            raise ValidationError(f"{name}.current_stats_sample_key mismatch")
        trace_index = frame.get("trace_frame_index")
        trace_timestamp = frame.get("trace_timestamp_ns")
        trace_loop = frame.get("trace_loop_index")
        if trace_index is None and trace_timestamp is None and trace_loop is None:
            fixed_frame_index = trace.get("frame_index")
            if fixed_frame_index is None:
                trace_key = f"orbit:{index}"
            else:
                fixed_frame_index = require_int(
                    fixed_frame_index, "manifest.trace.frame_index"
                )
                trace_key = f"fixed:{trace_sha256}:{fixed_frame_index}"
        else:
            trace_index = require_int(trace_index, f"{name}.trace_frame_index")
            trace_timestamp = require_int(
                trace_timestamp, f"{name}.trace_timestamp_ns"
            )
            trace_loop = require_int(trace_loop, f"{name}.trace_loop_index")
            trace_key = f"trace:{trace_index}:{trace_timestamp}:{trace_loop}"
        if frame.get("current_stats_trace_key") != trace_key:
            raise ValidationError(f"{name}.current_stats_trace_key mismatch")

        identity = require_identity(
            frame.get("current_stats_identity"), f"{name}.current_stats_identity"
        )
        camera_revision = require_int(frame.get("camera_revision"), f"{name}.camera_revision")
        if identity[1] != camera_revision:
            raise ValidationError(f"{name} current-stats camera identity drift")
        presentation_sequence = identity[9]
        if presentation_sequence in presentation_sequences:
            raise ValidationError(f"{name} repeats a current-stats presentation sequence")
        presentation_sequences.add(presentation_sequence)

        frame_counts_record = {
            "count_semantics": frame.get("current_stats_count_semantics"),
            "source": frame.get("source"),
            "visible": frame.get("visible"),
            "contributor": frame.get("contributor"),
            "drawn": frame.get("drawn"),
        }
        counts = require_counts(frame_counts_record, name)
        if counts[1] != source_count:
            raise ValidationError(f"{name}.source does not match the dataset")
        backend = frame.get("order_backend")
        projected_execution = frame.get("projected_execution")
        if backend not in {"cpu", "gpu"}:
            raise ValidationError(f"{name}.order_backend is invalid")
        if projected_execution not in {"candidate", "compact"}:
            raise ValidationError(f"{name}.projected_execution is invalid")
        validate_plan_counts(
            identity,
            counts,
            backend=backend,
            projected_execution=projected_execution,
            exact_compaction=frame.get("exact_contributor_compaction"),
            name=name,
        )
        join: SampleJoin = (ticket, index, sample_key, trace_key, identity)
        issued[ticket] = (join, counts)

        if frame.get("geometry_submit_ms") is not None:
            raise ValidationError(f"{name}.geometry_submit_ms must be null")
        preprocess = frame.get("preprocess_ms")
        sort = frame.get("sort_ms")
        if (preprocess is None) != (sort is None):
            raise ValidationError(f"{name} CPU preprocess/sort availability must match")
        if preprocess is None:
            has_missing_cpu_phase = True
        else:
            require_number(preprocess, f"{name}.preprocess_ms")
            require_number(sort, f"{name}.sort_ms")
            order_submission_ticket = require_int(
                frame.get("order_submission_ticket"),
                f"{name}.order_submission_ticket",
                positive=True,
            )
            order_terminal_ticket = require_int(
                frame.get("order_measurement_ticket"),
                f"{name}.order_measurement_ticket",
                positive=True,
            )
            require_number(
                frame.get("cpu_frame_complete_ms"), f"{name}.cpu_frame_complete_ms"
            )
            if backend != "cpu" or order_submission_ticket != order_terminal_ticket:
                raise ValidationError(
                    f"{name} CPU phase timing is not bound to the same sample ticket"
                )

    if has_missing_cpu_phase and not {
        "frames[*].preprocess_ms",
        "frames[*].sort_ms",
    }.issubset(unavailable):
        raise ValidationError("unavailable CPU phase timing must be declared")
    return issued, presentation_sequences


def validate_ledger(
    summary: dict[str, Any],
    frames: list[dict[str, Any]],
    frame_issued: dict[int, tuple[SampleJoin, Counts]],
) -> None:
    ledger = require_object(
        summary.get("current_stats_terminal_ledger"),
        "current_stats_terminal_ledger",
    )
    submissions = require_array(
        ledger.get("submissions"), "current_stats_terminal_ledger.submissions"
    )
    successes = require_array(
        ledger.get("successes"), "current_stats_terminal_ledger.successes"
    )
    failures = require_array(
        ledger.get("failures"), "current_stats_terminal_ledger.failures"
    )

    submitted: dict[int, SampleJoin] = {}
    for index, raw in enumerate(submissions):
        name = f"current_stats_terminal_ledger.submissions[{index}]"
        join = require_sample_join(require_object(raw, name), name)
        if join[0] in submitted:
            raise ValidationError(f"current-stats ticket {join[0]} was submitted twice")
        submitted[join[0]] = join

    completed: dict[int, tuple[SampleJoin, Counts]] = {}
    for index, raw in enumerate(successes):
        name = f"current_stats_terminal_ledger.successes[{index}]"
        success = require_object(raw, name)
        join = require_sample_join(success, name)
        counts = require_counts(success, name)
        if success.get("outcome") != "ready":
            raise ValidationError(f"{name}.outcome must be ready")
        if join[0] in completed:
            raise ValidationError(f"current-stats ticket {join[0]} has multiple Ready terminals")
        if submitted.get(join[0]) != join:
            raise ValidationError(f"current-stats ticket {join[0]} changed full terminal identity")
        completed[join[0]] = (join, counts)

    if failures:
        for index, raw in enumerate(failures):
            name = f"current_stats_terminal_ledger.failures[{index}]"
            failure = require_object(raw, name)
            join = require_sample_join(failure, name)
            if failure.get("outcome") != "failure":
                raise ValidationError(f"{name}.outcome must be failure")
            if failure.get("failure_reason") not in FAILURE_REASONS:
                raise ValidationError(f"{name}.failure_reason is invalid")
            if submitted.get(join[0]) != join:
                raise ValidationError(f"current-stats ticket {join[0]} changed failure identity")
        raise ValidationError("formal Apple evidence contains a current-stats terminal failure")

    if set(submitted) != set(frame_issued):
        raise ValidationError("measured frames and current-stats submissions differ")
    missing = sorted(set(submitted) - set(completed))
    if missing:
        raise ValidationError(f"current-stats terminal ledger is missing Ready tickets: {missing}")
    stale = sorted(set(completed) - set(submitted))
    if stale:
        raise ValidationError(f"current-stats terminal ledger contains stale tickets: {stale}")
    for ticket, expected in frame_issued.items():
        if submitted.get(ticket) != expected[0]:
            raise ValidationError(
                f"measured frame current-stats ticket {ticket} changed sample or full identity"
            )
        if completed.get(ticket) != expected:
            raise ValidationError(
                f"measured frame current-stats ticket {ticket} changed terminal S/V/C/D"
            )

    expected_counts = {
        "issued_count": len(submitted),
        "success_count": len(completed),
        "failure_count": 0,
    }
    for key, value in expected_counts.items():
        if ledger.get(key) != value:
            raise ValidationError(f"current_stats_terminal_ledger.{key} mismatch")
    if len(frame_issued) != len(frames):
        raise ValidationError("every measured frame must have one current-stats Ready receipt")


def validate(path: pathlib.Path) -> None:
    manifest = require_object(json.loads((path / "manifest.json").read_text()), "manifest")
    summary = require_object(json.loads((path / "summary.json").read_text()), "summary")
    frames = [
        require_object(json.loads(line), f"frames[{index}]")
        for index, line in enumerate((path / "frames.jsonl").read_text().splitlines())
        if line.strip()
    ]
    renderer = require_object(manifest.get("renderer"), "manifest.renderer")
    version = renderer.get("current_stats_evidence_version")
    if isinstance(version, bool) or not isinstance(version, int):
        raise ValidationError(
            "manifest.renderer.current_stats_evidence_version must be the integer 0 or 1"
        )
    if version == LEGACY_CURRENT_STATS_EVIDENCE_VERSION:
        if any(any(key.startswith("current_stats_") for key in frame) for frame in frames):
            raise ValidationError("legacy current-stats evidence contains current frame fields")
        if "current_stats_terminal_ledger" in summary:
            raise ValidationError("legacy current-stats evidence contains a terminal ledger")
        return
    if version != CURRENT_STATS_EVIDENCE_VERSION:
        raise ValidationError(
            "manifest.renderer.current_stats_evidence_version must equal 0 or 1"
        )
    if not frames:
        raise ValidationError("current-stats evidence has no measured frames")
    frame_issued, _ = validate_frames(manifest, frames)
    validate_ledger(summary, frames, frame_issued)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact", type=pathlib.Path)
    args = parser.parse_args()
    try:
        validate(args.artifact)
    except (OSError, json.JSONDecodeError, ValidationError) as error:
        parser.error(str(error))
    print(f"valid_ios_current_stats_artifact={args.artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
