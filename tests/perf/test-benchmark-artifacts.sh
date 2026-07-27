#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

VALIDATOR="tests/perf/validate-benchmark-artifacts.py"
VALID="tests/perf/fixtures/v1/valid"

python3 "$VALIDATOR" "$VALID"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cp -R "$VALID" "$TMP_DIR/image-valid"
python3 - "$TMP_DIR/image-valid" <<'PY'
import hashlib, json, pathlib, struct, sys, zlib

root = pathlib.Path(sys.argv[1])
width, height = 640, 480

def chunk(kind, payload):
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xffffffff)

raw = b"".join(b"\x00" + b"\x00\x00\x00\xff" * width for _ in range(height))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
(root / "final-frame.png").write_bytes(png)
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["renderer"]["exact_plan_requested"] = "cpu_post_sort"
manifest["unavailable_fields"].append("renderer.exact_plan_actual")
manifest["image"] = {
    "path": "final-frame.png",
    "sha256": hashlib.sha256(png).hexdigest(),
    "width": width,
    "height": height,
}
manifest_path.write_text(json.dumps(manifest))
PY
python3 "$VALIDATOR" "$TMP_DIR/image-valid"

cp -R "$TMP_DIR/image-valid" "$TMP_DIR/unlisted-actual-plan"
python3 - "$TMP_DIR/unlisted-actual-plan/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["unavailable_fields"].remove("renderer.exact_plan_actual")
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/unlisted-actual-plan" >"$TMP_DIR/unlisted-actual-plan.out" 2>&1; then
  echo "expected unobservable actual plan without unavailable marker to fail" >&2
  exit 1
fi
grep -Fq 'renderer.exact_plan_actual must be listed as unavailable' "$TMP_DIR/unlisted-actual-plan.out"

cp -R "$TMP_DIR/image-valid" "$TMP_DIR/damaged-image"
python3 - "$TMP_DIR/damaged-image/final-frame.png" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
value = bytearray(path.read_bytes())
value[0] = 0
path.write_bytes(value)
PY
if python3 "$VALIDATOR" "$TMP_DIR/damaged-image" >"$TMP_DIR/damaged-image.out" 2>&1; then
  echo "expected damaged PNG to fail" >&2
  exit 1
fi
grep -Fq 'not a PNG with an IHDR header' "$TMP_DIR/damaged-image.out"

cp -R "$TMP_DIR/image-valid" "$TMP_DIR/replaced-image"
printf 'replacement' >>"$TMP_DIR/replaced-image/final-frame.png"
if python3 "$VALIDATOR" "$TMP_DIR/replaced-image" >"$TMP_DIR/replaced-image.out" 2>&1; then
  echo "expected replaced PNG to fail" >&2
  exit 1
fi
grep -Fq 'image SHA-256 mismatch' "$TMP_DIR/replaced-image.out"

cp -R "$TMP_DIR/image-valid" "$TMP_DIR/wrong-image-hash"
python3 - "$TMP_DIR/wrong-image-hash/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["image"]["sha256"] = "0" * 64
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/wrong-image-hash" >"$TMP_DIR/wrong-image-hash.out" 2>&1; then
  echo "expected wrong image hash to fail" >&2
  exit 1
fi
grep -Fq 'image SHA-256 mismatch' "$TMP_DIR/wrong-image-hash.out"

cp -R "$TMP_DIR/image-valid" "$TMP_DIR/wrong-image-size"
python3 - "$TMP_DIR/wrong-image-size/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["image"]["width"] = 639
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/wrong-image-size" >"$TMP_DIR/wrong-image-size.out" 2>&1; then
  echo "expected wrong image dimensions to fail" >&2
  exit 1
fi
grep -Fq 'image dimensions must equal display dimensions' "$TMP_DIR/wrong-image-size.out"

cp -R "$TMP_DIR/image-valid" "$TMP_DIR/wrong-png-size"
python3 - "$TMP_DIR/wrong-png-size" <<'PY'
import hashlib, json, pathlib, struct, sys, zlib

root = pathlib.Path(sys.argv[1])

def chunk(kind, payload):
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xffffffff)

