# Findings: Full-Quality Native Rendering

> Evidence status: implementation is complete for this branch milestone. Formal current Truck
> evidence uses true 1920x1080 on Mac/Web and native 2412x1080 on Android,
> with full terminal ledgers. All 640x360 results are explicitly historical
> codec, phase, or rapid-regression diagnostics; they are not formal visual,
> throughput, or competitor-parity claims.

## Current conclusion

The architecture and image-quality decision are now evidence-backed:

- production Packed keeps every source Gaussian addressable and preserves the
  source SH0-SH3 degree;
- position, post-sigmoid alpha, and canonical world covariance are stored
  exactly as f32;
- compact DC/SH and RGB18E8 passed the historical fixed Direct-oracle codec
  gate on both reviewed 640x360 views of full Truck, Garden, and Bicycle, and
  the current full Truck path passes the two-view 1920x1080 Direct oracle;
- CPU and GPU ordering share the same depth arithmetic and produce the same
  complete visible source-ID set on the diagnosed 5.835M Garden view;
- adapter admission uses the final resident planes and returns structured
  failure instead of paging, sampling, or lowering quality.

The retained full-Truck implementation is now measured at formal resolution on
Mac, Chrome/WebGPU, and the connected Android device. It is correct and fully
populated. The current retained Mac CPU cohort is
43.321/43.169/43.748 FPS, while the pinned PlayCanvas run observes 56.5846 FPS
on its complete 1080p Truck sequence. The latter is an architecture/throughput
reference, not a strict equal-quality denominator: the two paths differ in
depth-key width, attribute/projected-cache precision, SH-update policy, and
pixels.

Current-code complete Garden and Bicycle now load and render at 1920x1080 on
Mac Metal and Chrome/WebGPU. The Web Bicycle result is also the first large
scene in this bundle where GPU completion is decisively faster than CPU and a
long Adaptive run chooses GPU. Android's complete 50k--2M forced ladder and
fresh 200k/1M/Truck cohorts choose CPU instead. This is the evidence for a
runtime measured policy rather than a fixed point-count or API rule.

## Environment and exact input inventory

- Branch base: `main` / `origin/main` at `28f77d0`.
- Development host: Apple M4, 10 GPU cores, Metal 4, 16 GB unified memory.
- Rust: `rustc 1.93.0`, repository toolchain override active.
- Android device available to the matrix: Nothing A065 (`Pong`), Android
  15/API 35, Qualcomm/Adreno, serial `033ed212`.
- Apple runtime available at task start: iPhone 17 Pro simulator on iOS 26.2;
  no physical Apple device was initially reported by `devicectl`.
- Dataset ladder includes complete Kitsune (279,199), Flowers (562,974),
  Bonsai (1,244,819), Truck (2,541,226), deterministic Truck prefixes from 50k
  through 2M, Garden (5,834,784), and Bicycle (6,131,954).

The two largest inputs are the complete official INRIA SH3 PLYs, not samples,
crops, or regenerated subsets:

| Scene | PLY bytes | Splats | SH degree | SHA-256 |
| --- | ---: | ---: | ---: | --- |
| Garden | 1,447,027,964 | 5,834,784 | 3 | `16701d5e0630dfaca74f8794ed7ce2aa23fa922f87dc09a7e37484e8d3f82d5a` |
| Bicycle | 1,520,726,124 | 6,131,954 | 3 | `64d357cb25bd85f710f8551a18d830f8497277fbd8c5805adfd72ffe9ca78227` |

They came from the pinned INRIA archive paths
`garden/point_cloud/iteration_30000/point_cloud.ply` and
`bicycle/point_cloud/iteration_30000/point_cloud.ply`. Rights remain
`NOASSERTION`; the files and screenshots are local research/evaluation
artifacts and must not be redistributed until upstream rights are clarified.

## Why the sparse paging result was invalid

The rejected experiment installed 126,686 of Truck's 2,541,226 source splats,
only 4.985%, under a fixed 32 MiB data budget. Those pages were disjoint source
subsets, not coverage-preserving proxy Gaussians. An extrema-driven automatic
camera simultaneously framed geometric outliers, making the main subject even
smaller.

The sparse image was therefore caused by partial source publication and poor
framing. It was not evidence that Truck was intrinsically too large for a
mobile renderer. The replacement contract makes the invalid state
unrepresentable for production Packed: all five count stages and SH degree
must match before publication.

The old 20-byte `PackedSceneCpu` hot record still exists only behind the
explicit Paged diagnostic. It is not the current Packed resident
representation, is not used by the production Packed preflight, and cannot
produce a full-quality exactness receipt. A preflight result whose compatibility
path field says `PagingRequired` is a structured failure for production Packed,
not permission to install that diagnostic path.

## Final representation established by code audit

The implemented SH3 static planes are:

```text
position + alpha       16n
world covariance 0    16n
world covariance 1     8n
DC color                8n
four SH planes         64n
chunk metadata         80 * ceil(n / 256)
resolved RGB18E8        8n
draw order              4n
```

This gives exact logical formulas:

