#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "iOS device app build is only supported on macOS" >&2
  exit 1
fi

IOS_VERSION="${IOS_VERSION:-17.0}"
BUNDLE_ID="${IOS_BUNDLE_ID:-com.gsplat.example.ios}"
APP_NAME="GsplatIOSExample"
APP_BUNDLE="$ROOT_DIR/target/ios-device-app/${APP_NAME}.app"
RUST_TARGET="aarch64-apple-ios"
SWIFT_TARGET="arm64-apple-ios${IOS_VERSION}"
SDK_PATH="$(xcrun --sdk iphoneos --show-sdk-path)"
DEFAULT_DATASET="tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply"
DATASET_PATH="${1:-$DEFAULT_DATASET}"
IOS_RUST_PROFILE="${IOS_RUST_PROFILE:-release}"
IOS_SWIFT_OPT_LEVEL="${IOS_SWIFT_OPT_LEVEL:--O}"

case "$IOS_RUST_PROFILE" in
  release)
    CARGO_PROFILE_ARGS=(--release)
    CARGO_TARGET_DIR_NAME="release"
    ;;
  dev|debug)
    CARGO_PROFILE_ARGS=()
    CARGO_TARGET_DIR_NAME="debug"
    ;;
  *)
    echo "Unsupported IOS_RUST_PROFILE: $IOS_RUST_PROFILE" >&2
    echo "Expected one of: release, dev" >&2
    exit 1
    ;;
esac

STATIC_LIB="$ROOT_DIR/target/$RUST_TARGET/$CARGO_TARGET_DIR_NAME/libgsplat_ffi_c.a"

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

select_provisioning_profile() {
  if [[ -n "${IOS_PROVISIONING_PROFILE:-}" ]]; then
    echo "$IOS_PROVISIONING_PROFILE"
    return 0
  fi

  local profiles_dir="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
  local profile plist app_identifier bundle_pattern get_task_allow
  for profile in "$profiles_dir"/*.mobileprovision; do
    [[ -f "$profile" ]] || continue
    plist="$(mktemp)"
    if security cms -D -i "$profile" >"$plist" 2>/dev/null; then
      get_task_allow="$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:get-task-allow' "$plist" 2>/dev/null || echo false)"
      app_identifier="$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:application-identifier' "$plist" 2>/dev/null || true)"
      bundle_pattern="${app_identifier#*.}"
      if [[ "$get_task_allow" == "true" && ( "$bundle_pattern" == "$BUNDLE_ID" || "$bundle_pattern" == "*" ) ]]; then
        rm -f "$plist"
        echo "$profile"
        return 0
      fi
    fi
    rm -f "$plist"
  done

  return 1
}

select_code_sign_identity() {
  if [[ -n "${IOS_CODE_SIGN_IDENTITY:-}" ]]; then
    echo "$IOS_CODE_SIGN_IDENTITY"
    return 0
  fi

  security find-identity -v -p codesigning \
    | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' \
    | head -n 1
}

PROVISIONING_PROFILE="$(select_provisioning_profile || true)"
CODE_SIGN_IDENTITY="$(select_code_sign_identity || true)"

if [[ ! -f "$PROVISIONING_PROFILE" ]]; then
  echo "missing provisioning profile for $BUNDLE_ID" >&2
  echo "set IOS_PROVISIONING_PROFILE to a development profile that matches IOS_BUNDLE_ID" >&2
  exit 1
fi

if [[ -z "$CODE_SIGN_IDENTITY" ]]; then
  echo "missing code signing identity" >&2
  echo "set IOS_CODE_SIGN_IDENTITY or install an Apple Development signing identity" >&2
  exit 1
fi

PROFILE_PLIST="$(mktemp)"
security cms -D -i "$PROVISIONING_PROFILE" >"$PROFILE_PLIST"
TEAM_ID="$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:com.apple.developer.team-identifier' "$PROFILE_PLIST")"
APP_IDENTIFIER="$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:application-identifier' "$PROFILE_PLIST" 2>/dev/null || true)"
BUNDLE_PATTERN="${APP_IDENTIFIER#*.}"
GET_TASK_ALLOW="$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:get-task-allow' "$PROFILE_PLIST" 2>/dev/null || echo false)"
if [[ "$GET_TASK_ALLOW" != "true" ]]; then
  rm -f "$PROFILE_PLIST"
  echo "provisioning profile is not a development profile: $PROVISIONING_PROFILE" >&2
  exit 1
fi
if [[ "$BUNDLE_PATTERN" != "$BUNDLE_ID" && "$BUNDLE_PATTERN" != "*" ]]; then
  rm -f "$PROFILE_PLIST"
  echo "provisioning profile App ID does not match bundle id" >&2
  echo "profile_app_id=$APP_IDENTIFIER" >&2
  echo "bundle_id=$BUNDLE_ID" >&2
  exit 1
fi

rustup target add "$RUST_TARGET" >/dev/null
cargo build -p gsplat-ffi-c --target "$RUST_TARGET" "${CARGO_PROFILE_ARGS[@]}"

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE"
cp examples/ios/app/Info.plist "$APP_BUNDLE/Info.plist"
cp "$DATASET_ABS" "$APP_BUNDLE/flowers_1.ply"
cp "$PROVISIONING_PROFILE" "$APP_BUNDLE/embedded.mobileprovision"

/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" "$APP_BUNDLE/Info.plist"
/usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $IOS_VERSION" "$APP_BUNDLE/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Set :MinimumOSVersion $IOS_VERSION" "$APP_BUNDLE/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$APP_BUNDLE/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string iPhoneOS" "$APP_BUNDLE/Info.plist" 2>/dev/null \
  || /usr/libexec/PlistBuddy -c "Set :CFBundleSupportedPlatforms:0 iPhoneOS" "$APP_BUNDLE/Info.plist"

xcrun --sdk iphoneos swiftc \
  bindings/apple/GsplatKit/Sources/GsplatKit/GsplatKit.swift \
  examples/ios/app/GsplatIOSExample.swift \
  -parse-as-library \
  "$IOS_SWIFT_OPT_LEVEL" \
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

ENTITLEMENTS="$ROOT_DIR/target/ios-device-app/${APP_NAME}.entitlements.plist"
cat >"$ENTITLEMENTS" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>application-identifier</key>
  <string>${TEAM_ID}.${BUNDLE_ID}</string>
  <key>com.apple.developer.team-identifier</key>
  <string>${TEAM_ID}</string>
  <key>get-task-allow</key>
  <true/>
</dict>
</plist>
EOF

/usr/bin/codesign \
  --force \
  --sign "$CODE_SIGN_IDENTITY" \
  --entitlements "$ENTITLEMENTS" \
  "$APP_BUNDLE"

rm -f "$PROFILE_PLIST"

echo "ios device app build complete"
echo "rust_profile=$IOS_RUST_PROFILE"
echo "swift_opt_level=$IOS_SWIFT_OPT_LEVEL"
echo "rust_target=$RUST_TARGET"
echo "swift_target=$SWIFT_TARGET"
echo "app=$APP_BUNDLE"
echo "bundle_id=$BUNDLE_ID"
echo "team_id=$TEAM_ID"
echo "provisioning_profile=$(basename "$PROVISIONING_PROFILE")"
echo "dataset=$DATASET_ABS"