raw = b"\x00\x00\x00\x00\xff"
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
(root / "final-frame.png").write_bytes(png)
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["image"]["sha256"] = hashlib.sha256(png).hexdigest()
manifest_path.write_text(json.dumps(manifest))
PY
if python3 "$VALIDATOR" "$TMP_DIR/wrong-png-size" >"$TMP_DIR/wrong-png-size.out" 2>&1; then
  echo "expected PNG with wrong actual dimensions to fail" >&2
  exit 1
fi
grep -Fq 'image PNG dimensions do not match its receipt' "$TMP_DIR/wrong-png-size.out"

cp -R "$VALID" "$TMP_DIR/bad-schema"
python3 - "$TMP_DIR/bad-schema/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["schema"] = "gsplat-benchmark/v2"
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/bad-schema" >"$TMP_DIR/bad-schema.out" 2>&1; then
  echo "expected bad schema to fail" >&2
  exit 1
fi
grep -Fq 'schema must equal' "$TMP_DIR/bad-schema.out"

cp -R "$VALID" "$TMP_DIR/bad-count"
python3 - "$TMP_DIR/bad-count/summary.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["sample_count"] = 4
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/bad-count" >"$TMP_DIR/bad-count.out" 2>&1; then
  echo "expected bad count to fail" >&2
  exit 1
fi
grep -Fq 'sample_count does not match' "$TMP_DIR/bad-count.out"

cp -R "$VALID" "$TMP_DIR/bad-percentile"
python3 - "$TMP_DIR/bad-percentile/summary.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["distributions"]["frame_wall_ms"]["p95"] = 4.0
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/bad-percentile" >"$TMP_DIR/bad-percentile.out" 2>&1; then
  echo "expected bad percentile to fail" >&2
  exit 1
fi
grep -Fq 'frame_wall_ms.p95 mismatch' "$TMP_DIR/bad-percentile.out"

cp -R "$VALID" "$TMP_DIR/bad-nonfinite"
sed -i.bak '1s/"call_ms":1.0/"call_ms":NaN/' "$TMP_DIR/bad-nonfinite/frames.jsonl"
rm -f "$TMP_DIR/bad-nonfinite/frames.jsonl.bak"
if python3 "$VALIDATOR" "$TMP_DIR/bad-nonfinite" >"$TMP_DIR/bad-nonfinite.out" 2>&1; then
  echo "expected non-finite sample to fail" >&2
  exit 1
fi
grep -Fq 'non-finite JSON number is forbidden' "$TMP_DIR/bad-nonfinite.out"

cp -R "$VALID" "$TMP_DIR/unavailable-build-state"
python3 - "$TMP_DIR/unavailable-build-state/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["build"]["repository_commit"] = None
value["build"]["dirty"] = None
value["unavailable_fields"].extend(["build.repository_commit", "build.dirty"])
path.write_text(json.dumps(value))
PY
python3 "$VALIDATOR" "$TMP_DIR/unavailable-build-state"

cp -R "$VALID" "$TMP_DIR/unavailable-phase-timings"
python3 - "$TMP_DIR/unavailable-phase-timings" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
metrics = ("preprocess_ms", "sort_ms", "geometry_submit_ms")
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["unavailable_fields"].extend(f"frames[*].{metric}" for metric in metrics)
manifest_path.write_text(json.dumps(manifest))
frames_path = root / "frames.jsonl"
frames = [json.loads(line) for line in frames_path.read_text().splitlines() if line]
for frame in frames:
    for metric in metrics:
        frame[metric] = None
frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
summary_path = root / "summary.json"
summary = json.loads(summary_path.read_text())
for metric in metrics:
    summary["distributions"][metric] = None
summary_path.write_text(json.dumps(summary))
PY
python3 "$VALIDATOR" "$TMP_DIR/unavailable-phase-timings"

