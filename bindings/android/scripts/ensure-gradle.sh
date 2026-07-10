#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"

GRADLE_VERSION="${GRADLE_VERSION:-8.7}"
GRADLE_SHA256="${GRADLE_SHA256:-544c35d6bd849ae8a5ed0bcea39ba677dc40f49df7d1835561582da2009b961d}"
TARGET_DIR="$ROOT_DIR/target"
GRADLE_DIR="$TARGET_DIR/gradle-$GRADLE_VERSION"
GRADLE_BIN="$GRADLE_DIR/bin/gradle"

hash_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    echo "Neither shasum nor sha256sum is available for Gradle checksum validation" >&2
    exit 1
  fi
}

if [[ ! -x "$GRADLE_BIN" ]]; then
  mkdir -p "$TARGET_DIR"
  ZIP_PATH="$TARGET_DIR/gradle-$GRADLE_VERSION-bin.zip"
  URL="https://services.gradle.org/distributions/gradle-$GRADLE_VERSION-bin.zip"
  curl -fsSL "$URL" -o "$ZIP_PATH"

  ACTUAL_SHA256="$(hash_file "$ZIP_PATH")"
  if [[ "$ACTUAL_SHA256" != "$GRADLE_SHA256" ]]; then
    echo "Gradle distribution checksum mismatch for $URL" >&2
    echo "expected=$GRADLE_SHA256" >&2
    echo "actual=$ACTUAL_SHA256" >&2
    exit 1
  fi

  rm -rf "$GRADLE_DIR"
  unzip -q "$ZIP_PATH" -d "$TARGET_DIR"
fi

echo "$GRADLE_BIN"
