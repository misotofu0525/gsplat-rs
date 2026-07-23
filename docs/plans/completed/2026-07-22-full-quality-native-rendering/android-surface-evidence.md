# Android full-resolution Surface evidence

Date: 2026-07-23 (Asia/Shanghai)

## Scope and non-negotiable quality contract

The final Android qualification is not the earlier 640x360 diagnostic. It ran
on the connected Nothing A065 (`Pong`, Android 15/API 35, Snapdragon SM8475,
Adreno Vulkan) with a real landscape Surface and presentation size of
2412x1080. The workload is complete INRIA Truck:

```text
PLY bytes          630,225,580
PLY SHA-256        65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c
source splats      2,541,226
resident splats    2,541,226
source/resident SH 3 / 3
membership         all
sampling / LOD     disabled / disabled
dynamic resolution disabled
upscaling          disabled
raster             PackedAtlas + ProjectedQuadsExact
trace              candidate-truck-quality-2view-2412x1080-v1
trace content SHA  0331bc277c7061a9fec5a026335dc6cf6a2c177d237be6e9a54c60330c1e3dfc
view visible/drawn 1,886,298/1,886,298 and 1,672,013/1,672,013
```

Every current manifest reports requested = Surface = internal render =
presented = 2412x1080 and `trace_display_exact`. The current performance APK
SHA-256 is
`7c00a103d2a6b8a788f79f8e448690d0bc5e6dd44eeb72eb8cc6b1458f2f8116`;
its native library SHA-256 is
`3627e897295d907439f15d23af8a4fa961e47eb508c23d2f0711e4b1fb96ef4e`.
These identities are recorded by the complete nine-run experiment manifest,
not inferred from the current file left in the build directory.

## Android radix-256 rejection and safety policy

The first Android GPU artifacts looked fast but were not quality-valid. The
GPU terminal receipt claimed all 1,886,298 visible splats were drawn while the
fixed-view screenshot was almost black with only sparse streaks. A same-build
CPU fixed view rendered the complete Truck correctly. This proved that count
telemetry alone is not an image-correctness gate.

The fault was isolated to the new Resident radix-256 x 4 ordering path:

1. Its four ping-pong passes end in the buffer bound by ProjectedQuadsExact, so
   final-buffer parity was not the cause.
2. Explicitly clearing every workgroup histogram, mask and prior did not repair
   the Adreno image.
3. Replacing the elected-lane atomic-prior update with deterministic per-digit
   reduction also did not repair it.
4. Changing only Resident ordering to the existing full-32-bit radix-16 x 8
   path immediately restored the complete image on the same device and view.

The accepted code therefore keeps Metal's validated radix-256 path but selects
the portable exact radix-16 path on Android. This is not a quality fallback:
it retains every source ID, all 32 depth-key bits, SH3, resolution and the same
draw path; it performs four additional stable radix passes. Radix-256 must not
be re-enabled on Android until a complete-device readback gate proves all
visible IDs are in range, unique, depth-monotonic and equal to the CPU stable
order, followed by real screenshot validation.

Fixed-view screenshots from the earlier image-correctness APK are:

```text
target/android-full-quality-2412x1080/truck-projected-final-apk-live/cpu-view0.png
  SHA-256 05b8eee0974f3252634735df01c91fbbad88f0d4a1131e369f2b15e691ccd207
target/android-full-quality-2412x1080/truck-projected-final-apk-live/gpu-view0.png
  SHA-256 59e04dfb4c374b23661d26c2eed4be47665cfdd49106bc28ed8e3446fe332f66
```

That image APK had SHA-256
`3c5a44ccaeb1c92c7fbc774cef3aeedc141fd9e3e16c48891945a73e58d774ef`
and native-library SHA-256
`89e3786e2247f8450bd2083b92a57ba3554c1d5f95bb9bf6df4da56f8fa028a1`.
Both screenshots are 2412x1080 and show the complete Truck. A central 1000x550 crop that
excludes the changing clock and app chrome measured 84.215 dB RGB PSNR and
SSIM `1.000000` (rounded to six decimals) between CPU and GPU screenshots.
This screenshot comparison is supporting device evidence, not a replacement
for the existing Direct-oracle image gate.

## Current forced CPU, forced GPU, and Adaptive result

