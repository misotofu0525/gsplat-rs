#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT_DIR"

TRACE_DIR="tests/perf/trace"
FIXTURE="$TRACE_DIR/fixtures/camera-trace-v1.json"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 "$TRACE_DIR/validate_trace_v1.py" "$FIXTURE"
for quality_fixture in "$TRACE_DIR"/fixtures/quality/*.json; do
  python3 "$TRACE_DIR/validate_trace_v1.py" "$quality_fixture"
done
python3 "$TRACE_DIR/generate_trace_v1.py" --output "$TMP_DIR/generated.json" >/dev/null
python3 - "$FIXTURE" "$TMP_DIR/generated.json" <<'PY'
import json, pathlib, sys
committed = json.loads(pathlib.Path(sys.argv[1]).read_text())
generated = json.loads(pathlib.Path(sys.argv[2]).read_text())
if committed != generated:
    raise SystemExit("generated trace does not match committed fixture")
PY
python3 "$TRACE_DIR/generate_trace_v1.py" \
  --width 2412 --height 1080 \
  --output "$TMP_DIR/generated-native.json" >/dev/null
python3 "$TRACE_DIR/validate_trace_v1.py" "$TMP_DIR/generated-native.json"
python3 - "$TMP_DIR/generated-native.json" <<'PY'
import json, pathlib, sys
trace = json.loads(pathlib.Path(sys.argv[1]).read_text())
if trace["display"] != {"width": 2412, "height": 1080}:
    raise SystemExit("native-size trace display drifted")
if trace["trace_id"] != "contract-lateral-three-frame-2412x1080-v1":
    raise SystemExit("native-size trace identity drifted")
PY

python3 "$TRACE_DIR/generate_scene_quality_traces.py" \
  --scene minimal=tests/datasets/minimal_binary.ply \
  --output-dir "$TMP_DIR/candidates" >/dev/null
python3 "$TRACE_DIR/generate_scene_quality_traces.py" \
  --scene minimal=tests/datasets/minimal_binary.ply \
  --output-dir "$TMP_DIR/candidates-repeat" >/dev/null
python3 "$TRACE_DIR/validate_trace_v1.py" \
  "$TMP_DIR/candidates/candidate-minimal-quality-640x360-v1.json"
cmp \
  "$TMP_DIR/candidates/candidate-minimal-quality-640x360-v1.json" \
  "$TMP_DIR/candidates-repeat/candidate-minimal-quality-640x360-v1.json"

python3 "$TRACE_DIR/generate_scene_quality_traces.py" \
  --derive-from "$TMP_DIR/candidates/candidate-minimal-quality-640x360-v1.json" \
  --width 2412 --height 1080 \
  --output-dir "$TMP_DIR/native-candidates" >/dev/null
python3 "$TRACE_DIR/validate_trace_v1.py" \
  "$TMP_DIR/native-candidates/candidate-minimal-quality-2412x1080-v1.json"
python3 - \
  "$TMP_DIR/candidates/candidate-minimal-quality-640x360-v1.json" \
  "$TMP_DIR/native-candidates/candidate-minimal-quality-2412x1080-v1.json" <<'PY'
import json, pathlib, sys
source = json.loads(pathlib.Path(sys.argv[1]).read_text())
variant = json.loads(pathlib.Path(sys.argv[2]).read_text())
if variant["display"] != {"width": 2412, "height": 1080}:
    raise SystemExit("scene native variant display drifted")
if variant["trace_id"] != "candidate-minimal-quality-2view-2412x1080-v1":
    raise SystemExit("scene native variant identity drifted")
for source_frame, variant_frame in zip(source["frames"], variant["frames"], strict=True):
    for field in ("pose", "intrinsics", "view_matrix"):
        if source_frame[field] != variant_frame[field]:
            raise SystemExit(f"scene native variant changed {field}")
    if source_frame["projection_matrix"] == variant_frame["projection_matrix"]:
        raise SystemExit("scene native variant did not update projection aspect")
if variant["derivation"]["display_variant"]["source_trace_sha256"] != source["content_sha256"]:
    raise SystemExit("scene native variant source hash receipt drifted")
PY

python3 - "$TMP_DIR/official-cameras.json" <<'PY'
import json, pathlib, sys
camera = {
    "id": 0,
    "img_name": "fixture",
    "width": 640,
    "height": 360,
    "position": [0.0, 1.0, -3.0],
    "rotation": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    "fx": 500.0,
    "fy": 500.0,
}
pathlib.Path(sys.argv[1]).write_text(json.dumps([camera, {**camera, "id": 1}]))
PY
python3 "$TRACE_DIR/generate_scene_quality_traces.py" \
  --scene minimal=tests/datasets/minimal_binary.ply \
  --camera-metadata "minimal=$TMP_DIR/official-cameras.json" \
  --camera-indices minimal=0,1 \
  --output-dir "$TMP_DIR/official-candidates" >/dev/null
python3 "$TRACE_DIR/validate_trace_v1.py" \
  "$TMP_DIR/official-candidates/candidate-minimal-quality-640x360-v1.json"
python3 - "$TMP_DIR/official-candidates/candidate-minimal-quality-640x360-v1.json" <<'PY'
import json, pathlib, sys
trace = json.loads(pathlib.Path(sys.argv[1]).read_text())
if trace["frames"][0]["pose"]["position"] != [0.0, -1.0, -3.0]:
    raise SystemExit("official RDF-to-RUF camera position conversion drifted")
if trace["frames"][0]["pose"]["rotation_xyzw"] != [0.0, 0.0, 0.0, 1.0]:
    raise SystemExit("official RDF-to-RUF camera rotation conversion drifted")
PY

python3 - "$FIXTURE" "$TMP_DIR/bad-hash.json" <<'PY'
import json, pathlib, sys
value = json.loads(pathlib.Path(sys.argv[1]).read_text())
value["frames"][0]["timestamp_ns"] = 1
pathlib.Path(sys.argv[2]).write_text(json.dumps(value))
PY
if python3 "$TRACE_DIR/validate_trace_v1.py" "$TMP_DIR/bad-hash.json" >"$TMP_DIR/bad-hash.out" 2>&1; then
  echo "expected bad content hash to fail" >&2
  exit 1
fi
grep -q 'content_sha256 mismatch' "$TMP_DIR/bad-hash.out"

python3 - "$FIXTURE" "$TMP_DIR/bad-matrix.json" <<'PY'
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(sys.argv[1]).resolve().parent.parent))
from trace_v1 import with_content_hash
value = json.loads(pathlib.Path(sys.argv[1]).read_text())
value["frames"][1]["view_matrix"][3] += 0.25
pathlib.Path(sys.argv[2]).write_text(json.dumps(with_content_hash(value)))
PY
if python3 "$TRACE_DIR/validate_trace_v1.py" "$TMP_DIR/bad-matrix.json" >"$TMP_DIR/bad-matrix.out" 2>&1; then
  echo "expected bad matrix to fail" >&2
  exit 1
fi
grep -q 'view_matrix\[3\] mismatch' "$TMP_DIR/bad-matrix.out"

echo "camera trace v1 tests passed"