```text
SH3 GPU static             = 156n + 80 * ceil(n / 256)
SH3 CPU upload staging     = 112n + 80 * ceil(n / 256)
SH3 CPU before GPU handoff = 124n + 80 * ceil(n / 256)
CPU retained after Surface = 12n
```

| Dataset | Upload staging | Static GPU | CPU pre-handoff | Retained exact positions |
| --- | ---: | ---: | ---: | ---: |
| Truck | 285,411,472 B | 397,225,416 B | 315,906,184 B | 30,494,712 B |
| Garden | 655,319,248 B | 912,049,744 B | 725,336,656 B | 70,017,408 B |
| Bicycle | 688,695,088 B | 958,501,064 B | 762,278,536 B | 73,583,448 B |

These are payload/descriptor receipts, not process RSS. CPU staging and GPU
static data coexist during transactional handoff; allocator granularity,
driver-private storage, swapchain images, decoder scratch, CPU radix workspace,
and lazy GPU-sort scratch are additional and reported independently. The GPU
total includes the default exact projected-quads cache as two separate `16n`
planes; lazy TiledExact diagnostic resources are excluded.

## Retained raster design and current full-resolution evidence

The product draw path retains two exact optimizations:

- all Direct, Resident, and projected-cache paths use a four-vertex
  `TriangleStrip` (`BL, BR, TL, TR`) instead of the redundant six-vertex
  triangle list;
- the quad is conservatively shrunk to the `1/256` opacity iso-contour while
  the fragment cutoff remains the stricter `1/255` threshold.

Neither optimization changes source/resident/addressable count, issued
instance count, visible/drawn count, SH degree, sorted order, resolution, or
the Gaussian mapping for a surviving sample. The opacity bound removes only
outer square area that is mathematically guaranteed to fail the existing
fragment cutoff. On the Apple M4 full-Truck 1920x1080 moving trace, the
combined retained changes improved the earlier no-shrink six-vertex result
from roughly 34.045 FPS to roughly 42.070 FPS. On Nothing A065 at native
2412x1080, the corresponding CPU frame median improved from 192.953 ms to
173.138 ms in the optimization A/B. These are attribution experiments; the
separate current-code qualification below is the product result.

Complete Truck (2,541,226 source/resident/addressable splats, SH3), alternating
both official views, every-frame sort/projection, true 1920x1080, 20 warmups
and 80 measured frames produced the following original paired policy cohort:

| Mac mode | Run 1 | Run 2 | Run 3 | Measured backend mix |
| --- | ---: | ---: | ---: | --- |
| CPU | 42.464 FPS | 41.898 FPS | 41.924 FPS | 80 CPU each |
| GPU | 38.515 FPS | 38.434 FPS | 37.812 FPS | 80 GPU each |
| Adaptive | 42.313 FPS | 42.390 FPS | 41.288 FPS | 76 CPU / 4 GPU each |

Adaptive's formal metric is full `FrameCompletion`, not the sorting timestamp.
GPU key/radix timestamps remain stage diagnostics. The 76/4 split means the
short qualification ended during a bounded challenger probe; it is not a GPU
win. A later current-code CPU-only cohort after bounded parallel preprocessing
runs at 43.321/43.169/43.748 FPS (median 43.321 FPS), with preprocessing
4.54--4.97 ms and radix 4.82--5.12 ms. All three current runs present 100/100
frames and close every terminal ticket. Forced GPU/Adaptive still require a
current-binary composite-plan rerun before a new product-policy publication;
the later same-binary PostSort/Preproject cohort closes the GPU-producer
question separately. The same two Mac views pass
Direct-vs-Packed at 1920x1080:

| View | SSIM | Normalized RGB MAE | Alpha |
| --- | ---: | ---: | --- |
| 0 | 0.9999693448 | 0.0000284315 | exact |
| 1 | 0.9999705553 | 0.0000261173 | exact |

Chrome/WebGPU at 1920x1080 used the same full scene and 80-frame terminal
window: CPU 34.412 FPS (`80 / 2324.8 ms`), GPU 30.588 FPS
(`80 / 2615.4 ms`), and Adaptive 36.929 FPS (`80 / 2166.3 ms`, 68 CPU / 12
GPU). The CPU, GPU, and Adaptive screenshots are byte/pixel exact for the
qualified cameras.

Nothing A065's current native 2412x1080 cohort produced CPU frame means of
170.383613/179.335038/179.656487 ms (median 179.335038 ms = 5.576 FPS) and GPU
means of 199.016450/203.805100/209.458163 ms (median 203.805100 ms = 4.907
FPS). Three Adaptive runs were 175.663313/184.785300/183.349937 ms; every run
used 69 CPU / 11 GPU measured frames and ended `cpu_stable`. Thermal status
remained 0 and all nine artifacts have complete five-stage counts, 100
successful terminal receipts, and zero failure/unsampled measurement. The
same-camera center comparison from the image qualification is SSIM
0.9999999791, normalized RGB MAE `4.278e-8`, and alpha exact.

