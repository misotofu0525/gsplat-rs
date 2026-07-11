#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT_DIR"

TRACE_DIR="tests/perf/trace"
FIXTURE="$TRACE_DIR/fixtures/camera-trace-v1.json"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 "$TRACE_DIR/validate_trace_v1.py" "$FIXTURE"
python3 "$TRACE_DIR/generate_trace_v1.py" --output "$TMP_DIR/generated.json" >/dev/null
python3 - "$FIXTURE" "$TMP_DIR/generated.json" <<'PY'
import json, pathlib, sys
committed = json.loads(pathlib.Path(sys.argv[1]).read_text())
generated = json.loads(pathlib.Path(sys.argv[2]).read_text())
if committed != generated:
    raise SystemExit("generated trace does not match committed fixture")
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
