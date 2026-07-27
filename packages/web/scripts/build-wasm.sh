#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
WASM_PATH="$ROOT_DIR/target/wasm32-unknown-unknown/release/gsplat_web.wasm"

# Resolve lexical aliases and every existing symlink parent without creating
# the requested leaf. Validation below completes before cargo, rm, bindgen, or
# any other command can create or remove an output directory.
normalize_output_path() {
  PYTHONDONTWRITEBYTECODE=1 python3 - "$1" <<'PY'
import os
import sys

print(os.path.realpath(os.path.abspath(sys.argv[1])))
PY
}

lexical_output_path() {
  PYTHONDONTWRITEBYTECODE=1 python3 - "$1" <<'PY'
import os
import sys

print(os.path.abspath(sys.argv[1]))
PY
}

DEFAULT_OUT_DIR="$ROOT_DIR/examples/web/pkg"
# Candidate20 compares against the real destination, but Exact deliberately
# keeps operating on the historical lexical leaf. If that leaf is a symlink,
# `rm -rf` removes only the link rather than its external target.
DEFAULT_OUT_DIR_REAL="$(normalize_output_path "$DEFAULT_OUT_DIR")"

# Private diagnostics must opt into both the exact profile name and a fresh,
# caller-owned destination. The normal package remains Exact and keeps its
# established examples/web/pkg output.
WASM_PROFILE="${GSPLAT_WEB_WASM_PROFILE:-exact}"
REQUESTED_OUT_DIR="${GSPLAT_WEB_WASM_OUT_DIR:-}"
case "$WASM_PROFILE" in
  exact)
    if [[ -n "$REQUESTED_OUT_DIR" ]]; then
      echo "GSPLAT_WEB_WASM_OUT_DIR is only valid for the candidate20 diagnostic profile" >&2
      exit 2
    fi
    OUT_DIR="$DEFAULT_OUT_DIR"
    ;;
  candidate20)
    if [[ -z "$REQUESTED_OUT_DIR" ]]; then
      echo "candidate20 requires an explicit fresh GSPLAT_WEB_WASM_OUT_DIR" >&2
      exit 2
    fi
    if [[ "$REQUESTED_OUT_DIR" = /* ]]; then
      REQUESTED_ABSOLUTE="$REQUESTED_OUT_DIR"
    else
      REQUESTED_ABSOLUTE="$ROOT_DIR/$REQUESTED_OUT_DIR"
    fi
    REQUESTED_LEXICAL="$(lexical_output_path "$REQUESTED_ABSOLUTE")"
    OUT_DIR="$(normalize_output_path "$REQUESTED_ABSOLUTE")"
    if [[ "$OUT_DIR" == "$DEFAULT_OUT_DIR_REAL" || "$OUT_DIR" == "$DEFAULT_OUT_DIR_REAL/"* ]]; then
      echo "candidate20 output must stay independent from examples/web/pkg" >&2
      exit 2
    fi
    if [[ -e "$REQUESTED_LEXICAL" || -L "$REQUESTED_LEXICAL" \
      || -e "$OUT_DIR" || -L "$OUT_DIR" ]]; then
      echo "candidate20 output must be fresh; preserve the existing path: $OUT_DIR" >&2
      exit 2
    fi
    ;;
  *)
    echo "unknown GSPLAT_WEB_WASM_PROFILE '$WASM_PROFILE'; expected exact or candidate20" >&2
    exit 2
    ;;
esac

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

if [[ "$WASM_PROFILE" == "candidate20" ]]; then
  cargo build -p gsplat-web --target wasm32-unknown-unknown --release \
    --features diagnostic-web-depth-key-candidate20
else
  cargo build -p gsplat-web --target wasm32-unknown-unknown --release
  rm -rf "$OUT_DIR"
fi
"$WASM_BINDGEN_BIN" "$WASM_PATH" --target web --out-dir "$OUT_DIR"
