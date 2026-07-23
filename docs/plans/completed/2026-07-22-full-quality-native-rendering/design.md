# Design: Full-Count Resident SortedAlpha

## Decision and scope

`PackedAtlas` is the production-facing compact geometry selection, while its
implementation is an exact-count resident scene. `SortedIndexDirect` remains
the wide-float compatibility path and image oracle. `PagedActiveAtlas` remains
an explicitly selected diagnostic; it is never an automatic capacity fallback
and cannot claim the full-quality contract below.

The contract is stronger than "the file opened":

```text
source_count == decoded_count == encoded_count
             == resident_count == addressable_count
source_sh_degree == resident_sh_degree, where degree is 0, 1, 2, or 3
source_membership == all
sampling == disabled
lod == disabled
partial_scene_published == false
```

Frustum and near/far rejection may remove mathematically invisible splats from
one frame's sort and draw work. It never removes them from the resident scene.
For a frame, `drawn_count == visible_count`; over the scene, every source ID
remains addressable. A device that cannot admit the complete requested scene
receives a structured error before a new scene or GPU-order backend is
published. The renderer does not sample, lower SH degree, install an incomplete
page set, or silently change geometry path.

## End-to-end flow

The complete resident flow is:

```text
validated PLY summary
  -> checked final-plane reservations
  -> one-splat-at-a-time decode
  -> 256-splat transactional encoder chunks
  -> exact-count ResidentSceneCpu
  -> adapter-limit preflight
  -> scoped GPU allocation and upload
  -> release Surface upload staging only after success
  -> coherent all-point SH color resolve for a camera position
  -> CPU, GPU, or measured Adaptive depth ordering
  -> one shared four-vertex TriangleStrip SortedAlpha draw path
```

Only the order producer changes between CPU and GPU. Geometry, covariance,
opacity, SH reconstruction, visible-set predicate, blending, and source-ID tie
semantics remain identical.

## Final resident representation

Points stay in source order. Consecutive groups of 256 points share only color
quantization metadata; chunking changes precision and metadata overhead, never
membership or order identity.

| Plane | Final representation | Bytes/splat |
| --- | --- | ---: |
| Position + alpha | exact `vec4<f32>` | 16 |
| World covariance 0 | exact `xx, xy, xz, yy` as `vec4<f32>` | 16 |
| World covariance 1 | exact `yz, zz` as `vec2<f32>` | 8 |
| DC color | chunk-local `u16 x 3` in two `u32` words | 8 |
| Each active SH plane | four `u32` words | 16 |
| Resolved color | shared-exponent RGB18E8 in two `u32` words | 8 |
| Draw order | source/resident ID as `u32` | 4 |
| Chunk metadata | five `vec4<f32>` per 256 splats | 80/chunk |

The canonical six world-covariance terms are calculated once with the same f32
source transform as Direct and then stored exactly. Rasterization does not need
to reconstruct geometry from a lower-precision parameterization. Position,
post-sigmoid alpha, and covariance therefore report zero encoding error.

DC uses a per-chunk, per-channel minimum and extent. Each component is encoded
to 16 bits. The two unused bytes in the second word are reserved.

SH degree controls the number of active 16-byte planes:

| SH degree | Rest coefficients per point | Packed payload bits | Active planes |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 0 |
| 1 | 9 | `9 * 11 + 5 = 104` | 1 |
| 2 | 24 | `24 * 11 + 10 = 274` | 3 |
| 3 | 45 | `45 * 11 + 15 = 510` | 4 |

Every active coefficient is a signed 11-bit value in coefficient-major RGB
order. Chunk metadata stores one absolute maximum for every `(SH band,
channel)`. Each point also stores one unsigned 5-bit amplitude ratio for every
active band, shared by that point's channels and coefficients in the band. The
ratio is rounded upward, so a non-zero band never becomes zero and the point's
largest coefficient cannot clip. The coefficient is then quantized against
`chunk_band_channel_scale * point_band_ratio`. This isolates a chunk outlier
without another buffer, binding, or byte.

For SH3, the 45 coefficients occupy bits 0 through 494, the L1/L2/L3 point
ratios occupy bits 495 through 509, and bits 510 through 511 are reserved.
Lower degrees place their active ratios immediately after their last
coefficient. `ResidentEncodingReport` records maximum DC and per-band SH
reconstruction error as well as the exact-count receipt.

