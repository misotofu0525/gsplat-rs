#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 - "$TMP_DIR/console.log" <<'PY'
import base64
import json
import pathlib
import sys

fixtures = pathlib.Path("tests/perf/fixtures/v1/valid")
output = []
for kind, name in (("manifest", "manifest.json"), ("summary", "summary.json")):
    payload = (fixtures / name).read_bytes()
    if kind == "manifest":
        manifest = json.loads(payload)
        manifest["renderer"]["projected_evidence_version"] = 0
        payload = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
    output.append(f"console BENCHMARK_ARTIFACT {kind} {base64.b64encode(payload).decode()}")
for payload in (fixtures / "frames.jsonl").read_bytes().splitlines():
    output.append(f"console BENCHMARK_ARTIFACT frame {base64.b64encode(payload).decode()}")
pathlib.Path(sys.argv[1]).write_text("\n".join(output) + "\n", encoding="utf-8")
PY

python3 bindings/apple/scripts/extract-ios-benchmark-artifacts.py \
  "$TMP_DIR/console.log" \
  "$TMP_DIR/artifact" \
  --validator tests/perf/validate-benchmark-artifacts.py

if python3 bindings/apple/scripts/extract-ios-benchmark-artifacts.py \
  "$TMP_DIR/console.log" \
  "$TMP_DIR/artifact" \
  --validator tests/perf/validate-benchmark-artifacts.py \
  >"$TMP_DIR/reuse.stdout" 2>"$TMP_DIR/reuse.stderr"; then
  echo "extractor unexpectedly reused an existing destination" >&2
  exit 1
fi
grep -Fq 'destination already exists' "$TMP_DIR/reuse.stderr"

python3 - "$TMP_DIR" <<'PY'
import base64
import copy
import json
import pathlib
import sys

fixtures = pathlib.Path("tests/perf/fixtures/v1/valid")
manifest = json.loads((fixtures / "manifest.json").read_text())
summary = json.loads((fixtures / "summary.json").read_text())
frames = [json.loads(line) for line in (fixtures / "frames.jsonl").read_text().splitlines()]
ticket = 1 << 52

manifest["renderer"]["count_semantics"] = "candidate_visible_contributor_issued_v1"
manifest["renderer"]["projected_evidence_version"] = 1
manifest["renderer"]["projected_policy_requested"] = "adaptive"
manifest["exactness"] = {"receipt_id": "fixture"}
manifest["unavailable_fields"].extend([
    "frames[*].cpu_frame_complete_ms",
    "summary.distributions.cpu_frame_complete_ms",
])
for index, frame in enumerate(frames):
    frame.update({
        "camera_revision": 10 + index,
        "order_backend": "cpu",
        "cpu_frame_complete_ms": None,
        "contributor": frame["visible"],
        "exact_contributor_compaction": False,
        "projected_policy": "adaptive",
        "projected_execution": "candidate",
        "projected_adaptive_state": "candidate_learning",
        "projected_measurement_submission": "issued" if index == 0 else "not_requested",
        "projected_measurement_ticket": ticket if index == 0 else None,
        "projected_measurement_execution": "candidate" if index == 0 else None,
        "projected_measurement_unsampled_reason": None,
        "projected_submission_flags": 1 if index == 0 else 0,
        "order_submission_ticket": None,
    })
summary["distributions"]["cpu_frame_complete_ms"] = None
summary["projected_draw_telemetry"] = {
    "policy_requested": "adaptive",
    "candidate_frame_count": len(frames),
    "compact_frame_count": 0,
    "measurement_scheduled_count": 1,
    "measurement_completed_count": 1,
    "measurement_terminal_failure_count": 0,
    "measurement_unsampled_count": 0,
    "adaptive_final_state": "candidate_learning",
    "execution_final": "candidate",
}
summary["projected_terminal_ledger"] = {
    "submissions": [{
        "ticket": ticket,
        "camera_revision": 10,
        "execution": "candidate",
        "order_backend": "cpu",
    }],
    "successes": [{
        "ticket": ticket,
        "camera_revision": 10,
        "projection_generation": 4,
        "probe_generation": 2,
        "execution": "candidate",
        "order_backend": "cpu",
        "outcome": "success",
        "frame_complete_ms": 2.5,
        "projection_rebuilt": True,
        "order_refreshed": False,
        "visible": 2,
        "contributor": 2,
        "drawn": 2,
        "exact_contributor_compaction": False,
        "exactness_receipt_id": "fixture",
        "measurement_flags": 1,
        "counts_flags": 0,
    }],
    "failures": [],
    "issued_count": 1,
    "success_count": 1,
    "failure_count": 0,
}

def write_log(path, manifest_record, frame_records, summary_record):
    records = [("manifest", manifest_record)]
    records.extend(("frame", frame) for frame in frame_records)
    records.append(("summary", summary_record))
    lines = []
    for kind, record in records:
        payload = json.dumps(record, sort_keys=True, separators=(",", ":")).encode()
        lines.append(f"console BENCHMARK_ARTIFACT {kind} {base64.b64encode(payload).decode()}")
    pathlib.Path(path).write_text("\n".join(lines) + "\n")