The current experiment used three randomized repetitions (seed 20260723), 20
warmups plus 80 measured frames per backend, alternating the two exact camera
views, sorting every frame, frame latency 2, and a fixed 10 second cooldown.
Thermal status was 0 before and after all nine runs. It includes the retained
bounded native CPU preprocessing implementation.

| backend | frame-wall trials | median frame wall | median throughput | relevant stages / policy |
| --- | --- | ---: | ---: | --- |
| forced CPU | 170.384 / 179.335 / 179.656 ms | 179.335 ms | 5.576 FPS | preprocess 19.49--20.64 ms; radix 21.44--22.72 ms |
| forced GPU | 199.016 / 203.805 / 209.458 ms | 203.805 ms | 4.907 FPS | total GPU order 45.84--45.91 ms |
| Adaptive | 175.663 / 184.785 / 183.350 ms | 183.350 ms | 5.454 FPS | 69 CPU / 11 GPU measured frames in every run; final `cpu_stable` |

On this device and complete moving Truck workload, the median forced-CPU run
has 12.0% lower frame wall and 13.6% higher throughput than the median
forced-GPU run. The result is workload-specific: a fixed view reuses the exact
projection cache and is substantially faster, so a stationary screenshot loop
must not be reported as moving-camera FPS.

All nine artifacts are complete and validator-clean. Each has 100 issued
terminal tickets including warmup and exactly 100 successful terminals, with
no failed, unsampled, dropped or fallback measurement. Every manifest has all
five exact counts equal to 2,541,226, SH3 preserved, and native 2412x1080
presentation. Raw evidence:

```text
target/full-quality-final/post-parallel-android-truck-2412x1080-cpu-gpu-adaptive-r3-20260723/
```

## Adaptive metric correction

The previous selector used only the local ordering interval. It observed GPU
order near 46 ms versus CPU preprocess+sort near 54 ms and promoted GPU, even
though forced end-to-end frame wall was worse. GPU sorting and projection/
raster share the same queue, so optimizing one isolated interval can increase
the time at which a complete frame becomes visible.

Two 160-frame runs demonstrated the failure: the old policy used 85 CPU and 75
GPU measured frames, ended in `cpu_probe` (GPU incumbent probing CPU), and
averaged 201.120 / 202.366 ms frame wall.

The primary adaptive metric now uses the existing paired ABBA queue-completion
receipts. While a formal asynchronous receipt is pending, normal frames remain
on the incumbent, preventing readback latency from turning a probe into long
challenger residency. The same two-run qualification then produced:

| run | mean frame wall including exploration | CPU/GPU measured frames | CPU completion mean | GPU completion mean | final state |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 200.556 ms | 137 / 23 | 401.464 ms | 416.268 ms | `cpu_stable` |
| 2 | 197.269 ms | 137 / 23 | 394.939 ms | 408.964 ms | `cpu_stable` |

The final twelve frames of both runs are all CPU, have zero presented revision
lag, and retain exact view counts. Exploration cost is included in the means;
the table is not presented as steady-state CPU FPS. Each run has 180 issued
and 180 successful terminal receipts, zero failure/unsampled/fallback, and
thermal status 0 at both ends. Raw evidence:

```text
target/android-full-quality-2412x1080/truck-projected-android-base16-frame-completion-adaptive-r2/
```

That two-run table is retained as the policy-fix attribution experiment. The
newer nine-run qualification above supersedes it as current performance
evidence: all three Adaptive runs converge to `cpu_stable`, each with exactly
69 CPU and 11 GPU measured frames and zero terminal failure or unsampled
measurement.

## Point-count ladder and the original 200k decision

The complete pre-parallel forced ladder remains useful because it covers eight
exact SH3 prefixes with three randomized CPU/GPU repetitions at native
2412x1080. All 48 runs completed with thermal status 0, full source/resident
count receipts, and no matching panic/validation/OOM artifact:

| source splats | CPU median frame wall | GPU median frame wall | lower wall |
| ---: | ---: | ---: | --- |
| 50,000 | 1.438 ms | 2.683 ms | CPU |
| 100,000 | 2.590 ms | 4.495 ms | CPU |
| 200,000 | 4.874 ms | 8.514 ms | CPU |
| 300,000 | 7.367 ms | 12.254 ms | CPU |
| 500,000 | 12.755 ms | 19.894 ms | CPU |
| 1,000,000 | 28.199 ms | 40.417 ms | CPU |
| 1,500,000 | 62.157 ms | 79.846 ms | CPU |
| 2,000,000 | 114.996 ms | 136.935 ms | CPU |

