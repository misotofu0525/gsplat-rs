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