The color compute pass evaluates DC plus every retained SH band coherently for
all points when camera position changes. Pure rotation may reuse the result
because view-dependent color depends on point-to-camera direction, not camera
orientation. RGB18E8 stores one 8-bit binary exponent shared by three unsigned
18-bit mantissas in 64 bits. It preserves the reference SH rule `max(rgb, 0)`
without silently clipping highlights to 1, while retaining materially more
precision than three binary16 channels at the fixed image gate. The draw pass
reads the exact covariance and RGB18E8 result. Direct, Resident, and
projected-cache rendering all issue one canonical four-vertex triangle strip
(`BL, BR, TL, TR`) per visible instance. The strip still covers the same two
triangles as the former six-vertex triangle list, but removes two redundant
vertex invocations without changing instance count, order, Gaussian
coordinates, or the SortedAlpha blend contract.

The quad extent is also bounded by the conservative `1/256` opacity
iso-contour:

```text
extent_scale = sqrt(clamp(ln(max(alpha, 1e-12) * 256) / 4.5, 0, 1))
```

The fragment contract discards only when evaluated alpha is strictly below
`1/255`. Because `1/256 < 1/255`, shrinking the square to this slightly wider
support retains every sample that can contribute, including the equality
boundary. It changes neither the issued splat count nor source membership; it
only avoids rasterizing an outer region that is mathematically guaranteed to
be discarded. The scale and local Gaussian coordinate are changed together,
so the pixel-to-Gaussian map inside the retained support is unchanged.

## Bindings and adapter admission

The color-resolve compute layout uses exactly eight storage buffers plus one
uniform:

1. position + alpha;
2. DC color;
3. SH plane 0;
4. SH plane 1;
5. SH plane 2;
6. SH plane 3;
7. chunk metadata;
8. resolved RGB18E8 output.

Inactive SH planes use minimum non-zero descriptors so the pipeline layout is
stable across SH0-SH3. `GlobalQuads` uses five draw storage buffers (order,
position + alpha, covariance 0, covariance 1, resolved color) plus one uniform.
The default `ProjectedQuadsExact` projection pass uses seven storage buffers
plus one uniform and writes two independent `16n` rank-indexed planes; its draw
then reads those two planes and resolved color. Keeping the two cache planes
separate is what preserves the same 128 MiB per-binding boundary.

The two planes are durable exact cache state, not transient per-frame scratch.
They are reusable only for the same ordering owner (CPU or GPU), order
generation, complete camera, viewport, and count guard. An order refresh,
backend transition, camera change, resize, or count change invalidates the
cache. This retains one-projection-per-visible-splat behavior on moving frames
and removes identical projection work on stationary frames without changing
membership, order, SH, blend math, or resolution.

Let:

```text
n = source splat count
c = ceil(n / 256)
p = active SH plane count: 0, 1, 3, or 4
e = min(max_storage_buffer_binding_size, max_buffer_size)
```

The largest resident binding is `max(16 * n, 80 * c)` bytes for non-empty
scenes; for practical scene sizes it is `16 * n`. Admission requires:

```text
SH degree <= 3
max_storage_buffers_per_shader_stage >= 8
largest resident binding <= e
n <= u32::MAX
```

`packed_scene_preflight_with_limits` reports every independently allocated
plane, both raw adapter limits, the effective limit, binding-count availability,
draw-addressability, and one of the structured failure classes:
unsupported SH degree, insufficient storage-binding count, storage-binding
size, or draw-instance count. The compatibility enum value `PagingRequired`
means only "the requested complete Packed scene does not fit"; production code
returns the preflight failure and does not enter Paged.

At an effective 128 MiB binding limit, the exact non-empty boundary is
8,388,608 splats. `16 * 8,388,608 == 134,217,728` fits; one additional splat
requires 134,217,744 bytes and fails. Garden and Bicycle are below this
per-binding boundary. Total memory can still fail independently, so GPU
resource construction runs under validation, out-of-memory, and internal error
scopes. Handles and Surface upload staging are committed only after all scopes
succeed.

Lazy GPU-order construction has the same transactional rule. Forced GPU mode
returns a structured initialization, validation, out-of-memory, internal, or
unsupported error. Only Adaptive mode may recover through CPU sorting; a
failed attempt leaves no partially initialized GPU-order object.

## Exact payload accounting

These formulas are logical payload bytes counted by the resident byte plans.
They intentionally exclude allocator granularity, driver-private memory,
pipelines, swapchain textures, telemetry readback, source transport buffers,
and optional sort workspaces.

For any degree:

```text
GPU active static payload = (92 + 16p) * n + 80c
CPU upload staging        = (48 + 16p) * n + 80c
CPU exact positions       = 12n
CPU before Surface handoff= (60 + 16p) * n + 80c
CPU after Surface handoff = 12n
```

For a non-empty SH0-SH2 scene, the stable four-plane bind-group layout also
creates one 16-byte placeholder for each inactive SH plane. Thus the sum of
actual GPU buffer descriptor sizes adds `16 * (4 - p)` to the active static
payload. SH3 activates all four planes and needs no placeholder adjustment.

