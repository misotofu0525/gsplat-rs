#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
BINDINGS_DIR="$ROOT_DIR/bindings/android"
cd "$ROOT_DIR"

ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
if [[ ! -d "$ANDROID_SDK_ROOT" ]]; then
  echo "ANDROID_SDK_ROOT not found: $ANDROID_SDK_ROOT"
  exit 1
fi

cat > "$BINDINGS_DIR/local.properties" <<LOCALPROPS
sdk.dir=$ANDROID_SDK_ROOT
LOCALPROPS

GRADLE_BIN="$("$BINDINGS_DIR/scripts/ensure-gradle.sh")"

"$GRADLE_BIN" -p "$BINDINGS_DIR" :gsplat-android:assembleRelease

echo "android aar build complete"
echo "aar=$BINDINGS_DIR/gsplat-android/build/outputs/aar/gsplat-android-release.aar"