The pinned PlayCanvas complete-Truck 1920x1080, 600-frame terminal run is
56.5846 FPS. It therefore remains materially faster than this branch's current
moving-camera Mac and Web runs in observed throughput. It is not a strict
equal-quality comparison; the competitor evidence boundary is detailed below.
A prior stationary cache-reuse experiment that sorted/projected once is
retained only as a cache diagnostic; it is not used to claim parity with an
every-frame moving sequence.

An additional early-fragment-support guard was tested and fully reverted. The
retained baseline was 42.141/41.915 FPS, a per-fragment logarithm guard fell to
41.377/41.510 FPS, and a flat-varying support guard fell to 40.990/41.137 FPS.
The full receipt and rejection reasoning are in
`early-fragment-support-experiment.md`.

## Current full-resolution large-scene evidence

Mac Metal now admits and renders both complete largest official SH3 inputs
with Packed + ProjectedQuadsExact and forced CPU ordering. These are short
runability/capacity receipts (two warmups + four measured frames), not sustained
FPS cohorts:

| Scene | source = resident | visible = drawn views | load to benchmark | mean call wall | mean isolated completion | max RSS / peak footprint |
| --- | ---: | --- | ---: | ---: | ---: | --- |
| Garden | 5,834,784 | 4,609,628 / 4,226,208 | 4,166.562 ms | 23.058 ms | 77.644 ms | 1,631,600,640 / 2,640,021,400 B |
| Bicycle | 6,131,954 | 4,321,800 / 3,927,910 | 2,663.229 ms | 23.893 ms | 74.061 ms | 1,751,252,992 / 2,794,244,064 B |

Every scene presents 6/6 scheduled frames, sorts every frame, completes all six
terminal tickets, and has zero fallback or outstanding ticket at 1920x1080.
The desktop text schema directly prints source/resident/SH but currently omits
decoded/encoded/addressable counts; the fail-closed builder enforces those
stages, but the artifact telemetry gap remains open and is not rewritten as a
direct observation. Raw logs are under
`target/full-quality-final/mac-large-scenes-current-20260723/`.

Chrome 150/WebGPU has complete five-stage exactness manifests for the same two
scenes at true 1920x1080:

| Scene/mode | measured schedule | frame-wall mean | order completion | exact visible/drawn views |
| --- | --- | ---: | --- | --- |
| Garden CPU | 2 warmup + 4 measured | 145.300 ms | CPU 122.325 ms mean | 4,609,628 / 4,226,208 |
| Bicycle CPU | 2 + 4 | 147.450 ms | CPU 130.575 ms mean | 4,321,800 / 3,927,910 |
| Bicycle GPU | 2 + 4 | 78.825 ms | GPU 67.550 ms mean | 4,321,800 / 3,927,910 |
| Bicycle Adaptive | 20 + 80 | 105.414 ms | CPU 129.976 ms (17 frames); GPU 67.805 ms (63 frames) | 4,321,800 / 3,927,910 |

The long Bicycle Adaptive run submits and terminally completes all 80 measured
frames, chooses GPU for 63 and CPU for 17, and ends in `cpu_probe`: GPU is the
incumbent and the policy is periodically checking CPU. Across warmup and
measurement the backend totals are 69 GPU and 31 CPU. The CPU, GPU and
Adaptive final PNGs have the same SHA-256
`53de9f788f3fb884136568c61fd227d97673dad2508b78cb2823edbabe49c73a`,
and all order/adaptive failure streams are empty. This is the clearest current
counterexample to “CPU always wins” or “GPU always wins.” Raw artifacts are
under `target/full-quality-final/web-large-scenes-current-20260723/`.

## Android scale ladder and artifact integrity

The pre-parallel native 2412x1080 forced ladder contains three randomized
CPU/GPU repetitions at every exact SH3 count. Median frame-wall milliseconds
are:

| source splats | CPU | GPU |
| ---: | ---: | ---: |
| 50,000 | 1.438 | 2.683 |
| 100,000 | 2.590 | 4.495 |
| 200,000 | 4.874 | 8.514 |
| 300,000 | 7.367 | 12.254 |
| 500,000 | 12.755 | 19.894 |
| 1,000,000 | 28.199 | 40.417 |
| 1,500,000 | 62.157 | 79.846 |
| 2,000,000 | 114.996 | 136.935 |

All 48 runs are complete, thermal status is 0, and the logs contain exact
five-count receipts for their requested count. Since CPU preprocessing changed
after this ladder, it establishes the direction across the range rather than
current final timings. Fresh current-code 200k and 1M CPU/GPU/Adaptive
two-repeat cohorts preserve the same outcome: at 200k CPU frame wall is
4.929/4.933 ms and completion is 10.42--10.44 ms, while GPU frame wall is
8.422/8.585 ms and completion is 17.60--18.09 ms. At 1M CPU frame wall is
27.950/29.391 ms versus GPU 40.289/40.189 ms. Adaptive ends `cpu_stable` with
69 CPU / 11 GPU measured frames in all four runs.

