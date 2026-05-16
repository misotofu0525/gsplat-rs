#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
BINDINGS_DIR="$ROOT_DIR/bindings/android"
cd "$ROOT_DIR"

DATASET_PATH="${1:-tests/datasets/minimal_ascii.ply}"
if [[ "$DATASET_PATH" != /* ]]; then
  DATASET_PATH="$ROOT_DIR/$DATASET_PATH"
fi
UNAME_S="$(uname -s)"

if [[ -n "${JAVA_HOME:-}" ]]; then
  JAVA_HOME="$JAVA_HOME"
elif [[ "$UNAME_S" == "Darwin" ]]; then
  JAVA_HOME="$(/usr/libexec/java_home)"
else
  JAVA_BIN="$(readlink -f "$(command -v java)")"
  JAVA_HOME="$(cd "$(dirname "$JAVA_BIN")/.." && pwd)"
fi

cargo build -p gsplat-ffi-c >/dev/null

LIB_DIR="$ROOT_DIR/target/debug"
OUT_DIR="$ROOT_DIR/target/android-jni"
JNI_LIB_DIR="$OUT_DIR/lib"
mkdir -p "$JNI_LIB_DIR"

if [[ "$UNAME_S" == "Darwin" ]]; then
  JNI_OS_INCLUDE="$JAVA_HOME/include/darwin"
  JNI_LIB_EXT="dylib"
else
  JNI_OS_INCLUDE="$JAVA_HOME/include/linux"
  JNI_LIB_EXT="so"
fi

clang \
  -fPIC \
  -shared \
  "$BINDINGS_DIR/jni/gsplat_jni.c" \
  -I"$JAVA_HOME/include" \
  -I"$JNI_OS_INCLUDE" \
  -L"$LIB_DIR" \
  -lgsplat_ffi_c \
  -o "$JNI_LIB_DIR/libgsplat_jni.$JNI_LIB_EXT"

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

"$GRADLE_BIN" \
  -p "$BINDINGS_DIR" \
  :host-smoke:run \
  -PgsplatJniLibPath="$JNI_LIB_DIR" \
  -PgsplatFfiLibPath="$LIB_DIR" \
  -PgsplatDatasetPath="$DATASET_PATH" \
  --quiet
