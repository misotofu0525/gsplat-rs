# Resident GPU base-256 radix evidence

Date: 2026-07-23 (Asia/Shanghai)

## Scope

> Android qualification update: the radix-256 path below remains accepted on
> the recorded Metal adapter only. It produced corrupt source-ID output and an
> almost-black full-Truck image on the connected Adreno Vulkan device even
> after two more deterministic workgroup variants were tested. Android now
> uses the exact radix-16 x 8 path. See `android-surface-evidence.md` for the
> rejection evidence, full-resolution screenshot, and final performance data.

This change affects only the full-resident SoA order path used by
Packed/Projected rendering on devices with at least five compute-stage storage
bindings. Direct and the four-binding SoA fallback retain the original stable
base-16 implementation and eight passes.

Resident now performs four stable LSD byte passes over the complete 32-bit
depth key. It does not truncate the key to 20 or 24 bits. Each pass uses:

- a 256-entry workgroup histogram;
- a digit-major `(256 * workgroup_count)` global histogram followed by the
  existing hierarchical exclusive prefix scan;
- 256 four-word masks covering every lane in the 128-lane workgroup;
- one elected lane per non-empty mask word to advance the prior count and
  clear the mask; and
- one fused scatter that writes both the full key and source ID.

WGSL-defined [zero initialization of workgroup atomic composites](https://www.w3.org/TR/WGSL/#var-decls)
removes a redundant bulk clear. Every contributing bit is cleared before the next round.
The conservative Resident preflight requires 6,144 bytes of workgroup storage.

## Correctness evidence

All commands used the Metal backend on the Apple M4 host.

```text
WGPU_BACKEND=metal cargo test -p gsplat-render-wgpu --lib
  203 passed; 3 intentionally ignored

WGPU_BACKEND=metal cargo test -p gsplat-render-wgpu --lib \
  resident_radix8_matches_cpu_at_full_truck_count -- --ignored --nocapture
  RESIDENT_RADIX8_FULL_TRUCK_ORDER count=2541226 passes=4 radix=256 stable_exact=true

WGPU_BACKEND=metal cargo test -p gsplat-render-wgpu --lib \
  external_garden_view1_cpu_gpu_visible_sets_are_identical -- --ignored --nocapture
  source_count=5834784 cpu_visible=4226208 gpu_visible=4226208 differing_source_ids=0

cargo clippy -p gsplat-render-wgpu --all-targets -- -D warnings
  passed
```

The normal library suite includes the Direct low-binding regression, adversarial
full-32-bit stable-order vectors, CPU/GPU key-generation parity, complete
Projected GPU-order draw-count parity, and the Projected-vs-Global byte-for-byte
image gate. The external Garden test additionally proves the complete real
5.835M-source visibility set after the new radix path.

Evidence hashes:

```text
87a8b40d512a94216c7f36b44ace590c99e2424326d2f678b18df3f8d5a05f42  render-lib-final-separated.log
baf83fc0dff9735d294764d5c6b09edff52a83dddd4904185fe979ca4f76acb6  full-truck-order-test-final-separated.log
01d4edc76d98a7e1c7b9e92dfd9e8facf0b92bdcff46cc6f94c255dd1b77f0  garden-view1-cpu-gpu-parity-final.log
```

The ignored artifacts live under `target/full-quality-surface-metal/radix8/`.

## 1080p full-Truck A/B

The controlled workload is complete Truck (2,541,226 resident splats, SH3),
Packed + ProjectedQuadsExact, Metal, 1920x1080, alternating two-view moving
trace, sort interval 1, forced GPU, 20 warmups and 80 measured frames. No
sampling, point reduction, SH downgrade, dynamic resolution, or upscaling is
active. Three before and three after runs use the same trace hash
`34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3`.

| implementation | trial GPU order ms | trial throughput FPS | median GPU order ms | median FPS |
| --- | --- | --- | ---: | ---: |
| stable base-16, 8 passes | 29.2456 / 29.6284 / 29.2031 | 29.5767 / 29.1616 / 29.6170 | 29.2456 | 29.5767 |
| stable base-256, 4 passes | 27.9698 / 27.6694 / 27.9934 | 30.7213 / 31.1006 / 30.7212 | 27.9698 | 30.7213 |

The median order stage is 4.36% shorter and end-to-end Surface throughput is
3.87% higher. This is a real but bounded gain: the exact projected raster and
queue contention still dominate much of the frame, so this radix change alone
does not establish competitor parity.

Final after-run hashes:

```text
f6e35bcc8e3f22e2f0454a7babb776f2902312c4122e5933d0787145846f8669  truck-1080-moving-radix8-final-gpu.log
5ffb6507531ec5d0cd5b43c136cbde06e7ea8622fc1fc260f0bcec1b6f42f256  truck-1080-moving-radix8-final2-gpu.log
5dbf4541aed6875e9e099d43542de4a33f8ce37fe7fb3f5832af8ac10e49bb47  truck-1080-moving-radix8-final3-gpu.log
f5c50d7bc57eb5a6adbe15f14c8d68a05238eda96beefc6e194772b0292407f5  truck-1080-moving-radix8-separated-gpu.log
```