cp -R "$VALID" "$TMP_DIR/unavailable-render-counts"
python3 - "$TMP_DIR/unavailable-render-counts" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["unavailable_fields"].extend(["frames[*].visible", "frames[*].drawn"])
manifest_path.write_text(json.dumps(manifest))
frames_path = root / "frames.jsonl"
frames = [json.loads(line) for line in frames_path.read_text().splitlines() if line]
for frame in frames:
    frame["active_splats"] = frame["visible"]
    frame["visible"] = None
    frame["drawn"] = None
frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
python3 "$VALIDATOR" "$TMP_DIR/unavailable-render-counts"

cp -R "$TMP_DIR/unavailable-render-counts" "$TMP_DIR/bound-control-counts"
python3 - "$TMP_DIR/bound-control-counts/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["renderer"]["count_semantics"] = "bound_current_stats_control_artifact"
path.write_text(json.dumps(value))
PY
python3 "$VALIDATOR" "$TMP_DIR/bound-control-counts"

cp -R "$TMP_DIR/bound-control-counts" "$TMP_DIR/terminal-queue-throughput-valid"
python3 - "$TMP_DIR/terminal-queue-throughput-valid" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["timing"] = {"performance_evidence": True}
manifest["ordering_window"] = {
    "terminal_current_stats_receipts": 2,
    "measured_submit_count": 5,
    "terminal_queue_done_count": 1,
    "warmup_queue_done_count": 1,
    "last_measured_ticket": 23,
    "queue_done_proven": True,
    "first_measured_input_monotonic_ms": 5,
    "first_measured_submit_monotonic_ms": 8,
    "last_measured_submit_monotonic_ms": 12,
    "last_measured_terminal_monotonic_ms": 15,
    "input_to_first_submit_ms": 3,
    "submit_span_ms": 4,
    "terminal_tail_ms": 3,
    "terminal_window_ms": 10,
}
manifest["benchmark_window"] = {
    "mode": "terminal_queue_throughput_window",
    "evidence_role": "cross_implementation_terminal_queue_throughput",
    "performance_evidence": True,
    "current_stats_policy": "one_untimed_warmup_boundary_and_one_final_measured_receipt",
    "terminal_policy": "drain_warmup_before_first_measured_input_and_stop_after_final_measured_submit",
    "configuration_sha256": "c" * 64,
    "control_artifact_identity": {
        "run_id": "control-fixture",
        "configuration_sha256": "c" * 64,
    },
    "warmup_submit_count": 2,
    "measured_submit_count": 5,
    "measured_wait_count_before_final_submit": 0,
    "warmup_terminal_receipt_submission_count": 1,
    "warmup_terminal_receipt_terminal_count": 1,
    "terminal_current_stats_submission_count": 1,
    "terminal_current_stats_terminal_count": 1,
    "warmup_boundary_current_stats_ticket": 11,
    "final_measured_current_stats_ticket": 23,
    "draw_count_at_warmup_drain_start": 2,
    "draw_count_at_warmup_drain_completion": 2,
    "draw_count_at_final_drain_start": 7,
    "draw_count_at_completion": 7,
    "first_measured_input_monotonic_ms": 5,
    "first_measured_submit_monotonic_ms": 8,
    "last_measured_submit_monotonic_ms": 12,
    "last_measured_terminal_monotonic_ms": 15,
    "input_to_first_submit_ms": 3,
    "submit_span_ms": 4,
    "terminal_tail_ms": 3,
    "terminal_window_ms": 10,
    "warmup_terminal_receipt": {
        "phase": "warmup_boundary",
        "ticket": 11,
        "status": "ready",
        "plan": "cpu_post_sort",
        "requested_at_monotonic_ms": 1,
        "submitted_at_monotonic_ms": 2,
        "terminal_at_monotonic_ms": 4,
    },
    "terminal_receipt": {
        "phase": "final_measured",
        "ticket": 23,
        "status": "ready",
        "plan": "cpu_post_sort",
        "requested_at_monotonic_ms": 11,
        "submitted_at_monotonic_ms": 12,
        "terminal_at_monotonic_ms": 15,
    },
    "terminal_receipt_overhead": {
        "kind": "renderer_current_stats_same_submission_map_v1",
        "readback_buffer_bytes": 8,
        "encoded_copy_bytes": 4,
        "extra_queue_submissions": 0,
        "map_async_result_required": True,
        "included_in_terminal_window": True,
        "warmup_terminal_boundary": "same_submission_result_ready_before_first_measured_input",
        "residual_warmup_queue_tail": "excluded",
    },
    "exact_adaptive_measured": [{
        "state": "cpu_learning",
        "plan": "cpu_post_sort",
        "projected_state": "disabled",
        "projected_execution": "candidate",
    } for _ in range(5)],
}
manifest_path.write_text(json.dumps(manifest))

