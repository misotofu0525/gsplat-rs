#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS simulator app run is only supported on macOS" >&2
  exit 1
fi

DATASET_ARGS=()
LAUNCH_ARGS=()

if [[ $# -gt 0 && "$1" != "--" && "$1" != --* ]]; then
  DATASET_ARGS=("$1")
  shift
fi

if [[ $# -gt 0 && "$1" == "--" ]]; then
  shift
fi

LAUNCH_ARGS=("$@")

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

if [[ ${#DATASET_ARGS[@]} -gt 0 ]]; then
  bash bindings/apple/scripts/build-ios-sim-app.sh "${DATASET_ARGS[@]}"
else
  bash bindings/apple/scripts/build-ios-sim-app.sh
fi

APP_BUNDLE="$ROOT_DIR/target/ios-sim-app/GsplatIOSDemo.app"
BUNDLE_ID="com.gsplat.demo.ios"

xcrun simctl install "$SIMULATOR_ID" "$APP_BUNDLE"
if [[ ${#LAUNCH_ARGS[@]} -gt 0 ]]; then
  xcrun simctl launch --terminate-running-process "$SIMULATOR_ID" "$BUNDLE_ID" "${LAUNCH_ARGS[@]}"
else
  xcrun simctl launch --terminate-running-process "$SIMULATOR_ID" "$BUNDLE_ID"
fi

echo "ios simulator app launched"
echo "simulator=$SIMULATOR_ID"
echo "bundle_id=$BUNDLE_ID"
if [[ ${#LAUNCH_ARGS[@]} -gt 0 ]]; then
  printf 'launch_args='
  printf '%q ' "${LAUNCH_ARGS[@]}"
  printf '\n'
fi
