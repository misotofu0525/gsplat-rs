#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

FIXTURE="$ROOT/tests/perf/fixtures/v1/valid"
LOG="$TMP_DIR/logcat.txt"
{
  echo "I/GsplatExample(123): unrelated"
  printf 'I/GsplatExample(123): GSPLAT_BENCHMARK_MANIFEST %s\n' "$(tr -d '\n' <"$FIXTURE/manifest.json")"
  while IFS= read -r frame; do
    [[ -n "$frame" ]] || continue
    printf 'I/GsplatExample(123): GSPLAT_BENCHMARK_FRAME %s\n' "$frame"
  done <"$FIXTURE/frames.jsonl"
  printf 'I/GsplatExample(123): GSPLAT_BENCHMARK_SUMMARY %s\n' "$(tr -d '\n' <"$FIXTURE/summary.json")"
} >"$LOG"

python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$LOG" \
  "$TMP_DIR/artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"

# A declared strict Android artifact must pass the same current-stats ledger
# validator as the full device collector.
STRICT_LOG="$TMP_DIR/strict-logcat.txt"
MISSING_LEDGER_LOG="$TMP_DIR/strict-missing-ledger-logcat.txt"
ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT_LOG="$TMP_DIR/adaptive-plan-backend-drift.txt"
ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT_LOG="$TMP_DIR/producer-count-drift.txt"
BAD_GPU_COUNT_SEMANTICS_LOG="$TMP_DIR/bad-gpu-count-semantics.txt"
python3 - \
  "$FIXTURE" \
  "$STRICT_LOG" \
  "$MISSING_LEDGER_LOG" \
  "$ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT_LOG" \
  "$ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT_LOG" \
  "$BAD_GPU_COUNT_SEMANTICS_LOG" <<'PY'
import copy
import json
import pathlib
import sys

fixture = pathlib.Path(sys.argv[1])
strict_destination = pathlib.Path(sys.argv[2])
missing_destination = pathlib.Path(sys.argv[3])
adaptive_drift_destination = pathlib.Path(sys.argv[4])
producer_drift_destination = pathlib.Path(sys.argv[5])
bad_gpu_semantics_destination = pathlib.Path(sys.argv[6])
manifest = json.loads((fixture / "manifest.json").read_text())
summary = json.loads((fixture / "summary.json").read_text())
frames = [
    json.loads(line)
    for line in (fixture / "frames.jsonl").read_text().splitlines()
    if line
]
manifest["renderer"].update(
    {
        "order_backend_requested": "cpu",
        "current_stats_schema": "gsplat-surface-current-stats/v1",
        "current_stats_strict": True,
        "count_source": "matching_current_stats_ready",
        "count_semantics": "candidate_visible_contributor_issued_v1",
        "gpu_producer_measurement_enabled": False,
    }
)
manifest["exactness"] = {"receipt_id": "strict-fixture-exactness"}
manifest["timing_contract"] = {
    "call_ms": "host_camera_request_render_transaction_wall",
    "frame_wall_ms": "host_iteration_request_through_receipt_queries",
    "preprocess_ms": "matching_cpu_order_terminal_only",
    "sort_ms": "matching_cpu_order_terminal_only",
    "raster_ms": None,
}
for unavailable in (
    "frames[*].cpu_frame_complete_ms",
    "frames[*].raster_ms",
):
    if unavailable not in manifest["unavailable_fields"]:
        manifest["unavailable_fields"].append(unavailable)

ledger = []
for index, frame in enumerate(frames):
    revision = index + 1
    ticket = 1_000 + index
    presentation_sequence = 2_000 + index
    identity = {
        "scene_generation": 1,
        "camera_revision": revision,
        "viewport_generation": 2,
        "contract_generation": 3,
        "plan_set_generation": 4,
        "order_generation": 5 + index,
        "raster_generation": 6,
        "encode_attempt": 100 + index,
        "presentation_sequence": presentation_sequence,
        "executed_plan": "cpu_post_sort",
    }
    frame.update(
        {
            "camera_revision": revision,
            "trace_frame_index": None,
            "trace_timestamp_ns": None,
            "contributor": frame["visible"],
            "exact_contributor_compaction": False,
            "raster_ms": None,
            "cpu_frame_complete_ms": None,
            "order_submission_ticket": ticket,
            "order_measurement_ticket": ticket,
            "order_measurement_camera_revision": revision,
            "current_stats_ticket": ticket,
            "current_stats_presentation_sequence": presentation_sequence,
            "current_stats_executed_plan": "cpu_post_sort",
            "order_backend": "cpu",
            "camera_receipt": {
                "camera_revision": revision,
                "presented_camera_revision": revision,
            },
        }
    )
    ledger.append(
        {
            "sample_index": index,
            "trace_frame_index": None,
            "trace_timestamp_ns": None,
            "request_status": "requested",
            "submission_status": "issued",
            "ticket": ticket,
            "identity": identity,
            "outcome": "ready",
            "source": manifest["dataset"]["splat_count"],
            "visible": frame["visible"],
            "contributor": frame["contributor"],
            "drawn": frame["drawn"],
            "count_semantics": "indirect_draw_equals_visible",
            "exactness_receipt_id": "strict-fixture-exactness",
        }
    )
