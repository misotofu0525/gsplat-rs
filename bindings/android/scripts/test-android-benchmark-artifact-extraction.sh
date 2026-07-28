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

# Formal image publication accepts only an exact 2412x1080 PNG paired with the
# collector's post-completion app-sandbox pull receipt.
FORMAL_PNG="$TMP_DIR/device-final-frame.png"
FORMAL_PNG_RECEIPT="$TMP_DIR/device-png-pull-receipt.json"
FORMAL_LOG="$TMP_DIR/formal-logcat.txt"
python3 - "$LOG" "$FORMAL_LOG" <<'PY'
import json
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text().splitlines()
result = []
marker = "GSPLAT_BENCHMARK_MANIFEST "
for line in source:
    if marker in line:
        prefix, raw = line.split(marker, 1)
        manifest = json.loads(raw)
        manifest["display"]["width"] = 2412
        manifest["display"]["height"] = 1080
        line = prefix + marker + json.dumps(manifest, separators=(",", ":"))
    result.append(line)
pathlib.Path(sys.argv[2]).write_text("\n".join(result) + "\n")
PY
python3 - "$FIXTURE/manifest.json" "$FORMAL_PNG" "$FORMAL_PNG_RECEIPT" <<'PY'
import hashlib
import json
import pathlib
import struct
import sys
import zlib

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
image_path = pathlib.Path(sys.argv[2])
receipt_path = pathlib.Path(sys.argv[3])
width, height = 2412, 1080

def chunk(kind, payload):
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xffffffff)
    )

rows = b"".join(b"\0" + b"\0\0\0\xff" * width for _ in range(height))
data = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(rows, 9))
    + chunk(b"IEND", b"")
)
image_path.write_bytes(data)
receipt_path.write_text(json.dumps({
    "schema": "gsplat-android-device-png-pull/v1",
    "source": "adb-exec-out-run-as-after-benchmark-complete",
    "package": "com.gsplat.example",
    "device_path": "files/benchmark-final-frame.png",
    "device_path_absent_after_package_clear": True,
    "benchmark_completed": True,
    "pulled_after_completed_log": True,
    "benchmark_run_id": manifest["run_id"],
    "device_identity": {
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    },
    "local_identity": {
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "width": width,
        "height": height,
    },
}, sort_keys=True))
PY

python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$FORMAL_LOG" \
  "$TMP_DIR/formal-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --final-png "$FORMAL_PNG" \
  --device-png-pull-receipt "$FORMAL_PNG_RECEIPT"
cmp "$FORMAL_PNG" "$TMP_DIR/formal-artifact/final-frame.png"

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$FORMAL_LOG" \
  "$TMP_DIR/missing-pull-receipt-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --final-png "$FORMAL_PNG"; then
  echo "extractor accepted a formal PNG without its device pull receipt" >&2
  exit 1
fi
[[ ! -e "$TMP_DIR/missing-pull-receipt-artifact" ]]

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$FORMAL_LOG" \
  "$TMP_DIR/missing-final-png-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --final-png "$TMP_DIR/does-not-exist.png" \
  --device-png-pull-receipt "$FORMAL_PNG_RECEIPT"; then
  echo "extractor accepted a missing formal PNG" >&2
  exit 1
fi
[[ ! -e "$TMP_DIR/missing-final-png-artifact" ]]

for mutation in wrong-run wrong-hash stale-path incomplete malformed-image wrong-size; do
  MUTATED_PNG="$TMP_DIR/$mutation.png"
  MUTATED_RECEIPT="$TMP_DIR/$mutation-receipt.json"
  cp "$FORMAL_PNG" "$MUTATED_PNG"
  cp "$FORMAL_PNG_RECEIPT" "$MUTATED_RECEIPT"
  python3 - "$mutation" "$MUTATED_PNG" "$MUTATED_RECEIPT" <<'PY'
import json
import pathlib
import struct
import sys

mutation = sys.argv[1]
image = pathlib.Path(sys.argv[2])
receipt_path = pathlib.Path(sys.argv[3])
receipt = json.loads(receipt_path.read_text())
if mutation == "wrong-run":
    receipt["benchmark_run_id"] = "old-run"
