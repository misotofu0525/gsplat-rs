#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS device app run is only supported on macOS" >&2
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

DEVICE_ID="${IOS_DEVICE_ID:-}"

if [[ -z "$DEVICE_ID" ]]; then
  echo "set IOS_DEVICE_ID to the target iPhone identifier" >&2
  echo "available devices:" >&2
  xcrun devicectl list devices >&2 || true
  exit 1
fi

if [[ ${#DATASET_ARGS[@]} -gt 0 ]]; then
  bash bindings/apple/scripts/build-ios-device-app.sh "${DATASET_ARGS[@]}"
else
  bash bindings/apple/scripts/build-ios-device-app.sh
fi

APP_BUNDLE="$ROOT_DIR/target/ios-device-app/GsplatIOSExample.app"
BUNDLE_ID="${IOS_BUNDLE_ID:-com.gsplat.example.ios}"

xcrun devicectl device install app --device "$DEVICE_ID" "$APP_BUNDLE"
if [[ ${#LAUNCH_ARGS[@]} -gt 0 ]]; then
  xcrun devicectl device process launch --terminate-existing --device "$DEVICE_ID" "$BUNDLE_ID" "${LAUNCH_ARGS[@]}"
else
  xcrun devicectl device process launch --terminate-existing --device "$DEVICE_ID" "$BUNDLE_ID"
fi

echo "ios device app launched"
echo "device=$DEVICE_ID"
echo "bundle_id=$BUNDLE_ID"
if [[ ${#LAUNCH_ARGS[@]} -gt 0 ]]; then
  printf 'launch_args='
  printf '%q ' "${LAUNCH_ARGS[@]}"
  printf '\n'
fi
