# GsplatKit

Local iOS SDK wrapper for the `gsplat-rs` v0.1 C ABI.

This package is a packaging smoke target, not a published binary distribution
yet. Build the local XCFramework before opening the package from Xcode or
SwiftPM:

```bash
bash bindings/apple/scripts/build-xcframework.sh
```

That command writes:

- `bindings/apple/GsplatKit/Binaries/GsplatFFI.xcframework`

The Swift target exposes a small Swift-first API over the C ABI and keeps the
raw `GsplatContext` / `GsplatSurfaceRenderer` pointers private. The stable
native contract is still `crates/gsplat-ffi-c/include/gsplat.h`.

Use the package locally by adding `bindings/apple/GsplatKit` as a Swift package
dependency after the XCFramework exists. Minimal offscreen usage:

```swift
import GsplatKit

let renderer = try GsplatContextRenderer(
    configuration: GsplatRenderConfiguration(width: 800, height: 600)
)
try renderer.loadScene(path: sceneURL.path)
try renderer.setAutoCamera()
try renderer.renderFrame()
let stats = try renderer.stats()
renderer.close()
```

UIKit Surface usage keeps the raw native handle private and serializes calls:

```swift
let renderer = try GsplatUIKitSurfaceRenderer(
    view: surfaceView,
    viewController: viewController,
    datasetPath: sceneURL.path,
    width: UInt32(surfaceView.bounds.width),
    height: UInt32(surfaceView.bounds.height)
)
try renderer.renderFrame()
let exactness = try renderer.exactness()
let presentation = try renderer.presentation()
let orderStatus = try renderer.orderStatus()
let orderSubmission = try renderer.orderSubmission()
let cpuOrderReceipts = try renderer.drainCpuOrderMeasurements()
let orderReceipts = try renderer.drainOrderMeasurements()
let orderFailures = try renderer.drainOrderMeasurementFailures()
let projectedSubmission = try renderer.projectedDrawSubmission()
let projectedTerminals = try renderer.drainProjectedDrawTerminals()
renderer.close()
```

Current S/V/C/D sampling is an additive, non-blocking request/submission/poll
API. Keep one consumer with the renderer, observe the presentation-committed
submission after a successful frame, and consume at most one global poll value
per call:

```swift
var currentStats = GsplatCurrentStatsConsumer()

let admission = try renderer.requestCurrentStats()
try renderer.renderFrame()
let submission = try renderer.currentStatsSubmission()
let pendingEvent = currentStats.observe(submission)
let terminalEvent = currentStats.consume(try renderer.pollCurrentStats())
```

`requested`, `busy`, `gpuUnavailable`, `resourceUnavailable`, and
`ticketExhausted` are admission values, not C-call failures. `notRequested`,
`empty`, and `unsampled` likewise carry no current counts. The consumer emits a
Ready receipt only when its nonzero ticket and complete identity match a
previously observed Issued submission; every identity value other than the
ticket is an opaque join value and may legally be zero. Terminal failures and
identity mismatches end that pending ticket without publishing S/V/C/D.
Because the submission getter is a read-only snapshot of the last successful
frame, observing the same Issued value after its unique terminal returns the
consumer's count-free `settled` event instead of creating a second pending
entry. A same-ticket identity drift also clears the pending entry and poisons
that last snapshot, so a later terminal cannot publish counts. The consumer
retains only live pending correlations plus one last-snapshot lifecycle slot;
it does not build an unbounded history of completed tickets.

The existing public `stats()` API remains unchanged. Its compatibility values
are not a substitute for a current-stats Ready receipt, and this adapter does
not change the iOS example's existing benchmark/artifact success contract.
Before the later Surface semantic cutover, the current Surface implementation
legally reports `gpuUnavailable`, followed by `notRequested` and `empty`.

UIKit defaults to exact resident `.packedAtlas`, `.adaptive` CPU/GPU ordering,
and a sort interval of `1`. Pass `GsplatSurfaceOptions` to force `.cpu`, `.gpu`,
or another explicit diagnostic configuration. GPU receipt collection is
non-blocking: timestamp-query-capable devices populate phase timing, while
completion-only devices leave those optional fields `nil`.
`exactness.isFullQuality` requires source/decoded/encoded/resident/addressable
count equality, source SH degree preservation, and the native no-sampling,
no-LOD, non-partial publication guarantees. Diagnostic `.pagedActiveAtlas`
does not claim full quality.
Every exact non-Paged CPU refresh requests a queue-completion ticket, so forced
CPU/GPU and Adaptive comparisons use frame-start-to-queue-done timing instead
of CPU submit wall time. Every issued CPU/GPU ticket reaches exactly one
success/failure drain
while the live renderer continues to render. `orderStatus.adaptiveGpuFailure`
also makes a pre-ticket Adaptive GPU fallback explicit.
Projected-draw execution is independently `.adaptive` by default. It compares
the full Candidate draw (`D == V`) with exact stable contributor compaction
(`D == C`) on each active CPU/GPU ordering lane without changing source
membership, SH degree, camera, or render resolution. Forced `.candidate` and
`.compact` are diagnostic controls; call `setProjectedDrawPolicy(_:)` to change
them at runtime. Adaptive formal probes expose a submission ticket followed by
exactly one terminal success or failure. A success is returned together with
its same-ticket V/C/D receipt so callers cannot accidentally join counts from a
different camera or projection generation.
For strict evidence, snapshot `orderSubmission()` after every successful
warmup, measured, and terminal-flush frame before draining receipts; reject
ring-busy, Surface-unavailable, dropped-prior, terminal failure, fallback, or
incomplete tickets.
Apply the same rule to `projectedDrawSubmission()`: retain every issued ticket,
drain both projected terminal queues, and reject unsampled, dropped, failed, or
missing V/C/D evidence.
`drainProjectedDrawTerminals()` holds the renderer ownership lock across both
terminal queues and takes each success's V/C/D receipt before another native
poll. All V1 calls initialize and verify `struct_size/version`; projected
tickets are also rejected unless they remain in the independent, JavaScript-safe
`[2^52, 2^53 - 1]` namespace. Forced Candidate/Compact receipts must report the
same actual execution, disabled Adaptive state, and no fabricated ticket.
`presentation.fullResolution` is true only after an actual presentation whose
requested, Surface, internal-render, and presented pixel dimensions all match;
the native Surface path does not use dynamic resolution or upscaling.

Current limits:

- local binary package only; no remote SwiftPM release artifact
- iOS 17+ in this validation slice
- scene loading is still file-path based
- `SortedAlpha` is the only release-gated render path
- simulator slice builds `aarch64-apple-ios-sim x86_64-apple-ios` by default
  unless `IOS_XCFRAMEWORK_SIM_TARGETS` is overridden
