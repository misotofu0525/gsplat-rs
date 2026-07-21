# Progress: Adaptive Surface Storage Limits

## 2026-07-22

- Created unbudgeted goal and branch `codex/adaptive-storage-limits` from
  `main` commit `9f51781`.
- Re-read the project architecture, verification, roadmap, principles, and
  Android integration boundaries.
- Confirmed device connectivity, thermal status 0, run-as access, and local
  750k/1M deterministic Truck tiers.
- Queried Android Vulkan capabilities directly; Adreno 730 reports
  `maxStorageBufferRange = 134217728` bytes.
- Replaced the fixed Surface storage request with an exact selected-path
  request checked against the adapter's reported limits.
- Preserved full adapter texture resize headroom and added nine focused Surface
  tests covering defaults, elevated limits, hard rejection, large buffers,
  Packed/Paged selection, and oversized surfaces.
- Updated the Android sample to log the native structured last error while
  keeping the UI message bounded.
- Built and installed one exact release-native APK, then ran 700k, 750k, and 1M
  Direct SH3 probes at thermal status 0.
- Confirmed 700k renders and both larger tiers fail deterministically at the
  driver-reported 128 MiB storage-binding boundary.
- Completed canonical workspace verification, focused limit tests, Android APK
  build/install, device artifact validation, and independent code review.