elif mutation == "wrong-hash":
    receipt["device_identity"]["sha256"] = "0" * 64
elif mutation == "stale-path":
    receipt["device_path_absent_after_package_clear"] = False
elif mutation == "incomplete":
    receipt["benchmark_completed"] = False
elif mutation == "malformed-image":
    image.write_bytes(b"not a png")
elif mutation == "wrong-size":
    data = bytearray(image.read_bytes())
    data[16:24] = struct.pack(">II", 1920, 1080)
    image.write_bytes(data)
receipt_path.write_text(json.dumps(receipt))
PY
  if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
    "$FORMAL_LOG" \
    "$TMP_DIR/$mutation-artifact" \
    --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
    --final-png "$MUTATED_PNG" \
    --device-png-pull-receipt "$MUTATED_RECEIPT"; then
    echo "extractor accepted invalid formal PNG mutation: $mutation" >&2
    exit 1
  fi
  [[ ! -e "$TMP_DIR/$mutation-artifact" ]]
done

# A declared strict Android artifact must pass the same current-stats ledger
# validator as the full device collector.
STRICT_LOG="$TMP_DIR/strict-logcat.txt"
MISSING_LEDGER_LOG="$TMP_DIR/strict-missing-ledger-logcat.txt"
MISMATCHED_REFRESH_TICKET_LOG="$TMP_DIR/strict-mismatched-refresh-ticket-logcat.txt"
ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT_LOG="$TMP_DIR/adaptive-plan-backend-drift.txt"
ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT_LOG="$TMP_DIR/producer-count-drift.txt"
BAD_GPU_COUNT_SEMANTICS_LOG="$TMP_DIR/bad-gpu-count-semantics.txt"
PRODUCER_GATE_MUTATION_DIR="$TMP_DIR/producer-gate-mutations"
WRONG_GPU_ACTUAL_PLAN_LOG="$TMP_DIR/wrong-gpu-actual-plan.txt"
ANDROID_ENVIRONMENT_RECEIPT="$TMP_DIR/android-environment-receipt.json"
MISMATCHED_ANDROID_ENVIRONMENT_RECEIPT="$TMP_DIR/mismatched-android-environment-receipt.json"
mkdir -p "$PRODUCER_GATE_MUTATION_DIR"
python3 - \
  "$FIXTURE" \
  "$STRICT_LOG" \
  "$MISSING_LEDGER_LOG" \
  "$MISMATCHED_REFRESH_TICKET_LOG" \
  "$ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT_LOG" \
  "$ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT_LOG" \
  "$BAD_GPU_COUNT_SEMANTICS_LOG" \
  "$PRODUCER_GATE_MUTATION_DIR" \
  "$WRONG_GPU_ACTUAL_PLAN_LOG" \
  "$ANDROID_ENVIRONMENT_RECEIPT" \
  "$MISMATCHED_ANDROID_ENVIRONMENT_RECEIPT" <<'PY'
import copy
import json
import pathlib
import sys

