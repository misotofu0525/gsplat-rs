#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

VALIDATOR="tests/perf/validate-benchmark-artifacts.py"
VALID="tests/perf/fixtures/v1/valid"

python3 "$VALIDATOR" "$VALID"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

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

echo "benchmark artifact fixture tests passed"
