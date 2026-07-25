#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "GsplatKit package tests are only supported on macOS" >&2
  exit 1
fi

find_available_iphone() {
  xcrun simctl list devices available \
    | sed -n '/iPhone/s/.*(\([0-9A-Fa-f-]\{36\}\)) (.*/\1/p' \
    | head -n 1
}

bash bindings/apple/scripts/build-xcframework.sh

XCFRAMEWORK="$ROOT_DIR/bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework"
ARCHIVES=()
while IFS= read -r archive; do
  ARCHIVES+=("$archive")
done < <(find "$XCFRAMEWORK" -type f -name '*.a' | sort)
if [[ "${#ARCHIVES[@]}" -ne 2 ]]; then
  echo "expected device and simulator archives in $XCFRAMEWORK" >&2
  exit 1
fi

SYMBOLS=(
  _gsplat_surface_renderer_request_current_stats_v1
  _gsplat_surface_renderer_get_current_stats_submission_v1
  _gsplat_surface_renderer_poll_current_stats_v1
)

for archive in "${ARCHIVES[@]}"; do
  # Apple nm can warn on newer LLVM attributes in unrelated Rust runtime
  # members while still reporting the public symbols from gsplat-ffi-c.
  archive_symbols="$(nm -gUj "$archive" 2>/dev/null || true)"
  for symbol in "${SYMBOLS[@]}"; do
    if [[ $'\n'"$archive_symbols"$'\n' != *$'\n'"$symbol"$'\n'* ]]; then
      echo "missing packaged symbol $symbol in $archive" >&2
      exit 1
    fi
  done
  echo "verified current-stats V1 symbols: ${archive#$ROOT_DIR/}"
done

echo "building GsplatKit against packaged device XCFramework slice"
(
  cd bindings/apple/GsplatKit
  xcodebuild \
    -scheme GsplatKit \
    -destination 'generic/platform=iOS' \
    -derivedDataPath "$ROOT_DIR/target/gsplatkit-package-device-build" \
    CODE_SIGNING_ALLOWED=NO \
    build
)

SIMULATOR_ID="${IOS_SIMULATOR_ID:-$(find_available_iphone)}"
if [[ -z "$SIMULATOR_ID" ]]; then
  echo "no available iPhone simulator found" >&2
  echo "set IOS_SIMULATOR_ID to a simulator UUID and retry" >&2
  exit 1
fi

if ! xcrun simctl list devices booted | grep -Fq "$SIMULATOR_ID"; then
  xcrun simctl boot "$SIMULATOR_ID" >/dev/null
fi
xcrun simctl bootstatus "$SIMULATOR_ID" -b >/dev/null

echo "testing GsplatKit through packaged XCFramework"
echo "simulator=$SIMULATOR_ID"
(
  cd bindings/apple/GsplatKit
  xcodebuild \
    -scheme GsplatKit \
    -destination "platform=iOS Simulator,id=$SIMULATOR_ID" \
    -derivedDataPath "$ROOT_DIR/target/gsplatkit-package-tests" \
    -only-testing:GsplatKitTests/CurrentStatsABIContractTests \
    -only-testing:GsplatKitTests/CurrentStatsTests \
    -only-testing:GsplatKitTests/ProjectedSubmissionContractTests \
    test
)