identity_fields = (
    "current_stats_ticket",
    "current_stats_plan",
    "current_stats_scene_generation",
    "current_stats_camera_revision",
    "current_stats_viewport_generation",
    "current_stats_contract_generation",
    "current_stats_plan_set_generation",
    "current_stats_order_generation",
    "current_stats_raster_generation",
    "current_stats_encode_attempt",
    "current_stats_presentation_sequence",
)
frames_path = root / "frames.jsonl"
frames = [json.loads(line) for line in frames_path.read_text().splitlines() if line]
for frame in frames:
    frame["current_stats_submission"] = "not_requested"
    for field in identity_fields:
        frame[field] = None
frames[-1].update({
    "current_stats_submission": "issued",
    "current_stats_ticket": 23,
    "current_stats_plan": "cpu_post_sort",
    "current_stats_scene_generation": 1,
    "current_stats_camera_revision": 5,
    "current_stats_viewport_generation": 1,
    "current_stats_contract_generation": 1,
    "current_stats_plan_set_generation": 1,
    "current_stats_order_generation": 5,
    "current_stats_raster_generation": 5,
    "current_stats_encode_attempt": 7,
    "current_stats_presentation_sequence": 7,
})
frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-throughput-valid"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-fixed-compact-valid"
python3 - "$TMP_DIR/terminal-queue-fixed-compact-valid" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
path = root / "manifest.json"
value = json.loads(path.read_text())
window = value["benchmark_window"]
window["execution_cell"] = "fixed_gpu_preproject_compact"
window["warmup_terminal_receipt"]["plan"] = "gpu_preproject"
window["terminal_receipt"]["plan"] = "gpu_preproject"
window["exact_adaptive_measured"] = [{
    "state": "disabled",
    "plan": "gpu_preproject",
    "projected_state": "disabled",
    "projected_execution": "compact",
} for _ in window["exact_adaptive_measured"]]
value["renderer"].update({
    "order_backend_requested": "gpu",
    "projected_policy_requested": "compact",
    "gpu_order_producer_requested": None,
    "gpu_order_producer_actual": "preproject",
})
path.write_text(json.dumps(value))

frames_path = root / "frames.jsonl"
frames = [json.loads(line) for line in frames_path.read_text().splitlines() if line]
for frame in frames:
    frame.update({
        "order_backend_requested": "gpu",
        "order_backend": "gpu",
        "gpu_sort_fallback": False,
        "adaptive_state": "disabled",
        "projected_policy": "compact",
        "projected_execution": "compact",
        "projected_adaptive_state": "disabled",
        "gpu_order_producer": "preproject",
    })
frames[-1]["current_stats_plan"] = "gpu_preproject"
frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-fixed-compact-valid"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-fixed-label-only"
python3 - "$TMP_DIR/terminal-queue-fixed-label-only/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
window = value["benchmark_window"]
window["execution_cell"] = "fixed_gpu_preproject_compact"
window["exact_adaptive_measured"] = [{
    "state": "disabled",
    "plan": "gpu_preproject",
    "projected_state": "disabled",
    "projected_execution": "compact",
} for _ in window["exact_adaptive_measured"]]
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-fixed-label-only" >"$TMP_DIR/terminal-queue-fixed-label-only.out" 2>&1; then
  echo "expected fixed label over CPU renderer identity to fail" >&2
  exit 1
fi
grep -Fq 'renderer identity mismatch' "$TMP_DIR/terminal-queue-fixed-label-only.out"

