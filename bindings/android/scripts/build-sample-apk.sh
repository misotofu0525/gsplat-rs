#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
BINDINGS_DIR="$ROOT_DIR/bindings/android"
SAMPLE_APP_DIR="$ROOT_DIR/examples/android/app"
cd "$ROOT_DIR"

KITSUNE_DATASET="tests/datasets/external/wakufactory_kitune/kitune1.ply"
FLOWERS_DATASET="tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply"
DATASET_PATH="${1:-}"

if [[ -z "$DATASET_PATH" ]]; then
  if [[ -f "$ROOT_DIR/$KITSUNE_DATASET" ]]; then
    DATASET_PATH="$KITSUNE_DATASET"
  elif [[ -f "$ROOT_DIR/$FLOWERS_DATASET" ]]; then
    DATASET_PATH="$FLOWERS_DATASET"
  fi
fi

GENERATED_ASSETS_DIR="$SAMPLE_APP_DIR/build/generated/showcase-assets"
rm -rf "$GENERATED_ASSETS_DIR"
mkdir -p "$GENERATED_ASSETS_DIR"

if [[ -n "$DATASET_PATH" ]]; then
  case "$DATASET_PATH" in
    /*) DATASET_ABS="$DATASET_PATH" ;;
    *) DATASET_ABS="$ROOT_DIR/$DATASET_PATH" ;;
  esac
  if [[ ! -f "$DATASET_ABS" ]]; then
    echo "missing dataset: $DATASET_PATH" >&2
    exit 1
  fi
  cp "$DATASET_ABS" "$GENERATED_ASSETS_DIR/showcase.ply"
  basename "$DATASET_ABS" > "$GENERATED_ASSETS_DIR/showcase.name"
else
  DATASET_ABS="runtime minimal fallback"
fi

ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
if [[ ! -d "$ANDROID_SDK_ROOT" ]]; then
  echo "ANDROID_SDK_ROOT not found: $ANDROID_SDK_ROOT"
  exit 1
fi

cat > "$BINDINGS_DIR/local.properties" <<LOCALPROPS
sdk.dir=$ANDROID_SDK_ROOT
LOCALPROPS

GRADLE_BIN="$("$BINDINGS_DIR/scripts/ensure-gradle.sh")"

"$GRADLE_BIN" -p "$BINDINGS_DIR" :sample-app:assembleDebug

METADATA="$SAMPLE_APP_DIR/build/outputs/apk/debug/output-metadata.json"
APK_FILE="$(sed -n 's/.*"outputFile"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$METADATA" | head -n 1)"
APK_PATH="$SAMPLE_APP_DIR/build/outputs/apk/debug/${APK_FILE:-sample-app-debug.apk}"
if [[ ! -f "$APK_PATH" ]]; then
  echo "sample APK missing: $APK_PATH" >&2
  exit 1
fi

echo "android apk build complete"
echo "apk=$APK_PATH"
echo "dataset=$DATASET_ABS"