Only
`target/full-quality-final/post-parallel-android-adaptive-ladder-2412x1080-20260723/200000-v2/`
is accepted as the current 200k cohort. Its sibling `200000/` has experiment
status `failed`, an unfinished first run, and no extracted artifact because an
early collector version stopped on a partial chunked summary. It is excluded
from all aggregates. This is the general evidence rule: failed, partial,
missing-terminal, mismatched-hash, or schema-invalid artifacts count as no
evidence, not as a performance sample.

The final color compute layout needs eight storage buffers per shader stage.
Its largest scene-wide plane is 16 bytes per splat, so a 128 MiB effective
binding limit admits exactly 8,388,608 splats. The boundary tests prove:

```text
8,388,608 splats -> 134,217,728-byte largest binding -> admitted
8,388,609 splats -> 134,217,744-byte largest binding -> structured failure
```

Preflight independently checks SH degree, storage-binding count,
`min(max_storage_buffer_binding_size, max_buffer_size)`, and `u32` draw
addressability. GPU geometry allocation is guarded by validation,
out-of-memory, and internal scopes. Lazy GPU-sort allocation uses the same
publish-after-success rule and has distinct initialization, validation,
out-of-memory, and internal failures. A forced GPU request never silently
falls back; only Adaptive may use CPU during a bounded cooldown.

## Why the final color codec is acceptable

Every chunk stores exact geometry, DC minimum/extent, and per-band/per-channel
SH maxima. Every point stores signed 11-bit SH coefficients plus a 5-bit
amplitude ratio for each active SH band. Upward rounding of that ratio keeps a
non-zero band non-zero and prevents its largest coefficient from clipping.
This fixes the chunk-outlier error without another plane or binding. A
256-point chunk remains the selected layout because it passes the fixed image
gate while amortizing the 80-byte metadata record.

The resolved view-dependent color is RGB18E8. SH evaluation is one coherent GPU
pass for the current camera position; there is no multi-frame color sweep and
no mixture of colors evaluated from different camera positions.

The following gate was fixed before accepting the codec and was not relaxed
for a difficult scene:

```text
SSIM (8x8 luma, sRGB)                     >= 0.9999
RGB mean absolute error, normalized       <= 0.00005
fraction of pixels with RGB error > 3/255 <= 0.001
alpha                                     exact
```

## Historical 640x360 codec-oracle evidence

These six comparisons use 640x360 fixed cameras, complete SH3 source data,
complete sorted membership, and the same blend path. They were valuable for
selecting the resident codec and checking the two largest available inputs,
but 640x360 is too small to serve as final visual-quality, product-throughput,
or competitor-parity evidence. `Visible = drawn` is listed to distinguish
legitimate view rejection from source loss.

| Scene/view | Source = resident | Visible = drawn | SSIM | RGB normalized MAE | Pixels over 3/255 | Alpha | Gate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Truck/0 | 2,541,226 | 1,886,298 | 0.9999831033 | 0.0000262346 | 0.0000086806 | exact | pass |
| Truck/1 | 2,541,226 | 1,672,013 | 0.9999819463 | 0.0000257750 | 0 | exact | pass |
| Garden/0 | 5,834,784 | 4,609,628 | 0.9999927735 | 0.0000271196 | 0.0000130208 | exact | pass |
| Garden/1 | 5,834,784 | 4,226,208 | 0.9999901335 | 0.0000314656 | 0 | exact | pass |
| Bicycle/0 | 6,131,954 | 4,321,800 | 0.9999885568 | 0.0000292643 | 0.0000043403 | exact | pass |
| Bicycle/1 | 6,131,954 | 3,927,910 | 0.9999897978 | 0.0000301493 | 0 | exact | pass |

The ignored local artifacts are content-addressed below. A changed renderer,
trace, comparator, or image must regenerate the receipt instead of reusing the
old result.

| Scene/view | Direct PNG SHA-256 | Resident PNG SHA-256 | Metrics SHA-256 |
| --- | --- | --- | --- |
| Truck/0 | `59c115b26168ffa6946f066e00225ca4c6f666bc77f8099514e58d8fa82ca2e0` | `c16573aa6ac4d36bfb041abe82fb0fec9421045165d487bbbe74e70a9488df1a` | `f16c26f0f9958c7469ed5b9b0f9ee5378b30715ab8f724b4dbc069c360dd14ff` |
| Truck/1 | `e584c22088c79de8bcb9031b2d5a44379ff59e6ec2632c6eb8e5de5672840e79` | `cd3986c5567046dfd42c71310c35557cb43aa808f012cf26df311fe95971924f` | `7ffc7ac23f19cbc5642c28446eb74ace4115ef78280995b55eb89c04eecb1a85` |
| Garden/0 | `fd8ea5a134c1908d96f7ad69784fd4a76f5302215cb606398adc34510dca0b89` | `b5edbf0c64ac80c7c3152b3c673f65d676af9aaa113c9d7f08aaf153b602831c` | `7493208160ba593d43351f64538943d892ecdff19b22d09ecf6e6f9943228946` |
| Garden/1 | `1311d5797051357713935d2bce7b3f98ddca53a9fd994dd7b55a7efd1ac5b1ec` | `f234c17d088f4f796f92147f3d7a86f68b943471c6a0107e8e6407e7909feaf1` | `8ef52cc6f7faa8866fee309bccdc069ac7d803fae5c8055ff7d3f2a3d1109095` |
| Bicycle/0 | `22b035227e2ec63639c1c36cbaa7e1721d26a16815abf25095262bbe7f8dccb0` | `0f77f31b70a73ad0e7c30734e7b76f01648f4f94fcbf92d4325f9190dd8f10de` | `ab4a58c55eefb19259636b84917b51da0237add6d5b10c54aa245b747610c1b6` |
| Bicycle/1 | `bb917a86a31438858d0327fb13e265bbcd33a16571cbe60cea8295125f26c65a` | `674e64b4f2a236a2b1b11d768c2f7df1bf7f69ca99c4fd4a1a9bfa36f46e9062` | `d2e035b20271d6be4c8bc3ea63d841b15de5cf20c74e13be213a54191914db37` |