fixture = pathlib.Path(sys.argv[1])
strict_destination = pathlib.Path(sys.argv[2])
missing_destination = pathlib.Path(sys.argv[3])
mismatched_refresh_ticket_destination = pathlib.Path(sys.argv[4])
adaptive_drift_destination = pathlib.Path(sys.argv[5])
producer_drift_destination = pathlib.Path(sys.argv[6])
bad_gpu_semantics_destination = pathlib.Path(sys.argv[7])
producer_gate_mutation_directory = pathlib.Path(sys.argv[8])
wrong_gpu_actual_plan_destination = pathlib.Path(sys.argv[9])
environment_receipt_destination = pathlib.Path(sys.argv[10])
mismatched_environment_receipt_destination = pathlib.Path(sys.argv[11])
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
manifest["environment"].update(
    {
        "platform": "android-native",
        "os": "Android 16 (API 36)",
        "device": "Fixture Phone (fixture_device)",
        "adapter": None,
        "driver": None,
        "hardware": "fixture-hardware",
    }
)
environment_receipt = {
    "schema": "gsplat-android-environment-receipt/v2",
    "source": "adb_getprop",
    "serial": "fixture-serial",
    "renderer_identity": {
        "adapter": {
            "source": "benchmark_manifest",
            "path": "environment.adapter",
        },
        "driver": {
            "source": "benchmark_manifest",
            "path": "environment.driver",
        },
        "backend": {
            "source": "benchmark_manifest",
            "path": "renderer.backend",
        },
    },
    "manufacturer": "Fixture",
    "model": "Phone",
    "device": "fixture_device",
    "android_release": "16",
    "android_sdk": "36",
    "hardware": "fixture-hardware",
    "build_fingerprint": "fixture/device/build:16/TEST/1:userdebug/test-keys",
    "device_properties": {
        "soc_manufacturer_property": {
            "getprop": "ro.soc.manufacturer",
            "value": "Fixture Silicon",
        },
        "soc_model_property": {
            "getprop": "ro.soc.model",
            "value": "F1",
        },
        "board_platform_property": {
            "getprop": "ro.board.platform",
            "value": "fixture-board",
        },
        "vulkan_hal_property": {
            "getprop": "ro.hardware.vulkan",
            "value": "vulkan.fixture",
        },
        "gfx_driver_0_property": {
            "getprop": "ro.gfx.driver.0",
            "value": None,
        },
    },
}
environment_receipt_destination.write_text(json.dumps(environment_receipt) + "\n")
mismatched_environment_receipt = copy.deepcopy(environment_receipt)
mismatched_environment_receipt["manufacturer"] = "Other"
mismatched_environment_receipt_destination.write_text(
    json.dumps(mismatched_environment_receipt) + "\n"
)
manifest["timing_contract"] = {
    "call_ms": "host_camera_request_render_transaction_wall",
    "frame_wall_ms": "host_iteration_request_through_receipt_queries",
    "preprocess_ms": "matching_cpu_order_terminal_only",
    "sort_ms": "matching_cpu_order_terminal_only",
    "raster_ms": None,
}
for unavailable in (
    "environment.adapter",
    "environment.driver",
    "frames[*].cpu_frame_complete_ms",
    "frames[*].raster_ms",
):
    if unavailable not in manifest["unavailable_fields"]:
        manifest["unavailable_fields"].append(unavailable)

ledger = []
order_ledger = []
for index, frame in enumerate(frames):
    revision = index + 1
    current_stats_ticket = 1_000 + index
    order_ticket = 10_000 + index
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
            "sort_refreshed": True,
            "exact_contributor_compaction": False,
            "raster_ms": None,
            "cpu_frame_complete_ms": None,
            "order_measurement_ticket_issued": True,
            "order_submission_ticket": order_ticket,
            "order_measurement_ticket": order_ticket,
            "order_measurement_camera_revision": revision,
            "current_stats_ticket": current_stats_ticket,
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
            "ticket": current_stats_ticket,
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
    order_ledger.append(
        {
            "ticket": order_ticket,
            "camera_revision": revision,
            "backend": "cpu",
            "exactness_receipt_id": "strict-fixture-exactness",
            "outcome": "success",
            "frame_complete_ms": 1.0,
            "visible": frame["visible"],
            "contributor": frame["contributor"],
            "drawn": frame["drawn"],
            "exact_contributor_compaction": False,
        }
    )
