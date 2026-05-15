#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SDK_DIR="$ROOT_DIR/apps/web-demo/gsplat-web-sdk"
DIST_DIR="$SDK_DIR/dist"
WASM_DIR="$DIST_DIR/wasm"
WASM_PATH="$ROOT_DIR/target/wasm32-unknown-unknown/release/gsplat_web.wasm"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen CLI is required: cargo install wasm-bindgen-cli" >&2
  exit 1
fi

cd "$ROOT_DIR"
cargo build -p gsplat-web --target wasm32-unknown-unknown --release

rm -rf "$DIST_DIR"
mkdir -p "$WASM_DIR"
wasm-bindgen "$WASM_PATH" --target web --out-dir "$WASM_DIR"
cp "$SDK_DIR/src/index.js" "$DIST_DIR/index.js"
cp "$SDK_DIR/src/index.d.ts" "$DIST_DIR/index.d.ts"

echo "web sdk build complete"
echo "sdk=$SDK_DIR"
echo "dist=$DIST_DIR"
echo "wasm=$WASM_DIR/gsplat_web_bg.wasm"
