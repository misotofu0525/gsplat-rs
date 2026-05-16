#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS device benchmark is only supported on macOS" >&2
  exit 1
fi

DATASET_ARGS=()
BENCHMARK_ARGS=()

if [[ $# -gt 0 && "$1" != "--" && "$1" != --* ]]; then
  DATASET_ARGS=("$1")
  shift
fi

if [[ $# -gt 0 && "$1" == "--" ]]; then
  shift
fi

if [[ $# -gt 0 ]]; then
  BENCHMARK_ARGS=("$@")
else
  BENCHMARK_ARGS=(
    --gsplat_benchmark true
    --gsplat_benchmark_frames 60
    --gsplat_benchmark_warmup_frames 5
    --gsplat_benchmark_yaw_step 0.001
    --gsplat_surface_sort_interval 2
    --gsplat_surface_gpu_preproject false
    --gsplat_surface_gpu_preproject_double_buffer false
    --gsplat_surface_static_direct false
    --gsplat_surface_async_sort false
    --gsplat_surface_async_geometry false
    --gsplat_surface_instance_buffers 1
    --gsplat_surface_frame_latency 2
  )
fi

find_connected_device() {
  xcrun devicectl list devices \
    | awk '/connected/ && /iPhone/ {print $3; exit}'
}

DEVICE_ID="${IOS_DEVICE_ID:-}"
if [[ -z "$DEVICE_ID" ]]; then
  DEVICE_ID="$(find_connected_device)"
fi

if [[ -z "$DEVICE_ID" ]]; then
  echo "no connected iPhone found" >&2
  echo "connect and unlock the device, enable Developer Mode, then retry" >&2
  exit 1
fi

if [[ ${#DATASET_ARGS[@]} -gt 0 ]]; then
  bash bindings/apple/scripts/build-ios-device-app.sh "${DATASET_ARGS[@]}"
else
  bash bindings/apple/scripts/build-ios-device-app.sh
fi

APP_BUNDLE="$ROOT_DIR/target/ios-device-app/GsplatIOSExample.app"
BUNDLE_ID="${IOS_BUNDLE_ID:-com.gsplat.example.ios}"
LOG_DIR="$ROOT_DIR/target/ios-device-benchmarks"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/benchmark-$(date +%Y%m%d-%H%M%S).log"
DEVICECTL_PID=""

cleanup() {
  if [[ -n "$DEVICECTL_PID" ]]; then
    kill "$DEVICECTL_PID" >/dev/null 2>&1 || true
    wait "$DEVICECTL_PID" >/dev/null 2>&1 || true
  fi

  local remote_pid
  remote_pid="$(xcrun devicectl device info processes --device "$DEVICE_ID" --columns '*' 2>/dev/null \
    | awk '/GsplatIOSExample/ {print $1; exit}')"
  if [[ -n "$remote_pid" ]]; then
    xcrun devicectl device process terminate --device "$DEVICE_ID" --pid "$remote_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

xcrun devicectl device install app --device "$DEVICE_ID" "$APP_BUNDLE" >/dev/null
(
  xcrun devicectl device process launch \
    --terminate-existing \
    --console \
    --device "$DEVICE_ID" \
    "$BUNDLE_ID" \
    "${BENCHMARK_ARGS[@]}" \
    >"$LOG_FILE" 2>&1
) &
DEVICECTL_PID="$!"

for _ in $(seq 1 120); do
  if rg -q 'BENCHMARK_RESULT' "$LOG_FILE"; then
    rg 'BENCHMARK_RESULT' "$LOG_FILE"
    echo "log=$LOG_FILE"
    exit 0
  fi
  sleep 1
done

echo "benchmark timed out; log follows" >&2
cat "$LOG_FILE" >&2
exit 1
