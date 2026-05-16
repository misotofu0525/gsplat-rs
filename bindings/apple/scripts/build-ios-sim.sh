#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS simulator build is only supported on macOS"
  exit 1
fi

ARCH="$(uname -m)"
IOS_VERSION="${IOS_VERSION:-17.0}"

if [[ "$ARCH" == "arm64" ]]; then
  RUST_TARGET="aarch64-apple-ios-sim"
  SWIFT_TARGET="arm64-apple-ios${IOS_VERSION}-simulator"
else
  RUST_TARGET="x86_64-apple-ios"
  SWIFT_TARGET="x86_64-apple-ios${IOS_VERSION}-simulator"
fi

rustup target add "$RUST_TARGET" >/dev/null
cargo build -p gsplat-ffi-c --target "$RUST_TARGET"

SDK_PATH="$(xcrun --sdk iphonesimulator --show-sdk-path)"
LIB_DIR="$ROOT_DIR/target/$RUST_TARGET/debug"
OUT_BIN="$ROOT_DIR/target/ios-sim-smoke"

xcrun --sdk iphonesimulator swiftc \
  bindings/apple/GsplatKit/Sources/GsplatKit/GsplatKit.swift \
  bindings/apple/smoke/main.swift \
  -import-objc-header crates/gsplat-ffi-c/include/gsplat.h \
  -sdk "$SDK_PATH" \
  -target "$SWIFT_TARGET" \
  -L "$LIB_DIR" \
  -lgsplat_ffi_c \
  -o "$OUT_BIN"

echo "ios simulator build complete"
echo "rust_target=$RUST_TARGET"
echo "swift_target=$SWIFT_TARGET"
echo "binary=$OUT_BIN"
