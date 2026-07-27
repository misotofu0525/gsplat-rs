#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT_DIR="$ROOT_DIR/examples/web/pkg"
WASM_PATH="$ROOT_DIR/target/wasm32-unknown-unknown/release/gsplat_web.wasm"

WASM_BINDGEN_BIN="${WASM_BINDGEN_BIN:-}"
if [[ -z "$WASM_BINDGEN_BIN" ]]; then
  WASM_BINDGEN_BIN="$(command -v wasm-bindgen 2>/dev/null || true)"
fi
if [[ -z "$WASM_BINDGEN_BIN" && -x "${CARGO_HOME:-$HOME/.cargo}/bin/wasm-bindgen" ]]; then
  WASM_BINDGEN_BIN="${CARGO_HOME:-$HOME/.cargo}/bin/wasm-bindgen"
fi
if [[ -z "$WASM_BINDGEN_BIN" ]]; then
  echo "wasm-bindgen CLI 0.2.121 is required: cargo install wasm-bindgen-cli --version 0.2.121 --locked" >&2
  exit 1
fi

cargo build -p gsplat-web --target wasm32-unknown-unknown --release
rm -rf "$OUT_DIR"
"$WASM_BINDGEN_BIN" "$WASM_PATH" --target web --out-dir "$OUT_DIR"
