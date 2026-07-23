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
