# Findings: Adaptive Surface Storage Limits

## Initial facts

- `SurfacePresenter` currently starts from `wgpu::Limits::downlevel_defaults()`
  and only raises the requested texture dimension.
- Direct preflight uses the smaller of `max_storage_buffer_binding_size` and
  `max_buffer_size` for every single buffer binding.
- Android's `cmd gpu vkjson` reports the physical Vulkan
  `maxStorageBufferRange` for Adreno 730 as exactly 128 MiB. This device cannot
  accept the 135,000,000-byte 750k SH3 binding even if wgpu requests more.
- The useful implementation is therefore capability-aware portability for
  adapters above the minimum, not a device-specific override.

## Implemented decision

- Build the resource plan against the adapter's reported physical limits.
- Validate the selected geometry path before device creation.
- Begin with `wgpu::Limits::downlevel_defaults()` and raise only
  `max_storage_buffer_binding_size` and `max_buffer_size` to the largest single
  selected-path binding when necessary.
- Preserve the adapter's full `max_texture_dimension_2d` so a Surface created
  for a small window can still resize to a larger display.
- Do not request the adapter's storage maximum, retry device creation, silently
  select Packed/Paged, or alter the offscreen lifetime in this change.
- Keep successful-device capability telemetry internal for now. The existing
  structured Direct preflight and Android last-error path expose all evidence
  needed for an actionable failure without expanding the public API.

## Device evidence

The same freshly built APK/native library was used for all three probes:

- APK SHA-256:
  `d44edfebeaf417d079b453a29ce624964f82a76e07ccb11a92e8b1abc42f87e3`
- `libgsplat_jni.so` SHA-256:
  `9b4d564635e259ab55f2ef1d1c0acf1540b20b16c2f1a4139b0f8d5328928e46`
- Device: Nothing A065, Android 15/API 35, Adreno 730, thermal status 0.

| Tier | Largest Direct SH3 binding | Result |
| --- | ---: | --- |
| 700k | 126,000,000 B | Rendered 20 measured frames after 5 warmup frames. |
| 750k | 135,000,000 B | Rejected: 782,272 B over the Vulkan limit. |
| 1M | 180,000,000 B | Rejected: 45,782,272 B over the Vulkan limit. |

The 700k capacity smoke reported all 700,000 points visible/drawn, average
frame time 62.748 ms, CPU preprocess 3.381 ms, and CPU sort 7.819 ms. This
single 20-frame run is capacity evidence, not a CPU/GPU performance verdict.

Both rejected scenes reported `ShRest` as the limiting resource,
`effective_storage_binding_limit = 134217728`, and
`max_direct_splats = 745654`. The Android UI receives a bounded message while
logcat retains the complete structured error.

## Interpretation

- The common 128 MiB wgpu downlevel default was unnecessarily restrictive on
  adapters that advertise more. Such adapters can now request the exact scene
  requirement: for example, 750k SH3 requests 135,000,000 B rather than the
  adapter maximum.
- This A065 cannot benefit because its Vulkan driver independently advertises
  the same 134,217,728-byte maximum. Raising the requested wgpu limit cannot
  exceed a physical adapter limit.
- OpenGL/OpenGL ES has the analogous runtime-queryable shader-storage-block
  limit; it is not a universal escape hatch. This experiment exercised the
  Android Vulkan backend, so a GL backend needs its own context query and
  device evidence before making a claim about a particular phone.

## Known scope boundary

The requested storage increase follows the initially selected geometry path.
The experimental runtime path switch remains transactional: switching from a
small Packed/Paged allocation to an oversized Direct scene can fail and keep
the old path intact. Automatically reserving Direct capacity would violate the
smallest-selected-path rule and waste portability headroom.

Adapter limits describe the largest legal allocation/binding, not available
GPU-memory budget. A device advertising more than 128 MiB can now receive the
correct request, but a large scene may still fail from memory pressure. Initial
geometry allocation does not yet wrap wgpu OOM/validation error scopes; adding
that structured allocation diagnostic is useful follow-up work, not evidence
that the capability request itself should retry or over-allocate.
