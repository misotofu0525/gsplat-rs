# Findings & Decisions

## Requirements

- Extend the new showcase treatment beyond Web to both Android and iOS.
- Reuse the CC0 Kitsune scene and visual story where technically appropriate.
- Keep platform validation capabilities intact.

## Research Findings

- The prior completed showcase selected Wakufactory `kitune1.ply`: 65.9 MB, 279,199 splats, CC0.
- The current worktree contains the completed Web showcase implementation and parser compatibility changes; mobile work must build on these without resetting or separating them.
- Android and iOS both render through the existing `SortedAlpha` path and already expose orbit, pan, pinch zoom, double-tap reset, PLY import, and benchmark launch arguments. Those interaction and benchmark paths should remain intact.
- Both examples currently place a dense monospaced diagnostics block over the render surface and use a platform-default `Import PLY` button. The diagnostic text is useful for smoke tests but should move behind an explicit Studio panel, with only a compact scene/frame summary always visible.
- Android resolves `imported_scene.ply`, then `flowers_1.ply`, then a generated minimal fixture. Its APK build does not package a dataset today.
- iOS always copies the selected build-time dataset into the app bundle as `flowers_1.ply`; both simulator and device scripts default to the NVIDIA Flowers fixture.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Audit native sources and scripts before choosing UI structure | Android and iOS currently own platform-specific Surface lifecycle and test conventions that must not be guessed. |
| Package the build-time model as `showcase.ply` | Both apps can share one stable runtime name while their build scripts continue accepting arbitrary dataset overrides. |
| Prefer Kitsune with Flowers fallback | The new showcase is immediately available after running the Kitsune fetch script without breaking existing contributor setups. |
| Keep a compact status layer and toggleable Studio panel | The first frame becomes editorial while smoke-test diagnostics remain observable. |

## Issues Encountered

| Issue | Resolution |
|-------|------------|

## Resources

- `AGENTS.md`
- `handbook/PROJECT_CONTEXT.md`
- `handbook/ARCHITECTURE.md`
- `handbook/VERIFICATION.md`
- `docs/plans/completed/2026-07-10-viral-showcase-demo/`

## Visual/Runtime Findings

- The iPhone 17 Pro simulator rendered the complete Kitsune scene at 279,199 drawn/visible splats with no create, configuration, or render failures in the captured runtime log.
- The final 368×800 simulator frame keeps the hero and scene telemetry legible without covering the fox or pedestal. Both `Studio` and `Open PLY +` are exposed as accessibility button targets.
- Source-name metadata reports `dataset=kitune1.ply` even though the stable bundled file path is `showcase.ply`; fallback and custom builds therefore remain accurately labeled.
- No Android device or emulator was attached, so Android evidence is limited to successful Kotlin/JNI/APK compilation, AAR/JNI smoke, and inspection of the packaged model/native library.
