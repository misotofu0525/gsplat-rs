#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS simulator smoke is only supported on macOS" >&2
  exit 1
fi

DEFAULT_DATASET="tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply"
DATASET_PATH="${1:-$DEFAULT_DATASET}"

case "$DATASET_PATH" in
  /*) DATASET_ABS="$DATASET_PATH" ;;
  *) DATASET_ABS="$ROOT_DIR/$DATASET_PATH" ;;
esac

if [[ ! -f "$DATASET_ABS" ]]; then
  echo "missing dataset: $DATASET_PATH" >&2
  echo "fetch the shared flower dataset first:" >&2
  echo "  bash tests/datasets/fetch-nvidia-flowers-1.sh" >&2
  exit 1
fi

find_booted_simulator() {
  xcrun simctl list devices booted \
    | sed -n '/iPhone/s/.*(\([0-9A-Fa-f-]\{36\}\)) (Booted).*/\1/p' \
    | head -n 1
}

find_available_iphone() {
  local name_filter="${IOS_SIMULATOR_NAME:-}"

  if [[ -n "$name_filter" ]]; then
    xcrun simctl list devices available \
      | sed -n "/$name_filter/s/.*(\([0-9A-Fa-f-]\{36\}\)) (.*/\1/p" \
      | head -n 1
    return
  fi

  xcrun simctl list devices available \
    | sed -n '/iPhone/s/.*(\([0-9A-Fa-f-]\{36\}\)) (Shutdown).*/\1/p' \
    | head -n 1
}

SIMULATOR_ID="${IOS_SIMULATOR_ID:-}"

if [[ -z "$SIMULATOR_ID" ]]; then
  SIMULATOR_ID="$(find_booted_simulator)"
fi

if [[ -z "$SIMULATOR_ID" ]]; then
  SIMULATOR_ID="$(find_available_iphone)"
  if [[ -z "$SIMULATOR_ID" ]]; then
    echo "no available iPhone simulator found" >&2
    echo "set IOS_SIMULATOR_ID to a simulator UUID and retry" >&2
    exit 1
  fi

  xcrun simctl boot "$SIMULATOR_ID" >/dev/null
fi

xcrun simctl bootstatus "$SIMULATOR_ID" -b >/dev/null

bash bindings/apple/scripts/build-ios-sim.sh

OUT_BIN="$ROOT_DIR/target/ios-sim-smoke"

echo "running iOS simulator smoke"
echo "simulator=$SIMULATOR_ID"
echo "dataset=$DATASET_ABS"

xcrun simctl spawn "$SIMULATOR_ID" "$OUT_BIN" "$DATASET_ABS"
