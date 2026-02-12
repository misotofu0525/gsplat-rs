#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

DATASET_PATH="${1:-tests/datasets/minimal_ascii.ply}"

cargo build -p gsplat-ffi-c >/dev/null

LIB_DIR="$ROOT_DIR/target/debug"
OUT_BIN="$ROOT_DIR/target/ffi-smoke"

clang tests/ffi/ffi_smoke.c -L"$LIB_DIR" -lgsplat_ffi_c -o "$OUT_BIN"

if [[ "$(uname -s)" == "Darwin" ]]; then
  DYLD_LIBRARY_PATH="$LIB_DIR" "$OUT_BIN" "$DATASET_PATH"
else
  LD_LIBRARY_PATH="$LIB_DIR" "$OUT_BIN" "$DATASET_PATH"
fi
