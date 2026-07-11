#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 - "$TMP_DIR/console.log" <<'PY'
import base64
import pathlib
import sys

fixtures = pathlib.Path("tests/perf/fixtures/v1/valid")
output = []
for kind, name in (("manifest", "manifest.json"), ("summary", "summary.json")):
    payload = (fixtures / name).read_bytes()
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
rg -q 'destination already exists' "$TMP_DIR/reuse.stderr"
echo "iOS benchmark artifact extraction tests passed"
