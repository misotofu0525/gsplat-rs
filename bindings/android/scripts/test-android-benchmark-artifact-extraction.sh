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

echo "Android benchmark artifact extraction tests passed"