summary["current_stats_terminal_ledger"] = ledger
summary["order_terminal_ledger"] = order_ledger
summary.setdefault("sort_telemetry", {}).update(
    {
        "order_measurement_scheduled_count": len(order_ledger),
        "cpu_order_measurement_completed_count": len(order_ledger),
        "gpu_order_measurement_completed_count": 0,
        "order_measurement_terminal_failure_count": 0,
    }
)


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
mismatched_refresh_ticket_frames = copy.deepcopy(frames)
mismatched_refresh_ticket_summary = copy.deepcopy(summary)
mismatched_order_ticket = 9_000_000
mismatched_refresh_ticket_frames[0]["order_submission_ticket"] = mismatched_order_ticket
mismatched_refresh_ticket_frames[0]["order_measurement_ticket"] = mismatched_order_ticket
write_log(
    mismatched_refresh_ticket_destination,
    manifest,
    mismatched_refresh_ticket_frames,
    mismatched_refresh_ticket_summary,
)
missing_renderer_identity_manifest = copy.deepcopy(manifest)
missing_renderer_identity_manifest["environment"].pop("adapter")
write_log(
    producer_gate_mutation_directory / "missing-renderer-identity.txt",
    missing_renderer_identity_manifest,
    frames,
    summary,
)
undeclared_renderer_identity_manifest = copy.deepcopy(manifest)
undeclared_renderer_identity_manifest["unavailable_fields"].remove(
    "environment.driver"
)
write_log(
    producer_gate_mutation_directory / "undeclared-renderer-identity.txt",
    undeclared_renderer_identity_manifest,
    frames,
    summary,
)
nonempty_renderer_identity_manifest = copy.deepcopy(manifest)
nonempty_renderer_identity_manifest["environment"]["adapter"] = (
    "Fixture wgpu adapter"
)
nonempty_renderer_identity_manifest["environment"]["driver"] = (
    "Fixture Vulkan driver"
)
nonempty_renderer_identity_manifest["renderer"]["backend"] = "vulkan"
nonempty_renderer_identity_manifest["unavailable_fields"].remove(
    "environment.adapter"
)
nonempty_renderer_identity_manifest["unavailable_fields"].remove(
    "environment.driver"
)
write_log(
    producer_gate_mutation_directory / "nonempty-renderer-identity.txt",
    nonempty_renderer_identity_manifest,
    frames,
    summary,
)
wrong_gpu_manifest = copy.deepcopy(manifest)
wrong_gpu_frames = copy.deepcopy(frames)
wrong_gpu_summary = copy.deepcopy(summary)
wrong_gpu_manifest["renderer"]["order_backend_requested"] = "gpu"
for frame, entry in zip(
    wrong_gpu_frames,
    wrong_gpu_summary["current_stats_terminal_ledger"],
):
    entry["identity"]["executed_plan"] = "gpu_preproject"
    entry["drawn"] = entry["contributor"]
    entry["count_semantics"] = "indirect_draw_equals_contributor"
    frame.update(
        {
            "current_stats_executed_plan": "gpu_preproject",
            "order_backend": "gpu",
            "drawn": entry["contributor"],
            "exact_contributor_compaction": True,
        }
    )
write_log(
    wrong_gpu_actual_plan_destination,
    wrong_gpu_manifest,
    wrong_gpu_frames,
    wrong_gpu_summary,
)
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
        "path": "packed_atlas",
        "raster_plan": "projected_quads_exact",
        "projected_policy_requested": "candidate",
        "gpu_order_producer_requested": "post_sort",
        "gpu_producer_measurement_enabled": True,
    }
)
producer_terminals = []
for index, (frame, entry) in enumerate(
    zip(producer_frames, producer_summary["current_stats_terminal_ledger"])
):
    producer_ticket = 3_000 + index
    producer_contributor = entry["visible"] - 1
    entry["identity"]["executed_plan"] = "gpu_post_sort"
    entry["contributor"] = producer_contributor - 1
    entry["count_semantics"] = "indirect_draw_equals_visible"
    order_entry = producer_summary["order_terminal_ledger"][index]
    order_entry.update(
        {
            "backend": "gpu",
            "visible": entry["visible"],
            "contributor": entry["contributor"],
            "drawn": entry["drawn"],
            "exact_contributor_compaction": False,
        }
    )
    frame.update(
        {
            "contributor": entry["contributor"],
            "drawn": entry["drawn"],
            "exact_contributor_compaction": False,
            "current_stats_executed_plan": "gpu_post_sort",
            "order_backend": "gpu",
            "gpu_order_producer": "post_sort",
            "gpu_producer_measurement_ticket": producer_ticket,
            "gpu_producer_measurement_camera_revision": entry["identity"][
                "camera_revision"
            ],
            "gpu_producer_order_generation": entry["identity"]["order_generation"],
            "gpu_producer_projection_generation": 4_000 + index,
            "gpu_producer_frame_complete_ms": 4.5 + index,
            "gpu_producer_draw_scope": "exact_current_candidates",
            "gpu_producer_order_refreshed": True,
            "gpu_producer_exact_current_draw": True,
            "gpu_producer_stale_order": False,
            "gpu_producer_dropped_prior": False,
            "gpu_producer_submission_flags": 9,
            "gpu_producer_source": entry["source"],
            "gpu_producer_contributor": producer_contributor,
            "gpu_producer_drawn": entry["visible"],
        }
    )
    producer_terminals.append(
        {
            "ticket": producer_ticket,
            "camera_revision": entry["identity"]["camera_revision"],
            "producer": "post_sort",
            "outcome": "success",
            "order_generation": entry["identity"]["order_generation"],
            "projection_generation": 4_000 + index,
            "source": entry["source"],
            "contributor": producer_contributor,
            "drawn": entry["visible"],
            "draw_scope": "exact_current_candidates",
            "exactness_receipt_id": entry["exactness_receipt_id"],
        }
    )
