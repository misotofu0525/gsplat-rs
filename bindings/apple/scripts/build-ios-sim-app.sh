#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS simulator app build is only supported on macOS" >&2
  exit 1
fi

ARCH="$(uname -m)"
IOS_VERSION="${IOS_VERSION:-17.0}"
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

if [[ "$ARCH" == "arm64" ]]; then
  RUST_TARGET="aarch64-apple-ios-sim"
  SWIFT_TARGET="arm64-apple-ios${IOS_VERSION}-simulator"
else
  RUST_TARGET="x86_64-apple-ios"
  SWIFT_TARGET="x86_64-apple-ios${IOS_VERSION}-simulator"
fi

APP_NAME="GsplatIOSDemo"
APP_BUNDLE="$ROOT_DIR/target/ios-sim-app/${APP_NAME}.app"
STATIC_LIB="$ROOT_DIR/target/$RUST_TARGET/debug/libgsplat_ffi_c.a"
SDK_PATH="$(xcrun --sdk iphonesimulator --show-sdk-path)"

rustup target add "$RUST_TARGET" >/dev/null
cargo build -p gsplat-ffi-c --target "$RUST_TARGET"

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE"
cp examples/ios/app/Info.plist "$APP_BUNDLE/Info.plist"
cp "$DATASET_ABS" "$APP_BUNDLE/flowers_1.ply"

xcrun --sdk iphonesimulator swiftc \
  bindings/apple/GsplatKit/Sources/GsplatKit/GsplatKit.swift \
  examples/ios/app/GsplatIOSDemo.swift \
  -parse-as-library \
  -import-objc-header crates/gsplat-ffi-c/include/gsplat.h \
  -sdk "$SDK_PATH" \
  -target "$SWIFT_TARGET" \
  "$STATIC_LIB" \
  -framework UIKit \
  -framework QuartzCore \
  -framework Metal \
  -framework CoreGraphics \
  -framework Foundation \
  -framework Security \
  -lz \
  -lc++ \
  -o "$APP_BUNDLE/$APP_NAME"

codesign --force --sign - "$APP_BUNDLE" >/dev/null

echo "ios simulator app build complete"
echo "rust_target=$RUST_TARGET"
echo "swift_target=$SWIFT_TARGET"
echo "app=$APP_BUNDLE"
echo "bundle_id=com.gsplat.demo.ios"
echo "dataset=$DATASET_ABS"
