#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
BINDINGS_DIR="$ROOT_DIR/bindings/android"
SAMPLE_APP_DIR="$ROOT_DIR/examples/android/app"
cd "$ROOT_DIR"

ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
if [[ ! -d "$ANDROID_SDK_ROOT" ]]; then
  echo "ANDROID_SDK_ROOT not found: $ANDROID_SDK_ROOT"
  exit 1
fi

bash "$BINDINGS_DIR/scripts/build-native.sh"

cat > "$BINDINGS_DIR/local.properties" <<LOCALPROPS
sdk.dir=$ANDROID_SDK_ROOT
LOCALPROPS

GRADLE_VERSION="${GRADLE_VERSION:-8.7}"
GRADLE_DIR="$ROOT_DIR/target/gradle-$GRADLE_VERSION"
GRADLE_BIN="$GRADLE_DIR/bin/gradle"

if [[ ! -x "$GRADLE_BIN" ]]; then
  ZIP_PATH="$ROOT_DIR/target/gradle-$GRADLE_VERSION-bin.zip"
  URL="https://services.gradle.org/distributions/gradle-$GRADLE_VERSION-bin.zip"
  curl -fsSL "$URL" -o "$ZIP_PATH"
  rm -rf "$GRADLE_DIR"
  unzip -q "$ZIP_PATH" -d "$ROOT_DIR/target"
fi

"$GRADLE_BIN" -p "$BINDINGS_DIR" :sample-app:assembleDebug

echo "android apk build complete"
echo "apk=$SAMPLE_APP_DIR/build/outputs/apk/debug/app-debug.apk"