producer_summary["gpu_producer_telemetry"] = {
    "requested_producer": "post_sort",
    "scheduled_count": len(producer_frames),
    "completed_count": len(producer_frames),
    "failure_count": 0,
    "unsampled_count": 0,
    "exact_current_count": len(producer_frames),
    "stale_count": 0,
    "dropped_count": 0,
    "order_refreshed_count": len(producer_frames),
    "frame_complete_ms": {"count": len(producer_frames)},
}
producer_summary["gpu_producer_terminal_ledger"] = producer_terminals
write_log(
    producer_drift_destination,
    producer_manifest,
    producer_frames,
    producer_summary,
)

for label, value in (
    ("missing-flag", None),
    ("null-flag", None),
    ("string-true", "true"),
):
    mutated_manifest = copy.deepcopy(manifest)
    if label == "missing-flag":
        mutated_manifest["renderer"].pop("gpu_producer_measurement_enabled")
    else:
        mutated_manifest["renderer"]["gpu_producer_measurement_enabled"] = value
    write_log(
        producer_gate_mutation_directory / f"{label}.txt",
        mutated_manifest,
        frames,
        summary,
    )

disabled_manifest = copy.deepcopy(producer_manifest)
disabled_manifest["renderer"]["gpu_producer_measurement_enabled"] = False
write_log(
    producer_gate_mutation_directory / "disabled-with-evidence.txt",
    disabled_manifest,
    producer_frames,
    producer_summary,
)

valid_producer_frames = copy.deepcopy(producer_frames)
valid_producer_summary = copy.deepcopy(producer_summary)
for frame, entry in zip(
    valid_producer_frames,
    valid_producer_summary["current_stats_terminal_ledger"],
):
    entry["contributor"] = frame["gpu_producer_contributor"]
    entry["drawn"] = frame["gpu_producer_drawn"]
    frame["contributor"] = entry["contributor"]
    frame["drawn"] = entry["drawn"]
    order_entry = valid_producer_summary["order_terminal_ledger"][entry["sample_index"]]
    order_entry["contributor"] = entry["contributor"]
    order_entry["drawn"] = entry["drawn"]

identity_drift_summary = copy.deepcopy(valid_producer_summary)
identity_drift_terminal = identity_drift_summary["gpu_producer_terminal_ledger"][0]
identity_drift_terminal["camera_revision"] += 1
identity_drift_terminal["order_generation"] += 1
identity_drift_terminal["projection_generation"] += 1
write_log(
    producer_gate_mutation_directory / "terminal-identity-drift.txt",
    producer_manifest,
    valid_producer_frames,
    identity_drift_summary,
)

