#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
if [[ ! -d "$ANDROID_SDK_ROOT" ]]; then
  echo "ANDROID_SDK_ROOT not found: $ANDROID_SDK_ROOT"
  exit 1
fi

bash "$ROOT_DIR/apps/android-demo/build-android-native.sh"

cat > "$ROOT_DIR/apps/android-demo/local.properties" <<LOCALPROPS
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

"$GRADLE_BIN" -p "$ROOT_DIR/apps/android-demo" :app:assembleDebug

echo "android apk build complete"
echo "apk=$ROOT_DIR/apps/android-demo/app/build/outputs/apk/debug/app-debug.apk"
