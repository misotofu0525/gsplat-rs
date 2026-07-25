#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/gsplat-ios-render-loop-lifecycle.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT

cd "$ROOT_DIR"

swiftc -parse-as-library \
  examples/ios/app/RenderLoopLifecycle.swift \
  examples/ios/app/tests/RenderLoopLifecycleTests.swift \
  -o "$TEST_DIR/render-loop-lifecycle-tests"

"$TEST_DIR/render-loop-lifecycle-tests"
