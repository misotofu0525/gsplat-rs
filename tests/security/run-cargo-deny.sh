#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="0.20.2"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    TARGET="x86_64-unknown-linux-musl"
    EXPECTED_SHA256="9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
    ;;
  Linux-aarch64 | Linux-arm64)
    TARGET="aarch64-unknown-linux-musl"
    EXPECTED_SHA256="995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d"
    ;;
  Darwin-arm64)
    TARGET="aarch64-apple-darwin"
    EXPECTED_SHA256="fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3"
    ;;
  Darwin-x86_64)
    TARGET="x86_64-apple-darwin"
    EXPECTED_SHA256="248da7f581724e470071990c088ffc55c811981715f4cbdb258621fb79f8b7a6"
    ;;
  *)
    echo "unsupported cargo-deny bootstrap platform: $(uname -s)-$(uname -m)" >&2
    echo "install cargo-deny $VERSION and run: cargo deny check" >&2
    exit 1
    ;;
esac

INSTALL_DIR="$ROOT_DIR/target/cargo-deny-$VERSION-$TARGET"
BINARY="$INSTALL_DIR/cargo-deny"
ARCHIVE="$INSTALL_DIR/cargo-deny.tar.gz"

if [[ ! -x "$BINARY" ]]; then
  mkdir -p "$INSTALL_DIR"
  URL="https://github.com/EmbarkStudios/cargo-deny/releases/download/$VERSION/cargo-deny-$VERSION-$TARGET.tar.gz"
  curl --fail --location --silent --show-error --retry 3 --retry-all-errors \
    --output "$ARCHIVE" "$URL"

  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL_SHA256="$(sha256sum "$ARCHIVE" | awk '{print $1}')"
  else
    ACTUAL_SHA256="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
  fi
  if [[ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]]; then
    echo "cargo-deny checksum mismatch" >&2
    echo "expected=$EXPECTED_SHA256" >&2
    echo "actual=$ACTUAL_SHA256" >&2
    exit 1
  fi

  tar -xzf "$ARCHIVE" -C "$INSTALL_DIR" --strip-components=1
fi

cd "$ROOT_DIR"
exec "$BINARY" check "$@"