Raw evidence:

```text
target/full-quality-final/final-android-ladder-2412x1080-20260723/
```

Because the CPU preprocessing implementation changed later, this full ladder
establishes the direction and broad shape, not final current-code values at
every row. Two fresh current-code CPU/GPU/Adaptive cohorts confirm the decision
at the user's original scale and at 1M:

| source splats | forced CPU wall (2 runs) | forced GPU wall (2 runs) | Adaptive wall (2 runs) | completion interpretation |
| ---: | --- | --- | --- | --- |
| 200,000 | 4.929 / 4.933 ms | 8.422 / 8.585 ms | 5.488 / 5.434 ms | CPU completion 10.42--10.44 ms is within 16.67 ms; GPU completion 17.60--18.09 ms is just outside it; Adaptive ends `cpu_stable` (69/11) |
| 1,000,000 | 27.950 / 29.391 ms | 40.289 / 40.189 ms | 29.620 / 29.591 ms | both exceed 60 Hz completion budget; Adaptive ends `cpu_stable` (69/11) |

Both cohorts preserve every source point and SH3 at 2412x1080, and all 12
artifacts have zero failure/unsampled terminal count. Evidence:

```text
target/full-quality-final/post-parallel-android-adaptive-ladder-2412x1080-20260723/200000-v2/
target/full-quality-final/post-parallel-android-adaptive-ladder-2412x1080-20260723/1000000/
```

The sibling `.../200000/` directory is explicitly excluded: its experiment
status is `failed`, its first run remained `running`, and no complete artifact
was extracted after the collector stopped on a partial chunked summary. The
collector's completion check was fixed and the `200000-v2` rerun is the only
current 200k evidence used here. A failed or incomplete directory never counts
as a slower trial, a faster trial, or a successful qualification.

## Producer and Adaptive closeout

The earlier 69 CPU / 11 GPU table is the retained PostSort baseline, not the
final word on GPU execution plans. A clean same-binary `7cabb6e` descriptive
cohort compared exact PostSort and Preproject for complete Truck at 2412x1080:
PostSort/Preproject mean frame wall was 192.518/101.620 ms and queue completion
was 401.465/213.648 ms. Exact contributor/drawn counts matched, the rendered
Truck/background region was unchanged, and thermal status remained 0. The two
AB/BA runs have null pairing metadata, so they are not presented as a formal
self-contained paired statistic.

That same clean commit also exposed continuous-motion probe starvation:
projected learning retained the shared owner and left order Adaptive at
`cpu_learning` for 240/240 CPU frames. Commit `28f79ee` fixed the grace/yield
rules. Two fresh 120-frame runs each use 108 CPU and 12 GPU measured frames,
end `cpu_stable`, and close 140/140 order tickets without failure, fallback or
unsampled evidence. Current Adaptive therefore makes a real CPU/PostSort-GPU
choice; it still does not learn PostSort versus Preproject.

Final-code `76a9267` capacity runs also admit complete SH3 Garden (5,834,784)
and Bicycle (6,131,954) at 2412x1080 with all five exact counts and thermal 0.
Their four-frame means are 648.735 and 630.352 ms, so they prove capacity and
correctness, not interactive performance.

## Product conclusion

The current Android result answers the resolution question directly: 640x360
is not the release claim; the accepted path presents all 2.541M Truck splats at
the device's actual 2412x1080 Surface. Within the universal PostSort product
path, CPU sorting is the measured choice for this A065 from 50k through
complete Truck, including the original 200k case. Exact Preproject+Compact is
materially faster in the separate Truck diagnostic, so neither conclusion is
a universal backend or producer rule. Roughly 5.6 FPS on CPU/PostSort while
changing full-scene camera views is correct but not
a competitive interaction target. The next performance work must preserve
this exact image/count contract and should target shared GPU queue pressure,
projected-raster cost, and a device-qualified portable radix implementation
rather than reducing points or pixels.