summary["current_stats_terminal_ledger"] = ledger


def write_log(destination, manifest_value, frames_value, summary_value):
    lines = [
        "I/GsplatExample(123): GSPLAT_BENCHMARK_MANIFEST "
        + json.dumps(manifest_value, separators=(",", ":"))
    ]
    lines.extend(
        "I/GsplatExample(123): GSPLAT_BENCHMARK_FRAME "
        + json.dumps(frame, separators=(",", ":"))
        for frame in frames_value
    )
    lines.append(
        "I/GsplatExample(123): GSPLAT_BENCHMARK_SUMMARY "
        + json.dumps(summary_value, separators=(",", ":"))
    )
    destination.write_text("\n".join(lines) + "\n")


write_log(strict_destination, manifest, frames, summary)
missing = dict(summary)
missing.pop("current_stats_terminal_ledger")
write_log(missing_destination, manifest, frames, missing)

# Regression fixture that bd44 previously accepted:
# ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT.
adaptive_manifest = copy.deepcopy(manifest)
adaptive_frames = copy.deepcopy(frames)
adaptive_summary = copy.deepcopy(summary)
adaptive_manifest["renderer"]["order_backend_requested"] = "adaptive"
for frame, entry in zip(adaptive_frames, adaptive_summary["current_stats_terminal_ledger"]):
    entry["identity"]["executed_plan"] = "gpu_post_sort"
    frame["current_stats_executed_plan"] = "gpu_post_sort"
    frame["order_backend"] = "cpu"
write_log(
    adaptive_drift_destination,
    adaptive_manifest,
    adaptive_frames,
    adaptive_summary,
)

# Regression fixture that bd44 previously accepted:
# ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT.
producer_manifest = copy.deepcopy(manifest)
producer_frames = copy.deepcopy(frames)
producer_summary = copy.deepcopy(summary)
producer_manifest["renderer"].update(
    {
        "order_backend_requested": "gpu",
        "raster_plan": "projected_quads_exact",
        "projected_policy_requested": "compact",
        "gpu_order_producer_requested": "preproject",
        "gpu_producer_measurement_enabled": True,
    }
)
producer_terminals = []
for index, (frame, entry) in enumerate(
    zip(producer_frames, producer_summary["current_stats_terminal_ledger"])
):
    producer_ticket = 3_000 + index
    producer_contributor = entry["visible"]
    entry["identity"]["executed_plan"] = "gpu_preproject"
    entry["contributor"] = producer_contributor - 1
    entry["drawn"] = producer_contributor - 1
    entry["count_semantics"] = "indirect_draw_equals_contributor"
    frame.update(
        {
            "contributor": entry["contributor"],
            "drawn": entry["drawn"],
            "exact_contributor_compaction": True,
            "current_stats_executed_plan": "gpu_preproject",
            "order_backend": "gpu",
            "gpu_order_producer": "preproject",
            "gpu_producer_measurement_ticket": producer_ticket,
            "gpu_producer_measurement_camera_revision": entry["identity"][
                "camera_revision"
            ],
            "gpu_producer_order_generation": entry["identity"]["order_generation"],
            "gpu_producer_projection_generation": 4_000 + index,
            "gpu_producer_draw_scope": "exact_current_contributors",
            "gpu_producer_order_refreshed": True,
            "gpu_producer_exact_current_draw": True,
            "gpu_producer_stale_order": False,
            "gpu_producer_dropped_prior": False,
            "gpu_producer_submission_flags": 9,
            "gpu_producer_source": entry["source"],
            "gpu_producer_contributor": producer_contributor,
            "gpu_producer_drawn": producer_contributor,
        }
    )
    producer_terminals.append(
        {
            "ticket": producer_ticket,
            "camera_revision": entry["identity"]["camera_revision"],
            "producer": "preproject",
            "outcome": "success",
            "order_generation": entry["identity"]["order_generation"],
            "projection_generation": 4_000 + index,
            "source": entry["source"],
            "contributor": producer_contributor,
            "drawn": producer_contributor,
            "draw_scope": "exact_current_contributors",
            "exactness_receipt_id": entry["exactness_receipt_id"],
        }
    )