tmp = pathlib.Path(sys.argv[1])
write_log(tmp / "projected-valid.log", manifest, frames, summary)

incomplete = copy.deepcopy(summary)
incomplete["projected_terminal_ledger"]["successes"] = []
incomplete["projected_terminal_ledger"]["success_count"] = 0
incomplete["projected_draw_telemetry"]["measurement_completed_count"] = 0
write_log(tmp / "projected-incomplete.log", manifest, frames, incomplete)

missing_policy = copy.deepcopy(manifest)
del missing_policy["renderer"]["projected_policy_requested"]
write_log(tmp / "projected-missing-policy.log", missing_policy, frames, summary)

missing_version = copy.deepcopy(manifest)
del missing_version["renderer"]["projected_evidence_version"]
write_log(tmp / "projected-missing-version.log", missing_version, frames, summary)

revision_mismatch = copy.deepcopy(summary)
revision_mismatch["projected_terminal_ledger"]["submissions"][0]["camera_revision"] = 999
revision_mismatch["projected_terminal_ledger"]["successes"][0]["camera_revision"] = 999
write_log(tmp / "projected-revision-mismatch.log", manifest, frames, revision_mismatch)

backend_mismatch = copy.deepcopy(summary)
backend_mismatch["projected_terminal_ledger"]["submissions"][0]["order_backend"] = "gpu"
backend_mismatch["projected_terminal_ledger"]["successes"][0]["order_backend"] = "gpu"
write_log(tmp / "projected-backend-mismatch.log", manifest, frames, backend_mismatch)

execution_mismatch = copy.deepcopy(summary)
execution_mismatch["projected_terminal_ledger"]["submissions"][0]["execution"] = "compact"
execution_success = execution_mismatch["projected_terminal_ledger"]["successes"][0]
execution_success.update({
    "execution": "compact",
    "exact_contributor_compaction": True,
    "measurement_flags": 5,
    "counts_flags": 1,
})
write_log(tmp / "projected-execution-mismatch.log", manifest, frames, execution_mismatch)

counts_mismatch = copy.deepcopy(summary)
success = counts_mismatch["projected_terminal_ledger"]["successes"][0]
success["contributor"] = 1
write_log(tmp / "projected-counts-mismatch.log", manifest, frames, counts_mismatch)

counts_flags_mismatch = copy.deepcopy(summary)
counts_flags_mismatch["projected_terminal_ledger"]["successes"][0]["counts_flags"] = 1
write_log(tmp / "projected-counts-flags-mismatch.log", manifest, frames, counts_flags_mismatch)

timing_frames = copy.deepcopy(frames)
timing_frames[0]["cpu_frame_complete_ms"] = 2.5
write_log(tmp / "projected-timing-mismatch.log", manifest, timing_frames, summary)
PY

python3 bindings/apple/scripts/extract-ios-benchmark-artifacts.py \
  "$TMP_DIR/projected-valid.log" \
  "$TMP_DIR/projected-artifact" \
  --validator tests/perf/validate-benchmark-artifacts.py

python3 - "$TMP_DIR/projected-artifact" <<'PY'
import json
import pathlib
import sys

artifact = pathlib.Path(sys.argv[1])
manifest = json.loads((artifact / "manifest.json").read_text())
summary = json.loads((artifact / "summary.json").read_text())
assert manifest["renderer"]["projected_policy_requested"] == "adaptive"
assert summary["projected_terminal_ledger"]["issued_count"] == 1
assert summary["projected_terminal_ledger"]["success_count"] == 1
PY

expect_rejected() {
  local name="$1"
  local expected="$2"
  if python3 bindings/apple/scripts/extract-ios-benchmark-artifacts.py \
    "$TMP_DIR/$name.log" \
    "$TMP_DIR/$name-artifact" \
    --validator tests/perf/validate-benchmark-artifacts.py \
    >"$TMP_DIR/$name.stdout" 2>"$TMP_DIR/$name.stderr"; then
    echo "extractor unexpectedly accepted $name" >&2
    exit 1
  fi
  test ! -e "$TMP_DIR/$name-artifact"
  grep -Fq "$expected" "$TMP_DIR/$name.stderr"
}

expect_rejected projected-incomplete 'projected terminal ledger is missing tickets'
expect_rejected projected-missing-policy 'projected_policy_requested is required'
expect_rejected projected-missing-version 'projected_evidence_version must be the integer 0 or 1'
expect_rejected projected-revision-mismatch 'changed camera revision, execution, or order lane'
expect_rejected projected-backend-mismatch 'changed camera revision, execution, or order lane'
expect_rejected projected-execution-mismatch 'changed camera revision, execution, or order lane'
expect_rejected projected-counts-mismatch 'changed its terminal V/C/D identity'
expect_rejected projected-counts-flags-mismatch 'counts_flags disagree with exact compaction'
expect_rejected projected-timing-mismatch 'available CPU completion timing is declared unavailable'
echo "iOS benchmark artifact extraction tests passed"
