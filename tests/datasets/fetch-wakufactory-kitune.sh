#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${ROOT}/external/wakufactory_kitune"
PLY_PATH="${OUT_DIR}/kitune1.ply"
TMP_PATH="${PLY_PATH}.download"

URL="https://www.wakufactory.jp/wxr/splats/data/kitune1.ply"
EXPECTED_SHA256="3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2"

mkdir -p "${OUT_DIR}"

if [[ ! -f "${PLY_PATH}" ]]; then
  echo "downloading CC0 showcase scene: ${URL}"
  curl -L --fail --retry 3 --retry-delay 1 -o "${TMP_PATH}" "${URL}"
  mv "${TMP_PATH}" "${PLY_PATH}"
else
  echo "using existing scene: ${PLY_PATH}"
fi

ACTUAL_SHA256="$(shasum -a 256 "${PLY_PATH}" | awk '{print $1}')"
if [[ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]]; then
  echo "checksum mismatch for: ${PLY_PATH}" >&2
  echo "expected=${EXPECTED_SHA256}" >&2
  echo "actual=${ACTUAL_SHA256}" >&2
  exit 1
fi

echo "ply=${PLY_PATH}"
echo "license=CC0"
echo "source=https://www.wakufactory.jp/wxr/splats/sample.html"