producer_summary["gpu_producer_terminal_ledger"] = producer_terminals
write_log(
    producer_drift_destination,
    producer_manifest,
    producer_frames,
    producer_summary,
)

bad_semantics_manifest = copy.deepcopy(manifest)
bad_semantics_manifest["renderer"][
    "gpu_count_semantics"
] = "source_count_upper_bound; sort-all/draw-all"
write_log(
    bad_gpu_semantics_destination,
    bad_semantics_manifest,
    frames,
    summary,
)
PY

python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$STRICT_LOG" \
  "$TMP_DIR/strict-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$MISSING_LEDGER_LOG" \
  "$TMP_DIR/strict-missing-ledger-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor unexpectedly accepted strict current-stats without a ledger" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT_LOG" \
  "$TMP_DIR/adaptive-plan-backend-drift-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor accepted ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT_LOG" \
  "$TMP_DIR/producer-count-drift-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor accepted ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$BAD_GPU_COUNT_SEMANTICS_LOG" \
  "$TMP_DIR/bad-gpu-count-semantics-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor accepted dishonest gpu_count_semantics" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$LOG" \
  "$TMP_DIR/artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor unexpectedly reused an existing destination" >&2
  exit 1
fi

CHUNKED_LOG="$TMP_DIR/chunked-logcat.txt"
python3 - "$FIXTURE" "$CHUNKED_LOG" <<'PY'
import base64
import hashlib
import json
import pathlib
import sys

fixture = pathlib.Path(sys.argv[1])
destination = pathlib.Path(sys.argv[2])
manifest = "".join((fixture / "manifest.json").read_text().splitlines())
frames = [line for line in (fixture / "frames.jsonl").read_text().splitlines() if line]
summary = "".join((fixture / "summary.json").read_text().splitlines())
summary_bytes = summary.encode("utf-8")
run_id = json.loads(summary)["run_id"]
digest = hashlib.sha256(summary_bytes).hexdigest()
chunks = [summary_bytes[index : index + 47] for index in range(0, len(summary_bytes), 47)]
lines = [f"I/GsplatExample(123): GSPLAT_BENCHMARK_MANIFEST {manifest}"]
lines.extend(f"I/GsplatExample(123): GSPLAT_BENCHMARK_FRAME {frame}" for frame in frames)
for index, chunk in enumerate(chunks):
    payload = base64.b64encode(chunk).decode("ascii")
    lines.append(
        "I/GsplatExample(123): GSPLAT_BENCHMARK_CHUNK "
        f"record=summary run_id={run_id} index={index} total={len(chunks)} "
        f"encoding=base64 sha256={digest} payload={payload}"
    )
destination.write_text("\n".join(lines) + "\n")
PY

python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$CHUNKED_LOG" \
  "$TMP_DIR/chunked-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"
python3 - "$FIXTURE/summary.json" "$TMP_DIR/chunked-artifact/summary.json" <<'PY'
import json
import pathlib
import sys

assert json.loads(pathlib.Path(sys.argv[1]).read_text()) == json.loads(
    pathlib.Path(sys.argv[2]).read_text()
)
PY

# The live collector must not stop logcat after merely seeing chunk zero.
python3 - "$ROOT/bindings/android/scripts/collect-android-sort-benchmarks.py" \
  "$CHUNKED_LOG" <<'PY'
import importlib.util
import pathlib
import sys

spec = importlib.util.spec_from_file_location("android_collector", sys.argv[1])
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
lines = pathlib.Path(sys.argv[2]).read_text().splitlines()
first_chunk = next(
    index for index, line in enumerate(lines)
    if "GSPLAT_BENCHMARK_CHUNK record=summary " in line
)
assert not module.has_complete_summary_artifact("\n".join(lines[: first_chunk + 1]))
assert module.has_complete_summary_artifact("\n".join(lines))
assert module.has_complete_summary_artifact(
    'I/GsplatExample: GSPLAT_BENCHMARK_SUMMARY {"record_type":"summary"}'
)
PY

# Dropping one terminal chunk must fail closed rather than publishing a
# truncated ledger as a valid benchmark summary.
sed '/index=1 /d' "$CHUNKED_LOG" >"$TMP_DIR/missing-chunk-logcat.txt"
if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$TMP_DIR/missing-chunk-logcat.txt" \
  "$TMP_DIR/missing-chunk-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor unexpectedly accepted an incomplete chunked summary" >&2
  exit 1
fi

echo "Android benchmark artifact extraction tests passed"
