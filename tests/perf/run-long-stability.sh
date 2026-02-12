#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

DATASET_PATH="${1:-tests/datasets/minimal_ascii.ply}"
STABILITY_SECONDS="${STABILITY_SECONDS:-1800}"
RSS_GROWTH_LIMIT_KIB="${RSS_GROWTH_LIMIT_KIB:-65536}"

cargo run -p bench-runner -- \
  "$DATASET_PATH" \
  --stability-seconds "$STABILITY_SECONDS" \
  --rss-growth-limit-kib "$RSS_GROWTH_LIMIT_KIB"
