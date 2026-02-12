#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

DATASET_PATH="${1:-tests/datasets/minimal_ascii.ply}"
UNAME_S="$(uname -s)"

if [[ -n "${JAVA_HOME:-}" ]]; then
  JAVA_HOME="$JAVA_HOME"
elif [[ "$UNAME_S" == "Darwin" ]]; then
  JAVA_HOME="$(/usr/libexec/java_home)"
else
  JAVA_BIN="$(readlink -f "$(command -v javac)")"
  JAVA_HOME="$(cd "$(dirname "$JAVA_BIN")/.." && pwd)"
fi

cargo build -p gsplat-ffi-c >/dev/null

LIB_DIR="$ROOT_DIR/target/debug"
OUT_DIR="$ROOT_DIR/target/android-jni"
CLASS_DIR="$OUT_DIR/classes"
JNI_LIB_DIR="$OUT_DIR/lib"
mkdir -p "$CLASS_DIR" "$JNI_LIB_DIR"

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
  apps/android-demo/jni/gsplat_jni.c \
  -I"$JAVA_HOME/include" \
  -I"$JNI_OS_INCLUDE" \
  -L"$LIB_DIR" \
  -lgsplat_ffi_c \
  -o "$JNI_LIB_DIR/libgsplat_jni.$JNI_LIB_EXT"

javac \
  -d "$CLASS_DIR" \
  apps/android-demo/src/com/gsplat/demo/GsplatJniSmoke.java

if [[ "$UNAME_S" == "Darwin" ]]; then
  DYLD_LIBRARY_PATH="$JNI_LIB_DIR:$LIB_DIR" \
  java --enable-native-access=ALL-UNNAMED \
    -Djava.library.path="$JNI_LIB_DIR:$LIB_DIR" \
    -cp "$CLASS_DIR" \
    com.gsplat.demo.GsplatJniSmoke "$DATASET_PATH"
else
  LD_LIBRARY_PATH="$JNI_LIB_DIR:$LIB_DIR" \
  java --enable-native-access=ALL-UNNAMED \
    -Djava.library.path="$JNI_LIB_DIR:$LIB_DIR" \
    -cp "$CLASS_DIR" \
    com.gsplat.demo.GsplatJniSmoke "$DATASET_PATH"
fi