For SH3 (`p = 4`):

```text
GPU resident static       = 156n + 80c
CPU upload staging        = 112n + 80c
CPU before Surface handoff= 124n + 80c
```

| Dataset | Points | SH3 upload staging | SH3 static GPU | CPU pre-handoff | Exact CPU positions retained |
| --- | ---: | ---: | ---: | ---: | ---: |
| Truck | 2,541,226 | 285,411,472 B | 397,225,416 B | 315,906,184 B | 30,494,712 B |
| Garden | 5,834,784 | 655,319,248 B | 912,049,744 B | 725,336,656 B | 70,017,408 B |
| Bicycle | 6,131,954 | 688,695,088 B | 958,501,064 B | 762,278,536 B | 73,583,448 B |

During a Surface handoff, CPU pre-handoff payload and static GPU payload coexist
until upload validation succeeds: 713,131,600 B for Truck, 1,637,386,400 B for
Garden, and 1,720,779,600 B for Bicycle before decoder, driver, sort, and
swapchain overhead. Surface then drops upload staging transactionally.
Offscreen rendering deliberately retains it because its public geometry-path
switching can destroy and recreate resident GPU resources; the two ownership
modes must not be combined into one misleading memory receipt. `TiledExact`
allocations are lazy diagnostic resources and are not included above.

GPU sort scratch is lazy and separate from static residency. For the resident
SoA sorter, let `C = max(n, 1)`, `g = max(ceil(n / 1024), 1)`, `P = 16g`, and
let `q0 = max(ceil(P / 512), 1)`, continuing
`q(i+1) = max(ceil(qi / 512), 1)` until a level reaches 1. If `L` is the level
count, `Q` the sum of all `qi`, and
`A = max(min_uniform_buffer_offset_alignment, 16)`, the allocated payload is:

```text
GPU sort scratch = 16C + 4P + 4Q + 8A + LA + 16
```

This covers two key arrays, two ID arrays, radix prefix, hierarchical scan
sums, radix/scan uniforms, and indirect arguments; the resident path does not
allocate the legacy AoS compatibility-pair output. CPU synchronous sorting has
two visible `u32` arrays plus two `u64` radix arrays and 256 counters as its
active logical workspace (`24v + 256 * sizeof(usize)` for `v` visible splats),
with vector capacity and native async-worker overlap reported separately.

## Complete CPU and GPU ordering

Both backends classify depth with the same explicit f32 sequence:

```text
x_product = row.x * relative.x
xy        = fma(row.y, relative.y, x_product)
depth     = fma(row.z, relative.z, xy)
visible   = near <= depth && depth <= far
```

CPU sorting uses a stable four-pass, 8-bit radix over all 32 depth-key bits.
Source IDs start in ascending order, so equal-depth ties retain source order.

GPU sorting generates exact visibility, depth keys, and source IDs on the GPU.
The portable baseline performs eight stable 4-bit LSD passes; the separately
qualified native-macOS Resident path performs four stable 8-bit passes over the
same full 32-bit key:

```text
key/visibility generation
  -> per-tile radix histogram
  -> hierarchical exclusive scan
  -> stable scatter of keys and IDs
  -> repeat for all eight nibbles, or four bytes on the qualified path
  -> indirect instance count and draw
```

It needs no subgroup feature, global spin loop, single-workgroup global prefix,
or atomic output-position allocation. Two-dimensional dispatch decomposition
handles scenes that exceed a one-dimensional workgroup count. CPU and GPU
therefore emit the same visible source-ID set and stable order semantics.
Radix-8 is an explicit native-macOS allowlist, not a platform assumption:
Adreno full-scene image qualification exposed corruption even though counts
looked plausible, so Android, Web, iOS, Windows, and Linux remain on the exact
base-16 path until per-stage full-ID readback independently qualifies them.
Resident SoA devices already satisfy the Packed path's eight-storage-binding
contract, so each pass uses one five-storage fused scatter that writes the key
and source ID together. The four-binding Direct compatibility path retains two
payload scatters. This removes eight full-capacity dispatches per refresh
on the base-16 path without reducing the 32-bit key, stability, or visible
membership.

## Runtime adaptive policy

The renderer exposes explicit CPU, explicit GPU, and Adaptive choices. Adaptive
does not hard-code a crossover point by point count. Its production metric is
`FrameCompletion`: both CPU-ordered and GPU-ordered probe frames are measured
from the sampled frame start through completion of that frame's submitted GPU
work. This intentionally includes the projection/raster queue pressure that
determines what the user experiences; changing raster plans resets learning
because samples from different plans are not comparable.

The policy:

- begin with six CPU bootstrap samples;
- wait four order refreshes, warm the GPU path, then run four ABBA blocks,
  producing eight samples per backend;