cp -R "$TMP_DIR/terminal-queue-fixed-compact-valid" "$TMP_DIR/terminal-queue-fixed-compact-adaptive"
python3 - "$TMP_DIR/terminal-queue-fixed-compact-adaptive/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["benchmark_window"]["exact_adaptive_measured"][0]["state"] = "gpu_stable"
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-fixed-compact-adaptive" >"$TMP_DIR/terminal-queue-fixed-compact-adaptive.out" 2>&1; then
  echo "expected fixed Compact cell with active Adaptive state to fail" >&2
  exit 1
fi
grep -Fq 'Exact adaptive record 0 is invalid' "$TMP_DIR/terminal-queue-fixed-compact-adaptive.out"

for mutation in gpu-post-sort candidate active-projected fallback; do
  target="$TMP_DIR/terminal-queue-fixed-compact-$mutation"
  cp -R "$TMP_DIR/terminal-queue-fixed-compact-valid" "$target"
  python3 - "$target/frames.jsonl" "$mutation" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
mutation = sys.argv[2]
frames = [json.loads(line) for line in path.read_text().splitlines() if line]
if mutation == "gpu-post-sort":
    frames[0]["gpu_order_producer"] = "post_sort"
elif mutation == "candidate":
    frames[0]["projected_execution"] = "candidate"
elif mutation == "active-projected":
    frames[0]["projected_adaptive_state"] = "compact_stable"
elif mutation == "fallback":
    frames[0]["gpu_sort_fallback"] = True
path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
  if python3 "$VALIDATOR" "$target" >"$target.out" 2>&1; then
    echo "expected fixed Compact $mutation identity to fail" >&2
    exit 1
  fi
  grep -Fq 'frame 0 identity mismatch' "$target.out"
done

cp -R "$TMP_DIR/terminal-queue-fixed-compact-valid" "$TMP_DIR/terminal-queue-fixed-wrong-final-plan"
python3 - "$TMP_DIR/terminal-queue-fixed-wrong-final-plan/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["benchmark_window"]["terminal_receipt"]["plan"] = "gpu_post_sort"
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-fixed-wrong-final-plan" >"$TMP_DIR/terminal-queue-fixed-wrong-final-plan.out" 2>&1; then
  echo "expected fixed Compact final plan drift to fail" >&2
  exit 1
fi
grep -Fq 'final receipt plan mismatch' "$TMP_DIR/terminal-queue-fixed-wrong-final-plan.out"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-adaptive-disabled"
python3 - "$TMP_DIR/terminal-queue-adaptive-disabled/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["benchmark_window"]["exact_adaptive_measured"][0]["state"] = "disabled"
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-adaptive-disabled" >"$TMP_DIR/terminal-queue-adaptive-disabled.out" 2>&1; then
  echo "expected Adaptive cell with disabled state to fail" >&2
  exit 1
fi
grep -Fq 'Exact adaptive record 0 is invalid' "$TMP_DIR/terminal-queue-adaptive-disabled.out"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-unknown-cell"
python3 - "$TMP_DIR/terminal-queue-unknown-cell/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["benchmark_window"]["execution_cell"] = "unknown"
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-unknown-cell" >"$TMP_DIR/terminal-queue-unknown-cell.out" 2>&1; then
  echo "expected unknown throughput execution cell to fail" >&2
  exit 1
fi
grep -Fq 'execution_cell is unsupported' "$TMP_DIR/terminal-queue-unknown-cell.out"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-warmup-tail"
python3 - "$TMP_DIR/terminal-queue-warmup-tail/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["benchmark_window"]["warmup_terminal_receipt"]["terminal_at_monotonic_ms"] = 6
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-warmup-tail" >"$TMP_DIR/terminal-queue-warmup-tail.out" 2>&1; then
  echo "expected residual warmup queue tail to fail" >&2
  exit 1
fi
grep -Fq 'did not drain warmup before measured input' "$TMP_DIR/terminal-queue-warmup-tail.out"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-omits-first-input"
python3 - "$TMP_DIR/terminal-queue-omits-first-input/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["benchmark_window"]["terminal_window_ms"] = 7
value["ordering_window"]["terminal_window_ms"] = 7
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-omits-first-input" >"$TMP_DIR/terminal-queue-omits-first-input.out" 2>&1; then
  echo "expected terminal window that omits first-frame CPU/encode to fail" >&2
  exit 1
