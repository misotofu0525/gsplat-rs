#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT_DIR/apps/web-demo/pkg"
WASM_PATH="$ROOT_DIR/target/wasm32-unknown-unknown/release/gsplat_web.wasm"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen CLI is required: cargo install wasm-bindgen-cli" >&2
  exit 1
fi

cargo build -p gsplat-web --target wasm32-unknown-unknown --release
rm -rf "$OUT_DIR"
wasm-bindgen "$WASM_PATH" --target web --out-dir "$OUT_DIR"
