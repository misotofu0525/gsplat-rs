#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CACHE_DIR="${ROOT}/external/.cache"
OUT_DIR="${ROOT}/external/nvidia_flowers_1"
ZIP_PATH="${CACHE_DIR}/flowers_1.zip"

URL="https://developer.download.nvidia.com/ProGraphics/nvpro-samples/flowers_1.zip"

mkdir -p "${CACHE_DIR}" "${OUT_DIR}"

if [[ ! -f "${ZIP_PATH}" ]]; then
  echo "downloading: ${URL}"
  curl -L --fail --retry 3 --retry-delay 1 -o "${ZIP_PATH}" "${URL}"
else
  echo "using cached zip: ${ZIP_PATH}"
fi

if [[ ! -f "${OUT_DIR}/.extracted" ]]; then
  echo "extracting: ${ZIP_PATH}"
  rm -rf "${OUT_DIR:?}/"*
  unzip -q "${ZIP_PATH}" -d "${OUT_DIR}"
  touch "${OUT_DIR}/.extracted"
fi

PLY_PATH="$(find "${OUT_DIR}" -type f -name '*.ply' -print0 | xargs -0 ls -S 2>/dev/null | head -1 || true)"
if [[ -z "${PLY_PATH}" ]]; then
  echo "no .ply found under: ${OUT_DIR}" >&2
  exit 1
fi

ln -sf "${PLY_PATH}" "${OUT_DIR}/model.ply"
echo "ply=${PLY_PATH}"
echo "link=${OUT_DIR}/model.ply"
