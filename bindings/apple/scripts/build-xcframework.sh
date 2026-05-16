#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS XCFramework build is only supported on macOS" >&2
  exit 1
fi

IOS_XCFRAMEWORK_PROFILE="${IOS_XCFRAMEWORK_PROFILE:-release}"
case "$IOS_XCFRAMEWORK_PROFILE" in
  release)
    CARGO_PROFILE_ARGS=(--release)
    CARGO_TARGET_DIR_NAME="release"
    ;;
  dev|debug)
    CARGO_PROFILE_ARGS=()
    CARGO_TARGET_DIR_NAME="debug"
    ;;
  *)
    echo "Unsupported IOS_XCFRAMEWORK_PROFILE: $IOS_XCFRAMEWORK_PROFILE" >&2
    echo "Expected one of: release, dev" >&2
    exit 1
    ;;
esac

ARCH="$(uname -m)"
if [[ -n "${IOS_XCFRAMEWORK_SIM_TARGETS:-}" ]]; then
  read -r -a SIM_TARGETS <<<"$IOS_XCFRAMEWORK_SIM_TARGETS"
elif [[ "$ARCH" == "arm64" ]]; then
  SIM_TARGETS=(aarch64-apple-ios-sim)
else
  SIM_TARGETS=(x86_64-apple-ios)
fi

DEVICE_TARGET="aarch64-apple-ios"
PACKAGE_DIR="$ROOT_DIR/bindings/apple/GsplatKit"
DEFAULT_OUTPUT="$PACKAGE_DIR/Binaries/GsplatFFI.xcframework"
OUTPUT_PATH="${IOS_XCFRAMEWORK_OUTPUT:-$DEFAULT_OUTPUT}"
STAGING_DIR="$ROOT_DIR/target/ios-xcframework"
HEADERS_DIR="$STAGING_DIR/Headers"

rm -rf "$STAGING_DIR"
mkdir -p "$HEADERS_DIR"
cp "$ROOT_DIR/crates/gsplat-ffi-c/include/gsplat.h" "$HEADERS_DIR/gsplat.h"
cat >"$HEADERS_DIR/module.modulemap" <<'EOF'
module GsplatFFI {
  umbrella header "gsplat.h"
  export *
}
EOF

rustup target add "$DEVICE_TARGET" >/dev/null
cargo build -p gsplat-ffi-c --target "$DEVICE_TARGET" "${CARGO_PROFILE_ARGS[@]}"
DEVICE_LIB="$ROOT_DIR/target/$DEVICE_TARGET/$CARGO_TARGET_DIR_NAME/libgsplat_ffi_c.a"

SIM_LIBS=()
for target in "${SIM_TARGETS[@]}"; do
  rustup target add "$target" >/dev/null
  cargo build -p gsplat-ffi-c --target "$target" "${CARGO_PROFILE_ARGS[@]}"
  SIM_LIBS+=("$ROOT_DIR/target/$target/$CARGO_TARGET_DIR_NAME/libgsplat_ffi_c.a")
done

if [[ "${#SIM_LIBS[@]}" -eq 1 ]]; then
  SIM_LIB="${SIM_LIBS[0]}"
else
  SIM_LIB="$STAGING_DIR/libgsplat_ffi_c_iphonesimulator.a"
  xcrun lipo -create "${SIM_LIBS[@]}" -output "$SIM_LIB"
fi

rm -rf "$OUTPUT_PATH"
mkdir -p "$(dirname "$OUTPUT_PATH")"
xcodebuild -create-xcframework \
  -library "$DEVICE_LIB" \
  -headers "$HEADERS_DIR" \
  -library "$SIM_LIB" \
  -headers "$HEADERS_DIR" \
  -output "$OUTPUT_PATH"

echo "ios xcframework build complete"
echo "profile=$IOS_XCFRAMEWORK_PROFILE"
echo "device_target=$DEVICE_TARGET"
echo "sim_targets=${SIM_TARGETS[*]}"
echo "xcframework=$OUTPUT_PATH"
