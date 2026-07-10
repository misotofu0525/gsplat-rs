# Task Plan: Mobile Showcase Demo

## Goal

Bring the Kitsune showcase experience to the existing Android and iOS examples while preserving their renderer, gesture, benchmark, and C ABI validation roles.

## Current Phase

Complete

## Phases

### Phase 1: Audit Android and iOS

- [x] Map current view hierarchy, scene packaging, overlays, gestures, and build scripts on both platforms.
- [x] Identify shared visual language that fits native constraints.
- [x] Record packaging and verification boundaries.
- **Status:** complete

### Phase 2: Mobile Design and Asset Path

- [x] Define a native mobile composition aligned with the Web showcase.
- [x] Make Kitsune the preferred optional scene with safe Flowers/minimal fallbacks.
- [x] Keep diagnostic and benchmark information available without dominating the first view.
- **Status:** complete

### Phase 3: Android Implementation

- [x] Update the Android sample UI and scene packaging.
- [x] Preserve Surface lifecycle, camera gestures, benchmark extras, and overlay evidence.
- [x] Run canonical Android host/build checks available locally.
- **Status:** complete

### Phase 4: iOS Implementation

- [x] Update the iOS sample UI and scene packaging.
- [x] Preserve UIKit Surface lifecycle, gestures, benchmark arguments, and import behavior.
- [x] Build and run through XcodeBuildMCP where supported.
- **Status:** complete

### Phase 5: Cross-Platform Verification and Delivery

- [x] Run workspace and platform-specific verification.
- [x] Inspect simulator/device presentation where available.
- [x] Update user-facing and architecture documentation.
- [x] Move the completed plan into project history.
- **Status:** complete

## Key Questions

1. How can both examples feel like the same showcase without hiding validation evidence?
2. Where should benchmark and renderer diagnostics live on small screens?
3. Can the existing optional external dataset path package Kitsune without committing the 65.9 MB PLY?
4. Which simulator/device checks are available in the current environment?

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Update the existing Android and iOS examples | Project guidance treats them as shared-library validation surfaces and discourages new app tracks. |
| Preserve the current C ABI and renderer calls | This task is presentation and asset routing, not an API expansion. |
| Use one stable runtime path plus source-name metadata | Native code gets a predictable local path without mislabeling fallback or custom models. |
| Keep full diagnostics behind `Studio` | The first frame stays cinematic while manual smoke and benchmark evidence remains available. |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|

## Notes

- Existing uncommitted Web showcase changes are in scope and must be preserved.
- Kitsune remains an optional fetched asset; repository size should not grow by the full PLY.
