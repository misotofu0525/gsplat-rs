# Task Plan: Adaptive Surface Storage Limits

## Goal

Replace the fixed Surface storage-buffer request with the smallest scene-aware
request supported by the selected adapter, then verify the 700k/750k/1M SH3
boundary on the connected Android device without weakening Direct preflight or
silently changing geometry paths.

## Boundaries

- Direct remains the default and release-gated geometry path.
- Never request the adapter maximum when the scene needs less.
- Never exceed adapter-reported limits or retry device creation in a loop.
- Packed/Paged remain explicit diagnostics; no automatic path promotion.
- Device failures must retain the adapter-limit evidence in the surfaced error.

## Phases

1. [x] Confirm current request logic and physical Android Vulkan limit.
2. [x] Derive exact selected-path storage/buffer requirements and request only
   the needed increase above portable defaults.
3. [x] Cover portable-default, elevated-adapter, insufficient-adapter, and
   packed-path cases with deterministic tests.
4. [x] Build/install once and run 700k, 750k, and 1M Direct probes on device.
5. [x] Record findings, run canonical verification, and archive the bundle.

## Evidence

- Device: Nothing A065 / Adreno 730 / Android 15, serial `033ed212`.
- Android Vulkan query: `maxStorageBufferRange = 134217728` bytes.
- Existing SH3 requirements: 700k = 126,000,000 bytes; 750k = 135,000,000
  bytes; 1M = 180,000,000 bytes.
- Device result: 700k rendered; 750k and 1M were rejected before allocation
  with the exact `ShRest` requirement and `max_direct_splats = 745654`.
