# Adaptive Surface Storage Limits Report

## Executive conclusion

The fixed 128 MiB request was partly a wgpu portability default, but it was not
the only limit. Surface device creation now asks for the smallest storage limit
needed by the selected scene/path, allowing adapters that truly expose more to
use that headroom. On the connected Adreno 730, Android's Vulkan capability
data itself says 128 MiB, so 750k and 1M Direct SH3 scenes remain physically
unsupported while 700k renders successfully.

This is the desired fail-safe result: capable devices gain headroom, limited
devices keep a portable request and receive a precise diagnostic, and no scene
is silently downgraded to another render path.

## What changed

Before device creation, the Surface path already has the loaded scene and the
selected geometry path. It now uses those facts in this order:

1. Inspect the adapter's real limits.
2. Calculate every buffer required by the selected path.
3. Take the largest individual storage binding.
4. Start from wgpu's portable defaults and raise storage/buffer limits only if
   that binding needs more.
5. Reject the request if the adapter cannot provide it.

For Direct SH3, the dominant allocation is usually the remaining spherical
harmonics data at 180 bytes per point. Therefore:

```text
700,000 × 180 = 126,000,000 B  -> fits
750,000 × 180 = 135,000,000 B  -> exceeds this device by 782,272 B
1,000,000 × 180 = 180,000,000 B -> exceeds this device by 45,782,272 B
```

Packed and Paged calculate only their selected resident data. They do not ask
for an oversized Direct buffer that will not be used. Texture resize capacity
is deliberately preserved at the full adapter limit because a window can grow
after the device is created.

## Android experiment

Environment:

- Nothing A065 (`033ed212`), Android 15/API 35
- Qualcomm SM8475, Adreno 730
- Vulkan `maxStorageBufferRange = 134217728`
- Thermal status 0 before every run
- APK SHA-256
  `d44edfebeaf417d079b453a29ce624964f82a76e07ccb11a92e8b1abc42f87e3`
- Native SHA-256
  `9b4d564635e259ab55f2ef1d1c0acf1540b20b16c2f1a4139b0f8d5328928e46`

Results:

| Dataset | Required SH3 binding | Observed result |
| --- | ---: | --- |
| Truck 700k | 126,000,000 B | Created and rendered; 700,000 visible/drawn. |
| Truck 750k | 135,000,000 B | Clean preflight failure; max 745,654 points. |
| Truck 1M | 180,000,000 B | Clean preflight failure; max 745,654 points. |

The 700k smoke used one CPU run, 5 warmup frames and 20 measured frames with a
sort interval of 2. It reported 62.748 ms average frame time, 3.381 ms average
CPU preprocessing, and 7.819 ms average CPU sorting. That run proves the
capacity boundary and end-to-end rendering; it is too small to compare sort
strategies statistically.

The 750k and 1M collectors intentionally timed out waiting for benchmark frames
because renderer creation was rejected. Their logcat records contain the exact
limiting resource, required bytes, device limit, and remediation threshold.

## wgpu versus OpenGL

wgpu has conservative defaults so one program can run across multiple backend
families. Those defaults are only the request baseline; the adapter may expose
more, which the new code can use.

OpenGL/OpenGL ES also limits a single shader-storage block and exposes that
limit through its runtime context. Its value depends on the GL driver and is
not inherently larger than Vulkan's. This Android experiment used Vulkan and
therefore does not establish an OpenGL number for the A065. Switching APIs
would also require a real GL backend measurement and would not remove the need
for capacity-aware planning.

## Product decision

Keep Direct as the default release path and keep Packed/Paged explicit. The
right next improvement for scenes over the physical Direct limit is a
deliberate packed/paged product path with quality and performance evidence, not
an unchecked limit override. The limit request itself should remain
deterministic: one exact request, one clear result, no retry loop or hidden
fallback.

The advertised binding limit is a legality ceiling, not a promise of free GPU
memory. Higher-limit devices can now be asked correctly, but allocation may
still fail under memory pressure. A later hardening step should wrap initial
geometry creation in wgpu validation/OOM error scopes so that case becomes as
actionable as the current capability preflight.
