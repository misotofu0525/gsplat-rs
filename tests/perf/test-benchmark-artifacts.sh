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
rg -q 'schema must equal' "$TMP_DIR/bad-schema.out"

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
rg -q 'sample_count does not match' "$TMP_DIR/bad-count.out"

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
rg -q 'frame_wall_ms.p95 mismatch' "$TMP_DIR/bad-percentile.out"

cp -R "$VALID" "$TMP_DIR/bad-nonfinite"
sed -i.bak '1s/"call_ms":1.0/"call_ms":NaN/' "$TMP_DIR/bad-nonfinite/frames.jsonl"
rm -f "$TMP_DIR/bad-nonfinite/frames.jsonl.bak"
if python3 "$VALIDATOR" "$TMP_DIR/bad-nonfinite" >"$TMP_DIR/bad-nonfinite.out" 2>&1; then
  echo "expected non-finite sample to fail" >&2
  exit 1
fi
rg -q 'non-finite JSON number is forbidden' "$TMP_DIR/bad-nonfinite.out"

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
rg -q 'null build.dirty must be listed as unavailable' "$TMP_DIR/unlisted-build-state.out"

echo "benchmark artifact fixture tests passed"