- compare p75 timing with hysteresis: GPU must be below 88% of the CPU value to
  replace CPU, while CPU must be below 92% of the GPU value to replace GPU;
- re-probe after 48 refreshes, then back off to at most 384 refreshes while the
  incumbent stays stable;
- on an Adaptive GPU failure, use CPU for a 96-refresh cooldown and retry;
- reset learning when the scene/device context changes.

GPU timestamp-query intervals for key generation and radix remain valuable
stage diagnostics, but they do not drive the production Adaptive decision. A
sort-only comparison would ignore whether GPU sorting contends with the same
GPU that must project and rasterize the frame, while CPU sorting can overlap
previous queue work. Frame-completion samples are paired by backend, ticket,
and camera revision. The incumbent keeps rendering while a challenger ticket
is pending, so asynchronous readback latency cannot turn one probe into many
challenger frames. Command-submission wall is never mislabeled as completion,
and unrelated warmup, stale receipts, or sort timestamps cannot change the
decision.

Backend selection changes only ordering. It is not permitted to change source
membership, SH degree, encoding precision, render resolution, or draw quality.

## Loading and ownership

The production Packed PLY endpoints first read a validated header summary,
reserve checked final planes, and feed fixed-size decoded splats directly into
`ResidentSceneBuilder`. Native file input uses `File + BufReader`; Web URL,
`File.stream()`, and custom `ReadableStream` input pass transport chunks to the
incremental decoder without first materializing the full response or a second
wide WASM scene. The summary is checked again at completion so a changed or
truncated source cannot publish a scene.

Declared SH is all-or-nothing: rest-property indices must be unique,
contiguous, and exactly match SH1, SH2, or SH3. Unsupported degree, malformed
attributes, non-finite values, count mismatch, checked-size overflow, and
fallible reservation failure are structured errors.

`SceneBuffers` remains the Direct oracle and compatibility owner. The current
SPZ v4 loader decompresses bounded attribute streams into that wide validated
type before an owned conversion to Resident; it does not yet share PLY's
direct-to-resident builder path. This is an honest CPU-peak limitation, not a
reason to lower point count or SH degree. A future SPZ visitor can target the
same builder without changing the GPU representation.

## Fixed quality and verification gate

The Direct-f32 and Resident images use the same checked-in camera, resolution,
source file, complete sorted order, and blend path. The gate is fixed and may
not be weakened per dataset or device:

```text
SSIM (8x8 luma, sRGB)                     >= 0.9999
RGB mean absolute error, normalized       <= 0.00005
fraction of pixels with RGB error > 3/255 <= 0.001
alpha                                     exact
```

Count/SH exactness and image similarity are independent requirements; passing
one never excuses failure of the other. The quality matrix covers SH0-SH3 in
unit/adversarial tests and two fixed views of full Truck, Garden, and Bicycle
for SH3. CPU/GPU ordering additionally compares adversarial keys and complete
real-scene visible source-ID sets.

Resolution is part of the formal contract. Desktop and Web qualification use
true `1920x1080`; the connected Android device uses its native `2412x1080`;
the available iOS simulator uses `2622x1206`. Requested, surface, internal,
and presented dimensions must agree, with dynamic resolution and upscaling
disabled. The checked-in `640x360` traces remain useful historical codec,
phase-cost, and rapid-regression diagnostics only. They are not formal visual
quality, product throughput, or competitor-parity evidence.

Every endpoint artifact must record source/decoded/encoded/resident/addressable
counts, source/resident SH degree, membership policy, adapter limits, requested
and actual backend, fallback state, visible/drawn counts, timing tickets, scene
and trace hashes, and a screenshot. Performance conclusions are valid only for
the final codec and exactness receipt; exploratory runs from an earlier layout
cannot be promoted into the final report.

Artifact acceptance is terminal and fail-closed. A run contributes to an
aggregate only when its experiment and run statuses are complete, required
manifest/summary/frame records are present, hashes and resolution match, and
every scheduled timing ticket has an allowed terminal outcome. Partial
chunked logs, an unfinished run, a missing terminal ledger, or a structured
failure are diagnostic inputs, not performance samples. A corrected rerun uses
a new artifact identity and never overwrites the rejected evidence trail.

Competitor comparison also names a precision/profile contract. Full source
residency and disabled LOD are necessary but not sufficient for an
“equal-quality” label: sort-key width/tie semantics, work-attribute precision,
projected-cache precision, SH update/reuse policy, contribution thresholds,
actual contributor/draw counts, camera, resolution, and terminal timing window
must all be recorded. A faster result under a quantized/fp16/cached-SH profile
remains useful architecture evidence, but it is not divided into this branch's
FPS to produce an equal-quality parity claim.