integer_flag_manifest = copy.deepcopy(producer_manifest)
integer_flag_manifest["renderer"]["gpu_producer_measurement_enabled"] = 1
write_log(
    producer_gate_mutation_directory / "integer-one.txt",
    integer_flag_manifest,
    valid_producer_frames,
    identity_drift_summary,
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
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"
python3 - "$TMP_DIR/strict-artifact/manifest.json" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
receipt = manifest["environment"]["android_device_receipt"]
assert receipt["device_properties"]["gfx_driver_0_property"] == {
    "getprop": "ro.gfx.driver.0",
    "value": None,
}
assert (
    "environment.android_device_receipt.device_properties.gfx_driver_0_property.value"
    in manifest["unavailable_fields"]
)
assert "environment.adapter" in manifest["unavailable_fields"]
assert "environment.driver" in manifest["unavailable_fields"]
PY

python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$PRODUCER_GATE_MUTATION_DIR/nonempty-renderer-identity.txt" \
  "$TMP_DIR/nonempty-renderer-identity-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"

for mutation in missing-renderer-identity undeclared-renderer-identity; do
  if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
    "$PRODUCER_GATE_MUTATION_DIR/$mutation.txt" \
    "$TMP_DIR/$mutation-artifact" \
    --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
    --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
    echo "extractor accepted renderer identity mutation: $mutation" >&2
    exit 1
  fi
  [[ ! -e "$TMP_DIR/$mutation-artifact" ]]
done

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$STRICT_LOG" \
  "$TMP_DIR/strict-missing-environment-receipt-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py"; then
  echo "extractor unexpectedly accepted strict artifact without Android environment receipt" >&2
  exit 1
fi
[[ ! -e "$TMP_DIR/strict-missing-environment-receipt-artifact" ]]

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$WRONG_GPU_ACTUAL_PLAN_LOG" \
  "$TMP_DIR/wrong-gpu-actual-plan-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
  echo "extractor unexpectedly accepted forced GPU actual-plan drift" >&2
  exit 1
fi
[[ ! -e "$TMP_DIR/wrong-gpu-actual-plan-artifact" ]]

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$STRICT_LOG" \
  "$TMP_DIR/strict-mismatched-environment-receipt-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$MISMATCHED_ANDROID_ENVIRONMENT_RECEIPT"; then
  echo "extractor unexpectedly accepted mismatched Android environment receipt" >&2
  exit 1
fi
[[ ! -e "$TMP_DIR/strict-mismatched-environment-receipt-artifact" ]]

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$MISSING_LEDGER_LOG" \
  "$TMP_DIR/strict-missing-ledger-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
  echo "extractor unexpectedly accepted strict current-stats without a ledger" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$MISMATCHED_REFRESH_TICKET_LOG" \
  "$TMP_DIR/strict-mismatched-refresh-ticket-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT" \
  2>"$TMP_DIR/strict-mismatched-refresh-ticket.stderr"; then
  echo "extractor accepted an issued order ticket without a terminal" >&2
  exit 1
fi
grep -F \
  "issued ticket lacks a terminal" \
  "$TMP_DIR/strict-mismatched-refresh-ticket.stderr"
[[ ! -e "$TMP_DIR/strict-mismatched-refresh-ticket-artifact" ]]

for mutation in \
  disabled-with-evidence \
  missing-flag \
  null-flag \
  integer-one \
  string-true \
  terminal-identity-drift; do
  if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
    "$PRODUCER_GATE_MUTATION_DIR/$mutation.txt" \
    "$TMP_DIR/$mutation-artifact" \
    --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
    --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
    echo "extractor accepted GPU producer gate mutation: $mutation" >&2
    exit 1
  fi
done

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT_LOG" \
  "$TMP_DIR/adaptive-plan-backend-drift-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
  echo "extractor accepted ACCEPTED_ADAPTIVE_PLAN_BACKEND_DRIFT" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT_LOG" \
  "$TMP_DIR/producer-count-drift-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
  echo "extractor accepted ACCEPTED_PRODUCER_CURRENT_STATS_COUNT_DRIFT" >&2
  exit 1
fi

if python3 "$ROOT/bindings/android/scripts/extract-android-benchmark-artifacts.py" \
  "$BAD_GPU_COUNT_SEMANTICS_LOG" \
  "$TMP_DIR/bad-gpu-count-semantics-artifact" \
  --validator "$ROOT/tests/perf/validate-benchmark-artifacts.py" \
  --android-environment-receipt "$ANDROID_ENVIRONMENT_RECEIPT"; then
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
lines = ["I/GsplatExample(123): BENCHMARK_RESULT samples=1"]
lines.append(f"I/GsplatExample(123): GSPLAT_BENCHMARK_MANIFEST {manifest}")
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
assert module.completed_benchmark_run_id("\n".join(lines)) == "fixture-valid-001"
assert module.has_complete_summary_artifact(
    'I/GsplatExample: GSPLAT_BENCHMARK_SUMMARY {"record_type":"summary","run_id":"direct-run"}'
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
