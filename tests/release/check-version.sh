#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

VERSION="${RELEASE_VERSION:-${GITHUB_REF_NAME:-}}"
VERSION="${VERSION#v}"
if [[ -z "$VERSION" ]]; then
  echo "set RELEASE_VERSION or run from a tag workflow" >&2
  exit 1
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?(\+[0-9A-Za-z][0-9A-Za-z.-]*)?$ ]]; then
  echo "release version is not semantic versioning: $VERSION" >&2
  exit 1
fi

manifests=(crates/*/Cargo.toml examples/desktop/Cargo.toml tools/bench-runner/Cargo.toml)
for manifest in "${manifests[@]}"; do
  manifest_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$manifest" | head -1)"
  if [[ "$manifest_version" != "$VERSION" ]]; then
    echo "version mismatch: $manifest has $manifest_version, expected $VERSION" >&2
    exit 1
  fi

  while IFS= read -r dependency_version; do
    if [[ "$dependency_version" != "$VERSION" ]]; then
      echo "internal dependency version mismatch: $manifest has $dependency_version, expected $VERSION" >&2
      exit 1
    fi
  done < <(sed -n 's/^gsplat-[^ ]* = {[^}]*version = "\([^"]*\)".*/\1/p' "$manifest")
done

web_version="$(sed -n 's/^  "version": "\([^"]*\)",/\1/p' packages/web/package.json)"
if [[ "$web_version" != "$VERSION" ]]; then
  echo "version mismatch: packages/web/package.json has $web_version, expected $VERSION" >&2
  exit 1
fi

if ! rg -Fq "GSPLAT_WEB_SDK_VERSION = \"$VERSION\"" packages/web/src/index.js; then
  echo "version mismatch: packages/web/src/index.js" >&2
  exit 1
fi
if ! rg -Fq "GSPLAT_WEB_SDK_VERSION: \"$VERSION\"" packages/web/src/index.d.ts; then
  echo "version mismatch: packages/web/src/index.d.ts" >&2
  exit 1
fi
if ! rg -Fq "versionName = \"$VERSION\"" examples/android/app/build.gradle.kts; then
  echo "version mismatch: examples/android/app/build.gradle.kts" >&2
  exit 1
fi

major="${VERSION%%.*}"
remainder="${VERSION#*.}"
minor="${remainder%%.*}"
if ! rg -q "GSPLAT_API_VERSION_MAJOR: u32 = $major;" crates/gsplat-core/src/lib.rs; then
  echo "C API major version mismatch" >&2
  exit 1
fi
if ! rg -q "GSPLAT_API_VERSION_MINOR: u32 = $minor;" crates/gsplat-core/src/lib.rs; then
  echo "C API minor version mismatch" >&2
  exit 1
fi
if ! rg -q "#define GSPLAT_API_VERSION_MAJOR_VALUE $major" crates/gsplat-ffi-c/include/gsplat.h; then
  echo "C header major version mismatch" >&2
  exit 1
fi
if ! rg -q "#define GSPLAT_API_VERSION_MINOR_VALUE $minor" crates/gsplat-ffi-c/include/gsplat.h; then
  echo "C header minor version mismatch" >&2
  exit 1
fi
if ! rg -Fq "const API_VERSION = \"$major.$minor\";" examples/web/src/main.js; then
  echo "Web example API version mismatch" >&2
  exit 1
fi

echo "release version check ok: $VERSION"