Truck was rendered repeatedly with identical Direct and Resident hashes. The
full Garden/Bicycle pairs extend the historical codec decision to the two
largest available scenes rather than extrapolating from Flowers or Kitsune;
they are not presented as current-code full-resolution Garden/Bicycle product
qualification.

## CPU/GPU correctness finding

One Garden view originally differed by a single source ID at the near plane.
The source position and camera caused independent multiply/add rounding on CPU
to fall just below `near`, while fused arithmetic on GPU fell just above it.
Both implementations now use the explicit sequence:

```text
x_product = row.x * relative.x
xy        = fma(row.y, relative.y, x_product)
depth     = fma(row.z, relative.z, xy)
```

The complete 5,834,784-point regression reads every GPU pair and indirect
count, computes the CPU visible set, and compares source membership. Its final
receipt is:

```text
trace_sha256=9dd7c8abc4ccfd74f54ae863df2123f3ff28817b4962048a02cafb4bf99a08ec
source_count=5834784
cpu_visible=4226208
gpu_visible=4226208
differing_source_ids=0
```

This is a correctness fix, not an epsilon or a point budget. Stable source-ID
tie semantics remain unchanged.

## Loading and publication findings

- File-backed Packed PLY loading reads a validated summary, then sends one
  fixed-size decoded splat at a time directly to `ResidentSceneBuilder`; it
  does not construct temporary wide render attributes.
- Browser Packed loading consumes URL responses, `File.stream()`, or a custom
  `ReadableStream`, hashes the transported bytes, and feeds the incremental
  decoder directly into the same resident builder.
- The builder reserves checked final planes, retains at most one 256-splat
  source chunk after construction has started, and requires the declared count
  exactly at `finish()`.
- Any declared SH rest payload must be unique, contiguous, and exactly one of
  the SH1/SH2/SH3 sizes. Malformed rest properties are no longer interpreted as
  SH0.
- The current SPZ v4 loader is bounded and cancellable, but still constructs
  wide `SceneBuffers` before owned Resident conversion. It does not yet have
  the PLY visitor's CPU-peak advantage.
- Surface upload staging is released only after complete GPU resource
  validation. Offscreen keeps it intentionally for path recreation.

## Competitor audit from primary sources

The useful competitor techniques are compact scene ownership, GPU-visible work
generation, scalable radix sorting, and tiled raster work. Their public paths
do not establish the same combined contract of full source membership,
unchanged SH degree, portable native/Web/mobile execution, and a runtime
measured CPU/GPU choice.

The pinned PlayCanvas Truck artifact itself is valid for what it records: all
2,541,226 source splats and SH3 are active, LOD/dynamic resolution/upscaling are
disabled, 600 submissions reach a terminal queue drain in 10,603.6 ms, and
observed throughput is 56.5846 FPS. It is nevertheless **not strict
equal-quality evidence** against this renderer:

- its [hybrid renderer chooses 10--20 sort-key bits](https://github.com/playcanvas/engine/blob/12e983b5441d05ca008a188d224a69e0c4ced389/src/scene/gsplat-unified/gsplat-hybrid-renderer.js#L417-L499); 2.541M splats select 20 bits/five nibble passes, while gsplat-rs retains all 32 bits and stable source-ID ties;
- its pinned [compact work-buffer declaration](https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/gsplat-unified/gsplat-params.js#L86-L101) is `R32U + RGBA32U + R32U` (24 bytes per splat) and quantizes color, rotation, scale, and opacity rather than matching this branch's exact f32 geometry/covariance contract;
- its pinned [projector cache](https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/shader-lib/wgsl/chunks/gsplat/compute-gsplat-projector.js) packs the two projected axes and RGBA into fp16 values (8-bit RGBA in the stereo case), whereas the gsplat-rs quality path retains f32 projected cache planes;
- its default [`colorUpdateAngle = 10`](https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/gsplat-unified/gsplat-params.js#L208-L230) feeds the [distance/angle-derived SH color update threshold](https://github.com/playcanvas/engine/blob/d5fe88878e338936fe763bbce1a58bc315e89cbe/src/scene/gsplat-unified/gsplat-world.js#L489-L530), so the path may reuse resolved SH color; the benchmark does not record actual color-update work per frame; and
- the artifact's count source is `totalActiveSplats`, not the projector's actual per-frame `renderCounter`, so it proves full active residency but does not directly receipt projected/drawn contributor count.

The recorded same-camera frame-0 comparison is SSIM `0.9381751521`, RGB MAE
`0.0282230777`, 65.01% of pixels above 3/255 RGB error, and exact alpha:
`target/full-quality-final/playcanvas-vs-gsplat-truck-view0-1920x1080-20260723.json`.
This does not prove which renderer looks better; it proves that the images are
not pixel-equivalent. The 56.5846 FPS run is therefore an architecture ceiling
signal and competitive product reference, not a same-numerics parity score.

| Project | Primary-source behavior | What is reusable here | Difference from this contract |
| --- | --- | --- | --- |
| PlayCanvas / SuperSplat | [Streaming](https://github.com/playcanvas/developer-site/blob/ae1b39a801cd8936bee9ea513e80cf94bf0a1628/docs/user-manual/supersplat/streaming.md#L6-L28) uses a hierarchy and view-dependent LOD. [SOG](https://github.com/playcanvas/developer-site/blob/ae1b39a801cd8936bee9ea513e80cf94bf0a1628/docs/user-manual/gaussian-splatting/formats/sog.md#L7-L32) is deliberately lossy; positions, rotations, and SH use compact quantized/palette forms ([position](https://github.com/playcanvas/developer-site/blob/ae1b39a801cd8936bee9ea513e80cf94bf0a1628/docs/user-manual/gaussian-splatting/formats/sog.md#L128-L153), [rotation](https://github.com/playcanvas/developer-site/blob/ae1b39a801cd8936bee9ea513e80cf94bf0a1628/docs/user-manual/gaussian-splatting/formats/sog.md#L155-L190), [SH](https://github.com/playcanvas/developer-site/blob/ae1b39a801cd8936bee9ea513e80cf94bf0a1628/docs/user-manual/gaussian-splatting/formats/sog.md#L231-L297)). Its [unified renderer](https://github.com/playcanvas/developer-site/blob/ae1b39a801cd8936bee9ea513e80cf94bf0a1628/docs/user-manual/gaussian-splatting/rendering-architecture/index.md#L6-L63) builds visible work and sorts it. | Planar compact data, visible-work compaction, hierarchical scan/radix, and coherent GPU work generation. | Streaming LOD does not render all source points in each view. Its [AUTO policy](https://github.com/playcanvas/engine/blob/12e983b5441d05ca008a188d224a69e0c4ced389/src/scene/gsplat-unified/gsplat-params.js#L159-L183) selects GPU on WebGPU and CPU on WebGL by backend, rather than learning the current device/scene crossover. |
| PlayCanvas GPU sorter | [Hybrid rendering](https://github.com/playcanvas/engine/blob/12e983b5441d05ca008a188d224a69e0c4ced389/src/scene/gsplat-unified/gsplat-hybrid-renderer.js#L417-L499) performs cull/project/sort work on GPU and chooses only 10--20 depth-key bits from scene size; its portable radix uses 4-bit passes and one combined key/value reorder. Its radix implementation explicitly [avoids a OneSweep-style dependency](https://github.com/playcanvas/engine/blob/12e983b5441d05ca008a188d224a69e0c4ced389/src/scene/gsplat-unified/compute-radix-sort.js#L74-L119). | Keep cull/project/stable contributor compaction, the portable multi-pass design, and fused key/ID scatter where the already-required binding count permits it. | This project retains all 32 IEEE depth-key bits and stable source-ID ties under its exact contract, so it cannot claim PlayCanvas's five-pass maximum by silently adopting a 20-bit order. PlayCanvas also does not provide this project's measured ABBA policy, hysteresis, cooldown, and periodic CPU/GPU re-probe. |
| WebSplatter | The 2026 paper specifies a portable, wait-free [four-pass 8-bit radix over the full 32-bit depth key](https://arxiv.org/html/2602.03207#S3.SS3.SSS2), with workgroup-local histograms, a multi-dispatch hierarchical Blelloch scan, and stable scatter. It evaluates Apple, Android-class Qualcomm devices, desktop GPUs, and multiple browsers, and also reduces fragment work with screen-space/opacity-aware bounds. | Its full-width radix contract and no-cross-workgroup-wait structure directly validate this branch's intended GPU-order architecture. The local rank/scatter implementation should use explicit, deterministic workgroup state and larger per-group tiles, then be qualified independently on Metal and Adreno. Opacity-aware quad bounds are useful only if they pass the unchanged Direct image gate. | The publication does not provide this repository's native Rust/wgpu ABI, exact Resident codec, CPU-order backend, or measured runtime CPU/GPU selector. Its opacity/contribution culling cannot be assumed quality-neutral here without source-ID, alpha, and image receipts. |
| UnityGaussianSplatting | The README publishes a full [6.1M Bicycle](https://github.com/aras-p/UnityGaussianSplatting/blob/2c6fed37da67a217367261fcfcd3316d34c73e76/README.md#L77-L89) desktop case and its [platform scope](https://github.com/aras-p/UnityGaussianSplatting/blob/2c6fed37da67a217367261fcfcd3316d34c73e76/README.md#L14-L30). Assets expose multiple [compression layouts](https://github.com/aras-p/UnityGaussianSplatting/blob/2c6fed37da67a217367261fcfcd3316d34c73e76/package/Runtime/GaussianSplatAsset.cs#L31-L101) and [presets](https://github.com/aras-p/UnityGaussianSplatting/blob/2c6fed37da67a217367261fcfcd3316d34c73e76/package/Editor/GaussianSplatAssetCreator.cs#L189-L224); rendering uses [32-bit depth keys](https://github.com/aras-p/UnityGaussianSplatting/blob/2c6fed37da67a217367261fcfcd3316d34c73e76/package/Shaders/SplatUtilities.compute#L51-L81) and [GPU radix sorting](https://github.com/aras-p/UnityGaussianSplatting/blob/2c6fed37da67a217367261fcfcd3316d34c73e76/package/Runtime/GpuSorting.cs#L142-L198). | Full Bicycle is a minimum large-scene proof; 32-bit keys and GPU radix are appropriate. | The published scope is a desktop Unity implementation, not one Rust/wgpu path proven on WebGPU, Android, and Apple runtimes, and it does not expose a measured CPU/GPU adaptive policy. |
| INRIA reference rasterizer | The original project is a [CUDA renderer](https://github.com/graphdeco-inria/gaussian-splatting/blob/54c035f7834b564019656c3e3fcc3646292f727d/README.md#L301-L312). It duplicates work into [tile/depth keys](https://github.com/graphdeco-inria/diff-gaussian-rasterization/blob/9c5c2028f6fbee2be239bc4c9421ff894fe4fbe0/cuda_rasterizer/rasterizer_impl.cu#L52-L111), sorts and builds [tile ranges](https://github.com/graphdeco-inria/diff-gaussian-rasterization/blob/9c5c2028f6fbee2be239bc4c9421ff894fe4fbe0/cuda_rasterizer/rasterizer_impl.cu#L278-L340), then blends with the reference [forward formulation](https://github.com/graphdeco-inria/diff-gaussian-rasterization/blob/9c5c2028f6fbee2be239bc4c9421ff894fe4fbe0/cuda_rasterizer/forward.cu#L180-L268). | Tile binning and range construction are credible future raster-load optimizations after exact residency/order is stable. | CUDA-specific execution is not the shared Metal/Vulkan/WebGPU implementation required here; adopting its renderer wholesale would also discard the CPU/GPU ordering feature. |
| Lightweight Web viewers | GaussianSplats3D optionally uses [half-precision covariance](https://github.com/mkkellogg/GaussianSplats3D/blob/eb2fc4593e3ea5e75388296fcdde2459542d1290/src/splatmesh/SplatMesh.js#L643-L706) and separate [SH texture planes](https://github.com/mkkellogg/GaussianSplats3D/blob/eb2fc4593e3ea5e75388296fcdde2459542d1290/src/splatmesh/SplatMesh.js#L788-L867). The original antimatter15 viewer documents its [compact conversion/runtime](https://github.com/antimatter15/splat/blob/ba182b51b7c2ad5738cdd6741cd63336d27470fb/README.md#L76-L107), while gsplat.js explicitly notes that its [PLY conversion loses SH](https://github.com/huggingface/gsplat.js/blob/7133e2b49bff4392ec0d507bd508aaae5305ccae/README.md#L76-L92). | Texture/plane decomposition demonstrates ways around monolithic buffers. | Dropping SH or choosing a lower-precision attribute path without the fixed Direct-oracle gate is outside the requested quality contract. |

The audit therefore supports a hybrid strategy: keep a complete compact
resident scene, generate exact visible work, use scalable GPU radix when it
wins, retain CPU radix when it wins or GPU capability is absent, and let
measured runtime evidence choose. LOD may remain a separately labeled future
product mode, but it cannot masquerade as full quality.

## The 128 MiB limit is not a wgpu-only rule

The implementation must use negotiated device values rather than infer a
platform from the API name:

- WebGPU defines a 128 MiB default `maxStorageBufferBindingSize` limit in its
  [limits table](https://github.com/gpuweb/gpuweb/blob/99d2ded3335433260fd756abacc2d2b280999b8d/spec/index.bs#L1738-L1784).
- Vulkan requires at least 128 MiB for `maxStorageBufferRange` in its
  [device-limit requirements](https://github.com/KhronosGroup/Vulkan-Docs/blob/d184375dcc5da2b06ca375a8d7d1f9d21ca64a76/chapters/limits.adoc#L6758-L6778).
- OpenGL ES 3.1 specifies the corresponding shader-storage block limit in its
  [implementation limits](https://registry.khronos.org/OpenGL/specs/es/3.1/es_spec_3.1.pdf#page=423).
- Metal exposes the device-specific
  [`maxBufferLength`](https://developer.apple.com/documentation/metal/mtldevice/2966563-maxbufferlength).

These are per-binding/resource limits, not total-memory promises. Splitting
attributes into bounded planes solves the monolithic-binding failure but does
not manufacture physical memory. That is why preflight checks negotiated
limits and actual allocation remains scoped and fallible.

## Historical 640x360 diagnostic cameras and reproducibility

Five two-view, 640x360 quality traces are checked in under
`tests/perf/trace/fixtures/quality/`. Official training-camera entries are used
where available; robust bounds set conservative clipping and never let global
position extrema choose composition. These fixtures remain fast codec and
depth-parity diagnostics only. Formal visual and throughput evidence uses
1920x1080 on Mac/Web, native 2412x1080 on Nothing A065, and 2622x1206 on the
available iOS simulator.

| Scene | Fixture | Trace SHA-256 |
| --- | --- | --- |
| Flowers | `candidate-flowers-quality-640x360-v1.json` | `9cfbe9632c143c0b5a96eeacd14e035b9c334240d63487ee063f2dabbaf8e5a4` |
| Bonsai | `candidate-bonsai-quality-640x360-v1.json` | `1a29761609fe70c36cb144640767392c59f33a1023b5320c2e4cc96d902871ce` |
| Truck | `candidate-truck-quality-640x360-v1.json` | `ac2f5dbe71c02f16c8876185b61686b6f7e3053e7662c5059e6aa9fc2378c02e` |
| Garden | `candidate-garden-quality-640x360-v1.json` | `9dd7c8abc4ccfd74f54ae863df2123f3ff28817b4962048a02cafb4bf99a08ec` |
| Bicycle | `candidate-bicycle-quality-640x360-v1.json` | `aa678d2cece7ecdaea055ddf3d16b8336314e0b22801c7ba308bbf3ca2902bc5` |

Representative reproduction:

```bash
python3 tests/datasets/fetch_inria_3dgs_scenes.py \
  --scenes bonsai truck garden bicycle --include-cameras \
  --acknowledge-local-use-only
bash tests/perf/trace/test-trace-v1.sh

cargo run --release -p desktop-example -- \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path direct \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-640x360-v1.json \
  --camera-frame 0 --png target/truck-view0-direct.png

cargo run --release -p desktop-example -- \
  tests/datasets/external/inria_3dgs/truck/point_cloud.ply \
  --geometry-path packed \
  --camera-trace tests/perf/trace/fixtures/quality/candidate-truck-quality-640x360-v1.json \
  --camera-frame 0 --png target/truck-view0-resident.png

node tests/perf/compare-image-ssim.mjs \
  target/truck-view0-direct.png target/truck-view0-resident.png \
  --threshold 0.9999 \
  --max-rgb-mae-normalized 0.00005 \
  --max-pixels-over3-fraction 0.001 \
  --require-alpha-exact \
  --output target/truck-view0-metrics.json
```

## Phase 2 pre-project contributor direction

The retained exact contributor pass currently sorts `V` before it projects and
compacts to `C`; on the two formal Truck 1080p views, only 52.85% and 57.55%
of `V` reaches `C = D`. Post-sort compaction is image exact, but its current
Mac/Web/Android throughput signals are mixed and the older comparisons are not
same-binary paired causal estimates.

The next candidate therefore constructs exact, source-stable `C` before GPU
radix, sorts all 32 key bits over `C`, and keeps hardware SortedAlpha drawing,
the unchanged CPU path, and measured Adaptive selection. The architecture,
primary-source audit, causal boundaries, target matrix, and finite experiment
slices are in [phase2-preproject-c-architecture.md](phase2-preproject-c-architecture.md).
Its performance bands guide retain/target-gate/reject decisions; they are not
task-completion barriers or permission to lower quality.

## Performance evidence boundary

The current full-resolution runs above are accepted evidence only for the
tested host/device, scene, cameras, resolution, raster plan, binary, and timing
window. They establish that 640x360 is unnecessary for runability, CPU is the
better current choice on the tested Mac/Android workloads, and GPU is the
better completion choice for 6.132M Bicycle in the tested Chrome/WebGPU
workload. They do not define a universal point-count crossover. The current
PlayCanvas comparison shows an observed throughput gap, while its different
numeric/image contract prevents a strict equal-quality parity ratio.

The cross-platform matrix closes with representative ladder anchors rather
than a wasteful endpoint-by-scene Cartesian product. Terminal additions are:

- same-binary exact PostSort/Preproject cohorts on M4, Chrome/WebGPU and A065;
- fixed continuous-motion A065 Adaptive evidence that leaves `cpu_learning`;
- complete Garden/Bicycle A065 capacity receipts through 6.132M SH3 splats.

Deferred work is explicit: a composite producer/draw-plan controller,
sustained Garden/Bicycle thermal cohorts, desktop text-receipt expansion,
SPZ duplicate-peak removal, and a physical-iPhone run when hardware/signing is
available. These are not permission to publish sampling, reduced SH, reduced
resolution or incomplete residency as full-quality output.

Older 640x360 CPU/GPU tables may still locate phases or reproduce a depth
boundary, but cannot replace the formal-resolution terminal evidence.