fi
grep -Fq 'benchmark_window.terminal_window_ms mismatch' "$TMP_DIR/terminal-queue-omits-first-input.out"

cp -R "$TMP_DIR/terminal-queue-throughput-valid" "$TMP_DIR/terminal-queue-early-observer"
python3 - "$TMP_DIR/terminal-queue-early-observer/frames.jsonl" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
frames = [json.loads(line) for line in path.read_text().splitlines() if line]
frames[0]["current_stats_submission"] = "issued"
frames[0]["current_stats_ticket"] = 9
path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
if python3 "$VALIDATOR" "$TMP_DIR/terminal-queue-early-observer" >"$TMP_DIR/terminal-queue-early-observer.out" 2>&1; then
  echo "expected a pre-final measured observer to fail" >&2
  exit 1
fi
grep -Fq 'frame 0 requested current stats' "$TMP_DIR/terminal-queue-early-observer.out"

cp -R "$TMP_DIR/unavailable-render-counts" "$TMP_DIR/unlisted-render-counts"
python3 - "$TMP_DIR/unlisted-render-counts/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["unavailable_fields"].remove("frames[*].drawn")
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/unlisted-render-counts" >"$TMP_DIR/unlisted-render-counts.out" 2>&1; then
  echo "expected unlisted null render count to fail" >&2
  exit 1
fi
grep -Fq 'null drawn must be listed as unavailable' "$TMP_DIR/unlisted-render-counts.out"

cp -R "$VALID" "$TMP_DIR/exact-contributor-valid"
python3 - "$TMP_DIR/exact-contributor-valid" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["renderer"]["count_semantics"] = "candidate_visible_contributor_issued_v1"
manifest_path.write_text(json.dumps(manifest))
frames_path = root / "frames.jsonl"
frames = [json.loads(line) for line in frames_path.read_text().splitlines() if line]
for frame in frames:
    frame.update({
        "visible": 2,
        "contributor": 1,
        "drawn": 1,
        "exact_contributor_compaction": True,
    })
frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
python3 "$VALIDATOR" "$TMP_DIR/exact-contributor-valid"

cp -R "$TMP_DIR/exact-contributor-valid" "$TMP_DIR/exact-contributor-missing-flag"
python3 - "$TMP_DIR/exact-contributor-missing-flag/frames.jsonl" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
frames = [json.loads(line) for line in path.read_text().splitlines() if line]
frames[0].pop("exact_contributor_compaction")
path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
if python3 "$VALIDATOR" "$TMP_DIR/exact-contributor-missing-flag" >"$TMP_DIR/exact-contributor-missing-flag.out" 2>&1; then
  echo "expected exact contributor receipt missing its flag to fail" >&2
  exit 1
fi
grep -Fq 'must be emitted together' "$TMP_DIR/exact-contributor-missing-flag.out"

cp -R "$TMP_DIR/exact-contributor-valid" "$TMP_DIR/exact-contributor-bad-draw"
python3 - "$TMP_DIR/exact-contributor-bad-draw/frames.jsonl" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
frames = [json.loads(line) for line in path.read_text().splitlines() if line]
frames[0]["drawn"] = 2
path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
if python3 "$VALIDATOR" "$TMP_DIR/exact-contributor-bad-draw" >"$TMP_DIR/exact-contributor-bad-draw.out" 2>&1; then
  echo "expected exact contributor receipt with D != C to fail" >&2
  exit 1
fi
grep -Fq 'drawn == contributor' "$TMP_DIR/exact-contributor-bad-draw.out"

cp -R "$VALID" "$TMP_DIR/legacy-budgeted-draw"
python3 - "$TMP_DIR/legacy-budgeted-draw/frames.jsonl" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
frames = [json.loads(line) for line in path.read_text().splitlines() if line]
frames[0]["drawn"] = 1
path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
if python3 "$VALIDATOR" "$TMP_DIR/legacy-budgeted-draw" >"$TMP_DIR/legacy-budgeted-draw.out" 2>&1; then
  echo "expected legacy D < V to fail" >&2
  exit 1
