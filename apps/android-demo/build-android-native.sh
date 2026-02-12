#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
if [[ ! -d "$ANDROID_SDK_ROOT" ]]; then
  echo "ANDROID_SDK_ROOT not found: $ANDROID_SDK_ROOT"
  exit 1
fi

NDK_VERSION="${NDK_VERSION:-29.0.14206865}"
NDK_ROOT="$ANDROID_SDK_ROOT/ndk/$NDK_VERSION"
if [[ ! -d "$NDK_ROOT" ]]; then
  echo "NDK not found: $NDK_ROOT"
  exit 1
fi

if [[ -d "$NDK_ROOT/toolchains/llvm/prebuilt/darwin-arm64" ]]; then
  TOOLCHAIN_ROOT="$NDK_ROOT/toolchains/llvm/prebuilt/darwin-arm64"
else
  TOOLCHAIN_ROOT="$NDK_ROOT/toolchains/llvm/prebuilt/darwin-x86_64"
fi

if [[ ! -d "$TOOLCHAIN_ROOT" ]]; then
  echo "NDK toolchain not found under: $NDK_ROOT/toolchains/llvm/prebuilt"
  exit 1
fi

CLANG="$TOOLCHAIN_ROOT/bin/aarch64-linux-android35-clang"
if [[ ! -x "$CLANG" ]]; then
  echo "Android clang not found: $CLANG"
  exit 1
fi

JAVA_HOME="${JAVA_HOME:-$(/usr/libexec/java_home)}"

rustup target add aarch64-linux-android >/dev/null
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG" \
  cargo build -p gsplat-ffi-c --target aarch64-linux-android

RUST_STATIC_LIB="$ROOT_DIR/target/aarch64-linux-android/debug/libgsplat_ffi_c.a"
if [[ ! -f "$RUST_STATIC_LIB" ]]; then
  echo "Rust static lib missing: $RUST_STATIC_LIB"
  exit 1
fi

OUT_DIR="$ROOT_DIR/apps/android-demo/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$OUT_DIR"
OUT_SO="$OUT_DIR/libgsplat_jni.so"

"$CLANG" \
  -fPIC \
  -shared \
  "$ROOT_DIR/apps/android-demo/jni/gsplat_jni.c" \
  "$RUST_STATIC_LIB" \
  -I"$JAVA_HOME/include" \
  -I"$JAVA_HOME/include/darwin" \
  -o "$OUT_SO" \
  -llog -ldl -lm

echo "android native build complete"
echo "so=$OUT_SO"
