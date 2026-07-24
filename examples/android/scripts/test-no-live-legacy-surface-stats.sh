#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

if rg -n \
  'NativeBridge\.getSurfaceStats|gsplat_surface_renderer_get_stats|GsplatSurfaceStats' \
  examples/android/app/src -g '*.kt' -g '*.java'; then
  echo "Android example source still contains a legacy Surface stats consumer" >&2
  exit 1
fi

echo "Android example live source contains no legacy Surface stats consumer"