fi
grep -Fq 'legacy frame requires drawn == visible' "$TMP_DIR/legacy-budgeted-draw.out"

cp -R "$TMP_DIR/unavailable-phase-timings" "$TMP_DIR/unlisted-phase-timing"
python3 - "$TMP_DIR/unlisted-phase-timing/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["unavailable_fields"].remove("frames[*].sort_ms")
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/unlisted-phase-timing" >"$TMP_DIR/unlisted-phase-timing.out" 2>&1; then
  echo "expected unlisted null phase timing to fail" >&2
  exit 1
fi
grep -Fq 'null sort_ms must be listed as unavailable' "$TMP_DIR/unlisted-phase-timing.out"

cp -R "$TMP_DIR/unavailable-build-state" "$TMP_DIR/unlisted-build-state"
python3 - "$TMP_DIR/unlisted-build-state/manifest.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
value = json.loads(path.read_text())
value["unavailable_fields"].remove("build.dirty")
path.write_text(json.dumps(value))
PY
if python3 "$VALIDATOR" "$TMP_DIR/unlisted-build-state" >"$TMP_DIR/unlisted-build-state.out" 2>&1; then
  echo "expected unlisted null build state to fail" >&2
  exit 1
fi
grep -Fq 'null build.dirty must be listed as unavailable' "$TMP_DIR/unlisted-build-state.out"

cp -R "$VALID" "$TMP_DIR/async-valid"
python3 - "$TMP_DIR/async-valid" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
manifest_path = root / "manifest.json"
manifest = json.loads(manifest_path.read_text())
manifest["renderer"]["sort_policy"] = "async_latest:2"
manifest["unavailable_fields"] = [
    field for field in manifest.get("unavailable_fields", [])
    if field != "frames[*].sort_refreshed"
]
manifest_path.write_text(json.dumps(manifest))

frames_path = root / "frames.jsonl"
frames = [json.loads(line) for line in frames_path.read_text().splitlines() if line]
for index, frame in enumerate(frames):
    frame.update({
        "sort_refreshed": True,
        "camera_revision": index,
        "applied_order_revision": index,
        "presented_order_revision_lag": 0,
        "async_sort_scheduled_revision": None,
        "async_sort_completed_revision": None,
        "async_sort_observed_result_lag": None,
        "async_sort_scheduled": False,
        "async_sort_result_applied": False,
        "stale_async_sort_dropped": False,
        "sync_sort_fallback": False,
    })
frames_path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
summary_path = root / "summary.json"
summary = json.loads(summary_path.read_text())
summary["sort_telemetry"] = {
    "scheduled_count": 0,
    "completed_count": 0,
    "applied_count": 0,
    "dropped_count": 0,
    "sync_fallback_count": 0,
    "max_presented_revision_lag": 0,
    "stale_applied_count": 0,
}
summary_path.write_text(json.dumps(summary))
PY
python3 "$VALIDATOR" "$TMP_DIR/async-valid"

cp -R "$TMP_DIR/async-valid" "$TMP_DIR/async-bad-lag"
python3 - "$TMP_DIR/async-bad-lag/frames.jsonl" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
frames = [json.loads(line) for line in path.read_text().splitlines() if line]
frames[-1]["applied_order_revision"] = frames[-1]["camera_revision"] - 3
frames[-1]["presented_order_revision_lag"] = 3
path.write_text("\n".join(json.dumps(frame) for frame in frames) + "\n")
PY
if python3 "$VALIDATOR" "$TMP_DIR/async-bad-lag" >"$TMP_DIR/async-bad-lag.out" 2>&1; then
  echo "expected over-limit async order lag to fail" >&2
  exit 1
fi
grep -Fq 'presented async order lag exceeds 2' "$TMP_DIR/async-bad-lag.out"

python3 tests/perf/test_full_quality_experiment.py
python3 tests/perf/validate-full-quality-experiment.py \
  tests/perf/full-quality-matrix-plan-v1.json --allow-incomplete

echo "benchmark artifact fixture tests passed"
